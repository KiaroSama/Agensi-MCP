use agensi_mcp::{
    config::{Mode, initial},
    network,
};
use tokio::{
    net::TcpListener,
    time::{Duration, timeout},
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer},
};

#[tokio::test]
async fn untrusted_destination_certificate_is_rejected() {
    timeout(Duration::from_secs(5), async {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".into()])
                .expect("fixture certificate");
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
            )
            .expect("fixture TLS config");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture port");
        let address = listener.local_addr().expect("fixture address");
        let acceptor = TlsAcceptor::from(std::sync::Arc::new(config));
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("fixture connect");
            let _ = acceptor.accept(socket).await;
        });
        let mut cfg = initial();
        cfg.mode = Mode::Direct;
        let client = network::client(&cfg).expect("route");
        let response = client
            .get(format!("https://localhost:{}/", address.port()))
            .timeout(Duration::from_secs(2))
            .send()
            .await;
        task.abort();
        let _ = task.await;
        assert!(response.is_err(), "Untrusted TLS must never succeed.");
    })
    .await
    .expect("TLS case deadline");
}
