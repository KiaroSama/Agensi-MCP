//! Disposable-hosted-runner native fixture state; the outer owner also reverts crash residue.
use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};
use tokio::{
    process::Command,
    time::{Duration, timeout},
};

pub async fn validate() -> Result<()> {
    ensure!(
        std::env::var("AGENSI_TLS_CI").as_deref() == Ok("1"),
        "TLS proxy controls require the disposable CI owner"
    );
    let addresses = timeout(
        Duration::from_secs(3),
        tokio::net::lookup_host(("mcp.agensi.io", 443)),
    )
    .await??
    .collect::<Vec<_>>();
    ensure!(
        !addresses.is_empty()
            && addresses
                .iter()
                .all(|address| address.ip() == std::net::Ipv4Addr::LOCALHOST),
        "TLS observer hostname is not isolated to IPv4 loopback"
    );
    invoke("Validate", None, None, None).await
}

pub struct Lease {
    root: PathBuf,
    binary: PathBuf,
}
impl Lease {
    pub fn new(root: &Path, binary: &Path) -> Self {
        Self {
            root: root.to_owned(),
            binary: binary.to_owned(),
        }
    }
    pub async fn trust(&self, roots: &[&Path]) -> Result<()> {
        // Removing the prior case roots before importing is essential to negative controls.
        self.close().await?;
        invoke("Protect", Some(&self.root), Some(&self.binary), None).await?;
        for root in roots {
            invoke("Trust", Some(&self.root), None, Some(root)).await?;
        }
        Ok(())
    }
    pub async fn close(&self) -> Result<()> {
        invoke("RemoveCase", Some(&self.root), None, None).await
    }
}

async fn invoke(
    action: &str,
    root: Option<&Path>,
    binary: Option<&Path>,
    certificate: Option<&Path>,
) -> Result<()> {
    let owner = PathBuf::from(
        std::env::var_os("AGENSI_TLS_CI_SCRIPT")
            .ok_or_else(|| anyhow::anyhow!("Missing CI fixture owner"))?,
    );
    let shell = PathBuf::from(
        std::env::var_os("AGENSI_TLS_CI_PWSH")
            .ok_or_else(|| anyhow::anyhow!("Missing CI fixture shell"))?,
    );
    ensure!(
        owner.is_absolute() && shell.is_absolute() && owner.is_file() && shell.is_file(),
        "Invalid CI fixture owner paths"
    );
    eprintln!("Native CI fixture action={action} started");
    let mut command = Command::new(shell);
    command
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
        .arg(owner)
        .arg("-Action")
        .arg(action);
    if let Some(root) = root {
        command.arg("-CaseRoot").arg(root);
    }
    if let Some(binary) = binary {
        command.arg("-Binary").arg(binary);
    }
    if let Some(certificate) = certificate {
        command.arg("-Certificate").arg(certificate);
    }
    command
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit());
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn()?;
    let result = timeout(Duration::from_secs(12), child.wait()).await;
    match result {
        Ok(result) => ensure!(
            result?.success(),
            "Native CI fixture action {action} failed"
        ),
        Err(_) => {
            child.kill().await?;
            child.wait().await?;
            anyhow::bail!("Native CI fixture action {action} deadline");
        }
    }
    eprintln!("Native CI fixture action={action} completed");
    Ok(())
}
