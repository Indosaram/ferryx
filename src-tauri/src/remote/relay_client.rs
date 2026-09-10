//! Desktop daemon reverse tunnel client.
//!
//! Counterpart to [`crate::remote::relay_server`]: this is the piece that
//! runs *on the desktop daemon*. It holds a long-lived outbound WebSocket
//! connection to a `ferryx-relay` instance's `/tunnel/control` endpoint,
//! authenticated with the daemon's Machine Token. Whenever the relay
//! notifies it of an incoming session (a remote client wants to attach),
//! it opens a second WebSocket to `/tunnel/data/:session_id` and proxies
//! frames between that socket and the local loopback gateway that Ferryx
//! serves at the address supplied by the gateway runtime.
//!
//! The control connection is expected to stay up for the lifetime of the
//! daemon process; if it drops (relay restart, network blip, ...) the
//! client reconnects automatically with exponential backoff.

use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use parking_lot::RwLock;
use rand::Rng;
use tokio::sync::{mpsc, oneshot, Mutex};
use crate::remote::auth::{MachineIdentity, sign_challenge};
use crate::remote::protocol::{ControlChallenge, ControlAuth, ControlAuthResponse, RegisterPairingPin, RegisterPairingPinAck, PairingState, PairingPinClaimed};

type ControlSocket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;
const REGISTER_ACK_TIMEOUT: Duration = Duration::from_secs(5);

pub struct RegisterPairingPinRequest {
    pub registration: RegisterPairingPin,
    pub ack: oneshot::Sender<Result<RegisterPairingPinAck, String>>,
}

#[derive(Debug, Clone)]
pub struct PairingSessionInfo {
    pub pin: String,
    pub pairing_token: String,
    pub expires_at: u64,
}

#[derive(Clone)]
pub struct PairingCoordinator {
    state: Arc<RwLock<PairingState>>,
    active_pin: Arc<RwLock<Option<String>>>,
    active_token: Arc<RwLock<Option<String>>>,
    register_tx: mpsc::Sender<RegisterPairingPinRequest>,
    machine_id: String,
}

impl PairingCoordinator {
    pub fn new(machine_id: impl Into<String>, register_tx: mpsc::Sender<RegisterPairingPinRequest>) -> Self {
        Self { state: Arc::new(RwLock::new(PairingState::Created)), active_pin: Arc::new(RwLock::new(None)), active_token: Arc::new(RwLock::new(None)), register_tx, machine_id: machine_id.into() }
    }

    pub fn state(&self) -> PairingState { *self.state.read() }

    pub fn transition(&self, target: PairingState) -> Result<(), String> {
        let mut state = self.state.write();
        if !state.can_transition_to(&target) {
            return Err(format!("Invalid pairing transition: {state:?} -> {target:?}"));
        }
        *state = target;
        if matches!(target, PairingState::Consumed | PairingState::Expired | PairingState::Cancelled) {
            *self.active_pin.write() = None;
            *self.active_token.write() = None;
        }
        Ok(())
    }

    /// `timeout` is the PIN lifetime; registration itself has a five-second deadline.
    pub async fn generate_pairing(&self, timeout: Duration) -> Result<PairingSessionInfo, String> {
        let expires_at = SystemTime::now().checked_add(timeout)
            .ok_or("Invalid pairing lifetime")?.duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?.as_secs();
        self.transition(PairingState::Registering)?;
        let pin = format!("{:06}", rand::rngs::OsRng.gen_range(0..1_000_000u32));
        let pairing_token = format!("{:032x}", rand::rngs::OsRng.gen::<u128>());
        *self.active_pin.write() = Some(pin.clone());
        *self.active_token.write() = Some(pairing_token.clone());
        let registration = RegisterPairingPin { pin: pin.clone(), pairing_token: pairing_token.clone(), machine_id: self.machine_id.clone(), expires_at };
        let (ack, rx) = oneshot::channel();
        let result = tokio::time::timeout(REGISTER_ACK_TIMEOUT.min(timeout), async {
            self.register_tx.send(RegisterPairingPinRequest { registration, ack }).await.map_err(|_| "Relay registration channel closed".to_string())?;
            let ack = rx.await.map_err(|_| "Relay disconnected before registration ACK".to_string())??;
            if ack.pin != pin || ack.machine_id != self.machine_id || ack.status != "ready" {
                return Err(format!("Relay rejected pairing registration: {}", ack.status));
            }
            Ok(())
        }).await;
        match result {
            Ok(Ok(())) => self.transition(PairingState::Ready)?,
            Ok(Err(error)) => { self.transition(PairingState::Cancelled)?; return Err(error); }
            Err(_) => { self.transition(PairingState::Expired)?; return Err("Timed out waiting for relay registration ACK".into()); }
        }
        let coordinator = self.clone();
        tokio::spawn(async move {
            let deadline = UNIX_EPOCH + Duration::from_secs(expires_at);
            tokio::time::sleep(deadline.duration_since(SystemTime::now()).unwrap_or_default()).await;
            if let Err(error) = coordinator.transition(PairingState::Expired) {
                tracing::debug!("Pairing expiry ignored: {error}");
            }
        });
        Ok(PairingSessionInfo { pin, pairing_token, expires_at })
    }
}
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::Message;

/// The local gateway that Ferryx's remote server listens on. Data channel
/// traffic is proxied to/from this address.
const LOCAL_GATEWAY_ADDR: &str = "127.0.0.1:43821";

/// Initial delay before the first reconnect attempt after a dropped or
/// failed control connection.
const INITIAL_BACKOFF: Duration = Duration::from_millis(500);

/// Ceiling on the reconnect backoff delay.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Notification pushed down the control channel when a client asks to open
/// a new session against this daemon. Mirrors
/// [`crate::remote::relay_server::IncomingSessionNotice`] - kept as a
/// separate type here (rather than a shared import) so this module only
/// depends on the wire shape, not the server's internal types.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SessionRequest {
    session_id: String,
}

/// Reverse tunnel client run by the desktop daemon.
///
/// Construct with [`RelayClient::new`] and drive with [`RelayClient::run`],
/// typically spawned onto its own long-lived task. `run` never returns
/// under normal operation - it reconnects on any control-channel failure
/// and only exits if the task itself is dropped/aborted.
#[derive(Clone)]
pub struct RelayClient {
    /// Base HTTP(S) URL of the relay, e.g. `https://relay.example.com`.
    /// Scheme is upgraded to `ws`/`wss` when building tunnel URLs.
    relay_url: String,
    /// This daemon's Machine Token, presented as a bearer credential on
    /// the control channel.
    machine_token: String,
    /// Local loopback gateway address that data channels are proxied to.
    gateway_addr: String,
    identity: Option<MachineIdentity>,
    pairing: PairingCoordinator,
    register_rx: Arc<Mutex<mpsc::Receiver<RegisterPairingPinRequest>>>,
}

impl RelayClient {
    /// Creates a client targeting `relay_url` (e.g.
    /// `https://relay.example.com` or `wss://relay.example.com`) using
    /// `machine_token` for authentication. Data channels are proxied to
    /// the local gateway on [`LOCAL_GATEWAY_ADDR`].
    pub fn new(relay_url: impl Into<String>, machine_token: impl Into<String>) -> Self {
        Self::with_gateway(relay_url, machine_token, LOCAL_GATEWAY_ADDR)
    }

    /// Creates a client proxying to the gateway's actual bound address,
    /// including when the gateway requests an OS-assigned port.
    pub fn with_gateway(
        relay_url: impl Into<String>,
        machine_token: impl Into<String>,
        gateway_addr: impl Into<String>,
    ) -> Self {
        let machine_token = machine_token.into();
        let (tx, rx) = mpsc::channel(1);
        Self {
            relay_url: relay_url.into(),
            pairing: PairingCoordinator::new(machine_token.clone(), tx),
            machine_token,
            gateway_addr: gateway_addr.into(),
            identity: None,
            register_rx: Arc::new(Mutex::new(rx)),
        }
    }

    pub fn with_identity(relay_url: impl Into<String>, identity: MachineIdentity, gateway_addr: impl Into<String>) -> Self {
        let mut client = Self::with_gateway(relay_url, "", gateway_addr);
        client.pairing.machine_id = identity.machine_id.clone();
        client.identity = Some(identity);
        client
    }

    pub fn pairing_coordinator(&self) -> PairingCoordinator { self.pairing.clone() }

    pub async fn connect_control(&self) -> anyhow::Result<ControlSocket> {
        let mut request = self.control_url().into_client_request()?;
        if self.identity.is_none() {
            request.headers_mut().insert(AUTHORIZATION, format!("Bearer {}", self.machine_token).parse()?);
        }
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await?;
        if let Some(identity) = &self.identity {
            tokio::time::timeout(REGISTER_ACK_TIMEOUT, async {
                let challenge: ControlChallenge = read_control_json(&mut socket).await?;
                let auth = ControlAuth {
                    machine_id: identity.machine_id.clone(), display_name: identity.display_name.clone(), public_key: identity.public_key.clone(),
                    signature: sign_challenge(identity, &challenge.nonce, challenge.timestamp).map_err(anyhow::Error::msg)?, timestamp: challenge.timestamp,
                };
                socket.send(Message::Text(serde_json::to_string(&auth)?.into())).await?;
                let response: ControlAuthResponse = read_control_json(&mut socket).await?;
                anyhow::ensure!(response.success, "Relay authentication rejected: {}", response.error.unwrap_or_default());
                Ok::<_, anyhow::Error>(())
            }).await??;
        }
        Ok(socket)
    }

    fn control_url(&self) -> String {
        format!("{}/tunnel/control", to_ws_base(&self.relay_url))
    }

    fn data_url(&self, session_id: &str) -> String {
        format!(
            "{}/tunnel/data/{}",
            to_ws_base(&self.relay_url),
            session_id
        )
    }

    /// Runs the reverse tunnel client forever: connects the control
    /// channel, handles incoming session notifications until the
    /// connection drops, then reconnects with exponential backoff and
    /// repeats. Intended to be spawned onto a dedicated task.
    pub async fn run(&self) {
        let mut backoff = INITIAL_BACKOFF;
        loop {
            match self.run_control_session().await {
                Ok(()) => {
                    // Clean shutdown (control socket closed normally).
                    // Still reconnect - the daemon is expected to stay
                    // tunneled for as long as it runs - but reset backoff
                    // since the prior session was healthy.
                    tracing::info!("relay control channel closed; reconnecting");
                    backoff = INITIAL_BACKOFF;
                }
                Err(err) => {
                    tracing::warn!(
                        "relay control channel error: {err}; retrying in {:.1}s",
                        backoff.as_secs_f64()
                    );
                }
            }

            tokio::time::sleep(backoff).await;
            backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
        }
    }

    /// Connects the control channel and services it until it closes or
    /// errors. Each incoming [`SessionRequest`] spawns an independent task
    /// that opens a data channel and proxies it against the local gateway,
    /// so a slow or stuck session cannot block subsequent notifications.
    async fn run_control_session(&self) -> anyhow::Result<()> {
        let socket = self.connect_control().await?;
        let mut registrations = self.register_rx.lock().await;
        let mut pending: Option<RegisterPairingPinRequest> = None;
        tracing::info!("relay control channel connected");
        let (mut write, mut read) = socket.split();
        // Dropping the control future also aborts its data channels.
        let mut sessions = tokio::task::JoinSet::new();

        loop {
            let message = tokio::select! {
                registration = registrations.recv(), if pending.is_none() => {
                    if let Some(registration) = registration {
                        if registration.ack.is_closed() { continue; }
                        write.send(Message::Text(serde_json::to_string(&registration.registration)?.into())).await?;
                        pending = Some(registration);
                    }
                    continue;
                }
                message = read.next() => message,
                result = sessions.join_next(), if !sessions.is_empty() => {
                    if let Some(Err(err)) = result {
                        tracing::warn!("relay data channel task failed: {err}");
                    }
                    continue;
                }
            };
            match message {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(ack) = serde_json::from_str::<RegisterPairingPinAck>(&text) {
                        if pending.as_ref().is_some_and(|request| request.registration.pin == ack.pin && request.registration.machine_id == ack.machine_id) {
                            if pending.take().unwrap().ack.send(Ok(ack)).is_err() {
                                tracing::debug!("Registration ACK arrived after caller disconnected");
                            }
                        }
                        continue;
                    }
                    if let Ok(claim) = serde_json::from_str::<PairingPinClaimed>(&text) {
                        let matches = claim.machine_id == self.pairing.machine_id && self.pairing.active_pin.read().as_ref() == Some(&claim.pin);
                        if matches {
                            if let Err(error) = self.pairing.transition(PairingState::Claimed) {
                                tracing::warn!("Invalid relay pairing claim: {error}");
                            }
                        }
                        continue;
                    }
                    match serde_json::from_str::<SessionRequest>(&text) {
                        Ok(SessionRequest { session_id }) => {
                            let client = self.clone();
                            sessions.spawn(async move {
                                if let Err(err) = client.handle_session(&session_id).await {
                                    tracing::warn!(
                                        "relay data channel for session {session_id} failed: {err}"
                                    );
                                }
                            });
                        }
                        Err(err) => {
                            tracing::warn!("relay control channel sent unrecognized payload: {err}");
                        }
                    }
                }
                Some(Ok(Message::Ping(payload))) => {
                    write.send(Message::Pong(payload)).await?;
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {
                    // Binary/Pong/Frame - control channel has no other
                    // traffic; ignore.
                }
                Some(Err(err)) => return Err(err.into()),
            }
        }

        Ok(())
    }

    /// Opens a data channel for `session_id` against the relay and proxies
    /// frames bidirectionally between it and the local loopback gateway
    /// until either side disconnects.
    async fn handle_session(&self, session_id: &str) -> anyhow::Result<()> {
        let (ws, _response) = tokio_tungstenite::connect_async(self.data_url(session_id)).await?;
        let gateway = TcpStream::connect(&self.gateway_addr).await?;
        proxy_ws_to_tcp(ws, gateway).await
    }
}

async fn read_control_json<T: serde::de::DeserializeOwned>(socket: &mut ControlSocket) -> anyhow::Result<T> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => return Ok(serde_json::from_str(&text)?),
            Some(Ok(Message::Ping(payload))) => socket.send(Message::Pong(payload)).await?,
            Some(Ok(Message::Pong(_))) => continue,
            Some(Err(error)) => return Err(error.into()),
            _ => anyhow::bail!("Expected relay control text frame"),
        }
    }
}

/// Rewrites an `http(s)://` (or already-`ws(s)://`) URL to its `ws(s)://`
/// equivalent, trimming any trailing slash so path segments can be
/// appended directly.
fn to_ws_base(url: &str) -> String {
    let rewritten = if let Some(rest) = url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        url.to_string()
    };
    rewritten.trim_end_matches('/').to_string()
}

/// Proxies frames bidirectionally between a relay data-channel WebSocket
/// and a local TCP connection (the loopback gateway) until either side
/// closes or errors.
async fn proxy_ws_to_tcp(
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    tcp: TcpStream,
) -> anyhow::Result<()> {
    let (mut ws_tx, mut ws_rx) = ws.split();
    let (mut tcp_rx, mut tcp_tx) = tcp.into_split();

    let ws_to_tcp = async {
        loop {
            match ws_rx.next().await {
                Some(Ok(Message::Binary(data))) => {
                    if tcp_tx.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Text(text))) => {
                    if tcp_tx.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => continue,
                Some(Err(_)) => break,
            }
        }
        let _ = tcp_tx.shutdown().await;
    };

    let tcp_to_ws = async {
        let mut buf = [0u8; 8192];
        loop {
            match tcp_rx.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if ws_tx.send(Message::Binary(buf[..n].to_vec().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = ws_tx.send(Message::Close(None)).await;
    };

    tokio::join!(ws_to_tcp, tcp_to_ws);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_identity() -> MachineIdentity {
        use base64::Engine;
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        MachineIdentity {
            machine_id: "test-machine".into(), display_name: "test".into(),
            public_key: base64::engine::general_purpose::STANDARD.encode(key.verifying_key().to_bytes()),
            private_key: base64::engine::general_purpose::STANDARD.encode(key.to_bytes()),
        }
    }

    async fn auth_fixture(success: bool) -> (RelayClient, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = RelayClient::with_identity(format!("http://{}", listener.local_addr().unwrap()), test_identity(), LOCAL_GATEWAY_ADDR);
        let task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_hdr_async(tcp, |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                assert_eq!(request.uri().path(), "/tunnel/control");
                assert!(request.headers().get(AUTHORIZATION).is_none());
                Ok(response)
            }).await.unwrap();
            let challenge = ControlChallenge { nonce: "unique-challenge".into(), timestamp: 1234 };
            socket.send(Message::Text(serde_json::to_string(&challenge).unwrap().into())).await.unwrap();
            let frame = socket.next().await.unwrap().unwrap();
            let auth: ControlAuth = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            assert_eq!(auth.machine_id, "test-machine");
            assert!(crate::remote::auth::verify_machine_signature(&auth.public_key, &challenge.nonce, auth.timestamp, &auth.signature));
            let response = ControlAuthResponse { success, error: (!success).then(|| "denied".into()) };
            socket.send(Message::Text(serde_json::to_string(&response).unwrap().into())).await.unwrap();
        });
        (client, task)
    }

    #[tokio::test]
    async fn test_relay_client_ed25519_auth_success() {
        let (client, server) = auth_fixture(true).await;
        assert!(tokio::time::timeout(Duration::from_secs(5), client.connect_control()).await.unwrap().is_ok());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_relay_client_ed25519_auth_rejection() {
        let (client, server) = auth_fixture(false).await;
        let error = tokio::time::timeout(Duration::from_secs(5), client.connect_control()).await.unwrap().unwrap_err();
        assert!(error.to_string().contains("denied"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_pairing_coordinator_lifecycle_and_ack() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = RelayClient::new(format!("http://{}", listener.local_addr().unwrap()), "machine");
        let coordinator = client.pairing_coordinator();
        let (registered_tx, registered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let frame = socket.next().await.unwrap().unwrap();
            let registration: RegisterPairingPin = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            registered_tx.send(registration.clone()).unwrap();
            release_rx.await.unwrap();
            let ack = RegisterPairingPinAck { pin: registration.pin, machine_id: registration.machine_id, status: "ready".into() };
            socket.send(Message::Text(serde_json::to_string(&ack).unwrap().into())).await.unwrap();
            while let Some(message) = socket.next().await { if message.is_err() { break; } }
        });
        let control = tokio::spawn(async move { client.run_control_session().await });
        let generator = coordinator.clone();
        let generation = tokio::spawn(async move { generator.generate_pairing(Duration::from_secs(60)).await });
        let registration = tokio::time::timeout(Duration::from_secs(5), registered_rx).await.unwrap().unwrap();
        assert_eq!(coordinator.state(), PairingState::Registering);
        assert!(!generation.is_finished());
        assert_eq!(registration.pin.len(), 6);
        assert!(registration.pin.bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(registration.pairing_token.len(), 32);
        assert!(u128::from_str_radix(&registration.pairing_token, 16).is_ok());
        assert!(coordinator.transition(PairingState::Consumed).is_err());
        release_tx.send(()).unwrap();
        let session = tokio::time::timeout(Duration::from_secs(5), generation).await.unwrap().unwrap().unwrap();
        assert_eq!(session.pin, registration.pin);
        assert_eq!(session.pairing_token, registration.pairing_token);
        assert_eq!(coordinator.state(), PairingState::Ready);
        assert!(coordinator.generate_pairing(Duration::from_secs(60)).await.is_err());
        coordinator.transition(PairingState::Claimed).unwrap();
        coordinator.transition(PairingState::Consumed).unwrap();
        assert!(coordinator.transition(PairingState::Expired).is_err());
        assert!(coordinator.active_pin.read().is_none());
        assert!(coordinator.active_token.read().is_none());
        control.abort(); server.abort();
    }

    #[tokio::test]
    async fn test_pairing_coordinator_timeout_or_rejection() {
        let (tx, mut rx) = mpsc::channel::<RegisterPairingPinRequest>(1);
        let coordinator = PairingCoordinator::new("machine", tx);
        let reject = tokio::spawn(async move {
            let request = rx.recv().await.unwrap();
            request.ack.send(Ok(RegisterPairingPinAck { pin: request.registration.pin, machine_id: "machine".into(), status: "rejected".into() })).unwrap();
        });
        assert!(coordinator.generate_pairing(Duration::from_secs(60)).await.is_err());
        assert_eq!(coordinator.state(), PairingState::Cancelled);
        reject.await.unwrap();

        let (tx, mut rx) = mpsc::channel::<RegisterPairingPinRequest>(1);
        let coordinator = PairingCoordinator::new("machine", tx);
        // Time is the behavior under test: retain the ACK sender without responding.
        let generator = coordinator.clone();
        let generation = tokio::spawn(async move { generator.generate_pairing(Duration::from_millis(20)).await });
        let request = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap().unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(5), generation).await.unwrap().unwrap().is_err());
        assert_eq!(coordinator.state(), PairingState::Expired);
        drop(request);
    }

    #[test]
    fn to_ws_base_rewrites_https_and_trims_trailing_slash() {
        assert_eq!(to_ws_base("https://relay.example.com/"), "wss://relay.example.com");
        assert_eq!(to_ws_base("http://localhost:8787"), "ws://localhost:8787");
        assert_eq!(to_ws_base("wss://relay.example.com"), "wss://relay.example.com");
    }

    #[test]
    fn control_and_data_urls_are_built_from_relay_url() {
        let client = RelayClient::new("https://relay.example.com", "tok");
        assert_eq!(
            client.control_url(),
            "wss://relay.example.com/tunnel/control"
        );
        assert_eq!(
            client.data_url("abc123"),
            "wss://relay.example.com/tunnel/data/abc123"
        );
    }

    #[test]
    fn with_gateway_preserves_address_and_credentials() {
        let client = RelayClient::with_gateway("http://localhost:8787/", "machine", "127.0.0.1:54321");
        assert_eq!(client.gateway_addr, "127.0.0.1:54321");
        assert_eq!(client.machine_token, "machine");
        assert_eq!(client.control_url(), "ws://localhost:8787/tunnel/control");
        assert_eq!(client.data_url("session"), "ws://localhost:8787/tunnel/data/session");
        assert_eq!(RelayClient::new("http://localhost", "tok").gateway_addr, LOCAL_GATEWAY_ADDR);
    }

    #[tokio::test]
    async fn proxies_frames_between_data_channel_and_local_gateway() {
        use tokio::net::TcpListener;

        // Fake local gateway: echoes back whatever it receives, prefixed.
        let gateway_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway_addr = gateway_listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = gateway_listener.accept().await.unwrap();
            let mut buf = [0u8; 5];
            sock.read_exact(&mut buf).await.unwrap();
            let mut reply = b"echo:".to_vec();
            reply.extend_from_slice(&buf);
            sock.write_all(&reply).await.unwrap();
        });

        // A relay fixture sends the exact session notification after upgrade,
        // then exposes the data socket through a signal, not a timing delay.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = listener.local_addr().unwrap();
        let (data_tx, data_rx) = tokio::sync::oneshot::channel();
        let relay_task = tokio::spawn(async move {
            let (control, _) = listener.accept().await.unwrap();
            let mut control = tokio_tungstenite::accept_async(control).await.unwrap();
            control.send(Message::Text(r#"{"session_id":"sess-1"}"#.into())).await.unwrap();
            let (data, _) = listener.accept().await.unwrap();
            let data = tokio_tungstenite::accept_async(data).await.unwrap();
            data_tx.send(data).unwrap();
            while let Some(message) = control.next().await {
                if message.is_err() { break; }
            }
        });

        let relay_url = format!("http://{relay_addr}");
        let client = RelayClient::with_gateway(&relay_url, "tok", gateway_addr.to_string());

        // Drive the daemon's control channel (and any sessions it spawns)
        // in the background.
        let client_task = tokio::spawn(async move {
            let _ = client.run_control_session().await;
        });

        let mut client_ws = tokio::time::timeout(Duration::from_secs(5), data_rx)
            .await.expect("data connection timed out").unwrap();

        client_ws
            .send(Message::Binary(b"hello".to_vec().into()))
            .await
            .unwrap();

        let reply = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("timed out waiting for proxied reply")
            .expect("stream ended without a reply")
            .expect("websocket error");

        match reply {
            Message::Binary(data) => assert_eq!(data.as_ref(), b"echo:hello"),
            other => panic!("expected binary reply, got {other:?}"),
        }

        client_task.abort();
        assert!(client_task.await.unwrap_err().is_cancelled());
        relay_task.abort();
    }
}
