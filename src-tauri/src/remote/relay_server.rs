//! ferryx-relay: the public relay server that brokers WebSocket tunnels
//! between a desktop daemon (behind NAT/firewall) and a remote client.
//!
//! Three endpoints cooperate to build a session:
//!
//! - `GET /tunnel/control`            - the desktop daemon holds this socket
//!   open for the lifetime of the process. The relay uses it to push
//!   incoming-session notifications so the daemon knows when to open a data
//!   channel.
//! - `GET /tunnel/data/:session_id`   - the desktop daemon opens one of these
//!   per session, in response to a control-channel notification.
//! - `GET /tunnel/client/:session_id` - the remote client opens one of these
//!   to attach to the session. Once both the data and client sockets are
//!   present the relay proxies frames between them, bidirectionally, until
//!   either side disconnects.
//!
//! Control channels authenticate by Ed25519 challenge-response, or by a
//! configured legacy Machine Token. Data and client
//! channels are bound to a `session_id` that is only ever handed out over
//! the (already authenticated) control channel, so they do not need to
//! re-present the Machine Token. An authenticated control socket can send
//! `{"type":"AllocateSession"}` to receive a fresh UUID in an
//! `IncomingSessionNotice`. Trusted in-process callers can also issue via
//! `notify_incoming_session`. Unknown IDs are rejected before upgrade;
//! each issued ID accepts exactly one data half and one client half.
//! Pending and active sessions share a 100-entry budget. Pairing expires
//! after 30 seconds; sends time out after 30 seconds and sessions after one hour.

use crate::remote::auth::{verify_control_challenge, write_private_json};
use crate::remote::protocol::{
    ControlAuth, ControlAuthResponse, ControlChallenge, PairingState, RegisterPairingPin,
    RegisterPairingPinAck, SocketTicketRequest, SocketTicketResponse,
};
use axum::{
    body::{to_bytes, Body},
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Path as AxumPath, Query, State,
    },
    http::{HeaderMap, Method, Request, StatusCode},
    response::Response,
    routing::{any, get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

/// How long a session may sit waiting for its data or client half before
/// the relay gives up and evicts it from the registry.
const SESSION_PAIRING_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PENDING_SESSIONS: usize = 100;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_TRANSFER_TIMEOUT: Duration = Duration::from_secs(3600);
const MAX_MESSAGE_SIZE: usize = 64 * 1024;

/// Interval on which the background reaper sweeps the session registry for
/// entries that have exceeded [`SESSION_PAIRING_TIMEOUT`] without pairing.
const SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(10);

/// Notification pushed down the control channel when a client asks to open
/// a new session against this daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingSessionNotice {
    pub session_id: String,
}

/// Which of the two tunnel halves a pending registry entry holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HalfKind {
    Data,
    Client,
}

/// An issued session, retained through reservation and active transfer.
/// `notify` hands the second socket to the first connection's task.
struct WaitingHalf {
    generation: u64,
    created_at: Instant,
    kind: Option<HalfKind>,
    notify: Option<oneshot::Sender<WebSocket>>,
    active: bool,
}

struct ControlChannel {
    generation: u64,
    tx: mpsc::Sender<IncomingSessionNotice>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ControlRequest {
    AllocateSession,
    RegisterPairingPin(RegisterPairingPin),
}

struct RegisteredPairing {
    registration: RegisterPairingPin,
    state: PairingState,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicPairExchangeRequest {
    pub pin: Option<String>,
    pub code: Option<String>,
    pub pairing_token: Option<String>,
    pub device_name: String,
}

/// Also cleans up failed upgrades and cancelled connection tasks.
struct SessionGuard {
    armed: bool,
    state: RelayState,
    session_id: String,
    generation: u64,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if self.armed {
            self.state.remove_pending(&self.session_id, self.generation);
        }
    }
}

/// Shared relay state: the set of authorized Machine Tokens, the live
/// control channels (one per connected daemon), and the registry of
/// sessions awaiting pairing.
#[derive(Clone)]
pub struct RelayState {
    inner: Arc<RelayInner>,
}

struct RelayInner {
    machine_tokens: Vec<String>,
    control_channels: Mutex<HashMap<String, ControlChannel>>,
    machine_public_keys: Mutex<HashMap<String, String>>,
    key_store_path: Option<std::path::PathBuf>,
    admission: Mutex<HashMap<IpAddr, AttemptTracker>>,
    pairing_admission: Mutex<HashMap<IpAddr, AttemptTracker>>,
    pairings: Mutex<HashMap<String, RegisteredPairing>>,
    next_generation: AtomicU64,
    pending_sessions: Mutex<HashMap<String, WaitingHalf>>,
    pending_socket_tickets: Mutex<HashMap<String, (String, String, String, u64)>>,
}

const ADMISSION_WINDOW: Duration = Duration::from_secs(60);
const CONTROL_AUTH_TIMEOUT: Duration = Duration::from_secs(5);

struct AttemptTracker {
    started: Instant,
    attempts: u32,
    failures: u32,
    locked_until: Option<Instant>,
}

fn current_time_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before Unix epoch")
        .as_secs()
}

/// Outcome of offering a socket to the pairing registry.
enum PairingOutcome {
    /// The opposite half is reserved; send it the socket after upgrade.
    HandOff(oneshot::Sender<WebSocket>),
    /// The first half is reserved and awaits the counterpart after upgrade.
    Waiting { rx: oneshot::Receiver<WebSocket> },
}

impl RelayState {
    pub fn new(machine_tokens: Vec<String>) -> Self {
        let path = std::env::var_os("FERRYX_RELAY_KEY_FILE").map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("FERRYX_RELAY_DATA_DIR").map(|dir| std::path::PathBuf::from(dir).join("machine_keys.json")))
            .or_else(|| std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
                .map(|dir| std::path::PathBuf::from(dir).join(".ferryx/relay/machine_keys.json")))
            .expect("Cannot resolve relay machine key store");
        Self::new_with_key_store(machine_tokens, path).expect("Failed to load relay machine key store")
    }

    pub fn new_with_key_store(machine_tokens: Vec<String>, path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        Self::with_key_store(machine_tokens, Some(path.as_ref().to_path_buf()))
    }

    fn with_key_store(machine_tokens: Vec<String>, key_store_path: Option<std::path::PathBuf>) -> std::io::Result<Self> {
        let keys = match key_store_path.as_ref().map(std::fs::read).transpose() {
            Ok(Some(bytes)) => serde_json::from_slice(&bytes)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
            Ok(None) => HashMap::new(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => return Err(error),
        };
        Ok(Self {
            inner: Arc::new(RelayInner {
                machine_tokens,
                next_generation: AtomicU64::new(1),
                control_channels: Mutex::new(HashMap::new()),
                machine_public_keys: Mutex::new(keys),
                key_store_path,
                admission: Mutex::new(HashMap::new()),
                pairing_admission: Mutex::new(HashMap::new()),
                pairings: Mutex::new(HashMap::new()),
                pending_sessions: Mutex::new(HashMap::new()),
                pending_socket_tickets: Mutex::new(HashMap::new()),
            }),
        })
    }

    fn bind_machine_key(&self, auth: &ControlAuth) -> Result<(), String> {
        // Serialize ownership checks and durable enrollment under the same lock.
        let mut keys = self.inner.machine_public_keys.lock();
        if let Some(key) = keys.get(&auth.machine_id) {
            return if key == &auth.public_key { Ok(()) } else {
                Err("Machine ID already claimed by another public key".into())
            };
        }
        if !self.inner.machine_tokens.is_empty()
            && !auth.enrollment_token.as_deref().is_some_and(|token| self.validate_machine_token(token))
        {
            return Err("Enrollment token required for private relay".into());
        }
        let mut enrolled = keys.clone();
        enrolled.insert(auth.machine_id.clone(), auth.public_key.clone());
        if let Some(path) = &self.inner.key_store_path {
            write_private_json(path, &enrolled).map_err(|error| format!("Failed to persist machine ownership: {error}"))?;
        }
        *keys = enrolled;
        Ok(())
    }

    fn admit(&self, ip: IpAddr, now: Instant) -> bool {
        let mut admission = self.inner.admission.lock();
        admission.retain(|_, tracker| {
            now.duration_since(tracker.started) < ADMISSION_WINDOW
                || tracker.locked_until.is_some_and(|until| now < until)
        });
        let tracker = admission.entry(ip).or_insert(AttemptTracker {
            started: now,
            attempts: 0,
            failures: 0,
            locked_until: None,
        });
        if tracker.locked_until.is_some_and(|until| now < until) {
            return false;
        }
        if now.duration_since(tracker.started) >= ADMISSION_WINDOW {
            tracker.started = now;
            tracker.attempts = 0;
            tracker.failures = 0;
            tracker.locked_until = None;
        }
        if tracker.attempts >= 30 {
            return false;
        }
        tracker.attempts += 1;
        true
    }

    fn record_auth(&self, ip: IpAddr, success: bool) {
        if let Some(tracker) = self.inner.admission.lock().get_mut(&ip) {
            if success {
                tracker.failures = 0;
            } else {
                tracker.failures += 1;
                if tracker.failures >= 5 {
                    tracker.locked_until = Some(Instant::now() + ADMISSION_WINDOW);
                }
            }
        }
    }

    fn validate_machine_token(&self, token: &str) -> bool {
        if self.inner.machine_tokens.is_empty() {
            // No configured tokens means the relay has nothing to check
            // against; reject explicitly rather than silently accepting
            // anything.
            return false;
        }
        self.inner
            .machine_tokens
            .iter()
            .any(|candidate| candidate == token)
    }

    fn register_control_channel(
        &self,
        machine_id: String,
    ) -> (u64, mpsc::Receiver<IncomingSessionNotice>) {
        let (tx, rx) = mpsc::channel(MAX_PENDING_SESSIONS);
        let generation = self.inner.next_generation.fetch_add(1, Ordering::Relaxed);
        self.inner
            .control_channels
            .lock()
            .insert(machine_id, ControlChannel { generation, tx });
        (generation, rx)
    }

    fn unregister_control_channel(&self, machine_token: &str, generation: u64) {
        let mut channels = self.inner.control_channels.lock();
        if channels
            .get(machine_token)
            .is_some_and(|channel| channel.generation == generation)
        {
            channels.remove(machine_token);
        }
    }

    /// Trusted in-process issuance API. The caller must supply an opaque,
    /// unpredictable ID and a machine identity with a live control channel.
    /// Registry insertion and notification are atomic with respect to admission.
    pub fn notify_incoming_session(&self, machine_token: &str, session_id: &str) -> bool {
        self.issue_session(machine_token, session_id, None)
    }

    fn issue_session(
        &self,
        machine_token: &str,
        session_id: &str,
        generation: Option<u64>,
    ) -> bool {
        let channels = self.inner.control_channels.lock();
        let Some(channel) = channels.get(machine_token) else {
            return false;
        };
        if generation.is_some_and(|id| id != channel.generation) {
            return false;
        }
        let mut sessions = self.inner.pending_sessions.lock();
        if sessions.len() >= MAX_PENDING_SESSIONS || sessions.contains_key(session_id) {
            return false;
        }
        if channel
            .tx
            .try_send(IncomingSessionNotice {
                session_id: session_id.into(),
            })
            .is_err()
        {
            return false;
        }
        sessions.insert(
            session_id.into(),
            WaitingHalf {
                generation: self.inner.next_generation.fetch_add(1, Ordering::Relaxed),
                created_at: Instant::now(),
                kind: None,
                notify: None,
                active: false,
            },
        );
        true
    }

    /// Reserve before upgrade so concurrent duplicates receive HTTP 409.
    fn reserve_half(
        &self,
        session_id: &str,
        kind: HalfKind,
    ) -> Result<(u64, PairingOutcome), StatusCode> {
        let mut sessions = self.inner.pending_sessions.lock();
        let waiting = sessions.get_mut(session_id).ok_or(StatusCode::NOT_FOUND)?;
        if waiting.active || waiting.kind == Some(kind) {
            return Err(StatusCode::CONFLICT);
        }
        if waiting.created_at.elapsed() >= SESSION_PAIRING_TIMEOUT {
            return Err(StatusCode::GONE);
        }
        let outcome = if let Some(tx) = waiting.notify.take() {
            waiting.active = true;
            PairingOutcome::HandOff(tx)
        } else {
            let (tx, rx) = oneshot::channel();
            waiting.kind = Some(kind);
            waiting.notify = Some(tx);
            PairingOutcome::Waiting { rx }
        };
        Ok((waiting.generation, outcome))
    }

    fn remove_pending(&self, session_id: &str, generation: u64) {
        let mut sessions = self.inner.pending_sessions.lock();
        if sessions
            .get(session_id)
            .is_some_and(|entry| entry.generation == generation)
        {
            sessions.remove(session_id);
        }
    }

    /// Sweeps the pending-session registry, dropping any entry that has
    /// been waiting longer than [`SESSION_PAIRING_TIMEOUT`]. Dropping the
    /// entry drops its `notify` sender, which causes the waiting task's
    /// `rx.await` to resolve to an error so it can close its socket.
    fn sweep_expired_sessions(&self) {
        let now = Instant::now();
        self.inner.pending_socket_tickets.lock().retain(|_, (_, _, _, expiry)| *expiry > current_time_secs());
        self.inner.pairings.lock().retain(|_, p| p.registration.expires_at > current_time_secs());
        self.inner.admission.lock().retain(|_, tracker| {
            now.duration_since(tracker.started) < ADMISSION_WINDOW
                || tracker.locked_until.is_some_and(|until| now < until)
        });
        let mut sessions = self.inner.pending_sessions.lock();
        sessions.retain(|_, waiting| {
            waiting.active || waiting.created_at.elapsed() < SESSION_PAIRING_TIMEOUT
        });
    }
}

impl RelayState {
    fn register_pairing(
        &self,
        machine: &str,
        generation: u64,
        registration: RegisterPairingPin,
    ) -> Result<(), &'static str> {
        let channels = self.inner.control_channels.lock();
        if !channels
            .get(machine)
            .is_some_and(|c| c.generation == generation)
            || registration.machine_id != machine
            || registration.pin.len() != 6
            || !registration.pin.bytes().all(|b| b.is_ascii_digit())
            || registration.pairing_token.is_empty()
            || registration.expires_at <= current_time_secs()
        {
            return Err("Invalid pairing registration");
        }
        let mut pairings = self.inner.pairings.lock();
        pairings.retain(|_, p| p.registration.expires_at > current_time_secs());
        if pairings.contains_key(&registration.pin)
            || pairings
                .values()
                .any(|p| p.registration.pairing_token == registration.pairing_token)
        {
            return Err("Pairing code already registered");
        }
        if pairings.values().filter(|p| p.registration.machine_id == machine).count()
            >= MAX_ACTIVE_PAIRING_PINS_PER_MACHINE
        {
            return Err("Maximum active pairing codes exceeded");
        }
        pairings.insert(
            registration.pin.clone(),
            RegisteredPairing {
                registration,
                state: PairingState::Ready,
            },
        );
        Ok(())
    }

    fn claim_pairing(
        &self,
        ip: IpAddr,
        payload: &PublicPairExchangeRequest,
        machine: Option<&str>,
    ) -> Result<(String, String, String), StatusCode> {
        let now = Instant::now();
        let mut admission = self.inner.pairing_admission.lock();
        admission.retain(|_, t| {
            now.duration_since(t.started) < ADMISSION_WINDOW
                || t.locked_until.is_some_and(|until| now < until)
        });
        let tracker = admission.entry(ip).or_insert(AttemptTracker {
            started: now,
            attempts: 0,
            failures: 0,
            locked_until: None,
        });
        if tracker.locked_until.is_some_and(|until| now < until) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let mut pairings = self.inner.pairings.lock();
        let pairing = pairings.values_mut().find(|p| {
            if let Some(token) = &payload.pairing_token {
                &p.registration.pairing_token == token
            } else {
                payload.pin.as_ref().map_or_else(
                    || payload.code.as_ref().is_some_and(|code| {
                        code == &p.registration.pin || code == &p.registration.pairing_token
                    }),
                    |pin| pin == &p.registration.pin,
                )
            }
        });
        if let Some(p) = pairing {
            if p.state == PairingState::Ready
                && p.registration.expires_at > current_time_secs()
                && machine.is_none_or(|m| m == p.registration.machine_id)
            {
                p.state = PairingState::Claimed;
                // A caller can register its own known PINs. A match must not
                // replenish its guessing budget before daemon authentication.
                return Ok((
                    p.registration.machine_id.clone(),
                    p.registration.pin.clone(),
                    p.registration.pairing_token.clone(),
                ));
            }
        }
        tracker.failures += 1;
        if tracker.failures >= 5 {
            tracker.locked_until = Some(now + ADMISSION_WINDOW);
        }
        Err(StatusCode::NOT_FOUND)
    }

    /// Reserve the relay's HTTP half before notifying the daemon, so even an
    /// immediate data connection hands its socket directly to this request.
    async fn open_session_channel(
        &self,
        machine: &str,
    ) -> Result<(WebSocket, SessionGuard), StatusCode> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        let generation = self.inner.next_generation.fetch_add(1, Ordering::Relaxed);
        let guard = SessionGuard {
            armed: true,
            state: self.clone(),
            session_id: session_id.clone(),
            generation,
        };
        {
            let channels = self.inner.control_channels.lock();
            let channel = channels.get(machine).ok_or(StatusCode::NOT_FOUND)?;
            let mut sessions = self.inner.pending_sessions.lock();
            if sessions.len() >= MAX_PENDING_SESSIONS {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
            sessions.insert(
                session_id.clone(),
                WaitingHalf {
                    generation,
                    created_at: Instant::now(),
                    kind: Some(HalfKind::Client),
                    notify: Some(tx),
                    active: false,
                },
            );
            channel
                .tx
                .try_send(IncomingSessionNotice { session_id })
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        }
        let socket = timeout(SESSION_PAIRING_TIMEOUT, rx)
            .await
            .map_err(|_| StatusCode::GATEWAY_TIMEOUT)?
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        Ok((socket, guard))
    }
}

const SOCKET_TICKET_TTL: u64 = 30;
const MAX_ACTIVE_PAIRING_PINS_PER_MACHINE: usize = 5;

async fn socket_ticket_handler(
    State(state): State<RelayState>,
    AxumPath(machine): AxumPath<String>,
    headers: HeaderMap,
    Json(mut body): Json<serde_json::Value>,
) -> Result<Json<SocketTicketResponse>, StatusCode> {
    let device_token = headers.get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty() && token.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(&b)))
        .ok_or(StatusCode::UNAUTHORIZED)?.to_owned();
    // The public route supplies machineId; the shared wire type also supports
    // callers that include it explicitly, but must not permit a conflicting ID.
    let object = body.as_object_mut().ok_or(StatusCode::BAD_REQUEST)?;
    object.entry("machineId").or_insert_with(|| machine.clone().into());
    let request: SocketTicketRequest = serde_json::from_value(body)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if request.machine_id != machine || !valid_socket_target(&request.target) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !state.inner.control_channels.lock().get(&machine).is_some_and(|c| !c.tx.is_closed()) {
        return Err(StatusCode::NOT_FOUND);
    }
    let now = current_time_secs();
    let mut tickets = state.inner.pending_socket_tickets.lock();
    tickets.retain(|_, (_, _, _, expiry)| *expiry > now);
    if tickets.len() >= MAX_PENDING_SESSIONS {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let ticket = uuid::Uuid::new_v4().to_string();
    let expires_at = now + SOCKET_TICKET_TTL;
    tickets.insert(ticket.clone(), (machine, request.target, device_token, expires_at));
    Ok(Json(SocketTicketResponse { ticket, expires_at }))
}

fn valid_socket_target(target: &str) -> bool {
    target == "/api/v1/events" || target.strip_prefix("/api/v1/terminal/").is_some_and(|id| {
        !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~'))
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SocketQuery {
    ticket: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalSocketQuery {
    pub ticket: String,
    pub render: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

fn consume_socket_ticket(state: &RelayState, query: Result<Query<SocketQuery>, axum::extract::rejection::QueryRejection>, machine: &str, target: &str) -> Result<String, StatusCode> {
    // Unknown query keys (including token/access_token/authorization) are
    // forbidden, even when accompanied by a valid one-time ticket.
    let Query(query) = query.map_err(|_| StatusCode::UNAUTHORIZED)?;
    let ticket = query.ticket.ok_or(StatusCode::UNAUTHORIZED)?;
    let (issued_machine, issued_target, device_token, expiry) = state.inner.pending_socket_tickets.lock()
        .remove(&ticket).ok_or(StatusCode::UNAUTHORIZED)?;
    if issued_machine != machine || issued_target != target || expiry <= current_time_secs() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(device_token)
}

async fn browser_events_handler(
    State(state): State<RelayState>,
    AxumPath(machine): AxumPath<String>,
    query: Result<Query<SocketQuery>, axum::extract::rejection::QueryRejection>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    let target = "/api/v1/events";
    let device_token = consume_socket_ticket(&state, query, &machine, target)?;
    browser_socket(state, machine, target.into(), device_token, ws).await
}

async fn browser_terminal_handler(
    State(state): State<RelayState>,
    AxumPath((machine, terminal)): AxumPath<(String, String)>,
    query: Result<Query<TerminalSocketQuery>, axum::extract::rejection::QueryRejection>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    let Query(query) = query.map_err(|_| StatusCode::UNAUTHORIZED)?;
    let target = format!("/api/v1/terminal/{terminal}");
    let device_token = consume_socket_ticket(&state, Ok(Query(SocketQuery { ticket: Some(query.ticket) })), &machine, &target)?;
    let mut uri = reqwest::Url::parse(&format!("ws://localhost{target}")).map_err(|_| StatusCode::BAD_REQUEST)?;
    if query.render.is_some() || query.cols.is_some() || query.rows.is_some() {
        let mut params = uri.query_pairs_mut();
        if let Some(render) = query.render { params.append_pair("render", &render); }
        if let Some(cols) = query.cols { params.append_pair("cols", &cols.to_string()); }
        if let Some(rows) = query.rows { params.append_pair("rows", &rows.to_string()); }
    }
    let target = match uri.query() {
        Some(query) => format!("{}?{query}", uri.path()),
        None => uri.path().to_owned(),
    };
    browser_socket(state, machine, target, device_token, ws).await
}

async fn browser_socket(
    state: RelayState,
    machine: String,
    target: String,
    device_token: String,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    let (data, guard) = state.open_session_channel(&machine).await?;
    Ok(ws.max_message_size(MAX_MESSAGE_SIZE).max_frame_size(MAX_MESSAGE_SIZE)
        .on_upgrade(move |browser| async move {
            match timeout(SESSION_TRANSFER_TIMEOUT, bridge_browser_socket(browser, data, &target, &device_token)).await {
                Ok(Ok(())) => {},
                Ok(Err(error)) => tracing::warn!(%error, "browser reverse WebSocket failed"),
                Err(error) => tracing::warn!(%error, "browser reverse WebSocket lifetime exceeded"),
            }
            drop(guard);
        }))
}

/// The existing daemon unwraps tunnel Binary messages onto a TCP stream.
/// Use a bounded duplex stream to run a real WebSocket client over that stream,
/// including its HTTP upgrade, masking, fragmentation and control frames.
async fn bridge_browser_socket(mut browser: WebSocket, mut data: WebSocket, target: &str, device_token: &str) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_tungstenite::tungstenite::Message as Wire;
    let (client_io, tunnel_io) = tokio::io::duplex(MAX_MESSAGE_SIZE);
    let (mut reader, mut writer) = tokio::io::split(tunnel_io);
    let tunnel = async {
        let mut buffer = vec![0; MAX_MESSAGE_SIZE];
        loop {
            tokio::select! {
                count = reader.read(&mut buffer) => {
                    let count = count?;
                    if count == 0 {
                        timeout(TRANSFER_TIMEOUT, data.send(Message::Close(None))).await??;
                        return Ok::<(), anyhow::Error>(());
                    }
                    timeout(TRANSFER_TIMEOUT, data.send(Message::Binary(buffer[..count].to_vec().into()))).await??;
                }
                frame = data.recv() => match frame {
                    Some(Ok(Message::Binary(bytes))) => timeout(TRANSFER_TIMEOUT, writer.write_all(&bytes)).await??,
                    Some(Ok(Message::Text(text))) => timeout(TRANSFER_TIMEOUT, writer.write_all(text.as_bytes())).await??,
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Err(error)) => return Err(error.into()),
                    Some(Ok(_)) => {},
                }
            }
        }
    };
    let application = async {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = format!("ws://localhost{target}").into_client_request()?;
        request.headers_mut().insert("authorization", format!("Bearer {device_token}").parse()?);
        let (mut daemon, _) = timeout(TRANSFER_TIMEOUT, tokio_tungstenite::client_async(request, client_io)).await??;
        loop {
            tokio::select! {
                frame = browser.recv() => {
                    let Some(frame) = frame else { return Ok::<(), anyhow::Error>(()); };
                    let frame = match frame? {
                        Message::Text(text) => Wire::Text(text.to_string().into()),
                        Message::Binary(bytes) => Wire::Binary(bytes),
                        Message::Ping(bytes) => Wire::Ping(bytes),
                        Message::Pong(bytes) => Wire::Pong(bytes),
                        Message::Close(frame) => Wire::Close(frame.map(|f| tokio_tungstenite::tungstenite::protocol::CloseFrame { code: f.code.into(), reason: f.reason.to_string().into() })),
                    };
                    let close = frame.is_close();
                    timeout(TRANSFER_TIMEOUT, daemon.send(frame)).await??;
                    if close { return Ok(()); }
                }
                frame = daemon.next() => {
                    let Some(frame) = frame else { return Ok(()); };
                    let frame = match frame? {
                        Wire::Text(text) => Message::Text(text.to_string().into()),
                        Wire::Binary(bytes) => Message::Binary(bytes),
                        Wire::Ping(bytes) => Message::Ping(bytes),
                        Wire::Pong(bytes) => Message::Pong(bytes),
                        Wire::Close(frame) => Message::Close(frame.map(|f| axum::extract::ws::CloseFrame { code: f.code.into(), reason: f.reason.to_string().into() })),
                        Wire::Frame(_) => continue,
                    };
                    let close = matches!(frame, Message::Close(_));
                    timeout(TRANSFER_TIMEOUT, browser.send(frame)).await??;
                    if close { return Ok(()); }
                }
            }
        }
    };
    tokio::pin!(tunnel);
    tokio::select! {
        result = &mut tunnel => result,
        result = application => {
            result?;
            // Flush the inner close frame before releasing the reverse channel.
            timeout(TRANSFER_TIMEOUT, &mut tunnel).await?
        },
    }
}

async fn pair_exchange_handler(
    State(state): State<RelayState>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    request: Request<Body>,
) -> Result<Response, StatusCode> {
    exchange_http(state, peer_ip(peer), None, request).await
}

async fn host_http_handler(
    State(state): State<RelayState>,
    AxumPath((machine, path)): AxumPath<(String, String)>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    request: Request<Body>,
) -> Result<Response, StatusCode> {
    if !(matches!(path.as_str(), "events" | "socket-ticket" | "pair/exchange")
        || ["workspace/", "terminal/", "push/"].iter().any(|prefix| path.starts_with(prefix)))
        || path.split('/').any(|part| matches!(part, "." | "..") || part.contains(['%', '\\']))
    {
        return Err(StatusCode::FORBIDDEN);
    }
    if path == "pair/exchange" && request.method() == Method::POST {
        return exchange_http(state, peer_ip(peer), Some(&machine), request).await;
    }
    let uri_path = request
        .uri()
        .path()
        .strip_prefix(&format!("/host/{machine}"))
        .ok_or(StatusCode::BAD_REQUEST)?;
    let target = match request.uri().query() {
        Some(query) => format!("{uri_path}?{query}"),
        None => uri_path.to_owned(),
    };
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, MAX_HTTP_SIZE)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    proxy_http(
        &state,
        &machine,
        parts.method,
        &target,
        parts.headers,
        body.to_vec(),
    )
    .await
}

const MAX_HTTP_SIZE: usize = 8 * 1024 * 1024;

async fn exchange_http(
    state: RelayState,
    ip: IpAddr,
    machine: Option<&str>,
    request: Request<Body>,
) -> Result<Response, StatusCode> {
    let body = to_bytes(request.into_body(), MAX_MESSAGE_SIZE)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let payload: PublicPairExchangeRequest =
        serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let (machine, pin, token) = state.claim_pairing(ip, &payload, machine)?;
    // The loopback gateway calls its pairing secret `code`.
    let body = serde_json::to_vec(&serde_json::json!({"code": token, "pairingToken": token, "deviceName": payload.device_name})).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());
    let response = proxy_http(
        &state,
        &machine,
        Method::POST,
        "/api/v1/pair/exchange",
        headers,
        body,
    )
    .await?;
    if response.status().is_success() {
        if let Some(p) = state.inner.pairings.lock().get_mut(&pin) {
            if p.state == PairingState::Claimed && p.registration.pairing_token == token {
                p.state = PairingState::Consumed;
            }
        }
    }
    Ok(response)
}

fn hop_header(name: &str, headers: &HeaderMap) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    ) || headers.get_all("connection").iter().any(|value| {
        value
            .to_str()
            .is_ok_and(|v| v.split(',').any(|h| h.trim().eq_ignore_ascii_case(name)))
    })
}

async fn proxy_http(
    state: &RelayState,
    machine: &str,
    method: Method,
    path: &str,
    headers: HeaderMap,
    body: Vec<u8>,
) -> Result<Response, StatusCode> {
    let transfer = async {
        let (mut socket, _guard) = state.open_session_channel(machine).await?;
        let mut raw = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n", body.len()).into_bytes();
        for (name, value) in &headers {
            if name == "host" || name == "content-length" || hop_header(name.as_str(), &headers) {
                continue;
            }
            raw.extend_from_slice(name.as_str().as_bytes());
            raw.extend_from_slice(b": ");
            raw.extend_from_slice(value.as_bytes());
            raw.extend_from_slice(b"\r\n");
        }
        raw.extend_from_slice(b"\r\n");
        raw.extend(body);
        for chunk in raw.chunks(MAX_MESSAGE_SIZE) {
            socket
                .send(Message::Binary(chunk.to_vec().into()))
                .await
                .map_err(|_| StatusCode::BAD_GATEWAY)?;
        }
        let mut response = Vec::new();
        loop {
            let eof = match socket.recv().await {
                Some(Ok(Message::Binary(bytes))) => {
                    response.extend_from_slice(&bytes);
                    false
                }
                Some(Ok(Message::Text(text))) => {
                    response.extend_from_slice(text.as_bytes());
                    false
                }
                Some(Ok(Message::Close(_))) | None => true,
                Some(Ok(_)) => continue,
                Some(Err(_)) => return Err(StatusCode::BAD_GATEWAY),
            };
            if response.len() > MAX_HTTP_SIZE {
                return Err(StatusCode::BAD_GATEWAY);
            }
            if let Some(parsed) = parse_http_response(&response, method == Method::HEAD, eof)? {
                return Ok(parsed);
            }
            if eof {
                return Err(StatusCode::BAD_GATEWAY);
            }
        }
    };
    timeout(TRANSFER_TIMEOUT, transfer)
        .await
        .map_err(|_| StatusCode::GATEWAY_TIMEOUT)?
}

/// Decode HTTP framing independently of WebSocket message boundaries.
fn parse_http_response(raw: &[u8], head: bool, eof: bool) -> Result<Option<Response>, StatusCode> {
    let bad = StatusCode::BAD_GATEWAY;
    let Some(end) = raw.windows(4).position(|b| b == b"\r\n\r\n") else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&raw[..end]).map_err(|_| bad)?;
    let mut lines = text.split("\r\n");
    let mut status_line = lines.next().ok_or(bad)?.split_whitespace();
    if !matches!(status_line.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
        return Err(bad);
    }
    let status = StatusCode::from_u16(status_line.next().ok_or(bad)?.parse().map_err(|_| bad)?)
        .map_err(|_| bad)?;
    if status.is_informational() {
        if status == StatusCode::SWITCHING_PROTOCOLS {
            return Err(bad);
        }
        return parse_http_response(&raw[end + 4..], head, eof);
    }
    let mut headers = HeaderMap::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(bad)?;
        headers.append(
            name.parse::<axum::http::HeaderName>().map_err(|_| bad)?,
            value.trim().parse().map_err(|_| bad)?,
        );
    }
    let data = &raw[end + 4..];
    let body = if head || status == StatusCode::NO_CONTENT || status == StatusCode::NOT_MODIFIED {
        Vec::new()
    } else if let Some(encoding) = headers.get("transfer-encoding") {
        if encoding != "chunked" {
            return Err(bad);
        }
        let mut remaining = data;
        let mut decoded = Vec::new();
        loop {
            let Some(end) = remaining.windows(2).position(|b| b == b"\r\n") else {
                return Ok(None);
            };
            let size = std::str::from_utf8(&remaining[..end])
                .map_err(|_| bad)?
                .split(';')
                .next()
                .ok_or(bad)?;
            let size = usize::from_str_radix(size, 16).map_err(|_| bad)?;
            remaining = &remaining[end + 2..];
            if size == 0 {
                if !remaining.starts_with(b"\r\n")
                    && !remaining.windows(4).any(|b| b == b"\r\n\r\n")
                {
                    return Ok(None);
                }
                break;
            }
            if size > MAX_HTTP_SIZE {
                return Err(bad);
            }
            if remaining.len() < size + 2 {
                return Ok(None);
            }
            if &remaining[size..size + 2] != b"\r\n" {
                return Err(bad);
            }
            decoded.extend_from_slice(&remaining[..size]);
            remaining = &remaining[size + 2..];
        }
        decoded
    } else if let Some(length) = headers.get("content-length") {
        let length: usize = length.to_str().map_err(|_| bad)?.parse().map_err(|_| bad)?;
        if length > MAX_HTTP_SIZE {
            return Err(bad);
        }
        if data.len() < length {
            return Ok(None);
        }
        data[..length].to_vec()
    } else {
        if !eof {
            return Ok(None);
        }
        data.to_vec()
    };
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    for (name, value) in &headers {
        if name != "content-length" && !hop_header(name.as_str(), &headers) {
            response.headers_mut().append(name.clone(), value.clone());
        }
    }
    let headers = response.headers_mut();
    headers.remove("set-cookie");
    headers.remove("service-worker-allowed");
    if headers.get_all("content-type").iter().any(|value| {
        value.to_str().is_ok_and(|value| value.to_ascii_lowercase().contains("text/html"))
    }) {
        headers.insert("content-type", "text/plain".parse().unwrap());
    }
    headers.insert("content-security-policy", "default-src 'none'".parse().unwrap());
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    Ok(Some(response))
}

/// Builds the Axum router exposing the three tunnel endpoints.
pub fn relay_router(state: RelayState) -> Router {
    Router::new()
        .route("/api/v1/pair/exchange", post(pair_exchange_handler))
        .route("/host/{machine_id}/api/v1/socket-ticket", post(socket_ticket_handler))
        .route("/host/{machine_id}/api/v1/events", get(browser_events_handler))
        .route("/host/{machine_id}/api/v1/terminal/{terminal_id}", get(browser_terminal_handler))
        .route("/host/{machine_id}/api/v1/{*path}", any(host_http_handler))
        .route("/tunnel/control", get(control_handler))
        .route("/tunnel/data/{session_id}", get(data_handler))
        .route("/tunnel/client/{session_id}", get(client_handler))
        .with_state(state)
}

/// Spawns the background task that periodically evicts sessions which
/// never paired within [`SESSION_PAIRING_TIMEOUT`].
pub fn spawn_session_reaper(state: RelayState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(SESSION_SWEEP_INTERVAL).await;
            state.sweep_expired_sessions();
        }
    });
}

fn extract_bearer_token(headers: &HeaderMap, query_token: Option<&str>) -> Option<String> {
    if let Some(value) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(text) = value.to_str() {
            if let Some(token) = text.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }
    query_token.map(|s| s.to_string())
}

#[derive(Debug, Deserialize)]
struct AuthQuery {
    token: Option<String>,
}

/// `GET /tunnel/control` - the desktop daemon's long-lived control channel.
///
/// Without a legacy bearer token, the upgraded socket must prove ownership
/// of its public key before it can allocate or receive sessions.
async fn control_handler(
    ws: WebSocketUpgrade,
    axum::extract::Query(query): axum::extract::Query<AuthQuery>,
    headers: HeaderMap,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    State(state): State<RelayState>,
) -> Result<Response, (StatusCode, String)> {
    let ip = peer_ip(peer);
    if !state.admit(ip, Instant::now()) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "Admission limit exceeded".into(),
        ));
    }
    let token = extract_bearer_token(&headers, query.token.as_deref());
    if let Some(token) = &token {
        // Legacy identities remain token-keyed. Never allow a bearer to replace
        // a previously bound Ed25519 identity, even if their strings coincide.
        if !state.validate_machine_token(token)
            || state.inner.machine_public_keys.lock().contains_key(token)
        {
            state.record_auth(ip, false);
            return Err((StatusCode::UNAUTHORIZED, "Invalid Machine Token".into()));
        }
    }

    Ok(ws
        .max_message_size(MAX_MESSAGE_SIZE)
        .max_frame_size(MAX_MESSAGE_SIZE)
        .on_upgrade(move |socket| authenticate_control_socket(socket, state, token, ip)))
}

// Do not trust forwarding headers. Servers must supply ConnectInfo from the
// accepted TCP socket; embeddings without it share one conservative bucket.
fn peer_ip(peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>) -> IpAddr {
    peer.map(|axum::Extension(ConnectInfo(addr))| addr.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

async fn authenticate_control_socket(
    mut socket: WebSocket,
    state: RelayState,
    token: Option<String>,
    ip: IpAddr,
) {
    if let Some(token) = token {
        state.record_auth(ip, true);
        handle_control_socket(socket, state, token).await;
        return;
    }
    let challenge = ControlChallenge {
        nonce: uuid::Uuid::new_v4().to_string(),
        timestamp: current_time_secs(),
    };
    let authenticate = async {
        socket
            .send(Message::Text(
                serde_json::to_string(&challenge).unwrap().into(),
            ))
            .await
            .map_err(|error| error.to_string())?;
        let auth = match socket.recv().await {
            Some(Ok(Message::Text(text))) => serde_json::from_str::<ControlAuth>(&text)
                .map_err(|_| "Invalid ControlAuth frame".to_string())?,
            _ => return Err("Expected ControlAuth text frame".to_string()),
        };
        if auth.machine_id.is_empty() || state.validate_machine_token(&auth.machine_id) {
            return Err("Invalid or reserved machine ID".to_string());
        }
        if current_time_secs().abs_diff(auth.timestamp) > 60 {
            return Err("Authentication timestamp expired".to_string());
        }
        if auth.timestamp != challenge.timestamp || !verify_control_challenge(
            &auth.public_key,
            &auth.machine_id,
            "relay",
            &challenge.nonce,
            auth.timestamp,
            &auth.signature,
        ) {
            return Err("Invalid machine signature".to_string());
        }
        state.bind_machine_key(&auth)?;
        Ok(auth.machine_id)
    };
    let result = match timeout(CONTROL_AUTH_TIMEOUT, authenticate).await {
        Ok(result) => result,
        Err(_) => Err("Control authentication timed out".to_string()),
    };
    state.record_auth(ip, result.is_ok());
    let response = ControlAuthResponse {
        success: result.is_ok(),
        error: result.as_ref().err().cloned(),
    };
    if !matches!(
        timeout(
            CONTROL_AUTH_TIMEOUT,
            socket.send(Message::Text(
                serde_json::to_string(&response).unwrap().into()
            ))
        )
        .await,
        Ok(Ok(()))
    ) {
        tracing::warn!("relay authentication response send failed or timed out");
        return;
    }
    match result {
        Ok(machine_id) => handle_control_socket(socket, state, machine_id).await,
        Err(_) => {
            if !matches!(
                timeout(CONTROL_AUTH_TIMEOUT, socket.close()).await,
                Ok(Ok(()))
            ) {
                tracing::debug!("relay rejected control socket close failed or timed out");
            }
        }
    }
}

async fn handle_control_socket(mut socket: WebSocket, state: RelayState, machine_token: String) {
    let (generation, mut notices) = state.register_control_channel(machine_token.clone());
    loop {
        tokio::select! {
            notice = notices.recv() => {
                let Some(notice) = notice else { break };
                let Ok(payload) = serde_json::to_string(&notice) else { continue };
                if !matches!(timeout(TRANSFER_TIMEOUT, socket.send(Message::Text(payload.into()))).await, Ok(Ok(()))) {
                    tracing::warn!("relay control send failed or timed out");
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ControlRequest>(&text).or_else(|_| {
                            serde_json::from_str::<RegisterPairingPin>(&text).map(ControlRequest::RegisterPairingPin)
                        }) {
                            Ok(ControlRequest::AllocateSession) => {
                                let id = uuid::Uuid::new_v4().to_string();
                                if !state.issue_session(&machine_token, &id, Some(generation)) {
                                    tracing::warn!("relay session allocation rejected");
                                    break;
                                }
                            }
                            Ok(ControlRequest::RegisterPairingPin(registration)) => {
                                let ack = RegisterPairingPinAck {
                                    generation: registration.generation,
                                    pin: registration.pin.clone(),
                                    machine_id: machine_token.clone(),
                                    status: match state.register_pairing(&machine_token, generation, registration) {
                                        Ok(()) => "ready",
                                        Err(error) => error,
                                    }.into(),
                                };
                                if !matches!(timeout(TRANSFER_TIMEOUT, socket.send(Message::Text(serde_json::to_string(&ack).unwrap().into()))).await, Ok(Ok(()))) {
                                    tracing::warn!("relay pairing acknowledgment failed");
                                    break;
                                }
                            }
                            Err(error) => tracing::warn!(%error, "invalid relay control request"),
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::warn!(%error, "relay control receive failed");
                        break;
                    }
                }
            }
        }
    }
    state.unregister_control_channel(&machine_token, generation);
}

/// `GET /tunnel/data/:session_id` - the desktop daemon's data channel for a
/// specific session, opened after receiving a control-channel notification.
async fn data_handler(
    ws: WebSocketUpgrade,
    AxumPath(session_id): AxumPath<String>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    State(state): State<RelayState>,
) -> Result<Response, StatusCode> {
    if !state.admit(peer_ip(peer), Instant::now()) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    upgrade_half(ws, state, session_id, HalfKind::Data)
}

/// `GET /tunnel/client/:session_id` - the remote client's channel for a
/// specific session.
async fn client_handler(
    ws: WebSocketUpgrade,
    AxumPath(session_id): AxumPath<String>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    State(state): State<RelayState>,
) -> Result<Response, StatusCode> {
    if !state.admit(peer_ip(peer), Instant::now()) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    upgrade_half(ws, state, session_id, HalfKind::Client)
}

fn upgrade_half(
    ws: WebSocketUpgrade,
    state: RelayState,
    session_id: String,
    kind: HalfKind,
) -> Result<Response, StatusCode> {
    let (generation, outcome) = state.reserve_half(&session_id, kind)?;
    let guard = SessionGuard {
        armed: true,
        state,
        session_id,
        generation,
    };
    Ok(ws
        .max_message_size(MAX_MESSAGE_SIZE)
        .max_frame_size(MAX_MESSAGE_SIZE)
        .on_upgrade(move |socket| handle_half_socket(socket, guard, outcome)))
}

/// Registers `socket` as one half of `session_id`'s pairing. If the
/// opposite half is already waiting, hands `socket` to it directly and
/// returns immediately (the other task drives the proxy). Otherwise waits
/// (bounded by [`SESSION_PAIRING_TIMEOUT`]) for the opposite half to show
/// up, then proxies frames bidirectionally between the two sockets until
/// either side disconnects.
async fn handle_half_socket(
    mut socket: WebSocket,
    mut guard: SessionGuard,
    outcome: PairingOutcome,
) {
    match outcome {
        PairingOutcome::HandOff(tx) => {
            if tx.send(socket).is_ok() {
                // The waiting task owns cleanup after a successful transfer.
                guard.armed = false;
            } else {
                tracing::warn!("relay counterpart disconnected before handoff");
            }
        }
        PairingOutcome::Waiting { mut rx } => {
            let transfer = async {
                let mut buffered = Vec::new();
                let mut bytes = 0;
                let pairing = async {
                    loop {
                        tokio::select! {
                            peer = &mut rx => return peer.map_err(anyhow::Error::from),
                            incoming = socket.recv() => {
                                match incoming {
                                    Some(Ok(Message::Close(_))) | None => anyhow::bail!("pending peer disconnected"),
                                    Some(Err(error)) => return Err(error.into()),
                                    Some(Ok(message)) => {
                                        bytes += match &message {
                                            Message::Text(text) => text.len(),
                                            Message::Binary(data) | Message::Ping(data) | Message::Pong(data) => data.len(),
                                            Message::Close(_) => 0,
                                        };
                                        if bytes > MAX_MESSAGE_SIZE || buffered.len() >= 100 {
                                            anyhow::bail!("pending frame buffer limit exceeded");
                                        }
                                        buffered.push(message);
                                    }
                                }
                            }
                        }
                    }
                };
                let mut peer = timeout(SESSION_PAIRING_TIMEOUT, pairing).await??;
                for message in buffered {
                    timeout(TRANSFER_TIMEOUT, peer.send(message)).await??;
                }
                proxy_sockets(socket, peer).await
            };
            match timeout(SESSION_TRANSFER_TIMEOUT, transfer).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(%error, "relay session failed"),
                Err(error) => tracing::warn!(%error, "relay session lifetime exceeded"),
            }
            drop(guard);
        }
    }
}

/// Proxies WebSocket frames bidirectionally between `a` and `b` until
/// either side closes or errors.
async fn proxy_sockets(a: WebSocket, b: WebSocket) -> anyhow::Result<()> {
    let (mut a_tx, mut a_rx) = a.split();
    let (mut b_tx, mut b_rx) = b.split();

    let a_to_b = async {
        while let Some(msg) = a_rx.next().await {
            let msg = msg?;
            let is_close = matches!(msg, Message::Close(_));
            timeout(TRANSFER_TIMEOUT, b_tx.send(msg)).await??;
            if is_close {
                break;
            }
        }
        Ok::<(), anyhow::Error>(())
    };
    let b_to_a = async {
        while let Some(msg) = b_rx.next().await {
            let msg = msg?;
            let is_close = matches!(msg, Message::Close(_));
            timeout(TRANSFER_TIMEOUT, a_tx.send(msg)).await??;
            if is_close {
                break;
            }
        }
        Ok::<(), anyhow::Error>(())
    };

    tokio::select! {
        result = a_to_b => result,
        result = b_to_a => result,
    }
}

#[cfg(test)]
mod tests {
    fn test_state(tokens: Vec<String>) -> RelayState {
        RelayState::with_key_store(tokens, None).unwrap()
    }
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message as TMessage;

    /// Starts an in-process relay server bound to an ephemeral loopback
    /// port and returns its base `ws://` URL along with a handle that
    /// keeps the server task alive for the duration of the test.
    async fn spawn_test_relay() -> (String, tokio::task::JoinHandle<()>) {
        let state = test_state(vec!["test-machine-token".to_string()]);
        spawn_test_relay_with_state(state).await
    }

    async fn spawn_test_relay_with_state(state: RelayState) -> (String, tokio::task::JoinHandle<()>) {
        let router = relay_router(state);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");

        let handle = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .expect("relay server exited unexpectedly");
        });

        (format!("ws://{addr}"), handle)
    }

    async fn issue_test_ticket(state: &RelayState, target: &str) -> SocketTicketResponse {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer test-device-token".parse().unwrap());
        socket_ticket_handler(State(state.clone()), AxumPath("browser-machine".into()), headers,
            Json(serde_json::json!({"target": target}))).await.unwrap().0
    }

    fn ticket_query(ticket: &str) -> Result<Query<SocketQuery>, axum::extract::rejection::QueryRejection> {
        Ok(Query(SocketQuery { ticket: Some(ticket.into()) }))
    }

    async fn register_security_pin(socket: &mut TestSocket, machine: &str) {
        let registration = security_registration(machine);
        socket.send(TMessage::Text(serde_json::to_string(&registration).unwrap().into())).await.unwrap();
        let ack: RegisterPairingPinAck = receive_json(socket).await;
        assert_eq!(ack.machine_id, machine);
        assert_eq!(ack.pin, registration.pin);
        assert_eq!(ack.status, "ready");
    }

    fn security_registration(machine: &str) -> RegisterPairingPin {
        RegisterPairingPin {
            generation: None,
            machine_id: machine.into(),
            pin: "123456".into(),
            pairing_token: "security-pairing-secret".into(),
            expires_at: current_time_secs() + 3600,
        }
    }

    async fn security_claim(state: &RelayState, pin: &str) -> StatusCode {
        let request = Request::builder().method(Method::POST)
            .uri("/api/v1/pair/exchange")
            .body(Body::from(serde_json::json!({"pin": pin, "deviceName": "security-client"}).to_string())).unwrap();
        // Supply the accepted peer address, not an untrusted forwarding header.
        let peer = Some(axum::Extension(ConnectInfo("192.0.2.1:12345".parse().unwrap())));
        match timeout(Duration::from_secs(5), pair_exchange_handler(State(state.clone()), peer, request)).await.unwrap() {
            Ok(response) => response.status(),
            Err(status) => status,
        }
    }

    async fn assert_socket_rejected(base: &str, path: &str, status: StatusCode) {
        let error = timeout(Duration::from_secs(5), tokio_tungstenite::connect_async(format!("{base}{path}")))
            .await.unwrap().unwrap_err();
        assert!(matches!(error, tokio_tungstenite::tungstenite::Error::Http(response) if response.status() == status));
    }

    #[tokio::test]
    async fn test_security_machine_id_hijack_rejection() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let owner = identity(31, "machine_alpha");
        let (mut original, auth) = authenticate(&base, &owner, false, false).await;
        assert!(auth.success);
        register_security_pin(&mut original, "machine_alpha").await;
        let generation = state.inner.control_channels.lock()["machine_alpha"].generation;
        let (mut attacker, rejection) = authenticate(&base, &identity(32, "machine_alpha"), false, false).await;
        assert!(!rejection.success);
        // Authentication happens after HTTP 101, so rejection is a protocol error, not HTTP 401.
        assert!(rejection.error.unwrap().contains("Machine ID already claimed"));
        assert!(matches!(timeout(Duration::from_secs(5), attacker.next()).await.unwrap(), Some(Ok(TMessage::Close(_)))));
        assert_eq!(state.inner.machine_public_keys.lock()["machine_alpha"], owner.public_key);
        assert_eq!(state.inner.control_channels.lock()["machine_alpha"].generation, generation);
        assert_eq!(state.inner.pairings.lock()["123456"].state, PairingState::Ready);
        let notice = allocate(&mut original).await;
        assert!(state.inner.pending_sessions.lock().contains_key(&notice.session_id));
        server.abort();
    }

    #[tokio::test]
    async fn test_security_fleet_pin_brute_force_lockout() {
        let state = test_state(vec![]);
        let (generation, mut notices) = state.register_control_channel("machine_alpha".into());
        assert!(state.register_pairing("machine_alpha", generation, security_registration("machine_alpha")).is_ok());
        for pin in ["000001", "000002", "000003", "000004", "000005"] {
            assert_eq!(security_claim(&state, pin).await, StatusCode::NOT_FOUND);
        }
        // The current pairing API represents lockout with 401 (control admission uses 429).
        assert_eq!(security_claim(&state, "123456").await, StatusCode::UNAUTHORIZED);
        let admission = state.inner.pairing_admission.lock();
        let tracker = &admission[&"192.0.2.1".parse::<IpAddr>().unwrap()];
        assert_eq!(tracker.failures, 5);
        assert!(tracker.locked_until.is_some());
        assert_eq!(state.inner.pairings.lock()["123456"].state, PairingState::Ready);
        assert!(state.inner.pending_sessions.lock().is_empty());
        assert!(matches!(notices.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn test_security_expired_pin_claim_rejected() {
        let state = test_state(vec![]);
        let (generation, mut notices) = state.register_control_channel("machine_alpha".into());
        let mut expired = security_registration("machine_alpha");
        expired.expires_at = current_time_secs() - 10;
        assert!(state.register_pairing("machine_alpha", generation, expired).is_err());
        assert_eq!(security_claim(&state, "123456").await, StatusCode::NOT_FOUND);
        assert!(state.register_pairing("machine_alpha", generation, security_registration("machine_alpha")).is_ok());
        // Exercise claim-time expiration as well as registration-time rejection, without waiting.
        state.inner.pairings.lock().get_mut("123456").unwrap().registration.expires_at = current_time_secs() - 10;
        assert_eq!(security_claim(&state, "123456").await, StatusCode::NOT_FOUND);
        assert_eq!(state.inner.pairings.lock()["123456"].state, PairingState::Ready);
        assert!(state.inner.pending_sessions.lock().is_empty());
        assert!(matches!(notices.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn test_security_role_swap_prevention() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (mut daemon, auth) = authenticate(&base, &identity(33, "machine_alpha"), false, false).await;
        assert!(auth.success);
        register_security_pin(&mut daemon, "machine_alpha").await;
        for path in ["/host/machine_alpha/api/v1/events", "/host/machine_alpha/api/v1/terminal/t1"] {
            assert_socket_rejected(&base, path, StatusCode::UNAUTHORIZED).await;
        }
        let (mut client, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/control")).await.unwrap();
        let _: ControlChallenge = receive_json(&mut client).await;
        // Trigger explicit rejection, rather than waiting for authentication timeout.
        client.send(TMessage::Text(r#"{"machineId":"public-client","publicKey":"","timestamp":0}"#.into())).await.unwrap();
        let rejection: ControlAuthResponse = receive_json(&mut client).await;
        assert!(!rejection.success);
        assert!(rejection.error.is_some());
        assert!(matches!(timeout(Duration::from_secs(5), client.next()).await.unwrap(), Some(Ok(TMessage::Close(_)))));
        assert_socket_rejected(&base, "/tunnel/data/unissued-session", StatusCode::NOT_FOUND).await;
        assert_eq!(state.inner.control_channels.lock().len(), 1);
        assert!(!state.inner.machine_public_keys.lock().contains_key("public-client"));
        assert!(state.inner.pending_sessions.lock().is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn test_security_concurrent_two_daemon_isolation() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let first = identity(34, "daemon_1");
        let second = identity(35, "daemon_2");
        let ((mut daemon_1, a), (mut daemon_2, b)) = tokio::join!(
            authenticate(&base, &first, false, false),
            authenticate(&base, &second, false, false),
        );
        assert!(a.success && b.success);
        register_security_pin(&mut daemon_1, "daemon_1").await;
        allocate(&mut daemon_2).await; // Registration barrier on the second real socket.
        // Observe dispatch at the actual mpsc boundary. Retain the original sender so
        // daemon_2 remains connected; no elapsed-time absence assertion is needed.
        let (capture, mut unexpected) = mpsc::channel(MAX_PENDING_SESSIONS);
        let original_sender = {
            let mut channels = state.inner.control_channels.lock();
            std::mem::replace(&mut channels.get_mut("daemon_2").unwrap().tx, capture)
        };
        let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().unwrap();
        let request = client.post(format!("{}/api/v1/pair/exchange", base.replace("ws://", "http://")))
            .body(r#"{"pin":"123456","deviceName":"isolated-client"}"#).send();
        let daemon = async {
            let notice: IncomingSessionNotice = receive_json(&mut daemon_1).await;
            let (mut data, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{}", notice.session_id)).await.unwrap();
            let frame = timeout(Duration::from_secs(5), data.next()).await.unwrap().unwrap().unwrap();
            let raw = String::from_utf8(frame.into_data().to_vec()).unwrap();
            assert!(raw.starts_with("POST /api/v1/pair/exchange HTTP/1.1\r\n"));
            let body: serde_json::Value = serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["code"], "security-pairing-secret");
            assert_eq!(body["deviceName"], "isolated-client");
            data.send(TMessage::Binary(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\ndaemon_1".to_vec().into())).await.unwrap();
            let _ = timeout(Duration::from_secs(5), data.next()).await.unwrap();
        };
        let (response, ()) = timeout(Duration::from_secs(5), async { tokio::join!(request, daemon) }).await.unwrap();
        let response = response.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "daemon_1");
        assert_eq!(state.inner.pairings.lock()["123456"].state, PairingState::Consumed);
        for target in ["/api/v1/events", "/api/v1/terminal/t1"] {
            let response = client.post(format!("{}/host/daemon_1/api/v1/socket-ticket", base.replace("ws://", "http://")))
                .header("authorization", "Bearer test-device-token")
                .header("content-type", "application/json").body(serde_json::json!({"target": target}).to_string()).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let ticket: SocketTicketResponse = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
            assert_socket_rejected(&base, &format!("/host/daemon_2{target}?ticket={}", ticket.ticket), StatusCode::UNAUTHORIZED).await;
        }
        // HTTP completion establishes that all synchronous dispatch decisions finished.
        assert!(matches!(unexpected.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
        state.inner.control_channels.lock().get_mut("daemon_2").unwrap().tx = original_sender;
        allocate(&mut daemon_2).await; // The unaffected daemon remains usable.
        server.abort();
    }

    #[tokio::test]
    async fn test_relay_socket_ticket_issuance_and_single_use() {
        let state = test_state(vec![]);
        let (_generation, _control) = state.register_control_channel("browser-machine".into());
        let ticket = issue_test_ticket(&state, "/api/v1/events").await;
        assert_eq!(uuid::Uuid::parse_str(&ticket.ticket).unwrap().get_version_num(), 4);
        assert!(ticket.expires_at > current_time_secs());
        assert!(ticket.expires_at <= current_time_secs() + 30);
        assert_eq!(consume_socket_ticket(&state, ticket_query(&ticket.ticket), "browser-machine", "/api/v1/events"), Ok("test-device-token".into()));
        assert_eq!(consume_socket_ticket(&state, ticket_query(&ticket.ticket), "browser-machine", "/api/v1/events"), Err(StatusCode::UNAUTHORIZED));
        for (machine, target) in [("other", "/api/v1/events"), ("browser-machine", "/api/v1/terminal/t1")] {
            let ticket = issue_test_ticket(&state, "/api/v1/events").await;
            assert_eq!(consume_socket_ticket(&state, ticket_query(&ticket.ticket), machine, target), Err(StatusCode::UNAUTHORIZED));
            assert!(!state.inner.pending_socket_tickets.lock().contains_key(&ticket.ticket));
        }
    }

    #[tokio::test]
    async fn test_relay_socket_ticket_expired_rejected() {
        let state = test_state(vec![]);
        let (_generation, _control) = state.register_control_channel("browser-machine".into());
        let ticket = issue_test_ticket(&state, "/api/v1/events").await;
        state.inner.pending_socket_tickets.lock().get_mut(&ticket.ticket).unwrap().3 = current_time_secs() - 1;
        assert_eq!(consume_socket_ticket(&state, ticket_query(&ticket.ticket), "browser-machine", "/api/v1/events"), Err(StatusCode::UNAUTHORIZED));
    }

    #[tokio::test]
    async fn test_relay_browser_ws_terminal_bridge_success() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (base, server) = spawn_test_relay().await;
        let (mut control, auth) = authenticate(&base, &identity(12, "browser-machine"), false, false).await;
        assert!(auth.success);
        // An acknowledged control operation is the registration barrier.
        allocate(&mut control).await;
        let response = reqwest::Client::new().post(format!("{}/host/browser-machine/api/v1/socket-ticket", base.replace("ws://", "http://")))
            .header("authorization", "Bearer test-device-token")
            .timeout(Duration::from_secs(5)).header("content-type", "application/json").body(r#"{"target":"/api/v1/terminal/t1"}"#).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let ticket: SocketTicketResponse = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        for query in [String::new(), "?token=permanent".into(), format!("?ticket={}&access_token=permanent", ticket.ticket)] {
            let error = tokio_tungstenite::connect_async(format!("{base}/host/browser-machine/api/v1/terminal/t1{query}")).await.unwrap_err();
            assert!(matches!(error, tokio_tungstenite::tungstenite::Error::Http(r) if r.status() == StatusCode::UNAUTHORIZED));
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let gateway = async {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_hdr_async(tcp, |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                assert_eq!(request.uri().path(), "/api/v1/terminal/t1");
                assert!(request.uri().query().is_none());
                Ok(response)
            }).await.unwrap();
            for expected in [TMessage::Text("input".into()), TMessage::Binary(vec![0, 1, 255].into()), TMessage::Ping(vec![7].into())] {
                assert_eq!(ws.next().await.unwrap().unwrap(), expected);
                ws.send(match expected { TMessage::Ping(bytes) => TMessage::Pong(bytes), other => other }).await.unwrap();
            }
            // Consume any forwarded pong before the close frame.
            loop {
                match ws.next().await.unwrap().unwrap() {
                    TMessage::Close(_) => break,
                    TMessage::Pong(_) => {},
                    other => panic!("unexpected frame: {other:?}"),
                }
            }
        };
        let daemon = async {
            let notice: IncomingSessionNotice = receive_json(&mut control).await;
            let (mut data, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{}", notice.session_id)).await.unwrap();
            let tcp = tokio::net::TcpStream::connect(address).await.unwrap();
            let (mut read, mut write) = tcp.into_split();
            let mut buffer = [0; 8192];
            loop {
                tokio::select! {
                    frame = data.next() => match frame {
                        Some(Ok(TMessage::Binary(bytes))) => write.write_all(&bytes).await.unwrap(),
                        Some(Ok(TMessage::Close(_))) | None => break,
                        Some(Ok(_)) => {},
                        Some(Err(error)) => panic!("data channel failed: {error}"),
                    },
                    count = read.read(&mut buffer) => {
                        let count = count.unwrap();
                        if count == 0 { break; }
                        data.send(TMessage::Binary(buffer[..count].to_vec().into())).await.unwrap();
                    }
                }
            }
        };
        let browser = async {
            let url = format!("{base}/host/browser-machine/api/v1/terminal/t1?ticket={}", ticket.ticket);
            let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
            let error = tokio_tungstenite::connect_async(&url).await.unwrap_err();
            assert!(matches!(error, tokio_tungstenite::tungstenite::Error::Http(r) if r.status() == StatusCode::UNAUTHORIZED));
            for frame in [TMessage::Text("input".into()), TMessage::Binary(vec![0, 1, 255].into()), TMessage::Ping(vec![7].into())] {
                ws.send(frame.clone()).await.unwrap();
                let expected = match frame { TMessage::Ping(bytes) => TMessage::Pong(bytes), other => other };
                assert_eq!(ws.next().await.unwrap().unwrap(), expected);
            }
            ws.close(None).await.unwrap();
        };
        timeout(Duration::from_secs(5), async { tokio::join!(browser, daemon, gateway); }).await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn coordinator_pairs_through_relay_to_real_gateway() {
        use crate::remote::{relay_client::RelayClient, state::RemoteGatewayState};
        let (base, relay) = spawn_test_relay().await;
        let terminal = Arc::new(crate::terminal::TerminalService::new(
            Arc::new(crate::terminal::PtyManager::new()),
            Arc::new(crate::terminal::TerminalOutputHub::default()),
        ));
        let state = Arc::new(RemoteGatewayState::new_with_paths(
            terminal, crate::worktree::WorkspaceRegistry::new(), None, None,
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway_addr = listener.local_addr().unwrap();
        let router = crate::remote::server::create_remote_router(state.clone());
        let gateway = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = RelayClient::with_identity(&base, identity(19, "real-gateway"), gateway_addr.to_string())
            .with_enrollment_token("test-machine-token")
            .with_auth_manager((*state.auth_manager).clone());
        let coordinator = client.pairing_coordinator();
        let control = tokio::spawn(async move { client.run().await });
        let session = coordinator.generate_pairing(Duration::from_secs(60)).await.unwrap();
        let http = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().unwrap();
        let url = format!("{}/api/v1/pair/exchange", base.replace("ws://", "http://"));
        let response = http.post(&url).header("content-type", "application/json")
            .body(serde_json::json!({
                "code": session.pairing_token, "deviceName": "real browser"
            }).to_string()).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert!(body["machineId"].as_str().is_some_and(|id| !id.is_empty()));
        assert!(body["displayName"].as_str().is_some_and(|name| !name.is_empty()));
        let device = state.auth_manager.validate_token(body["token"].as_str().unwrap()).unwrap();
        assert_eq!(device.name, "real browser");
        assert!(state.auth_manager.exchange_pairing_code(&session.pairing_token, "replay").is_err());
        control.abort(); gateway.abort(); relay.abort();
    }

    #[tokio::test]
    async fn test_relay_pair_exchange_success() {
        let (base, server) = spawn_test_relay().await;
        let (mut control, auth) =
            authenticate(&base, &identity(9, "pair-machine"), false, false).await;
        assert!(auth.success);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        for (pin, token, route, credential) in [
            (
                "123456",
                "secret-one",
                "/api/v1/pair/exchange",
                serde_json::json!({"pin":"123456"}),
            ),
            (
                "234567",
                "secret-two",
                "/host/pair-machine/api/v1/pair/exchange",
                serde_json::json!({"pairingToken":"secret-two"}),
            ),
            ("345678", "secret-three", "/api/v1/pair/exchange", serde_json::json!({"code":"345678"})),
            ("456789", "secret-four", "/api/v1/pair/exchange", serde_json::json!({"code":"secret-four"})),
        ] {
            let registration = RegisterPairingPin {
                generation: None,
                pin: pin.into(),
                pairing_token: token.into(),
                machine_id: "pair-machine".into(),
                expires_at: current_time_secs() + 60,
            };
            control
                .send(TMessage::Text(
                    serde_json::to_string(&registration).unwrap().into(),
                ))
                .await
                .unwrap();
            let ack: RegisterPairingPinAck = receive_json(&mut control).await;
            assert_eq!(ack.status, "ready");
            let mut payload = credential;
            payload["deviceName"] = "test-device".into();
            let url = format!("{}{route}", base.replace("ws://", "http://"));
            let request = client
                .post(&url)
                .header("content-type", "application/json")
                .body(payload.to_string())
                .send();
            let daemon = async {
                let notice: IncomingSessionNotice = receive_json(&mut control).await;
                let (mut data, _) = tokio_tungstenite::connect_async(format!(
                    "{base}/tunnel/data/{}",
                    notice.session_id
                ))
                .await
                .unwrap();
                let frame = timeout(Duration::from_secs(5), data.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                let raw = String::from_utf8(frame.into_data().to_vec()).unwrap();
                assert!(raw.starts_with("POST /api/v1/pair/exchange HTTP/1.1\r\n"));
                let body: serde_json::Value =
                    serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap();
                assert_eq!(body["code"], token);
                assert_eq!(body["deviceName"], "test-device");
                let body = br#"{"token":"device-token"}"#;
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Daemon: yes\r\nContent-Length: {}\r\n\r\n", body.len());
                data.send(TMessage::Binary(response.into_bytes().into()))
                    .await
                    .unwrap();
                data.send(TMessage::Binary(body.to_vec().into()))
                    .await
                    .unwrap();
                // Await relay completion rather than dropping a socket with unread frames.
                let _ = timeout(Duration::from_secs(5), data.next()).await.unwrap();
            };
            let (response, ()) = tokio::join!(request, daemon);
            let response = response.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["x-daemon"], "yes");
            let body: serde_json::Value =
                serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
            assert_eq!(body["token"], "device-token");
            let replay = client
                .post(url)
                .body(payload.to_string())
                .send()
                .await
                .unwrap();
            assert_eq!(replay.status(), StatusCode::NOT_FOUND);
        }
        server.abort();
    }

    #[tokio::test]
    async fn test_relay_pair_exchange_unknown_pin_returns_404() {
        let (base, server) = spawn_test_relay().await;
        let response = reqwest::Client::new()
            .post(format!(
                "{}/api/v1/pair/exchange",
                base.replace("ws://", "http://")
            ))
            .timeout(Duration::from_secs(5))
            .body(r#"{"pin":"000000","deviceName":"test"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        server.abort();
    }

    #[tokio::test]
    async fn test_relay_pair_exchange_lockout_after_failures() {
        let (base, server) = spawn_test_relay().await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        for attempt in 0..6 {
            let response = client
                .post(format!(
                    "{}/api/v1/pair/exchange",
                    base.replace("ws://", "http://")
                ))
                .header("x-forwarded-for", format!("192.0.2.{attempt}"))
                .body(r#"{"pin":"000000","deviceName":"test"}"#)
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if attempt < 5 {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::UNAUTHORIZED
                }
            );
        }
        server.abort();
    }

    #[tokio::test]
    async fn test_http_response_framing() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: x-hop\r\nX-Hop: hidden\r\nX-End: kept\r\n\r\n3\r\nabc\r\n0\r\n\r\n";
        assert!(parse_http_response(&raw[..raw.len() - 1], false, false)
            .unwrap()
            .is_none());
        let response = parse_http_response(raw, false, false).unwrap().unwrap();
        assert!(!response.headers().contains_key("x-hop"));
        assert!(!response.headers().contains_key("transfer-encoding"));
        assert_eq!(response.headers()["x-end"], "kept");
        assert_eq!(
            to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
            b"abc"
        );
    }

    type TestSocket = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    fn identity(seed: u8, machine_id: &str) -> crate::remote::auth::MachineIdentity {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let key = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
        crate::remote::auth::MachineIdentity {
            machine_id: machine_id.into(),
            display_name: machine_id.into(),
            public_key: STANDARD.encode(key.verifying_key().to_bytes()),
            private_key: STANDARD.encode(key.to_bytes()),
        }
    }

    async fn receive_json<T: serde::de::DeserializeOwned>(socket: &mut TestSocket) -> T {
        let frame = timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        serde_json::from_str(frame.to_text().unwrap()).unwrap()
    }

    async fn authenticate(
        base: &str,
        identity: &crate::remote::auth::MachineIdentity,
        wrong_signature: bool,
        stale: bool,
    ) -> (TestSocket, ControlAuthResponse) {
        authenticate_with_token(base, identity, wrong_signature, stale, Some("test-machine-token")).await
    }

    async fn authenticate_with_token(
        base: &str,
        identity: &crate::remote::auth::MachineIdentity,
        wrong_signature: bool,
        stale: bool,
        enrollment_token: Option<&str>,
    ) -> (TestSocket, ControlAuthResponse) {
        let (mut socket, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/control"))
            .await
            .unwrap();
        let challenge: ControlChallenge = receive_json(&mut socket).await;
        let timestamp = if stale {
            challenge.timestamp - 61
        } else {
            challenge.timestamp
        };
        let auth = ControlAuth {
            enrollment_token: enrollment_token.map(str::to_owned),
            machine_id: identity.machine_id.clone(),
            display_name: identity.display_name.clone(),
            public_key: identity.public_key.clone(),
            timestamp,
            signature: crate::remote::auth::sign_control_challenge(
                identity,
                "relay",
                if wrong_signature {
                    "different-nonce"
                } else {
                    &challenge.nonce
                },
                timestamp,
            )
            .unwrap(),
        };
        socket
            .send(TMessage::Text(serde_json::to_string(&auth).unwrap().into()))
            .await
            .unwrap();
        let response = receive_json(&mut socket).await;
        (socket, response)
    }

    async fn allocate(socket: &mut TestSocket) -> IncomingSessionNotice {
        socket
            .send(TMessage::Text(r#"{"type":"AllocateSession"}"#.into()))
            .await
            .unwrap();
        receive_json(socket).await
    }

    #[test]
    fn test_control_challenge_domain_separation() {
        use crate::remote::auth::{sign_challenge, sign_control_challenge};
        let owner = identity(1, "machine-a");
        let signature = sign_control_challenge(&owner, "relay", "nonce", 42).unwrap();
        assert!(verify_control_challenge(&owner.public_key, "machine-a", "relay", "nonce", 42, &signature));
        for (machine, audience, nonce, timestamp) in [
            ("machine-b", "relay", "nonce", 42),
            ("machine-a", "gateway", "nonce", 42),
            ("machine-a", "relay", "different", 42),
            ("machine-a", "relay", "nonce", 43),
        ] {
            assert!(!verify_control_challenge(&owner.public_key, machine, audience, nonce, timestamp, &signature));
        }
        let legacy = sign_challenge(&owner, "nonce", 42).unwrap();
        assert!(!verify_control_challenge(&owner.public_key, "machine-a", "relay", "nonce", 42, &legacy));
    }

    #[tokio::test]
    async fn test_machine_ownership_survives_restart_and_private_enrollment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("machine_keys.json");
        let state = RelayState::new_with_key_store(vec!["enroll".into()], &path).unwrap();
        let (base, server) = spawn_test_relay_with_state(state).await;
        let owner = identity(1, "durable-machine");
        for token in [None, Some("wrong")] {
            let (_, response) = authenticate_with_token(&base, &owner, false, false, token).await;
            assert!(!response.success);
            assert!(!path.exists());
        }
        let (mut socket, response) = authenticate_with_token(&base, &owner, false, false, Some("enroll")).await;
        assert!(response.success);
        allocate(&mut socket).await;
        socket.close(None).await.unwrap();
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());

        let state = RelayState::new_with_key_store(vec!["enroll".into()], &path).unwrap();
        let (base, server) = spawn_test_relay_with_state(state).await;
        let (mut socket, response) = authenticate_with_token(&base, &owner, false, false, None).await;
        assert!(response.success);
        allocate(&mut socket).await;
        let impostor = identity(2, "durable-machine");
        let (_, response) = authenticate_with_token(&base, &impostor, false, false, Some("enroll")).await;
        assert!(!response.success);
        let outsider = identity(3, "unlisted-machine");
        let (_, response) = authenticate_with_token(&base, &outsider, false, false, None).await;
        assert!(!response.success);
        server.abort();
    }

    #[test]
    fn test_key_store_corruption_and_write_failure_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("machine_keys.json");
        std::fs::write(&path, b"not json").unwrap();
        assert!(RelayState::new_with_key_store(vec![], &path).is_err());
        let blocked = dir.path().join("blocked");
        let state = RelayState::new_with_key_store(vec![], blocked.join("keys.json")).unwrap();
        std::fs::write(&blocked, b"not a directory").unwrap();
        let auth = ControlAuth {
            enrollment_token: None, machine_id: "machine".into(), display_name: "test".into(),
            public_key: identity(1, "machine").public_key, signature: String::new(), timestamp: 0,
        };
        assert!(state.bind_machine_key(&auth).is_err());
        assert!(state.inner.machine_public_keys.lock().is_empty());
    }

    #[tokio::test]
    async fn test_relay_ed25519_control_handshake_success() {
        let (base, server) = spawn_test_relay().await;
        let (mut socket, response) =
            authenticate(&base, &identity(1, "machine-a"), false, false).await;
        assert!(response.success);
        assert!(response.error.is_none());
        let notice = allocate(&mut socket).await;
        assert!(uuid::Uuid::parse_str(&notice.session_id).is_ok());
        let (mut data, _) =
            tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{}", notice.session_id))
                .await
                .unwrap();
        let (mut client, _) =
            tokio_tungstenite::connect_async(format!("{base}/tunnel/client/{}", notice.session_id))
                .await
                .unwrap();
        client
            .send(TMessage::Text("authenticated tunnel".into()))
            .await
            .unwrap();
        let frame = timeout(Duration::from_secs(5), data.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(frame.to_text().unwrap(), "authenticated tunnel");
        server.abort();
    }

    #[tokio::test]
    async fn test_relay_ed25519_control_handshake_wrong_sig_rejected() {
        let (base, server) = spawn_test_relay().await;
        let (mut socket, response) =
            authenticate(&base, &identity(1, "machine-a"), true, false).await;
        assert!(!response.success);
        assert!(response.error.is_some());
        assert!(matches!(
            timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap(),
            Some(Ok(TMessage::Close(_))) | None
        ));
        // A failed proof must not claim the ID.
        let (_, response) = authenticate(&base, &identity(2, "machine-a"), false, false).await;
        assert!(response.success);
        server.abort();
    }

    #[tokio::test]
    async fn test_relay_ed25519_stale_timestamp_rejected() {
        let (base, server) = spawn_test_relay().await;
        let (_, response) = authenticate(&base, &identity(1, "machine-a"), false, true).await;
        assert!(!response.success);
        server.abort();
    }

    #[tokio::test]
    async fn test_relay_machine_hijack_with_different_key_fails() {
        let (base, server) = spawn_test_relay().await;
        let owner = identity(1, "machine-a");
        let (mut original, response) = authenticate(&base, &owner, false, false).await;
        assert!(response.success);
        let (_, response) = authenticate(&base, &identity(2, "machine-a"), false, false).await;
        assert!(!response.success);
        allocate(&mut original).await;
        original.close(None).await.unwrap();
        let (_, response) = authenticate(&base, &identity(2, "machine-a"), false, false).await;
        assert!(!response.success);
        let (mut replacement, response) = authenticate(&base, &owner, false, false).await;
        assert!(response.success);
        allocate(&mut replacement).await;
        server.abort();
    }

    #[tokio::test]
    async fn test_relay_multi_machine_concurrent_registration() {
        let (base, server) = spawn_test_relay().await;
        let a = identity(1, "machine-a");
        let b = identity(2, "machine-b");
        let ((mut first, a), (mut second, b)) = tokio::join!(
            authenticate(&base, &a, false, false),
            authenticate(&base, &b, false, false),
        );
        assert!(a.success && b.success);
        let (a, b) = tokio::join!(allocate(&mut first), allocate(&mut second));
        assert_ne!(a.session_id, b.session_id);
        server.abort();
    }

    #[tokio::test]
    async fn test_relay_admission_rate_limiting() {
        let state = test_state(vec![]);
        let ip = "127.0.0.1".parse().unwrap();
        let other_ip = "127.0.0.2".parse().unwrap();
        let now = Instant::now();
        for _ in 0..30 {
            assert!(state.admit(ip, now));
        }
        assert!(!state.admit(ip, now));
        assert!(state.admit(other_ip, now));
        assert!(state.admit(ip, now + ADMISSION_WINDOW));

        let (base, server) = spawn_test_relay().await;
        // Success resets consecutive failures without resetting the attempt cap.
        for _ in 0..4 {
            let (_, response) = authenticate(&base, &identity(1, "a"), true, false).await;
            assert!(!response.success);
        }
        let (_, response) = authenticate(&base, &identity(1, "a"), false, false).await;
        assert!(response.success);
        for _ in 0..5 {
            let error =
                tokio_tungstenite::connect_async(format!("{base}/tunnel/control?token=wrong"))
                    .await
                    .unwrap_err();
            assert!(
                matches!(error, tokio_tungstenite::tungstenite::Error::Http(response)
                if response.status() == StatusCode::UNAUTHORIZED)
            );
        }
        for path in ["control", "data/unknown", "client/unknown"] {
            let error = tokio_tungstenite::connect_async(format!("{base}/tunnel/{path}"))
                .await
                .unwrap_err();
            assert!(
                matches!(error, tokio_tungstenite::tungstenite::Error::Http(response)
                if response.status() == StatusCode::TOO_MANY_REQUESTS)
            );
        }
        server.abort();

        let (base, server) = spawn_test_relay().await;
        for _ in 0..30 {
            let error = tokio_tungstenite::connect_async(format!("{base}/tunnel/client/unknown"))
                .await
                .unwrap_err();
            assert!(
                matches!(error, tokio_tungstenite::tungstenite::Error::Http(response)
                if response.status() == StatusCode::NOT_FOUND)
            );
        }
        let error = tokio_tungstenite::connect_async(format!("{base}/tunnel/control"))
            .await
            .unwrap_err();
        assert!(
            matches!(error, tokio_tungstenite::tungstenite::Error::Http(response)
            if response.status() == StatusCode::TOO_MANY_REQUESTS)
        );
        server.abort();
    }

    /// End-to-end integration test: starts a real relay server on an
    /// ephemeral port, connects a simulated "data" channel (standing in
    /// for the desktop daemon's side of the tunnel) and a "client"
    /// channel to the same session id, and verifies that frames sent from
    /// either side are proxied through to the other - i.e. the relay's
    /// reverse-tunnel multiplexing bridges the two independently-opened
    /// WebSocket connections into one bidirectional pipe.
    #[tokio::test]
    async fn test_relay_reverse_tunnel_multiplex() {
        let (base_url, _server) = spawn_test_relay().await;
        let (mut control, _) = tokio_tungstenite::connect_async(format!(
            "{base_url}/tunnel/control?token=test-machine-token"
        ))
        .await
        .unwrap();
        control
            .send(TMessage::Text(r#"{"type":"AllocateSession"}"#.into()))
            .await
            .unwrap();
        let notice = timeout(Duration::from_secs(5), control.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let notice: IncomingSessionNotice =
            serde_json::from_str(notice.to_text().unwrap()).unwrap();
        let session_id = notice.session_id;

        let data_url = format!("{base_url}/tunnel/data/{session_id}");
        let client_url = format!("{base_url}/tunnel/client/{session_id}");

        // Open the "data" half first (simulating the desktop daemon
        // opening its data channel in response to a control-channel
        // notification). It will sit in the pending registry until the
        // client half connects.
        let (mut data_socket, _) = tokio_tungstenite::connect_async(&data_url)
            .await
            .expect("data channel should connect");

        // Now open the "client" half for the same session id. This should
        // be paired with the waiting data half and the relay should start
        // proxying frames between them.
        let (mut client_socket, _) = tokio_tungstenite::connect_async(&client_url)
            .await
            .expect("client channel should connect");

        for url in [&data_url, &client_url] {
            let error = tokio_tungstenite::connect_async(url).await.unwrap_err();
            assert!(
                matches!(error, tokio_tungstenite::tungstenite::Error::Http(response)
                if response.status() == StatusCode::CONFLICT)
            );
        }

        // client -> data
        client_socket
            .send(TMessage::Text("hello from client".into()))
            .await
            .expect("client send should succeed");

        let received = tokio::time::timeout(Duration::from_secs(5), data_socket.next())
            .await
            .expect("data side should receive client frame before timeout")
            .expect("data socket stream should not end")
            .expect("data socket frame should not error");
        match received {
            TMessage::Text(text) => assert_eq!(text, "hello from client"),
            other => panic!("expected text frame relayed from client, got {other:?}"),
        }

        // data -> client (bidirectional echo path back the other way)
        data_socket
            .send(TMessage::Text("hello from data".into()))
            .await
            .expect("data send should succeed");

        let received = tokio::time::timeout(Duration::from_secs(5), client_socket.next())
            .await
            .expect("client side should receive data frame before timeout")
            .expect("client socket stream should not end")
            .expect("client socket frame should not error");
        match received {
            TMessage::Text(text) => assert_eq!(text, "hello from data"),
            other => panic!("expected text frame relayed from data channel, got {other:?}"),
        }

        // Round-trip a second message each way to confirm the bridge
        // stays open for more than a single exchange (i.e. it is a
        // genuine bidirectional proxy, not a one-shot handoff).
        client_socket
            .send(TMessage::Text("second client message".into()))
            .await
            .expect("second client send should succeed");
        let received = tokio::time::timeout(Duration::from_secs(5), data_socket.next())
            .await
            .expect("data side should receive second client frame before timeout")
            .expect("data socket stream should not end")
            .expect("data socket frame should not error");
        match received {
            TMessage::Text(text) => assert_eq!(text, "second client message"),
            other => panic!("expected second text frame relayed from client, got {other:?}"),
        }

        let _ = client_socket.close(None).await;
        let _ = data_socket.close(None).await;
        _server.abort();
    }

    #[tokio::test]
    async fn test_relay_unissued_session_rejected() {
        let (base, server) = spawn_test_relay().await;
        for half in ["data", "client"] {
            let error = tokio_tungstenite::connect_async(format!("{base}/tunnel/{half}/unissued"))
                .await
                .unwrap_err();
            assert!(
                matches!(error, tokio_tungstenite::tungstenite::Error::Http(response)
                if response.status() == StatusCode::NOT_FOUND)
            );
        }
        server.abort();
    }

    #[test]
    fn test_relay_generation_safe_cleanup_and_limits() {
        let state = test_state(vec!["tok".into()]);
        assert!(!state.notify_incoming_session("tok", "missing-control"));
        let (old, _old_rx) = state.register_control_channel("tok".into());
        let (new, mut rx) = state.register_control_channel("tok".into());
        state.unregister_control_channel("tok", old);
        assert!(!state.issue_session("tok", "stale", Some(old)));
        assert!(state.notify_incoming_session("tok", "session"));
        rx.try_recv().unwrap();
        let (generation, waiting) = state.reserve_half("session", HalfKind::Data).unwrap();
        assert!(matches!(
            state.reserve_half("session", HalfKind::Data),
            Err(StatusCode::CONFLICT)
        ));
        state.remove_pending("session", generation);
        drop(waiting);
        assert!(state.notify_incoming_session("tok", "session"));
        rx.try_recv().unwrap();
        state.remove_pending("session", generation);
        let (_, waiting) = state.reserve_half("session", HalfKind::Client).unwrap();
        let (_, handoff) = state.reserve_half("session", HalfKind::Data).unwrap();
        assert!(matches!(
            state.reserve_half("session", HalfKind::Client),
            Err(StatusCode::CONFLICT)
        ));
        for n in 1..MAX_PENDING_SESSIONS {
            assert!(state.notify_incoming_session("tok", &format!("session-{n}")));
            rx.try_recv().unwrap();
        }
        assert!(!state.notify_incoming_session("tok", "overflow"));
        state.unregister_control_channel("tok", new);
        assert!(!state.notify_incoming_session("tok", "no-control"));
        drop((waiting, handoff));
    }
}
