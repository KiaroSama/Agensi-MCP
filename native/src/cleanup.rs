use crate::config;
use anyhow::{Result, anyhow, ensure};
use fs2::FileExt;
use serde_json::{Value, json};
use std::fs::{self, File, Metadata, OpenOptions};
use std::path::{Component, Path, PathBuf};

const MAX_ENTRIES: usize = 100_000;
const REMOVABLE: &[&str] = &["node_modules", ".ai/ARCHIVE/pre-ci-binary"];
const DEFERRED: &[(&str, &str)] = &[
    (
        ".cache",
        "Build caches require build-script cleanup; lock not coordinated.",
    ),
    (
        "target",
        "Build caches require build-script cleanup; lock not coordinated.",
    ),
    (
        "dist",
        "Distribution artifacts are preserved; ownership is not established.",
    ),
    (
        ".tmp",
        "Shared temporary root is preserved; operation ownership is not established.",
    ),
    (
        "plugins",
        "Plugin files are preserved; not automatically disposable.",
    ),
];

fn validate_root(root: &Path) -> Result<()> {
    ensure!(
        root.is_absolute()
            && !root
                .components()
                .any(|part| matches!(part, Component::ParentDir)),
        "Cleanup requires an absolute project root without parent traversal."
    );
    config::reject_linked_ancestors(root)?;
    ensure!(
        fs::symlink_metadata(root)?.is_dir(),
        "Project root is not a directory."
    );
    for marker in ["Cargo.toml", "run.ps1"] {
        let path = root.join(marker);
        config::reject_linked_ancestors(&path)?;
        ensure!(
            fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()),
            "Cleanup root does not contain the project markers."
        );
    }
    Ok(())
}

fn linked(metadata: &Metadata) -> bool {
    let linked = metadata.file_type().is_symlink();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        linked || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    linked
}

fn open_lock(root: &Path, create: bool) -> Result<Option<File>> {
    let path = root.join(".runtime.lock");
    config::reject_linked_ancestors(&path)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) => ensure!(metadata.is_file(), "Runtime lock is not a regular file."),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(anyhow!("Cannot inspect runtime lock.")),
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(create)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(&path)
        .map_err(|_| anyhow!("Cannot open runtime lock."))?;
    config::reject_linked_ancestors(&path)?;
    Ok(Some(file))
}

/// Hold this file for the entire runtime operation; dropping it releases the lease.
pub fn runtime_guard(root: &Path) -> Result<File> {
    validate_root(root)?;
    let file = open_lock(root, true)?.ok_or_else(|| anyhow!("Cannot open runtime lock."))?;
    FileExt::try_lock_shared(&file)
        .map_err(|_| anyhow!("Cleanup is busy; retry after cleanup finishes."))?;
    Ok(file)
}

struct Entry {
    path: PathBuf,
    directory: bool,
    bytes: u64,
}

fn protected(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        ".git"
            | ".ai"
            | ".state"
            | ".auth"
            | "auth"
            | "state"
            | "config"
            | "logs"
            | "skills"
            | ".ignoreme"
            | "secrets.md"
            | "config.local.json"
            | ".runtime.lock"
            | ".management.lock"
    ) || name == ".env"
        || name.starts_with(".env.")
}

// Preflight is complete before the first removal; no contents or descendant names are reported.
fn inspect(path: &Path) -> Result<Vec<Entry>> {
    config::reject_linked_ancestors(path)?;
    let mut pending = vec![path.to_path_buf()];
    let mut entries = Vec::new();
    while let Some(path) = pending.pop() {
        ensure!(
            entries.len() < MAX_ENTRIES,
            "Candidate exceeds 100000-entry safety limit."
        );
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| anyhow!("Cannot inspect candidate; nothing removed."))?;
        ensure!(
            !linked(&metadata),
            "Linked candidate or descendant refused; nothing removed."
        );
        ensure!(
            !protected(&path),
            "Protected data in candidate; nothing removed."
        );
        ensure!(
            metadata.is_dir() || metadata.is_file(),
            "Special file refused; nothing removed."
        );
        if metadata.is_dir() {
            for child in fs::read_dir(&path)
                .map_err(|_| anyhow!("Cannot inspect candidate directory; nothing removed."))?
            {
                let child = child
                    .map_err(|_| anyhow!("Cannot inspect candidate directory; nothing removed."))?;
                ensure!(
                    entries.len() + pending.len() < MAX_ENTRIES,
                    "Candidate exceeds 100000-entry safety limit."
                );
                pending.push(child.path());
            }
        }
        entries.push(Entry {
            path,
            directory: metadata.is_dir(),
            bytes: if metadata.is_file() {
                metadata.len()
            } else {
                0
            },
        });
    }
    Ok(entries)
}

fn present(root: &Path, name: &str) -> Result<bool> {
    let path = root.join(name);
    config::reject_linked_ancestors(&path)?;
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(anyhow!("Cannot inspect candidate.")),
    }
}

/// Dry-run by default at the CLI. Only explicit apply removes exact legacy allowlisted trees.
pub fn run(root: &Path, apply: bool) -> Result<Value> {
    validate_root(root)?;
    // Reject a linked/invalid lock before any inspection or write, even when no candidates exist.
    let mut lease = open_lock(root, false)?;
    if let Some(file) = &lease {
        FileExt::try_lock_exclusive(file).map_err(|_| {
            anyhow!("Runtime is active; cleanup refused without deleting anything.")
        })?;
    }
    let mut candidates = Vec::new();
    let mut plans = Vec::new();
    let mut skipped = 0usize;
    let mut bytes = 0u64;
    for name in REMOVABLE {
        match present(root, name).and_then(|exists| {
            if exists {
                inspect(&root.join(name)).map(Some)
            } else {
                Ok(None)
            }
        }) {
            Ok(Some(entries)) => {
                let size = entries
                    .iter()
                    .try_fold(0u64, |total, entry| total.checked_add(entry.bytes))
                    .ok_or_else(|| anyhow!("Candidate size exceeds safety limit."))?;
                bytes = bytes
                    .checked_add(size)
                    .ok_or_else(|| anyhow!("Cleanup size exceeds safety limit."))?;
                candidates.push(json!({"name": name, "bytes": size, "status": "candidate"}));
                plans.push(candidates.len() - 1);
            }
            Ok(None) => {}
            Err(error) => {
                skipped += 1;
                candidates.push(json!({"name": name, "bytes": null, "status": "skipped", "reason": error.to_string()}));
            }
        }
    }
    for (name, reason) in DEFERRED {
        match present(root, name) {
            Ok(false) => {}
            result => {
                skipped += 1;
                candidates.push(json!({"name": name, "bytes": null, "status": "skipped",
                    "reason": result.err().map(|error| error.to_string()).unwrap_or_else(|| (*reason).into())}));
            }
        }
    }
    let mut deleted = 0usize;
    let mut deleted_bytes = 0u64;
    let mut partial = false;
    if apply && !plans.is_empty() {
        if lease.is_none() {
            lease = open_lock(root, true)?;
            FileExt::try_lock_exclusive(
                lease
                    .as_ref()
                    .ok_or_else(|| anyhow!("Cannot open runtime lock."))?,
            )
            .map_err(|_| {
                anyhow!("Runtime is active; cleanup refused without deleting anything.")
            })?;
        }
        for index in plans {
            let name = candidates[index]["name"]
                .as_str()
                .expect("constant candidate name")
                .to_owned();
            // The lock may have been created after the first scan. Re-preflight under the lease.
            let entries = match inspect(&root.join(&name)) {
                Ok(entries) => entries,
                Err(error) => {
                    candidates[index]["status"] = json!("skipped");
                    candidates[index]["reason"] = json!(error.to_string());
                    skipped += 1;
                    continue;
                }
            };
            let mut removed_entries = 0usize;
            let mut removed_bytes = 0u64;
            let mut failure = None;
            for entry in entries.iter().rev() {
                if config::reject_linked_ancestors(&entry.path).is_err() {
                    failure =
                        Some("Candidate changed to a linked path; removal stopped.".to_owned());
                    break;
                }
                let result = fs::symlink_metadata(&entry.path).and_then(|metadata| {
                    if linked(&metadata)
                        || metadata.is_dir() != entry.directory
                        || (!entry.directory
                            && (!metadata.is_file() || metadata.len() != entry.bytes))
                        || protected(&entry.path)
                    {
                        return Err(std::io::Error::other("Candidate changed during cleanup."));
                    }
                    if entry.directory {
                        fs::remove_dir(&entry.path)
                    } else {
                        fs::remove_file(&entry.path)
                    }
                });
                if let Err(error) = result {
                    failure = Some(format!(
                        "Removal stopped: {} (OS code {:?}).",
                        error.kind(),
                        error.raw_os_error()
                    ));
                    break;
                }
                removed_entries += 1;
                removed_bytes += entry.bytes;
            }
            deleted_bytes += removed_bytes;
            candidates[index]["deleted_bytes"] = json!(removed_bytes);
            candidates[index]["removed_entries"] = json!(removed_entries);
            if let Some(error) = failure {
                partial |= removed_entries != 0;
                skipped += 1;
                candidates[index]["status"] = json!(if removed_entries == 0 {
                    "skipped"
                } else {
                    "partial"
                });
                candidates[index]["reason"] = json!(error);
            } else {
                deleted += 1;
                candidates[index]["status"] = json!("deleted");
            }
        }
    }
    Ok(
        json!({"dry_run": !apply, "candidates": candidates, "bytes": bytes,
        "deleted": deleted, "deleted_bytes": deleted_bytes, "skipped": skipped,
        "status": if partial || (deleted > 0 && skipped > 0) { "partial" } else if skipped > 0 { "skipped" } else if apply { "complete" } else { "dry_run" }}),
    )
}
