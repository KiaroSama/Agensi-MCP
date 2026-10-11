use crate::{
    config::{Config, Mode},
    socks4_userid,
};
use anyhow::{Result, bail};
use futures::StreamExt;
use rmcp::transport::auth::{OAuthHttpClient, OAuthHttpClientFuture, OAuthHttpRequest};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[derive(Clone)]
pub struct RoutedClient {
    pub http: reqwest::Client,
    _adapter: Option<Arc<socks4_userid::Adapter>>,
}
impl std::ops::Deref for RoutedClient {
    type Target = reqwest::Client;
    fn deref(&self) -> &Self::Target {
        &self.http
    }
}
pub fn effective_proxy_url(value: &str) -> Result<url::Url> {
    let mut url = crate::config::validate_proxy(value)?;
    let scheme = match url.scheme() {
        "socks4" => "socks4a",
        "socks5" => "socks5h",
        other => other,
    };
    let scheme = scheme.to_string();
    url.set_scheme(&scheme)
        .map_err(|_| anyhow::anyhow!("Cannot enforce proxy-side destination DNS."))?;
    Ok(url)
}
pub fn client(config: &Config) -> Result<RoutedClient> {
    if config.mode == Mode::Disconnected {
        bail!("Agensi is disconnected; select a route.");
    }
    let mut adapter = None;
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10));
    if config.mode == Mode::Proxy {
        let profile = config
            .profiles
            .iter()
            .find(|p| Some(&p.name) == config.selected.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Selected proxy is missing; connection blocked."))?;
        let url = effective_proxy_url(&profile.url)?;
        let proxy = if matches!(url.scheme(), "socks4" | "socks4a") && !url.username().is_empty() {
            let relay = Arc::new(socks4_userid::start(&url)?);
            let local = relay.proxy_url.clone();
            adapter = Some(relay);
            local
        } else {
            url.to_string()
        };
        builder = builder.proxy(
            reqwest::Proxy::all(&proxy)
                .map_err(|_| anyhow::anyhow!("Invalid proxy; connection blocked."))?,
        );
    }
    Ok(RoutedClient {
        http: builder
            .build()
            .map_err(|_| anyhow::anyhow!("Cannot create selected-route client."))?,
        _adapter: adapter,
    })
}

pub struct OAuthHttp {
    pub client: RoutedClient,
    pub settings: PathBuf,
    pub snapshot: Vec<u8>,
}
impl OAuthHttpClient for OAuthHttp {
    fn execute(&self, input: OAuthHttpRequest) -> OAuthHttpClientFuture<'_> {
        Box::pin(async move {
            if !crate::storage::matches(&self.settings, &self.snapshot) {
                return Err(std::io::Error::other("Route changed; reconnect.").into());
            }
            watch_route(&self.settings, &self.snapshot, async {
                let (parts, body) = input.request.into_parts();
                let response = self
                    .client
                    .request(parts.method, parts.uri.to_string())
                    .headers(parts.headers)
                    .body(body)
                    .timeout(input.timeout.unwrap_or(Duration::from_secs(20)))
                    .send()
                    .await?;
                let status = response.status();
                let headers = response.headers().clone();
                let mut bytes = Vec::new();
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk?;
                    if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                        return Err(std::io::Error::other("OAuth response exceeds limit.").into());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                let mut output = http::Response::new(bytes);
                *output.status_mut() = status;
                *output.headers_mut() = headers;
                Ok(output)
            })
            .await
            .map_err(|_| {
                std::io::Error::other("OAuth operation failed or route changed; values omitted.")
                    .into()
            })
        })
    }
}

/// Cancel bounded work on a route change, and reject stale results before returning them.
pub async fn watch_route<T>(
    settings: &std::path::Path,
    snapshot: &[u8],
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    anyhow::ensure!(
        crate::storage::matches(settings, snapshot),
        "Route changed; reconnect."
    );
    tokio::pin!(future);
    let mut interval = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                anyhow::ensure!(crate::storage::matches(settings, snapshot), "Route changed; reconnect.");
            }
            result = &mut future => {
                anyhow::ensure!(crate::storage::matches(settings, snapshot), "Route changed; reconnect.");
                return result;
            }
        }
    }
}
