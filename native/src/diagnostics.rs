use crate::{config, credentials::FileCredentials, network, settings};
use anyhow::{Result, anyhow, bail};
use futures::StreamExt;
use rmcp::transport::auth::CredentialStore;
use serde_json::{Value, json};
use std::{
    net::IpAddr,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub async fn doctor(root: &Path) -> Result<Value> {
    let cfg = config::load(root)?;
    let session = FileCredentials::new(root).load().await;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let state = match session {
        Ok(Some(saved)) => {
            let value = serde_json::to_value(&saved)
                .map_err(|_| anyhow!("Cannot inspect session metadata."))?;
            session_summary(&value, now)
        }
        Ok(None) => json!({"present":false,"usable":"not_checked"}),
        Err(_) => {
            json!({"present":root.join(".state/auth/rust-session.json").exists(),"readable":false,"usable":"unknown"})
        }
    };
    Ok(
        json!({"version":env!("CARGO_PKG_VERSION"),"route_mode":cfg.mode,"selected":cfg.selected,
        "saved_profiles":cfg.profiles.len(),"local_http_port":settings::http_port(&cfg)?,"session":state,
        "paths":{"session":".state/auth/rust-session.json","temporary":".tmp","logs":"logs","build_cache":".cache/cargo"},
        "network_checked":false,"warning":"Session metadata does not prove token refresh or live account access."}),
    )
}

pub fn session_summary(value: &Value, now: u64) -> Value {
    let receipt = value.get("token_received_at").and_then(Value::as_u64);
    let token = &value["token_response"];
    let expiry = receipt
        .zip(token.get("expires_in").and_then(Value::as_u64))
        .and_then(|(start, lifetime)| start.checked_add(lifetime));
    json!({"present":true,"readable":true,"access_token_present":token.get("access_token").and_then(Value::as_str).is_some_and(|v| !v.is_empty()),
        "refresh_token_present":token.get("refresh_token").and_then(Value::as_str).is_some_and(|v| !v.is_empty()),
        "received_at_epoch":receipt,"expires_at_epoch":expiry,"expired":expiry.map(|v| v<=now),
        "issuer_present":value.get("issuer").and_then(Value::as_str).is_some_and(|v| !v.is_empty()),"usable":"not_checked"})
}

pub fn address_summary(value: &Value) -> Result<Value> {
    let text = value
        .get("ip")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Diagnostic service returned no IP."))?;
    let _: IpAddr = text
        .parse()
        .map_err(|_| anyhow!("Diagnostic service returned an invalid IP."))?;
    let mut result = json!({"ip":text});
    for key in ["country", "city", "region", "org", "timezone"] {
        if let Some(text) = value
            .get(key)
            .and_then(Value::as_str)
            .filter(|v| v.len() <= 200 && !v.chars().any(char::is_control))
        {
            result[key] = json!(text);
        }
    }
    Ok(result)
}

pub fn resolver_summary(value: &Value) -> Result<Value> {
    let rows = value
        .as_array()
        .filter(|v| v.len() <= 100)
        .ok_or_else(|| anyhow!("Unexpected DNS observation response."))?;
    let mut resolvers = Vec::new();
    for row in rows.iter().filter(|v| v["type"] == "dns") {
        let mut item = address_summary(row)?;
        for key in ["country_name", "asn"] {
            if let Some(text) = row
                .get(key)
                .and_then(Value::as_str)
                .filter(|v| v.len() <= 200 && !v.chars().any(char::is_control))
            {
                item[key] = json!(text);
            }
        }
        resolvers.push(item);
    }
    Ok(
        json!({"observed":!resolvers.is_empty(),"resolvers":resolvers,"source":"bash.ws authoritative DNS observations",
        "country_guaranteed":false,"local_dns_leak_verified":false,
        "warning":"Observed resolver addresses are not proof of absence of every DNS leak; provider geography can change."}),
    )
}

pub fn route_unchanged(root: &Path, snapshot: &[u8]) -> Result<()> {
    let current = crate::storage::snapshot(root)
        .map_err(|_| anyhow!("Cannot verify selected route."))?;
    if current != snapshot {
        bail!("Route changed; diagnostic stopped without fallback.");
    }
    Ok(())
}
async fn get_json(route: &network::RoutedClient, url: &str) -> Result<Value> {
    let response = route
        .get(url)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| anyhow!("Selected-proxy diagnostic request failed; no direct fallback."))?;
    if !response.status().is_success() {
        bail!("Diagnostic service refused the request.");
    }
    let bytes = bounded_body(response).await?;
    serde_json::from_slice(&bytes).map_err(|_| anyhow!("Diagnostic service returned invalid JSON."))
}
async fn bounded_body(response: reqwest::Response) -> Result<Vec<u8>> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| anyhow!("Diagnostic response failed."))?;
        if bytes.len() + chunk.len() > 128 * 1024 {
            bail!("Diagnostic response exceeds limit.");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub async fn network_check(root: &Path) -> Result<Value> {
    tokio::time::timeout(Duration::from_secs(90), check(root))
        .await
        .map_err(|_| anyhow!("Network diagnostic exceeded 90 seconds; no fallback."))?
}
async fn check(root: &Path) -> Result<Value> {
    config::reject_linked_ancestors(&root.join("config.local.json"))?;
    let snapshot = crate::storage::snapshot(root)
        .map_err(|_| anyhow!("Cannot read selected route."))?;
    let cfg = config::from_bytes(&snapshot)?;
    if cfg.mode != config::Mode::Proxy {
        bail!("Network diagnostic requires an explicitly selected proxy.");
    }
    let route = network::client(&cfg)?;
    network::watch_route(&root.join("config.local.json"), &snapshot, async {
    route_unchanged(root, &snapshot)?;
    let egress = address_summary(&get_json(&route, "https://ipinfo.io/json").await?)?;
    route_unchanged(root, &snapshot)?;
    let verification = get_json(&route, "https://api.ipify.org?format=json")
        .await
        .and_then(|v| address_summary(&v));
    route_unchanged(root, &snapshot)?;
    let dns = match dns_check(root, &snapshot, &route).await {
        Ok(value) => value,
        Err(_) => {
            route_unchanged(root, &snapshot)?;
            json!({"observed":false,"resolvers":[],"error":"DNS observation unavailable; resolver identity remains unknown."})
        }
    };
    route_unchanged(root, &snapshot)?;
    let second = verification.ok();
    Ok(
        json!({"selected":cfg.selected,"checked_at_utc":chrono::Utc::now().to_rfc3339(),"egress":egress,
        "second_source":second,"egress_matches_second":second.as_ref().map(|v|v["ip"]==egress["ip"]),"dns":dns,
        "destination_dns":"proxy-side","timezone_metadata_added":false,"method":"Selected native routed client; no Agensi token sent to diagnostic services."}),
    )
    }).await
}
async fn dns_check(root: &Path, snapshot: &[u8], route: &network::RoutedClient) -> Result<Value> {
    route_unchanged(root, snapshot)?;
    let response = route
        .get("https://bash.ws/id")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|_| anyhow!("DNS test identifier unavailable."))?;
    if !response.status().is_success() {
        bail!("DNS test identifier unavailable.");
    }
    let bytes = bounded_body(response).await?;
    let id = std::str::from_utf8(&bytes)
        .map_err(|_| anyhow!("Invalid DNS test identifier."))?
        .trim();
    if !(6..=80).contains(&id.len()) || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        bail!("Invalid DNS test identifier.");
    }
    for number in 1..=4 {
        route_unchanged(root, snapshot)?;
        // TLS failures are expected for some test names; validation stays enabled. DNS precedes TLS.
        let _ = route
            .get(format!("https://{number}.{id}.bash.ws/"))
            .timeout(Duration::from_secs(8))
            .send()
            .await;
    }
    route_unchanged(root, snapshot)?;
    resolver_summary(&get_json(route, &format!("https://bash.ws/dnsleak/test/{id}?json")).await?)
}
