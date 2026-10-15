use anyhow::{Result, anyhow, bail, ensure};
use fs2::FileExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Disconnected,
    Direct,
    Proxy,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub url: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl std::fmt::Debug for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Profile")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Profile {
    pub fn new(name: String, url: String) -> Self {
        Self {
            name,
            url,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub schema: u32,
    pub mode: Mode,
    pub selected: Option<String>,
    pub profiles: Vec<Profile>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("schema", &self.schema)
            .field("mode", &self.mode)
            .field("selected", &self.selected)
            .field("profiles", &self.profiles)
            .finish_non_exhaustive()
    }
}

pub fn initial() -> Config {
    Config {
        schema: 1,
        mode: Mode::Disconnected,
        selected: None,
        profiles: Vec::new(),
        extra: BTreeMap::new(),
    }
}

fn port_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^[A-Za-z0-9_]+://(?:[^/@]*@)?(?:\[[^\]]+\]|[^/:]+):([0-9]+)(?:/)?$")
            .expect("constant proxy port pattern")
    })
}

pub fn validate_proxy(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| anyhow!("Invalid proxy URL."))?;
    let port = port_pattern()
        .captures(value)
        .and_then(|m| m.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok());
    ensure!(
        matches!(
            url.scheme(),
            "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h"
        ) && url.host_str().is_some_and(|host| !host.is_empty())
            && port.is_some_and(|port| (1..=65535).contains(&port))
            && matches!(url.path(), "" | "/")
            && url.query().is_none()
            && url.fragment().is_none(),
        "Proxy needs a supported scheme, host and explicit port; no path or query."
    );
    for credential in [url.username(), url.password().unwrap_or("")] {
        let bytes = credential.as_bytes();
        for (i, byte) in bytes.iter().enumerate() {
            ensure!(
                *byte != b'%'
                    || (bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                        && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit)),
                "Invalid percent-encoded proxy credentials."
            );
        }
        percent_encoding::percent_decode_str(credential)
            .decode_utf8()
            .map_err(|_| anyhow!("Invalid percent-encoded proxy credentials."))?;
    }
    ensure!(
        !url.scheme().starts_with("socks4") || url.password().is_none_or(str::is_empty),
        "SOCKS4 supports a user ID, not password authentication."
    );
    Ok(url)
}

pub fn validate(config: &Config) -> Result<()> {
    ensure!(
        config.schema == 1 && config.profiles.len() <= 100,
        "Invalid route settings; connection blocked."
    );
    ensure!(
        !["schema", "mode", "selected", "profiles"]
            .iter()
            .any(|k| config.extra.contains_key(*k)),
        "Invalid route settings; connection blocked."
    );
    let mut names = HashSet::new();
    for profile in &config.profiles {
        ensure!(
            (1..=80).contains(&profile.name.len())
                && profile.name == profile.name.trim()
                && profile
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_ .-".contains(&b))
                && names.insert(profile.name.to_ascii_lowercase()),
            "Proxy names must be unique, 1-80 plain characters."
        );
        ensure!(
            !profile.extra.contains_key("name") && !profile.extra.contains_key("url"),
            "Invalid route settings; connection blocked."
        );
        validate_proxy(&profile.url)?;
    }
    ensure!(
        config.mode != Mode::Proxy
            || config
                .profiles
                .iter()
                .any(|p| Some(&p.name) == config.selected.as_ref()),
        "Selected proxy is missing; connection blocked."
    );
    Ok(())
}

/// Load schema 1 settings without logging or discarding unknown JSON fields.
pub fn load(root: &Path) -> Result<Config> {
    let path = root.join("config.local.json");
    let Some(bytes) = crate::storage::read_optional(&path)? else {
        return Ok(initial());
    };
    from_bytes(&bytes)
}

/// Validate settings from one captured read, so route and change detection share an identity.
pub fn from_bytes(bytes: &[u8]) -> Result<Config> {
    ensure!(
        bytes.len() <= crate::storage::MAX_JSON_BYTES,
        "Route settings exceed 1 MiB limit."
    );
    let text = std::str::from_utf8(bytes)
        .map_err(|_| anyhow!("Route settings are not valid JSON; connection blocked."))?;
    let mut value: Value = serde_json::from_str(text)
        .map_err(|_| anyhow!("Route settings are not valid JSON; connection blocked."))?;
    ensure!(
        value.is_object()
            && value["schema"].as_f64() == Some(1.0)
            && matches!(
                value["mode"].as_str(),
                Some("disconnected" | "direct" | "proxy")
            )
            && value
                .get("selected")
                .is_some_and(|v| v.is_null() || v.is_string())
            && value["profiles"].as_array().is_some_and(|v| v.len() <= 100),
        "Invalid route settings; connection blocked."
    );
    if let Some(profiles) = value["profiles"].as_array() {
        for profile in profiles {
            ensure!(
                profile.is_object() && profile["name"].is_string(),
                "Proxy names must be unique, 1-80 plain characters."
            );
            ensure!(profile["url"].is_string(), "Invalid proxy URL.");
        }
    }
    value["schema"] = json!(1);
    let config: Config = serde_json::from_value(value)
        .map_err(|_| anyhow!("Invalid route settings; connection blocked."))?;
    validate(&config)?;
    Ok(config)
}

pub(crate) fn reject_linked_ancestors(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                let linked = metadata.file_type().is_symlink();
                #[cfg(windows)]
                let linked = {
                    use std::os::windows::fs::MetadataExt;
                    linked || metadata.file_attributes() & 0x400 != 0
                };
                ensure!(!linked, "Linked directory or settings file refused.");
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => bail!("Cannot inspect local settings directory."),
        }
    }
    Ok(())
}

// Lock the entire management transaction, not just the final replacement. Refuse busy
// operations rather than waiting indefinitely or overwriting a concurrent profile addition.
pub(crate) fn lock(root: &Path) -> Result<File> {
    reject_linked_ancestors(root)?;
    fs::create_dir_all(root).map_err(|_| anyhow!("Cannot create local settings directory."))?;
    let path = root.join(".management.lock");
    crate::storage::check_file(&path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .map_err(|_| anyhow!("Cannot open management lock."))?;
    FileExt::try_lock_exclusive(&file)
        .map_err(|_| anyhow!("Management is busy; retry when the current operation finishes."))?;
    Ok(file)
}

pub fn save(root: &Path, config: &Config) -> Result<()> {
    validate(config)?;
    let _lock = lock(root)?;
    save_unlocked(root, config)
}

pub(crate) fn save_unlocked(root: &Path, config: &Config) -> Result<()> {
    validate(config)?;
    // A malformed existing file is evidence, not an invitation to reset user data.
    load(root)?;
    let path = root.join("config.local.json");
    let mut temporary = tempfile::Builder::new()
        .prefix(".config-")
        .suffix(".tmp")
        .tempfile_in(root)
        .map_err(|_| anyhow!("Cannot create temporary route settings."))?;
    let mut bytes = serde_json::to_vec_pretty(config)
        .map_err(|_| anyhow!("Cannot serialize route settings."))?;
    bytes.push(b'\n');
    ensure!(
        bytes.len() <= crate::storage::MAX_JSON_BYTES,
        "Route settings exceed 1 MiB limit."
    );
    temporary
        .write_all(&bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|_| anyhow!("Cannot write route settings; existing settings preserved."))?;
    reject_linked_ancestors(&path)?;
    // tempfile uses atomic rename / Windows MoveFileExW(REPLACE_EXISTING). Never unlink
    // the old file first: a failed replacement must leave its contents intact.
    temporary
        .persist(&path)
        .map_err(|_| anyhow!("Cannot replace route settings; existing settings preserved."))?;
    #[cfg(unix)]
    File::open(root)
        .and_then(|dir| dir.sync_all())
        .map_err(|_| {
            anyhow!("Route settings saved, but directory durability confirmation failed.")
        })?;
    Ok(())
}

pub fn public_status(config: &Config) -> Value {
    let profiles: Vec<Value> = config
        .profiles
        .iter()
        .map(|p| {
            match validate_proxy(&p.url) {
                Ok(url) => json!({ "name": p.name, "protocol": url.scheme(),
                "host": url.host_str().unwrap_or(""),
                "port": url.port().unwrap_or(if url.scheme() == "https" { 443 } else { 80 }),
                "authenticated": !url.username().is_empty() || url.password().is_some() }),
                // Callers load/validate first; an invalid in-memory profile never exposes its URL.
                Err(_) => json!({ "name": p.name, "invalid": true }),
            }
        })
        .collect();
    json!({ "mode": config.mode, "selected": config.selected, "profiles": profiles })
}
