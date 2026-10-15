use agensi_mcp::{
    config::{Mode, initial},
    network,
};
use std::{error::Error, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
    time::{Duration, timeout},
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{self, ServerConfig, pki_types::PrivatePkcs8KeyDer},
};

struct OwnedTask(JoinHandle<()>);
impl Drop for OwnedTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
fn certificate_rejected(error: &(dyn Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if matches!(
            error.downcast_ref::<rustls::Error>(),
            Some(rustls::Error::InvalidCertificate(_))
        ) {
            return true;
        }
        // io::Error::source forwards its payload's source, skipping the payload itself.
        // TLS connectors nest these wrappers; inspect the typed inner error before advancing.
        current = error
            .downcast_ref::<std::io::Error>()
            .and_then(|error| error.get_ref())
            .map(|error| error as &(dyn Error + 'static))
            .or_else(|| error.source());
    }
    false
}

#[test]
fn certificate_classification_follows_io_payloads_without_accepting_connection_errors() {
    let certificate = || rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer);
    assert!(certificate_rejected(&certificate()));
    let wrapped = std::io::Error::other(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        certificate(),
    ));
    assert!(certificate_rejected(&wrapped));
    for error in [
        std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
        std::io::Error::from(std::io::ErrorKind::TimedOut),
        std::io::Error::other(rustls::Error::General("Fixture setup failure".into())),
        std::io::Error::other("invalid peer certificate: arbitrary string is not typed evidence"),
    ] {
        assert!(!certificate_rejected(&error));
    }
}

#[tokio::test]
async fn untrusted_destination_certificate_is_rejected() {
    timeout(Duration::from_secs(5), async {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()])
                .expect("Fixture certificate");
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
            )
            .expect("Fixture TLS config");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Fixture port");
        let address = listener.local_addr().expect("Fixture address");
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let (attempted, handshake) = tokio::sync::oneshot::channel();
        let mut task = OwnedTask(tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("Negative TCP accepted");
            let result = acceptor.accept(socket).await;
            attempted
                .send(result.is_err())
                .expect("Handshake observation");
            let (socket, _) = listener.accept().await.expect("Positive TCP accepted");
            let mut tls = acceptor.accept(socket).await.expect("Trusted handshake");
            let mut input = Vec::new();
            while !input.ends_with(b"\r\n\r\n") {
                assert!(input.len() < 16384, "Fixture request bound");
                input.push(tls.read_u8().await.expect("Fixture HTTP request"));
            }
            tls.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\ntrusted-body!",
            )
            .await
            .expect("Fixture response");
            tls.shutdown().await.expect("Fixture TLS shutdown");
        }));
        let mut cfg = initial();
        cfg.mode = Mode::Direct;
        // An IP SAN and literal address prevent IPv4/IPv6 resolution from masking TLS evidence.
        let url = format!("https://{address}/");
        let client = network::client(&cfg).expect("Route");
        let error = client
            .get(&url)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect_err("Untrusted TLS must never succeed");
        assert!(
            certificate_rejected(&error),
            "Failure must be typed certificate validation, not connection setup"
        );
        assert!(
            timeout(Duration::from_secs(1), handshake)
                .await
                .expect("Handshake observation deadline")
                .expect("Handshake observation sender")
        );
        let trusted = reqwest::Client::builder()
            .no_proxy()
            .tls_certs_only([
                reqwest::Certificate::from_der(cert.der().as_ref()).expect("Fixture root")
            ])
            .build()
            .expect("Explicit fixture trust");
        let response = trusted
            .get(&url)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect("Trusted fixture request");
        assert!(response.status().is_success());
        assert_eq!(
            response.text().await.expect("Trusted body"),
            "trusted-body!"
        );
        timeout(Duration::from_secs(1), &mut task.0)
            .await
            .expect("Server join deadline")
            .expect("Server task");
    })
    .await
    .expect("TLS case deadline");
}
