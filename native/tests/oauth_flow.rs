use agensi_mcp::{auth::complete_authorization, credentials::FileCredentials, network::OAuthHttp};
use axum::{Json, Router, routing::get};
use rmcp::transport::auth::{
    AuthorizationManager, AuthorizationRequest, CredentialStore, OAuthState,
};
use serde_json::json;
use std::sync::{Arc, Mutex};
#[path = "support/pkce.rs"]
mod pkce;
struct OwnedTask<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for OwnedTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
use tokio::{
    net::TcpListener,
    time::{Duration, timeout},
};
use url::Url;

async fn oauth_roundtrip(noisy: bool) {
    timeout(Duration::from_secs(if noisy { 12 } else { 8 }), async {
        let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp");
        std::fs::create_dir_all(&scratch).expect("Project scratch");
        let root = tempfile::Builder::new().prefix("oauth-é-").tempdir_in(scratch).expect("root");
        let challenge = Arc::new(Mutex::new(None::<String>));
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
            .route("/token",axum::routing::post({let challenge=challenge.clone();move |body:String| {let challenge=challenge.clone();async move {
                use axum::response::IntoResponse;
                let grants:Vec<_>=url::form_urlencoded::parse(body.as_bytes()).filter(|(key,_)|key=="grant_type").collect();
                let valid=grants.len()==1 && (grants[0].1=="refresh_token" || (grants[0].1=="authorization_code" && challenge.lock().expect("Challenge state").as_ref().is_some_and(|expected| pkce::authorization_form_valid(body.as_bytes(),expected))));
                if !valid { return (axum::http::StatusCode::BAD_REQUEST,Json(json!({"error":"invalid_grant"}))).into_response(); }
                Json(json!({"access_token":"fixture-access","token_type":"Bearer","expires_in":3600,"refresh_token":"fixture-refresh","scope":"read install"})).into_response()
            }}}));
        let mut server = OwnedTask(tokio::spawn(async move { axum::serve(listener,app).await.expect("AS serve"); }));
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
        *challenge.lock().expect("Challenge state")=Some(pkce::challenge(&link).expect("Unique nonempty S256 challenge"));
        let wire = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(2)).build().expect("Wire control client");
        for invalid in ["grant_type=authorization_code", "grant_type=authorization_code&code_verifier=", "grant_type=authorization_code&code_verifier=one&code_verifier=two", "grant_type=authorization_code&code_verifier=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"] {
            let reply=wire.post(format!("{base}/token")).header("Content-Type","application/x-www-form-urlencoded").body(invalid).send().await.expect("Negative token wire request");
            assert_eq!(reply.status(), axum::http::StatusCode::BAD_REQUEST);
            let body:serde_json::Value=reply.json().await.expect("Invalid grant response");
            assert_eq!(body["error"],"invalid_grant");
            assert!(store.load().await.expect("Store before valid exchange").is_none(),"Rejected verifier must not create a usable session");
        }
        let mut received=Url::parse(&format!("http://127.0.0.1:{port}/oauth/callback")).expect("callback URL");
        received.query_pairs_mut().append_pair("state",&expected).append_pair("code","fixture-code").append_pair("iss",&base);
        let mut request=OwnedTask(tokio::spawn(async move {
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
        }));
        complete_authorization(&callback,port,&mut state,&expected).await.expect("complete");
        (&mut request.0).await.expect("callbacktask");
        let saved=store.load().await.expect("saved").expect("session");
        assert_eq!(saved.client_id,"fixture-client");assert_eq!(saved.issuer.as_deref(),Some(base.as_str()));assert!(saved.token_received_at.is_some());
        server.0.abort();let _=(&mut server.0).await;
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

#[test]
fn rfc7636_vector_and_invalid_verifiers() {
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    assert!(pkce::authorization_form_valid(
        format!("code_verifier={verifier}").as_bytes(),
        challenge
    ));
    for body in [
        "",
        "code_verifier=",
        "code_verifier=wrong",
        &format!("code_verifier={verifier}&code_verifier={verifier}"),
        &format!("code_verifier={verifier}&code%5Fverifier={verifier}"),
        "code_verifier=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "code_verifier=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa%20",
    ] {
        assert!(
            !pkce::authorization_form_valid(body.as_bytes(), challenge),
            "Invalid verifier accepted"
        );
    }
}
