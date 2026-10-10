use agensi_mcp::bridge;
use axum::{
    Json, Router,
    extract::State,
    response::{IntoResponse, Response},
    routing::post,
};
use rmcp::RoleClient;
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::{
    StreamableHttpClientTransport, Transport, async_rw::AsyncRwTransport,
    streamable_http_client::StreamableHttpClientTransportConfig,
};
use serde_json::{Value, json};
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{
    AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

// Actual SDK transport + newline-delimited bytes, not a separately initialized SDK
// client/service. Cases exercise the production relay seam with original wire IDs.
// Test-only channel transport controls send failure and close events; it never runs
// as app protocol implementation. Each test has a five-second outer deadline.
const LIMIT: Duration = Duration::from_secs(5);
struct OwnedTask<T>(JoinHandle<T>);
impl<T> Drop for OwnedTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl<T> OwnedTask<T> {
    async fn result(&mut self) -> T {
        (&mut self.0).await.expect("owned relay task join")
    }
}
async fn bounded(future: impl Future<Output = ()>) {
    tokio::time::timeout(LIMIT, future)
        .await
        .expect("protocol case exceeded five seconds");
}
fn root() -> TempDir {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp");
    std::fs::create_dir_all(&base).expect("project scratch root");
    tempfile::Builder::new()
        .prefix("protocol fixture space-")
        .tempdir_in(base)
        .expect("isolated route snapshot")
}
fn settings(dir: &TempDir) -> (PathBuf, Vec<u8>) {
    let path = dir.path().join("config.local.json");
    let snapshot = br#"{"schema":1,"mode":"direct","selected":null,"profiles":[]}"#.to_vec();
    std::fs::write(&path, &snapshot).expect("synthetic route snapshot");
    (path, snapshot)
}
type Reader = BufReader<ReadHalf<DuplexStream>>;
type Writer = WriteHalf<DuplexStream>;
fn peer(stream: DuplexStream) -> (Reader, Writer) {
    let (read, write) = tokio::io::split(stream);
    (BufReader::new(read), write)
}
async fn write(writer: &mut Writer, value: &Value) {
    let mut bytes = serde_json::to_vec(value).expect("wire fixture JSON");
    bytes.push(b'\n');
    writer.write_all(&bytes).await.expect("write wire fixture");
}
async fn read(reader: &mut Reader) -> Value {
    let mut line = String::new();
    assert!(
        reader.read_line(&mut line).await.expect("read wire JSON") != 0,
        "Relay output must not end before expected message"
    );
    serde_json::from_str(&line).expect("actual SDK output JSON")
}
fn initialize() -> Value {
    json!({ "jsonrpc": "2.0", "id": "owner-init-7", "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": { "roots": { "listChanged": true } },
        "clientInfo": { "name": "fixture-original-client", "version": "7.2.1" }
    } })
}

async fn fake_http_mcp(
    State(observed): State<mpsc::Sender<Value>>,
    Json(message): Json<Value>,
) -> Response {
    observed
        .send(message.clone())
        .await
        .expect("owned HTTP observation channel");
    if message.get("id").is_none() {
        return axum::http::StatusCode::ACCEPTED.into_response();
    }
    let result = match message["method"].as_str() {
        Some("initialize") => json!({ "protocolVersion": "2025-06-18", "capabilities": {},
            "serverInfo": { "name": "fixture-http-server", "version": "1" } }),
        Some("resources/list") => json!({ "resources": [] }),
        _ => json!({ "fixtureEcho": message["params"] }),
    };
    Json(json!({ "jsonrpc": "2.0", "id": message["id"], "result": result })).into_response()
}

#[tokio::test(flavor = "current_thread")]
async fn official_http_transport_relays_original_initialize_and_nontool_requests() {
    bounded(async {
        let dir = root(); let (settings, snapshot) = settings(&dir);
        let (observe, mut observed) = mpsc::channel(16);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("fake HTTP MCP bind");
        let address = listener.local_addr().expect("fake HTTP address");
        let app = Router::new().route("/mcp", post(fake_http_mcp)).with_state(observe);
        let (shutdown, ended) = tokio::sync::oneshot::channel();
        let mut server = OwnedTask(tokio::spawn(async move {
            axum::serve(listener, app).with_graceful_shutdown(async { let _ = ended.await; }).await
        }));
        let (owner, downstream) = tokio::io::duplex(32768);
        let (downread, downwrite) = tokio::io::split(downstream);
        let http = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().expect("loopback HTTP client");
        let upstream = StreamableHttpClientTransport::with_client(http,
            StreamableHttpClientTransportConfig::with_uri(format!("http://{address}/mcp"))
                .reinit_on_expired_session(false));
        let mut relay = OwnedTask(tokio::spawn(bridge::relay(AsyncRwTransport::new_server(downread, downwrite), upstream, settings, snapshot)));
        let (mut owner_read, mut owner_write) = peer(owner);
        let original = initialize();
        write(&mut owner_write, &original).await;
        assert_eq!(observed.recv().await.expect("HTTP initialize observed"), original,
            "Official HTTP worker must receive the owner's initialize, not an SDK substitute");
        let response = read(&mut owner_read).await;
        assert_eq!(response["id"], "owner-init-7");
        assert_eq!(response["result"]["serverInfo"]["name"], "fixture-http-server");
        let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        write(&mut owner_write, &notification).await;
        assert_eq!(observed.recv().await.expect("HTTP initialized observed"), notification);
        for message in [json!({ "jsonrpc": "2.0", "id": 71, "method": "resources/list", "params": {} }),
            json!({ "jsonrpc": "2.0", "id": "custom-http", "method": "fixture/custom", "params": { "data": [true, "café"] } })] {
            write(&mut owner_write, &message).await;
            assert_eq!(observed.recv().await.expect("HTTP request observed"), message);
            let reply = read(&mut owner_read).await;
            assert_eq!(reply["id"], message["id"]);
            if message["method"] == "resources/list" { assert_eq!(reply["result"]["resources"], json!([])); }
            else { assert_eq!(reply["result"]["fixtureEcho"], message["params"]); }
        }
        owner_write.shutdown().await.expect("HTTP owner EOF");
        relay.result().await.expect("official worker closes at owner EOF");
        shutdown.send(()).expect("HTTP server shutdown signal");
        server.result().await.expect("HTTP listener stopped");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn relay_preserves_both_directions_ids_custom_methods_errors_and_notifications() {
    bounded(async {
        let dir = root(); let (settings, snapshot) = settings(&dir);
        let (owner, downstream) = tokio::io::duplex(32768);
        let (upstream, remote) = tokio::io::duplex(32768);
        let (downread, downwrite) = tokio::io::split(downstream);
        let (upread, upwrite) = tokio::io::split(upstream);
        let mut relay = OwnedTask(tokio::spawn(bridge::relay(
            AsyncRwTransport::new_server(downread, downwrite),
            AsyncRwTransport::new_client(upread, upwrite), settings, snapshot)));
        let (mut owner_read, mut owner_write) = peer(owner);
        let (mut remote_read, mut remote_write) = peer(remote);
        let initialization = initialize();
        write(&mut owner_write, &initialization).await;
        assert_eq!(read(&mut remote_read).await, initialization, "Original client initialize must be relayed, not recreated");
        let initialized_response = json!({ "jsonrpc": "2.0", "id": "owner-init-7", "result": {
            "protocolVersion": "2025-06-18", "capabilities": {}, "serverInfo": { "name": "fixture-remote", "version": "1" }
        } });
        write(&mut remote_write, &initialized_response).await;
        assert_eq!(read(&mut owner_read).await, initialized_response);
        for message in [
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "id": 42, "method": "resources/list", "params": {} }),
            json!({ "jsonrpc": "2.0", "id": "custom-8", "method": "fixture/custom-request", "params": { "unicode": "café نمونه", "nested": [null, 1, true] } }),
            json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": "custom-8", "reason": "fixture cancellation" } }),
            json!({ "jsonrpc": "2.0", "method": "fixture/custom-notification", "params": { "ordered": [2, 1] } }),
        ] {
            write(&mut owner_write, &message).await;
            assert_eq!(read(&mut remote_read).await, message);
        }
        for message in [
            json!({ "jsonrpc": "2.0", "id": 42, "result": { "resources": [] } }),
            json!({ "jsonrpc": "2.0", "id": "custom-8", "error": { "code": -32001, "message": "fixture failure", "data": { "kind": "fixture" } } }),
            json!({ "jsonrpc": "2.0", "id": "server-request", "method": "fixture/server-request", "params": { "clientAction": "inspect" } }),
            json!({ "jsonrpc": "2.0", "method": "notifications/resources/list_changed" }),
            json!({ "jsonrpc": "2.0", "method": "fixture/server-notification", "params": { "event": "fixture" } }),
        ] {
            write(&mut remote_write, &message).await;
            assert_eq!(read(&mut owner_read).await, message);
        }
        let client_response = json!({ "jsonrpc": "2.0", "id": "server-request", "result": { "accepted": true } });
        write(&mut owner_write, &client_response).await;
        assert_eq!(read(&mut remote_read).await, client_response);
        owner_write.shutdown().await.expect("owner EOF");
        relay.result().await.expect("EOF closes relay cleanly");
        let mut rest = Vec::new();
        owner_read.read_to_end(&mut rest).await.expect("owner output closed");
        assert!(rest.is_empty(), "No autonomous handshake or unsolicited output at shutdown");
        remote_read.read_to_end(&mut rest).await.expect("upstream output closed");
        assert!(rest.is_empty());
    }).await;
}

struct ControlledUpstream {
    outgoing: mpsc::Sender<TxJsonRpcMessage<RoleClient>>,
    incoming: mpsc::Receiver<RxJsonRpcMessage<RoleClient>>,
    closed: Arc<AtomicBool>,
    fail_send: bool,
    hold_requests: bool,
}
impl Transport<RoleClient> for ControlledUpstream {
    type Error = io::Error;
    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        let sender = self.outgoing.clone();
        let fail = self.fail_send;
        let hold = self.hold_requests
            && serde_json::to_value(&item)
                .ok()
                .is_some_and(|value| value["method"] == "fixture/slow");
        async move {
            if fail {
                return Err(io::Error::other("fixture upstream send failure"));
            }
            sender
                .send(item)
                .await
                .map_err(|_| io::Error::other("fixture outgoing closed"))?;
            if hold {
                std::future::pending::<()>().await;
            }
            Ok(())
        }
    }
    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
        self.incoming.recv().await
    }
    async fn close(&mut self) -> io::Result<()> {
        self.closed.store(true, Ordering::SeqCst);
        self.incoming.close();
        Ok(())
    }
}
type ControlledFixture = (
    ControlledUpstream,
    mpsc::Receiver<TxJsonRpcMessage<RoleClient>>,
    mpsc::Sender<RxJsonRpcMessage<RoleClient>>,
    Arc<AtomicBool>,
);
fn controlled(fail_send: bool) -> ControlledFixture {
    let (outgoing, observed) = mpsc::channel(64);
    let (remote, incoming) = mpsc::channel(64);
    let closed = Arc::new(AtomicBool::new(false));
    (
        ControlledUpstream {
            outgoing,
            incoming,
            closed: closed.clone(),
            fail_send,
            hold_requests: false,
        },
        observed,
        remote,
        closed,
    )
}

#[tokio::test(flavor = "current_thread")]
async fn thirty_two_blocked_sends_do_not_starve_cancellation_or_server_requests() {
    bounded(async {
        let dir = root(); let (settings, snapshot) = settings(&dir);
        let (owner, downstream) = tokio::io::duplex(32768);
        let (downread, downwrite) = tokio::io::split(downstream);
        let (mut upstream, mut observed, remote, closed) = controlled(false);
        upstream.hold_requests = true;
        let mut relay = OwnedTask(tokio::spawn(bridge::relay(AsyncRwTransport::new_server(downread, downwrite), upstream, settings, snapshot)));
        let (mut owner_read, mut owner_write) = peer(owner);
        for id in 0..32 {
            write(&mut owner_write, &json!({ "jsonrpc": "2.0", "id": id, "method": "fixture/slow" })).await;
            let message = observed.recv().await.expect("each blocked send polled");
            assert_eq!(serde_json::to_value(message).expect("message")["id"], id);
        }
        let cancelled = json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 0, "reason": "fixture cancel under backpressure" } });
        write(&mut owner_write, &cancelled).await;
        assert_eq!(serde_json::to_value(observed.recv().await.expect("cancellation must not be starved")).expect("message"), cancelled);
        let server_request = json!({ "jsonrpc": "2.0", "id": "server-under-load", "method": "fixture/server-request", "params": { "control": true } });
        remote.send(serde_json::from_value(server_request.clone()).expect("SDK server request")).await.expect("server request queued");
        assert_eq!(read(&mut owner_read).await, server_request, "Upstream receive must remain live while sends await completion");
        owner_write.shutdown().await.expect("EOF under backpressure");
        relay.result().await.expect("EOF drops blocked sends instead of waiting indefinitely");
        assert!(closed.load(Ordering::SeqCst));
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn changed_route_closes_idle_relay_and_rejects_old_transport() {
    bounded(async {
        let dir = root();
        let (settings, snapshot) = settings(&dir);
        let (owner, downstream) = tokio::io::duplex(8192);
        let (downread, downwrite) = tokio::io::split(downstream);
        let (upstream, mut observed, _remote, closed) = controlled(false);
        let changed = settings.clone();
        let mut relay = OwnedTask(tokio::spawn(bridge::relay(
            AsyncRwTransport::new_server(downread, downwrite),
            upstream,
            settings,
            snapshot,
        )));
        let (mut read_owner, mut write_owner) = peer(owner);
        write(
            &mut write_owner,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "fixture/ready" }),
        )
        .await;
        let message = observed.recv().await.expect("relay ready event");
        assert_eq!(serde_json::to_value(message).expect("message")["id"], 1);
        std::fs::write(changed, b"{\"mode\":\"disconnected\"}")
            .expect("route changed by another manager");
        let result = relay.result().await.expect_err("old relay must stop");
        assert!(result.to_string().contains("Route changed"));
        assert!(
            closed.load(Ordering::SeqCst),
            "Upstream must close after route generation changes"
        );
        let mut line = String::new();
        assert_eq!(
            read_owner
                .read_line(&mut line)
                .await
                .expect("closed downstream"),
            0
        );
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn bounded_pending_capacity_fails_closed_instead_of_hanging() {
    bounded(async {
        let dir = root();
        let (settings, snapshot) = settings(&dir);
        let (owner, downstream) = tokio::io::duplex(32768);
        let (downread, downwrite) = tokio::io::split(downstream);
        let (mut upstream, mut observed, _remote, closed) = controlled(false);
        upstream.hold_requests = true;
        let mut relay = OwnedTask(tokio::spawn(bridge::relay(
            AsyncRwTransport::new_server(downread, downwrite),
            upstream,
            settings,
            snapshot,
        )));
        let (_owner_read, mut owner_write) = peer(owner);
        for id in 0..64 {
            write(
                &mut owner_write,
                &json!({ "jsonrpc": "2.0", "id": id, "method": "fixture/slow" }),
            )
            .await;
            assert!(
                observed.recv().await.is_some(),
                "bounded send must reach controlled transport"
            );
        }
        write(
            &mut owner_write,
            &json!({ "jsonrpc": "2.0", "id": 65, "method": "fixture/slow" }),
        )
        .await;
        let error = relay
            .result()
            .await
            .expect_err("finite capacity must stop explicitly");
        assert!(error.to_string().contains("Pending request limit"));
        assert!(closed.load(Ordering::SeqCst));
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn upstream_send_failure_is_fatal_without_replay() {
    bounded(async {
        let dir = root();
        let (settings, snapshot) = settings(&dir);
        let (owner, downstream) = tokio::io::duplex(8192);
        let (downread, downwrite) = tokio::io::split(downstream);
        let (upstream, _observed, _remote, closed) = controlled(true);
        let mut relay = OwnedTask(tokio::spawn(bridge::relay(
            AsyncRwTransport::new_server(downread, downwrite),
            upstream,
            settings,
            snapshot,
        )));
        let (_read_owner, mut write_owner) = peer(owner);
        write(
            &mut write_owner,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "fixture/no-retry" }),
        )
        .await;
        let error = relay
            .result()
            .await
            .expect_err("send failure terminates relay");
        assert!(error.to_string().contains("Upstream request failed"));
        assert!(closed.load(Ordering::SeqCst));
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn upstream_eof_closes_client_with_explicit_failure() {
    bounded(async {
        let dir = root();
        let (settings, snapshot) = settings(&dir);
        let (owner, downstream) = tokio::io::duplex(8192);
        let (downread, downwrite) = tokio::io::split(downstream);
        let (upstream, _observed, remote, closed) = controlled(false);
        let mut relay = OwnedTask(tokio::spawn(bridge::relay(
            AsyncRwTransport::new_server(downread, downwrite),
            upstream,
            settings,
            snapshot,
        )));
        let (mut read_owner, _write_owner) = peer(owner);
        drop(remote);
        let error = relay
            .result()
            .await
            .expect_err("upstream EOF explicit failure");
        assert!(error.to_string().contains("Upstream connection closed"));
        assert!(closed.load(Ordering::SeqCst));
        let mut line = String::new();
        assert_eq!(
            read_owner
                .read_line(&mut line)
                .await
                .expect("downstream EOF"),
            0
        );
    })
    .await;
}
