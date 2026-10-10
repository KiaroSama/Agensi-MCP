use crate::config;
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub struct InlineFile {
    pub path: String,
    pub content: String,
}

fn safe_component(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
    let device = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
        });
    !part.is_empty()
        && !matches!(part, "." | "..")
        && !part.ends_with(['.', ' '])
        && !part.chars().any(|c| c < '\u{20}' || "<>:\"|?*".contains(c))
        && !device
}

fn ensure_new_target(skills: &Path, name: &str) -> Result<()> {
    if skills.exists() {
        let entries =
            fs::read_dir(skills).map_err(|_| anyhow!("Cannot inspect skill directory."))?;
        for entry in entries {
            let entry = entry.map_err(|_| anyhow!("Cannot inspect skill directory."))?;
            ensure!(
                !entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(name),
                "Skill folder already exists; nothing overwritten."
            );
        }
    }
    Ok(())
}

pub fn save_skill(root: &Path, name: &str, files: &[InlineFile]) -> Result<PathBuf> {
    ensure!(
        (1..=80).contains(&name.len())
            && name.as_bytes()[0].is_ascii_alphanumeric()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            && safe_component(name)
            && !files.is_empty()
            && files.len() <= 500,
        "Invalid skill folder name or file list."
    );
    let skills = root.join("skills");
    config::reject_linked_ancestors(&skills)
        .map_err(|_| anyhow!("Linked or inaccessible skill directory refused."))?;
    let _lock = config::lock(root)?;
    ensure_new_target(&skills, name)?;
    let mut names = HashSet::new();
    let mut prefixes = HashMap::<String, String>::new();
    let mut paths = Vec::with_capacity(files.len());
    let mut bytes = 0usize;
    for entry in files {
        let parts: Vec<&str> = entry.path.split(['/', '\\']).collect();
        ensure!(
            parts.iter().all(|part| safe_component(part)),
            "Unsafe skill file path refused."
        );
        let key = parts.join("/").to_lowercase();
        ensure!(
            names.insert(key.clone()),
            "Duplicate skill file path refused."
        );
        bytes = bytes
            .checked_add(entry.content.len())
            .ok_or_else(|| anyhow!("Skill text exceeds 20 MiB safety limit."))?;
        ensure!(
            bytes <= 20 * 1024 * 1024,
            "Skill text exceeds 20 MiB safety limit."
        );
        // Windows directory casing is insensitive. Reuse the first spelling so the
        // same package has one directory tree when installed on Linux as well.
        let mut path = PathBuf::new();
        let mut prefix = String::new();
        for part in parts {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(&part.to_lowercase());
            let spelling = prefixes
                .entry(prefix.clone())
                .or_insert_with(|| part.to_owned());
            path.push(spelling.as_str());
        }
        paths.push(path);
    }
    ensure!(
        names.contains("skill.md"),
        "Skill package must include SKILL.md."
    );
    for name in &names {
        for (offset, _) in name.match_indices('/') {
            ensure!(
                !names.contains(&name[..offset]),
                "Skill file and directory paths conflict; nothing installed."
            );
        }
    }
    fs::create_dir_all(&skills).map_err(|_| anyhow!("Cannot create skill directory."))?;
    config::reject_linked_ancestors(&skills)?;
    let staging = tempfile::Builder::new()
        .prefix(".install-")
        .tempdir_in(&skills)
        .map_err(|_| anyhow!("Cannot create temporary skill directory."))?;
    let installed = (|| -> Result<PathBuf> {
        for (entry, relative) in files.iter().zip(&paths) {
            let path = staging.path().join(relative);
            let parent = path
                .parent()
                .ok_or_else(|| anyhow!("Unsafe skill file path refused."))?;
            fs::create_dir_all(parent)
                .map_err(|_| anyhow!("Cannot create skill file directory."))?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(path)
                .map_err(|_| anyhow!("Cannot create skill file; nothing installed."))?;
            file.write_all(entry.content.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|_| anyhow!("Cannot write skill text; nothing installed."))?;
        }
        #[cfg(unix)]
        {
            let directories: HashSet<PathBuf> = paths
                .iter()
                .flat_map(|path| {
                    path.parent()
                        .into_iter()
                        .flat_map(|parent| parent.ancestors())
                        .map(|parent| staging.path().join(parent))
                })
                .collect();
            for directory in directories {
                fs::File::open(directory)
                    .and_then(|directory| directory.sync_all())
                    .map_err(|_| {
                        anyhow!("Cannot synchronize skill directory; nothing installed.")
                    })?;
            }
        }
        config::reject_linked_ancestors(&skills)?;
        ensure_new_target(&skills, name)?;
        let target = skills.join(name);
        fs::rename(staging.path(), &target)
            .map_err(|_| anyhow!("Cannot install skill folder; nothing overwritten."))?;
        #[cfg(unix)]
        fs::File::open(&skills)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| {
                anyhow!("Skill installed, but directory durability confirmation failed.")
            })?;
        Ok(target)
    })();
    if installed.is_err() && staging.path().exists() {
        staging
            .close()
            .map_err(|_| anyhow!("Cannot clean temporary skill directory."))?;
    }
    // After successful rename the staging path no longer exists; TempDir's drop
    // tolerates that. Explicit close would report NotFound despite a successful install.
    installed
}
