use agensi_mcp::{auth, config, credentials::FileCredentials, logging::Logger, network, skill_files, storage};
use rmcp::transport::auth::{AuthorizationRequest, CredentialStore, OAuthState};
use std::{fs, path::Path, sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::Duration};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener};

fn root() -> tempfile::TempDir {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp");
    fs::create_dir_all(&path).unwrap();
    tempfile::tempdir_in(path.canonicalize().unwrap()).unwrap()
}
fn direct(root: &Path) -> Vec<u8> {
    let mut cfg = config::initial();
    cfg.mode = config::Mode::Direct;
    config::save(root, &cfg).unwrap();
    storage::snapshot(root).unwrap()
}

#[test]
fn skill_entrypoint_casing_is_portable_and_duplicates_remain_rejected() {
    let root = root();
    for (i, name) in ["SKILL.md", "skill.md", "Skill.md"].iter().enumerate() {
        let input = vec![skill_files::InlineFile { path: (*name).into(), content: "fixture instructions".into() }];
        let installed = skill_files::save_skill(root.path(), &format!("fixture-{i}"), &input).unwrap();
        assert_eq!(fs::read_to_string(installed.join("SKILL.md")).unwrap(), "fixture instructions");
        assert_eq!(fs::read_dir(&installed).unwrap().count(), 1);
    }
    let duplicate = vec![
        skill_files::InlineFile { path: "SKILL.md".into(), content: "a".into() },
        skill_files::InlineFile { path: "skill.md".into(), content: "b".into() },
    ];
    assert!(skill_files::save_skill(root.path(), "duplicate", &duplicate).is_err());
    assert!(!root.path().join("skills/duplicate").exists());
}

#[test]
fn local_json_limits_apply_before_parsing_and_before_replacement() {
    let root = root();
    assert!(storage::read_optional(&root.path().join("absent")).unwrap().is_none());
    let before = direct(root.path());
    let mut cfg = config::from_bytes(&before).unwrap();
    cfg.extra.insert("oversized".into(), serde_json::json!("x".repeat(storage::MAX_JSON_BYTES)));
    assert!(config::save(root.path(), &cfg).is_err());
    assert_eq!(storage::snapshot(root.path()).unwrap(), before);
    let path = root.path().join("large.json");
    fs::write(&path, vec![b'x'; storage::MAX_JSON_BYTES]).unwrap();
    assert_eq!(storage::read_optional(&path).unwrap().unwrap().len(), storage::MAX_JSON_BYTES);
    fs::write(&path, vec![b'x'; storage::MAX_JSON_BYTES + 1]).unwrap();
    assert!(storage::read_optional(&path).is_err());
    assert!(config::from_bytes(&vec![b' '; storage::MAX_JSON_BYTES + 1]).is_err());
    assert!(storage::read_optional(root.path()).is_err());
}

#[test]
fn logging_records_all_primary_levels_and_multiple_instances() {
    let root = root();
    for _ in 0..2 {
        let logger = Logger::new(root.path(), "fixture");
        for level in ["INFO", "WARNING", "ERROR", "DEBUG"] {
            logger.event(level, "fixture", "Safe diagnostic fixture.");
        }
    }
    let files: Vec<_> = fs::read_dir(root.path().join("logs")).unwrap().collect();
    assert_eq!(files.len(), 2);
    for entry in files {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        for level in ["INFO", "WARNING", "ERROR", "DEBUG"] {
            assert!(text.contains(&format!("UTC] [{level}] [fixture] Safe diagnostic fixture.")));
        }
    }
}

#[tokio::test]
async fn oauth_keeps_socks4_userid_adapter_alive_after_manager_takes_ownership() {
    tokio::time::timeout(Duration::from_secs(8), async {
        for scheme in ["socks4", "socks4a"] {
            let root = root();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let count = Arc::new(AtomicUsize::new(0));
            let observed = count.clone();
            let task = tokio::spawn(async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else { break; };
                    let mut header = [0; 8];
                    if stream.read_exact(&mut header).await.is_ok() {
                        assert_eq!(header[0], 4);
                        observed.fetch_add(1, Ordering::SeqCst);
                        let _ = stream.write_all(&[0, 91, 0, 0, 0, 0, 0, 0]).await;
                    }
                }
            });
            let mut cfg = config::initial();
            cfg.mode = config::Mode::Proxy;
            cfg.selected = Some("fixture".into());
            cfg.profiles.push(config::Profile { name: "fixture".into(), url: format!("{scheme}://fixture-user@{address}"), extra: Default::default() });
            config::save(root.path(), &cfg).unwrap();
            let client = network::client(&cfg).unwrap();
            let manager = auth::manager(root.path(), client, storage::snapshot(root.path()).unwrap()).await.unwrap();
            let mut state = OAuthState::Unauthorized(manager);
            let result = state.start_authorization(AuthorizationRequest::new("http://127.0.0.1:12345/oauth/callback".to_string())).await;
            task.abort();
            let _ = task.await;
            assert!(result.is_err(), "Fixture proxy deliberately rejects, not a live sign-in");
            assert!(count.load(Ordering::SeqCst) > 0, "OAuth lost the sole adapter owner before reaching the selected proxy");
        }
    }).await.expect("OAuth adapter regression deadline");
}

#[tokio::test]
async fn manager_refuses_an_intervening_route_change() {
    let root = root();
    let before = direct(root.path());
    let client = network::client(&config::from_bytes(&before).unwrap()).unwrap();
    config::save(root.path(), &config::initial()).unwrap();
    assert!(auth::manager(root.path(), client, before).await.is_err());
    assert!(!root.path().join(".state").exists());
}

#[tokio::test]
async fn route_guard_rejects_stale_results_and_cancels_pending_work() {
    let root = root();
    let snapshot = direct(root.path());
    let path = root.path().join("config.local.json");
    assert_eq!(network::watch_route(&path, &snapshot, async { Ok(7) }).await.unwrap(), 7);
    let result = network::watch_route(&path, &snapshot, async {
        fs::write(&path, b"{}").unwrap();
        Ok(7)
    }).await;
    assert!(result.is_err(), "A completed stale result must not be accepted");
    fs::write(&path, &snapshot).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        let result = network::watch_route(&path, &snapshot, async {
            fs::remove_file(&path).unwrap();
            std::future::pending::<anyhow::Result<()>>().await
        }).await;
        assert!(result.is_err());
    }).await.unwrap();
}

#[test]
fn ambiguous_oauth_callback_parameters_are_rejected() {
    for pair in ["state=fixture-state", "code=fixture-code", "iss=https%3A%2F%2Fissuer.example"] {
        let url = url::Url::parse(&format!("http://127.0.0.1/oauth/callback?state=fixture-state&code=fixture-code&iss=https%3A%2F%2Fissuer.example&{pair}")).unwrap();
        assert!(auth::checked_callback(&url, "fixture-state").is_err());
    }
    let url = url::Url::parse("http://127.0.0.1/oauth/callback?state=fixture-state&code=fixture-code").unwrap();
    assert!(auth::checked_callback(&url, "fixture-state").is_ok());
}

#[tokio::test]
async fn oversized_native_credentials_are_refused_without_mutation() {
    let root = root();
    let store = FileCredentials::new(root.path());
    fs::create_dir_all(store.path.parent().unwrap()).unwrap();
    let before = vec![b'x'; storage::MAX_JSON_BYTES + 1];
    fs::write(&store.path, &before).unwrap();
    assert!(store.load().await.is_err());
    assert_eq!(fs::read(&store.path).unwrap(), before);
}

#[cfg(unix)]
#[tokio::test]
async fn linked_credential_paths_and_locks_never_touch_external_files() {
    use rmcp::transport::auth::StoredCredentials;
    use std::os::unix::fs::symlink;
    let root = root();
    let outside = root.path().join("external");
    fs::create_dir(&outside).unwrap();
    fs::create_dir(root.path().join(".state")).unwrap();
    symlink(&outside, root.path().join(".state/auth")).unwrap();
    let store = FileCredentials::new(root.path());
    assert!(store.load().await.is_err());
    assert!(store.guard().await.is_err());
    assert!(store.save(StoredCredentials::new("fixture".into(), None, vec![], None)).await.is_err());
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    fs::remove_file(root.path().join(".state/auth")).unwrap();
    fs::create_dir(root.path().join(".state/auth")).unwrap();
    let sentinel = outside.join("sentinel");
    fs::write(&sentinel, b"unchanged").unwrap();
    symlink(&sentinel, store.path.with_extension("lock")).unwrap();
    assert!(store.guard().await.is_err());
    symlink(&sentinel, &store.path).unwrap();
    assert!(store.load().await.is_err());
    assert!(store.save(StoredCredentials::new("fixture".into(), None, vec![], None)).await.is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"unchanged");
}

#[cfg(unix)]
#[test]
fn linked_log_directory_is_refused_and_nonregular_reads_do_not_block() {
    use std::os::unix::fs::symlink;
    let root = root();
    let outside = root.path().join("external");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, root.path().join("logs")).unwrap();
    let logger = Logger::new(root.path(), "fixture");
    logger.event("INFO", "fixture", "Safe fallback fixture.");
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    let fifo = root.path().join("fixture-fifo");
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || { let _ = tx.send(storage::read_optional(&fifo).is_err()); });
    assert!(rx.recv_timeout(Duration::from_secs(1)).expect("FIFO must be refused before open"));
}

#[tokio::test]
async fn route_change_during_token_acquisition_prevents_upstream_contact() {
    use agensi_mcp::local_http::{AccessToken, HttpState};
    struct ChangingToken(std::path::PathBuf);
    impl AccessToken for ChangingToken {
        fn token(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send + '_>> {
            Box::pin(async move {
                fs::write(&self.0, b"{}")?;
                Ok("fixture-access".into())
            })
        }
    }
    tokio::time::timeout(Duration::from_secs(4), async {
        let root = root();
        let snapshot = direct(root.path());
        let settings = root.path().join("config.local.json");
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let state = Arc::new(HttpState {
            client: client.clone(), credentials: Arc::new(ChangingToken(settings.clone())),
            endpoint: format!("http://{}/mcp", origin.local_addr().unwrap()),
            port: address.port(), settings, snapshot,
            cancel: tokio_util::sync::CancellationToken::new(),
            requests: Arc::new(tokio::sync::Semaphore::new(32)),
        });
        let server = tokio::spawn(async move {
            axum::serve(listener, agensi_mcp::local_http::router(state)).await.unwrap();
        });
        let response = client.post(format!("http://{address}/mcp")).body("{}").send().await.unwrap();
        server.abort();
        let _ = server.await;
        assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        let origin = origin.into_std().unwrap();
        assert!(matches!(origin.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock));
    }).await.expect("HTTP route transition deadline");
}

#[cfg(unix)]
#[test]
fn operator_input_fifos_are_rejected_before_blocking_open() {
    for action in ["import", "save-skill"] {
        let root = root();
        let fifo = root.path().join("input-fifo");
        assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
        let path = fifo.to_string_lossy().to_string();
        let args = if action == "import" { vec![action.to_string(), path] } else { vec![action.to_string(), "fixture".into(), path] };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = agensi_mcp::management::command(root.path(), &args, "");
            let _ = tx.send(result.is_err());
        });
        assert!(rx.recv_timeout(Duration::from_secs(1)).expect("Operator input must not block on a FIFO"));
    }
}
