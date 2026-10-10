use crate::config::{self, Mode, Profile};
use crate::proxy_import;
use crate::skill_files::{InlineFile, save_skill};
use anyhow::{Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub fn command(root: &Path, args: &[String], stdin: &str) -> Result<Value> {
    let action = args.first().map(String::as_str).unwrap_or("");
    let name = args.get(1).map(String::as_str).unwrap_or("");
    let value = args.get(2).map(String::as_str).unwrap_or("");
    if action == "import" {
        let file = if name.is_empty() {
            root.join("Proxy list.txt")
        } else {
            Path::new(name).to_owned()
        };
        return proxy_import::import(root, &file);
    }
    if action == "save-skill" {
        // The source checks settings even for inline installation; preserve failclosed behavior.
        config::load(root)?;
        let read = || -> Result<Vec<InlineFile>> {
            let mut file = File::open(value)?;
            ensure!(
                file.metadata()?.is_file() && file.metadata()?.len() <= 128 * 1024 * 1024,
                "Inline JSON exceeds safety limit."
            );
            let mut bytes = Vec::new();
            Read::by_ref(&mut file)
                .take(128 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 128 * 1024 * 1024,
                "Inline JSON exceeds safety limit."
            );
            let data: Value = serde_json::from_slice(&bytes)?;
            Ok(serde_json::from_value(if data.is_array() {
                data
            } else {
                data["files"].clone()
            })?)
        };
        let files = read().map_err(|_| anyhow!("Cannot read a valid inline skill JSON file."))?;
        return Ok(json!({ "saved": save_skill(root, name, &files)? }));
    }
    if action == "status" {
        return Ok(config::public_status(&config::load(root)?));
    }
    let _lock = config::lock(root)?;
    let mut config = config::load(root)?;
    match action {
        "add" => {
            ensure!(
                stdin.len() <= 8192 || value != "--stdin",
                "Proxy input too long."
            );
            let url = if value == "--stdin" { stdin } else { value }.trim();
            config::validate_proxy(url)?;
            ensure!(
                !config
                    .profiles
                    .iter()
                    .any(|p| p.name.eq_ignore_ascii_case(name)),
                "That proxy name already exists."
            );
            config
                .profiles
                .push(Profile::new(name.to_owned(), url.to_owned()));
        }
        "select" => {
            ensure!(
                config.profiles.iter().any(|p| p.name == name),
                "Unknown proxy profile."
            );
            config.mode = Mode::Proxy;
            config.selected = Some(name.to_owned());
        }
        "direct" => config.mode = Mode::Direct,
        "disconnect" => config.mode = Mode::Disconnected,
        _ => bail!("Use status, add, import, select, direct, disconnect or save-skill."),
    }
    config::save_unlocked(root, &config)?;
    Ok(config::public_status(&config))
}
