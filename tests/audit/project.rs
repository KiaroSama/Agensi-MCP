//! Independent executable processes, bounded pipe readers and isolated project roots.
use crate::fixture::{Certificates, Fixture, ISSUER};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    process::{Child, Command},
    sync::Notify,
    task::JoinHandle,
    time::{Duration, timeout},
};
use url::Url;

pub const CLI_BOUND: Duration = Duration::from_secs(3);
const CAPTURE_LIMIT: usize = 2 * 1024 * 1024;

pub struct Context {
    pub source: PathBuf,
    pub work: PathBuf,
    pub certificates: Option<Certificates>,
}
impl Context {
    pub fn new(source: PathBuf, work: PathBuf) -> Self {
        Self {
            source,
            work,
            certificates: None,
        }
    }
    pub fn project(&self) -> Result<Project> {
        Project::new(&self.source, &self.work)
    }
    pub async fn route(&mut self, project: &mut Project, protocol: &str, user: bool) -> Result<()> {
        if self.certificates.is_none() {
            self.certificates = Some(Certificates::new(&self.work.join("fixture-tls"))?);
        }
        project
            .route(
                self.certificates.as_ref().expect("created CA"),
                protocol,
                user,
            )
            .await
    }
    pub async fn signin(
        &mut self,
        project: &mut Project,
        protocol: &str,
        user: bool,
    ) -> Result<(usize, bool)> {
        self.route(project, protocol, user).await?;
        let index = project.start("signin")?;
        if !project.processes[index].ready(false, CLI_BOUND).await {
            return Ok((index, false));
        }
        project.bind_authorization(index)?;
        let status = project.processes[index]
            .callback(Callback::default())
            .await?;
        let exit = project.processes[index].wait(CLI_BOUND).await?;
        Ok((
            index,
            status == Some(200) && exit == Some(0) && project.session_exists(),
        ))
    }
}

pub struct Project {
    pub root: PathBuf,
    directory: Option<tempfile::TempDir>,
    binary: PathBuf,
    env: BTreeMap<String, std::ffi::OsString>,
    pub processes: Vec<NativeProcess>,
    pub fixture: Option<Fixture>,
}
impl Project {
    fn new(source: &Path, work: &Path) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("agensi audit-é-")
            .tempdir_in(work)?;
        let root = directory.path().canonicalize()?;
        std::fs::create_dir(root.join("bin"))?;
        std::fs::create_dir(root.join(".tmp"))?;
        let name = if cfg!(windows) {
            "agensi-mcp.exe"
        } else {
            "agensi-mcp"
        };
        let binary = root.join("bin").join(name);
        std::fs::copy(source.join("bin").join(name), &binary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))?;
        }
        for name in ["Cargo.toml", "run.ps1"] {
            std::fs::copy(source.join(name), root.join(name))?;
        }
        let mut env = BTreeMap::new();
        for name in ["PATH", "SYSTEMROOT", "WINDIR"] {
            if let Some(value) = std::env::var_os(name) {
                env.insert(name.into(), value);
            }
        }
        env.insert("HOME".into(), root.clone().into_os_string());
        env.insert("RUST_BACKTRACE".into(), "0".into());
        for key in ["TEMP", "TMP", "TMPDIR"] {
            env.insert(key.into(), root.join(".tmp").into_os_string());
        }
        Ok(Self {
            root,
            directory: Some(directory),
            binary,
            env,
            processes: Vec::new(),
            fixture: None,
        })
    }
    fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .env_clear()
            .envs(&self.env)
            .current_dir(self.root.parent().expect("project parent"));
        command
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        command
    }
    pub fn trust_fixture(&mut self, ca: &Path) {
        self.env
            .insert("SSL_CERT_FILE".into(), ca.as_os_str().to_owned());
        let empty = self.root.join(".tmp/empty-trust-directory");
        std::fs::create_dir_all(&empty).expect("Empty child-only trust directory");
        self.env
            .insert("SSL_CERT_DIR".into(), empty.into_os_string());
    }
    pub fn bind_authorization(&self, index: usize) -> Result<()> {
        let capture = self.processes[index]
            .capture
            .lock()
            .expect("Process capture");
        let url = capture
            .authorization
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Authorization not ready"))?;
        self.fixture
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing authorization fixture"))?
            .authorize(url)
    }
    pub fn config(&self, value: Value) -> Result<()> {
        self.write_json("config.local.json", &value)
    }
    #[cfg(unix)]
    pub fn default_config(&self) -> Result<()> {
        self.config(json!({"schema":1,"mode":"disconnected","selected":null,"profiles":[]}))
    }
    pub fn write_json(&self, name: &str, value: &Value) -> Result<()> {
        std::fs::write(self.root.join(name), serde_json::to_vec(value)?)?;
        Ok(())
    }
    pub fn read_json(&self, name: &str) -> Result<Value> {
        Ok(serde_json::from_slice(&std::fs::read(
            self.root.join(name),
        )?)?)
    }
    pub fn session_exists(&self) -> bool {
        self.root.join(".state/auth/rust-session.json").is_file()
    }
    async fn route(
        &mut self,
        certificates: &Certificates,
        protocol: &str,
        user: bool,
    ) -> Result<()> {
        if self.fixture.is_none() {
            self.fixture = Some(Fixture::start(certificates).await?);
            self.env.insert(
                "SSL_CERT_FILE".into(),
                certificates.ca.clone().into_os_string(),
            );
            self.env.insert(
                "SSL_CERT_DIR".into(),
                certificates
                    .ca
                    .parent()
                    .expect("CA parent")
                    .as_os_str()
                    .to_owned(),
            );
        }
        self.config(json!({"schema":1,"mode":"proxy","selected":"fixture","profiles":[{"name":"fixture","url":format!("{protocol}://{}127.0.0.1:{}",if user {"fixture-user@"} else {""},self.fixture.as_ref().expect("fixture").port)}]}))
    }
    pub async fn run<S: AsRef<OsStr>>(
        &self,
        args: &[S],
        input: Option<&str>,
        bound: Duration,
    ) -> Result<Run> {
        let mut command = self.command();
        command.args(args);
        let mut process = NativeProcess::spawn(command)?;
        let execution: Result<Option<i32>> = async {
            if let Some(input) = input {
                let mut stdin = process.child.stdin.take().expect("piped stdin");
                timeout(bound, async {
                    stdin.write_all(input.as_bytes()).await?;
                    stdin.shutdown().await
                })
                .await??;
            } else {
                process.child.stdin.take();
            }
            process.wait(bound).await
        }
        .await;
        process.stop().await?;
        let exit = execution?;
        let timed_out = exit.is_none();
        let state = process.capture.lock().expect("process capture");
        let stdout = String::from_utf8(state.stdout.clone())?;
        let stderr = String::from_utf8(state.stderr.clone())?;
        Ok(Run {
            exit,
            timed_out,
            json: serde_json::from_str(&stdout).ok(),
            stderr,
        })
    }
    pub async fn cli(&self, args: &[&str]) -> Result<Run> {
        self.run(args, None, CLI_BOUND).await
    }
    pub fn start(&mut self, action: &str) -> Result<usize> {
        let mut command = self.command();
        command.arg(action);
        let process = NativeProcess::spawn(command)?;
        self.processes.push(process);
        Ok(self.processes.len() - 1)
    }
    pub async fn close(mut self) -> Result<()> {
        let mut failed = false;
        for process in &mut self.processes {
            failed |= process.stop().await.is_err();
        }
        if let Some(fixture) = &mut self.fixture {
            failed |= fixture.close().await.is_err();
        }
        self.processes.clear();
        self.fixture.take();
        if let Some(directory) = self.directory.take() {
            if failed {
                let _ = directory.keep();
            } else {
                directory.close()?;
            }
        }
        ensure!(!failed, "Owned project shutdown failed");
        Ok(())
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        for process in &mut self.processes {
            let _ = process.child.start_kill();
        }
        // On an exceptional path runtime shutdown reaps killed children; retain diagnostics
        // rather than remove the project while an owned process may still have files open.
        if (!self.processes.is_empty() || self.fixture.is_some())
            && let Some(directory) = self.directory.take()
        {
            let _ = directory.keep();
        }
    }
}
pub struct Run {
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub json: Option<Value>,
    pub stderr: String,
}
impl Run {
    pub fn success(&self) -> bool {
        !self.timed_out && self.exit == Some(0)
    }
}

#[derive(Default)]
struct Capture {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    authorization: Option<Url>,
    http_ready: bool,
}
pub struct NativeProcess {
    child: Child,
    capture: Arc<Mutex<Capture>>,
    changed: Arc<Notify>,
    readers: Vec<JoinHandle<Result<()>>>,
    pub exit: Option<i32>,
}
impl NativeProcess {
    fn spawn(mut command: Command) -> Result<Self> {
        let mut child = command.spawn()?;
        let capture = Arc::new(Mutex::new(Capture::default()));
        let changed = Arc::new(Notify::new());
        let stdout = child.stdout.take().expect("stdout pipe");
        let stderr = child.stderr.take().expect("stderr pipe");
        let readers = vec![
            tokio::spawn(consume(stdout, false, capture.clone(), changed.clone())),
            tokio::spawn(consume(stderr, true, capture.clone(), changed.clone())),
        ];
        Ok(Self {
            child,
            capture,
            changed,
            readers,
            exit: None,
        })
    }
    pub async fn ready(&self, http: bool, bound: Duration) -> bool {
        timeout(bound, async {
            loop {
                let signal = self.changed.notified();
                tokio::pin!(signal);
                signal.as_mut().enable();
                let ready = {
                    let capture = self.capture.lock().expect("capture");
                    if http {
                        capture.http_ready
                    } else {
                        capture.authorization.is_some()
                    }
                };
                if ready {
                    return;
                }
                signal.await;
            }
        })
        .await
        .is_ok()
    }
    pub fn http_ready(&self) -> bool {
        self.capture.lock().expect("capture").http_ready
    }
    pub fn alive(&mut self) -> Result<bool> {
        if let Some(status) = self.child.try_wait()? {
            self.exit = status.code();
            return Ok(false);
        }
        Ok(true)
    }
    pub async fn initialize(&mut self) -> Result<bool> {
        let input = json!({"jsonrpc":"2.0","id":7001,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"audit-fixture","version":"1"}}}).to_string() + "\n";
        timeout(
            CLI_BOUND,
            self.child
                .stdin
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("Missing held stdin"))?
                .write_all(input.as_bytes()),
        )
        .await??;
        Ok(timeout(CLI_BOUND, async {
            loop {
                let changed = self.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if self
                    .capture
                    .lock()
                    .expect("Capture")
                    .stdout
                    .split(|byte| *byte == b'\n')
                    .any(|line| {
                        serde_json::from_slice::<Value>(line).is_ok_and(|message| {
                            message["id"] == 7001
                                && message["result"]["protocolVersion"] == "2025-11-25"
                                && message["result"]["serverInfo"]["name"] == "audit-fixture"
                        })
                    })
                {
                    return;
                }
                changed.await;
            }
        })
        .await
        .is_ok())
    }
    pub fn close_input(&mut self) {
        self.child.stdin.take();
    }
    #[cfg(unix)]
    pub fn interrupt(&self) -> Result<()> {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        let pid = self
            .child
            .id()
            .ok_or_else(|| anyhow::anyhow!("Missing owned PID"))?;
        // SAFETY: signal only this live owned direct child; never a console or unrelated process group.
        ensure!(
            unsafe { kill(i32::try_from(pid)?, 2) } == 0,
            "Owned interrupt delivery failed"
        );
        Ok(())
    }
    pub async fn wait(&mut self, bound: Duration) -> Result<Option<i32>> {
        match timeout(bound, self.child.wait()).await {
            Ok(result) => {
                self.exit = result?.code();
                Ok(self.exit)
            }
            Err(_) => Ok(None),
        }
    }
    pub fn callback_target(&self, mutation: &Callback<'_>) -> Result<(u16, String)> {
        let capture = self.capture.lock().expect("capture");
        let query = capture
            .authorization
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Authorization not ready"))?;
        let pairs: BTreeMap<_, _> = query.query_pairs().into_owned().collect();
        let redirect = Url::parse(
            pairs
                .get("redirect_uri")
                .ok_or_else(|| anyhow::anyhow!("Missing redirect"))?,
        )?;
        ensure!(
            redirect.host_str() == Some("127.0.0.1") && redirect.scheme() == "http",
            "Callback must be loopback HTTP"
        );
        let port = redirect
            .port()
            .ok_or_else(|| anyhow::anyhow!("Missing callback port"))?;
        let state = if mutation.wrong_state {
            "wrong-state"
        } else {
            pairs
                .get("state")
                .ok_or_else(|| anyhow::anyhow!("Missing state"))?
        };
        let parameters = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("code", "synthetic-fixture-code")
            .append_pair("state", state)
            .append_pair("iss", ISSUER)
            .finish();
        Ok((
            port,
            format!("{}?{parameters}{}", redirect.path(), mutation.extra),
        ))
    }
    pub async fn callback(&self, mutation: Callback<'_>) -> Result<Option<u16>> {
        let (port, path) = self.callback_target(&mutation)?;
        let headers = match mutation.headers {
            Some(headers) => headers.replace("{port}", &port.to_string()),
            None => format!(
                "Host: {}\r\n",
                mutation
                    .host
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("127.0.0.1:{port}"))
            ),
        };
        let request = format!("GET {path} HTTP/1.1\r\n{headers}\r\n");
        match raw_callback(
            port,
            request.as_bytes(),
            mutation.fragment,
            Duration::from_secs(2),
        )
        .await
        {
            Ok(reply) => Ok(reply_status(&reply)),
            Err(_) => Ok(None),
        }
    }
    pub async fn stop(&mut self) -> Result<()> {
        let mut error = false;
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                error |= self.child.start_kill().is_err();
            }
        }
        error |= !matches!(timeout(CLI_BOUND, self.child.wait()).await, Ok(Ok(_)));
        for mut reader in self.readers.drain(..) {
            match timeout(Duration::from_secs(2), &mut reader).await {
                Ok(Ok(Ok(()))) => {}
                Ok(_) => error = true,
                Err(_) => {
                    reader.abort();
                    let _ = reader.await;
                    error = true;
                }
            }
        }
        ensure!(
            !error,
            "Native pipe reader failed or exceeded shutdown deadline"
        );
        Ok(())
    }
}
impl Drop for NativeProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        for reader in &self.readers {
            reader.abort();
        }
    }
}
#[derive(Default)]
pub struct Callback<'a> {
    pub extra: &'a str,
    pub wrong_state: bool,
    pub fragment: bool,
    pub host: Option<&'a str>,
    pub headers: Option<&'a str>,
}
pub fn reply_status(reply: &[u8]) -> Option<u16> {
    if !reply.starts_with(b"HTTP/") {
        return None;
    }
    std::str::from_utf8(reply.split(|b| *b == b' ').nth(1)?)
        .ok()?
        .parse()
        .ok()
}
pub async fn raw_callback(
    port: u16,
    request: &[u8],
    fragment: bool,
    bound: Duration,
) -> Result<Vec<u8>> {
    timeout(bound, async {
        let mut socket = TcpStream::connect(("127.0.0.1", port)).await?;
        if fragment {
            let split = b"GET /oauth/call".len();
            socket.write_all(&request[..split]).await?;
            // This delay models a partial TCP peer, never fixture readiness.
            tokio::time::sleep(Duration::from_millis(150)).await;
            socket.write_all(&request[split..]).await?;
        } else if !request.is_empty() {
            socket.write_all(request).await?;
        }
        let mut reply = Vec::new();
        let mut buffer = [0; 1024];
        while !reply.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let size = socket.read(&mut buffer).await?;
            if size == 0 {
                break;
            }
            ensure!(
                reply.len() + size <= 4096,
                "Callback response bound exceeded"
            );
            reply.extend_from_slice(&buffer[..size]);
        }
        Ok(reply)
    })
    .await?
}
async fn consume<R: AsyncRead + Unpin>(
    mut stream: R,
    stderr: bool,
    capture: Arc<Mutex<Capture>>,
    changed: Arc<Notify>,
) -> Result<()> {
    let mut buffer = [0; 4096];
    let mut pending = Vec::new();
    loop {
        let size = stream.read(&mut buffer).await?;
        if size == 0 {
            break;
        }
        let mut state = capture.lock().expect("capture");
        let output = if stderr {
            &mut state.stderr
        } else {
            &mut state.stdout
        };
        ensure!(
            output.len() + size <= CAPTURE_LIMIT,
            "Native capture bound exceeded"
        );
        output.extend_from_slice(&buffer[..size]);
        if stderr {
            pending.extend_from_slice(&buffer[..size]);
            ensure!(pending.len() <= 65536, "Native stderr line bound exceeded");
            while let Some(index) = pending.iter().position(|byte| *byte == b'\n') {
                let line = String::from_utf8(pending.drain(..=index).collect())?;
                if line.starts_with(&format!("{ISSUER}/authorize?")) {
                    state.authorization = Some(Url::parse(line.trim())?);
                }
                if line.contains("Local MCP HTTP listening") {
                    state.http_ready = true;
                }
            }
        }
        drop(state);
        changed.notify_waiters();
    }
    changed.notify_waiters();
    Ok(())
}
