use crate::{credentials::FileCredentials, network::OAuthHttp};
use anyhow::{Result, bail};
use rmcp::transport::auth::{AuthorizationManager, AuthorizationRequest, OAuthState};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use url::Url;

pub const ENDPOINT: &str = "https://mcp.agensi.io/mcp";
pub fn callback_port() -> u16 {
    let hash = format!("{:x}", md5::compute(ENDPOINT));
    let value = u16::from_str_radix(&hash[..4], 16).unwrap_or(0);
    (3335 + u32::from(value) % 45816) as u16
}
pub fn checked_callback(url: &Url, expected: &str) -> Result<(String, Option<String>)> {
    let get = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.to_string())
    };
    let state = get("state").unwrap_or_default();
    use subtle::ConstantTimeEq;
    if expected.is_empty()
        || state.len() != expected.len()
        || !bool::from(state.as_bytes().ct_eq(expected.as_bytes()))
    {
        bail!("Invalid authorization state.");
    }
    if get("error").is_some() {
        bail!("Authorization was not approved.");
    }
    let code = get("code")
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Authorization was not approved."))?;
    Ok((code, get("iss").filter(|v| !v.is_empty())))
}
pub async fn manager(
    root: &Path,
    client: crate::network::RoutedClient,
) -> Result<AuthorizationManager> {
    let settings = root.join("config.local.json");
    let snapshot =
        std::fs::read(&settings).map_err(|_| anyhow::anyhow!("Cannot read selected route."))?;
    let mut manager = AuthorizationManager::new_with_oauth_http_client(
        ENDPOINT,
        Arc::new(OAuthHttp {
            client: client.http.clone(),
            settings,
            snapshot,
        }),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Cannot initialize OAuth; selected route preserved."))?;
    let store = FileCredentials::new(root);
    {
        let _guard = store
            .guard()
            .await
            .map_err(|_| anyhow::anyhow!("Session is busy; retry later."))?;
        store
            .import_legacy(root)
            .await
            .map_err(|_| anyhow::anyhow!("Legacy session import failed; old session preserved."))?;
    }
    manager.set_credential_store(store);
    Ok(manager)
}
pub async fn authenticated(
    root: &Path,
    client: crate::network::RoutedClient,
) -> Result<AuthorizationManager> {
    let mut manager = manager(root, client).await?;
    if !manager
        .initialize_from_store()
        .await
        .map_err(|_| anyhow::anyhow!("Session validation failed; renew sign-in."))?
    {
        bail!("Rust session not available. Use Sign in; existing legacy session is preserved.");
    }
    manager
        .get_access_token()
        .await
        .map_err(|_| anyhow::anyhow!("Session expired or rejected; renew sign-in."))?;
    Ok(manager)
}
pub async fn signin(root: &Path, client: crate::network::RoutedClient) -> Result<()> {
    let store = FileCredentials::new(root);
    let auth_manager = manager(root, client).await?;
    let _guard = store
        .guard()
        .await
        .map_err(|_| anyhow::anyhow!("Another sign-in is active."))?;
    let port = callback_port();
    let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|_| {
        anyhow::anyhow!("OAuth callback port busy; close the previous connection and retry.")
    })?;
    let mut state = OAuthState::Unauthorized(auth_manager);
    state
        .start_authorization(
            AuthorizationRequest::new(format!("http://127.0.0.1:{port}/oauth/callback"))
                .with_client_name("Agensi Local Rust"),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!("OAuth discovery/registration failed through selected route.")
        })?;
    let link = state
        .get_authorization_url()
        .await
        .map_err(|_| anyhow::anyhow!("No authorization URL returned."))?;
    let url = Url::parse(&link)?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        bail!("Unsafe authorization URL refused.");
    }
    let expected = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| anyhow::anyhow!("Missing OAuth state."))?;
    eprintln!(
        "Copy this complete Agensi sign-in link into your external browser:\n{link}\nKeep this process open until approval completes."
    );
    complete_authorization(&listener, port, &mut state, &expected).await?;
    let manager = state
        .into_authorization_manager()
        .ok_or_else(|| anyhow::anyhow!("Authorization not completed."))?;
    manager
        .get_access_token()
        .await
        .map_err(|_| anyhow::anyhow!("No saved usable session."))?;
    eprintln!("Sign-in complete. Session saved locally for Desktop and Code.");
    Ok(())
}

pub async fn complete_authorization(
    listener: &TcpListener,
    port: u16,
    state: &mut OAuthState,
    expected: &str,
) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(300), async {
        loop {
            let (mut socket, _) = listener.accept().await?;
            let mut request = vec![0u8; 16384];
            let count = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut request)).await??;
            let request = std::str::from_utf8(&request[..count]).map_err(|_| anyhow::anyhow!("Invalid callback request."))?;
            let path = request.lines().next().and_then(|l| l.strip_prefix("GET ")).and_then(|l| l.split_whitespace().next()).unwrap_or("");
            let callback = Url::parse(&format!("http://127.0.0.1:{port}{path}"))?;
            let get = |name: &str| callback.query_pairs().find(|(k,_)| k == name).map(|(_,v)| v.to_string());
            if callback.path() != "/oauth/callback" || get("state").as_deref() != Some(expected) {
                socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?; continue;
            }
            let (code, issuer) = checked_callback(&callback, expected)?;
            let result = state.handle_callback_with_issuer(&code, expected, issuer.as_deref()).await;
            let reply = if result.is_ok() { "Authorization received; return to the menu." } else { "Authorization rejected; no session claimed." };
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).as_bytes()).await?;
            result.map_err(|_| anyhow::anyhow!("Callback/token exchange failed; no session claimed."))?;
            return Ok::<(), anyhow::Error>(());
        }
    }).await.map_err(|_| anyhow::anyhow!("Sign-in timed out; selected route preserved."))??;
    Ok(())
}
