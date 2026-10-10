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
        let url = crate::config::validate_proxy(&profile.url)?;
        let proxy = if matches!(url.scheme(), "socks4" | "socks4a") && !url.username().is_empty() {
            let relay = Arc::new(socks4_userid::start(&url)?);
            let local = relay.proxy_url.clone();
            adapter = Some(relay);
            local
        } else {
            profile.url.clone()
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
    pub client: reqwest::Client,
    pub settings: PathBuf,
    pub snapshot: Vec<u8>,
}
impl OAuthHttpClient for OAuthHttp {
    fn execute(&self, input: OAuthHttpRequest) -> OAuthHttpClientFuture<'_> {
        Box::pin(async move {
            if std::fs::read(&self.settings).unwrap_or_default() != self.snapshot {
                return Err(std::io::Error::other("Route changed; reconnect.").into());
            }
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
    }
}
