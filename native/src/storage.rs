//! Bounded cooperative file access. Noncooperating filesystem swaps are not supported.
use anyhow::{Result, anyhow, ensure};
use std::{fs, io::Read, path::Path};

pub const MAX_JSON_BYTES: usize = 1024 * 1024;

/// Refuse links/reparse points and nonregular existing files before opening them.
pub fn check_file(path: &Path) -> Result<()> {
    crate::config::reject_linked_ancestors(path)?;
    match fs::symlink_metadata(path) {
        Ok(meta) => ensure!(meta.is_file(), "Nonregular local state file refused."),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(anyhow!("Cannot inspect local state file.")),
    }
    Ok(())
}

pub fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    check_file(path)?;
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(anyhow!("Cannot inspect local state file.")),
    };
    ensure!(
        meta.len() <= MAX_JSON_BYTES as u64,
        "Local state exceeds 1 MiB limit."
    );
    let file = fs::File::open(path).map_err(|_| anyhow!("Cannot open local state file."))?;
    let meta = file
        .metadata()
        .map_err(|_| anyhow!("Cannot inspect opened state file."))?;
    ensure!(
        meta.is_file() && meta.len() <= MAX_JSON_BYTES as u64,
        "Local state file refused."
    );
    let mut bytes = Vec::new();
    file.take(MAX_JSON_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow!("Cannot read local state file."))?;
    ensure!(
        bytes.len() <= MAX_JSON_BYTES,
        "Local state exceeds 1 MiB limit."
    );
    Ok(Some(bytes))
}

pub fn snapshot(root: &Path) -> Result<Vec<u8>> {
    read_optional(&root.join("config.local.json"))?
        .ok_or_else(|| anyhow!("Cannot read selected route."))
}

pub fn matches(path: &Path, expected: &[u8]) -> bool {
    matches!(read_optional(path), Ok(Some(bytes)) if bytes == expected)
}
