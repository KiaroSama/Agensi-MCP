use agensi_mcp::auth::{self, ENDPOINT};
use agensi_mcp::credentials::FileCredentials;
use fs2::FileExt;
use rmcp::transport::auth::{CredentialStore, OAuthTokenResponse, StoredCredentials};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;
use url::Url;

// Local adapter contracts only: no OAuth network/discovery/sign-in or user caches.
// Deterministic public-client legacy fixtures preserve original absolute expiry.
// Each async case has a 300 ms outer ceiling; no contention retry/sleep is tested.
// Parent owns issuer discovery, PKCE/token exchange/refresh and callback listener.
const LIMIT: Duration = Duration::from_millis(300);
const RECEIVED: u64 = 1_700_000_000;
const ISSUER: &str = "https://oauth-fixture.test";

fn root() -> TempDir {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp");
    fs::create_dir_all(&base).expect("project scratch root");
    tempfile::Builder::new()
        .prefix("auth fixture space-")
        .tempdir_in(base)
        .expect("isolated auth root")
}

async fn bounded(future: impl std::future::Future<Output = ()>) {
    tokio::time::timeout(LIMIT, future)
        .await
        .expect("local credential adapter case exceeded 300 ms");
}

fn tokens() -> Value {
    json!({
        "access_token": "fixture-access-token",
        "refresh_token": "fixture-refresh-token",
        "token_type": "Bearer",
        "expires_in": 3600,
        "expires_at": (RECEIVED + 3600) * 1000,
        "requested_scope": "read install",
        "scope": "read install",
        "issuer": ISSUER,
        "fixture_vendor_field": { "preserved": true }
    })
}

fn stored(received: Option<u64>) -> StoredCredentials {
    let response: OAuthTokenResponse =
        serde_json::from_value(tokens()).expect("realistic bearer token shape");
    StoredCredentials::new(
        "fixture-client".into(),
        Some(response),
        vec!["read".into(), "install".into()],
        received,
    )
    .with_issuer(Some(ISSUER.into()))
}

fn legacy(root: &Path, info: &Value, token: &Value) -> (PathBuf, PathBuf) {
    let directory = root.join(".state/auth/mcp-remote-v1");
    fs::create_dir_all(&directory).expect("legacy fixture directory");
    let hash = format!("{:x}", md5::compute(ENDPOINT));
    let info_path = directory.join(format!("{hash}_client_info.json"));
    let token_path = directory.join(format!("{hash}_tokens.json"));
    fs::write(
        &info_path,
        serde_json::to_vec_pretty(info).expect("client JSON"),
    )
    .expect("client fixture");
    fs::write(
        &token_path,
        serde_json::to_vec_pretty(token).expect("token JSON"),
    )
    .expect("token fixture");
    (info_path, token_path)
}

fn info() -> Value {
    json!({ "client_id": "fixture-client", "token_endpoint_auth_method": "none", "issuer": ISSUER,
        "redirect_uris": [format!("http://127.0.0.1:{}/oauth/callback", auth::callback_port())] })
}

#[tokio::test(flavor = "current_thread")]
async fn credential_roundtrip_preserves_receipt_issuer_scopes_and_vendor_fields() {
    bounded(async {
        let dir = root();
        let store = FileCredentials::new(dir.path());
        assert!(store.load().await.expect("absent session").is_none());
        for received in [Some(RECEIVED), Some(0), None] {
            let expected = stored(received);
            let expected_json = serde_json::to_value(&expected).expect("expected structural JSON");
            store.save(expected).await.expect("atomic session save");
            let actual = store
                .load()
                .await
                .expect("session reload")
                .expect("saved session");
            assert_eq!(
                actual.token_received_at, received,
                "Receipt must never become now on reload"
            );
            assert_eq!(actual.issuer.as_deref(), Some(ISSUER));
            assert_eq!(actual.granted_scopes, ["read", "install"]);
            assert_eq!(
                serde_json::to_value(&actual).expect("reloaded structural JSON"),
                expected_json
            );
            assert!(!format!("{actual:?}").contains("fixture-access-token"));
            assert!(!format!("{actual:?}").contains("fixture-refresh-token"));
        }
        let entries: Vec<_> = fs::read_dir(store.path.parent().expect("store directory"))
            .expect("entries")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert!(
            entries.iter().all(|name| name == "rust-session.json"),
            "Atomic save must clean its temporary file"
        );
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn issuer_clear_refuses_without_deleting_recovery_session() {
    bounded(async {
        let dir = root();
        let store = FileCredentials::new(dir.path());
        store
            .save(stored(Some(RECEIVED)))
            .await
            .expect("saved recovery session");
        let before = fs::read(&store.path).expect("before");
        let error = store
            .clear()
            .await
            .expect_err("automatic issuer clear must refuse");
        assert!(error.to_string().contains("preserved"));
        assert!(!error.to_string().contains("fixture-access-token"));
        assert_eq!(fs::read(&store.path).expect("recovery bytes"), before);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn corrupt_or_oversized_session_errors_are_sanitized() {
    bounded(async {
        let dir = root();
        let store = FileCredentials::new(dir.path());
        fs::create_dir_all(store.path.parent().expect("parent"))
            .expect("session fixture directory");
        for bytes in [
            b"{\"access_token\":\"FIXTURE_PRIVATE_VALUE\"".to_vec(),
            vec![b'x'; 1024 * 1024 + 1],
            vec![0xff, 0xfe],
        ] {
            fs::write(&store.path, &bytes).expect("invalid input");
            let error = store.load().await.expect_err("invalid session refused");
            assert!(!error.to_string().contains("FIXTURE_PRIVATE_VALUE"));
            assert_eq!(
                fs::read(&store.path).expect("preserved invalid input"),
                bytes
            );
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn public_legacy_import_keeps_expiry_epoch_and_original_files() {
    bounded(async {
        let dir = root();
        let (client_path, token_path) = legacy(dir.path(), &info(), &tokens());
        let client_before = fs::read(&client_path).expect("original client bytes");
        let token_before = fs::read(&token_path).expect("original token bytes");
        let store = FileCredentials::new(dir.path());
        assert!(
            store
                .import_legacy(dir.path())
                .await
                .expect("public client import")
        );
        let imported = store
            .load()
            .await
            .expect("imported session")
            .expect("session exists");
        assert_eq!(imported.client_id, "fixture-client");
        assert_eq!(
            imported.token_received_at,
            Some(RECEIVED),
            "Old token must retain original issue epoch even though expired"
        );
        assert_eq!(imported.issuer.as_deref(), Some(ISSUER));
        assert_eq!(imported.granted_scopes, ["read", "install"]);
        let json = serde_json::to_value(imported).expect("imported token JSON");
        assert_eq!(
            json["token_response"]["access_token"],
            "fixture-access-token"
        );
        assert_eq!(
            json["token_response"]["refresh_token"],
            "fixture-refresh-token"
        );
        assert_eq!(json["token_response"]["expires_in"], 3600);
        assert_eq!(
            json["token_response"]["fixture_vendor_field"],
            json!({ "preserved": true })
        );
        assert_eq!(
            fs::read(&client_path).expect("old client retained"),
            client_before
        );
        assert_eq!(
            fs::read(&token_path).expect("old token retained"),
            token_before
        );
        let before = fs::read(&store.path).expect("first import bytes");
        assert!(
            !store
                .import_legacy(dir.path())
                .await
                .expect("no repeat import")
        );
        assert_eq!(
            fs::read(&store.path).expect("no reset on repeated import"),
            before
        );
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn issuer_can_be_sourced_from_client_metadata_without_inventing_identity() {
    bounded(async {
        let dir = root();
        let mut token = tokens();
        token.as_object_mut().expect("object").remove("issuer");
        legacy(dir.path(), &info(), &token);
        let store = FileCredentials::new(dir.path());
        assert!(
            store
                .import_legacy(dir.path())
                .await
                .expect("explicit metadata issuer")
        );
        assert_eq!(
            store
                .load()
                .await
                .expect("load")
                .expect("imported session")
                .issuer
                .as_deref(),
            Some(ISSUER)
        );
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn existing_native_session_is_never_overwritten_by_legacy_import() {
    bounded(async {
        let dir = root();
        let store = FileCredentials::new(dir.path());
        store
            .save(stored(Some(RECEIVED + 100)))
            .await
            .expect("native session");
        let before = fs::read(&store.path).expect("existing native bytes");
        let (_, token_path) = legacy(dir.path(), &info(), &tokens());
        fs::write(token_path, b"invalid fixture-access-token").expect("legacy incompatible bytes");
        assert!(
            !store
                .import_legacy(dir.path())
                .await
                .expect("native session takes precedence")
        );
        assert_eq!(fs::read(&store.path).expect("unchanged native"), before);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn incompatible_legacy_metadata_preserves_old_files_and_claims_no_import() {
    let mut cases = Vec::new();
    let mut private = info();
    private["client_secret"] = json!("fixture-client-secret");
    cases.push(("private client", private, tokens()));
    let mut missing_client = info();
    missing_client
        .as_object_mut()
        .expect("object")
        .remove("client_id");
    cases.push(("missing client", missing_client, tokens()));
    let mut missing_issuer_info = info();
    missing_issuer_info
        .as_object_mut()
        .expect("object")
        .remove("issuer");
    let mut missing_issuer_token = tokens();
    missing_issuer_token
        .as_object_mut()
        .expect("object")
        .remove("issuer");
    cases.push(("unknown issuer", missing_issuer_info, missing_issuer_token));
    for field in ["expires_at", "expires_in", "requested_scope"] {
        let mut token = tokens();
        token.as_object_mut().expect("object").remove(field);
        cases.push((field, info(), token));
    }
    let mut invalid_epoch = tokens();
    invalid_epoch["expires_at"] = json!(1000);
    invalid_epoch["expires_in"] = json!(3600);
    cases.push(("expiry underflow", info(), invalid_epoch));
    for issuer in [
        "",
        "http://oauth-fixture.test",
        "https://fixture-user:fixture-pass@oauth-fixture.test",
    ] {
        let mut token = tokens();
        token["issuer"] = json!(issuer);
        cases.push(("unsafe issuer", info(), token));
    }
    for (name, client, token) in cases {
        bounded(async {
            let dir = root();
            let (client_path, token_path) = legacy(dir.path(), &client, &token);
            let before_client = fs::read(&client_path).expect("before client");
            let before_token = fs::read(&token_path).expect("before token");
            let store = FileCredentials::new(dir.path());
            assert!(
                !store.import_legacy(dir.path()).await.expect(name),
                "{name}"
            );
            assert!(
                !store.path.exists(),
                "{name}: no new session may be claimed"
            );
            assert_eq!(
                fs::read(client_path).expect("client preserved"),
                before_client,
                "{name}"
            );
            assert_eq!(
                fs::read(token_path).expect("token preserved"),
                before_token,
                "{name}"
            );
        })
        .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn corrupt_legacy_import_is_sanitized_and_preserves_both_files() {
    bounded(async {
        let dir = root();
        let (client_path, token_path) = legacy(dir.path(), &info(), &tokens());
        fs::write(&token_path, b"{\"access_token\":\"FIXTURE_PRIVATE_VALUE\"")
            .expect("malformed legacy token");
        let before_client = fs::read(&client_path).expect("client before");
        let before_token = fs::read(&token_path).expect("token before");
        let store = FileCredentials::new(dir.path());
        let error = store
            .import_legacy(dir.path())
            .await
            .expect_err("malformed legacy token refused");
        assert!(!error.to_string().contains("FIXTURE_PRIVATE_VALUE"));
        assert!(!store.path.exists());
        assert_eq!(
            fs::read(client_path).expect("client preserved"),
            before_client
        );
        assert_eq!(fs::read(token_path).expect("token preserved"), before_token);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn absent_or_half_legacy_cache_does_not_claim_a_session() {
    bounded(async {
        let dir = root();
        let store = FileCredentials::new(dir.path());
        assert!(!store.import_legacy(dir.path()).await.expect("no old cache"));
        let (_, token_path) = legacy(dir.path(), &info(), &tokens());
        fs::remove_file(token_path).expect("missing token fixture");
        assert!(!store.import_legacy(dir.path()).await.expect("half cache"));
        assert!(!store.path.exists());
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn refresh_guard_does_not_relock_load_save_and_releases_on_drop() {
    bounded(async {
        let dir = root();
        let store = FileCredentials::new(dir.path());
        let guard = store
            .acquire_refresh_guard()
            .await
            .expect("uncontended guard")
            .expect("file-backed guard");
        let lock_path = store.path.with_extension("lock");
        let external = OpenOptions::new()
            .read(true)
            .write(true)
            .open(lock_path)
            .expect("other lock handle");
        assert!(
            FileExt::try_lock_exclusive(&external).is_err(),
            "Guard must hold cross-handle exclusive lease"
        );
        assert!(
            store
                .load()
                .await
                .expect("load under refresh guard")
                .is_none()
        );
        store
            .save(stored(Some(RECEIVED)))
            .await
            .expect("save under existing refresh guard must not acquire it again");
        let loaded = store
            .load()
            .await
            .expect("load after guarded save")
            .expect("session");
        assert_eq!(loaded.token_received_at, Some(RECEIVED));
        drop(guard);
        FileExt::try_lock_exclusive(&external).expect("guard drop releases lease");
        FileExt::unlock(&external).expect("external release");
    })
    .await;
}

#[test]
fn generated_callback_source_oracle() {
    let fixtures: Value = serde_json::from_str(include_str!("../../tests/fixtures/migration.json"))
        .expect("source-generated fixture");
    let cases = fixtures["callbacks"]
        .as_array()
        .expect("callback source cases");
    assert!(
        !cases.is_empty(),
        "callback oracle coverage must not disappear"
    );
    for case in cases {
        let id = case["name"].as_str().expect("case name");
        let url = Url::parse(case["input"]["url"].as_str().expect("callback URL"))
            .expect("source input URL");
        let actual = auth::checked_callback(
            &url,
            case["input"]["state"].as_str().expect("expected state"),
        );
        if let Some(error) = case["error"].as_str() {
            assert_eq!(actual.expect_err(id).to_string(), error, "{id}");
        } else {
            let (code, issuer) = actual.expect(id);
            let mut value = json!({ "authorizationCode": code });
            if let Some(issuer) = issuer {
                value["iss"] = json!(issuer);
            }
            assert_eq!(value, case["expected"], "{id}");
        }
    }
}

#[test]
fn callback_checks_state_approval_and_returns_issuer_for_sdk_validation() {
    let callback = |query: &str| {
        Url::parse(&format!(
            "http://127.0.0.1:{}/oauth/callback?{query}",
            auth::callback_port()
        ))
        .expect("synthetic callback URL")
    };
    let accepted = auth::checked_callback(
        &callback("state=expected&code=fixture-code&iss=https%3A%2F%2Foauth-fixture.test"),
        "expected",
    )
    .expect("approved callback");
    assert_eq!(accepted, ("fixture-code".into(), Some(ISSUER.into())));
    assert_eq!(
        auth::checked_callback(&callback("state=expected&code=fixture-code"), "expected")
            .expect("missing issuer allowed before SDKvalidation"),
        ("fixture-code".into(), None)
    );
    for query in [
        "state=wrong&code=fixture-code",
        "state=&code=fixture-code",
        "code=fixture-code",
        "state=expecteD&code=fixture-code",
        "state=éxpected&code=fixture-code",
    ] {
        let error =
            auth::checked_callback(&callback(query), "expected").expect_err("invalid state");
        assert_eq!(error.to_string(), "Invalid authorization state.");
        assert!(!error.to_string().contains("fixture-code"));
    }
    assert!(auth::checked_callback(&callback("state=&code=fixture-code"), "").is_err());
    for query in [
        "state=expected&error=access_denied&code=fixture-code",
        "state=expected&error=",
        "state=expected",
        "state=expected&code=",
    ] {
        let error = auth::checked_callback(&callback(query), "expected")
            .expect_err("callback approval refused");
        assert!(!error.to_string().contains("fixture-code"));
        assert!(!error.to_string().contains("access_denied"));
    }
    // Parsing returns issuer; the SDK, not this pure query helper, binds it to
    // discovered authorization metadata before accepting token exchange.
    let (_, issuer) = auth::checked_callback(
        &callback("state=expected&code=fixture-code&iss=https%3A%2F%2Fother-fixture.test"),
        "expected",
    )
    .expect("issuer parsing only");
    assert_eq!(issuer.as_deref(), Some("https://other-fixture.test"));
}
