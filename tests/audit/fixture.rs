//! Synthetic loopback-only SOCKS/CONNECT, TLS, OAuth and MCP wire fixture.
use anyhow::{Result, bail, ensure};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, watch},
    task::{JoinHandle, JoinSet},
    time::{Duration, timeout},
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer},
};

#[path = "../../native/tests/support/pkce.rs"]
mod pkce;

pub const ISSUER: &str = "https://auth.agensi-audit.test";
const RESOURCE: &str = "https://mcp.agensi.io/mcp";
const HOSTS: [&str; 5] = [
    "mcp.agensi.io",
    "auth.agensi-audit.test",
    "ipinfo.io",
    "api.ipify.org",
    "bash.ws",
];

pub struct Certificates {
    pub ca: PathBuf,
    pub(crate) acceptor: TlsAcceptor,
}
impl Certificates {
    pub fn new(directory: &Path) -> Result<Self> {
        std::fs::create_dir_all(directory)?;
        let ca_key = KeyPair::generate()?;
        let mut ca_params = CertificateParams::new(Vec::<String>::new())?;
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "Ephemeral Agensi audit CA");
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca_cert = ca_params.self_signed(&ca_key)?;
        let issuer = Issuer::from_params(&ca_params, &ca_key);
        let key = KeyPair::generate()?;
        let mut hosts = HOSTS.map(str::to_owned).to_vec();
        hosts.push("127.0.0.1".into());
        let mut params = CertificateParams::new(hosts)?;
        params.distinguished_name.push(DnType::CommonName, HOSTS[0]);
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let cert = params.signed_by(&key, &issuer)?;
        let ca = directory.join("ca.pem");
        std::fs::write(&ca, ca_cert.pem())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
            std::fs::set_permissions(&ca, std::fs::Permissions::from_mode(0o600))?;
        }
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone(), ca_cert.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )?;
        Ok(Self {
            ca,
            acceptor: TlsAcceptor::from(Arc::new(config)),
        })
    }
}

#[derive(Default)]
struct State {
    requests: Vec<Value>,
    connections: Vec<String>,
    pause: Option<(String, Option<String>)>,
    paused: bool,
    resumed: bool,
    challenge: Option<String>,
    initial_expiry: u64,
    reject_refresh: bool,
    omit_refresh: bool,
    exchanges: usize,
    refreshes: usize,
}
pub struct Fixture {
    pub port: u16,
    state: Arc<Mutex<State>>,
    changed: Arc<Notify>,
    shutdown: watch::Sender<bool>,
    task: Option<JoinHandle<Result<()>>>,
}
impl Fixture {
    pub async fn start(certificates: &Certificates) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let state = Arc::new(Mutex::new(State::default()));
        let changed = Arc::new(Notify::new());
        let (shutdown, mut stop) = watch::channel(false);
        let acceptor = certificates.acceptor.clone();
        let shared = state.clone();
        let signal = changed.clone();
        let task = tokio::spawn(async move {
            let mut peers = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = stop.changed() => break,
                    peer = peers.join_next(), if !peers.is_empty() => {
                        if let Some(Err(error)) = peer { ensure!(error.is_cancelled(), "Fixture peer task failed"); }
                    }
                    accepted = listener.accept(), if peers.len() < 64 => {
                        let (socket, address) = accepted?;
                        ensure!(address.ip().is_loopback(), "Non-loopback fixture peer");
                        let (tls, state, changed) = (acceptor.clone(), shared.clone(), signal.clone());
                        peers.spawn(async move {
                            // A revoked route may close a connection mid-handshake; no payload is logged.
                            let _ = timeout(Duration::from_secs(8), handle(socket, tls, state, changed)).await;
                        });
                    }
                }
            }
            peers.abort_all();
            while peers.join_next().await.is_some() {}
            Ok(())
        });
        Ok(Self {
            port,
            state,
            changed,
            shutdown,
            task: Some(task),
        })
    }
    pub fn authorize(&self, url: &url::Url) -> Result<()> {
        let challenge =
            pkce::challenge(url).ok_or_else(|| anyhow::anyhow!("Invalid unique S256 challenge"))?;
        self.state.lock().expect("Fixture state").challenge = Some(challenge);
        Ok(())
    }
    pub fn token_policy(&self, initial_expiry: u64, reject_refresh: bool) {
        let mut state = self.state.lock().expect("Fixture state");
        state.initial_expiry = initial_expiry;
        state.reject_refresh = reject_refresh;
    }
    pub fn omit_refresh_token(&self) {
        self.state.lock().expect("Fixture state").omit_refresh = true;
    }
    pub fn token_counts(&self) -> (usize, usize) {
        let state = self.state.lock().expect("Fixture state");
        (state.exchanges, state.refreshes)
    }
    pub fn requests(&self) -> Vec<Value> {
        self.state.lock().expect("fixture state").requests.clone()
    }
    pub fn connections(&self) -> usize {
        self.state.lock().expect("fixture state").connections.len()
    }
    pub fn pause(&self, path: &str, rpc: Option<&str>) {
        let mut state = self.state.lock().expect("fixture state");
        state.pause = Some((path.into(), rpc.map(str::to_owned)));
        state.paused = false;
        state.resumed = false;
    }
    pub fn resume(&self) {
        self.state.lock().expect("fixture state").resumed = true;
        self.changed.notify_waiters();
    }
    pub async fn wait_paused(&self, bound: Duration) -> bool {
        timeout(bound, async {
            loop {
                let notified = self.changed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.state.lock().expect("fixture state").paused {
                    return;
                }
                notified.await;
            }
        })
        .await
        .is_ok()
    }
    pub async fn close(&mut self) -> Result<()> {
        self.resume();
        let _ = self.shutdown.send(true);
        if let Some(mut task) = self.task.take() {
            match timeout(Duration::from_secs(2), &mut task).await {
                Ok(result) => result??,
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    bail!("Fixture shutdown deadline");
                }
            }
        }
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn exact<R: AsyncRead + Unpin>(stream: &mut R, size: usize) -> Result<Vec<u8>> {
    let mut data = vec![0; size];
    stream.read_exact(&mut data).await?;
    Ok(data)
}
async fn until<R: AsyncRead + Unpin>(
    stream: &mut R,
    marker: &[u8],
    maximum: usize,
) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    while !data.ends_with(marker) {
        ensure!(data.len() < maximum, "Fixture input bound");
        data.push(stream.read_u8().await?);
    }
    Ok(data)
}
async fn handle(
    mut socket: TcpStream,
    acceptor: TlsAcceptor,
    state: Arc<Mutex<State>>,
    changed: Arc<Notify>,
) -> Result<()> {
    let first = socket.read_u8().await?;
    let protocol = match first {
        4 => {
            let request = exact(&mut socket, 7).await?;
            ensure!(request[0] == 1, "SOCKS4 CONNECT required");
            until(&mut socket, &[0], 512).await?;
            // SOCKS4a uses 0.0.0.x followed by hostname; plain SOCKS4 has no extra field.
            if request[3..6] == [0, 0, 0] && request[6] != 0 {
                until(&mut socket, &[0], 512).await?;
            }
            socket.write_all(&[0, 0x5a, 0, 80, 127, 0, 0, 1]).await?;
            "socks4a"
        }
        5 => {
            let count = socket.read_u8().await?;
            let methods = exact(&mut socket, usize::from(count)).await?;
            ensure!(methods.contains(&0), "Fixture needs unauthenticated SOCKS5");
            socket.write_all(&[5, 0]).await?;
            let header = exact(&mut socket, 4).await?;
            ensure!(header[..3] == [5, 1, 0], "SOCKS5 CONNECT required");
            let length = match header[3] {
                1 => 4,
                4 => 16,
                3 => usize::from(socket.read_u8().await?),
                _ => bail!("Invalid SOCKS address"),
            };
            exact(&mut socket, length).await?;
            exact(&mut socket, 2).await?;
            socket.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 80]).await?;
            "socks5"
        }
        _ => {
            let mut header = vec![first];
            header.extend(until(&mut socket, b"\r\n\r\n", 65535).await?);
            ensure!(header.starts_with(b"CONNECT "), "CONNECT required");
            socket
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await?;
            "http"
        }
    };
    state
        .lock()
        .expect("fixture state")
        .connections
        .push(protocol.into());
    let mut tls = acceptor.accept(socket).await?;
    let header = until(&mut tls, b"\r\n\r\n", 65536).await?;
    let header = std::str::from_utf8(&header)?;
    let mut lines = header.split("\r\n");
    let parts: Vec<_> = lines.next().unwrap_or_default().split(' ').collect();
    ensure!(parts.len() == 3, "Malformed fixture request");
    let (method, path) = (parts[0], parts[1]);
    let mut length = 0;
    let mut authenticated = false;
    for (index, line) in lines.filter(|line| !line.is_empty()).enumerate() {
        ensure!(index < 100, "Fixture header count bound");
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("Invalid fixture header"))?;
        if key.eq_ignore_ascii_case("content-length") {
            length = value.trim().parse::<usize>()?;
        }
        if key.eq_ignore_ascii_case("authorization") {
            authenticated = true;
        }
    }
    ensure!(length <= 2 * 1024 * 1024, "Fixture body bound");
    let body = exact(&mut tls, length).await?;
    let rpc = if path == "/mcp" && !body.is_empty() {
        serde_json::from_slice::<Value>(&body)?
            .get("method")
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let paused = {
        let mut state = state.lock().expect("fixture state");
        state.requests.push(
            json!({"method":method,"path":path,"authenticated":authenticated,"rpc_method":rpc}),
        );
        let paused = state.pause.as_ref().is_some_and(|(target, target_rpc)| {
            target == path
                && target_rpc
                    .as_ref()
                    .is_none_or(|value| rpc.as_str() == Some(value))
        });
        if paused {
            state.paused = true;
        }
        paused
    };
    changed.notify_waiters();
    if paused {
        timeout(Duration::from_secs(6), async {
            loop {
                let notification = changed.notified();
                tokio::pin!(notification);
                notification.as_mut().enable();
                if state.lock().expect("fixture state").resumed {
                    break;
                }
                notification.await;
            }
        })
        .await?;
    }
    let (status, data) = response(method, path, &body, &state)?;
    let encoded = serde_json::to_vec(&data)?;
    tls.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", encoded.len()).as_bytes()).await?;
    tls.write_all(&encoded).await?;
    tls.shutdown().await?;
    Ok(())
}
fn response(
    _method: &str,
    target: &str,
    body: &[u8],
    state: &Mutex<State>,
) -> Result<(u16, Value)> {
    let path = target.split('?').next().unwrap_or(target);
    Ok(match path {
        "/.well-known/oauth-protected-resource/mcp" | "/.well-known/oauth-protected-resource" => (
            200,
            json!({"resource":RESOURCE,"authorization_servers":[ISSUER],"scopes_supported":["read","install"]}),
        ),
        "/.well-known/oauth-authorization-server" | "/.well-known/openid-configuration" => (
            200,
            json!({"issuer":ISSUER,"authorization_endpoint":format!("{ISSUER}/authorize"),"token_endpoint":format!("{ISSUER}/token"),"registration_endpoint":format!("{ISSUER}/register"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none"],"scopes_supported":["read","install"],"authorization_response_iss_parameter_supported":true}),
        ),
        "/register" => {
            let request: Value = serde_json::from_slice(body)?;
            (
                201,
                json!({"client_id":"fixture-client","redirect_uris":request["redirect_uris"],"token_endpoint_auth_method":"none","grant_types":["authorization_code","refresh_token"],"response_types":["code"]}),
            )
        }
        "/token" => {
            let fields: Vec<_> = url::form_urlencoded::parse(body).collect();
            let grants: Vec<_> = fields
                .iter()
                .filter(|(key, _)| key == "grant_type")
                .collect();
            let mut state = state.lock().expect("Fixture state");
            let valid = grants.len() == 1
                && match grants[0].1.as_ref() {
                    "authorization_code" => {
                        let valid = state.challenge.as_ref().is_some_and(|challenge| {
                            pkce::authorization_form_valid(body, challenge)
                        });
                        if valid {
                            state.exchanges += 1;
                        }
                        valid
                    }
                    "refresh_token" => {
                        state.refreshes += 1;
                        !state.reject_refresh
                    }
                    _ => false,
                };
            if !valid {
                (400, json!({"error":"invalid_grant"}))
            } else {
                let expiry = if grants[0].1 == "authorization_code" && state.initial_expiry != 0 {
                    state.initial_expiry
                } else {
                    3600
                };
                let mut response = json!({"access_token":"synthetic-fixture-access","token_type":"Bearer","expires_in":expiry,"refresh_token":"synthetic-fixture-refresh","scope":"read install"});
                if state.omit_refresh {
                    response
                        .as_object_mut()
                        .expect("Token response")
                        .remove("refresh_token");
                }
                (200, response)
            }
        }
        "/mcp" => {
            let request: Value = if body.is_empty() {
                json!({})
            } else {
                serde_json::from_slice(body)?
            };
            let method = request["method"].as_str().unwrap_or("");
            let result = match method {
                "initialize" => {
                    json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"audit-fixture","version":"1"}})
                }
                "tools/list" => {
                    json!({"tools":[{"name":"my_credits","description":"Synthetic read-only fixture","inputSchema":{"type":"object"}}]})
                }
                "tools/call" => {
                    json!({"content":[{"type":"text","text":"synthetic account fixture"}]})
                }
                _ => json!({}),
            };
            (
                if method.starts_with("notifications/") {
                    202
                } else {
                    200
                },
                json!({"jsonrpc":"2.0","id":request.get("id").cloned().unwrap_or(json!(1)),"result":result}),
            )
        }
        _ => (404, json!({"error":"fixture not found"})),
    })
}

/// The TLS-proxy origin is stateless: a probe needs only the public MCP fixture response.
pub async fn serve_http<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    stream: &mut S,
) -> Result<()> {
    let header = until(stream, b"\r\n\r\n", 65536).await?;
    let header = std::str::from_utf8(&header)?;
    let mut lines = header.split("\r\n");
    let parts: Vec<_> = lines.next().unwrap_or_default().split(' ').collect();
    ensure!(parts.len() == 3, "Malformed TLS origin request");
    let mut length = 0;
    for line in lines {
        if let Some((key, value)) = line.split_once(':')
            && key.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>()?;
        }
    }
    ensure!(length <= 2 * 1024 * 1024, "TLS origin body bound");
    let body = exact(stream, length).await?;
    let (status, value) = response(parts[0], parts[1], &body, &Mutex::new(State::default()))?;
    let encoded = serde_json::to_vec(&value)?;
    stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",encoded.len()).as_bytes()).await?;
    stream.write_all(&encoded).await?;
    stream.shutdown().await?;
    Ok(())
}
