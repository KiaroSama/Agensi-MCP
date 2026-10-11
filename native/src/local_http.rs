use crate::{auth, config, network, settings};
use anyhow::{Result, bail};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::Response,
    routing::any,
};
use futures::StreamExt;
use rmcp::transport::auth::{AuthClient, AuthorizationManager};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::Semaphore};
use tokio_util::sync::CancellationToken;

pub trait AccessToken: Send + Sync {
    fn token(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + '_>>;
}
impl AccessToken for AuthClient<reqwest::Client> {
    fn token(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + '_>> {
        Box::pin(async move {
            self.get_access_token()
                .await
                .map_err(|_| anyhow::anyhow!("Session unavailable; renew sign-in."))
        })
    }
}
pub struct HttpState {
    pub client: reqwest::Client,
    pub credentials: Arc<dyn AccessToken>,
    pub endpoint: String,
    pub port: u16,
    pub settings: PathBuf,
    pub snapshot: Vec<u8>,
    pub cancel: CancellationToken,
    pub requests: Arc<Semaphore>,
}
fn error(code: StatusCode, text: &'static str) -> Response {
    Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(text))
        .expect("static response")
}
pub fn valid_local_request(headers: &HeaderMap, port: u16) -> bool {
    let authority = format!("127.0.0.1:{port}");
    if headers.get_all(header::HOST).iter().count() != 1
        || headers.get(header::HOST).and_then(|v| v.to_str().ok()) != Some(authority.as_str())
    {
        return false;
    }
    let origins: Vec<_> = headers.get_all(header::ORIGIN).iter().collect();
    if origins.len() > 1 {
        return false;
    }
    origins
        .first()
        .is_none_or(|v| v.to_str().ok() == Some(format!("http://{authority}").as_str()))
}
pub fn router(state: Arc<HttpState>) -> Router {
    Router::new().route("/mcp", any(forward)).with_state(state)
}
async fn forward(State(state): State<Arc<HttpState>>, request: Request) -> Response {
    if !valid_local_request(request.headers(), state.port) {
        return error(StatusCode::FORBIDDEN, "Local Host/Origin refused.");
    }
    if request.uri().query().is_some() || request.uri().authority().is_some() {
        return error(
            StatusCode::BAD_REQUEST,
            "Only the fixed MCP endpoint is allowed.",
        );
    }
    if !matches!(
        *request.method(),
        Method::GET | Method::POST | Method::DELETE
    ) {
        return error(
            StatusCode::METHOD_NOT_ALLOWED,
            "Unsupported MCP HTTP method.",
        );
    }
    if !crate::storage::matches(&state.settings, &state.snapshot) || state.cancel.is_cancelled() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Route changed; reconnect.");
    }
    let permit = match state.requests.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "Local request capacity reached.",
            );
        }
    };
    let (parts, body) = request.into_parts();
    let body = match tokio::time::timeout(Duration::from_secs(10), to_bytes(body, 2 * 1024 * 1024))
        .await
    {
        Ok(Ok(b)) => b,
        _ => return error(StatusCode::PAYLOAD_TOO_LARGE, "MCP request body refused."),
    };
    if !crate::storage::matches(&state.settings, &state.snapshot) || state.cancel.is_cancelled() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Route changed; reconnect.");
    }
    let token = match tokio::select! {
        _ = state.cancel.cancelled() => return error(StatusCode::SERVICE_UNAVAILABLE, "Route changed; reconnect."),
        token = tokio::time::timeout(Duration::from_secs(20), state.credentials.token()) => token,
    } {
        Ok(Ok(t)) => t,
        _ => {
            return error(
                StatusCode::UNAUTHORIZED,
                "Session unavailable; renew sign-in.",
            );
        }
    };
    if !crate::storage::matches(&state.settings, &state.snapshot) || state.cancel.is_cancelled() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Route changed; reconnect.");
    }
    let mut upstream = state
        .client
        .request(parts.method, &state.endpoint)
        .bearer_auth(token)
        .body(body);
    for name in [
        header::ACCEPT,
        header::CONTENT_TYPE,
        header::HeaderName::from_static("mcp-protocol-version"),
        header::HeaderName::from_static("mcp-session-id"),
        header::HeaderName::from_static("last-event-id"),
    ] {
        if let Some(value) = parts.headers.get(&name) {
            upstream = upstream.header(name, value);
        }
    }
    for (name, value) in &parts.headers {
        let key = name.as_str();
        if key == "mcp-method" || key == "mcp-name" || key.starts_with("mcp-param-") {
            upstream = upstream.header(name, value);
        }
    }
    let response = tokio::select! {
        _=state.cancel.cancelled()=>return error(StatusCode::SERVICE_UNAVAILABLE,"Route changed; reconnect."),
        response=tokio::time::timeout(Duration::from_secs(30),upstream.send())=>match response {Ok(Ok(r))=>r,_=>return error(StatusCode::BAD_GATEWAY,"Selected-route upstream request failed; no fallback or replay.")},
    };
    if !crate::storage::matches(&state.settings, &state.snapshot) || state.cancel.is_cancelled() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Route changed; reconnect.");
    }
    let mut builder = Response::builder().status(response.status());
    for name in [
        header::CONTENT_TYPE,
        header::CACHE_CONTROL,
        header::WWW_AUTHENTICATE,
        header::HeaderName::from_static("mcp-session-id"),
        header::HeaderName::from_static("x-accel-buffering"),
    ] {
        if let Some(value) = response.headers().get(&name) {
            builder = builder.header(name, value);
        }
    }
    let cancelled = state.cancel.clone();
    let bytes = response
        .bytes_stream()
        .take_until(cancelled.cancelled_owned());
    // Holding permit in the stream bounds active SSE/session response lifetimes too.
    let bytes = bytes.map(move |chunk| {
        let _ = &permit;
        chunk
    });
    builder
        .body(Body::from_stream(bytes))
        .unwrap_or_else(|_| error(StatusCode::BAD_GATEWAY, "Invalid upstream response."))
}
pub async fn run(root: &std::path::Path) -> Result<()> {
    let snapshot = crate::storage::snapshot(root)?;
    let config = config::from_bytes(&snapshot)?;
    let port = settings::http_port(&config)?;
    let route = network::client(&config)?;
    let manager: AuthorizationManager =
        auth::authenticated(root, route.clone(), snapshot.clone()).await?;
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|_| anyhow::anyhow!("Local HTTP port is busy; change http-port settings."))?;
    anyhow::ensure!(
        crate::storage::matches(&root.join("config.local.json"), &snapshot),
        "Route changed; reconnect."
    );
    let cancel = CancellationToken::new();
    let state = Arc::new(HttpState {
        client: route.http.clone(),
        credentials: Arc::new(AuthClient::new(route.http.clone(), manager)),
        endpoint: auth::ENDPOINT.into(),
        port,
        settings: root.join("config.local.json"),
        snapshot,
        cancel: cancel.clone(),
        requests: Arc::new(Semaphore::new(32)),
    });
    eprintln!(
        "Local MCP HTTP listening at http://127.0.0.1:{port}/mcp; local processes are trusted without a token."
    );
    let settings_path = state.settings.clone();
    let initial = state.snapshot.clone();
    let stop = cancel.clone();
    let watcher = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(200));
        loop {
            tokio::select! {_=stop.cancelled()=>break,_=interval.tick()=>if !crate::storage::matches(&settings_path,&initial){stop.cancel();break;},_=tokio::signal::ctrl_c()=>{stop.cancel();break;}}
        }
    });
    let result = axum::serve(listener, router(state))
        .with_graceful_shutdown(cancel.clone().cancelled_owned())
        .await;
    cancel.cancel();
    watcher.abort();
    let _ = watcher.await;
    drop(route);
    if result.is_err() {
        bail!("Local HTTP server stopped with an error.");
    }
    Ok(())
}
