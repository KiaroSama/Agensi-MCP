pub mod auth;
pub mod bridge;
pub mod config;
pub mod credentials;
pub mod logging;
pub mod management;
pub mod network;
pub mod proxy_import;
pub mod skill_files;
pub mod storage;

pub fn project_root() -> anyhow::Result<std::path::PathBuf> {
    let executable = std::env::current_exe()?;
    for parent in executable.ancestors().skip(1) {
        if parent.join("Cargo.toml").is_file() && parent.join("run.ps1").is_file() {
            return Ok(parent.to_path_buf());
        }
    }
    Err(anyhow::anyhow!(
        "Cannot resolve project root beside native executable."
    ))
}

pub mod local_http;
pub mod settings;
pub mod socks4_userid;

pub mod cleanup;
pub mod device_privacy;
pub mod diagnostics;
