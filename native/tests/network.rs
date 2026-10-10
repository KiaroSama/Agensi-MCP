use agensi_mcp::config::{self, Config, Mode, Profile};
use agensi_mcp::network;
use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

// Source mapping: routing.test.mjs dead proxy, HTTP/SOCKS profiles/clean close,
// and route state credential redaction. HTTP/SOCKS server fakes speak the real
// wire protocol and forward bytes to an independently listening HTTP origin.
// No external DNS/network, env mutation, sleeps, subprocesses, or user settings.
// Parent owns TLS, environment bypass, route-generation and bridge/OAuth coverage.
const LIMIT: Duration = Duration::from_secs(5);

struct OwnedTask<T>(JoinHandle<io::Result<T>>);
impl<T> Drop for OwnedTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl<T> OwnedTask<T> {
    async fn result(&mut self) -> T {
        (&mut self.0)
            .await
            .expect("owned loopback task join")
            .expect("loopback server I/O")
    }
}

async fn bounded(future: impl Future<Output = ()>) {
    tokio::time::timeout(LIMIT, future)
        .await
        .expect("network case must finish within five seconds");
}

fn assert_no_contacts(listener: TcpListener) {
    let listener = listener.into_std().expect("destination socket inspection");
    assert!(
        matches!(listener.accept(), Err(e) if e.kind() == io::ErrorKind::WouldBlock),
        "No destination TCP connection may bypass the selected policy"
    );
}

fn routed(scheme: &str, address: SocketAddr, auth: &str) -> Config {
    let mut config = config::initial();
    config.mode = Mode::Proxy;
    config.selected = Some("fixture".into());
    config.profiles.push(Profile::new(
        "fixture".into(),
        format!("{scheme}://{auth}{address}"),
    ));
    config
}

async fn headers(stream: &mut TcpStream) -> io::Result<String> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() >= 16384 {
            return Err(io::Error::other("HTTP fixture headers exceed limit"));
        }
        bytes.push(stream.read_u8().await?);
    }
    String::from_utf8(bytes).map_err(|_| io::Error::other("HTTP fixture headers are not UTF-8"))
}

async fn origin(body: &'static str) -> (SocketAddr, OwnedTask<String>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("origin bind");
    let address = listener.local_addr().expect("origin address");
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let request = headers(&mut stream).await?;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await?;
        Ok(request)
    });
    (address, OwnedTask(task))
}

async fn http_proxy(
    destination: SocketAddr,
    response: Option<String>,
) -> (SocketAddr, OwnedTask<String>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("HTTP proxy bind");
    let address = listener.local_addr().expect("proxy address");
    let task = tokio::spawn(async move {
        let (mut downstream, _) = listener.accept().await?;
        let request = headers(&mut downstream).await?;
        if let Some(response) = response {
            downstream.write_all(response.as_bytes()).await?;
            downstream.shutdown().await?;
            return Ok(request);
        }
        let mut upstream = TcpStream::connect(destination).await?;
        let first = request
            .lines()
            .next()
            .ok_or_else(|| io::Error::other("HTTP request line missing"))?;
        let mut parts = first.split_whitespace();
        let method = parts
            .next()
            .ok_or_else(|| io::Error::other("HTTP method missing"))?;
        let target = parts
            .next()
            .ok_or_else(|| io::Error::other("HTTP target missing"))?;
        let url = url::Url::parse(target)
            .map_err(|_| io::Error::other("Proxy target is not absolute URL"))?;
        let mut forwarded = format!("{method} {} HTTP/1.1\r\n", url.path());
        for line in request.lines().skip(1).filter(|line| !line.is_empty()) {
            if !line
                .to_ascii_lowercase()
                .starts_with("proxy-authorization:")
            {
                forwarded.push_str(line);
                forwarded.push_str("\r\n");
            }
        }
        forwarded.push_str("\r\n");
        upstream.write_all(forwarded.as_bytes()).await?;
        tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await?;
        Ok(request)
    });
    (address, OwnedTask(task))
}

#[derive(Debug)]
enum SocksTarget {
    Ip(IpAddr),
    Domain(String),
}
#[derive(Debug)]
struct SocksObservation {
    target: SocksTarget,
    port: u16,
    authenticated: bool,
}

async fn c_string(stream: &mut TcpStream) -> io::Result<String> {
    let mut bytes = Vec::new();
    for _ in 0..=255 {
        let byte = stream.read_u8().await?;
        if byte == 0 {
            return String::from_utf8(bytes)
                .map_err(|_| io::Error::other("SOCKS fixture string is not UTF-8"));
        }
        bytes.push(byte);
    }
    Err(io::Error::other("SOCKS fixture string exceeds limit"))
}

async fn socks4(stream: &mut TcpStream) -> io::Result<SocksObservation> {
    let mut request = [0; 8];
    stream.read_exact(&mut request).await?;
    if request[..2] != [4, 1] {
        return Err(io::Error::other("Expected SOCKS4 connect"));
    }
    let _user_id = c_string(stream).await?;
    let target = if request[4..7] == [0, 0, 0] && request[7] != 0 {
        SocksTarget::Domain(c_string(stream).await?)
    } else {
        SocksTarget::Ip(IpAddr::V4(Ipv4Addr::new(
            request[4], request[5], request[6], request[7],
        )))
    };
    Ok(SocksObservation {
        target,
        port: u16::from_be_bytes([request[2], request[3]]),
        authenticated: false,
    })
}

async fn sized_string(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let length = stream.read_u8().await? as usize;
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    Ok(bytes)
}

async fn socks5(stream: &mut TcpStream, authenticate: bool) -> io::Result<SocksObservation> {
    let version = stream.read_u8().await?;
    let methods = sized_string(stream).await?;
    if version != 5 || !methods.contains(&if authenticate { 2 } else { 0 }) {
        return Err(io::Error::other("Expected supported SOCKS5 greeting"));
    }
    stream
        .write_all(&[5, if authenticate { 2 } else { 0 }])
        .await?;
    if authenticate {
        let version = stream.read_u8().await?;
        let user = sized_string(stream).await?;
        let password = sized_string(stream).await?;
        // Deliberately fake RFC1929 test credentials; never print request bytes.
        if version != 1 || user != b"fixture-user" || password != b"fixture-pass" {
            return Err(io::Error::other("SOCKS5 fixture authentication mismatch"));
        }
        stream.write_all(&[1, 0]).await?;
    }
    let mut request = [0; 4];
    stream.read_exact(&mut request).await?;
    if request[..3] != [5, 1, 0] {
        return Err(io::Error::other("Expected SOCKS5 connect"));
    }
    let target = match request[3] {
        1 => {
            let mut ip = [0; 4];
            stream.read_exact(&mut ip).await?;
            SocksTarget::Ip(IpAddr::V4(Ipv4Addr::from(ip)))
        }
        3 => SocksTarget::Domain(
            String::from_utf8(sized_string(stream).await?)
                .map_err(|_| io::Error::other("SOCKS5 domain is not UTF-8"))?,
        ),
        4 => {
            let mut ip = [0; 16];
            stream.read_exact(&mut ip).await?;
            SocksTarget::Ip(IpAddr::V6(Ipv6Addr::from(ip)))
        }
        _ => return Err(io::Error::other("Unexpected SOCKS5 address type")),
    };
    let port = stream.read_u16().await?;
    Ok(SocksObservation {
        target,
        port,
        authenticated: authenticate,
    })
}

async fn socks_proxy(
    version: u8,
    destination: SocketAddr,
    authenticate: bool,
) -> (SocketAddr, OwnedTask<SocksObservation>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("SOCKS proxy bind");
    let address = listener.local_addr().expect("SOCKS proxy address");
    let task = tokio::spawn(async move {
        let (mut downstream, _) = listener.accept().await?;
        let observed = if version == 4 {
            socks4(&mut downstream).await?
        } else {
            socks5(&mut downstream, authenticate).await?
        };
        if observed.port != destination.port() {
            return Err(io::Error::other("Unexpected SOCKS destination port"));
        }
        // Proxy-side DNS maps the fixture-only .invalid name to the independent origin.
        // Local-DNS schemes must instead present a loopback IP, asserted by the caller.
        let mut upstream = TcpStream::connect(destination).await?;
        if version == 4 {
            downstream.write_all(&[0, 90, 0, 0, 0, 0, 0, 0]).await?;
        } else {
            downstream
                .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                .await?;
        }
        tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await?;
        Ok(observed)
    });
    (address, OwnedTask(task))
}

#[tokio::test(flavor = "current_thread")]
async fn disconnected_and_dead_selected_proxy_never_contact_origin() {
    bounded(async {
        assert!(network::client(&config::initial()).is_err());
        let destination = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("destination bind");
        let address = destination.local_addr().expect("destination address");
        let reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("closed proxy reservation");
        let dead = reservation.local_addr().expect("dead proxy address");
        drop(reservation);
        let client = network::client(&routed("http", dead, "fixture-user:fixture-pass@"))
            .expect("selected proxy client");
        let error = client
            .get(format!("http://{address}/must-not-contact"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect_err("dead proxy must fail");
        assert!(!format!("{error:?}").contains("fixture-pass"));
        assert_no_contacts(destination);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn http_proxy_authentication_reaches_proxy_only_and_status_is_redacted() {
    bounded(async {
        let (destination, mut origin_task) = origin("through HTTP proxy").await;
        let (proxy, mut proxy_task) = http_proxy(destination, None).await;
        let config = routed("http", proxy, "fixture-user:fixture-pass@");
        assert!(
            !config::public_status(&config)
                .to_string()
                .contains("fixture-pass")
        );
        let client = network::client(&config).expect("HTTP selected proxy client");
        let response = client
            .get(format!("http://{destination}/observed"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect("proxied request");
        assert_eq!(
            response.text().await.expect("response body"),
            "through HTTP proxy"
        );
        drop(client);
        let proxy_request = proxy_task.result().await;
        assert!(
            proxy_request.starts_with(&format!("GET http://{destination}/observed HTTP/1.1\r\n"))
        );
        assert!(
            proxy_request
                .to_ascii_lowercase()
                .contains("proxy-authorization: basic ")
        );
        let origin_request = origin_task.result().await;
        assert!(origin_request.starts_with("GET /observed HTTP/1.1\r\n"));
        assert!(
            !origin_request
                .to_ascii_lowercase()
                .contains("proxy-authorization:")
        );
        assert!(!origin_request.contains("fixture-pass"));
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn socks_schemes_forward_real_http_and_preserve_dns_policy() {
    for (scheme, version, remote_dns) in [
        ("socks4", 4, true),
        ("socks4a", 4, true),
        ("socks5", 5, true),
        ("socks5h", 5, true),
    ] {
        bounded(async {
            let (destination, mut origin_task) = origin("through SOCKS proxy").await;
            let authenticate = version == 5;
            let (proxy, mut proxy_task) = socks_proxy(version, destination, authenticate).await;
            let auth = if authenticate {
                "fixture-user:fixture-pass@"
            } else {
                ""
            };
            let client =
                network::client(&routed(scheme, proxy, auth)).expect("SOCKS selected proxy client");
            let host = if remote_dns {
                "proxy-only.invalid"
            } else if version == 5 {
                "localhost"
            } else {
                "127.0.0.1"
            };
            let response = client
                .get(format!("http://{host}:{}/observed", destination.port()))
                .timeout(Duration::from_secs(2))
                .send()
                .await
                .expect("SOCKS proxied request");
            assert_eq!(
                response.text().await.expect("SOCKS response body"),
                "through SOCKS proxy",
                "{scheme}"
            );
            drop(client);
            let observed = proxy_task.result().await;
            assert_eq!(observed.port, destination.port());
            assert_eq!(observed.authenticated, authenticate);
            match (&observed.target, remote_dns) {
                (SocksTarget::Domain(domain), true) => {
                    assert_eq!(domain, "proxy-only.invalid", "{scheme}")
                }
                (SocksTarget::Ip(ip), false) => assert!(ip.is_loopback(), "{scheme}"),
                _ => panic!("{scheme}: local/proxy DNS boundary changed"),
            }
            let request = origin_task.result().await;
            assert!(request.starts_with("GET /observed HTTP/1.1\r\n"));
            assert!(!request.contains("fixture-pass"));
        })
        .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn redirects_never_contact_a_second_destination() {
    bounded(async {
        let second = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("second destination bind");
        let second_address = second.local_addr().expect("second destination address");
        let redirect = format!("HTTP/1.1 302 Found\r\nLocation: http://{second_address}/must-not-follow\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let (proxy, mut task) = http_proxy(second_address, Some(redirect)).await;
        let client = network::client(&routed("http", proxy, "")).expect("no redirect client");
        let response = client.get("http://original.invalid/resource")
            .timeout(Duration::from_secs(2)).send().await.expect("redirect response");
        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
        drop(response);
        drop(client);
        task.result().await;
        assert_no_contacts(second);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_direct_mode_is_the_only_nonproxy_client() {
    bounded(async {
        let (destination, mut task) = origin("explicit direct").await;
        let mut config = config::initial();
        config.mode = Mode::Direct;
        let client = network::client(&config).expect("explicit direct client");
        let response = client
            .get(format!("http://{destination}/direct"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect("direct request");
        assert_eq!(
            response.text().await.expect("direct body"),
            "explicit direct"
        );
        drop(client);
        assert!(task.result().await.starts_with("GET /direct HTTP/1.1\r\n"));
    })
    .await;
}
