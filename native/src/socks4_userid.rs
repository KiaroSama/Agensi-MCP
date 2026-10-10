use anyhow::{Result, bail};
use fast_socks5::{
    ReplyError, Socks5Command, server::Socks5ServerProtocol, util::target_addr::TargetAddr,
};
use percent_encoding::percent_decode_str;
use std::{net::Ipv4Addr, sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use tokio::{
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};
use tokio_socks::tcp::Socks4Stream;
use url::Url;

const SETUP_LIMIT: Duration = Duration::from_secs(10);
const MAX_SESSIONS: usize = 64;

pub struct Adapter {
    pub proxy_url: String,
    task: JoinHandle<()>,
}
impl Drop for Adapter {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Route {
    host: String,
    port: u16,
    userid: String,
    remote_dns: bool,
    local_user: String,
    local_password: String,
}

pub fn start(profile_url: &Url) -> Result<Adapter> {
    if !matches!(profile_url.scheme(), "socks4" | "socks4a") {
        bail!("SOCKS4 adapter requires a SOCKS4 profile.");
    }
    let userid = percent_decode_str(profile_url.username())
        .decode_utf8()
        .map_err(|_| anyhow::anyhow!("SOCKS4 user ID is not valid UTF-8."))?
        .into_owned();
    if userid.is_empty() || userid.len() > 255 || userid.contains('\0') {
        bail!("SOCKS4 user ID must contain 1 to 255 bytes without NUL.");
    }
    let host = match profile_url.host() {
        Some(url::Host::Domain(value)) => value.to_owned(),
        Some(url::Host::Ipv4(value)) => value.to_string(),
        Some(url::Host::Ipv6(value)) => value.to_string(),
        None => bail!("SOCKS4 proxy host is missing."),
    };
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|_| anyhow::anyhow!("SOCKS4 adapter requires an active Tokio runtime."))?;
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let listener = TcpListener::from_std(listener)?;
    let local_user = uuid::Uuid::new_v4().simple().to_string();
    let local_password = uuid::Uuid::new_v4().simple().to_string();
    // Preserve target DNS policy while reqwest retains end-to-end TLS and SNI.
    let local_scheme = if profile_url.scheme() == "socks4a" {
        "socks5h"
    } else {
        "socks5"
    };
    let proxy_url = format!("{local_scheme}://{local_user}:{local_password}@{address}");
    let route = Arc::new(Route {
        host,
        port: profile_url.port().unwrap_or(1080),
        userid,
        remote_dns: profile_url.scheme() == "socks4a",
        local_user,
        local_password,
    });
    let task = runtime.spawn(async move {
        let mut sessions = JoinSet::new();
        loop {
            tokio::select! {
                biased;
                result = sessions.join_next(), if !sessions.is_empty() => {
                    match result {
                        Some(Ok(Ok(()))) => {},
                        Some(Ok(Err(_))) => eprintln!("SOCKS4 relay session failed; no direct fallback."),
                        Some(Err(_)) => eprintln!("SOCKS4 relay session ended unexpectedly."),
                        None => {},
                    }
                }
                accepted = listener.accept(), if sessions.len() < MAX_SESSIONS => {
                    match accepted {
                        Ok((stream, peer)) if peer.ip().is_loopback() => {
                            let route = Arc::clone(&route);
                            sessions.spawn(async move { relay(stream, &route).await });
                        }
                        Ok(_) => {},
                        Err(_) => {
                            eprintln!("SOCKS4 relay listener failed; connection blocked.");
                            break;
                        }
                    }
                }
            }
        }
        // JoinSet also aborts every owned session when the listener task is dropped.
        sessions.shutdown().await;
    });
    Ok(Adapter { proxy_url, task })
}

async fn relay(stream: TcpStream, route: &Route) -> Result<()> {
    let (mut downstream, mut upstream) = tokio::time::timeout(SETUP_LIMIT, async {
        let (proto, _) = Socks5ServerProtocol::accept_password_auth(stream, |user, password| {
            bool::from(
                user.as_bytes().ct_eq(route.local_user.as_bytes())
                    & password.as_bytes().ct_eq(route.local_password.as_bytes()),
            )
        })
        .await?;
        let (proto, command, target) = proto.read_command().await?;
        if command != Socks5Command::TCPConnect {
            proto.reply_error(&ReplyError::CommandNotSupported).await?;
            bail!("SOCKS4 relay supports CONNECT only.");
        }
        let valid_target = match &target {
            TargetAddr::Ip(address) => address.is_ipv4(),
            TargetAddr::Domain(domain, _) => {
                route.remote_dns
                    && !domain.is_empty()
                    && domain.len() <= 255
                    && !domain.contains('\0')
                    && route.userid.len() + domain.len() <= 503
            }
        };
        if !valid_target {
            proto
                .reply_error(&ReplyError::AddressTypeNotSupported)
                .await?;
            bail!("SOCKS4 target is incompatible with selected DNS policy.");
        }
        // Only this selected upstream is contacted; no target/direct connector exists.
        let upstream_socket = TcpStream::connect((route.host.as_str(), route.port)).await;
        let upstream_socket = match upstream_socket {
            Ok(socket) => socket,
            Err(_) => {
                proto.reply_error(&ReplyError::ConnectionRefused).await?;
                bail!("Selected SOCKS4 proxy is unavailable.");
            }
        };
        let upstream = match target {
            TargetAddr::Ip(address) => {
                Socks4Stream::connect_with_userid_and_socket(
                    upstream_socket,
                    address,
                    &route.userid,
                )
                .await
            }
            TargetAddr::Domain(domain, port) => {
                Socks4Stream::connect_with_userid_and_socket(
                    upstream_socket,
                    (domain.as_str(), port),
                    &route.userid,
                )
                .await
            }
        };
        let upstream = match upstream {
            Ok(stream) => stream.into_inner(),
            Err(_) => {
                proto.reply_error(&ReplyError::GeneralFailure).await?;
                bail!("Selected SOCKS4 proxy rejected the connection.");
            }
        };
        let downstream = proto.reply_success(upstream.local_addr()?).await?;
        anyhow::Ok((downstream, upstream))
    })
    .await
    .map_err(|_| anyhow::anyhow!("SOCKS4 relay setup timed out."))??;
    tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await?;
    Ok(())
}
