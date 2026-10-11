use crate::config::{self, Profile};
use anyhow::{Result, anyhow, bail, ensure};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use regex::Regex;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// encodeURIComponent's unescaped ASCII set, not application/x-www-form-urlencoded.
const COMPONENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}')
    .add(0x7f);

#[derive(Clone, Serialize)]
pub struct Entry {
    #[serde(flatten)]
    pub profile: Profile,
    pub line: usize,
}

pub fn parse_proxy_list(text: &str) -> Result<Vec<Entry>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let pattern = RE.get_or_init(|| {
        Regex::new(r"(?i)^([a-z0-9]+)://(\[[^\]]+\]|[^:\s/]+):([0-9]+):([^:]*):([^:]*):([^:]+)$")
            .expect("constant named proxy pattern")
    });
    let mut entries = Vec::new();
    for (i, raw) in text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .split('\n')
        .enumerate()
    {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let parsed = (|| -> Result<Profile> {
            let m = pattern
                .captures(line)
                .ok_or_else(|| anyhow!("Invalid row."))?;
            let auth = if m[4].is_empty() && m[5].is_empty() {
                String::new()
            } else {
                format!(
                    "{}:{}@",
                    utf8_percent_encode(&m[4], COMPONENT),
                    utf8_percent_encode(&m[5], COMPONENT)
                )
            };
            let profile = Profile::new(
                m[6].trim().to_owned(),
                format!(
                    "{}://{}{}:{}",
                    m[1].to_ascii_lowercase(),
                    auth,
                    &m[2],
                    &m[3]
                ),
            );
            let mut config = config::initial();
            config.profiles.push(profile.clone());
            config::validate(&config)?;
            Ok(profile)
        })();
        let profile = parsed.map_err(|_| {
            anyhow!(
                "Line {}: invalid proxy format, name or credentials; nothing imported.",
                i + 1
            )
        })?;
        entries.push(Entry {
            profile,
            line: i + 1,
        });
        ensure!(entries.len() <= 10000, "Proxy list exceeds 10000 entries.");
    }
    ensure!(!entries.is_empty(), "Proxy list contains no entries.");
    Ok(entries)
}

fn unquote_path(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_owned();
    };
    let text = text.trim();
    let text = if text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\'')))
    {
        &text[1..text.len() - 1]
    } else {
        text
    };
    PathBuf::from(text)
}

pub fn import(root: &Path, file: &Path) -> Result<Value> {
    let read = || -> Result<String> {
        let path = unquote_path(file);
        ensure!(
            std::fs::metadata(&path)?.is_file(),
            "Proxy input must be a regular file."
        );
        let mut file = File::open(path)?;
        ensure!(
            file.metadata()?.is_file() && file.metadata()?.len() <= 1024 * 1024,
            "Too large."
        );
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 1024 * 1024, "Too large.");
        Ok(String::from_utf8(bytes)?)
    };
    let text = read()
        .map_err(|_| anyhow!("Cannot read proxy list. Use a UTF-8 text file of at most 1 MiB."))?;
    let _lock = config::lock(root)?;
    let mut config = config::load(root)?;
    let entries = parse_proxy_list(&text)?;
    let mut names: HashMap<String, usize> = config
        .profiles
        .iter()
        .enumerate()
        .map(|(i, profile)| (profile.name.to_ascii_lowercase(), i))
        .collect();
    let (mut added, mut skipped) = (0, 0);
    for entry in entries {
        let key = entry.profile.name.to_ascii_lowercase();
        if let Some(previous) = names.get(&key) {
            if config::validate_proxy(&config.profiles[*previous].url)?.as_str()
                != config::validate_proxy(&entry.profile.url)?.as_str()
            {
                bail!(
                    "Line {}: profile name conflicts with an existing entry; nothing imported.",
                    entry.line
                );
            }
            skipped += 1;
        } else {
            names.insert(key, config.profiles.len());
            config.profiles.push(entry.profile);
            added += 1;
        }
    }
    config::validate(&config)?;
    if added != 0 {
        config::save_unlocked(root, &config)?;
    }
    Ok(json!({ "added": added, "skipped": skipped }))
}
