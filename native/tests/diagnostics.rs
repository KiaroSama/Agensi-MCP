use agensi_mcp::diagnostics::{address_summary, resolver_summary, session_summary};
use serde_json::json;

#[tokio::test]
async fn doctor_reports_build_identity_without_contacting_the_network() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp");
    std::fs::create_dir_all(&base).unwrap();
    let dir = tempfile::tempdir_in(base.canonicalize().unwrap()).unwrap();
    let report = agensi_mcp::diagnostics::doctor(dir.path()).await.unwrap();
    assert_eq!(report["publisher"], "KiaroSama");
    assert_eq!(report["version"], "2.1.6");
    match option_env!("AGENSI_SOURCE_COMMIT") {
        Some(commit) if commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()) => {
            assert_eq!(report["source_commit"], commit);
        }
        _ => assert_eq!(report["source_commit"], "unknown"),
    }
    assert_eq!(report["network_checked"], false);
    assert_eq!(report["session"]["present"], false);
    assert_eq!(report["route_mode"], "disconnected");
    assert!(!dir.path().join(".state/auth/rust-session.json").exists());
}

#[test]
fn session_projection_never_exposes_tokens_or_client_identity() {
    let input = json!({"client_id":"private-client-fixture","issuer":"https://issuer.example","token_received_at":100,
        "token_response":{"access_token":"private-access-fixture","refresh_token":"private-refresh-fixture","expires_in":3600}});
    let report = session_summary(&input, 3700);
    assert_eq!(report["expired"], true);
    assert_eq!(report["expires_at_epoch"], 3700);
    assert_eq!(report["refresh_token_present"], true);
    assert_eq!(report["usable"], "not_checked");
    let text = report.to_string();
    for sensitive in [
        "private-client-fixture",
        "private-access-fixture",
        "private-refresh-fixture",
        "https://issuer.example",
    ] {
        assert!(!text.contains(sensitive));
    }
    assert_eq!(session_summary(&input, 3699)["expired"], false);
}

#[test]
fn absent_invalid_or_overflowing_expiry_stays_unknown() {
    for value in [
        json!({}),
        json!({"token_received_at":u64::MAX,"token_response":{"expires_in":1}}),
        json!({"token_received_at":100,"token_response":{"expires_in":"3600"}}),
    ] {
        let report = session_summary(&value, 200);
        assert!(report["expired"].is_null());
        assert!(report["expires_at_epoch"].is_null());
    }
}

#[test]
fn public_address_projection_validates_and_allowlists_fields() {
    let value = json!({"ip":"192.0.2.1","country":"GB","city":"London","org":"AS example","timezone":"Europe/London","private":"secret"});
    let report = address_summary(&value).unwrap();
    assert_eq!(report["ip"], "192.0.2.1");
    assert_eq!(report["country"], "GB");
    assert!(report.get("private").is_none());
    for bad in [json!({"ip":"not-an-ip"}), json!({}), json!({"ip":123})] {
        assert!(address_summary(&bad).is_err());
    }
    assert!(
        address_summary(&json!({"ip":"::1","country":"bad\nvalue"}))
            .unwrap()
            .get("country")
            .is_none()
    );
}

#[test]
fn dns_observations_do_not_convert_service_conclusion_into_leak_proof() {
    let rows = json!([{"type":"ip","ip":"192.0.2.1"},{"type":"dns","ip":"192.0.2.2","country":"gb","asn":"AS example"},
        {"type":"conclusion","ip":"DNS is not leaking."}]);
    let report = resolver_summary(&rows).unwrap();
    assert_eq!(report["observed"], true);
    assert_eq!(report["resolvers"].as_array().unwrap().len(), 1);
    assert_eq!(report["country_guaranteed"], false);
    assert_eq!(report["local_dns_leak_verified"], false);
    assert_eq!(resolver_summary(&json!([])).unwrap()["observed"], false);
    assert!(resolver_summary(&json!({})).is_err());
    assert!(resolver_summary(&json!([{"type":"dns","ip":"bad"}])).is_err());
}

#[test]
fn captured_route_and_change_detection_cannot_accept_an_intervening_selection() {
    use agensi_mcp::{config, diagnostics::route_unchanged};
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp");
    std::fs::create_dir_all(&base).unwrap();
    let dir = tempfile::tempdir_in(base.canonicalize().unwrap()).unwrap();
    let a = json!({"schema":1,"mode":"proxy","selected":"A","profiles":[
        {"name":"A","url":"socks5://127.0.0.1:1594"},{"name":"B","url":"socks5://127.0.0.1:2594"}]});
    let snapshot = serde_json::to_vec(&a).unwrap();
    std::fs::write(dir.path().join("config.local.json"), &snapshot).unwrap();
    let route = config::from_bytes(&snapshot).unwrap();
    assert_eq!(route.selected.as_deref(), Some("A"));
    assert!(route_unchanged(dir.path(), &snapshot).is_ok());
    let mut b = a;
    b["selected"] = json!("B");
    std::fs::write(
        dir.path().join("config.local.json"),
        serde_json::to_vec(&b).unwrap(),
    )
    .unwrap();
    assert!(route_unchanged(dir.path(), &snapshot).is_err());
    assert_eq!(route.selected.as_deref(), Some("A"));
}
