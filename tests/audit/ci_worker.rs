//! One private Windows fixture session; no action starts a second PowerShell runtime.
use anyhow::{Result, bail, ensure};
use serde_json::json;
use std::{path::Path, process::Stdio};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
    time::{Duration, timeout},
};

struct Owner {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    next: u32,
}
struct State {
    owner: Option<Owner>,
    invalid: bool,
}
static STATE: Mutex<State> = Mutex::const_new(State {
    owner: None,
    invalid: false,
});

fn response(line: &[u8], id: Option<u32>) -> Result<()> {
    let expected = id.map_or_else(|| "READY\n".to_owned(), |id| format!("{id}:OK\n"));
    ensure!(
        line == expected.as_bytes(),
        "Native fixture worker response rejected"
    );
    Ok(())
}
fn contracts() -> Result<()> {
    response(b"READY\n", None)?;
    response(b"1:OK\n", Some(1))?;
    for line in [
        b"".as_slice(),
        b"READY",
        b"2:OK\n",
        b"1:ERR\n",
        b"1:OK\nextra",
    ] {
        ensure!(
            response(line, Some(1)).is_err(),
            "Worker response negative failed"
        );
    }
    Ok(())
}
async fn line(output: &mut ChildStdout) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(32);
    while bytes.len() < 32 {
        let byte = output.read_u8().await?;
        bytes.push(byte);
        if byte == b'\n' {
            return Ok(bytes);
        }
    }
    bail!("Native fixture worker response exceeds bound")
}
impl Owner {
    async fn start() -> Result<Self> {
        contracts()?;
        let owner = std::path::PathBuf::from(
            std::env::var_os("AGENSI_TLS_CI_SCRIPT")
                .ok_or_else(|| anyhow::anyhow!("Missing native fixture script"))?,
        );
        let shell = std::path::PathBuf::from(
            std::env::var_os("AGENSI_TLS_CI_PWSH")
                .ok_or_else(|| anyhow::anyhow!("Missing native fixture shell"))?,
        );
        let script = owner
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid fixture script"))?
            .join("windows-fixture-worker.ps1");
        ensure!(
            owner.is_absolute()
                && shell.is_absolute()
                && owner.is_file()
                && shell.is_file()
                && script.is_file(),
            "Invalid native fixture worker paths"
        );
        crate::reject_links(&script)?;
        let mut command = Command::new(shell);
        command
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .creation_flags(0x08000000);
        let mut child = command.spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Missing worker input"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Missing worker output"))?;
        let mut worker = Self {
            child,
            input: Some(input),
            output,
            next: 1,
        };
        let initialized = timeout(Duration::from_secs(30), async {
            response(&line(&mut worker.output).await?, None)
        })
        .await;
        if !matches!(&initialized, Ok(Ok(()))) {
            let cleanup = worker.terminate().await;
            ensure!(
                cleanup.is_ok(),
                "Native fixture bootstrap and cleanup failed"
            );
            bail!("Native fixture worker initialization failed");
        }
        eprintln!("Native CI fixture persistent worker initialized");
        Ok(worker)
    }
    async fn invoke(
        &mut self,
        action: &str,
        root: Option<&Path>,
        binary: Option<&Path>,
        certificate: Option<&Path>,
    ) -> Result<()> {
        ensure!(
            ["Validate", "Protect", "Trust", "RemoveCase"].contains(&action),
            "Invalid worker action"
        );
        let id = self.next;
        let bytes = serde_json::to_vec(
            &json!({"id":id,"action":action,"root":root,"binary":binary,"certificate":certificate}),
        )?;
        ensure!(
            !bytes.is_empty() && bytes.len() <= 8192,
            "Native fixture request exceeds bound"
        );
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Closed worker input"))?;
        timeout(Duration::from_secs(12), async {
            input
                .write_all(format!("{:04}", bytes.len()).as_bytes())
                .await?;
            input.write_all(&bytes).await?;
            input.flush().await?;
            response(&line(&mut self.output).await?, Some(id))
        })
        .await
        .map_err(|_| anyhow::anyhow!("Native fixture request deadline"))??;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Worker sequence exhausted"))?;
        Ok(())
    }
    async fn terminate(&mut self) -> Result<()> {
        self.input.take();
        if self.child.try_wait()?.is_some() {
            return Ok(());
        }
        let id = self
            .child
            .id()
            .ok_or_else(|| anyhow::anyhow!("Missing owned worker PID"))?;
        let tool = std::path::PathBuf::from(
            std::env::var_os("SYSTEMROOT")
                .ok_or_else(|| anyhow::anyhow!("Missing Windows system root"))?,
        )
        .join("System32/taskkill.exe");
        ensure!(
            tool.is_absolute() && tool.is_file(),
            "Native tree cleanup unavailable"
        );
        // Retain the actual Child handle through taskkill so this PID cannot be reused.
        let mut killer = Command::new(tool)
            .args(["/PID", &id.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .kill_on_drop(true)
            .spawn()?;
        let killed = match timeout(Duration::from_secs(5), killer.wait()).await {
            Ok(status) => status?.success(),
            Err(_) => {
                killer.start_kill()?;
                timeout(Duration::from_secs(3), killer.wait()).await??;
                false
            }
        };
        if !killed && self.child.try_wait()?.is_none() {
            self.child.start_kill()?;
        }
        timeout(Duration::from_secs(3), self.child.wait()).await??;
        ensure!(
            killed,
            "Native fixture tree cleanup failed; outer recovery required"
        );
        Ok(())
    }
    async fn close(&mut self) -> Result<()> {
        self.input.take();
        match timeout(Duration::from_secs(3), self.child.wait()).await {
            Ok(status) => ensure!(status?.success(), "Native fixture worker exit failed"),
            Err(_) => {
                self.terminate().await?;
                bail!("Native fixture worker shutdown deadline");
            }
        }
        eprintln!("Native CI fixture persistent worker reaped");
        Ok(())
    }
}

pub async fn invoke(
    action: &str,
    root: Option<&Path>,
    binary: Option<&Path>,
    certificate: Option<&Path>,
) -> Result<()> {
    let mut state = STATE.lock().await;
    ensure!(
        !state.invalid,
        "Native fixture worker is permanently invalid"
    );
    if state.owner.is_none() {
        match Owner::start().await {
            Ok(owner) => state.owner = Some(owner),
            Err(error) => {
                state.invalid = true;
                return Err(error);
            }
        }
    }
    let result = state
        .owner
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("Missing fixture worker"))?
        .invoke(action, root, binary, certificate)
        .await;
    if let Err(error) = result {
        state.invalid = true;
        if let Some(mut owner) = state.owner.take() {
            ensure!(
                owner.terminate().await.is_ok(),
                "Native fixture action and owned cleanup failed"
            );
        }
        return Err(error);
    }
    Ok(())
}
pub async fn shutdown() -> Result<()> {
    let mut state = STATE.lock().await;
    state.invalid = true;
    if let Some(mut owner) = state.owner.take() {
        owner.close().await?;
    }
    Ok(())
}
