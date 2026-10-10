use crate::{auth, config, network};
use anyhow::{Result, bail};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use rmcp::transport::{
    StreamableHttpClientTransport, Transport, async_rw::AsyncRwTransport, auth::AuthClient,
    streamable_http_client::StreamableHttpClientTransportConfig,
};
use std::{path::Path, time::Duration};

pub async fn run(root: &Path) -> Result<()> {
    let settings = root.join("config.local.json");
    let snapshot =
        std::fs::read(&settings).map_err(|_| anyhow::anyhow!("Cannot read selected route."))?;
    let cfg = config::load(root)?;
    let http = network::client(&cfg)?;
    let manager = auth::authenticated(root, http.clone()).await?;
    let upstream = StreamableHttpClientTransport::with_client(
        AuthClient::new(http.http.clone(), manager),
        StreamableHttpClientTransportConfig::with_uri(auth::ENDPOINT)
            .reinit_on_expired_session(false),
    );
    let downstream = AsyncRwTransport::new_server(tokio::io::stdin(), tokio::io::stdout());
    relay(downstream, upstream, settings, snapshot).await
}

pub async fn relay<D, U>(
    mut downstream: D,
    mut upstream: U,
    settings: std::path::PathBuf,
    snapshot: Vec<u8>,
) -> Result<()>
where
    D: Transport<rmcp::RoleServer>,
    U: Transport<rmcp::RoleClient>,
{
    let mut pending: FuturesUnordered<BoxFuture<'static, Result<()>>> = FuturesUnordered::new();
    let mut watcher = tokio::time::interval(Duration::from_millis(200));
    let result = loop {
        tokio::select! {
            _ = watcher.tick() => {
                if std::fs::read(&settings).unwrap_or_default() != snapshot { break Err(anyhow::anyhow!("Route changed; reconnect Agensi.")); }
            }
            _ = tokio::signal::ctrl_c() => break Ok(()),
            value = pending.next(), if !pending.is_empty() => {
                if let Some(Err(error)) = value { break Err(error); }
            }
            message = downstream.receive() => {
                let Some(message) = message else { break Ok(()); };
                if pending.len() >= 64 { break Err(anyhow::anyhow!("Pending request limit reached; no replay.")); }
                pending.push(upstream.send(message).map(|r| r.map_err(|_| anyhow::anyhow!("Upstream request failed; no replay or direct fallback."))).boxed());
            }
            message = upstream.receive() => {
                let Some(message) = message else { break Err(anyhow::anyhow!("Upstream connection closed.")); };
                if pending.len() >= 64 { break Err(anyhow::anyhow!("Pending output limit reached; reconnect.")); }
                pending.push(downstream.send(message).map(|r| r.map_err(|_| anyhow::anyhow!("Client output closed."))).boxed());
            }
        }
    };
    drop(pending);
    let _ = tokio::time::timeout(Duration::from_secs(5), upstream.close()).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), downstream.close()).await;
    result
}

pub async fn probe(root: &Path) -> Result<serde_json::Value> {
    use rmcp::{ServiceExt, model::ClientConfig};
    let cfg = config::load(root)?;
    let http = network::client(&cfg)?;
    let manager = auth::authenticated(root, http.clone()).await?;
    let transport = StreamableHttpClientTransport::with_client(
        AuthClient::new(http.http.clone(), manager),
        StreamableHttpClientTransportConfig::with_uri(auth::ENDPOINT)
            .reinit_on_expired_session(false),
    );
    let service = tokio::time::timeout(
        Duration::from_secs(20),
        ClientConfig::default().serve(transport),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Discovery timed out."))?
    .map_err(|_| anyhow::anyhow!("Discovery failed."))?;
    let tools = tokio::time::timeout(
        Duration::from_secs(15),
        service.list_tools(Default::default()),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Tool listing timed out."))?
    .map_err(|_| anyhow::anyhow!("Tool listing failed."))?;
    let count = tools.tools.len();
    let credit = tokio::time::timeout(
        Duration::from_secs(15),
        service.call_tool(rmcp::model::CallToolRequestParams::new("my_credits")),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Account read timed out."))?
    .map_err(|_| anyhow::anyhow!("Account read failed."))?;
    let account = credit.is_error != Some(true);
    let _ = service.cancel().await;
    if !account {
        bail!("Account service returned an error; authentication not claimed.");
    }
    if count == 0 {
        bail!("Upstream returned no tools.");
    }
    Ok(serde_json::json!({"connected":true,"tools":count,"account_read":account}))
}
