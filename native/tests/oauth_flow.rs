use agensi_mcp::{auth::complete_authorization, credentials::FileCredentials, network::OAuthHttp};
use axum::{Json, Router, routing::get};
use rmcp::transport::auth::{
    AuthorizationManager, AuthorizationRequest, CredentialStore, OAuthState,
};
use serde_json::json;
use std::sync::Arc;
use tokio::{
    net::TcpListener,
    time::{Duration, timeout},
};
use url::Url;

async fn oauth_roundtrip(noisy: bool) {
    timeout(Duration::from_secs(if noisy { 12 } else { 8 }), async {
        let root = tempfile::tempdir().expect("root");
        let root_path = root.path().canonicalize().expect("canonical fixture root");
        let settings = root_path.join("config.local.json");
        std::fs::write(&settings, b"{}").expect("snapshot");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("AS port");
        let base = format!("http://{}",listener.local_addr().expect("AS address"));
        let metadata = json!({"issuer":base,"authorization_endpoint":format!("{base}/authorize"),"token_endpoint":format!("{base}/token"),
            "registration_endpoint":format!("{base}/register"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],
            "code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none"],"scopes_supported":["read","install"],"authorization_response_iss_parameter_supported":true});
        let prm = json!({"resource":format!("{base}/mcp"),"authorization_servers":[base],"scopes_supported":["read","install"]});
        let app = Router::new()
            .route("/.well-known/oauth-protected-resource/mcp", get({let v=prm.clone();move || async move {Json(v.clone())}}))
            .route("/.well-known/oauth-protected-resource", get({let v=prm.clone();move || async move {Json(v.clone())}}))
            .route("/.well-known/oauth-authorization-server", get({let v=metadata.clone();move || async move {Json(v.clone())}}))
            .route("/register",axum::routing::post(|Json(request):Json<serde_json::Value>| async move {
                Json(json!({"client_id":"fixture-client","redirect_uris":request["redirect_uris"],"token_endpoint_auth_method":"none","grant_types":["authorization_code","refresh_token"],"response_types":["code"]}))
            }))
            .route("/token",axum::routing::post(|body:String| async move {
                assert!(body.contains("code_verifier="),"PKCE verifier required");
                Json(json!({"access_token":"fixture-access","token_type":"Bearer","expires_in":3600,"refresh_token":"fixture-refresh","scope":"read install"}))
            }));
        let server = tokio::spawn(async move { axum::serve(listener,app).await.expect("AS serve"); });
        let mut config = agensi_mcp::config::initial();
        config.mode = agensi_mcp::config::Mode::Direct;
        let client = agensi_mcp::network::client(&config).expect("fixture client");
        let mut manager = AuthorizationManager::new_with_oauth_http_client(format!("{base}/mcp"),Arc::new(OAuthHttp {client,settings,snapshot:b"{}".to_vec()})).await.expect("manager");
        let store=FileCredentials::new(&root_path);manager.set_credential_store(store.clone());
        let callback=TcpListener::bind("127.0.0.1:0").await.expect("callback");let port=callback.local_addr().expect("callback port").port();
        let mut state=OAuthState::Unauthorized(manager);
        state.start_authorization(AuthorizationRequest::new(format!("http://127.0.0.1:{port}/oauth/callback")).with_client_name("Fixture")).await.expect("start");
        let link=Url::parse(&state.get_authorization_url().await.expect("URL")).expect("URL parse");
        let expected=link.query_pairs().find(|(k,_)|k=="state").expect("state").1.to_string();
        assert!(link.query_pairs().any(|(k,v)|k=="code_challenge_method" && v=="S256"));
        let mut received=Url::parse(&format!("http://127.0.0.1:{port}/oauth/callback")).expect("callback URL");
        received.query_pairs_mut().append_pair("state",&expected).append_pair("code","fixture-code").append_pair("iss",&base);
        let request=tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            if noisy {
                let mut idle = tokio::net::TcpStream::connect(("127.0.0.1",port)).await.unwrap();
                let mut rejected = Vec::new();
                idle.read_to_end(&mut rejected).await.unwrap();
                assert!(rejected.starts_with(b"HTTP/1.1 400"));
                for invalid in [b"\xff\r\n\r\n".as_slice(), b"POST /oauth/callback HTTP/1.1\r\n\r\n".as_slice(), b"GET /unrelated HTTP/1.1\r\n\r\n".as_slice()] {
                    let mut socket = tokio::net::TcpStream::connect(("127.0.0.1",port)).await.unwrap();
                    socket.write_all(invalid).await.unwrap();
                    let mut reply=Vec::new(); socket.read_to_end(&mut reply).await.unwrap();
                    assert!(reply.starts_with(b"HTTP/1.1 400"));
                }
                let mut socket = tokio::net::TcpStream::connect(("127.0.0.1",port)).await.unwrap();
                let target = format!("{}?{}", received.path(), received.query().unwrap());
                let request = format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
                socket.write_all(&request.as_bytes()[..12]).await.unwrap();
                tokio::time::sleep(Duration::from_millis(25)).await;
                socket.write_all(&request.as_bytes()[12..]).await.unwrap();
                let mut reply=Vec::new(); socket.read_to_end(&mut reply).await.unwrap();
                assert!(reply.starts_with(b"HTTP/1.1 200"));
            } else {
                let response=reqwest::Client::builder().no_proxy().build().expect("callbackclient").get(received).send().await.expect("callbacksend");
                assert!(response.status().is_success());
            }
        });
        complete_authorization(&callback,port,&mut state,&expected).await.expect("complete");
        request.await.expect("callbacktask");
        let saved=store.load().await.expect("saved").expect("session");
        assert_eq!(saved.client_id,"fixture-client");assert_eq!(saved.issuer.as_deref(),Some(base.as_str()));assert!(saved.token_received_at.is_some());
        server.abort();let _=server.await;
    }).await.expect("OAuth case deadline");
}

#[tokio::test]
async fn official_oauth_pkce_callback_saves_reusable_session() {
    oauth_roundtrip(false).await;
}

#[tokio::test]
async fn malformed_peers_and_fragmented_callback_do_not_abort_signin() {
    oauth_roundtrip(true).await;
}
