//! Separate TLS proxy/origin listeners; CONNECT can reach only the owned loopback origin.
use crate::fixture::Certificates;
use anyhow::{Result, ensure};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
    task::{JoinHandle, JoinSet},
    time::{Duration, timeout},
};

pub struct TlsProxy {
    pub port: u16,
    pub connects: Arc<AtomicUsize>,
    pub proxy_tcp: Arc<AtomicUsize>,
    pub proxy_tls: Arc<AtomicUsize>,
    pub origin_tcp: Arc<AtomicUsize>,
    pub origin_tls: Arc<AtomicUsize>,
    shutdown: watch::Sender<bool>,
    task: Option<JoinHandle<Result<()>>>,
}
impl TlsProxy {
    pub async fn start(proxy: &Certificates, origin: &Certificates) -> Result<Self> {
        let destination = TcpListener::bind(("127.0.0.1", 0)).await?;
        let origin_address = destination.local_addr()?;
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let connects = Arc::new(AtomicUsize::new(0));
        let proxy_tcp = Arc::new(AtomicUsize::new(0));
        let proxy_tls = Arc::new(AtomicUsize::new(0));
        let (proxy_accepted, proxy_handshakes) = (proxy_tcp.clone(), proxy_tls.clone());
        let origin_tcp = Arc::new(AtomicUsize::new(0));
        let origin_tls = Arc::new(AtomicUsize::new(0));
        let (shutdown, mut stopped) = watch::channel(false);
        let (proxy_acceptor, origin_acceptor) = (proxy.acceptor.clone(), origin.acceptor.clone());
        let (requests, accepted, handshakes) =
            (connects.clone(), origin_tcp.clone(), origin_tls.clone());
        let task = tokio::spawn(async move {
            let mut peers = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = stopped.changed() => break,
                    _ = peers.join_next(), if !peers.is_empty() => {},
                    socket = listener.accept(), if peers.len() < 32 => {
                        let (socket, address) = socket?;
                        ensure!(address.ip().is_loopback(), "Non-loopback TLS proxy peer");
                        proxy_accepted.fetch_add(1, Ordering::SeqCst);
                        let (tls, count, handshakes) = (proxy_acceptor.clone(), requests.clone(), proxy_handshakes.clone());
                        peers.spawn(async move {
                            let _ = timeout(Duration::from_secs(8), async {
                                let mut tls = tls.accept(socket).await?;
                                handshakes.fetch_add(1, Ordering::SeqCst);
                                let mut header = Vec::new();
                                while !header.ends_with(b"\r\n\r\n") {
                                    ensure!(header.len() < 16384, "TLS proxy header bound");
                                    header.push(tls.read_u8().await?);
                                }
                                let line = header.split(|byte| *byte == b'\n').next().unwrap_or_default();
                                ensure!(line == b"CONNECT mcp.agensi.io:443 HTTP/1.1\r" || line == b"CONNECT auth.agensi-audit.test:443 HTTP/1.1\r", "CONNECT target outside synthetic allowlist");
                                count.fetch_add(1, Ordering::SeqCst);
                                let mut origin = TcpStream::connect(origin_address).await?;
                                tls.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;
                                tokio::io::copy_bidirectional(&mut tls, &mut origin).await?;
                                Ok::<(), anyhow::Error>(())
                            }).await;
                        });
                    }
                    socket = destination.accept(), if peers.len() < 32 => {
                        let (socket, address) = socket?;
                        ensure!(address.ip().is_loopback(), "Non-loopback TLS origin peer");
                        accepted.fetch_add(1, Ordering::SeqCst);
                        let (tls, count) = (origin_acceptor.clone(), handshakes.clone());
                        peers.spawn(async move {
                            let _ = timeout(Duration::from_secs(8), async {
                                let mut tls = tls.accept(socket).await?;
                                count.fetch_add(1, Ordering::SeqCst);
                                crate::fixture::serve_http(&mut tls).await
                            }).await;
                        });
                    }
                }
            }
            peers.abort_all();
            while peers.join_next().await.is_some() {}
            Ok(())
        });
        Ok(Self {
            port,
            connects,
            proxy_tcp,
            proxy_tls,
            origin_tcp,
            origin_tls,
            shutdown,
            task: Some(task),
        })
    }
    pub async fn close(&mut self) -> Result<()> {
        let _ = self.shutdown.send(true);
        if let Some(mut task) = self.task.take() {
            match timeout(Duration::from_secs(2), &mut task).await {
                Ok(result) => result??,
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    anyhow::bail!("TLS proxy shutdown deadline");
                }
            }
        }
        Ok(())
    }
}
impl Drop for TlsProxy {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
