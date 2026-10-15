use crate::{credentials::FileCredentials, network::OAuthHttp};
use anyhow::{Result, bail};
use rmcp::transport::auth::{AuthorizationManager, AuthorizationRequest, OAuthState};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use url::Url;

#[path = "callback_headers.rs"]
mod callback_headers;

pub const ENDPOINT: &str = "https://mcp.agensi.io/mcp";
pub fn callback_port() -> u16 {
    let hash = format!("{:x}", md5::compute(ENDPOINT));
    let value = u16::from_str_radix(&hash[..4], 16).unwrap_or(0);
    (3335 + u32::from(value) % 45816) as u16
}
fn valid_callback_state(url: &Url, expected: &str) -> Result<()> {
    for key in ["state", "code", "iss", "error"] {
        anyhow::ensure!(
            url.query_pairs().filter(|(k, _)| k == key).count() <= 1,
            "Ambiguous callback parameters refused."
        );
    }
    let actual = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();
    use subtle::ConstantTimeEq;
    anyhow::ensure!(
        !expected.is_empty()
            && actual.len() == expected.len()
            && bool::from(actual.as_bytes().ct_eq(expected.as_bytes())),
        "Invalid authorization state."
    );
    Ok(())
}

pub fn checked_callback(url: &Url, expected: &str) -> Result<(String, Option<String>)> {
    valid_callback_state(url, expected)?;
    let get = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    if get("error").is_some() {
        bail!("Authorization was not approved.");
    }
    let code = get("code")
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Authorization was not approved."))?;
    Ok((code, get("iss").filter(|v| !v.is_empty())))
}

async fn read_callback(socket: &mut tokio::net::TcpStream, port: u16) -> Result<Url> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        let count = socket.read(&mut buffer).await?;
        anyhow::ensure!(count != 0, "Incomplete callback request.");
        bytes.extend_from_slice(&buffer[..count]);
        anyhow::ensure!(bytes.len() <= 16384, "Callback headers exceed limit.");
        if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| anyhow::anyhow!("Invalid callback request."))?;
    callback_headers::validate(text, port)?;
    let line = text.split("\r\n").next().unwrap_or_default();
    let parts: Vec<_> = line.split(' ').collect();
    anyhow::ensure!(
        parts.len() == 3
            && parts[0] == "GET"
            && matches!(parts[2], "HTTP/1.0" | "HTTP/1.1")
            && parts[1].starts_with('/')
            && !parts[1].starts_with("//")
            && !parts[1].contains('#')
            && !parts[1].chars().any(char::is_control),
        "Invalid callback request."
    );
    let url = Url::parse(&format!("http://127.0.0.1:{port}{}", parts[1]))
        .map_err(|_| anyhow::anyhow!("Invalid callback request."))?;
    anyhow::ensure!(url.path() == "/oauth/callback", "Invalid callback path.");
    Ok(url)
}

async fn callback_reply(socket: &mut tokio::net::TcpStream, accepted: bool) {
    let (status, body) = if accepted {
        ("200 OK", "Authorization received; return to the menu.")
    } else {
        ("400 Bad Request", "Authorization request rejected.")
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    // A browser disconnect must not turn a persisted session into a reported failure.
    let _ = tokio::time::timeout(Duration::from_secs(2), socket.write_all(reply.as_bytes())).await;
}

pub async fn manager(
    root: &Path,
    client: crate::network::RoutedClient,
    snapshot: Vec<u8>,
) -> Result<AuthorizationManager> {
    let settings = root.join("config.local.json");
    anyhow::ensure!(
        crate::storage::matches(&settings, &snapshot),
        "Route changed; reconnect."
    );
    let mut manager = AuthorizationManager::new_with_oauth_http_client(
        ENDPOINT,
        Arc::new(OAuthHttp {
            client,
            settings: settings.clone(),
            snapshot: snapshot.clone(),
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
    anyhow::ensure!(
        crate::storage::matches(&settings, &snapshot),
        "Route changed; reconnect."
    );
    Ok(manager)
}
pub async fn authenticated(
    root: &Path,
    client: crate::network::RoutedClient,
    snapshot: Vec<u8>,
) -> Result<AuthorizationManager> {
    let mut manager = manager(root, client, snapshot.clone()).await?;
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
    anyhow::ensure!(
        crate::storage::matches(&root.join("config.local.json"), &snapshot),
        "Route changed; reconnect."
    );
    Ok(manager)
}
pub async fn signin(
    root: &Path,
    client: crate::network::RoutedClient,
    snapshot: Vec<u8>,
) -> Result<()> {
    let store = FileCredentials::new(root);
    let auth_manager = manager(root, client, snapshot.clone()).await?;
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
    crate::network::watch_route(
        &root.join("config.local.json"),
        &snapshot,
        complete_authorization(&listener, port, &mut state, &expected),
    )
    .await?;
    let manager = state
        .into_authorization_manager()
        .ok_or_else(|| anyhow::anyhow!("Authorization not completed."))?;
    // Callback persistence is complete; SDK refresh now acquires its own exclusive guard.
    drop(_guard);
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
            let callback = match tokio::time::timeout(
                Duration::from_secs(5),
                read_callback(&mut socket, port),
            )
            .await
            {
                Ok(Ok(url)) => url,
                _ => {
                    callback_reply(&mut socket, false).await;
                    continue;
                }
            };
            if valid_callback_state(&callback, expected).is_err() {
                callback_reply(&mut socket, false).await;
                continue;
            }
            if callback.query_pairs().any(|(key, _)| key == "error") {
                callback_reply(&mut socket, false).await;
                bail!("Authorization was not approved.");
            }
            let (code, issuer) = match checked_callback(&callback, expected) {
                Ok(value) => value,
                Err(_) => {
                    callback_reply(&mut socket, false).await;
                    continue;
                }
            };
            let result = state
                .handle_callback_with_issuer(&code, expected, issuer.as_deref())
                .await;
            callback_reply(&mut socket, result.is_ok()).await;
            result.map_err(|_| {
                anyhow::anyhow!("Callback/token exchange failed; no session claimed.")
            })?;
            return Ok::<(), anyhow::Error>(());
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("Sign-in timed out; selected route preserved."))??;
    Ok(())
}
