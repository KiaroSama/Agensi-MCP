use agensi_mcp::local_http::{self, AccessToken, HttpState};
use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::Response,
    routing::any,
};
use futures::{StreamExt, stream};
use std::{
    future::Future,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    net::TcpListener,
    sync::{Notify, Semaphore, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

// Real loopback HTTP on both sides of the public router, synthetic MCP messages,
// no external DNS/OAuth/network or settings. Every case has a five-second ceiling;
// bound sockets signal readiness, no sleeps, owned server tasks abort even on panic.
const LIMIT: Duration = Duration::from_secs(5);
const INIT: &str = r#"{ "jsonrpc":"2.0", "id":"original-7", "method":"initialize", "params":{"protocolVersion":"2025-11-25","capabilities":{"roots":{"listChanged":true}},"clientInfo":{"name":"original-client","version":"7"},"fixtureExtension":"世界"} }"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
const INIT_REPLY: &str = r#"{"jsonrpc":"2.0","id":"original-7","result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture-upstream","version":"1"}}}"#;
const SSE: &str = "id: event-7\nretry: 125\ndata: {\"jsonrpc\":\"2.0\",\"id\":\"server-root-1\",\"method\":\"roots/list\"}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":\"call-7\",\"result\":{\"content\":[]}}\n\n";
const MODERN_SSE: &str = "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"progressToken\":\"fixture-progress\",\"progress\":1}}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":\"call-7\",\"result\":{\"content\":[]}}\n\n";

async fn bounded(future: impl Future<Output = ()>) {
    tokio::time::timeout(LIMIT, future)
        .await
        .expect("local HTTP case exceeded five seconds");
}
struct OwnedTask(JoinHandle<()>);
impl Drop for OwnedTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl OwnedTask {
    async fn stop(&mut self) {
        self.0.abort();
        assert!(matches!((&mut self.0).await, Err(e) if e.is_cancelled()));
    }
}
struct FixtureToken {
    calls: AtomicUsize,
    fail: bool,
}
impl AccessToken for FixtureToken {
    fn token(&self) -> std::pin::Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + '_>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if self.fail {
                anyhow::bail!("Synthetic authorization failure");
            }
            Ok("fixture-upstream-access".into())
        })
    }
}
struct Observed {
    method: Method,
    headers: HeaderMap,
    body: Bytes,
}
#[derive(Clone)]
struct Backend {
    observed: mpsc::Sender<Observed>,
    stream_dropped: Arc<Notify>,
}
struct StreamDrop(Arc<Notify>);
impl Drop for StreamDrop {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
async fn upstream(State(state): State<Backend>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, 4 * 1024 * 1024).await.expect("fixture body");
    let method = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v["method"].as_str().map(str::to_owned))
        .unwrap_or_default();
    state
        .observed
        .send(Observed {
            method: parts.method.clone(),
            headers: parts.headers,
            body,
        })
        .await
        .expect("fixture observation");
    let mut reply = Response::builder().header(header::CACHE_CONTROL, "no-store");
    if method == "initialize" {
        return reply
            .header(header::CONTENT_TYPE, "application/json")
            .header("mcp-session-id", "fixture-session-7")
            .body(Body::from(INIT_REPLY))
            .unwrap();
    }
    if method.starts_with("notifications/") || parts.method == Method::DELETE {
        return reply
            .status(StatusCode::ACCEPTED)
            .body(Body::empty())
            .unwrap();
    }
    if method == "subscriptions/listen" {
        let guard = StreamDrop(state.stream_dropped);
        let data = stream::once(async {
            Ok::<_, std::io::Error>(Bytes::from_static(b": fixture-ready\n\n"))
        })
        .chain(stream::pending())
        .map(move |chunk| {
            let _ = &guard;
            chunk
        });
        return reply
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header("x-accel-buffering", "no")
            .body(Body::from_stream(data))
            .unwrap();
    }
    if method == "tools/call" {
        return reply
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(MODERN_SSE))
            .unwrap();
    }
    if method == "fixture/status" {
        reply = reply
            .status(StatusCode::UNAUTHORIZED)
            .header("www-authenticate", "Bearer error=\"invalid_token\"");
        return reply
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":9,"error":{"code":-32000,"message":"fixture rejected"}}"#,
            ))
            .unwrap();
    }
    reply
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(SSE))
        .unwrap()
}
struct Fixture {
    _dir: TempDir,
    local: String,
    client: reqwest::Client,
    state: Arc<HttpState>,
    token: Arc<FixtureToken>,
    observed: mpsc::Receiver<Observed>,
    stream_dropped: Arc<Notify>,
    tasks: Vec<OwnedTask>,
}
impl Fixture {
    async fn new(fail_auth: bool) -> Self {
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp");
        std::fs::create_dir_all(&scratch).expect("project test scratch");
        let dir = tempfile::Builder::new()
            .prefix("local HTTP fixture space-")
            .tempdir_in(scratch)
            .expect("isolated fixture directory");
        let settings = dir.path().join("config.local.json");
        let snapshot = br#"{"schema":1,"mode":"direct","profiles":[]}"#.to_vec();
        std::fs::write(&settings, &snapshot).expect("synthetic settings");
        let origin = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture upstream bind");
        let endpoint = format!("http://{}/mcp", origin.local_addr().unwrap());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("local relay bind");
        let address = listener.local_addr().unwrap();
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(3))
            .build()
            .expect("loopback-only fixture client");
        let token = Arc::new(FixtureToken {
            calls: AtomicUsize::new(0),
            fail: fail_auth,
        });
        let state = Arc::new(HttpState {
            client: client.clone(),
            credentials: token.clone(),
            endpoint,
            port: address.port(),
            settings,
            snapshot,
            cancel: CancellationToken::new(),
            requests: Arc::new(Semaphore::new(32)),
        });
        let (observe, observed) = mpsc::channel(32);
        let stream_dropped = Arc::new(Notify::new());
        let backend = Backend {
            observed: observe,
            stream_dropped: stream_dropped.clone(),
        };
        let app = Router::new()
            .route("/mcp", any(upstream))
            .with_state(backend);
        let origin = OwnedTask(tokio::spawn(async move {
            axum::serve(origin, app).await.unwrap();
        }));
        let app = local_http::router(state.clone());
        let relay = OwnedTask(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        Self {
            _dir: dir,
            local: format!("http://{address}/mcp"),
            client,
            state,
            token,
            observed,
            stream_dropped,
            tasks: vec![origin, relay],
        }
    }
    fn post(&self, body: impl Into<reqwest::Body>) -> reqwest::RequestBuilder {
        self.client
            .post(&self.local)
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
    }
    fn assert_no_egress(&mut self) {
        assert!(matches!(
            self.observed.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert_eq!(self.token.calls.load(Ordering::SeqCst), 0);
    }
    async fn stop(&mut self) {
        self.state.cancel.cancel();
        for task in &mut self.tasks {
            task.stop().await;
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn original_initialize_bytes_identity_session_and_json_response_survive() {
    bounded(async {
        let mut fixture = Fixture::new(false).await;
        let response = fixture.post(INIT).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["mcp-session-id"], "fixture-session-7");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        assert_eq!(response.text().await.unwrap(), INIT_REPLY);
        let request = fixture.observed.recv().await.unwrap();
        assert_eq!(request.body.as_ref(), INIT.as_bytes());
        assert_eq!(request.method, Method::POST);
        assert_eq!(
            request.headers[header::AUTHORIZATION],
            "Bearer fixture-upstream-access"
        );
        assert_eq!(
            request.headers[header::ACCEPT],
            "application/json, text/event-stream"
        );
        assert_eq!(fixture.token.calls.load(Ordering::SeqCst), 1);
        fixture.stop().await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_notification_get_sse_cancel_and_delete_preserve_protocol_headers() {
    bounded(async {
        let mut fixture = Fixture::new(false).await;
        for (method, body) in [(Method::POST, INITIALIZED), (Method::GET, ""),
            (Method::POST, r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"call-7"}}"#),
            (Method::DELETE, "")] {
            let response = fixture.client.request(method.clone(), &fixture.local)
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header("mcp-session-id", "fixture-session-7")
                .header("mcp-protocol-version", "2025-11-25")
                .header("last-event-id", "event-previous").body(body).send().await.unwrap();
            if method == Method::GET {
                assert_eq!(response.headers()[header::CONTENT_TYPE], "text/event-stream");
                assert_eq!(response.text().await.unwrap(), SSE);
            } else {
                assert_eq!(response.status(), StatusCode::ACCEPTED);
                assert!(response.bytes().await.unwrap().is_empty());
            }
            let seen = fixture.observed.recv().await.unwrap();
            assert_eq!(seen.method, method);
            assert_eq!(seen.body.as_ref(), body.as_bytes());
            assert_eq!(seen.headers["mcp-session-id"], "fixture-session-7");
            assert_eq!(seen.headers["mcp-protocol-version"], "2025-11-25");
            assert_eq!(seen.headers["last-event-id"], "event-previous");
        }
        fixture.stop().await;
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn modern_metadata_and_sse_bytes_survive_but_local_credentials_and_timezone_do_not() {
    bounded(async {
        let mut fixture = Fixture::new(false).await;
        let body = r#"{"jsonrpc":"2.0","id":"call-7","method":"tools/call","params":{"name":"fixture","arguments":{"region":"uk"},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#;
        let response = fixture.post(body)
            .header("mcp-protocol-version", "2026-07-28").header("mcp-method", "tools/call")
            .header("mcp-name", "fixture").header("mcp-param-region", "uk")
            .header(header::ORIGIN, format!("http://127.0.0.1:{}", fixture.state.port))
            .header(header::AUTHORIZATION, "Bearer untrusted-local-credential")
            .header(header::COOKIE, "fixture-cookie=local")
            .header("proxy-authorization", "Basic fixture-local")
            .header("timezone", "Europe/London").header("x-timezone", "Europe/London")
            .header("x-forwarded-host", "untrusted.invalid").send().await.unwrap();
        assert_eq!(response.text().await.unwrap(), MODERN_SSE);
        let request = fixture.observed.recv().await.unwrap();
        assert_eq!(request.body.as_ref(), body.as_bytes());
        for (name, value) in [("mcp-protocol-version", "2026-07-28"), ("mcp-method", "tools/call"),
            ("mcp-name", "fixture"), ("mcp-param-region", "uk")] {
            assert_eq!(request.headers[name], value, "required modern metadata {name}");
        }
        assert_eq!(request.headers[header::AUTHORIZATION], "Bearer fixture-upstream-access");
        for name in ["cookie", "origin", "proxy-authorization", "timezone", "x-timezone", "x-forwarded-host"] {
            assert!(!request.headers.contains_key(name), "local header leaked: {name}");
        }
        fixture.stop().await;
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn host_and_origin_rejections_happen_before_tokens_or_upstream_contact() {
    bounded(async {
        let mut fixture = Fixture::new(false).await;
        for (name, value) in [
            ("host", "untrusted.invalid:1594"),
            ("host", "127.0.0.1:1"),
            ("origin", "https://untrusted.invalid"),
            ("origin", "null"),
            ("origin", "http://127.0.0.1:1"),
            ("origin", "http://127.0.0.1:1594/path"),
        ] {
            let response = fixture.post(INIT).header(name, value).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{name}={value}");
            fixture.assert_no_egress();
        }
        let mut headers = HeaderMap::new();
        let host = HeaderValue::from_str(&format!("127.0.0.1:{}", fixture.state.port)).unwrap();
        headers.insert(header::HOST, host.clone());
        assert!(local_http::valid_local_request(
            &headers,
            fixture.state.port
        ));
        headers.append(header::HOST, host);
        assert!(!local_http::valid_local_request(
            &headers,
            fixture.state.port
        ));
        headers.remove(header::HOST);
        assert!(!local_http::valid_local_request(
            &headers,
            fixture.state.port
        ));
        headers.insert(
            header::HOST,
            HeaderValue::from_str(&format!("127.0.0.1:{}", fixture.state.port)).unwrap(),
        );
        let origin =
            HeaderValue::from_str(&format!("http://127.0.0.1:{}", fixture.state.port)).unwrap();
        headers.append(header::ORIGIN, origin.clone());
        assert!(local_http::valid_local_request(
            &headers,
            fixture.state.port
        ));
        headers.append(header::ORIGIN, origin);
        assert!(!local_http::valid_local_request(
            &headers,
            fixture.state.port
        ));
        fixture.stop().await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn wrong_path_query_method_and_oversized_body_never_reach_upstream() {
    bounded(async {
        let mut fixture = Fixture::new(false).await;
        for (method, path, expected) in [
            (Method::POST, "/wrong", StatusCode::NOT_FOUND),
            (
                Method::POST,
                "/mcp?target=untrusted",
                StatusCode::BAD_REQUEST,
            ),
            (Method::PUT, "/mcp", StatusCode::METHOD_NOT_ALLOWED),
            (Method::OPTIONS, "/mcp", StatusCode::METHOD_NOT_ALLOWED),
        ] {
            let base = fixture.local.strip_suffix("/mcp").unwrap();
            let response = fixture
                .client
                .request(method, format!("{base}{path}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            assert!(
                !response
                    .headers()
                    .contains_key("access-control-allow-origin")
            );
            fixture.assert_no_egress();
        }
        let response = fixture
            .post(vec![b'x'; 2 * 1024 * 1024 + 1])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        fixture.assert_no_egress();
        fixture.stop().await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn auth_failure_is_closed_and_upstream_error_status_is_not_replayed() {
    bounded(async {
        let mut denied = Fixture::new(true).await;
        let response = denied.post(INIT).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(matches!(
            denied.observed.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert_eq!(denied.token.calls.load(Ordering::SeqCst), 1);
        denied.stop().await;
        let mut fixture = Fixture::new(false).await;
        let response = fixture
            .post(r#"{"jsonrpc":"2.0","id":9,"method":"fixture/status"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers()["www-authenticate"],
            "Bearer error=\"invalid_token\""
        );
        assert!(response.text().await.unwrap().contains("fixture rejected"));
        fixture.observed.recv().await.unwrap();
        assert!(matches!(
            fixture.observed.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert_eq!(fixture.token.calls.load(Ordering::SeqCst), 1);
        fixture.stop().await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn changed_route_blocks_new_work_and_cancellation_closes_active_sse() {
    bounded(async {
        let mut fixture = Fixture::new(false).await;
        let response = fixture
            .post(r#"{"jsonrpc":"2.0","id":4,"method":"subscriptions/listen"}"#)
            .header("mcp-protocol-version", "2026-07-28")
            .header("mcp-method", "subscriptions/listen")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-accel-buffering"], "no");
        let mut bytes = response.bytes_stream();
        assert_eq!(
            bytes.next().await.unwrap().unwrap().as_ref(),
            b": fixture-ready\n\n"
        );
        fixture.observed.recv().await.unwrap();
        assert_eq!(fixture.state.requests.available_permits(), 31);
        std::fs::write(&fixture.state.settings, b"changed fixture route").unwrap();
        let response = fixture.post(INIT).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(fixture.token.calls.load(Ordering::SeqCst), 1);
        assert!(matches!(
            fixture.observed.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        // run() owns the file watcher; the public router receives its cancellation signal.
        fixture.state.cancel.cancel();
        assert!(
            bytes.next().await.is_none(),
            "cancelled SSE must end, not keep waiting"
        );
        fixture.stream_dropped.notified().await;
        assert_eq!(fixture.state.requests.available_permits(), 32);
        fixture.stop().await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn capacity_returns_429_without_authorization_and_recovers_after_release() {
    bounded(async {
        let mut fixture = Fixture::new(false).await;
        let permits = fixture
            .state
            .requests
            .clone()
            .acquire_many_owned(32)
            .await
            .unwrap();
        let response = fixture.post(INIT).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        fixture.assert_no_egress();
        drop(permits);
        let response = fixture.post(INIT).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), INIT_REPLY);
        fixture.observed.recv().await.unwrap();
        fixture.stop().await;
    })
    .await;
}
