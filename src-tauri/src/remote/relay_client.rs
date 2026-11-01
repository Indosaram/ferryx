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

use crate::remote::auth::{
    canonical_auth_path, sign_control_challenge, AuthManager, DevicePermission, MachineIdentity,
};
use crate::remote::protocol::{
    AccountGrantOfferDelivered, AccountGrantOfferDelivery, ControlAuth, ControlAuthResponse,
    ControlChallenge, PairingPinClaimed, PairingState, RegisterPairingPin, RegisterPairingPinAck,
};
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use rand::Rng;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot, Mutex};

type ControlSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;
const REGISTER_ACK_TIMEOUT: Duration = Duration::from_secs(5);
/// Upper bound on one whole control connect: TCP, TLS, WebSocket upgrade and the signed
/// challenge exchange. A blackholed primary must give way to `fallback_urls` quickly.
const CONTROL_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Matches the relay's `TRANSFER_TIMEOUT`: a control write that cannot complete in this
/// long means the relay stopped reading, and the session is treated as failed.
const CONTROL_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// How often the control channel pings the relay.
const CONTROL_PING_INTERVAL: Duration = Duration::from_secs(15);
/// Three missed ping intervals with no inbound frame at all (pong, notice, ACK) means the
/// socket is half-open; the session fails so status flips and the client reconnects.
const CONTROL_IDLE_TIMEOUT: Duration = Duration::from_secs(45);

/// Sends one control frame, bounded by [`CONTROL_WRITE_TIMEOUT`]. Expiry is an error so a
/// relay that stops reading cannot stall every notice, ACK and attach reply behind it.
async fn send_control<S>(write: &mut S, message: Message) -> anyhow::Result<()>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    match tokio::time::timeout(CONTROL_WRITE_TIMEOUT, write.send(message)).await {
        Ok(result) => result.map_err(anyhow::Error::from),
        Err(_) => Err(anyhow::anyhow!(
            "relay control write timed out after {}s",
            CONTROL_WRITE_TIMEOUT.as_secs()
        )),
    }
}

pub struct RegisterPairingPinRequest {
    pub registration: RegisterPairingPin,
    pub ack: oneshot::Sender<Result<RegisterPairingPinAck, String>>,
}

#[derive(Debug, Clone)]
pub struct PairingSessionInfo {
    pub pin: String,
    pub pairing_token: String,
    pub machine_id: String,
    pub expires_at: u64,
}

pub struct AttachSessionRequest {
    pub opaque: bool,
    pub ack: oneshot::Sender<Result<String, String>>,
}

#[derive(Clone)]
pub struct PairingCoordinator {
    state: Arc<RwLock<PairingState>>,
    generation_id: Arc<RwLock<u64>>,
    active_pin: Arc<RwLock<Option<String>>>,
    active_token: Arc<RwLock<Option<String>>>,
    register_tx: mpsc::Sender<RegisterPairingPinRequest>,
    machine_id: String,
    auth: AuthManager,
}

impl PairingCoordinator {
    pub fn new(
        machine_id: impl Into<String>,
        register_tx: mpsc::Sender<RegisterPairingPinRequest>,
    ) -> Self {
        Self {
            generation_id: Arc::new(RwLock::new(0)),
            state: Arc::new(RwLock::new(PairingState::Created)),
            active_pin: Arc::new(RwLock::new(None)),
            active_token: Arc::new(RwLock::new(None)),
            register_tx,
            machine_id: machine_id.into(),
            auth: AuthManager::with_persistence(canonical_auth_path()),
        }
    }

    /// Builds a coordinator over an explicit [`AuthManager`].
    ///
    /// [`Self::new`] resolves the on-disk auth store, which a test must not touch;
    /// this lets a caller supply an isolated one and then inspect what the pairing
    /// capability actually issues.
    pub fn new_with_auth(
        machine_id: impl Into<String>,
        register_tx: mpsc::Sender<RegisterPairingPinRequest>,
        auth: AuthManager,
    ) -> Self {
        Self {
            generation_id: Arc::new(RwLock::new(0)),
            state: Arc::new(RwLock::new(PairingState::Created)),
            active_pin: Arc::new(RwLock::new(None)),
            active_token: Arc::new(RwLock::new(None)),
            register_tx,
            machine_id: machine_id.into(),
            auth,
        }
    }

    pub fn state(&self) -> PairingState {
        *self.state.read()
    }

    pub fn transition(&self, target: PairingState) -> Result<(), String> {
        let mut state = self.state.write();
        if !state.can_transition_to(&target) {
            return Err(format!(
                "Invalid pairing transition: {state:?} -> {target:?}"
            ));
        }
        *state = target;
        if matches!(
            target,
            PairingState::Consumed | PairingState::Expired | PairingState::Cancelled
        ) {
            if let Some(pin) = self.active_pin.write().take() {
                self.auth.cancel_pairing_capability(&pin);
            }
            if let Some(token) = self.active_token.write().take() {
                self.auth.cancel_pairing_capability(&token);
            }
        }
        Ok(())
    }

    /// `timeout` is the PIN lifetime; registration itself has a five-second deadline.
    pub async fn generate_pairing(&self, timeout: Duration) -> Result<PairingSessionInfo, String> {
        self.generate_pairing_with_permission(timeout, DevicePermission::Control)
            .await
    }

    /// Registers a pairing whose redeemed device receives exactly `permission`.
    pub async fn generate_pairing_with_permission(
        &self,
        timeout: Duration,
        permission: DevicePermission,
    ) -> Result<PairingSessionInfo, String> {
        self.generate_scoped_pairing(
            timeout,
            permission,
            crate::remote::auth::DeviceAccessScope::Mirror,
        )
        .await
    }

    pub async fn generate_scoped_pairing(
        &self,
        timeout: Duration,
        permission: DevicePermission,
        scope: crate::remote::auth::DeviceAccessScope,
    ) -> Result<PairingSessionInfo, String> {
        if scope == crate::remote::auth::DeviceAccessScope::Machine
            && permission != DevicePermission::Control
        {
            return Err("Machine access requires Control permission".into());
        }
        // Gateway (phone mirror) pairing codes are typed into the paired browser
        // within seconds of being shown, so they stay at sixty seconds.
        // Machine-scope pairing codes are redeemed from another machine's settings
        // UI after an operator shells in and runs the CLI, which routinely takes
        // longer than a minute; they get the full relay lease (ten minutes) and
        // remain single-use and revoked on redemption.
        let max_lifetime = if scope == crate::remote::auth::DeviceAccessScope::Machine {
            Duration::from_secs(600)
        } else {
            Duration::from_secs(60)
        };
        let timeout = timeout.min(max_lifetime);
        let expires_at = SystemTime::now()
            .checked_add(timeout)
            .ok_or("Invalid pairing lifetime")?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        let (generation, pin, pairing_token) = {
            let mut generation = self.generation_id.write();
            // An owner request replaces the previous offer, including after a
            // redemption that does not send a control-channel state transition.
            // Retire both local capabilities before publishing the next generation.
            match self.state() {
                PairingState::Ready | PairingState::Claimed | PairingState::Cancelled => {
                    self.transition(PairingState::Expired)?;
                }
                PairingState::Created
                | PairingState::Registering
                | PairingState::Consumed
                | PairingState::Expired => {}
            }
            self.transition(PairingState::Registering)?;
            *generation = generation
                .checked_add(1)
                .ok_or("Pairing generation exhausted")?;
            let pin = format!("{:06}", rand::rngs::OsRng.gen_range(0..1_000_000u32));
            let pairing_token = format!("{:032x}", rand::rngs::OsRng.gen::<u128>());
            *self.active_pin.write() = Some(pin.clone());
            *self.active_token.write() = Some(pairing_token.clone());
            self.auth
                .register_scoped_pairing_capability(&pairing_token, permission, scope)
                .map_err(|error| error.to_string())?;
            self.auth
                .register_scoped_pairing_capability(&pin, permission, scope)
                .map_err(|error| error.to_string())?;
            (*generation, pin, pairing_token)
        };
        let registration = RegisterPairingPin {
            generation: Some(generation),
            pin: pin.clone(),
            pairing_token: pairing_token.clone(),
            machine_id: self.machine_id.clone(),
            expires_at,
        };
        let (ack, rx) = oneshot::channel();
        let result = tokio::time::timeout(REGISTER_ACK_TIMEOUT.min(timeout), async {
            self.register_tx
                .send(RegisterPairingPinRequest { registration, ack })
                .await
                .map_err(|_| "Relay registration channel closed".to_string())?;
            let ack = rx
                .await
                .map_err(|_| "Relay disconnected before registration ACK".to_string())??;
            if ack.generation != Some(generation)
                || ack.pin != pin
                || ack.machine_id != self.machine_id
                || ack.status != "ready"
            {
                return Err(format!(
                    "Relay rejected pairing registration: {}",
                    ack.status
                ));
            }
            Ok(())
        })
        .await;
        let current_generation = self.generation_id.read();
        if *current_generation != generation {
            return Err("Pairing registration superseded".into());
        }
        match result {
            Ok(Ok(())) => self.transition(PairingState::Ready)?,
            Ok(Err(error)) => {
                self.transition(PairingState::Cancelled)?;
                return Err(error);
            }
            Err(_) => {
                self.transition(PairingState::Expired)?;
                return Err("Timed out waiting for relay registration ACK".into());
            }
        }
        drop(current_generation);
        let coordinator = self.clone();
        tokio::spawn(async move {
            let deadline = UNIX_EPOCH + Duration::from_secs(expires_at);
            tokio::time::sleep(
                deadline
                    .duration_since(SystemTime::now())
                    .unwrap_or_default(),
            )
            .await;
            coordinator.expire_generation(generation);
        });
        Ok(PairingSessionInfo {
            pin,
            pairing_token,
            machine_id: self.machine_id.clone(),
            expires_at,
        })
    }

    fn expire_generation(&self, generation: u64) {
        let current = self.generation_id.read();
        if *current == generation {
            if let Err(error) = self.transition(PairingState::Expired) {
                tracing::debug!("Pairing expiry ignored: {error}");
            }
        }
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

/// A control session must stay up this long before it counts as healthy and resets the
/// backoff. A relay that accepts and then drops at once must not cause a reconnect storm.
const HEALTHY_SESSION_UPTIME: Duration = Duration::from_secs(30);

/// A random delay in `[backoff / 2, backoff]`, so daemons dropped by one relay restart do not
/// reconnect in lockstep and trip the relay's admission limit together.
fn jittered(backoff: Duration, rng: &mut impl Rng) -> Duration {
    let half = backoff / 2;
    half + Duration::from_nanos(rng.gen_range(0..=(backoff - half).as_nanos() as u64))
}

/// Notification pushed down the control channel when a client asks to open
/// a new session against this daemon. Mirrors
/// [`crate::remote::relay_server::IncomingSessionNotice`] - kept as a
/// separate type here (rather than a shared import) so this module only
/// depends on the wire shape, not the server's internal types.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SessionRequest {
    session_id: String,
    #[serde(default)]
    opaque: bool,
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
    /// Further relay origins tried in order when the current one refuses or drops the control
    /// channel (a warm standby reached directly, bypassing DNS failover). Empty by default.
    fallback_urls: Arc<Vec<String>>,
    /// Index into `[relay_url, fallback_urls..]` of the endpoint in use. Data channels follow
    /// the control channel, so both always reach the same relay process.
    active_endpoint: Arc<std::sync::atomic::AtomicUsize>,
    /// This daemon's Machine Token, presented as a bearer credential on
    /// the control channel.
    machine_token: String,
    /// Local loopback gateway address that data channels are proxied to.
    gateway_addr: String,
    identity: Option<MachineIdentity>,
    pairing: PairingCoordinator,
    register_rx: Arc<Mutex<mpsc::Receiver<RegisterPairingPinRequest>>>,
    attach_tx: mpsc::Sender<AttachSessionRequest>,
    attach_rx: Arc<Mutex<mpsc::Receiver<AttachSessionRequest>>>,
    /// True only while a control session has passed authentication and has not dropped.
    /// Shared across clones so status reads the client the gateway published, not a copy.
    control_accepted: Arc<std::sync::atomic::AtomicBool>,
}

/// Holds `control_accepted` for one live control session and clears it on every exit.
pub(crate) struct ControlChannelAdmission {
    accepted: Arc<std::sync::atomic::AtomicBool>,
}

impl ControlChannelAdmission {
    fn admit(accepted: &Arc<std::sync::atomic::AtomicBool>) -> Self {
        accepted.store(true, std::sync::atomic::Ordering::Release);
        Self {
            accepted: Arc::clone(accepted),
        }
    }
}

impl Drop for ControlChannelAdmission {
    fn drop(&mut self) {
        self.accepted
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

/// Both status bits follow the accepted control session. A saved relay URL or a published
/// pairing coordinator is not a connection.
pub(crate) fn relay_connection_flags(client: Option<&RelayClient>) -> (Option<bool>, Option<bool>) {
    let accepted = client.is_some_and(RelayClient::control_channel_accepted);
    (Some(accepted), Some(accepted))
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
        let (attach_tx, attach_rx) = mpsc::channel(1);
        Self {
            relay_url: relay_url.into(),
            fallback_urls: Arc::new(Vec::new()),
            active_endpoint: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            pairing: PairingCoordinator::new(machine_token.clone(), tx),
            machine_token,
            gateway_addr: gateway_addr.into(),
            identity: None,
            register_rx: Arc::new(Mutex::new(rx)),
            attach_tx,
            attach_rx: Arc::new(Mutex::new(attach_rx)),
            control_accepted: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn with_machine_id(mut self, machine_id: impl Into<String>) -> Self {
        self.pairing.machine_id = machine_id.into();
        self
    }

    /// The machine id this client announces to the relay and binds into every attach
    /// handshake: its signing identity's, or the one set with [`Self::with_machine_id`] when it
    /// authenticates with a machine token instead.
    fn machine_id(&self) -> &str {
        self.identity
            .as_ref()
            .map(|identity| identity.machine_id.as_str())
            .unwrap_or(&self.pairing.machine_id)
    }

    pub fn with_identity(
        relay_url: impl Into<String>,
        identity: MachineIdentity,
        gateway_addr: impl Into<String>,
    ) -> Self {
        let mut client = Self::with_gateway(relay_url, "", gateway_addr);
        client.pairing.machine_id = identity.machine_id.clone();
        client.identity = Some(identity);
        client
    }

    #[cfg(test)]
    pub(crate) fn with_identity_and_auth(
        relay_url: impl Into<String>,
        identity: MachineIdentity,
        gateway_addr: impl Into<String>,
        auth: AuthManager,
    ) -> Self {
        let (tx, rx) = mpsc::channel(1);
        let (attach_tx, attach_rx) = mpsc::channel(1);
        Self {
            relay_url: relay_url.into(),
            fallback_urls: Arc::new(Vec::new()),
            active_endpoint: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            pairing: PairingCoordinator::new_with_auth(identity.machine_id.clone(), tx, auth),
            machine_token: String::new(),
            gateway_addr: gateway_addr.into(),
            identity: Some(identity),
            register_rx: Arc::new(Mutex::new(rx)),
            attach_tx,
            attach_rx: Arc::new(Mutex::new(attach_rx)),
            control_accepted: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Supplies the private relay's enrollment credential for key-based authentication.
    pub fn with_enrollment_token(mut self, token: impl Into<String>) -> Self {
        self.machine_token = token.into();
        self
    }

    /// Adds relay origins to fail over to when the primary refuses or drops the control
    /// channel. Blank entries and duplicates of the primary are ignored.
    pub fn with_fallback_urls<I, S>(mut self, urls: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let primary = self.relay_url.trim_end_matches('/').to_string();
        let mut fallbacks: Vec<String> = Vec::new();
        for url in urls {
            let url: String = url.into();
            let url = url.trim().trim_end_matches('/').to_string();
            if !url.is_empty() && url != primary && !fallbacks.contains(&url) {
                fallbacks.push(url);
            }
        }
        self.fallback_urls = Arc::new(fallbacks);
        self.active_endpoint
            .store(0, std::sync::atomic::Ordering::Release);
        self
    }

    /// The relay origin the control and data channels currently target.
    pub fn active_relay_url(&self) -> &str {
        let index = self
            .active_endpoint
            .load(std::sync::atomic::Ordering::Acquire);
        match index {
            0 => self.relay_url.as_str(),
            n => self
                .fallback_urls
                .get(n - 1)
                .map(String::as_str)
                .unwrap_or(self.relay_url.as_str()),
        }
    }

    /// Moves to the next configured endpoint after a failed or dropped control session.
    /// A single-endpoint client stays where it is.
    fn advance_endpoint(&self) {
        let count = self.fallback_urls.len() + 1;
        if count == 1 {
            return;
        }
        let next = (self
            .active_endpoint
            .load(std::sync::atomic::Ordering::Acquire)
            + 1)
            % count;
        self.active_endpoint
            .store(next, std::sync::atomic::Ordering::Release);
        tracing::warn!("relay failover: switching control channel to {}", self.active_relay_url());
    }

    /// Shares the daemon's authority, including in-memory gateway configurations.
    pub fn with_auth_manager(mut self, auth: AuthManager) -> Self {
        self.pairing.auth = auth;
        self
    }

    pub fn pairing_coordinator(&self) -> PairingCoordinator {
        self.pairing.clone()
    }

    /// The relay has accepted this daemon's control session and the socket is still up.
    /// Pairing configuration alone is not acceptance: a rejected handshake stays false.
    pub fn control_channel_accepted(&self) -> bool {
        self.control_accepted
            .load(std::sync::atomic::Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn test_admit_control_channel(&self) -> ControlChannelAdmission {
        ControlChannelAdmission::admit(&self.control_accepted)
    }

    pub async fn connect_control(&self) -> anyhow::Result<ControlSocket> {
        match tokio::time::timeout(CONTROL_CONNECT_TIMEOUT, self.connect_control_unbounded()).await
        {
            Ok(result) => result,
            Err(_) => Err(anyhow::anyhow!(
                "relay control connect timed out after {}s",
                CONTROL_CONNECT_TIMEOUT.as_secs()
            )),
        }
    }

    async fn connect_control_unbounded(&self) -> anyhow::Result<ControlSocket> {
        let mut request = self.control_url()?.into_client_request()?;
        if self.identity.is_none() {
            request.headers_mut().insert(
                AUTHORIZATION,
                format!("Bearer {}", self.machine_token).parse()?,
            );
        }
        let machine_id = self.machine_id();
        if !machine_id.is_empty() {
            if let Ok(val) = machine_id.parse() {
                request.headers_mut().insert("x-ferryx-machine-id", val);
            }
        }
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await?;
        if let Some(identity) = &self.identity {
            tokio::time::timeout(REGISTER_ACK_TIMEOUT, async {
                let challenge: ControlChallenge = read_control_json(&mut socket).await?;
                let auth = ControlAuth {
                    enrollment_token: (!self.machine_token.is_empty())
                        .then(|| self.machine_token.clone()),
                    machine_id: identity.machine_id.clone(),
                    display_name: identity.display_name.clone(),
                    public_key: identity.public_key.clone(),
                    // Sign the audience this relay announced, so the signature is
                    // only usable against that relay. A relay that omits the field
                    // predates it and still expects the legacy constant.
                    signature: sign_control_challenge(
                        identity,
                        challenge
                            .audience
                            .as_deref()
                            .unwrap_or(crate::remote::relay_server::LEGACY_CONTROL_AUDIENCE),
                        &challenge.nonce,
                        challenge.timestamp,
                    )
                    .map_err(anyhow::Error::msg)?,
                    timestamp: challenge.timestamp,
                };
                socket
                    .send(Message::Text(serde_json::to_string(&auth)?.into()))
                    .await?;
                let response: ControlAuthResponse = read_control_json(&mut socket).await?;
                anyhow::ensure!(
                    response.success,
                    "Relay authentication rejected: {}",
                    response.error.unwrap_or_default()
                );
                Ok::<_, anyhow::Error>(())
            })
            .await??;
        }
        Ok(socket)
    }

    /// A URL that fails relay validation is never dialed: falling back to the raw string
    /// would reach a plaintext relay the security policy just refused.
    fn control_url(&self) -> Result<String, RelayUrlSecurityError> {
        Ok(format!(
            "{}/tunnel/control",
            to_ws_base(self.active_relay_url())?
        ))
    }

    fn data_url(&self, session_id: &str) -> Result<String, RelayUrlSecurityError> {
        Ok(format!(
            "{}/tunnel/data/{}",
            to_ws_base(self.active_relay_url())?,
            session_id
        ))
    }

    /// Runs the reverse tunnel client forever: connects the control
    /// channel, handles incoming session notifications until the
    /// connection drops, then reconnects with exponential backoff and
    /// repeats. Intended to be spawned onto a dedicated task.
    pub async fn run(&self) {
        let mut backoff = INITIAL_BACKOFF;
        loop {
            let started = tokio::time::Instant::now();
            let result = self.run_control_session().await;
            backoff = self.next_backoff(started.elapsed(), backoff);
            match result {
                // Still reconnect - the daemon is expected to stay tunneled for as long
                // as it runs.
                Ok(()) => tracing::info!(
                    "relay control channel closed; reconnecting in {:.1}s",
                    backoff.as_secs_f64()
                ),
                Err(err) => tracing::warn!(
                    "relay control channel error: {err}; retrying in {:.1}s",
                    backoff.as_secs_f64()
                ),
            }

            // Computed in its own statement: the thread-local rng is !Send and must not be
            // held across the await.
            let delay = jittered(backoff, &mut rand::thread_rng());
            tokio::time::sleep(delay).await;
            backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
        }
    }

    /// Picks the delay before the next connect from how long the last session stayed up.
    /// Only a session that lasted [`HEALTHY_SESSION_UPTIME`] resets the backoff and returns
    /// to the primary (a failover is temporary); a session that dropped at once, cleanly or
    /// not, keeps backing off and moves to the next endpoint.
    fn next_backoff(&self, uptime: Duration, backoff: Duration) -> Duration {
        if uptime >= HEALTHY_SESSION_UPTIME {
            self.active_endpoint
                .store(0, std::sync::atomic::Ordering::Release);
            INITIAL_BACKOFF
        } else {
            self.advance_endpoint();
            backoff
        }
    }

    /// Connects the control channel and services it until it closes or
    /// errors. Each incoming [`SessionRequest`] spawns an independent task
    /// that opens a data channel and proxies it against the local gateway,
    /// so a slow or stuck session cannot block subsequent notifications.
    /// Asks the relay for a session the caller can attach to with the account handshake, and
    /// returns its id. The relay mints the id; the daemon only carries the request and the
    /// answer, so the client learns the id from the machine it is attaching to.
    pub async fn allocate_opaque_session(&self) -> anyhow::Result<String> {
        let (ack, wait) = oneshot::channel::<Result<String, String>>();
        self.attach_tx
            .send(AttachSessionRequest { opaque: true, ack })
            .await
            .map_err(|_| anyhow::anyhow!("relay control channel is not running"))?;
        match tokio::time::timeout(Duration::from_secs(10), wait).await {
            Ok(Ok(Ok(session_id))) => Ok(session_id),
            Ok(Ok(Err(error))) => Err(anyhow::anyhow!(error)),
            Ok(Err(_)) => Err(anyhow::anyhow!("relay control channel dropped the request")),
            Err(_) => Err(anyhow::anyhow!("relay did not allocate a session in time")),
        }
    }

    async fn run_control_session(&self) -> anyhow::Result<()> {
        let socket = self.connect_control().await?;
        self.serve_control_session(socket).await
    }

    /// Services an authenticated control socket until it closes or errors.
    async fn serve_control_session(&self, socket: ControlSocket) -> anyhow::Result<()> {
        // Set only after authentication succeeds. Drop clears every exit, including a later reject.
        let _admitted = ControlChannelAdmission::admit(&self.control_accepted);
        let mut registrations = self.register_rx.lock().await;
        let mut attachments = self.attach_rx.lock().await;
        let mut pending: Option<RegisterPairingPinRequest> = None;
        let mut pending_attach: Option<AttachSessionRequest> = None;
        tracing::info!("relay control channel connected");
        let (mut write, mut read) = socket.split();
        // Dropping the control future also aborts its data channels.
        let mut sessions = tokio::task::JoinSet::new();
        let mut ping_interval = tokio::time::interval(CONTROL_PING_INTERVAL);
        ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_inbound = tokio::time::Instant::now();

        loop {
            let message = tokio::select! {
                _ = ping_interval.tick() => {
                    if last_inbound.elapsed() >= CONTROL_IDLE_TIMEOUT {
                        return Err(anyhow::anyhow!(
                            "relay control channel silent for {}s; treating it as dead",
                            last_inbound.elapsed().as_secs()
                        ));
                    }
                    if let Err(err) = send_control(&mut write, Message::Ping(Vec::new().into())).await {
                        return Err(err.context("relay control ping failed"));
                    }
                    continue;
                }
                request = attachments.recv(), if pending_attach.is_none() => {
                    if let Some(request) = request {
                        if request.ack.is_closed() { continue; }
                        let payload = serde_json::json!({"type": "AllocateSession", "opaque": request.opaque});
                        if let Err(err) = send_control(&mut write, Message::Text(payload.to_string().into())).await {
                            return Err(err.context("relay attach allocation failed"));
                        }
                        pending_attach = Some(request);
                    }
                    continue;
                }
                registration = registrations.recv(), if pending.is_none() => {
                    if let Some(registration) = registration {
                        if registration.ack.is_closed() { continue; }
                        send_control(&mut write, Message::Text(serde_json::to_string(&registration.registration)?.into())).await?;
                        pending = Some(registration);
                    }
                    continue;
                }
                _ = async { pending.as_mut().unwrap().ack.closed().await }, if pending.is_some() => {
                    pending = None;
                    continue;
                }
                message = read.next() => {
                    last_inbound = tokio::time::Instant::now();
                    message
                }
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
                        if pending.as_ref().is_some_and(|request| {
                            request.registration.generation == ack.generation
                                && request.registration.pin == ack.pin
                                && request.registration.machine_id == ack.machine_id
                        }) && pending.take().unwrap().ack.send(Ok(ack)).is_err() {
                            tracing::debug!(
                                "Registration ACK arrived after caller disconnected"
                            );
                        }
                        continue;
                    }
                    if let Ok(claim) = serde_json::from_str::<PairingPinClaimed>(&text) {
                        let matches = claim.machine_id == self.pairing.machine_id
                            && self.pairing.active_pin.read().as_ref() == Some(&claim.pin);
                        if matches {
                            if let Err(error) = self.pairing.transition(PairingState::Claimed) {
                                tracing::warn!("Invalid relay pairing claim: {error}");
                            }
                        }
                        continue;
                    }
                    if let Ok(delivery) = serde_json::from_str::<AccountGrantOfferDelivery>(&text) {
                        if delivery.machine_id == self.pairing.machine_id {
                            let now = crate::account::store::now_secs();
                            let auth = self.pairing.auth.clone();
                            let envelope = delivery.envelope;
                            let applied = crate::ipc::run_blocking(move || {
                                Ok(crate::account::offer_sink::apply_envelope_for_this_machine(
                                    &auth, &envelope, now,
                                ))
                            })
                            .await
                            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                            let ack = match applied {
                                Ok(ack) => AccountGrantOfferDelivered {
                                    grant_id: Some(ack.grant_id),
                                    status: ack.status,
                                },
                                Err(error) => {
                                    tracing::warn!(
                                        "Refused an account grant offer for this machine: {error}"
                                    );
                                    AccountGrantOfferDelivered {
                                        grant_id: None,
                                        status: error.code().to_string(),
                                    }
                                }
                            };
                            let encoded = serde_json::to_string(&ack)?;
                            send_control(&mut write, Message::Text(encoded.into())).await?;
                        }
                        continue;
                    }
                    match serde_json::from_str::<SessionRequest>(&text) {
                        Ok(SessionRequest { session_id, opaque }) => {
                            if opaque {
                                if let Some(request) = pending_attach.take() {
                                    let _ = request.ack.send(Ok(session_id));
                                    continue;
                                }
                            }
                            let client = self.clone();
                            sessions.spawn(async move {
                                match client.handle_session(&session_id, opaque).await {
                                    Ok(()) => tracing::debug!(
                                        "relay data channel for session {session_id} ended"
                                    ),
                                    Err(err) => tracing::warn!(
                                        "relay data channel for session {session_id} failed: {err}"
                                    ),
                                }
                            });
                        }
                        Err(err) => {
                            tracing::warn!(
                                "relay control channel sent unrecognized payload: {err}"
                            );
                        }
                    }
                }
                Some(Ok(Message::Ping(payload))) => {
                    send_control(&mut write, Message::Pong(payload)).await?;
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
    async fn handle_session(&self, session_id: &str, opaque: bool) -> anyhow::Result<()> {
        let (ws, _response) = tokio_tungstenite::connect_async(self.data_url(session_id)?).await?;
        let gateway = TcpStream::connect(&self.gateway_addr).await?;
        if !opaque {
            return proxy_ws_to_tcp(ws, gateway).await;
        }

        let (attach, enrollment_record) = crate::ipc::run_blocking(|| {
            Ok((
                crate::remote::attach_identity::load_or_generate_canonical_attach_identity(),
                crate::account::enroll_client::load_enrollment_record(),
            ))
        })
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let attach = attach.map_err(|error| anyhow::anyhow!(error))?;
        // The browser binds the account machine id into the prologue; a token-authenticated
        // client has no identity here and must still bind the id it announced.
        let machine_id = self.machine_id().to_owned();
        let enrollment_epoch = enrollment_record
            .map(|record| record.enrollment_epoch)
            .unwrap_or_default();
        let auth = self.pairing.auth.clone();

        let stream = crate::remote::attach_crypto::WebSocketByteStream::new(ws);
        let transport = crate::remote::session_transport::establish_session(
            stream,
            true,
            Some(&attach),
            &machine_id,
            session_id,
            &enrollment_epoch,
            move |key: &[u8; 32]| auth.authorizes_attach_key_bytes(key),
        )
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

        match transport {
            crate::remote::session_transport::SessionTransport::Attached(secure) => {
                crate::remote::session_transport::proxy_secure_to_gateway(secure, gateway)
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))
            }
            crate::remote::session_transport::SessionTransport::Plain(stream) => {
                proxy_ws_to_tcp(stream.into_inner(), gateway).await
            }
        }
    }
}

async fn read_control_json<T: serde::de::DeserializeOwned>(
    socket: &mut ControlSocket,
) -> anyhow::Result<T> {
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, thiserror::Error)]
pub enum RelayUrlSecurityError {
    #[error("Invalid relay URL '{url}': {reason}")]
    InvalidUrl { url: String, reason: String },

    #[error("Insecure relay scheme: non-loopback relay '{url}' must use https:// or wss://")]
    InsecureSchemeTlsRequired { url: String },

    #[error("Insecure non-loopback relay '{url}' requires explicit development opt-in and is restricted to RFC1918 private network addresses")]
    InsecureDevelopmentRelayRequiresOptIn { url: String },

    #[error("Insecure relay '{url}' is on a public/non-private network: plaintext http/ws relay connections are forbidden over public networks")]
    InsecurePublicRelayForbidden { url: String },
}

impl RelayUrlSecurityError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidUrl { .. } => "INVALID_RELAY_URL",
            Self::InsecureSchemeTlsRequired { .. } => "INSECURE_RELAY_SCHEME",
            Self::InsecureDevelopmentRelayRequiresOptIn { .. } => "INSECURE_RELAY_OPT_IN_REQUIRED",
            Self::InsecurePublicRelayForbidden { .. } => "INSECURE_PUBLIC_RELAY_FORBIDDEN",
        }
    }

    pub fn to_error_envelope(&self) -> serde_json::Value {
        serde_json::json!({
            "code": self.code(),
            "message": self.to_string(),
            "details": match self {
                Self::InvalidUrl { url, reason } => serde_json::json!({ "url": url, "reason": reason }),
                Self::InsecureSchemeTlsRequired { url } => serde_json::json!({ "url": url, "tlsRequired": true }),
                Self::InsecureDevelopmentRelayRequiresOptIn { url } => serde_json::json!({ "url": url, "rfc1918Required": true, "optInRequired": true }),
                Self::InsecurePublicRelayForbidden { url } => serde_json::json!({ "url": url, "publicNetwork": true }),
            }
        })
    }
}

pub static ALLOW_INSECURE_RELAY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn set_allow_insecure_relay(allow: bool) {
    ALLOW_INSECURE_RELAY.store(allow, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_insecure_relay_allowed() -> bool {
    ALLOW_INSECURE_RELAY.load(std::sync::atomic::Ordering::Relaxed)
        || crate::remote::security_settings::insecure_relay_env_enabled()
}

pub fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let clean_host = host.trim_matches(|c| c == '[' || c == ']');
    if let Ok(ip) = clean_host.parse::<std::net::IpAddr>() {
        return ip.is_loopback();
    }
    false
}

pub fn is_rfc1918_private_host(host: &str) -> bool {
    let clean_host = host.trim_matches(|c| c == '[' || c == ']');
    if let Ok(std::net::IpAddr::V4(ipv4)) = clean_host.parse::<std::net::IpAddr>() {
        ipv4.is_private()
    } else {
        false
    }
}

pub fn validate_relay_url(
    url_str: &str,
    allow_insecure_dev: bool,
) -> Result<String, RelayUrlSecurityError> {
    let parsed = reqwest::Url::parse(url_str).map_err(|e| RelayUrlSecurityError::InvalidUrl {
        url: url_str.to_string(),
        reason: e.to_string(),
    })?;

    let scheme = parsed.scheme();
    if !matches!(scheme, "http" | "https" | "ws" | "wss") {
        return Err(RelayUrlSecurityError::InvalidUrl {
            url: url_str.to_string(),
            reason: format!("unsupported scheme '{scheme}', expected http(s) or ws(s)"),
        });
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| RelayUrlSecurityError::InvalidUrl {
            url: url_str.to_string(),
            reason: "missing host in relay URL".to_string(),
        })?;

    let is_loopback = is_loopback_host(host);
    let is_rfc1918 = is_rfc1918_private_host(host);

    let rest = url_str.split_once("://").map(|(_, r)| r).unwrap_or(url_str);
    let trimmed_rest = rest.trim_end_matches('/');

    match scheme {
        "https" | "wss" => Ok(format!("wss://{trimmed_rest}")),
        "http" | "ws" => {
            if is_loopback {
                Ok(format!("ws://{trimmed_rest}"))
            } else if is_rfc1918 {
                if allow_insecure_dev {
                    tracing::warn!(
                        "Insecure development relay allowed on RFC1918 private address '{host}'. Plaintext traffic is exposed on local network."
                    );
                    Ok(format!("ws://{trimmed_rest}"))
                } else {
                    Err(
                        RelayUrlSecurityError::InsecureDevelopmentRelayRequiresOptIn {
                            url: url_str.to_string(),
                        },
                    )
                }
            } else {
                Err(RelayUrlSecurityError::InsecurePublicRelayForbidden {
                    url: url_str.to_string(),
                })
            }
        }
        _ => unreachable!(),
    }
}

/// Rewrites an `http(s)://` (or already-`ws(s)://`) URL to its `ws(s)://`
/// equivalent, trimming any trailing slash so path segments can be
/// appended directly.
pub fn to_ws_base(url: &str) -> Result<String, RelayUrlSecurityError> {
    validate_relay_url(&live_relay_url(url), is_insecure_relay_allowed())
}

/// Comma-separated relay origins a daemon fails over to when its configured relay is down.
pub const RELAY_FALLBACK_URLS_ENV: &str = "FERRYX_RELAY_FALLBACK_URLS";

/// Parses [`RELAY_FALLBACK_URLS_ENV`], dropping entries that fail relay URL validation (and
/// the retired relay.checka.cc origin), so a typo can never downgrade the transport.
pub fn fallback_relay_urls_from(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .filter(|url| {
            let retired = url.trim_end_matches('/') == RETIRED_RELAY_ORIGIN;
            let valid = validate_relay_url(url, is_insecure_relay_allowed()).is_ok();
            if retired || !valid {
                tracing::warn!("ignoring relay fallback URL {url}");
            }
            !retired && valid
        })
        .map(str::to_string)
        .collect()
}

pub fn fallback_relay_urls_from_env() -> Vec<String> {
    fallback_relay_urls_from(std::env::var(RELAY_FALLBACK_URLS_ENV).ok().as_deref())
}

/// The retired public relay hostname. Pairings and daemon configs saved before the move to
/// relay.ferryx.dev still carry it (host ids embed it), and it no longer resolves to a relay.
pub const RETIRED_RELAY_ORIGIN: &str = "https://relay.checka.cc";

/// Rewrites a URL on the retired relay origin to the same path on the current default relay,
/// leaving every other URL untouched. Stored ids keep the old origin; only connections move.
pub fn live_relay_url(url: &str) -> std::borrow::Cow<'_, str> {
    match url.strip_prefix(RETIRED_RELAY_ORIGIN) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => std::borrow::Cow::Owned(format!(
            "{}{rest}",
            crate::remote::state::DEFAULT_RELAY_URL
        )),
        _ => std::borrow::Cow::Borrowed(url),
    }
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
        // 64 KiB matches the relay data channel's per-message cap (relay_server MAX_MESSAGE_SIZE).
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match tcp_rx.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if ws_tx
                        .send(Message::Binary(buf[..n].to_vec().into()))
                        .await
                        .is_err()
                    {
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
            machine_id: "test-machine".into(),
            display_name: "test".into(),
            public_key: base64::engine::general_purpose::STANDARD
                .encode(key.verifying_key().to_bytes()),
            private_key: base64::engine::general_purpose::STANDARD.encode(key.to_bytes()),
        }
    }

    #[test]
    fn a_token_authenticated_client_binds_its_announced_machine_id() {
        // The daemon with FERRYX_MACHINE_TOKEN: no signing identity, machine id set separately.
        // An empty id here gives the attach responder a prologue the browser never used.
        let token =
            RelayClient::with_gateway("https://relay.example", "operator-token", LOCAL_GATEWAY_ADDR)
                .with_machine_id("enrolled-machine");
        assert_eq!(token.machine_id(), "enrolled-machine");
        let signed =
            RelayClient::with_identity("https://relay.example", test_identity(), LOCAL_GATEWAY_ADDR);
        assert_eq!(signed.machine_id(), "test-machine");
    }

    async fn auth_fixture(success: bool) -> (RelayClient, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = RelayClient::with_identity(
            format!("http://{}", listener.local_addr().unwrap()),
            test_identity(),
            LOCAL_GATEWAY_ADDR,
        );
        let task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_hdr_async(
                tcp,
                crate::remote::InspectHandshake(|request: &tokio_tungstenite::tungstenite::handshake::server::Request| {
                    assert_eq!(request.uri().path(), "/tunnel/control");
                    assert!(request.headers().get(AUTHORIZATION).is_none());
                }),
            )
            .await
            .unwrap();
            let challenge = ControlChallenge {
                nonce: "unique-challenge".into(),
                timestamp: 1234,
                audience: None,
            };
            socket
                .send(Message::Text(
                    serde_json::to_string(&challenge).unwrap().into(),
                ))
                .await
                .unwrap();
            let frame = socket.next().await.unwrap().unwrap();
            let auth: ControlAuth = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            assert_eq!(auth.machine_id, "test-machine");
            assert!(crate::remote::auth::verify_control_challenge(
                &auth.public_key,
                &auth.machine_id,
                "relay",
                &challenge.nonce,
                auth.timestamp,
                &auth.signature
            ));
            let response = ControlAuthResponse {
                success,
                error: (!success).then(|| "denied".into()),
            };
            socket
                .send(Message::Text(
                    serde_json::to_string(&response).unwrap().into(),
                ))
                .await
                .unwrap();
        });
        (client, task)
    }

    #[tokio::test]
    async fn an_opaque_allocation_returns_the_relays_session_id() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = RelayClient::new(format!("http://{}", listener.local_addr().unwrap()), "machine");
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let request: serde_json::Value = loop {
                match socket.next().await.unwrap().unwrap() {
                    Message::Text(text) => break serde_json::from_str(&text).unwrap(),
                    _ => continue,
                }
            };
            assert_eq!(request["type"], "AllocateSession");
            assert_eq!(request["opaque"], true);
            socket
                .send(Message::Text(
                    serde_json::json!({"session_id": "opaque-1", "opaque": true})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            while let Some(message) = socket.next().await {
                if message.is_err() {
                    break;
                }
            }
        });
        let control = tokio::spawn({
            let client = client.clone();
            async move { client.run_control_session().await }
        });
        let id = tokio::time::timeout(Duration::from_secs(5), client.allocate_opaque_session())
            .await
            .expect("bounded allocation")
            .expect("session id");
        assert_eq!(id, "opaque-1");
        control.abort();
        server.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_stalled_relay_cannot_hold_the_control_connect_open() {
        // The listener's backlog completes TCP, but nothing ever answers the upgrade.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = RelayClient::new(format!("http://{}", listener.local_addr().unwrap()), "tok");
        let error = tokio::time::timeout(Duration::from_secs(60), client.connect_control())
            .await
            .expect("connect_control must give up on its own")
            .unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error}");
        drop(listener);
    }

    #[tokio::test(start_paused = true)]
    async fn a_control_write_the_relay_never_reads_fails_instead_of_hanging() {
        // A sink that accepts the frame but never finishes flushing it, like a relay whose
        // TCP window stays open while it stops reading.
        let mut stalled = std::pin::pin!(futures_util::sink::unfold((), |(), _message: Message| {
            futures_util::future::pending::<Result<(), tokio_tungstenite::tungstenite::Error>>()
        }));
        let error = tokio::time::timeout(
            CONTROL_WRITE_TIMEOUT * 2,
            send_control(&mut stalled, Message::Ping(Vec::new().into())),
        )
        .await
        .expect("the write must give up on its own")
        .unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error}");
    }

    #[tokio::test]
    async fn a_silent_relay_fails_the_control_session_and_clears_connected() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = RelayClient::new(format!("http://{}", listener.local_addr().unwrap()), "tok");
        let (release_tx, release_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            // Hold the socket without ever reading it: pings are never answered, like a
            // half-open connection after a NAT drop.
            let socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let _ = release_rx.await;
            drop(socket);
        });
        let socket = client.connect_control().await.unwrap();
        // Only the idle deadline is under test; the paused clock auto-advances while the
        // runtime waits on a relay that never answers.
        tokio::time::pause();
        let error = tokio::time::timeout(
            CONTROL_IDLE_TIMEOUT * 4,
            client.serve_control_session(socket),
        )
        .await
        .expect("a silent control channel must fail on its own")
        .unwrap_err();
        assert!(error.to_string().contains("silent"), "{error}");
        assert!(!client.control_channel_accepted());
        release_tx.send(()).ok();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_relay_client_ed25519_auth_success() {
        let (client, server) = auth_fixture(true).await;
        assert!(
            tokio::time::timeout(Duration::from_secs(5), client.connect_control())
                .await
                .unwrap()
                .is_ok()
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_relay_client_ed25519_auth_rejection() {
        let (client, server) = auth_fixture(false).await;
        let error = tokio::time::timeout(Duration::from_secs(5), client.connect_control())
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("denied"));
        assert!(
            !client.control_channel_accepted(),
            "a rejected handshake must not count as connected"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejected_control_session_never_reports_connected() {
        let (client, server) = auth_fixture(false).await;
        let error = tokio::time::timeout(Duration::from_secs(5), client.run_control_session())
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("denied") || error.to_string().contains("rejected"));
        assert_eq!(relay_connection_flags(Some(&client)), (Some(false), Some(false)));
        assert_eq!(relay_connection_flags(None), (Some(false), Some(false)));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn control_channel_is_accepted_only_while_the_session_is_up() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (release_tx, release_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let _ = release_rx.await;
            let _ = socket.close(None).await;
        });
        let client = RelayClient::new(format!("http://{addr}"), "machine");
        assert_eq!(
            relay_connection_flags(Some(&client)),
            (Some(false), Some(false))
        );
        let running = client.clone();
        let task = tokio::spawn(async move { running.run_control_session().await });
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if client.control_channel_accepted() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("control channel was never accepted");
        assert_eq!(relay_connection_flags(Some(&client)), (Some(true), Some(true)));
        release_tx.send(()).ok();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if !client.control_channel_accepted() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("accepted flag stayed set after the session dropped");
        let _ = task.await;
        server.abort();
    }

    #[test]
    fn admitting_the_control_channel_is_what_status_reads() {
        let client = RelayClient::new("https://relay.example", "token");
        assert!(!client.control_channel_accepted());
        let admission = client.test_admit_control_channel();
        assert_eq!(relay_connection_flags(Some(&client)), (Some(true), Some(true)));
        drop(admission);
        assert_eq!(relay_connection_flags(Some(&client)), (Some(false), Some(false)));
    }

    #[tokio::test]
    async fn test_pairing_coordinator_lifecycle_and_ack() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = RelayClient::new(
            format!("http://{}", listener.local_addr().unwrap()),
            "machine",
        );
        let coordinator = client.pairing_coordinator();
        let (registered_tx, registered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let frame = socket.next().await.unwrap().unwrap();
            let registration: RegisterPairingPin =
                serde_json::from_str(frame.to_text().unwrap()).unwrap();
            registered_tx.send(registration.clone()).unwrap();
            release_rx.await.unwrap();
            let ack = RegisterPairingPinAck {
                generation: registration.generation,
                pin: registration.pin,
                machine_id: registration.machine_id,
                status: "ready".into(),
            };
            socket
                .send(Message::Text(serde_json::to_string(&ack).unwrap().into()))
                .await
                .unwrap();
            while let Some(message) = socket.next().await {
                if message.is_err() {
                    break;
                }
            }
        });
        let control = tokio::spawn(async move { client.run_control_session().await });
        let generator = coordinator.clone();
        let generation =
            tokio::spawn(async move { generator.generate_pairing(Duration::from_secs(60)).await });
        let registration = tokio::time::timeout(Duration::from_secs(5), registered_rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(coordinator.state(), PairingState::Registering);
        assert!(!generation.is_finished());
        assert_eq!(registration.pin.len(), 6);
        assert!(registration.pin.bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(registration.pairing_token.len(), 32);
        assert!(u128::from_str_radix(&registration.pairing_token, 16).is_ok());
        assert!(coordinator.transition(PairingState::Consumed).is_err());
        assert!(coordinator
            .generate_pairing(Duration::from_secs(60))
            .await
            .is_err());
        release_tx.send(()).unwrap();
        let session = tokio::time::timeout(Duration::from_secs(5), generation)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(session.pin, registration.pin);
        assert_eq!(session.pairing_token, registration.pairing_token);
        assert_eq!(coordinator.state(), PairingState::Ready);
        coordinator.transition(PairingState::Claimed).unwrap();
        coordinator.transition(PairingState::Consumed).unwrap();
        assert!(coordinator.transition(PairingState::Expired).is_err());
        assert!(coordinator.active_pin.read().is_none());
        assert!(coordinator.active_token.read().is_none());
        control.abort();
        server.abort();
    }

    #[tokio::test]
    async fn test_pairing_coordinator_regenerates_and_ignores_old_expiry() {
        let (tx, mut rx) = mpsc::channel::<RegisterPairingPinRequest>(1);
        let mut coordinator = PairingCoordinator::new("machine", tx);
        coordinator.auth = AuthManager::with_persistence(None);
        let responder = tokio::spawn(async move {
            for generation in 1..=4 {
                let request = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(request.registration.generation, Some(generation));
                request
                    .ack
                    .send(Ok(RegisterPairingPinAck {
                        generation: Some(generation),
                        pin: request.registration.pin,
                        machine_id: request.registration.machine_id,
                        status: "ready".into(),
                    }))
                    .unwrap();
            }
        });
        let mut previous: Option<PairingSessionInfo> = None;
        for generation in 1..=4 {
            // Given an earlier offer, when the owner issues its replacement,
            // then neither local credential from that offer remains redeemable.
            let session = coordinator
                .generate_pairing(Duration::from_secs(60))
                .await
                .unwrap();
            if let Some(old) = previous {
                assert!(coordinator
                    .auth
                    .exchange_pairing_code(&old.pin, "stale")
                    .is_err());
                assert!(coordinator
                    .auth
                    .exchange_pairing_code(&old.pairing_token, "stale")
                    .is_err());
            }
            previous = Some(session);
            assert_eq!(*coordinator.generation_id.read(), generation);
            coordinator.expire_generation(generation - 1);
            assert_eq!(coordinator.state(), PairingState::Ready);
            if generation % 2 == 0 {
                coordinator.transition(PairingState::Claimed).unwrap();
                coordinator.transition(PairingState::Consumed).unwrap();
            } else if generation == 3 {
                coordinator.expire_generation(generation);
                assert_eq!(coordinator.state(), PairingState::Expired);
            }
        }
        responder.await.unwrap();
    }

    #[tokio::test]
    async fn test_pairing_coordinator_rejects_wrong_ack_generation() {
        let (tx, mut rx) = mpsc::channel::<RegisterPairingPinRequest>(1);
        let mut coordinator = PairingCoordinator::new("machine", tx);
        coordinator.auth = AuthManager::with_persistence(None);
        let responder = tokio::spawn(async move {
            let request = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .unwrap()
                .unwrap();
            request
                .ack
                .send(Ok(RegisterPairingPinAck {
                    generation: Some(0),
                    pin: request.registration.pin,
                    machine_id: request.registration.machine_id,
                    status: "ready".into(),
                }))
                .unwrap();
        });
        assert!(coordinator
            .generate_pairing(Duration::from_secs(60))
            .await
            .is_err());
        assert_eq!(coordinator.state(), PairingState::Cancelled);
        responder.await.unwrap();
    }

    #[tokio::test]
    async fn test_pairing_coordinator_timeout_or_rejection() {
        let (tx, mut rx) = mpsc::channel::<RegisterPairingPinRequest>(1);
        let coordinator = PairingCoordinator::new("machine", tx);
        let reject = tokio::spawn(async move {
            let request = rx.recv().await.unwrap();
            request
                .ack
                .send(Ok(RegisterPairingPinAck {
                    generation: request.registration.generation,
                    pin: request.registration.pin,
                    machine_id: "machine".into(),
                    status: "rejected".into(),
                }))
                .unwrap();
        });
        assert!(coordinator
            .generate_pairing(Duration::from_secs(60))
            .await
            .is_err());
        assert_eq!(coordinator.state(), PairingState::Cancelled);
        reject.await.unwrap();
    }

    #[tokio::test]
    async fn test_pairing_coordinator_pin_can_be_redeemed_directly() {
        let (tx, mut rx) = mpsc::channel::<RegisterPairingPinRequest>(1);
        let mut coordinator = PairingCoordinator::new("machine", tx);
        coordinator.auth = AuthManager::with_persistence(None);
        let responder = tokio::spawn(async move {
            let request = rx.recv().await.unwrap();
            request
                .ack
                .send(Ok(RegisterPairingPinAck {
                    generation: request.registration.generation,
                    pin: request.registration.pin,
                    machine_id: "machine".into(),
                    status: "ready".into(),
                }))
                .unwrap();
        });
        let session = coordinator
            .generate_pairing(Duration::from_secs(60))
            .await
            .unwrap();
        // The PIN itself is registered with the local auth authority for direct LAN redemption
        let exchanged = coordinator.auth.exchange_pairing_code_with_installation(
            &session.pin,
            "DirectPhone",
            None,
        );
        assert!(exchanged.is_ok(), "Direct pairing with PIN must succeed");
        coordinator.transition(PairingState::Claimed).unwrap();
        coordinator.transition(PairingState::Consumed).unwrap();
        // The paired token is also cleaned up
        assert!(coordinator.active_pin.read().is_none());
        assert!(coordinator.active_token.read().is_none());
        responder.await.unwrap();

        let (tx, mut rx) = mpsc::channel::<RegisterPairingPinRequest>(1);
        let coordinator = PairingCoordinator::new("machine", tx);
        // Time is the behavior under test: retain the ACK sender without responding.
        let generator = coordinator.clone();
        let generation =
            tokio::spawn(
                async move { generator.generate_pairing(Duration::from_millis(20)).await },
            );
        let request = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(5), generation)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert_eq!(coordinator.state(), PairingState::Expired);
        drop(request);
    }

    #[test]
    fn to_ws_base_rewrites_https_and_trims_trailing_slash() {
        assert_eq!(
            to_ws_base("https://relay.example.com/").unwrap(),
            "wss://relay.example.com"
        );
        assert_eq!(
            to_ws_base("http://localhost:8787").unwrap(),
            "ws://localhost:8787"
        );
        assert_eq!(
            to_ws_base("wss://relay.example.com").unwrap(),
            "wss://relay.example.com"
        );
    }

    #[test]
    fn retired_relay_origin_connects_through_the_current_relay() {
        assert_eq!(
            live_relay_url("https://relay.checka.cc/host/m-1"),
            "https://relay.ferryx.dev/host/m-1"
        );
        assert_eq!(live_relay_url("https://relay.checka.cc"), "https://relay.ferryx.dev");
        assert_eq!(to_ws_base("https://relay.checka.cc").unwrap(), "wss://relay.ferryx.dev");
        // A different host that merely shares the prefix is left alone.
        assert_eq!(
            live_relay_url("https://relay.checka.cc.example/host/m-1"),
            "https://relay.checka.cc.example/host/m-1"
        );
        assert_eq!(live_relay_url("https://relay.example.com"), "https://relay.example.com");
    }

    #[test]
    fn control_and_data_urls_are_built_from_relay_url() {
        let client = RelayClient::new("https://relay.example.com", "tok");
        assert_eq!(
            client.control_url().unwrap(),
            "wss://relay.example.com/tunnel/control"
        );
        assert_eq!(
            client.data_url("abc123").unwrap(),
            "wss://relay.example.com/tunnel/data/abc123"
        );
    }

    #[tokio::test]
    async fn a_relay_url_that_fails_validation_is_never_dialed() {
        // A public plaintext relay is refused by policy; the raw URL must not be used instead.
        let client = RelayClient::new("http://public.example.com", "tok");
        assert!(matches!(
            client.control_url(),
            Err(RelayUrlSecurityError::InsecurePublicRelayForbidden { .. })
        ));
        assert!(matches!(
            client.data_url("s1"),
            Err(RelayUrlSecurityError::InsecurePublicRelayForbidden { .. })
        ));
        let error = client.connect_control().await.unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<RelayUrlSecurityError>()
                .map(RelayUrlSecurityError::code),
            Some("INSECURE_PUBLIC_RELAY_FORBIDDEN")
        );
    }

    #[test]
    fn failover_rotates_control_and_data_urls_together_and_returns_home() {
        let client = RelayClient::new("https://relay.example.com", "tok").with_fallback_urls([
            "https://relay.example.com/",
            " https://standby.example.com/ ",
            "",
            "https://standby.example.com",
        ]);
        assert_eq!(
            *client.fallback_urls,
            vec!["https://standby.example.com".to_string()]
        );
        assert_eq!(client.control_url().unwrap(), "wss://relay.example.com/tunnel/control");

        client.advance_endpoint();
        assert_eq!(client.active_relay_url(), "https://standby.example.com");
        assert_eq!(client.control_url().unwrap(), "wss://standby.example.com/tunnel/control");
        // A clone (the per-session task) follows the same endpoint as the control channel.
        assert_eq!(
            client.clone().data_url("s1").unwrap(),
            "wss://standby.example.com/tunnel/data/s1"
        );

        client.advance_endpoint();
        assert_eq!(client.active_relay_url(), "https://relay.example.com");
    }

    #[test]
    fn reconnect_delays_are_jittered_within_half_to_full_backoff() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let backoff = Duration::from_secs(8);
        let delays: Vec<Duration> = (0..256).map(|_| jittered(backoff, &mut rng)).collect();
        assert!(delays.iter().all(|d| *d >= backoff / 2 && *d <= backoff));
        // Spread across the window, not pinned to the full backoff.
        assert!(delays.iter().any(|d| *d < backoff * 3 / 4));
        assert!(delays.iter().any(|d| *d > backoff * 3 / 4));
    }

    #[test]
    fn only_a_session_that_stayed_up_resets_the_backoff() {
        let client = RelayClient::new("https://relay.example.com", "tok")
            .with_fallback_urls(["https://standby.example.com"]);
        let grown = Duration::from_secs(8);

        // Accepted and dropped at once: keep backing off and try the standby.
        assert_eq!(client.next_backoff(Duration::from_secs(1), grown), grown);
        assert_eq!(client.active_relay_url(), "https://standby.example.com");

        // A session that stayed up was healthy: start over at the primary.
        assert_eq!(
            client.next_backoff(HEALTHY_SESSION_UPTIME, grown),
            INITIAL_BACKOFF
        );
        assert_eq!(client.active_relay_url(), "https://relay.example.com");
    }

    #[test]
    fn single_endpoint_client_never_rotates() {
        let client = RelayClient::new("https://relay.example.com", "tok");
        client.advance_endpoint();
        assert_eq!(client.active_relay_url(), "https://relay.example.com");
    }

    #[test]
    fn fallback_env_rejects_insecure_and_retired_origins() {
        let urls = fallback_relay_urls_from(Some(
            "https://standby.example.com, http://public.example.com,https://relay.checka.cc,,",
        ));
        assert_eq!(urls, vec!["https://standby.example.com".to_string()]);
        assert!(fallback_relay_urls_from(None).is_empty());
    }

    #[test]
    fn with_gateway_preserves_address_and_credentials() {
        let client =
            RelayClient::with_gateway("http://localhost:8787/", "machine", "127.0.0.1:54321");
        assert_eq!(client.gateway_addr, "127.0.0.1:54321");
        assert_eq!(client.machine_token, "machine");
        assert_eq!(client.control_url().unwrap(), "ws://localhost:8787/tunnel/control");
        assert_eq!(
            client.data_url("session").unwrap(),
            "ws://localhost:8787/tunnel/data/session"
        );
        assert_eq!(
            RelayClient::new("http://localhost", "tok").gateway_addr,
            LOCAL_GATEWAY_ADDR
        );
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
            control
                .send(Message::Text(r#"{"session_id":"sess-1"}"#.into()))
                .await
                .unwrap();
            let (data, _) = listener.accept().await.unwrap();
            let data = tokio_tungstenite::accept_async(data).await.unwrap();
            data_tx.send(data).unwrap();
            while let Some(message) = control.next().await {
                if message.is_err() {
                    break;
                }
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
            .await
            .expect("data connection timed out")
            .unwrap();

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
