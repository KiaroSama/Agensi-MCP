use agensi_mcp::socks4_userid;
use std::{io, net::Ipv4Addr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

const LIMIT: Duration = Duration::from_secs(5);

fn assert_no_contacts(listener: TcpListener) {
    let listener = listener.into_std().unwrap();
    assert!(
        matches!(listener.accept(), Err(e) if e.kind() == io::ErrorKind::WouldBlock),
        "Rejected requests must not contact the selected upstream"
    );
}
struct OwnedTask<T>(JoinHandle<T>);
impl<T> Drop for OwnedTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn c_string(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut value = Vec::new();
    for _ in 0..256 {
        let byte = stream.read_u8().await?;
        if byte == 0 {
            return Ok(value);
        }
        value.push(byte);
    }
    Err(io::Error::other("Fixture string exceeds limit"))
}

#[tokio::test(flavor = "current_thread")]
async fn userid_and_dns_policy_reach_selected_socks4_proxy() {
    tokio::time::timeout(LIMIT, async {
        for (scheme, host, expected_ip, expected_domain) in [
            ("socks4", "127.0.0.1", [127, 0, 0, 1], None),
            (
                "socks4a",
                "proxy-only.invalid",
                [0, 0, 0, 1],
                Some("proxy-only.invalid"),
            ),
        ] {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
            let address = listener.local_addr().unwrap();
            let mut task = OwnedTask(tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut header = [0; 8];
                stream.read_exact(&mut header).await.unwrap();
                assert_eq!(header[..4], [4, 1, 31, 144]);
                assert_eq!(header[4..], expected_ip);
                assert_eq!(c_string(&mut stream).await.unwrap(), b"fixture user");
                if let Some(domain) = expected_domain {
                    assert_eq!(c_string(&mut stream).await.unwrap(), domain.as_bytes());
                }
                stream.write_all(&[0, 90, 0, 0, 0, 0, 0, 0]).await.unwrap();
                let mut headers = Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {
                    assert!(headers.len() < 16384);
                    headers.push(stream.read_u8().await.unwrap());
                }
                assert!(!String::from_utf8_lossy(&headers).contains("fixture user"));
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    )
                    .await
                    .unwrap();
            }));
            let profile =
                url::Url::parse(&format!("{scheme}://fixture%20user:ignored@{address}")).unwrap();
            let adapter = socks4_userid::start(&profile).unwrap();
            let client = reqwest::Client::builder()
                .no_proxy()
                .proxy(reqwest::Proxy::all(&adapter.proxy_url).unwrap())
                .build()
                .unwrap();
            let response = client
                .get(format!("http://{host}:8080/check"))
                .timeout(Duration::from_secs(2))
                .send()
                .await
                .unwrap();
            assert_eq!(response.text().await.unwrap(), "ok");
            (&mut task.0).await.unwrap();
            drop(client);
            drop(adapter);
        }
    })
    .await
    .expect("SOCKS4 userid test deadline");
}

#[tokio::test(flavor = "current_thread")]
async fn rejected_local_credentials_never_contact_upstream() {
    tokio::time::timeout(LIMIT, async {
        let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let profile = url::Url::parse(&format!(
            "socks4://fixture@{}",
            upstream.local_addr().unwrap()
        ))
        .unwrap();
        let adapter = socks4_userid::start(&profile).unwrap();
        let mut local = url::Url::parse(&adapter.proxy_url).unwrap();
        local.set_password(Some("wrong-fixture-password")).unwrap();
        let mut stream = TcpStream::connect((local.host_str().unwrap(), local.port().unwrap()))
            .await
            .unwrap();
        stream.write_all(&[5, 1, 2]).await.unwrap();
        let mut response = [0; 2];
        stream.read_exact(&mut response).await.unwrap();
        assert_eq!(response, [5, 2]);
        let user = local.username().as_bytes();
        let password = local.password().unwrap().as_bytes();
        let mut request = vec![1, user.len() as u8];
        request.extend_from_slice(user);
        request.push(password.len() as u8);
        request.extend_from_slice(password);
        stream.write_all(&request).await.unwrap();
        stream.read_exact(&mut response).await.unwrap();
        assert_eq!(response, [1, 0xff]);
        assert_no_contacts(upstream);
        drop(adapter);
    })
    .await
    .expect("Local authentication rejection deadline");
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_adapter_closes_listener_and_active_relay() {
    tokio::time::timeout(LIMIT, async {
        let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let profile = url::Url::parse(&format!(
            "socks4://fixture@{}",
            upstream.local_addr().unwrap()
        ))
        .unwrap();
        let adapter = socks4_userid::start(&profile).unwrap();
        let local = url::Url::parse(&adapter.proxy_url).unwrap();
        let local_address = (local.host_str().unwrap().to_owned(), local.port().unwrap());
        let mut downstream = TcpStream::connect((&*local_address.0, local_address.1))
            .await
            .unwrap();
        downstream.write_all(&[5, 1, 2]).await.unwrap();
        let mut response = [0; 2];
        downstream.read_exact(&mut response).await.unwrap();
        let user = local.username().as_bytes();
        let password = local.password().unwrap().as_bytes();
        let mut auth = vec![1, user.len() as u8];
        auth.extend_from_slice(user);
        auth.push(password.len() as u8);
        auth.extend_from_slice(password);
        downstream.write_all(&auth).await.unwrap();
        downstream.read_exact(&mut response).await.unwrap();
        assert_eq!(response, [1, 0]);
        downstream
            .write_all(&[5, 1, 0, 1, 127, 0, 0, 1, 31, 144])
            .await
            .unwrap();
        let (mut selected, _) = upstream.accept().await.unwrap();
        let mut header = [0; 8];
        selected.read_exact(&mut header).await.unwrap();
        assert_eq!(c_string(&mut selected).await.unwrap(), b"fixture");
        selected
            .write_all(&[0, 90, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        let mut connected = [0; 10];
        downstream.read_exact(&mut connected).await.unwrap();
        assert_eq!(connected[..2], [5, 0]);
        drop(adapter);
        let mut byte = [0];
        assert!(matches!(downstream.read(&mut byte).await, Ok(0) | Err(_)));
        assert!(matches!(selected.read(&mut byte).await, Ok(0) | Err(_)));
        assert!(
            TcpStream::connect((&*local_address.0, local_address.1))
                .await
                .is_err()
        );
    })
    .await
    .expect("Adapter cleanup deadline");
}
