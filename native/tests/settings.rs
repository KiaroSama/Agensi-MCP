use agensi_mcp::{
    config::{self, initial},
    management, network, settings,
};

#[test]
fn local_http_port_defaults_and_atomic_update_preserve_route() {
    let mut cfg = initial();
    assert_eq!(settings::http_port(&cfg).unwrap(), 1594);
    for bad in ["0", "65536", "-1", "1.2", "invalid"] {
        assert!(settings::set_http_port(&mut cfg, bad).is_err());
    }
    assert_eq!(settings::http_port(&cfg).unwrap(), 1594);
    let root = tempfile::tempdir().unwrap();
    config::save(root.path(), &cfg).unwrap();
    management::command(root.path(), &["http-port".into(), "2594".into()], "").unwrap();
    let saved = config::load(root.path()).unwrap();
    assert_eq!(settings::http_port(&saved).unwrap(), 2594);
    assert_eq!(saved.mode, cfg.mode);
    assert_eq!(saved.profiles, cfg.profiles);
    let bytes = std::fs::read(root.path().join("config.local.json")).unwrap();
    assert!(management::command(root.path(), &["http-port".into(), "0".into()], "").is_err());
    assert_eq!(
        std::fs::read(root.path().join("config.local.json")).unwrap(),
        bytes
    );
}
#[test]
fn proxy_destination_dns_always_remote_without_changing_port_or_credentials() {
    for (scheme, effective) in [
        ("socks4", "socks4a"),
        ("socks4a", "socks4a"),
        ("socks5", "socks5h"),
        ("socks5h", "socks5h"),
        ("http", "http"),
        ("https", "https"),
    ] {
        let input = format!("{scheme}://fixture-user@127.0.0.1:1594");
        let selected = network::effective_proxy_url(&input).unwrap();
        assert_eq!(selected.scheme(), effective);
        assert_eq!(selected.port(), Some(1594));
        assert_eq!(selected.username(), "fixture-user");
    }
}

#[test]
fn malformed_saved_http_port_is_rejected_without_changing_proxy() {
    let mut cfg = initial();
    for bad in [
        serde_json::json!(0),
        serde_json::json!(65536),
        serde_json::json!("1594"),
        serde_json::json!(-1),
    ] {
        cfg.extra.insert("local_http_port".into(), bad);
        assert!(settings::http_port(&cfg).is_err());
    }
}
