//! Direct TLS termination for `ferryx-relay`.
//!
//! The relay can terminate HTTPS itself (no reverse proxy, no tunnel) either with an
//! operator-supplied certificate or with certificates obtained and renewed automatically from
//! Let's Encrypt over TLS-ALPN-01, which needs only the HTTPS port.
//!
//! Two properties the rest of the relay depends on are preserved here:
//! - Every request still carries `ConnectInfo<SocketAddr>` with the real TCP peer, because rate
//!   limiting keys on it and never trusts forwarding headers (see `relay_server::peer_ip`).
//! - Only the TLS handshake is bounded by a timeout. Established connections, including the
//!   long-lived control, data and browser WebSockets, get no extra idle timeout.

use axum::serve::{Listener, ListenerExt};
use axum::Router;
use futures_util::StreamExt;
use rustls::crypto::CryptoProvider;
use rustls::ServerConfig;
use rustls_acme::caches::DirCache;
use rustls_acme::{AcmeConfig, EventOk};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_rustls::server::TlsStream;
use tokio_rustls::LazyConfigAcceptor;

/// Upper bound for a client to finish the TLS handshake. It does not apply after the
/// handshake, so it cannot cut an idle WebSocket.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Completed handshakes waiting for the HTTP server to pick them up.
const ACCEPTED_BACKLOG: usize = 128;
/// Handshakes allowed in flight at once. A peer that opens connections and never sends a
/// ClientHello holds one slot for at most [`HANDSHAKE_TIMEOUT`]; past this bound the accept
/// loop waits for a slot instead of spawning until file descriptors run out.
const MAX_IN_FLIGHT_HANDSHAKES: usize = 512;
/// The relay serves HTTP/1.1 only (axum is built without http2).
const HTTP11_ALPN: &[u8] = b"http/1.1";

fn crypto_provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// How accepted connections pick their `ServerConfig`.
#[derive(Clone)]
pub enum TlsAcceptorMode {
    /// A fixed certificate chain loaded at startup.
    Static(Arc<ServerConfig>),
    /// Certificates managed by ACME; TLS-ALPN-01 validation connections get `challenge`.
    Acme {
        default: Arc<ServerConfig>,
        challenge: Arc<ServerConfig>,
    },
}

/// Settings for automatic Let's Encrypt certificates.
pub struct AcmeSettings {
    pub domains: Vec<String>,
    pub contact: Option<String>,
    pub cache_dir: PathBuf,
    pub staging: bool,
}

/// Loads a PEM certificate chain and private key into a server config.
/// Returns an operator-facing message on failure.
pub fn load_static_server_config(
    cert_path: &Path,
    key_path: &Path,
) -> Result<Arc<ServerConfig>, String> {
    let certs = CertificateDer::pem_file_iter(cert_path)
        .map_err(|err| format!("cannot read TLS certificate {}: {err}", cert_path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("invalid PEM in TLS certificate {}: {err}", cert_path.display()))?;
    if certs.is_empty() {
        return Err(format!(
            "TLS certificate {} contains no CERTIFICATE block",
            cert_path.display()
        ));
    }
    let key = PrivateKeyDer::from_pem_file(key_path)
        .map_err(|err| format!("cannot load TLS private key {}: {err}", key_path.display()))?;
    let mut config = ServerConfig::builder_with_provider(crypto_provider())
        .with_safe_default_protocol_versions()
        .map_err(|err| format!("TLS configuration error: {err}"))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|err| {
            format!(
                "TLS certificate {} and key {} are not a usable pair: {err}",
                cert_path.display(),
                key_path.display()
            )
        })?;
    config.alpn_protocols = vec![HTTP11_ALPN.to_vec()];
    Ok(Arc::new(config))
}

/// Creates the ACME cache directory and proves it is writable, so a bad path fails at
/// startup instead of after a certificate was issued (and Let's Encrypt quota spent).
/// Blocking: call from a blocking thread.
pub fn prepare_cache_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|err| format!("cannot create TLS cache directory {}: {err}", dir.display()))?;
    let probe = dir.join(".ferryx-write-probe");
    std::fs::write(&probe, b"")
        .map_err(|err| format!("TLS cache directory {} is not writable: {err}", dir.display()))?;
    let _ = std::fs::remove_file(&probe);
    // The cache holds the ACME account key and certificate private keys.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(err) = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)) {
            eprintln!(
                "ferryx-relay: tls: warning: cannot restrict {} to mode 0700: {err}",
                dir.display()
            );
        }
    }
    Ok(())
}

/// Starts certificate management for `settings` and returns the acceptor mode that serves
/// the managed certificates. Acquisition and renewal run on a spawned task that logs every
/// event to stderr. Must be called inside a Tokio runtime.
pub fn start_acme(settings: AcmeSettings) -> Result<TlsAcceptorMode, String> {
    let provider = crypto_provider();
    let directory = if settings.staging {
        "Let's Encrypt staging"
    } else {
        "Let's Encrypt production"
    };
    let label = settings.domains.join(", ");
    let mut state = AcmeConfig::new_with_provider(settings.domains.iter(), provider.clone())
        .contact(settings.contact.iter().map(|contact| mailto(contact)))
        .directory_lets_encrypt(!settings.staging)
        .cache(DirCache::new(settings.cache_dir.clone()))
        .state();
    let challenge = state.challenge_rustls_config_with_provider(provider.clone());
    let mut default = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|err| format!("TLS configuration error: {err}"))?
        .with_no_client_auth()
        .with_cert_resolver(state.resolver());
    default.alpn_protocols = vec![HTTP11_ALPN.to_vec()];

    eprintln!(
        "ferryx-relay: tls: managing certificate for {label} via {directory} (TLS-ALPN-01), cache {}",
        settings.cache_dir.display()
    );
    tokio::spawn(async move {
        while let Some(event) = state.next().await {
            match event {
                Ok(ok) => eprintln!("ferryx-relay: tls: {label}: {}", describe_event(&ok)),
                Err(err) => eprintln!("ferryx-relay: tls: {label}: certificate error: {err}"),
            }
        }
    });

    Ok(TlsAcceptorMode::Acme {
        default: Arc::new(default),
        challenge,
    })
}

fn mailto(contact: &str) -> String {
    if contact.starts_with("mailto:") {
        contact.to_string()
    } else {
        format!("mailto:{contact}")
    }
}

fn describe_event(event: &EventOk) -> &'static str {
    match event {
        EventOk::DeployedCachedCert => "deployed certificate from cache",
        EventOk::DeployedNewCert => "deployed newly issued certificate",
        EventOk::CertCacheStore => "stored certificate in cache",
        EventOk::AccountCacheStore => "stored ACME account in cache",
    }
}

/// Serves `router` over TLS on `tcp`, exactly like the plain-HTTP relay but encrypted.
pub async fn serve(tcp: TcpListener, mode: TlsAcceptorMode, router: Router) -> io::Result<()> {
    // axum implements `Connected` for `SocketAddr` on any `TapIo<L>` whose `L::Addr` is a
    // `SocketAddr`, so the no-op tap is what hands each request the TCP peer address.
    let listener = TlsListener::new(tcp, mode)?.tap_io(|_| {});
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
}

/// An axum listener yielding TLS streams paired with the remote TCP address.
///
/// Handshakes run on their own tasks so a slow or silent client cannot hold up accepting
/// others.
pub struct TlsListener {
    accepted: mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    local_addr: SocketAddr,
    accept_task: JoinHandle<()>,
}

impl TlsListener {
    pub fn new(tcp: TcpListener, mode: TlsAcceptorMode) -> io::Result<Self> {
        let local_addr = tcp.local_addr()?;
        let (tx, accepted) = mpsc::channel(ACCEPTED_BACKLOG);
        let handshakes = Arc::new(tokio::sync::Semaphore::new(MAX_IN_FLIGHT_HANDSHAKES));
        let accept_task = tokio::spawn(accept_loop(tcp, mode, tx, handshakes));
        Ok(Self {
            accepted,
            local_addr,
            accept_task,
        })
    }
}

impl Drop for TlsListener {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.accepted.recv().await {
            Some(connection) => connection,
            // The accept loop only stops when this listener is dropped.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local_addr)
    }
}

async fn accept_loop(
    tcp: TcpListener,
    mode: TlsAcceptorMode,
    tx: mpsc::Sender<(TlsStream<TcpStream>, SocketAddr)>,
    handshakes: Arc<tokio::sync::Semaphore>,
) {
    loop {
        // Taken before accepting, so connections past the bound wait in the kernel backlog
        // rather than each holding a task and a descriptor here.
        let Ok(permit) = handshakes.clone().acquire_owned().await else {
            return;
        };
        let (stream, peer) = match tcp.accept().await {
            Ok(accepted) => accepted,
            Err(err) => {
                if !is_connection_error(&err) {
                    // Same policy as axum's own accept loop (e.g. EMFILE): log and back off.
                    eprintln!("ferryx-relay: accept error: {err}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                continue;
            }
        };
        let tx = tx.clone();
        let mode = mode.clone();
        tokio::spawn(async move {
            let outcome = tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake(stream, &mode)).await;
            // The slot covers the handshake only, not the wait for the HTTP server.
            drop(permit);
            match outcome {
                Ok(Ok(Some(tls))) => {
                    let _ = tx.send((tls, peer)).await;
                }
                Ok(Ok(None)) => {}
                Ok(Err(err)) => tracing::debug!(%peer, "TLS handshake failed: {err}"),
                Err(_) => tracing::debug!(%peer, "TLS handshake timed out"),
            }
        });
    }
}

/// Completes the handshake. Returns `None` for ACME validation connections, which are
/// answered here and never reach the HTTP server.
async fn handshake(
    stream: TcpStream,
    mode: &TlsAcceptorMode,
) -> io::Result<Option<TlsStream<TcpStream>>> {
    let start = LazyConfigAcceptor::new(rustls::server::Acceptor::default(), stream).await?;
    let config = match mode {
        TlsAcceptorMode::Static(config) => config.clone(),
        TlsAcceptorMode::Acme { default, challenge } => {
            if rustls_acme::is_tls_alpn_challenge(&start.client_hello()) {
                let mut tls = start.into_stream(challenge.clone()).await?;
                eprintln!("ferryx-relay: tls: answered TLS-ALPN-01 validation request");
                let _ = tls.shutdown().await;
                return Ok(None);
            }
            default.clone()
        }
    };
    start.into_stream(config).await.map(Some)
}

fn is_connection_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::relay_server::{relay_router, RelayState};
    use axum::extract::ConnectInfo;
    use axum::routing::get;
    use base64::Engine as _;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::TlsConnector;
    use tokio_tungstenite::tungstenite::Message;

    const TEST_TIMEOUT: Duration = Duration::from_secs(20);

    struct TestCert {
        _dir: tempfile::TempDir,
        cert_path: PathBuf,
        key_path: PathBuf,
        der: CertificateDer<'static>,
    }

    fn pem(label: &str, der: &[u8]) -> String {
        let body = base64::engine::general_purpose::STANDARD.encode(der);
        let mut out = format!("-----BEGIN {label}-----\n");
        for line in body.as_bytes().chunks(64) {
            out.push_str(std::str::from_utf8(line).unwrap());
            out.push('\n');
        }
        out.push_str(&format!("-----END {label}-----\n"));
        out
    }

    fn self_signed() -> TestCert {
        let generated = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");
        std::fs::write(&cert_path, pem("CERTIFICATE", generated.cert.der())).unwrap();
        std::fs::write(
            &key_path,
            pem("PRIVATE KEY", &generated.signing_key.serialize_der()),
        )
        .unwrap();
        TestCert {
            _dir: dir,
            cert_path,
            key_path,
            der: generated.cert.der().clone(),
        }
    }

    fn client_config(cert: &TestCert) -> Arc<rustls::ClientConfig> {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert.der.clone()).unwrap();
        Arc::new(
            rustls::ClientConfig::builder_with_provider(crypto_provider())
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    }

    /// Serves `router` through the production TLS path on an ephemeral loopback port.
    async fn spawn_tls(cert: &TestCert, router: Router) -> (SocketAddr, JoinHandle<()>) {
        let config = load_static_server_config(&cert.cert_path, &cert.key_path).unwrap();
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            serve(tcp, TlsAcceptorMode::Static(config), router)
                .await
                .expect("TLS relay server exited");
        });
        (addr, handle)
    }

    /// One HTTP/1.1 GET over TLS; returns the client's local socket address and the raw response.
    async fn https_get(
        addr: SocketAddr,
        cert: &TestCert,
        path: &str,
    ) -> (SocketAddr, String) {
        let tcp = TcpStream::connect(addr).await.unwrap();
        let local = tcp.local_addr().unwrap();
        let name = rustls_pki_types::ServerName::try_from("localhost").unwrap();
        let mut tls = TlsConnector::from(client_config(cert))
            .connect(name, tcp)
            .await
            .unwrap();
        tls.write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
        let mut response = Vec::new();
        tls.read_to_end(&mut response).await.unwrap();
        (local, String::from_utf8_lossy(&response).into_owned())
    }

    fn test_relay_router() -> (tempfile::TempDir, Router) {
        let dir = tempfile::tempdir().unwrap();
        let state = RelayState::new_with_key_store(
            vec!["test-machine-token".to_string()],
            dir.path().join("machine_keys.json"),
        )
        .unwrap();
        (dir, relay_router(state))
    }

    #[tokio::test]
    async fn relay_router_serves_https_with_static_certificate() {
        let cert = self_signed();
        let (_keys, router) = test_relay_router();
        let (addr, server) = spawn_tls(&cert, router).await;

        let (_, response) = tokio::time::timeout(TEST_TIMEOUT, https_get(addr, &cert, "/install.sh"))
            .await
            .unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("text/x-shellscript"), "{response}");
        server.abort();
    }

    #[tokio::test]
    async fn relay_control_websocket_upgrades_over_tls() {
        let cert = self_signed();
        let (_keys, router) = test_relay_router();
        let (addr, server) = spawn_tls(&cert, router).await;

        let tcp = TcpStream::connect(addr).await.unwrap();
        let (mut socket, response) = tokio::time::timeout(
            TEST_TIMEOUT,
            tokio_tungstenite::client_async_tls_with_config(
                format!("wss://localhost:{}/tunnel/control", addr.port()),
                tcp,
                None,
                Some(tokio_tungstenite::Connector::Rustls(client_config(&cert))),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::SWITCHING_PROTOCOLS);
        // The relay opens the control channel with a signed-challenge nonce.
        let first = tokio::time::timeout(TEST_TIMEOUT, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        match first {
            Message::Text(text) => assert!(text.contains("\"nonce\""), "{text}"),
            other => panic!("expected the control challenge, got {other:?}"),
        }
        server.abort();
    }

    #[tokio::test]
    async fn tls_requests_carry_the_real_tcp_peer_as_connect_info() {
        let cert = self_signed();
        let router = Router::new().route(
            "/peer",
            get(|ConnectInfo(peer): ConnectInfo<SocketAddr>| async move { peer.to_string() }),
        );
        let (addr, server) = spawn_tls(&cert, router).await;

        let (client, response) = tokio::time::timeout(TEST_TIMEOUT, https_get(addr, &cert, "/peer"))
            .await
            .unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let body = response.split("\r\n\r\n").nth(1).unwrap_or_default();
        // The exact client socket: loopback IP (never UNSPECIFIED) and the client's own port.
        assert_eq!(body, client.to_string(), "{response}");
        assert!(client.ip().is_loopback());
        server.abort();
    }

    #[tokio::test]
    async fn stalled_handshake_does_not_block_other_clients() {
        let cert = self_signed();
        let (_keys, router) = test_relay_router();
        let (addr, server) = spawn_tls(&cert, router).await;

        // Connects but never sends a ClientHello.
        let _silent = TcpStream::connect(addr).await.unwrap();
        let (_, response) = tokio::time::timeout(TEST_TIMEOUT, https_get(addr, &cert, "/install.sh"))
            .await
            .unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        server.abort();
    }

    #[tokio::test]
    async fn handshakes_wait_for_a_free_slot_instead_of_spawning_unbounded() {
        let cert = self_signed();
        let config = load_static_server_config(&cert.cert_path, &cert.key_path).unwrap();
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        // Every slot is taken (as by stalled peers): no new handshake may start.
        let handshakes = Arc::new(tokio::sync::Semaphore::new(0));
        let (tx, mut accepted) = mpsc::channel(ACCEPTED_BACKLOG);
        let accept_task = tokio::spawn(accept_loop(
            tcp,
            TlsAcceptorMode::Static(config),
            tx,
            handshakes.clone(),
        ));

        let client = async {
            let tcp = TcpStream::connect(addr).await.unwrap();
            let name = rustls_pki_types::ServerName::try_from("localhost").unwrap();
            TlsConnector::from(client_config(&cert))
                .connect(name, tcp)
                .await
                .unwrap()
        };
        let mut client = std::pin::pin!(client);
        // With no slot the handshake cannot progress however long it waits; the paused
        // clock only makes that wait instant.
        tokio::time::pause();
        assert!(
            tokio::time::timeout(HANDSHAKE_TIMEOUT * 3, &mut client)
                .await
                .is_err(),
            "a handshake ran without a free slot"
        );
        tokio::time::resume();

        handshakes.add_permits(1);
        let _tls = tokio::time::timeout(TEST_TIMEOUT, &mut client)
            .await
            .expect("a freed slot must let the waiting handshake finish");
        tokio::time::timeout(TEST_TIMEOUT, accepted.recv())
            .await
            .unwrap()
            .expect("the completed handshake reaches the HTTP server");
        // The single slot came back once that handshake finished: a second one completes.
        let tcp = TcpStream::connect(addr).await.unwrap();
        let name = rustls_pki_types::ServerName::try_from("localhost").unwrap();
        tokio::time::timeout(
            TEST_TIMEOUT,
            TlsConnector::from(client_config(&cert)).connect(name, tcp),
        )
        .await
        .expect("the slot must be released after a handshake")
        .unwrap();
        accept_task.abort();
    }

    #[test]
    fn static_config_errors_name_the_offending_file() {
        let cert = self_signed();
        let missing = cert.cert_path.with_file_name("missing.pem");

        let err = load_static_server_config(&missing, &cert.key_path).err().unwrap();
        assert!(err.contains("missing.pem"), "{err}");

        let err = load_static_server_config(&cert.key_path, &cert.key_path).err().unwrap();
        assert!(err.contains("no CERTIFICATE block"), "{err}");

        let err = load_static_server_config(&cert.cert_path, &cert.cert_path).err().unwrap();
        assert!(err.contains("private key"), "{err}");

        let other = self_signed();
        let err = load_static_server_config(&cert.cert_path, &other.key_path).err().unwrap();
        assert!(err.contains("not a usable pair"), "{err}");
    }

    #[test]
    fn cache_dir_is_created_and_unwritable_paths_fail() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("relay-tls");
        prepare_cache_dir(&nested).unwrap();
        assert!(nested.is_dir());

        // A regular file where the directory should be cannot become a cache.
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        let err = prepare_cache_dir(&file.join("relay-tls")).err().unwrap();
        assert!(err.contains("TLS cache directory"), "{err}");
    }
}

