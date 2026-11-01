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
//! Sessions still waiting for their second half share a 100-entry budget (20 per
//! machine); paired sessions do not count. Pairing expires after 30 seconds; sends
//! time out after 30 seconds and sessions after their kind's lifetime.

use crate::remote::auth::{verify_control_challenge, write_private_json};
use crate::remote::protocol::{
    AccountGrantOfferDelivered, AccountGrantOfferDelivery, ControlAuth, ControlAuthResponse,
    ControlChallenge, PairingState, RegisterPairingPin, RegisterPairingPinAck, SocketTicketRequest,
    SocketTicketResponse,
};
pub use crate::remote::state::DEFAULT_RELAY_URL;
use axum::{
    body::{to_bytes, Body, Bytes},
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Path as AxumPath, Query, RawQuery, State,
    },
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::timeout;

/// How long a session may sit waiting for its data or client half before
/// the relay gives up and evicts it from the registry.
const SESSION_PAIRING_TIMEOUT: Duration = Duration::from_secs(30);
/// Relay-wide cap on sessions still waiting for their second half. Paired (active) sessions
/// live up to a session lifetime and are bounded by their sockets, so they do not count.
const MAX_PENDING_SESSIONS: usize = 100;
/// Per-machine share of [`MAX_PENDING_SESSIONS`], so one tenant cannot starve the others.
const MAX_PENDING_SESSIONS_PER_MACHINE: usize = 20;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_TRANSFER_TIMEOUT: Duration = Duration::from_secs(3600);
/// Lifetime of a spliced remote desktop session. A viewer watches for hours, and the bytes stay
/// end to end encrypted (TLS-PSK inside Noise), so only the one-day ceiling of the maho relay
/// protocol applies.
const REMOTE_DESKTOP_SESSION_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

/// What an opaque session carries, as the allocating client declared it. Only its lifetime
/// differs; the relay never parses the spliced frames either way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionKind {
    #[default]
    Http,
    RemoteDesktop,
}

impl SessionKind {
    /// How long a spliced session of this kind may live.
    pub fn lifetime(self) -> Duration {
        match self {
            SessionKind::Http => SESSION_TRANSFER_TIMEOUT,
            SessionKind::RemoteDesktop => REMOTE_DESKTOP_SESSION_TIMEOUT,
        }
    }
}
const PAIRING_CLAIM_LEASE_DURATION: Duration = Duration::from_secs(45);
const MAX_MESSAGE_SIZE: usize = 64 * 1024;

/// Interval on which the background reaper sweeps the session registry for
/// entries that have exceeded [`SESSION_PAIRING_TIMEOUT`] without pairing.
const SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(10);
/// Idle data channels kept per machine for HTTP keep-alive reuse. Every fresh
/// channel costs the daemon a notice plus a new TCP/TLS/WebSocket handshake
/// back through the tunnel, so reuse removes several round trips per request.
const DATA_POOL_PER_MACHINE: usize = 4;
/// Shorter than common proxy idle limits (Cloudflare closes idle WebSockets
/// after ~100s), so a pooled channel is dropped before the path silently dies.
const DATA_POOL_IDLE: Duration = Duration::from_secs(50);

/// Interval on which the suspension sweeper re-evaluates the owners of every live control
/// tunnel and relay session and ends remote access for owners whose grace period ended.
/// One minute bounds how far a stopped owner's live streams can outlive the deadline
/// without turning every paid account into a per-second billing query.
const SUSPENSION_SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// The close reason every stream ended by the suspension sweeper carries. Clients branch on
/// this exact string.
pub const SUSPENSION_REASON: &str = "REMOTE_SUSPENDED";

/// Arming handle for one session's or one control registration's suspension signal.
///
/// A `watch` channel rather than a queue, so a half that starts draining *after* the sweeper
/// armed still observes the reason: the value is state, and a late reader reads it as its
/// first observation instead of missing it.
type SuspensionSignal = Arc<watch::Sender<Option<String>>>;

fn new_suspension_signal() -> SuspensionSignal {
    Arc::new(watch::channel(None).0)
}

/// Resolves with the reason the suspension sweeper armed for this signal.
///
/// A signal whose session or control registration ended on its own is dropped, which closes
/// the channel; that is ordinary teardown, not a suspension, so this future then never
/// resolves and the caller's normal path continues.
async fn next_suspension_reason(signal: &mut watch::Receiver<Option<String>>) -> String {
    loop {
        if let Some(reason) = signal.borrow_and_update().clone() {
            return reason;
        }
        if signal.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

/// Resolves with an armed suspension reason, or never when this direction does not write to
/// the browser half (which is the half that carries the exact reason).
async fn await_suspension(signal: &mut Option<watch::Receiver<Option<String>>>) -> String {
    match signal {
        Some(signal) => next_suspension_reason(signal).await,
        None => std::future::pending().await,
    }
}

/// Which of the two spliced sockets faces the remote browser.
///
/// Only that half receives the exact suspension reason; the daemon half is torn down by the
/// same close.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BrowserSide {
    /// The half this task reserved first (`WaitingHalf.kind`), passed as `a` to
    /// [`proxy_sockets`].
    Own,
    /// The half handed over by the opposite end, passed as `b` to [`proxy_sockets`].
    Peer,
}

/// How a half's pairing wait ended.
// Short-lived stack value that is matched immediately; boxing the socket would only add an
// allocation per pairing.
#[allow(clippy::large_enum_variant)]
enum PairingWait {
    /// The opposite half arrived; the session is live.
    Paired(WebSocket),
    /// The suspension sweeper ended the session before the opposite half arrived.
    Suspended(String),
    /// The registry entry was retired (pairing deadline) before the opposite half arrived.
    Abandoned,
}

pub const RELAY_STAGE_HEADER: &str = "x-ferryx-relay-stage";
/// Response headers a browser client may read across origins: the relay stage, the
/// replacement bearer token the host gateway answers after a transparent rotation, and the
/// e2e marker on attach allocations.
const RELAY_EXPOSED_HEADERS: &str =
    "x-ferryx-relay-stage, x-ferryx-rotated-token, x-ferryx-relay-e2e";
/// Set on every response to an `?e2e=1` attach allocation.
const RELAY_E2E_HEADER: &str = "x-ferryx-relay-e2e";
pub const RELAY_STAGE_DATA_PAIRING_TIMEOUT: &str = "data_pairing_timeout";
pub const RELAY_STAGE_UPSTREAM_TRANSFER_TIMEOUT: &str = "upstream_transfer_timeout";

/// Logged once per plaintext relay-proxied request so operators can see which clients still
/// bypass end-to-end encryption before the relay starts refusing them.
pub(crate) const RELAY_PLAINTEXT_DEPRECATION: &str = "plaintext relay proxy is deprecated and will be refused in the next release; update this client so it connects end-to-end encrypted (?e2e=1)";
const CLIENT_BUILD_HEADER: &str = "x-ferryx-client-build";
const MAX_CLIENT_BUILD_LEN: usize = 128;

/// `FERRYX_RELAY_REQUIRE_E2E=1` makes every plaintext relay proxy route refuse with 426.
fn require_e2e_from_env() -> AtomicBool {
    AtomicBool::new(std::env::var("FERRYX_RELAY_REQUIRE_E2E").is_ok_and(|value| value == "1"))
}

/// The client build a request names: `X-Ferryx-Client-Build`, else `User-Agent`, else
/// `unknown`. Control characters are stripped and the value is bounded so a log line cannot
/// be forged or inflated by the caller.
fn client_build(headers: &HeaderMap) -> String {
    [CLIENT_BUILD_HEADER, "user-agent"]
        .iter()
        .filter_map(|name| headers.get(*name))
        .filter_map(|value| value.to_str().ok())
        .map(|value| {
            value
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_CLIENT_BUILD_LEN)
                .collect::<String>()
        })
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Admits one plaintext relay-proxied request: logs the deprecation, or, when the relay
/// requires end-to-end encryption, answers 426 `E2E_REQUIRED` before any data channel opens.
// The refusal is a full `Response`, matching what every caller hands back to axum.
#[allow(clippy::result_large_err)]
fn plaintext_gate(
    state: &RelayState,
    headers: &HeaderMap,
    machine: &str,
    route: &str,
) -> Result<(), Response> {
    if state.inner.require_e2e.load(Ordering::Relaxed) {
        return Err(crate::remote::server::machine_error_with_details(
            StatusCode::UPGRADE_REQUIRED,
            "E2E_REQUIRED",
            "This relay only carries end-to-end encrypted traffic. Update the client or re-pair this device through your Ferryx account.",
            serde_json::Map::new(),
        ));
    }
    let build = client_build(headers);
    tracing::warn!(
        target: "ferryx::relay::e2e",
        client_build = %build,
        machine = %machine,
        route = %route,
        "{}",
        RELAY_PLAINTEXT_DEPRECATION
    );
    #[cfg(test)]
    {
        state
            .inner
            .plaintext_deprecations
            .fetch_add(1, Ordering::Relaxed);
        *state.inner.last_plaintext_build.lock() = Some(build);
    }
    Ok(())
}

fn relay_timeout_response(stage: &'static str) -> Response {
    let mut response = StatusCode::GATEWAY_TIMEOUT.into_response();
    let headers = response.headers_mut();
    headers.insert(
        RELAY_STAGE_HEADER,
        HeaderValue::from_static(stage),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static(RELAY_STAGE_HEADER),
    );
    response
}

/// Is this forwarded request a non-idempotent reference-chat mutation (submit, stop, answer)?
///
/// The host runs such a transaction on its own task, so once the relay has forwarded the
/// request, losing the answer does not mean the pane was left untouched.
fn reference_chat_mutation(method: &Method, path: &str) -> bool {
    let route = path.split('?').next().unwrap_or_default();
    *method == Method::POST
        && matches!(
            route
                .strip_prefix("/api/v1/")
                .map(|route| route.split('/').collect::<Vec<_>>())
                .as_deref(),
            Some(["reference-chat", _, "submit" | "stop" | "answer"])
        )
}

/// Send, answer and stop type into a pane. A dropped reply is outcome-unknown, never "unsent".
/// Wait and events are reads: a dropped reply stays a gateway timeout.
fn agent_control_mutation(method: &Method, path: &str) -> bool {
    let route = path.split('?').next().unwrap_or_default();
    *method == Method::POST
        && matches!(
            route
                .strip_prefix("/api/v1/")
                .map(|route| route.split('/').collect::<Vec<_>>())
                .as_deref(),
            Some(["agent-control", "sessions", _, "send" | "answer" | "stop"])
        )
}

fn outcome_unknown_mutation(method: &Method, path: &str) -> bool {
    reference_chat_mutation(method, path) || agent_control_mutation(method, path)
}

/// A `wait` may block for the clamped timeout. The relay budget covers that plus a margin,
/// and never more than five minutes plus that margin.
fn agent_control_wait_budget(path: &str) -> Option<Duration> {
    let (route, query) = path.split_once('?')?;
    let parts = route
        .strip_prefix("/api/v1/")?
        .split('/')
        .collect::<Vec<_>>();
    if !matches!(parts.as_slice(), ["agent-control", "sessions", _, "wait"]) {
        return None;
    }
    let timeout_ms = query.split('&').find_map(|field| {
        let (key, value) = field.split_once('=')?;
        (key == "timeoutMs")
            .then(|| value.parse::<u64>().ok())
            .flatten()
    })?;
    let clamped = timeout_ms.min(crate::agent_control::policy::AGENT_CONTROL_TIMEOUT_MAX_MS);
    Some(Duration::from_millis(clamped) + Duration::from_secs(15))
}

/// The frozen ScopeResult failure a forwarded reference-chat mutation answers with when the
/// relay lost the host's answer: the host may have typed it, so it is never retried as unsent.
fn reference_chat_outcome_unknown(request_id: &str) -> Response {
    (
        StatusCode::CONFLICT,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "ok": false,
            "error": {
                "code": crate::remote::reference_chat::types::REFERENCE_OUTCOME_UNKNOWN_CODE,
                "message": "the relay lost the host's answer after forwarding the request; it may have been applied",
                "retryable": false,
                "details": serde_json::Value::Null,
            },
            "requestId": request_id,
        })),
    )
        .into_response()
}

/// Notification pushed down the control channel when a client asks to open
/// a new session against this daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingSessionNotice {
    pub session_id: String,
    #[serde(default)]
    pub opaque: bool,
}

/// Which of the two tunnel halves a pending registry entry holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HalfKind {
    Data,
    Client,
    /// The account client's half of a session whose traffic it encrypts end to end. Kept
    /// distinct from `Client` so a plain session can never be taken over the opaque route
    /// and an opaque session can never be taken over the plain one.
    Opaque,
}

/// An issued session, retained through reservation and active transfer.
/// `notify` hands the second socket to the first connection's task.
struct WaitingHalf {
    generation: u64,
    /// Machine that owns this session, and the control generation it was allocated
    /// under. A half offered after the owner reconnects under a new generation must
    /// not attach to a session authorized by the previous one.
    owner: String,
    control_generation: u64,
    created_at: Instant,
    kind: Option<HalfKind>,
    /// True when the allocation asked for the encrypted attach path. The relay then only
    /// ever splices frames for this session and never parses them.
    opaque: bool,
    /// Declared at allocation; selects the splice lifetime.
    session_kind: SessionKind,
    notify: Option<oneshot::Sender<WebSocket>>,
    active: bool,
    /// Armed by the suspension sweeper when the owning account's grace period ended. A live
    /// splice watches this instead of vanishing when its socket is dropped, so the browser
    /// half still receives the exact reason.
    suspension: SuspensionSignal,
}

/// A data channel whose last HTTP exchange completed cleanly on a keep-alive
/// connection. It stays bound to the control generation that issued it.
struct PooledDataSocket {
    socket: WebSocket,
    control_generation: u64,
    idle_since: Instant,
}

#[derive(Clone)]
struct ControlChannel {
    generation: u64,
    tx: mpsc::Sender<IncomingSessionNotice>,
}

/// Why a session could not be issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueRejection {
    /// No live control channel is registered for the machine.
    Offline,
    /// The caller's control generation is no longer the machine's live one.
    StaleGeneration,
    /// The pending-session budget (relay-wide or per machine) or the notice queue is full.
    Busy,
}

/// True when another waiting (not yet paired) session for `owner` would exceed a budget.
fn pending_budget_exhausted(sessions: &HashMap<String, WaitingHalf>, owner: &str) -> bool {
    let mut total = 0;
    let mut owned = 0;
    for waiting in sessions.values().filter(|waiting| !waiting.active) {
        total += 1;
        if waiting.owner == owner {
            owned += 1;
        }
    }
    total >= MAX_PENDING_SESSIONS || owned >= MAX_PENDING_SESSIONS_PER_MACHINE
}

/// A sealed account grant on its way to a daemon, with the slot its acknowledgement must
/// fill. The relay only ever carries the sealed blob, never the plaintext pairing token.
pub struct GrantDeliveryRequest {
    pub delivery: AccountGrantOfferDelivery,
    pub ack: oneshot::Sender<AccountGrantOfferDelivered>,
}

/// How long a grant delivery waits for the daemon's acknowledgement before the caller is
/// told the machine did not answer.
const GRANT_DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ControlRequest {
    AllocateSession {
        #[serde(default)]
        opaque: bool,
    },
    RegisterPairingPin(RegisterPairingPin),
}

struct RegisteredPairing {
    registration: RegisterPairingPin,
    /// The relay's own authenticated control generation at registration time.
    ///
    /// `registration.generation` is the CLIENT's pairing-attempt counter
    /// (relay_client.rs starts it at 0 per coordinator and bumps it per attempt).
    /// The relay allocates control generations from a relay-wide counter that
    /// session allocation also advances, so the two numbers are unrelated and must
    /// never be compared. Only this relay-stamped value may gate a claim.
    control_generation: u64,
    state: PairingState,
    /// Fence token for the active temporary claim lease. Monotonically increasing
    /// so that stale downstream completions or rollbacks cannot touch subsequent claims.
    claim_fence: u64,
    /// Expiry instant of the active claim lease. If a claim lease expires without
    /// being consumed or rolled back, it can be reclaimed if the registration lease is still valid.
    claim_expires_at: Option<Instant>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicPairExchangeRequest {
    pub pin: Option<String>,
    pub code: Option<String>,
    pub pairing_token: Option<String>,
    pub device_name: String,
    #[serde(default)]
    pub installation_id: Option<String>,
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

/// RAII guard for an in-flight pairing claim lease.
///
/// If downstream exchange fails, times out, or the request future is cancelled,
/// dropping the guard atomically rolls the registration back from `Claimed` to
/// `Ready` (or `Expired` if the pin lease expired). On downstream exchange success,
/// `commit()` transitions the registration to `Consumed`.
struct PairingClaimGuard {
    armed: bool,
    state: RelayState,
    pub machine_id: String,
    pub pin: String,
    pub pairing_token: String,
    pub control_generation: u64,
    pub claim_fence: u64,
}

impl PairingClaimGuard {
    pub fn commit(&mut self) {
        if self.armed {
            self.armed = false;
            self.state.consume_pairing(
                &self.pin,
                &self.pairing_token,
                self.control_generation,
                self.claim_fence,
            );
        }
    }

    pub fn rollback(&mut self) {
        if self.armed {
            self.armed = false;
            self.state.rollback_pairing(
                &self.pin,
                &self.pairing_token,
                self.control_generation,
                self.claim_fence,
            );
        }
    }

    #[cfg(test)]
    pub fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PairingClaimGuard {
    fn drop(&mut self) {
        if self.armed {
            self.state.rollback_pairing(
                &self.pin,
                &self.pairing_token,
                self.control_generation,
                self.claim_fence,
            );
        }
    }
}

impl std::fmt::Debug for PairingClaimGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingClaimGuard")
            .field("armed", &self.armed)
            .field("machine_id", &self.machine_id)
            .field("pin", &self.pin)
            .field("pairing_token", &self.pairing_token)
            .field("control_generation", &self.control_generation)
            .field("claim_fence", &self.claim_fence)
            .finish()
    }
}

/// Shared relay state: the set of authorized Machine Tokens, the live
/// control channels (one per connected daemon), and the registry of
/// sessions awaiting pairing.
#[derive(Clone)]
pub struct RelayState {
    inner: Arc<RelayInner>,
    paired_tokens: Arc<Mutex<HashMap<(String, String), CachedDeviceToken>>>,
}

struct CachedDeviceToken {
    validated_at: Instant,
}

struct RelayInner {
    machine_tokens: Vec<String>,
    /// Audience this relay announces in its control challenge and requires in the
    /// signed transcript, so a signature is bound to THIS relay.
    ///
    /// Defaults to [`LEGACY_CONTROL_AUDIENCE`]; set from `FERRYX_RELAY_AUDIENCE`
    /// (normally the public origin, e.g. `relay.ferryx.dev`) to stop a signature
    /// captured by one relay from being replayed against another.
    control_audience: String,
    control_channels: Mutex<HashMap<String, ControlChannel>>,
    /// Daemon-ward grant deliveries, keyed by machine. Kept beside `control_channels`
    /// rather than inside it so a control socket's session notices and its grant
    /// deliveries never share a queue, and so tests that build a `ControlChannel`
    /// directly keep working with notices alone.
    grant_channels: Mutex<HashMap<String, mpsc::Sender<GrantDeliveryRequest>>>,
    account_state: Mutex<Option<Arc<crate::account::service::AccountState>>>,
    machine_public_keys: Mutex<HashMap<String, String>>,
    key_store_path: Option<std::path::PathBuf>,
    admission: Mutex<HashMap<IpAddr, AttemptTracker>>,
    pairing_admission: Mutex<HashMap<IpAddr, AttemptTracker>>,
    pairings: Mutex<HashMap<String, RegisteredPairing>>,
    next_generation: AtomicU64,
    pending_sessions: Mutex<HashMap<String, WaitingHalf>>,
    /// Suspension signals keyed by control generation, so that every alias of one control
    /// tunnel resolves to the same live registration no matter which name the sweeper matched.
    control_suspensions: Mutex<HashMap<u64, SuspensionSignal>>,
    pending_socket_tickets: Mutex<HashMap<String, (String, String, String, u64)>>,
    /// Device tokens a machine recently refused, keyed by (machine, token), so a repeated bad
    /// bearer is answered without opening another data channel to the host.
    rejected_device_tokens: Mutex<HashMap<(String, String), Instant>>,
    /// Lock order: always after `control_channels`.
    data_pool: Mutex<HashMap<String, Vec<PooledDataSocket>>>,
    /// Refuse plaintext relay proxy routes (`FERRYX_RELAY_REQUIRE_E2E=1`).
    require_e2e: AtomicBool,
    #[cfg(test)]
    spliced_taps: Mutex<HashMap<String, Arc<Mutex<Vec<u8>>>>>,
    #[cfg(test)]
    plaintext_deprecations: AtomicU64,
    #[cfg(test)]
    last_plaintext_build: Mutex<Option<String>>,
}

/// Audience used before challenges carried one.
///
/// A client that predates the audience field signs this constant, so the relay keeps
/// accepting it; a relay that sets a distinct audience no longer shares a transcript
/// with any other deployment.
pub const LEGACY_CONTROL_AUDIENCE: &str = "relay";

const MACHINE_ID_CLAIMED: &str = "Machine ID already claimed by another public key";
const ENROLLMENT_TOKEN_REQUIRED: &str = "Enrollment token required for private relay";

const ADMISSION_WINDOW: Duration = Duration::from_secs(60);
const CONTROL_AUTH_TIMEOUT: Duration = Duration::from_secs(5);
/// How long an authenticated control socket may send nothing (not even a Ping) before the
/// relay treats it as half-open.
const CONTROL_READ_IDLE: Duration = Duration::from_secs(60);
const DEVICE_TOKEN_CACHE_TTL: Duration = Duration::from_secs(30);
const MAX_CACHED_DEVICE_TOKENS: usize = 1_000;
const REJECTED_DEVICE_TOKEN_TTL: Duration = Duration::from_secs(30);
const MAX_REJECTED_DEVICE_TOKENS: usize = 1_000;

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
        let path = std::env::var_os("FERRYX_RELAY_KEY_FILE")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("FERRYX_RELAY_DATA_DIR")
                    .map(|dir| std::path::PathBuf::from(dir).join("machine_keys.json"))
            })
            .or_else(|| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|dir| {
                        std::path::PathBuf::from(dir).join(".ferryx/relay/machine_keys.json")
                    })
            })
            .expect("Cannot resolve relay machine key store");
        Self::new_with_key_store(machine_tokens, path)
            .expect("Failed to load relay machine key store")
    }

    pub fn new_with_key_store(
        machine_tokens: Vec<String>,
        path: impl AsRef<std::path::Path>,
    ) -> std::io::Result<Self> {
        Self::with_key_store(machine_tokens, Some(path.as_ref().to_path_buf()))
    }

    /// Builds a relay that announces and verifies `audience`.
    ///
    /// Normally the deployment's public origin. Binding it makes a control
    /// signature usable only against this relay.
    pub fn new_with_audience(
        machine_tokens: Vec<String>,
        path: impl AsRef<std::path::Path>,
        audience: impl Into<String>,
    ) -> std::io::Result<Self> {
        let state = Self::with_key_store(machine_tokens, Some(path.as_ref().to_path_buf()))?;
        let keys = state.inner.machine_public_keys.lock().clone();
        Ok(Self {
            paired_tokens: Arc::clone(&state.paired_tokens),
            inner: Arc::new(RelayInner {
                control_audience: audience.into(),
                machine_tokens: state.inner.machine_tokens.clone(),
                control_channels: Mutex::new(HashMap::new()),
                grant_channels: Mutex::new(HashMap::new()),
                account_state: Mutex::new(None),
                machine_public_keys: Mutex::new(keys),
                key_store_path: state.inner.key_store_path.clone(),
                admission: Mutex::new(HashMap::new()),
                pairing_admission: Mutex::new(HashMap::new()),
                pairings: Mutex::new(HashMap::new()),
                next_generation: AtomicU64::new(1),
                pending_sessions: Mutex::new(HashMap::new()),
                control_suspensions: Mutex::new(HashMap::new()),
                pending_socket_tickets: Mutex::new(HashMap::new()),
                rejected_device_tokens: Mutex::new(HashMap::new()),
                data_pool: Mutex::new(HashMap::new()),
                require_e2e: require_e2e_from_env(),
                #[cfg(test)]
                spliced_taps: Mutex::new(HashMap::new()),
                #[cfg(test)]
                plaintext_deprecations: AtomicU64::new(0),
                #[cfg(test)]
                last_plaintext_build: Mutex::new(None),
            }),
        })
    }

    fn with_key_store(
        machine_tokens: Vec<String>,
        key_store_path: Option<std::path::PathBuf>,
    ) -> std::io::Result<Self> {
        let keys: HashMap<String, String> =
            match key_store_path.as_ref().map(std::fs::read).transpose() {
                Ok(Some(bytes)) => serde_json::from_slice(&bytes)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
                Ok(None) => HashMap::new(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
                Err(error) => return Err(error),
            };
        // A persisted record is an authorization decision, so reject a store that cannot
        // be trusted rather than loading entries that would admit or lock out a machine.
        // Keys are base64-encoded Ed25519 public keys (32 bytes -> 44 chars with padding).
        if let Some((machine, _)) = keys.iter().find(|(machine, key)| {
            machine.is_empty() || key.is_empty() || !crate::remote::auth::is_valid_public_key(key)
        }) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid machine ownership record for {machine}"),
            ));
        }
        Ok(Self {
            paired_tokens: Arc::new(Mutex::new(HashMap::new())),
            inner: Arc::new(RelayInner {
                machine_tokens,
                control_audience: std::env::var("FERRYX_RELAY_AUDIENCE")
                    .ok()
                    .map(|value| value.trim().to_owned())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| LEGACY_CONTROL_AUDIENCE.to_string()),
                next_generation: AtomicU64::new(1),
                control_channels: Mutex::new(HashMap::new()),
                grant_channels: Mutex::new(HashMap::new()),
                account_state: Mutex::new(None),
                machine_public_keys: Mutex::new(keys),
                key_store_path,
                admission: Mutex::new(HashMap::new()),
                pairing_admission: Mutex::new(HashMap::new()),
                pairings: Mutex::new(HashMap::new()),
                pending_sessions: Mutex::new(HashMap::new()),
                control_suspensions: Mutex::new(HashMap::new()),
                pending_socket_tickets: Mutex::new(HashMap::new()),
                rejected_device_tokens: Mutex::new(HashMap::new()),
                data_pool: Mutex::new(HashMap::new()),
                require_e2e: require_e2e_from_env(),
                #[cfg(test)]
                spliced_taps: Mutex::new(HashMap::new()),
                #[cfg(test)]
                plaintext_deprecations: AtomicU64::new(0),
                #[cfg(test)]
                last_plaintext_build: Mutex::new(None),
            }),
        })
    }

    #[cfg(test)]
    pub fn set_require_e2e(&self, on: bool) {
        self.inner.require_e2e.store(on, Ordering::Relaxed);
    }

    /// Cache a token just issued or revalidated by this machine's gateway.
    pub fn set_account_state(&self, account: Arc<crate::account::service::AccountState>) {
        let previous = {
            let mut guard = self.inner.account_state.lock();
            let prev = guard.take();
            *guard = Some(account.clone());
            prev
        };
        if let Some(prev) = previous {
            if !Arc::ptr_eq(&prev, &account) {
                prev.set_liveness_probe(None);
            }
        }
        let weak_inner = Arc::downgrade(&self.inner);
        account.set_liveness_probe(Some(Arc::new(move |machine_id: &str| {
            let Some(inner) = weak_inner.upgrade() else {
                return false;
            };
            let online = inner
                .control_channels
                .lock()
                .get(machine_id)
                .is_some_and(|ch| !ch.tx.is_closed());
            online
        })));
    }

    pub fn clear_account_state(&self) {
        let previous = self.inner.account_state.lock().take();
        if let Some(prev) = previous {
            prev.set_liveness_probe(None);
        }
    }

    #[cfg(test)]
    pub fn tap_session_traffic(&self, session_id: &str) -> Arc<Mutex<Vec<u8>>> {
        let log = Arc::new(Mutex::new(Vec::new()));
        self.inner
            .spliced_taps
            .lock()
            .insert(session_id.to_string(), Arc::clone(&log));
        log
    }

    pub fn account_state(&self) -> Option<Arc<crate::account::service::AccountState>> {
        self.inner.account_state.lock().clone()
    }

    /// The suspension signal for one session, or a signal that never fires when the session
    /// is already gone.
    fn session_suspension_receiver(&self, session_id: &str) -> watch::Receiver<Option<String>> {
        self.inner
            .pending_sessions
            .lock()
            .get(session_id)
            .map(|entry| entry.suspension.subscribe())
            .unwrap_or_else(|| watch::channel(None).1)
    }

    /// The suspension signal for one control registration, or a signal that never fires when
    /// the registration is gone.
    fn control_suspension_receiver(&self, generation: u64) -> watch::Receiver<Option<String>> {
        self.inner
            .control_suspensions
            .lock()
            .get(&generation)
            .map(|signal| signal.subscribe())
            .unwrap_or_else(|| watch::channel(None).1)
    }

    pub fn register_device_token(&self, machine_id: &str, token: &str) {
        self.inner
            .rejected_device_tokens
            .lock()
            .remove(&(machine_id.to_owned(), token.to_owned()));
        let mut tokens = self.paired_tokens.lock();
        let now = Instant::now();
        tokens.retain(|_, cached| now.duration_since(cached.validated_at) < DEVICE_TOKEN_CACHE_TTL);
        if tokens.len() >= MAX_CACHED_DEVICE_TOKENS
            && !tokens.contains_key(&(machine_id.to_owned(), token.to_owned()))
        {
            if let Some(oldest) = tokens
                .iter()
                .min_by_key(|(_, cached)| cached.validated_at)
                .map(|(key, _)| key.clone())
            {
                tokens.remove(&oldest);
            }
        }
        tokens.insert(
            (machine_id.to_owned(), token.to_owned()),
            CachedDeviceToken { validated_at: now },
        );
    }

    fn has_fresh_device_token(&self, machine_id: &str, token: &str) -> bool {
        let key = (machine_id.to_owned(), token.to_owned());
        let mut tokens = self.paired_tokens.lock();
        let fresh = tokens
            .get(&key)
            .is_some_and(|cached| cached.validated_at.elapsed() < DEVICE_TOKEN_CACHE_TTL);
        if !fresh {
            tokens.remove(&key);
        }
        fresh
    }

    async fn authorize_device_token(
        &self,
        machine_id: &str,
        token: &str,
    ) -> Result<bool, StatusCode> {
        if self.has_fresh_device_token(machine_id, token) {
            return Ok(true);
        }
        let key = (machine_id.to_owned(), token.to_owned());
        if self
            .inner
            .rejected_device_tokens
            .lock()
            .get(&key)
            .is_some_and(|rejected_at| rejected_at.elapsed() < REJECTED_DEVICE_TOKEN_TTL)
        {
            return Ok(false);
        }
        let mut headers = HeaderMap::new();
        let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) else {
            return Ok(false);
        };
        headers.insert(header::AUTHORIZATION, value);
        let response = proxy_http(
            self,
            machine_id,
            None,
            Method::GET,
            "/api/v1/devices",
            headers,
            Vec::new(),
        )
        .await?;
        if response.status().is_success() {
            self.register_device_token(machine_id, token);
            Ok(true)
        } else if response.status() == StatusCode::UNAUTHORIZED
            || response.status() == StatusCode::FORBIDDEN
        {
            let now = Instant::now();
            let mut rejected = self.inner.rejected_device_tokens.lock();
            if rejected.len() >= MAX_REJECTED_DEVICE_TOKENS {
                rejected.retain(|_, at| now.duration_since(*at) < REJECTED_DEVICE_TOKEN_TTL);
            }
            if rejected.len() < MAX_REJECTED_DEVICE_TOKENS {
                rejected.insert(key, now);
            }
            Ok(false)
        } else {
            Err(response.status())
        }
    }

    fn bind_machine_key(&self, auth: &ControlAuth) -> Result<(), String> {
        let account_record = if let Some(account) = self.account_state() {
            if account.deployment_mode.is_billing_enabled() {
                crate::account::service::try_enrolled_machine_by_id(&account, &auth.machine_id)
                    .map_err(|error| {
                        format!(
                            "ACCOUNT_ENTITLEMENT_UNAVAILABLE: {}: {}",
                            error.code, error.message
                        )
                    })?
            } else {
                crate::account::service::enrolled_machine_by_id(&account, &auth.machine_id)
            }
        } else {
            None
        };
        let account_enrolled = account_record
            .as_ref()
            .is_some_and(|record| record.public_key == auth.public_key);

        if account_enrolled {
            if let Some(account) = self.account_state() {
                if account.deployment_mode.is_billing_enabled() {
                    let record = account_record.as_ref().unwrap();
                    let now = crate::account::billing::routes::billing_now();
                    match crate::account::billing::routes::entitlement_for_user(
                        &account,
                        &record.owner_user_id,
                        now,
                    ) {
                        Ok(entitlement) => {
                            if entitlement.status == crate::account::billing::entitlement::EntitlementStatus::Stopped {
                                return Err("REMOTE_SUSPENDED".into());
                            }
                        }
                        Err(error) => {
                            return Err(format!("ACCOUNT_ENTITLEMENT_UNAVAILABLE: {}: {}", error.code, error.message));
                        }
                    }
                }
            }
        }

        // Serialize ownership checks and durable enrollment under the same lock.
        let mut keys = self.inner.machine_public_keys.lock();
        if let Some(key) = keys.get(&auth.machine_id) {
            return if key == &auth.public_key {
                Ok(())
            } else {
                Err(MACHINE_ID_CLAIMED.into())
            };
        }
        // A private relay requires an operator-issued enrollment token. An account-enrolled
        // machine on an account-enabled relay satisfies that requirement with the key it
        // enrolled instead, because the account store already pins this machine id to it.
        if !self.inner.machine_tokens.is_empty()
            && !auth
                .enrollment_token
                .as_deref()
                .is_some_and(|token| self.validate_machine_token(token))
            && !account_enrolled
        {
            return Err(ENROLLMENT_TOKEN_REQUIRED.into());
        }
        let mut enrolled = keys.clone();
        if let Some(path) = &self.inner.key_store_path {
            // Another relay process may have enrolled machines since this one loaded the
            // store. Writing our in-memory snapshot would silently drop their records, so
            // re-read and merge under a cross-process transaction lock before persisting.
            // The publication helper uses a separate .lock sidecar; keeping this
            // .tx.lock file open covers both the reload and atomic publication.
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    format!("Failed to create machine ownership directory: {error}")
                })?;
            }
            let mut options = std::fs::OpenOptions::new();
            options.create(true).truncate(false).read(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            let transaction = options
                .open(path.with_extension("tx.lock"))
                .map_err(|error| {
                    format!("Failed to open machine ownership transaction: {error}")
                })?;
            transaction.lock().map_err(|error| {
                format!("Failed to lock machine ownership transaction: {error}")
            })?;
            #[cfg(test)]
            tests::probe_enrollment_transaction(path, "read");
            match std::fs::read(path) {
                Ok(bytes) => {
                    let on_disk: HashMap<String, String> = serde_json::from_slice(&bytes)
                        .map_err(|error| format!("Corrupt machine ownership store: {error}"))?;
                    for (machine, key) in on_disk {
                        match enrolled.get(&machine) {
                            // A conflicting on-disk owner must not be silently replaced.
                            Some(existing) if existing != &key => {
                                return Err(
                                    MACHINE_ID_CLAIMED.into()
                                );
                            }
                            Some(_) => {}
                            None => {
                                enrolled.insert(machine, key);
                            }
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!("Failed to read machine ownership: {error}"));
                }
            }
            // Re-check after merging: the claim may have been taken by another process.
            if enrolled
                .get(&auth.machine_id)
                .is_some_and(|key| key != &auth.public_key)
            {
                return Err(MACHINE_ID_CLAIMED.into());
            }
            enrolled.insert(auth.machine_id.clone(), auth.public_key.clone());
            #[cfg(test)]
            tests::probe_enrollment_transaction(path, "publication");
            write_private_json(path, &enrolled)
                .map_err(|error| format!("Failed to persist machine ownership: {error}"))?;
        } else {
            enrolled.insert(auth.machine_id.clone(), auth.public_key.clone());
        }
        *keys = enrolled;
        Ok(())
    }

    fn admit(&self, ip: IpAddr, now: Instant) -> bool {
        // Expired trackers are pruned by the session sweeper, not on every request: a full
        // scan per admission is O(n) under the global lock.
        let mut admission = self.inner.admission.lock();
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
        let mut admission = self.inner.admission.lock();
        if success {
            if let Some(tracker) = admission.get_mut(&ip) {
                tracker.failures = 0;
            }
            return;
        }
        // A failure is recorded even for an address that never went through `admit` (the
        // device-token routes), so repeated bad bearers reach the same lockout.
        let now = Instant::now();
        let tracker = admission.entry(ip).or_insert(AttemptTracker {
            started: now,
            attempts: 0,
            failures: 0,
            locked_until: None,
        });
        tracker.failures += 1;
        if tracker.failures >= 5 {
            tracker.locked_until = Some(now + ADMISSION_WINDOW);
        }
    }

    /// True while `ip` is locked out by repeated authentication failures.
    fn locked_out(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        self.inner
            .admission
            .lock()
            .get(&ip)
            .is_some_and(|tracker| tracker.locked_until.is_some_and(|until| now < until))
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

    pub fn register_control_channel(
        &self,
        machine_id: String,
    ) -> (
        u64,
        mpsc::Receiver<IncomingSessionNotice>,
        mpsc::Receiver<GrantDeliveryRequest>,
    ) {
        let (generation, rx, grant_rx, _, _) = self.register_control_channel_handle(machine_id);
        (generation, rx, grant_rx)
    }

    /// [`Self::register_control_channel`], also returning the registered channel and grant
    /// sender so the owning socket can bind aliases to exactly its own registration.
    fn register_control_channel_handle(
        &self,
        machine_id: String,
    ) -> (
        u64,
        mpsc::Receiver<IncomingSessionNotice>,
        mpsc::Receiver<GrantDeliveryRequest>,
        ControlChannel,
        mpsc::Sender<GrantDeliveryRequest>,
    ) {
        let (tx, rx) = mpsc::channel(MAX_PENDING_SESSIONS);
        let (grant_tx, grant_rx) = mpsc::channel(1);
        let generation = self.inner.next_generation.fetch_add(1, Ordering::Relaxed);
        let channel = ControlChannel { generation, tx };
        {
            // One lock order everywhere: `control_channels` before `grant_channels`.
            let mut channels = self.inner.control_channels.lock();
            let mut grants = self.inner.grant_channels.lock();
            channels.insert(machine_id.clone(), channel.clone());
            grants.insert(machine_id, grant_tx.clone());
        }
        self.inner
            .control_suspensions
            .lock()
            .insert(generation, new_suspension_signal());
        (generation, rx, grant_rx, channel, grant_tx)
    }

    fn unregister_control_channel(&self, _machine_token: &str, generation: u64) {
        let mut channels = self.inner.control_channels.lock();
        let mut grants = self.inner.grant_channels.lock();
        let retired: Vec<String> = channels
            .iter()
            .filter(|(_, channel)| channel.generation == generation)
            .map(|(machine, _)| machine.clone())
            .collect();
        channels.retain(|_, channel| channel.generation != generation);
        let mut pool = self.inner.data_pool.lock();
        for machine in retired {
            grants.remove(&machine);
            if let Some(idle) = pool.get_mut(&machine) {
                idle.retain(|entry| entry.control_generation != generation);
            }
        }
        drop(pool);
        drop(grants);
        drop(channels);
        self.inner.control_suspensions.lock().remove(&generation);
    }

    /// Takes an idle data channel still backed by the machine's live control
    /// generation (and the caller's expected one, when given).
    fn take_pooled_data(
        &self,
        machine: &str,
        expected_generation: Option<u64>,
    ) -> Option<(WebSocket, u64)> {
        let live = self.inner.control_channels.lock().get(machine)?.generation;
        if expected_generation.is_some_and(|expected| expected != live) {
            return None;
        }
        let mut pool = self.inner.data_pool.lock();
        let idle = pool.get_mut(machine)?;
        while let Some(entry) = idle.pop() {
            if entry.control_generation == live && entry.idle_since.elapsed() < DATA_POOL_IDLE {
                return Some((entry.socket, live));
            }
        }
        None
    }

    fn return_pooled_data(&self, machine: &str, control_generation: u64, socket: WebSocket) {
        let channels = self.inner.control_channels.lock();
        if channels.get(machine).map(|channel| channel.generation) != Some(control_generation) {
            return;
        }
        let mut pool = self.inner.data_pool.lock();
        let idle = pool.entry(machine.to_owned()).or_default();
        if idle.len() < DATA_POOL_PER_MACHINE {
            idle.push(PooledDataSocket {
                socket,
                control_generation,
                idle_since: Instant::now(),
            });
        }
    }

    /// Hands one sealed grant to a machine's live control socket and waits for the
    /// daemon's acknowledgement. The envelope is already sealed to the daemon's attach
    /// key, so the relay carries ciphertext and never a plaintext pairing token.
    pub async fn deliver_grant_offer(
        &self,
        machine_id: &str,
        envelope: crate::remote::account_protocol::AccountGrantOfferEnvelope,
    ) -> Result<AccountGrantOfferDelivered, StatusCode> {
        let sender = {
            let channels = self.inner.grant_channels.lock();
            channels.get(machine_id).cloned()
        }
        .ok_or(StatusCode::NOT_FOUND)?;
        let (ack_tx, ack_rx) = oneshot::channel();
        let delivery = AccountGrantOfferDelivery {
            machine_id: machine_id.to_owned(),
            envelope,
        };
        sender
            .try_send(GrantDeliveryRequest {
                delivery,
                ack: ack_tx,
            })
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        match timeout(GRANT_DELIVERY_TIMEOUT, ack_rx).await {
            Ok(Ok(delivered)) => Ok(delivered),
            Ok(Err(_)) => Err(StatusCode::SERVICE_UNAVAILABLE),
            Err(_) => Err(StatusCode::GATEWAY_TIMEOUT),
        }
    }

    /// Test shorthand for [`Self::bind_control_alias_for`] that binds whatever channel is
    /// currently registered under `machine_token`.
    #[cfg(test)]
    pub fn bind_control_alias(&self, machine_token: &str, alias: String) -> bool {
        let channel = self.inner.control_channels.lock().get(machine_token).cloned();
        let grant = self.inner.grant_channels.lock().get(machine_token).cloned();
        match (channel, grant) {
            (Some(channel), Some(grant)) => self.bind_control_alias_for(&channel, &grant, alias),
            _ => false,
        }
    }

    /// Makes the caller's own control registration (`channel`, `grant`) reachable as `alias`
    /// (the machine id a token-authenticated daemon announces): session allocation and grant
    /// delivery both look the machine up by that id, so both maps get the alias.
    /// Unregistering the generation retires every name it was bound to.
    ///
    /// The alias is bound to the caller's generation, never to whatever currently sits under
    /// a shared token, and it is refused when another live control generation already holds
    /// it or when an Ed25519 identity has enrolled that machine id: a token-authenticated
    /// daemon must not take over another machine's routing.
    fn bind_control_alias_for(
        &self,
        channel: &ControlChannel,
        grant: &mpsc::Sender<GrantDeliveryRequest>,
        alias: String,
    ) -> bool {
        if self.inner.machine_public_keys.lock().contains_key(&alias) {
            return false;
        }
        // One lock order everywhere: `control_channels` before `grant_channels`.
        let mut channels = self.inner.control_channels.lock();
        let mut grants = self.inner.grant_channels.lock();
        if channels.get(&alias).is_some_and(|existing| {
            existing.generation != channel.generation && !existing.tx.is_closed()
        }) {
            return false;
        }
        grants.insert(alias.clone(), grant.clone());
        channels.insert(alias, channel.clone());
        true
    }

    /// Trusted in-process issuance API. The caller must supply an opaque,
    /// unpredictable ID and a machine identity with a live control channel.
    /// Registry insertion and notification are atomic with respect to admission.
    pub fn notify_incoming_session(&self, machine_token: &str, session_id: &str) -> bool {
        self.issue_session(machine_token, session_id, None, false)
    }

    pub fn issue_session(
        &self,
        machine_token: &str,
        session_id: &str,
        generation: Option<u64>,
        opaque: bool,
    ) -> bool {
        self.issue_session_of_kind(machine_token, session_id, generation, opaque, SessionKind::Http)
    }

    /// [`Self::issue_session`] for a declared [`SessionKind`]. The kind is recorded in the same
    /// registry insertion the daemon's notice races against, so either half sees it. A plain
    /// session is parsed by the relay and always keeps the default kind.
    pub fn issue_session_of_kind(
        &self,
        machine_token: &str,
        session_id: &str,
        generation: Option<u64>,
        opaque: bool,
        session_kind: SessionKind,
    ) -> bool {
        self.try_issue_session_of_kind(machine_token, session_id, generation, opaque, session_kind)
            .is_ok()
    }

    /// [`Self::issue_session_of_kind`] with the reason a session was refused.
    pub fn try_issue_session_of_kind(
        &self,
        machine_token: &str,
        session_id: &str,
        generation: Option<u64>,
        opaque: bool,
        session_kind: SessionKind,
    ) -> Result<(), IssueRejection> {
        let channels = self.inner.control_channels.lock();
        let Some(channel) = channels.get(machine_token) else {
            return Err(IssueRejection::Offline);
        };
        if generation.is_some_and(|id| id != channel.generation) {
            return Err(IssueRejection::StaleGeneration);
        }
        let mut sessions = self.inner.pending_sessions.lock();
        if pending_budget_exhausted(&sessions, machine_token) || sessions.contains_key(session_id) {
            return Err(IssueRejection::Busy);
        }
        match channel.tx.try_send(IncomingSessionNotice {
            session_id: session_id.into(),
            opaque,
        }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => return Err(IssueRejection::Busy),
            Err(mpsc::error::TrySendError::Closed(_)) => return Err(IssueRejection::Offline),
        }
        sessions.insert(
            session_id.into(),
            WaitingHalf {
                generation: self.inner.next_generation.fetch_add(1, Ordering::Relaxed),
                owner: machine_token.to_owned(),
                control_generation: channel.generation,
                created_at: Instant::now(),
                kind: None,
                opaque,
                session_kind: if opaque { session_kind } else { SessionKind::Http },
                notify: None,
                active: false,
                suspension: new_suspension_signal(),
            },
        );
        Ok(())
    }

    /// The lifetime a splice of `session_id` gets; the default for an unknown session.
    fn session_lifetime(&self, session_id: &str) -> Duration {
        self.inner
            .pending_sessions
            .lock()
            .get(session_id)
            .map_or(SessionKind::Http, |waiting| waiting.session_kind)
            .lifetime()
    }

    /// Reserve before upgrade so concurrent duplicates receive HTTP 409.
    fn reserve_half(
        &self,
        session_id: &str,
        kind: HalfKind,
    ) -> Result<(u64, PairingOutcome), StatusCode> {
        // Lock order matches allocate_session (control_channels before
        // pending_sessions) so the two paths cannot deadlock against each other.
        let channels = self.inner.control_channels.lock();
        let mut sessions = self.inner.pending_sessions.lock();
        let waiting = sessions.get_mut(session_id).ok_or(StatusCode::NOT_FOUND)?;
        // An allocation for the encrypted path is only ever taken by the opaque route, and a
        // plain allocation is never taken by it: the two carry different traffic and must not
        // be interchangeable.
        match kind {
            HalfKind::Opaque if !waiting.opaque => return Err(StatusCode::NOT_FOUND),
            HalfKind::Client if waiting.opaque => return Err(StatusCode::NOT_FOUND),
            _ => {}
        }
        if waiting.active || waiting.kind == Some(kind) {
            return Err(StatusCode::CONFLICT);
        }
        if waiting.created_at.elapsed() >= SESSION_PAIRING_TIMEOUT {
            return Err(StatusCode::GONE);
        }
        // The authorization that created this session belongs to one control
        // generation. If the owner has since reconnected (new generation) or gone
        // away, the session is no longer backed by a live authorization.
        if channels
            .get(&waiting.owner)
            .map(|channel| channel.generation)
            != Some(waiting.control_generation)
        {
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
        let now_secs = current_time_secs();
        self.inner
            .pending_socket_tickets
            .lock()
            .retain(|_, (_, _, _, expiry)| *expiry > now_secs);
        self.paired_tokens
            .lock()
            .retain(|_, cached| now.duration_since(cached.validated_at) < DEVICE_TOKEN_CACHE_TTL);
        self.inner
            .rejected_device_tokens
            .lock()
            .retain(|_, at| now.duration_since(*at) < REJECTED_DEVICE_TOKEN_TTL);
        self.inner
            .pairings
            .lock()
            .retain(|_, p| p.registration.expires_at > now_secs);
        let live = |tracker: &AttemptTracker| {
            now.duration_since(tracker.started) < ADMISSION_WINDOW
                || tracker.locked_until.is_some_and(|until| now < until)
        };
        self.inner.admission.lock().retain(|_, tracker| live(tracker));
        self.inner
            .pairing_admission
            .lock()
            .retain(|_, tracker| live(tracker));
        let mut sessions = self.inner.pending_sessions.lock();
        sessions.retain(|_, waiting| {
            waiting.active || waiting.created_at.elapsed() < SESSION_PAIRING_TIMEOUT
        });
        drop(sessions);
        let mut pool = self.inner.data_pool.lock();
        pool.retain(|_, idle| {
            idle.retain(|entry| entry.idle_since.elapsed() < DATA_POOL_IDLE);
            !idle.is_empty()
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
        if channels
            .get(machine)
            .is_none_or(|c| c.generation != generation)
            || registration.machine_id != machine
            || registration.pin.len() != 6
            || !registration.pin.bytes().all(|b| b.is_ascii_digit())
            || !(16..=64).contains(&registration.pairing_token.len())
            || !registration
                .pairing_token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || registration.expires_at <= current_time_secs()
            || registration.expires_at > current_time_secs() + MAX_PAIRING_LEASE
        {
            return Err("Invalid pairing registration");
        }
        let mut pairings = self.inner.pairings.lock();
        pairings.retain(|_, p| p.registration.expires_at > current_time_secs());
        // A daemon that retries after a lost ACK re-sends the identical registration.
        // Treat that as already-satisfied instead of an error, without consuming another
        // slot against MAX_ACTIVE_PAIRING_PINS_PER_MACHINE and without refreshing the
        // lease (which would let a caller keep one short code alive indefinitely).
        if let Some(existing) = pairings.get(&registration.pin) {
            // Idempotency must not resurrect a record owned by a superseded control
            // generation: the retry has to come from the same owner AND the same live
            // control generation, not merely reuse the machine id and token.
            let same_request = existing.registration.machine_id == registration.machine_id
                && existing.registration.pairing_token == registration.pairing_token
                && existing.control_generation == generation;
            return if same_request {
                Ok(())
            } else {
                Err("Pairing code already registered")
            };
        }
        if pairings
            .values()
            .any(|p| p.registration.pairing_token == registration.pairing_token)
        {
            return Err("Pairing code already registered");
        }
        if pairings
            .values()
            .filter(|p| p.registration.machine_id == machine)
            .count()
            >= MAX_ACTIVE_PAIRING_PINS_PER_MACHINE
        {
            return Err("Maximum active pairing codes exceeded");
        }
        pairings.insert(
            registration.pin.clone(),
            RegisteredPairing {
                registration,
                control_generation: generation,
                state: PairingState::Ready,
                claim_fence: 0,
                claim_expires_at: None,
            },
        );
        Ok(())
    }

    fn consume_pairing(&self, pin: &str, token: &str, control_generation: u64, claim_fence: u64) {
        let mut pairings = self.inner.pairings.lock();
        if let Some(p) = pairings.get_mut(pin) {
            if p.state == PairingState::Claimed
                && p.registration.pairing_token == token
                && p.control_generation == control_generation
                && p.claim_fence == claim_fence
            {
                p.claim_expires_at = None;
                p.state = PairingState::Consumed;
            }
        }
    }

    fn rollback_pairing(&self, pin: &str, token: &str, control_generation: u64, claim_fence: u64) {
        let mut pairings = self.inner.pairings.lock();
        if let Some(p) = pairings.get_mut(pin) {
            if p.state == PairingState::Claimed
                && p.registration.pairing_token == token
                && p.control_generation == control_generation
                && p.claim_fence == claim_fence
            {
                p.claim_expires_at = None;
                if p.registration.expires_at > current_time_secs() {
                    p.state = PairingState::Ready;
                } else {
                    p.state = PairingState::Expired;
                }
            }
        }
    }

    fn claim_pairing(
        &self,
        ip: IpAddr,
        payload: &PublicPairExchangeRequest,
        machine: Option<&str>,
    ) -> Result<PairingClaimGuard, StatusCode> {
        let now = Instant::now();
        // Expired trackers are pruned by the session sweeper; a stale one is reset here.
        let mut admission = self.inner.pairing_admission.lock();
        let tracker = admission.entry(ip).or_insert(AttemptTracker {
            started: now,
            attempts: 0,
            failures: 0,
            locked_until: None,
        });
        if tracker.locked_until.is_some_and(|until| now < until) {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        if now.duration_since(tracker.started) >= ADMISSION_WINDOW {
            *tracker = AttemptTracker {
                started: now,
                attempts: 0,
                failures: 0,
                locked_until: None,
            };
        }
        // Snapshot live control generations first. register_pairing takes
        // control_channels before pairings, so acquiring them in that same order here
        // keeps the two paths deadlock-free.
        let live_generations: HashMap<String, u64> = self
            .inner
            .control_channels
            .lock()
            .iter()
            .map(|(machine, channel)| (machine.clone(), channel.generation))
            .collect();
        let mut pairings = self.inner.pairings.lock();
        let pairing = pairings.values_mut().find(|p| {
            if let Some(token) = &payload.pairing_token {
                &p.registration.pairing_token == token
            } else {
                payload.pin.as_ref().map_or_else(
                    || {
                        payload.code.as_ref().is_some_and(|code| {
                            code == &p.registration.pin || code == &p.registration.pairing_token
                        })
                    },
                    |pin| pin == &p.registration.pin,
                )
            }
        });
        if let Some(p) = pairing {
            // The credentials handed back are attributed to the registering machine, so
            // that machine must still be the live control identity for the generation it
            // registered under. Otherwise a code registered by an earlier (or departed)
            // control channel could still mint credentials for that identity.
            // Compare the relay's own stamped generation - never the client-supplied
            // attempt counter, and with no optional-field bypass.
            let control_identity_is_live = live_generations
                .get(&p.registration.machine_id)
                .is_some_and(|current| *current == p.control_generation);
            let is_claim_available = match p.state {
                PairingState::Ready => true,
                PairingState::Claimed => p
                    .claim_expires_at
                    .is_some_and(|expires_at| now >= expires_at),
                _ => false,
            };
            if is_claim_available
                && p.registration.expires_at > current_time_secs()
                && machine.is_none_or(|m| m == p.registration.machine_id)
                && control_identity_is_live
            {
                let claim_fence = self.inner.next_generation.fetch_add(1, Ordering::Relaxed);
                p.state = PairingState::Claimed;
                p.claim_fence = claim_fence;
                p.claim_expires_at = Some(now + PAIRING_CLAIM_LEASE_DURATION);
                // A caller can register its own known PINs. A match must not
                // replenish its guessing budget before daemon authentication.
                // The validated control generation travels with the claim so dispatch
                // can refuse a channel that was replaced after this check.
                return Ok(PairingClaimGuard {
                    armed: true,
                    state: self.clone(),
                    machine_id: p.registration.machine_id.clone(),
                    pin: p.registration.pin.clone(),
                    pairing_token: p.registration.pairing_token.clone(),
                    control_generation: p.control_generation,
                    claim_fence,
                });
            }
        }
        tracker.failures += 1;
        if tracker.failures >= 5 {
            tracker.locked_until = Some(now + ADMISSION_WINDOW);
        }
        Err(StatusCode::NOT_FOUND)
    }

    /// How long `ip` stays locked out of pairing claims.
    fn pairing_retry_after(&self, ip: IpAddr) -> Duration {
        let now = Instant::now();
        self.inner
            .pairing_admission
            .lock()
            .get(&ip)
            .and_then(|tracker| tracker.locked_until)
            .map_or(Duration::ZERO, |until| until.saturating_duration_since(now))
    }

    /// Reserve the relay's HTTP half before notifying the daemon, so even an
    /// immediate data connection hands its socket directly to this request.
    async fn open_session_channel(
        &self,
        machine: &str,
        expected_generation: Option<u64>,
    ) -> Result<(WebSocket, SessionGuard), StatusCode> {
        self.open_session_channel_at(machine, expected_generation)
            .await
            .map(|(socket, guard, _)| (socket, guard))
    }

    /// Like [`Self::open_session_channel`], also returning the control generation
    /// the channel was issued under so it can be pooled against that fence.
    async fn open_session_channel_at(
        &self,
        machine: &str,
        expected_generation: Option<u64>,
    ) -> Result<(WebSocket, SessionGuard, u64), StatusCode> {
        // The relay's own proxied channel is plaintext by construction: this is the legacy
        // `/host/...` path, where the relay itself speaks HTTP to the machine. Encrypted
        // sessions are only issued to clients through `attach_session_handler` (`?e2e=1`),
        // because the relay holds no attach key and cannot run the Noise handshake itself.
        let session_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        let generation = self.inner.next_generation.fetch_add(1, Ordering::Relaxed);
        let guard = SessionGuard {
            armed: true,
            state: self.clone(),
            session_id: session_id.clone(),
            generation,
        };
        let control_generation;
        {
            let channels = self.inner.control_channels.lock();
            let channel = channels.get(machine).ok_or(StatusCode::NOT_FOUND)?;
            // A claim authorized against one control generation must not be dispatched
            // to a replacement channel that never held that authorization.
            if expected_generation.is_some_and(|expected| expected != channel.generation) {
                return Err(StatusCode::GONE);
            }
            control_generation = channel.generation;
            let mut sessions = self.inner.pending_sessions.lock();
            if pending_budget_exhausted(&sessions, machine) {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
            sessions.insert(
                session_id.clone(),
                WaitingHalf {
                    generation,
                    owner: machine.to_owned(),
                    control_generation: channel.generation,
                    created_at: Instant::now(),
                    kind: Some(HalfKind::Client),
                    notify: Some(tx),
                    active: false,
                    opaque: false,
                    session_kind: SessionKind::Http,
                    suspension: new_suspension_signal(),
                },
            );
            channel
                .tx
                .try_send(IncomingSessionNotice {
                    session_id,
                    opaque: false,
                })
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        }
        let socket = match timeout(SESSION_PAIRING_TIMEOUT, rx).await {
            Ok(Ok(socket)) => socket,
            // The sweeper retires an unpaired entry at the same deadline this wait uses, so a
            // dropped sender is the same pairing timeout, reported with its stage.
            Err(_) | Ok(Err(_)) => return Err(StatusCode::GATEWAY_TIMEOUT),
        };
        Ok((socket, guard, control_generation))
    }
}

const SOCKET_TICKET_TTL: u64 = 30;
const MAX_ACTIVE_PAIRING_PINS_PER_MACHINE: usize = 5;
/// Upper bound on how long a registering daemon may keep a 6-digit pairing code live.
/// A short numeric code is only safe as a brief lease, so the relay caps the lifetime
/// rather than trusting the client-supplied `expires_at`.
const MAX_PAIRING_LEASE: u64 = 600;

/// The bearer a device presents, trimmed. Every device-token route parses it the same way,
/// so a token that works over HTTP also works for a socket ticket.
fn device_bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

async fn socket_ticket_handler(
    State(state): State<RelayState>,
    AxumPath(machine): AxumPath<String>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(mut body): Json<serde_json::Value>,
) -> Result<Response, StatusCode> {
    let ip = peer_ip(peer);
    if let Err(refusal) = plaintext_gate(&state, &headers, &machine, "/api/v1/socket-ticket") {
        return Ok(refusal);
    }
    let device_token = device_bearer(&headers)
        .ok_or(StatusCode::UNAUTHORIZED)?
        .to_owned();
    // The public route supplies machineId; the shared wire type also supports
    // callers that include it explicitly, but must not permit a conflicting ID.
    let object = body.as_object_mut().ok_or(StatusCode::BAD_REQUEST)?;
    object
        .entry("machineId")
        .or_insert_with(|| machine.clone().into());
    let request: SocketTicketRequest =
        serde_json::from_value(body).map_err(|_| StatusCode::BAD_REQUEST)?;
    if request.machine_id != machine || !valid_socket_target(&request.target) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if state
        .inner
        .control_channels
        .lock()
        .get(&machine)
        .is_none_or(|c| c.tx.is_closed())
    {
        return Err(StatusCode::NOT_FOUND);
    }
    // A locked-out address is refused before its bearer is probed against the host.
    if state.locked_out(ip) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    match state.authorize_device_token(&machine, &device_token).await {
        Ok(true) => {}
        Ok(false) => {
            state.record_auth(ip, false);
            return Ok((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "Device token not authorized for this machine"
                })),
            )
                .into_response());
        }
        Err(status) => return Err(status),
    }
    let now = current_time_secs();
    let mut tickets = state.inner.pending_socket_tickets.lock();
    tickets.retain(|_, (_, _, _, expiry)| *expiry > now);
    if tickets.len() >= MAX_PENDING_SESSIONS {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let ticket = uuid::Uuid::new_v4().to_string();
    let expires_at = now + SOCKET_TICKET_TTL;
    tickets.insert(
        ticket.clone(),
        (machine, request.target, device_token, expires_at),
    );
    Ok(Json(SocketTicketResponse { ticket, expires_at }).into_response())
}

fn valid_socket_target(target: &str) -> bool {
    target == "/api/v1/events"
        || target.strip_prefix("/api/v1/terminal/").is_some_and(|id| {
            !id.is_empty()
                && id.bytes().all(|b| {
                    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':')
                })
        })
        || target.strip_prefix("/api/v1/browser/").is_some_and(|id| {
            !id.is_empty()
                && id.bytes().all(|b| {
                    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':')
                })
        })
        || target == crate::remote::dag_api::DAG_SOCKET_TARGET
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SocketQuery {
    ticket: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DagSocketQuery {
    ticket: String,
    workspace_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalSocketQuery {
    pub ticket: String,
    pub render: Option<String>,
    #[serde(default, deserialize_with = "canonical_socket_dimension")]
    pub cols: Option<u16>,
    #[serde(default, deserialize_with = "canonical_socket_dimension")]
    pub rows: Option<u16>,
    #[serde(rename = "daemonEpoch")]
    pub daemon_epoch: Option<crate::scoped_contracts::Epoch>,
    #[serde(rename = "afterSequence")]
    pub after_sequence: Option<crate::scoped_contracts::Epoch>,
}

fn canonical_socket_dimension<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u16>, D::Error> {
    let value = String::deserialize(deserializer)?;
    let number: u16 = value.parse().map_err(serde::de::Error::custom)?;
    if number.to_string() != value {
        return Err(serde::de::Error::custom("noncanonical socket dimension"));
    }
    Ok(Some(number))
}

/// Status for a socket query the handler could not parse. A missing ticket or a key that
/// carries a credential is an authorization failure (401); anything else is a malformed
/// request (400), so a client bug in a resize parameter never reads as a revoked device.
fn socket_query_rejection(raw: Option<&str>) -> StatusCode {
    let mut ticket = false;
    for (key, _) in url::form_urlencoded::parse(raw.unwrap_or_default().as_bytes()) {
        let key = key.to_ascii_lowercase();
        if matches!(key.as_str(), "token" | "access_token" | "authorization") {
            return StatusCode::UNAUTHORIZED;
        }
        ticket |= key == "ticket";
    }
    if ticket {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::UNAUTHORIZED
    }
}

fn consume_socket_ticket(
    state: &RelayState,
    query: Result<Query<SocketQuery>, axum::extract::rejection::QueryRejection>,
    raw_query: Option<&str>,
    machine: &str,
    target: &str,
) -> Result<String, StatusCode> {
    // Unknown query keys (including token/access_token/authorization) are
    // forbidden, even when accompanied by a valid one-time ticket.
    let Query(query) = query.map_err(|_| socket_query_rejection(raw_query))?;
    let ticket = query.ticket.ok_or(StatusCode::UNAUTHORIZED)?;
    let (issued_machine, issued_target, device_token, expiry) = state
        .inner
        .pending_socket_tickets
        .lock()
        .remove(&ticket)
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if issued_machine != machine || issued_target != target || expiry <= current_time_secs() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(device_token)
}

async fn browser_events_handler(
    State(state): State<RelayState>,
    AxumPath(machine): AxumPath<String>,
    query: Result<Query<SocketQuery>, axum::extract::rejection::QueryRejection>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    let target = "/api/v1/events";
    if let Err(refusal) = plaintext_gate(&state, &headers, &machine, target) {
        return Ok(refusal);
    }
    let device_token = consume_socket_ticket(&state, query, raw_query.as_deref(), &machine, target)?;
    browser_socket(state, machine, target.into(), device_token, ws).await
}

async fn browser_screencast_handler(
    State(state): State<RelayState>,
    AxumPath((machine, browser_id)): AxumPath<(String, String)>,
    query: Result<Query<SocketQuery>, axum::extract::rejection::QueryRejection>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    if let Err(refusal) = plaintext_gate(&state, &headers, &machine, "/api/v1/browser") {
        return Ok(refusal);
    }
    let target = format!("/api/v1/browser/{browser_id}");
    if !valid_socket_target(&target) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let device_token =
        consume_socket_ticket(&state, query, raw_query.as_deref(), &machine, &target)?;
    browser_socket(state, machine, target, device_token, ws).await
}

/// Forwards one authenticated DAG stream to the host. The tunnel bridge is the
/// same one terminal and event sockets already use; only the target differs.
async fn dag_stream_handler(
    State(state): State<RelayState>,
    AxumPath(machine): AxumPath<String>,
    query: Result<Query<DagSocketQuery>, axum::extract::rejection::QueryRejection>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    if let Err(refusal) = plaintext_gate(
        &state,
        &headers,
        &machine,
        crate::remote::dag_api::DAG_SOCKET_TARGET,
    ) {
        return Ok(refusal);
    }
    let Query(query) = query.map_err(|_| socket_query_rejection(raw_query.as_deref()))?;
    if query.workspace_id.is_empty()
        || query.workspace_id.len() > 256
        || !query
            .workspace_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':'))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let base = crate::remote::dag_api::DAG_SOCKET_TARGET;
    let device_token = consume_socket_ticket(
        &state,
        Ok(Query(SocketQuery {
            ticket: Some(query.ticket),
        })),
        None,
        &machine,
        base,
    )?;
    let target = format!("{base}?workspaceId={}", query.workspace_id);
    browser_socket(state, machine, target, device_token, ws).await
}

async fn browser_terminal_handler(
    State(state): State<RelayState>,
    AxumPath((machine, terminal)): AxumPath<(String, String)>,
    query: Result<Query<TerminalSocketQuery>, axum::extract::rejection::QueryRejection>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    if let Err(refusal) = plaintext_gate(&state, &headers, &machine, "/api/v1/terminal") {
        return Ok(refusal);
    }
    let Query(query) = query.map_err(|_| socket_query_rejection(raw_query.as_deref()))?;
    let target = format!("/api/v1/terminal/{terminal}");
    if !valid_socket_target(&target)
        || query
            .render
            .as_ref()
            .is_some_and(|value| value.chars().any(char::is_control))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let device_token = consume_socket_ticket(
        &state,
        Ok(Query(SocketQuery {
            ticket: Some(query.ticket),
        })),
        None,
        &machine,
        &target,
    )?;
    let mut uri = reqwest::Url::parse(&format!("ws://localhost{target}"))
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if query.render.is_some()
        || query.cols.is_some()
        || query.rows.is_some()
        || query.daemon_epoch.is_some()
        || query.after_sequence.is_some()
    {
        let mut params = uri.query_pairs_mut();
        if let Some(render) = query.render {
            params.append_pair("render", &render);
        }
        if let Some(cols) = query.cols {
            params.append_pair("cols", &cols.to_string());
        }
        if let Some(rows) = query.rows {
            params.append_pair("rows", &rows.to_string());
        }
        if let Some(epoch) = query.daemon_epoch {
            params.append_pair("daemonEpoch", &epoch.0.to_string());
        }
        if let Some(sequence) = query.after_sequence {
            params.append_pair("afterSequence", &sequence.0.to_string());
        }
    }
    let target = match uri.query() {
        Some(query) => format!("{}?{query}", uri.path()),
        None => uri.path().to_owned(),
    };
    browser_socket(state, machine, target, device_token, ws).await
}

const BROWSER_MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;
/// Read buffer for one browser bridge's tunnel side.
const BRIDGE_READ_BUFFER: usize = 64 * 1024;

async fn browser_socket(
    state: RelayState,
    machine: String,
    target: String,
    device_token: String,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    let is_browser = target.starts_with("/api/v1/browser/");
    let max_size = if is_browser {
        BROWSER_MAX_MESSAGE_SIZE
    } else {
        MAX_MESSAGE_SIZE
    };
    let (data, guard) = state.open_session_channel(&machine, None).await?;
    let session_id = guard.session_id.clone();
    let mut suspension = state.session_suspension_receiver(&session_id);
    Ok(ws
        .max_message_size(max_size)
        .max_frame_size(max_size)
        .on_upgrade(move |browser| async move {
            // The bridge ends itself at the deadline with a close frame; the outer bound only
            // backstops that best-effort close.
            let deadline = tokio::time::Instant::now() + SESSION_TRANSFER_TIMEOUT;
            match timeout(
                SESSION_TRANSFER_TIMEOUT + TRANSFER_TIMEOUT,
                bridge_browser_socket(browser, data, &target, &device_token, &mut suspension, deadline),
            )
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(%error, "browser reverse WebSocket failed"),
                Err(error) => tracing::warn!(%error, "browser reverse WebSocket lifetime exceeded"),
            }
            drop(guard);
        }))
}

/// The existing daemon unwraps tunnel Binary messages onto a TCP stream.
/// Use a bounded duplex stream to run a real WebSocket client over that stream,
/// including its HTTP upgrade, masking, fragmentation and control frames.
async fn bridge_browser_socket(
    mut browser: WebSocket,
    mut data: WebSocket,
    target: &str,
    device_token: &str,
    suspension: &mut watch::Receiver<Option<String>>,
    deadline: tokio::time::Instant,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_tungstenite::tungstenite::Message as Wire;
    let is_browser = target.starts_with("/api/v1/browser/");
    let buffer_size = if is_browser {
        BROWSER_MAX_MESSAGE_SIZE
    } else {
        MAX_MESSAGE_SIZE
    };
    let (client_io, tunnel_io) = tokio::io::duplex(buffer_size);
    let (mut reader, mut writer) = tokio::io::split(tunnel_io);
    let tunnel = async {
        let result = async {
        // One read never needs to hold a whole message: tungstenite reassembles frames, so a
        // small read buffer bounds per-socket memory without limiting message size.
        let mut buffer = bytes::BytesMut::with_capacity(BRIDGE_READ_BUFFER);
        loop {
            tokio::select! {
                count = reader.read_buf(&mut buffer) => {
                    let count = count?;
                    if count == 0 {
                        timeout(TRANSFER_TIMEOUT, data.send(Message::Close(None))).await??;
                        return Ok::<(), anyhow::Error>(());
                    }
                    let chunk = buffer.split().freeze();
                    buffer.reserve(BRIDGE_READ_BUFFER);
                    timeout(TRANSFER_TIMEOUT, data.send(Message::Binary(chunk))).await??;
                }
                frame = data.recv() => match frame {
                    Some(Ok(Message::Binary(bytes))) => timeout(TRANSFER_TIMEOUT, writer.write_all(&bytes)).await??,
                    Some(Ok(Message::Text(text))) => timeout(TRANSFER_TIMEOUT, writer.write_all(text.as_bytes())).await??,
                    Some(Ok(Message::Close(_))) | None => { writer.shutdown().await?; return Ok(()); },
                    Some(Err(error)) => return Err(error.into()),
                    Some(Ok(_)) => {},
                }
            }
        }
        }
        .await;
        if result.is_err() {
            // End the inner stream so the application side sees the host go away and closes
            // the browser with a reason instead of waiting out its own timeout.
            let _ = timeout(TRANSFER_TIMEOUT, writer.shutdown()).await;
        }
        result
    };
    let application = async {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = format!("ws://localhost{target}").into_client_request()?;
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {device_token}").parse()?);
        let (mut daemon, _) = match timeout(
            TRANSFER_TIMEOUT,
            tokio_tungstenite::client_async(request, client_io),
        )
        .await
        {
            Ok(Ok(connected)) => connected,
            Ok(Err(error)) => {
                close_browser(&mut browser, axum::extract::ws::close_code::ERROR, "host disconnected").await;
                return Err(anyhow::Error::from(error));
            }
            Err(error) => {
                close_browser(&mut browser, axum::extract::ws::close_code::ERROR, "host disconnected").await;
                return Err(anyhow::Error::from(error));
            }
        };
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    close_browser(&mut browser, axum::extract::ws::close_code::AWAY, "session lifetime exceeded").await;
                    return Ok(());
                }
                reason = next_suspension_reason(suspension) => {
                    // A stopped owner's browser stream ends with the exact reason rather than
                    // being cut off by the tunnel dropping.
                    let frame = axum::extract::ws::CloseFrame {
                        code: axum::extract::ws::close_code::POLICY,
                        reason: reason.into(),
                    };
                    timeout(TRANSFER_TIMEOUT, browser.send(Message::Close(Some(frame)))).await??;
                    return Ok(());
                }
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
                    if close {
                        while let Some(frame) = timeout(TRANSFER_TIMEOUT, daemon.next()).await? {
                            if let Wire::Close(_) = frame? {
                                // Receiving Close queued tungstenite's acknowledgement.
                                timeout(TRANSFER_TIMEOUT, browser.flush()).await??;
                                return Ok(());
                            }
                        }
                        anyhow::bail!("daemon closed without terminal close acknowledgement");
                    }
                }
                frame = daemon.next() => {
                    let frame = match frame {
                        Some(Ok(frame)) => frame,
                        None => {
                            close_browser(&mut browser, axum::extract::ws::close_code::ERROR, "host disconnected").await;
                            return Ok(());
                        }
                        Some(Err(error)) => {
                            close_browser(&mut browser, axum::extract::ws::close_code::ERROR, "host disconnected").await;
                            return Err(anyhow::Error::from(error));
                        }
                    };
                    let frame = match frame {
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
    tokio::pin!(application);
    tokio::select! {
        result = &mut tunnel => match result {
            Ok(()) => timeout(TRANSFER_TIMEOUT, &mut application).await?,
            Err(error) => {
                // Let the application side deliver its "host disconnected" close first.
                let _ = timeout(TRANSFER_TIMEOUT, &mut application).await;
                Err(error)
            }
        },
        result = &mut application => {
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachSessionRequest {
    pub machine_id: String,
    /// Absent from older clients, which get the default lifetime.
    #[serde(default)]
    pub session_kind: SessionKind,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachSessionResponse {
    pub session_id: String,
    pub machine_id: String,
    pub opaque: bool,
    /// Echoes the honoured kind, so a client can tell this relay from one that predates kinds
    /// (which omits the field and cuts every session at the default lifetime).
    #[serde(default)]
    pub session_kind: SessionKind,
}

/// Reads the allocation query: absent or empty means the account-only allocation, `e2e=1`
/// (exactly once) admits a device token too. Anything else is `None` and answers 400.
fn attach_e2e_query(query: Option<&str>) -> Option<bool> {
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return Some(false);
    };
    let mut e2e = false;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if key != "e2e" || value != "1" || e2e {
            return None;
        }
        e2e = true;
    }
    Some(e2e)
}

/// The billing gate both attach authorizations share: an owner whose remote access is
/// suspended gets 402 `REMOTE_SUSPENDED` with the entitlement details clients branch on.
fn remote_entitlement_gate(
    account: &crate::account::service::AccountState,
    owner_user_id: &str,
) -> Result<(), crate::account::service::ApiError> {
    let now = crate::account::billing::routes::billing_now();
    let entitlement =
        crate::account::billing::routes::entitlement_for_user(account, owner_user_id, now)?;
    if entitlement.remote_allowed {
        return Ok(());
    }
    Err(crate::account::service::ApiError::new(
        StatusCode::PAYMENT_REQUIRED,
        "REMOTE_SUSPENDED",
        "remote access is suspended for this account",
    )
    .with_details(serde_json::json!({
        "plan": match entitlement.effective_plan {
            crate::account::billing::entitlement::PlanKey::Free => "free",
            crate::account::billing::entitlement::PlanKey::ProMonthly => "pro_monthly",
            crate::account::billing::entitlement::PlanKey::ProAnnual => "pro_annual",
            crate::account::billing::entitlement::PlanKey::TeamMonthly => "team_monthly",
            crate::account::billing::entitlement::PlanKey::TeamAnnual => "team_annual",
        },
        "status": match entitlement.status {
            crate::account::billing::entitlement::EntitlementStatus::Ok => "ok",
            crate::account::billing::entitlement::EntitlementStatus::OverLimit => "over_limit",
            crate::account::billing::entitlement::EntitlementStatus::PastDue => "past_due",
            crate::account::billing::entitlement::EntitlementStatus::Stopped => "stopped",
        },
        "graceEndsAt": entitlement.grace_ends_at,
        "stoppedAt": crate::account::service::owner_stopped_at(account, owner_user_id),
    })))
}

async fn attach_session_handler(
    State(state): State<RelayState>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Response {
    // Every answer to an encrypted allocation request says this relay speaks e2e, so a client
    // can tell this relay's 401 (a revoked token) from an older relay that lacks the route.
    let e2e_requested = url::form_urlencoded::parse(query.as_deref().unwrap_or_default().as_bytes())
        .any(|(key, value)| key == "e2e" && value == "1");
    let mut response = attach_session(state, peer, headers, query, body).await;
    if e2e_requested {
        response
            .headers_mut()
            .insert(RELAY_E2E_HEADER, HeaderValue::from_static("1"));
    }
    response
}

async fn attach_session(
    state: RelayState,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    query: Option<String>,
    body: Bytes,
) -> Response {
    let ip = peer_ip(peer);
    if !state.admit(ip, Instant::now()) {
        return crate::account::service::ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMITED",
            "admission limit exceeded",
        )
        .into_response();
    }

    let Some(e2e) = attach_e2e_query(query.as_deref()) else {
        return crate::account::service::ApiError::new(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "the only admissible query is e2e=1",
        )
        .into_response();
    };

    let account_state = state.account_state();
    if account_state.is_none() && !e2e {
        return crate::account::service::ApiError::new(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "relay is not an account origin",
        )
        .into_response();
    }

    let request: AttachSessionRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(error) => {
            return crate::account::service::ApiError::new(
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                error.to_string(),
            )
            .into_response();
        }
    };

    // An account session bearer is resolved first, exactly as before. Only an `?e2e=1`
    // caller whose bearer is not an account session falls through to the device-token check.
    let authorized_by_account = match account_state.clone() {
        None => false,
        Some(account_state_clone) => {
            let auth_headers = headers.clone();
            let auth_machine_id = request.machine_id.clone();
            let auth_res: Result<bool, crate::account::service::ApiError> =
                crate::ipc::run_blocking(move || {
                    Ok((|| -> Result<bool, crate::account::service::ApiError> {
                        let user = match crate::account::service::authenticate_user(
                            &account_state_clone,
                            &auth_headers,
                        ) {
                            Ok(user) => user,
                            Err(_) if e2e => return Ok(false),
                            Err(error) => return Err(error),
                        };
                        let machine = crate::account::service::machine_for_owner(
                            &account_state_clone,
                            &user.user_id,
                            &auth_machine_id,
                        );
                        if machine.is_none() {
                            return Err(crate::account::service::ApiError::new(
                                StatusCode::NOT_FOUND,
                                "MACHINE_NOT_FOUND",
                                "no such machine on this account",
                            ));
                        }
                        if account_state_clone.deployment_mode.is_billing_enabled() {
                            remote_entitlement_gate(&account_state_clone, &user.user_id)?;
                        }
                        Ok(true)
                    })())
                })
                .await
                .map_err(|error| crate::account::service::ApiError::internal(error.to_string()))
                .and_then(|res| res);
            match auth_res {
                Ok(authorized) => authorized,
                Err(error) => return error.into_response(),
            }
        }
    };

    if !authorized_by_account {
        // Reached only with `?e2e=1`: the bearer must be a device token this machine accepts.
        let unauthorized = || {
            crate::account::service::ApiError::new(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "bearer is neither an account session nor a device token for this machine",
            )
            .into_response()
        };
        let Some(token) = device_bearer(&headers).map(str::to_owned) else {
            return unauthorized();
        };
        match state.authorize_device_token(&request.machine_id, &token).await {
            Ok(true) => {}
            Ok(false) => return unauthorized(),
            Err(status) => return device_token_unconfirmed(status),
        }
        if let Some(account) = account_state
            .clone()
            .filter(|account| account.deployment_mode.is_billing_enabled())
        {
            // The same owner entitlement gate host_http_handler applies to device tokens.
            let machine_id = request.machine_id.clone();
            let gate: Result<(), crate::account::service::ApiError> =
                crate::ipc::run_blocking(move || {
                    Ok(
                        match crate::account::service::enrolled_machine_by_id(&account, &machine_id)
                        {
                            Some(record) => {
                                remote_entitlement_gate(&account, &record.owner_user_id)
                            }
                            None => Ok(()),
                        },
                    )
                })
                .await
                .map_err(|error| crate::account::service::ApiError::internal(error.to_string()))
                .and_then(|res| res);
            if let Err(error) = gate {
                return error.into_response();
            }
        }
    }

    let channel = {
        let channels = state.inner.control_channels.lock();
        channels.get(&request.machine_id).cloned()
    };
    let Some(channel) = channel else {
        return crate::account::service::ApiError::new(
            StatusCode::CONFLICT,
            "MACHINE_OFFLINE",
            "machine is offline",
        )
        .into_response();
    };
    if channel.tx.is_closed() {
        return crate::account::service::ApiError::new(
            StatusCode::CONFLICT,
            "MACHINE_OFFLINE",
            "machine is offline",
        )
        .into_response();
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    match state.try_issue_session_of_kind(
        &request.machine_id,
        &session_id,
        Some(channel.generation),
        true,
        request.session_kind,
    ) {
        Ok(()) => {}
        Err(IssueRejection::Busy) => {
            return crate::account::service::ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "RELAY_BUSY",
                "the relay has too many sessions waiting for this machine; retry shortly",
            )
            .into_response();
        }
        Err(IssueRejection::Offline | IssueRejection::StaleGeneration) => {
            return crate::account::service::ApiError::new(
                StatusCode::CONFLICT,
                "MACHINE_OFFLINE",
                "machine is offline",
            )
            .into_response();
        }
    }

    Json(AttachSessionResponse {
        session_id,
        machine_id: request.machine_id,
        opaque: true,
        session_kind: request.session_kind,
    })
    .into_response()
}

/// Answers a device-token check that never reached a verdict from the host. The token was not
/// refused, so this must never be a 401: clients treat 401 as a revoked credential.
fn device_token_unconfirmed(status: StatusCode) -> Response {
    // The token could not be checked because no control channel reaches the machine.
    if status == StatusCode::NOT_FOUND || status == StatusCode::GONE {
        return crate::account::service::ApiError::new(
            StatusCode::CONFLICT,
            "MACHINE_OFFLINE",
            "machine is offline",
        )
        .into_response();
    }
    crate::account::service::ApiError::new(
        StatusCode::BAD_GATEWAY,
        "HOST_UNAVAILABLE",
        "the machine could not confirm this device token",
    )
    .into_response()
}

async fn host_http_handler(
    State(state): State<RelayState>,
    AxumPath((machine, path)): AxumPath<(String, String)>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    request: Request<Body>,
) -> Result<Response, StatusCode> {
    if !allowed_http_route(request.method(), &path) {
        return Err(StatusCode::FORBIDDEN);
    }
    // A refused query answers with the frozen machine envelope every other refusal on this
    // surface uses, so the caller sees a typed INVALID_REQUEST instead of the bodyless 400 a
    // bare `StatusCode` becomes. Only the reference-chat routes took that shape in this batch;
    // every other route keeps the parent's bodyless 400, which is the shape the machine's own
    // handlers still answer on those paths.
    if let Err(status) = validate_http_query(&path, request.uri().query()) {
        if reference_chat_http_path(&path) {
            return Ok(crate::remote::server::machine_error_with_details(
                status,
                "INVALID_REQUEST",
                "the forwarded query is not admissible on this route",
                serde_json::Map::new(),
            ));
        }
        return Err(status);
    }
    if path == "pair/exchange" && request.method() == Method::POST {
        return exchange_http(state, peer_ip(peer), Some(&machine), request).await;
    }
    if let Err(refusal) = plaintext_gate(
        &state,
        request.headers(),
        &machine,
        &format!("/api/v1/{path}"),
    ) {
        return Ok(refusal);
    }
    let (parts, body) = request.into_parts();
    if let Some(account_state) = state.account_state() {
        // An account session bearer is resolved against the account store first and is never
        // used to probe the target host: probing with it would hand one account's session to
        // another account's machine. Only a bearer that is not an account session is treated
        // as a candidate device token and validated against the host.
        let auth_headers = parts.headers.clone();
        let auth_machine = machine.clone();
        let auth_state = account_state.clone();
        let session_gate: Result<Result<Option<String>, crate::account::service::ApiError>, StatusCode> =
            crate::ipc::run_blocking(move || {
                Ok((|| -> Result<Option<String>, crate::account::service::ApiError> {
                    let Ok(user) =
                        crate::account::service::authenticate_user(&auth_state, &auth_headers)
                    else {
                        return Ok(None);
                    };
                    if crate::account::service::machine_for_owner(
                        &auth_state,
                        &user.user_id,
                        &auth_machine,
                    )
                    .is_none()
                    {
                        return Err(crate::account::service::ApiError::new(
                            StatusCode::NOT_FOUND,
                            "MACHINE_NOT_FOUND",
                            "no such machine on this account",
                        ));
                    }
                    Ok(Some(user.user_id))
                })())
            })
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
        let session_owner = match session_gate {
            Ok(Ok(owner)) => owner,
            Ok(Err(error)) => return Ok(error.into_response()),
            Err(status) => return Err(status),
        };

        let billing_owner = if let Some(owner) = session_owner {
            Some(owner)
        } else {
            let bearer_token = device_bearer(&parts.headers);
            let ip = peer_ip(peer);
            if bearer_token.is_some() && state.locked_out(ip) {
                return Err(StatusCode::TOO_MANY_REQUESTS);
            }
            let device_authorized = match bearer_token {
                Some(token) => match state.authorize_device_token(&machine, token).await {
                    Ok(true) => true,
                    Ok(false) => {
                        state.record_auth(ip, false);
                        false
                    }
                    Err(status) => return Ok(device_token_unconfirmed(status)),
                },
                None => false,
            };
            if !device_authorized {
                // Neither an account session nor a device token the host accepts: answer with
                // the account service's own typed refusal for the bearer.
                let refusal_headers = parts.headers.clone();
                let refusal_state = account_state.clone();
                let refusal: Result<crate::account::service::ApiError, StatusCode> =
                    crate::ipc::run_blocking(move || {
                        Ok(crate::account::service::authenticate_user(
                            &refusal_state,
                            &refusal_headers,
                        )
                        .err()
                        .unwrap_or_else(|| {
                            crate::account::service::ApiError::new(
                                StatusCode::UNAUTHORIZED,
                                "UNAUTHORIZED",
                                "unknown or expired session",
                            )
                        }))
                    })
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
                return match refusal {
                    Ok(error) => Ok(error.into_response()),
                    Err(status) => Err(status),
                };
            }
            if account_state.deployment_mode.is_billing_enabled() {
                let owner_state = account_state.clone();
                let owner_machine = machine.clone();
                crate::ipc::run_blocking(move || {
                    Ok(crate::account::service::enrolled_machine_by_id(
                        &owner_state,
                        &owner_machine,
                    )
                    .map(|record| record.owner_user_id))
                })
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            } else {
                None
            }
        };

        if let (Some(owner_user_id), true) = (
            billing_owner,
            account_state.deployment_mode.is_billing_enabled(),
        ) {
            let billing_state = account_state.clone();
            let billing_res: Result<Result<(), crate::account::service::ApiError>, StatusCode> =
                crate::ipc::run_blocking(move || {
                    Ok((|| -> Result<(), crate::account::service::ApiError> {
                        let now = crate::account::billing::routes::billing_now();
                        let entitlement = crate::account::billing::routes::entitlement_for_user(
                            &billing_state,
                            &owner_user_id,
                            now,
                        )?;
                        if !entitlement.remote_allowed {
                            let stopped_at = crate::account::service::owner_stopped_at(
                                &billing_state,
                                &owner_user_id,
                            );
                            return Err(crate::account::service::ApiError::new(
                                StatusCode::PAYMENT_REQUIRED,
                                "REMOTE_SUSPENDED",
                                "remote access is suspended for this account",
                            )
                            .with_details(serde_json::json!({
                                "plan": match entitlement.effective_plan {
                                    crate::account::billing::entitlement::PlanKey::Free => "free",
                                    crate::account::billing::entitlement::PlanKey::ProMonthly => "pro_monthly",
                                    crate::account::billing::entitlement::PlanKey::ProAnnual => "pro_annual",
                                    crate::account::billing::entitlement::PlanKey::TeamMonthly => "team_monthly",
                                    crate::account::billing::entitlement::PlanKey::TeamAnnual => "team_annual",
                                },
                                "status": match entitlement.status {
                                    crate::account::billing::entitlement::EntitlementStatus::Ok => "ok",
                                    crate::account::billing::entitlement::EntitlementStatus::OverLimit => "over_limit",
                                    crate::account::billing::entitlement::EntitlementStatus::PastDue => "past_due",
                                    crate::account::billing::entitlement::EntitlementStatus::Stopped => "stopped",
                                },
                                "graceEndsAt": entitlement.grace_ends_at,
                                "stoppedAt": stopped_at,
                            })));
                        }
                        Ok(())
                    })())
                })
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
            match billing_res {
                Ok(Err(error)) => return Ok(error.into_response()),
                Err(status) => return Err(status),
                Ok(Ok(())) => {}
            }
        }
    }
    let uri_path = parts
        .uri
        .path()
        .strip_prefix(&format!("/host/{machine}"))
        .ok_or(StatusCode::BAD_REQUEST)?;
    let target = match parts.uri.query() {
        Some(query) => format!("{uri_path}?{query}"),
        None => uri_path.to_owned(),
    };
    // The generic forwarded-body bound is 64 KiB. The reference-chat file lane carries a
    // base64 image, so its paths get the bound derived from the frozen attachment limit —
    // and only those paths, which allowed_http_route has already admitted above.
    let body_limit = if reference_chat_http_path(&path) {
        REFERENCE_CHAT_RELAY_BODY_MAX
    } else {
        64 * 1024
    };
    let body = to_bytes(body, body_limit)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    // Only the relay itself may name a pairing client to the gateway.
    let mut headers = parts.headers;
    headers.remove(crate::remote::server::PAIRING_PEER_HEADER);
    proxy_http(
        &state,
        &machine,
        None,
        parts.method,
        &target,
        headers,
        body.to_vec(),
    )
    .await
}

const MAX_HTTP_SIZE: usize = 8 * 1024 * 1024;
/// Most informational (1xx) responses one forwarded request may receive before its final one.
const MAX_INFORMATIONAL_RESPONSES: usize = 8;
/// How long a request on a pooled (idle) data channel waits for its first response byte before
/// the channel is presumed dead and the request is replayed on a fresh one.
const POOLED_FIRST_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// How large a forwarded response one reference-chat route may return.
///
/// A staged-file preview is JSON carrying base64 of up to ATTACHMENT_MAX_FILE_BYTES, which the
/// generic 8 MiB response bound refuses with 502. The bound is derived from that frozen limit,
/// rounded up to whole base64 quads, plus headroom for the JSON envelope and response headers,
/// and applies only to the reference-chat paths the allowlist admits.
const REFERENCE_CHAT_RELAY_RESPONSE_MAX: usize =
    (crate::scoped_contracts::ATTACHMENT_MAX_FILE_BYTES as usize).div_ceil(3) * 4 + 64 * 1024;

// This literal HTTP endpoint must outrank /terminal/{terminal_id}. Reuse the
// same method/query/body checks; no other terminal path becomes an HTTP proxy.
async fn terminal_preferences_handler(
    state: State<RelayState>,
    AxumPath(machine): AxumPath<String>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    request: Request<Body>,
) -> Result<Response, StatusCode> {
    host_http_handler(
        state,
        AxumPath((machine, "terminal/preferences".into())),
        peer,
        request,
    )
    .await
}

// These literal HTTP endpoints must outrank /browser/{browser_id} so that
// HTTP discovery requests are forwarded to HTTP endpoints rather than captured
// by the WebSocket upgrade handler.
async fn browser_sessions_http_handler(
    state: State<RelayState>,
    AxumPath(machine): AxumPath<String>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    request: Request<Body>,
) -> Result<Response, StatusCode> {
    host_http_handler(
        state,
        AxumPath((machine, "browser/sessions".into())),
        peer,
        request,
    )
    .await
}

async fn browser_identify_http_handler(
    state: State<RelayState>,
    AxumPath(machine): AxumPath<String>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    request: Request<Body>,
) -> Result<Response, StatusCode> {
    host_http_handler(
        state,
        AxumPath((machine, "browser/identify".into())),
        peer,
        request,
    )
    .await
}

pub(super) fn allowed_http_route(method: &Method, path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    if parts.iter().any(|part| {
        part.is_empty()
            || matches!(*part, "." | "..")
            || !part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.' | b'~'))
    }) {
        return false;
    }
    matches!(
        (method.as_str(), parts.as_slice()),
        ("GET", ["health" | "capabilities" | "sessions" | "resources"])
            | ("POST", ["sessions"])
            | ("GET" | "DELETE", ["sessions", _])
            | ("GET", ["fs", "directories"])
            | ("GET", ["fs", "repositories"])
            | ("GET" | "POST", ["workspace", "projects"])
            | ("DELETE", ["workspace", "projects", _])
            | ("GET", ["workspace", "sidebar"])
            | ("GET" | "POST" | "DELETE", ["workspace", "worktrees"])
            | ("GET", ["workspace", "worktrees", "status"])
            | ("GET", ["workspace", "operations", _])
            | ("GET", ["workspace", "state"])
            | ("GET", ["terminal", "preferences"])
            | ("GET", ["browser", "sessions" | "identify"])
            | ("POST", ["workspace", "select" | "selection"])
            // Paired desktop machine reads/uploads; each paste-upload chunk is 32 KiB raw,
            // which stays under the 64 KiB forwarded-body bound once base64 + JSON wrapped.
            | ("POST", ["workspace", "file-read" | "paste-upload"])
            | ("POST", ["pair", "exchange"])
            | ("POST", ["direct", "offer"])
            | ("GET", ["push", "vapid-public-key"])
            | ("POST", ["push", "subscribe" | "unsubscribe" | "preferences"])
            | ("POST", ["push", "test"])
            | ("GET", ["session", _])
            | ("GET", ["agent-history", _])
            // Reference chat (plan task 13). The relay only admits the frozen route table;
            // the machine's own gateway authenticates, fences the target and enforces the
            // read-only policy, exactly as it does for a direct caller.
            | ("GET", ["reference-chat", _, "history" | "screen" | "prompt" | "files"])
            | ("POST", ["reference-chat", _, "submit" | "stop" | "answer" | "files" | "acknowledge-outcome"])
            | ("GET" | "DELETE", ["reference-chat", _, "files", _])
            // Agent control. Reads and mutations are separate paths so a dropped mutation
            // reply is outcome-unknown and a dropped wait stays a timeout.
            | ("GET", ["agent-control", "sessions" | "events"])
            | ("GET", ["agent-control", "sessions", _])
            | ("GET", ["agent-control", "sessions", _, "wait"])
            | ("POST", ["agent-control", "sessions", _, "send" | "answer" | "stop"])
            // Supervisor view. The lane key rides in the pause body, never in the path.
            | ("GET", ["supervisor"])
            | ("POST", ["supervisor", "pause" | "start"])
            // Unified supervisor API (todo 13). The machine authorizes the device and derives
            // the actor; the relay only admits the route shape.
            | ("POST", ["supervisor", "request"])
            | ("GET", ["supervisor", "conversation"])
            | ("GET" | "POST", ["supervisor", "memory"])
            | ("GET", ["attach"])
    )
}

/// How large a forwarded body one reference-chat route may carry.
///
/// The file lane's stage payload is base64 over the frozen ATTACHMENT_MAX_FILE_BYTES, which the
/// generic 64 KiB forwarded-body bound would refuse before the machine ever saw it. The bound is
/// derived from that frozen limit and applies only to the reference-chat paths the allowlist
/// above already admits.
const REFERENCE_CHAT_RELAY_BODY_MAX: usize =
    (crate::scoped_contracts::ATTACHMENT_MAX_FILE_BYTES as usize / 3) * 4 + 64 * 1024;

/// Is this forwarded path one of the reference-chat routes?
fn reference_chat_http_path(path: &str) -> bool {
    path.starts_with("reference-chat/")
}

fn decode_http_query_component(value: &str) -> Result<String, StatusCode> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(match byte {
            b'+' => b' ',
            b'%' => {
                let hi = bytes
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or(StatusCode::BAD_REQUEST)?;
                let lo = bytes
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or(StatusCode::BAD_REQUEST)?;
                (hi * 16 + lo) as u8
            }
            byte => byte,
        });
    }
    String::from_utf8(decoded).map_err(|_| StatusCode::BAD_REQUEST)
}

pub(super) fn validate_http_query(path: &str, query: Option<&str>) -> Result<(), StatusCode> {
    let Some(query) = query else {
        return Ok(());
    };
    if query.len() > 16 * 1024 {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Validate one decoded view, but forward the original bytes without decoding
    // again: a literal %2F in a filename must not become a separator.
    let bytes = query.as_bytes();
    for (i, byte) in bytes.iter().enumerate() {
        if *byte == b'%'
            && (i + 2 >= bytes.len()
                || !bytes[i + 1].is_ascii_hexdigit()
                || !bytes[i + 2].is_ascii_hexdigit())
        {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    // The fields the host's own `ReferenceChatReadQuery` declares. `history`, `screen`,
    // `prompt` and the staged-file routes all deserialize that one struct, so a field it
    // accepts must not be refused here.
    let reference_chat_read_fields: &[&str] = &[
        "hostId",
        "ownerId",
        "epoch",
        "backendSessionId",
        "providerSessionId",
        "registryId",
        "limit",
        "cursor",
        "cursorStream",
    ];
    let allowed: &[&str] = match path.split('/').collect::<Vec<_>>().as_slice() {
        ["fs", "directories"] => &["path", "includeHidden"],
        ["fs", "repositories"] => &["path", "depth", "includeHidden"],
        ["browser", "sessions"] | ["browser", "identify"] => &["workspaceId", "worktreeSlug"],
        ["workspace", "worktrees"] => &["workspaceId"],
        // The remote sidebar lists desktop-scoped projects (`?scope=desktopLocal`).
        ["workspace", "projects"] => &["scope"],
        ["workspace", "worktrees", "status"] => &["workspaceId", "wsId", "slug"],
        // Session listing is workspace-scoped; a single session is epoch-fenced.
        ["sessions"] => &["workspaceId", "daemonEpoch"],
        ["sessions", _] => &["daemonEpoch"],
        // Chat history pages by limit/cursor. Without this the relay answers 400 and the
        // remote chat view stays empty even though the machine serves the route.
        ["agent-history", _] => &["limit", "cursor"],
        // Reference chat. A read names its whole target in the query; `history`, `screen`,
        // `prompt` and the staged-file routes all deserialize the host's own
        // `ReferenceChatReadQuery`, so they admit exactly its fields. A mutation's target
        // travels in its own envelope body, so its query must stay empty.
        ["reference-chat", _, "history" | "screen" | "prompt" | "acknowledge-outcome"] => reference_chat_read_fields,
        ["reference-chat", _, "files"] => reference_chat_read_fields,
        ["reference-chat", _, "files", _] => reference_chat_read_fields,
        ["agent-control", "events"] => &["after"],
        ["supervisor", "conversation"] => &["projectId", "after"],
        ["agent-control", "sessions", _, "wait"] => &["until", "after", "timeoutMs", "revision"],
        _ => &[],
    };
    let mut seen = std::collections::HashSet::new();
    for field in query.split('&').filter(|field| !field.is_empty()) {
        let (key, value) = field.split_once('=').ok_or(StatusCode::BAD_REQUEST)?;
        let key = decode_http_query_component(key)?;
        let value = decode_http_query_component(value)?;
        if !allowed.contains(&key.as_str())
            || !seen.insert(key.clone())
            || value.chars().any(char::is_control)
            || (key == "includeHidden" && !matches!(value.as_str(), "true" | "false"))
            || (key == "depth" && !matches!(value.as_str(), "1" | "2" | "3" | "4"))
        {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    Ok(())
}

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
    let mut claim = match state.claim_pairing(ip, &payload, machine) {
        Ok(claim) => claim,
        // A locked-out source is told it is rate limited and when to retry, not that its
        // code was wrong.
        Err(StatusCode::TOO_MANY_REQUESTS) => {
            let retry_after = state.pairing_retry_after(ip).as_secs().max(1);
            let mut response = crate::account::service::ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "pairing_rate_limited",
                "too many pairing attempts; wait before trying again",
            )
            .into_response();
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_str(&retry_after.to_string())
                    .unwrap_or_else(|_| HeaderValue::from_static("60")),
            );
            return Ok(response);
        }
        Err(status) => return Err(status),
    };
    // The loopback gateway calls its pairing secret `code`.
    let body = serde_json::to_vec(&serde_json::json!({
        "code": claim.pairing_token,
        "pairingToken": claim.pairing_token,
        "deviceName": payload.device_name,
        "installationId": payload.installation_id,
    }))
    .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());
    // The gateway keys its per-source PIN failure budget on this address.
    headers.insert(
        crate::remote::server::PAIRING_PEER_HEADER,
        HeaderValue::from_str(&ip.to_string()).unwrap(),
    );
    // The owner's entitlement is checked before the host mints anything: a suspended owner
    // must never end up with a paired device on the machine.
    if let Some(account_state) = state.account_state() {
        if account_state.deployment_mode.is_billing_enabled() {
            let machine_id = claim.machine_id.clone();
            let billing_res: Result<Result<(), crate::account::service::ApiError>, StatusCode> =
                crate::ipc::run_blocking(move || {
                    Ok((|| -> Result<(), crate::account::service::ApiError> {
                        if let Some(record) = crate::account::service::enrolled_machine_by_id(
                            &account_state,
                            &machine_id,
                        ) {
                            let now = crate::account::billing::routes::billing_now();
                            let entitlement = crate::account::billing::routes::entitlement_for_user(
                                &account_state,
                                &record.owner_user_id,
                                now,
                            )?;
                            if !entitlement.remote_allowed {
                                let stopped_at = crate::account::service::owner_stopped_at(
                                    &account_state,
                                    &record.owner_user_id,
                                );
                                return Err(crate::account::service::ApiError::new(
                                    StatusCode::PAYMENT_REQUIRED,
                                    "REMOTE_SUSPENDED",
                                    "remote access is suspended for this account",
                                )
                                .with_details(serde_json::json!({
                                    "plan": match entitlement.effective_plan {
                                        crate::account::billing::entitlement::PlanKey::Free => "free",
                                        crate::account::billing::entitlement::PlanKey::ProMonthly => "pro_monthly",
                                        crate::account::billing::entitlement::PlanKey::ProAnnual => "pro_annual",
                                        crate::account::billing::entitlement::PlanKey::TeamMonthly => "team_monthly",
                                        crate::account::billing::entitlement::PlanKey::TeamAnnual => "team_annual",
                                    },
                                    "status": match entitlement.status {
                                        crate::account::billing::entitlement::EntitlementStatus::Ok => "ok",
                                        crate::account::billing::entitlement::EntitlementStatus::OverLimit => "over_limit",
                                        crate::account::billing::entitlement::EntitlementStatus::PastDue => "past_due",
                                        crate::account::billing::entitlement::EntitlementStatus::Stopped => "stopped",
                                    },
                                    "graceEndsAt": entitlement.grace_ends_at,
                                    "stoppedAt": stopped_at,
                                })));
                            }
                        }
                        Ok(())
                    })())
                })
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);

            match billing_res {
                Ok(Err(error)) => {
                    claim.rollback();
                    return Ok(error.into_response());
                }
                Err(status) => {
                    claim.rollback();
                    return Err(status);
                }
                Ok(Ok(())) => {}
            }
        }
    }
    let response = proxy_http(
        &state,
        &claim.machine_id,
        Some(claim.control_generation),
        Method::POST,
        "/api/v1/pair/exchange",
        headers,
        body,
    )
    .await?;
    if response.status().is_success() {
        #[derive(Deserialize)]
        struct IssuedDeviceToken {
            token: String,
        }
        // The host accepted the code and consumed it on its side, so the claim is spent
        // whatever happens to the answer: rolling it back would offer a code the host already
        // refuses.
        claim.commit();
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, MAX_HTTP_SIZE)
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        let issued: IssuedDeviceToken =
            serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_GATEWAY)?;
        if issued.token.is_empty() {
            return Err(StatusCode::BAD_GATEWAY);
        }
        state.register_device_token(&claim.machine_id, &issued.token);
        return Ok(Response::from_parts(parts, Body::from(body)));
    }
    claim.rollback();
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
    expected_generation: Option<u64>,
    method: Method,
    path: &str,
    headers: HeaderMap,
    body: Vec<u8>,
) -> Result<Response, StatusCode> {
    // HTTP/1.1 defaults to keep-alive, so a cleanly framed response leaves the
    // data channel reusable for the next request to the same machine.
    let mut raw = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n", body.len()).into_bytes();
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
    let mutation = outcome_unknown_mutation(&method, path);
    let request_id = if mutation {
        serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|body| body.get("requestId")?.as_str().map(str::to_owned))
            .unwrap_or_default()
    } else {
        String::new()
    };
    // Set once the request is handed to a fresh channel: from then on the host may act on it.
    let forwarded = std::sync::atomic::AtomicBool::new(false);
    // Set once any attached data channel (pooled or fresh) carries the request, so a budget
    // that runs out afterwards is an upstream timeout rather than a pairing timeout.
    let attached = std::sync::atomic::AtomicBool::new(false);
    raw.extend(body);
    let head = method == Method::HEAD;
    let max_response = if path
        .strip_prefix("/api/v1/")
        .is_some_and(reference_chat_http_path)
    {
        REFERENCE_CHAT_RELAY_RESPONSE_MAX
    } else {
        MAX_HTTP_SIZE
    };
    let transfer = async {
        // Only safe methods ride an idle channel: if it turns out to be dead
        // before any response byte arrives, the request is replayed on a fresh
        // channel, which must never repeat a mutation.
        if matches!(method, Method::GET | Method::HEAD) {
            if let Some((socket, control_generation)) =
                state.take_pooled_data(machine, expected_generation)
            {
                attached.store(true, Ordering::SeqCst);
                // An idle channel can die silently (a NAT drops it): bound the wait for its
                // first response byte, then replay this safe request on a fresh channel.
                match http_round_trip(socket, &raw, head, max_response, Some(POOLED_FIRST_RESPONSE_TIMEOUT)).await {
                    Ok((response, reusable)) => {
                        if let Some(socket) = reusable {
                            state.return_pooled_data(machine, control_generation, socket);
                        }
                        return Ok(response);
                    }
                    Err(RoundTripError::Unanswered) => {}
                    Err(RoundTripError::Failed(status)) => return Err(status),
                }
            }
        }
        let (socket, _guard, control_generation) = state
            .open_session_channel_at(machine, expected_generation)
            .await?;
        forwarded.store(true, Ordering::SeqCst);
        attached.store(true, Ordering::SeqCst);
        match http_round_trip(socket, &raw, head, max_response, None).await {
            Ok((response, reusable)) => {
                if let Some(socket) = reusable {
                    state.return_pooled_data(machine, control_generation, socket);
                }
                Ok(response)
            }
            Err(RoundTripError::Unanswered) if mutation => {
                Ok(reference_chat_outcome_unknown(&request_id))
            }
            Err(RoundTripError::Unanswered) => Err(StatusCode::BAD_GATEWAY),
            Err(RoundTripError::Failed(_)) if mutation => {
                Ok(reference_chat_outcome_unknown(&request_id))
            }
            Err(RoundTripError::Failed(status)) => Err(status),
        }
    };
    // Machine Git mutations have a 30-second child deadline. Leave time for
    // admission and the final response without relaxing socket write deadlines.
    // A reference-chat mutation may wait behind earlier input for the host's own
    // REFERENCE_SUBMIT_DEADLINE_MS before it types, so its budget covers that wait.
    let budget = if mutation {
        Duration::from_millis(crate::remote::reference_chat::input::REFERENCE_SUBMIT_DEADLINE_MS)
            + Duration::from_secs(15)
    } else if let Some(wait) = agent_control_wait_budget(path) {
        wait
    } else {
        Duration::from_secs(40)
    };
    match timeout(budget, transfer).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(StatusCode::GATEWAY_TIMEOUT)) => {
            Ok(relay_timeout_response(RELAY_STAGE_DATA_PAIRING_TIMEOUT))
        }
        // The relay's own refusals keep their status but carry the typed machine envelope, so
        // a client can tell "machine offline" from a missing route.
        Ok(Err(status)) => relay_refusal(status).ok_or(status),
        Err(_) if mutation && forwarded.load(Ordering::SeqCst) => {
            Ok(reference_chat_outcome_unknown(&request_id))
        }
        // The budget ran out before a data channel ever carried the request: the machine
        // never attached, which is a pairing timeout, not a slow upstream.
        Err(_) if !attached.load(Ordering::SeqCst) => {
            Ok(relay_timeout_response(RELAY_STAGE_DATA_PAIRING_TIMEOUT))
        }
        Err(_) => {
            Ok(relay_timeout_response(RELAY_STAGE_UPSTREAM_TRANSFER_TIMEOUT))
        }
    }
}

/// The typed envelope for a status the relay itself produced while reaching a machine, or
/// `None` for a status that needs no explanation.
fn relay_refusal(status: StatusCode) -> Option<Response> {
    let (code, message) = match status {
        StatusCode::NOT_FOUND => ("MACHINE_OFFLINE", "machine is offline"),
        StatusCode::SERVICE_UNAVAILABLE => (
            "RELAY_BUSY",
            "the relay has too many sessions waiting for this machine; retry shortly",
        ),
        StatusCode::GONE => (
            "MACHINE_RECONNECTED",
            "the machine reconnected; retry the request",
        ),
        _ => return None,
    };
    Some(crate::remote::server::machine_error_with_details(
        status,
        code,
        message,
        serde_json::Map::new(),
    ))
}

enum RoundTripError {
    /// The channel failed before any response byte arrived.
    Unanswered,
    Failed(StatusCode),
}

/// Sends one HTTP request over a data channel and reads exactly one response.
/// Returns the channel back when it can carry another request.
async fn http_round_trip(
    mut socket: WebSocket,
    raw: &[u8],
    head: bool,
    max_response: usize,
    // When set, an empty response after this long is `Unanswered` (the channel is presumed
    // dead before the host saw the request).
    first_response_timeout: Option<Duration>,
) -> Result<(Response, Option<WebSocket>), RoundTripError> {
    for chunk in raw.chunks(MAX_MESSAGE_SIZE) {
        socket
            .send(Message::Binary(chunk.to_vec().into()))
            .await
            .map_err(|_| RoundTripError::Unanswered)?;
    }
    let mut response = Vec::new();
    let lost = |response: &Vec<u8>| {
        if response.is_empty() {
            RoundTripError::Unanswered
        } else {
            RoundTripError::Failed(StatusCode::BAD_GATEWAY)
        }
    };
    let first_deadline = first_response_timeout.map(|limit| tokio::time::Instant::now() + limit);
    loop {
        let received = match first_deadline.filter(|_| response.is_empty()) {
            Some(deadline) => match tokio::time::timeout_at(deadline, socket.recv()).await {
                Ok(received) => received,
                Err(_) => return Err(RoundTripError::Unanswered),
            },
            None => socket.recv().await,
        };
        let eof = match received {
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
            Some(Err(_)) => return Err(lost(&response)),
        };
        if response.len() > max_response {
            return Err(RoundTripError::Failed(StatusCode::BAD_GATEWAY));
        }
        if let Some(parsed) =
            parse_http_response_within(&response, head, eof, max_response)
                .map_err(RoundTripError::Failed)?
        {
            let reusable = !eof && response_is_reusable(&response, head);
            return Ok((parsed, reusable.then_some(socket)));
        }
        if eof {
            return Err(lost(&response));
        }
    }
}

/// True only when `raw` is exactly one complete HTTP/1.1 keep-alive response
/// whose length is fixed by its headers, so no stray bytes stay on the channel.
fn response_is_reusable(raw: &[u8], head: bool) -> bool {
    let Some(end) = raw.windows(4).position(|b| b == b"\r\n\r\n") else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(&raw[..end]) else {
        return false;
    };
    let mut lines = text.split("\r\n");
    let mut status_line = lines.next().unwrap_or_default().split_whitespace();
    if status_line.next() != Some("HTTP/1.1") {
        return false;
    }
    let Some(status) = status_line.next().and_then(|s| s.parse::<u16>().ok()) else {
        return false;
    };
    if (100..200).contains(&status) {
        return false;
    }
    let mut length = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("connection")
            && value.split(',').any(|v| v.trim().eq_ignore_ascii_case("close"))
        {
            return false;
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return false;
        }
        if name.eq_ignore_ascii_case("content-length") {
            let Ok(n) = value.parse::<usize>() else {
                return false;
            };
            if length.replace(n).is_some() {
                return false;
            }
        }
    }
    let body = if head || status == 204 || status == 304 {
        0
    } else {
        match length {
            Some(n) => n,
            None => return false,
        }
    };
    raw.len() == end + 4 + body
}

/// Decode HTTP framing independently of WebSocket message boundaries.
#[cfg(test)]
fn parse_http_response(raw: &[u8], head: bool, eof: bool) -> Result<Option<Response>, StatusCode> {
    parse_http_response_within(raw, head, eof, MAX_HTTP_SIZE)
}

/// Decode HTTP framing with an explicit body bound (see REFERENCE_CHAT_RELAY_RESPONSE_MAX).
fn parse_http_response_within(
    raw: &[u8],
    head: bool,
    eof: bool,
    max_body: usize,
) -> Result<Option<Response>, StatusCode> {
    let bad = StatusCode::BAD_GATEWAY;
    // Informational (1xx) blocks precede the final response. Skip them iteratively, and
    // refuse a host that sends more than a handful: each one used to be a recursive call.
    let mut raw = raw;
    let mut informational = 0usize;
    let (end, mut lines, status) = loop {
        let Some(end) = raw.windows(4).position(|b| b == b"\r\n\r\n") else {
            return Ok(None);
        };
        let text = std::str::from_utf8(&raw[..end]).map_err(|_| bad)?;
        let mut lines = text.split("\r\n");
        let mut status_line = lines.next().ok_or(bad)?.split_whitespace();
        if !matches!(status_line.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
            return Err(bad);
        }
        let status =
            StatusCode::from_u16(status_line.next().ok_or(bad)?.parse().map_err(|_| bad)?)
                .map_err(|_| bad)?;
        if !status.is_informational() {
            break (end, lines, status);
        }
        if status == StatusCode::SWITCHING_PROTOCOLS {
            return Err(bad);
        }
        informational += 1;
        if informational > MAX_INFORMATIONAL_RESPONSES {
            return Err(bad);
        }
        raw = &raw[end + 4..];
    };
    let mut headers = HeaderMap::new();
    for line in lines.by_ref() {
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
        // Coding names are case-insensitive. Only plain chunked is accepted: the relay cannot
        // undo another transfer coding, and stripping the hop header would leave the client
        // with bytes it cannot decode.
        let encoding = encoding.to_str().map_err(|_| bad)?;
        let mut codings = encoding.split(',').map(str::trim);
        if !(codings.next().is_some_and(|coding| coding.eq_ignore_ascii_case("chunked"))
            && codings.next().is_none())
        {
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
            if size > max_body {
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
        if length > max_body {
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
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, private"),
    );
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers.remove("clear-site-data");
    headers.remove("set-cookie");
    headers.remove("service-worker-allowed");
    if headers.get_all("content-type").iter().any(|value| {
        value
            .to_str()
            .is_ok_and(|value| value.to_ascii_lowercase().contains("text/html"))
    }) {
        headers.insert("content-type", "text/plain".parse().unwrap());
    }
    headers.insert(
        "content-security-policy",
        "default-src 'none'".parse().unwrap(),
    );
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    Ok(Some(response))
}

fn apply_cors_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static(
            "Authorization, Content-Type, X-Device-Name, X-Requested-With, Accept, X-Ferryx-Client-Build",
        ),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static(RELAY_EXPOSED_HEADERS),
    );
}

async fn relay_cors_middleware(request: Request<Body>, next: axum::middleware::Next) -> Response {
    if request.method() == Method::OPTIONS {
        let mut response = StatusCode::OK.into_response();
        apply_cors_headers(response.headers_mut());
        return response;
    }

    let mut response = next.run(request).await;
    apply_cors_headers(response.headers_mut());
    response
}

/// Builds the Axum router exposing the three tunnel endpoints.
pub fn relay_router(state: RelayState) -> Router {
    relay_router_with_grant_key(state, None)
}

/// The relay operator pins the account service's Ed25519 public key here; `None` keeps the grant
/// route closed even if the route itself is reachable. When `account_state` is present, the
/// account HTTP service is embedded into the same relay router.
pub fn relay_router_with_account(
    state: RelayState,
    account_public_key: Option<String>,
    account_state: Option<Arc<crate::account::service::AccountState>>,
) -> Router {
    if let Some(ref account) = account_state {
        state.set_account_state(account.clone());
    } else {
        state.clear_account_state();
    }
    let grant_gate = crate::remote::account_grants::GrantGate {
        account_public_key,
        delivery: state.clone(),
    };
    let mut router = Router::new()
        .route("/healthz", get(healthz_handler))
        .route("/api/v1/pair/exchange", post(pair_exchange_handler))
        .route("/api/v1/attach/session", post(attach_session_handler))
        .route(
            "/host/{machine_id}/api/v1/socket-ticket",
            post(socket_ticket_handler),
        )
        .route(
            "/host/{machine_id}/api/v1/events",
            get(browser_events_handler),
        )
        .route(
            "/host/{machine_id}/api/v1/terminal/{terminal_id}",
            get(browser_terminal_handler),
        )
        .route(
            "/host/{machine_id}/api/v1/browser/sessions",
            get(browser_sessions_http_handler),
        )
        .route(
            "/host/{machine_id}/api/v1/browser/identify",
            get(browser_identify_http_handler),
        )
        .route(
            "/host/{machine_id}/api/v1/browser/{browser_id}",
            get(browser_screencast_handler),
        )
        .route(
            "/host/{machine_id}/api/v1/terminal/preferences",
            get(terminal_preferences_handler),
        )
        .route(
            "/host/{machine_id}/api/v1/workspace/dag",
            get(dag_stream_handler),
        )
        .route("/host/{machine_id}/api/v1/{*path}", any(host_http_handler))
        .route("/tunnel/control", get(control_handler))
        .route("/tunnel/data/{session_id}", get(data_handler))
        .route("/tunnel/client/{session_id}", get(client_handler))
        .route("/tunnel/opaque/{session_id}", get(opaque_handler))
        .route("/install.sh", get(install_script_handler))
        .route("/install.ps1", get(install_ps1_handler))
        .route("/download/ferryx-cli", get(download_cli_handler))
        .route("/download/{artifact}", get(download_cli_artifact_handler))
        .fallback(axum::routing::get(
            crate::remote::server::serve_static_or_index,
        ))
        .with_state(state)
        .merge(crate::remote::account_grants::grant_gate_router(grant_gate));

    if let Some(account) = account_state {
        router = router.merge(crate::account::service::router(account));
    }

    // Applied last on purpose: `Router::layer` only wraps the routes that exist at
    // the moment it is called, so layering before the account and grant routers are
    // merged leaves their preflights unanswered and their responses without CORS
    // headers. That blocks every account call from the desktop app (origin
    // `tauri://localhost`) and from the remote client the app serves.
    router.layer(axum::middleware::from_fn(relay_cors_middleware))
}

/// Back-compat wrapper: relay-only router with no embedded account service.
pub fn relay_router_with_grant_key(state: RelayState, account_public_key: Option<String>) -> Router {
    relay_router_with_account(state, account_public_key, None)
}

/// Liveness probe for load balancers and the public status page.
///
/// Side-effect free and unauthenticated. `role` tells a failover pool which host it reached:
/// `primary` serves a writable account store, `standby` a read-only replica, and `relay` has
/// no embedded account service.
async fn healthz_handler(State(state): State<RelayState>) -> Response {
    // `is_read_only` touches the filesystem: run it off the async worker and without holding
    // the account-state lock every `/host/...` request also takes.
    let role = match state.account_state() {
        Some(account) => match crate::ipc::run_blocking(move || Ok(account.is_read_only())).await {
            Ok(true) => "standby",
            Ok(false) => "primary",
            Err(error) => {
                tracing::warn!(%error, "healthz could not read the account store role");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        },
        None => "relay",
    };
    let mut response = Json(serde_json::json!({ "ok": true, "role": role })).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
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

/// Spawns the background task that ends remote access for account owners whose grace
/// period has ended.
///
/// Only a commercial relay that serves an account service enforces suspension, and only
/// against machines enrolled with that account: every other deployment is a no-op. The task
/// closes relay streams; it never touches a daemon process, a PTY or a stored record.
pub fn spawn_suspension_sweeper(state: RelayState) -> tokio::task::JoinHandle<()> {
    spawn_suspension_sweeper_with_interval(state, SUSPENSION_SWEEP_INTERVAL)
}

fn spawn_suspension_sweeper_with_interval(
    state: RelayState,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            let now = crate::account::billing::routes::billing_now();
            match state.sweep_suspended_owners(now).await {
                Ok(sweep) if sweep.stopped_owners > 0 => tracing::warn!(
                    owners = sweep.stopped_owners,
                    sessions = sweep.closed_sessions,
                    controls = sweep.closed_controls,
                    "remote access stopped for owners past their grace period"
                ),
                Ok(_) => {}
                Err(error) => tracing::error!(
                    code = error.code,
                    message = %error.message,
                    "suspension sweep failed; the next tick retries"
                ),
            }
        }
    })
}

/// Outcome of one suspension pass, for the caller's log line and for tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SuspensionSweep {
    /// Distinct owners found to be `Stopped` at the injected clock.
    pub stopped_owners: usize,
    /// Live relay sessions whose browser and daemon halves were ended.
    pub closed_sessions: usize,
    /// Live control tunnels that were ended.
    pub closed_controls: usize,
}

/// The account machines whose relay streams must end, with the owners behind them.
#[derive(Debug, Default, Clone)]
struct SuspensionTargets {
    owners: HashSet<String>,
    machines: HashSet<String>,
}

/// Blocking half of [`RelayState::sweep_suspended_owners`]: resolves every candidate machine
/// to its account record, evaluates each distinct owner once at `now`, and returns the
/// machines owned by an owner whose grace period has ended.
///
/// A store read error is never defaulted to healthy, but it also does not abort the pass: the
/// failing machine or owner is logged and retried on the next tick while every other owner is
/// still enforced.
fn suspension_targets(
    account: &crate::account::service::AccountState,
    candidates: &[String],
    now: u64,
) -> Result<SuspensionTargets, crate::account::service::ApiError> {
    Ok(suspension_targets_with(
        candidates,
        |machine_id| {
            crate::account::service::try_enrolled_machine_by_id(account, machine_id)
                .map(|record| record.map(|record| (record.machine_id, record.owner_user_id)))
        },
        |owner_user_id| {
            crate::account::billing::routes::entitlement_for_user(account, owner_user_id, now)
                .map(|entitlement| {
                    entitlement.status
                        == crate::account::billing::entitlement::EntitlementStatus::Stopped
                })
        },
    ))
}

/// [`suspension_targets`] over injected reads: `machine` resolves a candidate to its
/// `(machine_id, owner_user_id)`, `stopped` evaluates one owner.
fn suspension_targets_with(
    candidates: &[String],
    machine: impl Fn(&str) -> Result<Option<(String, String)>, crate::account::service::ApiError>,
    stopped: impl Fn(&str) -> Result<bool, crate::account::service::ApiError>,
) -> SuspensionTargets {
    let mut owned: Vec<(String, String)> = Vec::new();
    for machine_id in candidates {
        match machine(machine_id) {
            Ok(Some(record)) => owned.push(record),
            // A machine that never enrolled with this account is outside billing, so the
            // operator-token path stays exactly as it was.
            Ok(None) => continue,
            Err(error) => {
                tracing::error!(
                    %machine_id,
                    code = error.code,
                    message = %error.message,
                    "suspension sweep could not read a machine; the next tick retries it"
                );
                continue;
            }
        }
    }

    let mut evaluated: HashSet<String> = HashSet::new();
    let mut targets = SuspensionTargets::default();
    for (machine_id, owner_user_id) in owned {
        if evaluated.insert(owner_user_id.clone()) {
            match stopped(&owner_user_id) {
                Ok(true) => {
                    targets.owners.insert(owner_user_id.clone());
                }
                Ok(false) => {}
                Err(error) => tracing::error!(
                    %owner_user_id,
                    code = error.code,
                    message = %error.message,
                    "suspension sweep could not evaluate an owner; the next tick retries it"
                ),
            }
        }
        if targets.owners.contains(&owner_user_id) {
            targets.machines.insert(machine_id);
        }
    }
    targets
}

impl RelayState {
    /// Every machine that currently holds a live control tunnel or relay session.
    fn suspension_candidates(&self) -> Vec<String> {
        let mut candidates: HashSet<String> = self
            .inner
            .control_channels
            .lock()
            .keys()
            .cloned()
            .collect();
        candidates.extend(
            self.inner
                .pending_sessions
                .lock()
                .values()
                .map(|entry| entry.owner.clone()),
        );
        candidates.into_iter().collect()
    }

    /// One suspension pass over every account machine that currently holds a live control
    /// tunnel or relay session: owners evaluated at `now` are `Stopped`, so their sessions
    /// and control tunnels end with [`SUSPENSION_REASON`] and the stop is persisted through
    /// the billing store.
    ///
    /// `now` is the billing clock ([`crate::account::billing::routes::billing_now`] in
    /// production): the grace deadline is decided by it, so it is a parameter rather than an
    /// ambient read. Account I/O is synchronous, so it runs on a blocking thread; no relay
    /// lock is held across it, and no socket is written to while a map is locked.
    async fn sweep_suspended_owners(
        &self,
        now: u64,
    ) -> Result<SuspensionSweep, crate::account::service::ApiError> {
        let Some(account) = self.account_state() else {
            return Ok(SuspensionSweep::default());
        };
        if !account.deployment_mode.is_billing_enabled() {
            return Ok(SuspensionSweep::default());
        }
        let candidates = self.suspension_candidates();
        if candidates.is_empty() {
            return Ok(SuspensionSweep::default());
        }
        let account_for_evaluation = Arc::clone(&account);
        let targets = crate::ipc::run_blocking(move || {
            Ok(suspension_targets(&account_for_evaluation, &candidates, now))
        })
        .await
        .map_err(|error| crate::account::service::ApiError::internal(error.to_string()))??;
        if targets.machines.is_empty() {
            return Ok(SuspensionSweep {
                stopped_owners: targets.owners.len(),
                ..SuspensionSweep::default()
            });
        }

        // One control tunnel may be registered under its machine id and under an alias.
        // Both keys share a generation, so matching on the generation ends exactly one
        // tunnel once, whichever name the account store knows the machine by.
        let (stopped_generations, stopped_keys): (HashSet<u64>, HashSet<String>) = {
            let channels = self.inner.control_channels.lock();
            let generations: HashSet<u64> = channels
                .iter()
                .filter(|(key, _)| targets.machines.contains(*key))
                .map(|(_, channel)| channel.generation)
                .collect();
            // A session outlives its control tunnel, so the stopped machines themselves are
            // keys too: matching only on live channel names would leave a session that was
            // already paired running with no control tunnel left to close.
            let mut keys: HashSet<String> = targets.machines.clone();
            keys.extend(
                channels
                    .iter()
                    .filter(|(key, channel)| {
                        targets.machines.contains(*key)
                            || generations.contains(&channel.generation)
                    })
                    .map(|(key, _)| key.clone()),
            );
            (generations, keys)
        };

        let suspended_sessions: Vec<(String, SuspensionSignal)> = self
            .inner
            .pending_sessions
            .lock()
            .iter()
            .filter(|(_, entry)| stopped_keys.contains(&entry.owner))
            .map(|(session_id, entry)| (session_id.clone(), Arc::clone(&entry.suspension)))
            .collect();
        let suspended_controls: Vec<SuspensionSignal> = {
            let signals = self.inner.control_suspensions.lock();
            stopped_generations
                .iter()
                .filter_map(|generation| signals.get(generation).cloned())
                .collect()
        };

        // Arm outside every lock: waking a splice task must never run while a relay map is
        // locked.
        for (session_id, signal) in &suspended_sessions {
            tracing::warn!(
                %session_id,
                reason = SUSPENSION_REASON,
                "closing relay session of a stopped account owner"
            );
            signal.send_replace(Some(SUSPENSION_REASON.to_string()));
        }
        for signal in &suspended_controls {
            signal.send_replace(Some(SUSPENSION_REASON.to_string()));
        }

        Ok(SuspensionSweep {
            stopped_owners: targets.owners.len(),
            closed_sessions: suspended_sessions.len(),
            closed_controls: suspended_controls.len(),
        })
    }
}

async fn install_script_handler() -> impl axum::response::IntoResponse {
    let script = include_str!("../../../scripts/install.sh");
    (
        [(axum::http::header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8")],
        script,
    )
}

async fn install_ps1_handler() -> impl axum::response::IntoResponse {
    let script = include_str!("../../../scripts/install.ps1");
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        script,
    )
}

async fn download_cli_handler() -> axum::response::Response {
    download_cli_artifact_handler(axum::extract::Path("ferryx-cli".to_string())).await
}

/// Artifact names the unauthenticated `/download/{artifact}` route may serve.
///
/// The relay proxy is reachable without authentication, so an open filename
/// lookup would turn the download route into a file-read oracle for anything
/// sitting in the staging directories. Only published `ferryx-cli` artifacts
/// are addressable.
fn is_downloadable_artifact(artifact: &str) -> bool {
    matches!(
        artifact,
        "ferryx-cli"
            | "ferryx-cli-linux-amd64"
            | "ferryx-cli-linux-arm64"
            | "ferryx-cli-darwin-universal"
            | "ferryx-cli-windows-amd64.exe"
    )
}

async fn download_cli_artifact_handler(
    axum::extract::Path(artifact): axum::extract::Path<String>,
) -> axum::response::Response {
    let candidate_dirs = [
        std::env::var("FERRYX_DOWNLOADS_DIR").ok().map(std::path::PathBuf::from),
        std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join(".ferryx").join("downloads")),
        std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join("bin")),
        Some(std::path::PathBuf::from("/usr/local/bin")),
    ];

    download_cli_artifact_from_candidate_dirs(
        &artifact,
        candidate_dirs.into_iter().flatten(),
    )
    .await
}

/// Internal artifact download implementation searching caller-provided candidate directories.
async fn download_cli_artifact_from_candidate_dirs(
    artifact: &str,
    candidate_dirs: impl IntoIterator<Item = std::path::PathBuf>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !is_downloadable_artifact(artifact) {
        return (StatusCode::BAD_REQUEST, "Unknown artifact name").into_response();
    }

    for dir in candidate_dirs {
        let file_path = dir.join(artifact);
        if let Ok(meta) = tokio::fs::symlink_metadata(&file_path).await {
            if meta.file_type().is_file() {
                if let Ok(file) = tokio::fs::File::open(&file_path).await {
                    // Stream the binary instead of reading tens of MB into memory per request.
                    let body = futures_util::stream::unfold(file, |mut file| async move {
                        use tokio::io::AsyncReadExt as _;
                        let mut chunk = bytes::BytesMut::with_capacity(64 * 1024);
                        match file.read_buf(&mut chunk).await {
                            Ok(0) => None,
                            Ok(_) => Some((Ok::<_, std::io::Error>(chunk.freeze()), file)),
                            Err(error) => Some((Err(error), file)),
                        }
                    });
                    // Every name comes from the fixed allowlist above, so it is safe to quote.
                    let disposition = format!("attachment; filename=\"{artifact}\"");
                    let mut response = Body::from_stream(body).into_response();
                    let headers = response.headers_mut();
                    headers.insert(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("application/octet-stream"),
                    );
                    if let Ok(value) = HeaderValue::from_str(&disposition) {
                        headers.insert(header::CONTENT_DISPOSITION, value);
                    }
                    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(meta.len()));
                    return response;
                }
            }
        }
    }

    let github_url = format!("https://github.com/Indosaram/ferryx/releases/latest/download/{artifact}");
    axum::response::Redirect::temporary(&github_url).into_response()
}

#[cfg(all(test, unix))]
fn download_artifact_router(
    candidate_dirs: Vec<std::path::PathBuf>,
) -> axum::Router {
    use axum::routing::get;
    axum::Router::new().route(
        "/download/{artifact}",
        get(move |axum::extract::Path(artifact): axum::extract::Path<String>| {
            let dirs = candidate_dirs.clone();
            async move {
                download_cli_artifact_from_candidate_dirs(&artifact, dirs).await
            }
        }),
    )
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
    let machine_id = headers
        .get("x-ferryx-machine-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
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
        .on_upgrade(move |socket| {
            authenticate_control_socket(socket, state, token, machine_id, ip)
        }))
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
    machine_id: Option<String>,
    ip: IpAddr,
) {
    if let Some(token) = token {
        state.record_auth(ip, true);
        handle_control_socket(socket, state, token, machine_id, false).await;
        return;
    }
    let challenge = ControlChallenge {
        nonce: uuid::Uuid::new_v4().to_string(),
        timestamp: current_time_secs(),
        audience: Some(state.inner.control_audience.clone()),
    };
    // Set false when a refusal is not the caller's credential (billing, suspension, disk).
    let mut credential_failure = true;
    let authenticate = async {
        socket
            .send(Message::Text(
                serde_json::to_string(&challenge).unwrap().into(),
            ))
            .await
            .map_err(|error| error.to_string())?;
        // Keepalives may precede the auth frame; the outer timeout still bounds the wait.
        let auth = loop {
            match socket.recv().await {
                Some(Ok(Message::Text(text))) => {
                    break serde_json::from_str::<ControlAuth>(&text)
                        .map_err(|_| "Invalid ControlAuth frame".to_string())?
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                _ => return Err("Expected ControlAuth text frame".to_string()),
            }
        };
        if auth.machine_id.is_empty() || state.validate_machine_token(&auth.machine_id) {
            return Err("Invalid or reserved machine ID".to_string());
        }
        if current_time_secs().abs_diff(auth.timestamp) > 60 {
            return Err("Authentication timestamp expired".to_string());
        }
        // The signature must cover THIS relay's audience. A client that predates the
        // audience field is accepted only while this relay still runs the legacy
        // audience, so a deployment that sets its own origin cannot be presented with
        // a signature minted for a different relay.
        if auth.timestamp != challenge.timestamp
            || !verify_control_challenge(
                &auth.public_key,
                &auth.machine_id,
                &state.inner.control_audience,
                &challenge.nonce,
                auth.timestamp,
                &auth.signature,
            )
        {
            return Err("Invalid machine signature".to_string());
        }
        // The credential checks passed; from here a slow or failed bind (billing, disk) is not
        // the caller's fault unless the bind itself refuses the key or enrollment token.
        credential_failure = false;
        let auth_for_bind = auth.clone();
        let state_for_bind = state.clone();
        let bound =
            crate::ipc::run_blocking(move || Ok(state_for_bind.bind_machine_key(&auth_for_bind)))
                .await
                .map_err(|error| error.to_string())?;
        if let Err(error) = bound {
            // Only a key or enrollment-token mismatch is the daemon's own credential problem;
            // billing, suspension and persistence refusals are not.
            credential_failure = error == MACHINE_ID_CLAIMED || error == ENROLLMENT_TOKEN_REQUIRED;
            return Err(error);
        }
        Ok(auth.machine_id)
    };
    let result = match timeout(CONTROL_AUTH_TIMEOUT, authenticate).await {
        Ok(result) => result,
        Err(_) => Err("Control authentication timed out".to_string()),
    };
    // Only a credential the daemon got wrong counts toward the per-IP lockout. A billing
    // outage, a suspended owner or a disk error is the relay's or the account's state, and
    // locking the IP for it would lock out every daemon behind that address.
    match &result {
        Ok(_) => state.record_auth(ip, true),
        Err(_) if credential_failure => state.record_auth(ip, false),
        Err(_) => {}
    }
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
        Ok(machine_id) => handle_control_socket(socket, state, machine_id, None, true).await,
        Err(ref err) => {
            if err == "REMOTE_SUSPENDED" {
                let close_frame = axum::extract::ws::CloseFrame {
                    code: axum::extract::ws::close_code::POLICY,
                    reason: "REMOTE_SUSPENDED".into(),
                };
                if !matches!(
                    timeout(
                        CONTROL_AUTH_TIMEOUT,
                        socket.send(Message::Close(Some(close_frame)))
                    )
                    .await,
                    Ok(Ok(()))
                ) {
                    tracing::debug!("relay rejected control socket close failed or timed out");
                }
            } else if !matches!(
                timeout(CONTROL_AUTH_TIMEOUT, socket.close()).await,
                Ok(Ok(()))
            ) {
                tracing::debug!("relay rejected control socket close failed or timed out");
            }
        }
    }
}

async fn handle_control_socket(
    mut socket: WebSocket,
    state: RelayState,
    machine_token: String,
    machine_id: Option<String>,
    // True when `machine_token` is the Ed25519-authenticated machine id itself. Such a socket
    // speaks only for that id and may never bind an alias.
    key_authenticated: bool,
) {
    let (generation, mut notices, mut grants, own_channel, own_grant) =
        state.register_control_channel_handle(machine_token.clone());
    let mut pending_grant: Option<oneshot::Sender<AccountGrantOfferDelivered>> = None;
    // An announced alias another live generation still holds (for example this daemon's own
    // previous, half-open socket) is retried on later inbound frames until that holder is gone.
    let mut pending_alias = machine_id.filter(|id| id != &machine_token);
    if let Some(id) = pending_alias.clone() {
        if state.bind_control_alias_for(&own_channel, &own_grant, id) {
            pending_alias = None;
        } else {
            tracing::warn!("relay deferred a control alias another live machine holds");
        }
    }
    let mut suspension = state.control_suspension_receiver(generation);
    // The daemon pings every 15 s. A socket that sends nothing at all for this long is
    // half-open, and must stop being offered sessions it will never answer.
    let mut read_deadline = tokio::time::Instant::now() + CONTROL_READ_IDLE;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(read_deadline) => {
                tracing::warn!("relay control socket idle; closing");
                let close_frame = axum::extract::ws::CloseFrame {
                    code: axum::extract::ws::close_code::AWAY,
                    reason: "control channel idle".into(),
                };
                let _ = timeout(TRANSFER_TIMEOUT, socket.send(Message::Close(Some(close_frame)))).await;
                break;
            }
            reason = next_suspension_reason(&mut suspension) => {
                // The owner's grace period ended: end the tunnel with the exact reason
                // instead of leaving a live control channel behind for a stopped account.
                let close_frame = axum::extract::ws::CloseFrame {
                    code: axum::extract::ws::close_code::POLICY,
                    reason: reason.into(),
                };
                if !matches!(
                    timeout(TRANSFER_TIMEOUT, socket.send(Message::Close(Some(close_frame)))).await,
                    Ok(Ok(()))
                ) {
                    tracing::warn!("relay suspended control close failed or timed out");
                }
                break;
            }
            // One delivery at a time: the daemon's ack carries no reliable correlation, so a
            // second grant waits until the first is acknowledged or its caller gave up.
            _ = async {
                if let Some(ack) = pending_grant.as_mut() {
                    ack.closed().await;
                }
            }, if pending_grant.is_some() => {
                // The caller timed out; its slot must not capture the next delivery's ack.
                pending_grant = None;
            }
            grant = grants.recv(), if pending_grant.is_none() => {
                let Some(GrantDeliveryRequest { delivery, ack }) = grant else { break };
                let Ok(payload) = serde_json::to_string(&delivery) else { continue };
                if !matches!(timeout(TRANSFER_TIMEOUT, socket.send(Message::Text(payload.into()))).await, Ok(Ok(()))) {
                    tracing::warn!("relay grant delivery failed or timed out");
                    break;
                }
                pending_grant = Some(ack);
            }
            notice = notices.recv() => {
                let Some(notice) = notice else { break };
                let Ok(payload) = serde_json::to_string(&notice) else { continue };
                if !matches!(timeout(TRANSFER_TIMEOUT, socket.send(Message::Text(payload.into()))).await, Ok(Ok(()))) {
                    tracing::warn!("relay control send failed or timed out");
                    break;
                }
            }
            incoming = socket.recv() => {
                read_deadline = tokio::time::Instant::now() + CONTROL_READ_IDLE;
                if let Some(id) = pending_alias.clone() {
                    if state.bind_control_alias_for(&own_channel, &own_grant, id) {
                        pending_alias = None;
                    }
                }
                match incoming {
                    Some(Ok(Message::Close(_))) => {
                        // Flush tungstenite's queued close acknowledgement back to the client
                        // so the client receives a clean close handshake rather than an abrupt drop.
                        match timeout(TRANSFER_TIMEOUT, socket.flush()).await {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                tracing::warn!(%error, "relay control close flush IO error");
                            }
                            Err(_) => {
                                tracing::warn!("relay control close flush timed out");
                            }
                        }
                        break;
                    }
                    None => break,
                    Some(Ok(Message::Text(text))) => {
                        // The daemon answers a grant delivery with `AccountGrantOfferDelivered`;
                        // that frame fills the slot the caller of `deliver_grant_offer` waits on.
                        if pending_grant.is_some() {
                            if let Ok(delivered) =
                                serde_json::from_str::<AccountGrantOfferDelivered>(&text)
                            {
                                if let Some(ack) = pending_grant.take() {
                                    let _ = ack.send(delivered);
                                }
                                continue;
                            }
                        }
                        match serde_json::from_str::<ControlRequest>(&text).or_else(|_| {
                            serde_json::from_str::<RegisterPairingPin>(&text).map(ControlRequest::RegisterPairingPin)
                        }) {
                            Ok(ControlRequest::AllocateSession { opaque }) => {
                                let id = uuid::Uuid::new_v4().to_string();
                                match state.try_issue_session_of_kind(&machine_token, &id, Some(generation), opaque, SessionKind::Http) {
                                    Ok(()) => {}
                                    // A full budget is transient: tell the daemon and keep its
                                    // control channel, or every busy moment becomes a reconnect.
                                    Err(IssueRejection::Busy) => {
                                        tracing::warn!("relay session allocation rejected: busy");
                                        let payload = serde_json::json!({"type": "AllocateSessionRejected", "reason": "busy"}).to_string();
                                        if !matches!(timeout(TRANSFER_TIMEOUT, socket.send(Message::Text(payload.into()))).await, Ok(Ok(()))) {
                                            tracing::warn!("relay allocation rejection send failed or timed out");
                                            break;
                                        }
                                    }
                                    Err(rejection) => {
                                        tracing::warn!(?rejection, "relay session allocation rejected");
                                        break;
                                    }
                                }
                            }
                            Ok(ControlRequest::RegisterPairingPin(registration)) => {
                                let foreign = !registration.machine_id.is_empty()
                                    && registration.machine_id != machine_token;
                                // An Ed25519 socket speaks only for its authenticated id; a token
                                // socket may alias an id no other live machine holds.
                                let alias_refused = foreign
                                    && (key_authenticated
                                        || !state.bind_control_alias_for(
                                            &own_channel,
                                            &own_grant,
                                            registration.machine_id.clone(),
                                        ));
                                let target_machine = if !registration.machine_id.is_empty() {
                                    registration.machine_id.clone()
                                } else {
                                    machine_token.clone()
                                };
                                let ack_generation = registration.generation;
                                let ack_pin = registration.pin.clone();
                                let status = if alias_refused {
                                    "Machine ID does not match this control identity"
                                } else {
                                    match state.register_pairing(&target_machine, generation, registration) {
                                        Ok(()) => "ready",
                                        Err(error) => error,
                                    }
                                };
                                let ack = RegisterPairingPinAck {
                                    generation: ack_generation,
                                    pin: ack_pin,
                                    machine_id: target_machine,
                                    status: status.into(),
                                };
                                if !matches!(timeout(TRANSFER_TIMEOUT, socket.send(Message::Text(serde_json::to_string(&ack).unwrap().into()))).await, Ok(Ok(()))) {
                                    tracing::warn!("relay pairing acknowledgment failed");
                                    break;
                                }
                            }
                            Err(error) => tracing::warn!(%error, "invalid relay control request"),
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if !matches!(
                            timeout(TRANSFER_TIMEOUT, socket.send(Message::Pong(payload))).await,
                            Ok(Ok(()))
                        ) {
                            tracing::warn!("relay control pong failed or timed out");
                            break;
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
    upgrade_half(ws, state, session_id, HalfKind::Data, peer_ip(peer))
}

/// `GET /tunnel/client/:session_id` - the remote client's channel for a
/// specific session.
async fn client_handler(
    ws: WebSocketUpgrade,
    AxumPath(session_id): AxumPath<String>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    State(state): State<RelayState>,
) -> Result<Response, StatusCode> {
    upgrade_half(ws, state, session_id, HalfKind::Client, peer_ip(peer))
}

/// `GET /tunnel/opaque/:session_id` - the account client's channel for a session whose
/// traffic it encrypts end to end. The relay splices frames verbatim through
/// `proxy_sockets` and never runs a WebSocket handshake over them, so no plaintext of this
/// session passes through the relay.
async fn opaque_handler(
    ws: WebSocketUpgrade,
    AxumPath(session_id): AxumPath<String>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    State(state): State<RelayState>,
) -> Result<Response, StatusCode> {
    upgrade_half(ws, state, session_id, HalfKind::Opaque, peer_ip(peer))
}

fn upgrade_half(
    ws: WebSocketUpgrade,
    state: RelayState,
    session_id: String,
    kind: HalfKind,
    ip: IpAddr,
) -> Result<Response, StatusCode> {
    // Issued session capabilities are already bounded by the pending-session
    // capacity, expiry, generation and duplicate-half guards. Do not charge
    // their redemption to the shared proxy IP's unauthenticated attempt limit.
    let (generation, outcome) = match state.reserve_half(&session_id, kind) {
        Ok(reservation) => reservation,
        Err(status) => {
            return Err(if state.admit(ip, Instant::now()) {
                status
            } else {
                StatusCode::TOO_MANY_REQUESTS
            });
        }
    };
    let guard = SessionGuard {
        armed: true,
        state,
        session_id,
        generation,
    };
    let lifetime = guard.state.session_lifetime(&guard.session_id);
    Ok(ws
        .max_message_size(MAX_MESSAGE_SIZE)
        .max_frame_size(MAX_MESSAGE_SIZE)
        .on_upgrade(move |socket| handle_half_socket(socket, guard, outcome, kind, lifetime)))
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
    kind: HalfKind,
    lifetime: Duration,
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
            let mut suspension = guard.state.session_suspension_receiver(&guard.session_id);
            // Whoever reserves the first half drives the splice, so this task's own half is
            // `kind` and the half that arrives second is the opposite one.
            let browser_side = if matches!(kind, HalfKind::Data) {
                BrowserSide::Peer
            } else {
                BrowserSide::Own
            };
            let deadline = tokio::time::Instant::now() + lifetime;
            let transfer = async {
                let mut buffered = Vec::new();
                let mut bytes = 0;
                let pairing = async {
                    let mut suspended = suspension.borrow_and_update().clone();
                    while suspended.is_none() {
                        tokio::select! {
                            peer = &mut rx => {
                                return Ok::<PairingWait, anyhow::Error>(match peer {
                                    Ok(peer) => PairingWait::Paired(peer),
                                    Err(_) => PairingWait::Abandoned,
                                });
                            }
                            reason = next_suspension_reason(&mut suspension) => {
                                suspended = Some(reason);
                            }
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
                    // The `select!` above borrows `socket` for receiving, so the close frame is
                    // sent here, where the socket is free to write again.
                    Ok(PairingWait::Suspended(suspended.unwrap_or_default()))
                };
                let pairing_result = timeout(SESSION_PAIRING_TIMEOUT, pairing).await;
                let mut peer = match pairing_result {
                    Ok(Ok(PairingWait::Paired(peer))) => peer,
                    // Timed out, or the entry was retired (by the sweeper at the same deadline)
                    // before the opposite half arrived.
                    Err(_) | Ok(Ok(PairingWait::Abandoned)) => {
                        if browser_side == BrowserSide::Own {
                            close_browser(&mut socket, axum::extract::ws::close_code::AGAIN, "machine did not connect").await;
                        }
                        anyhow::bail!("relay session pairing timed out");
                    }
                    Ok(Err(error)) => return Err(error),
                    Ok(Ok(PairingWait::Suspended(reason))) => {
                        if browser_side == BrowserSide::Own {
                            close_suspended_browser(&mut socket, &reason).await;
                        }
                        anyhow::bail!(
                            "relay session suspended before its halves paired: {reason}"
                        );
                    }
                };
                #[cfg(test)]
                let tap = guard
                    .state
                    .inner
                    .spliced_taps
                    .lock()
                    .get(&guard.session_id)
                    .cloned();
                for message in buffered {
                    #[cfg(test)]
                    if let Some(tap) = &tap {
                        match &message {
                            Message::Binary(data) => tap.lock().extend_from_slice(data),
                            Message::Text(text) => tap.lock().extend_from_slice(text.as_bytes()),
                            _ => {}
                        }
                    }
                    timeout(TRANSFER_TIMEOUT, peer.send(message)).await??;
                }
                proxy_sockets(
                    socket,
                    peer,
                    browser_side,
                    suspension,
                    deadline,
                    #[cfg(test)]
                    tap,
                )
                .await
            };
            // The browser-facing pump closes with a reason at `deadline`; the outer bound only
            // backstops that best-effort close.
            match timeout(lifetime + TRANSFER_TIMEOUT, transfer).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(%error, "relay session failed"),
                Err(error) => tracing::warn!(%error, "relay session lifetime exceeded"),
            }
            drop(guard);
        }
    }
}

/// Best-effort close of a browser-facing socket with a code and reason, bounded by
/// [`TRANSFER_TIMEOUT`].
async fn close_browser(socket: &mut WebSocket, code: u16, reason: &'static str) {
    let frame = axum::extract::ws::CloseFrame {
        code,
        reason: reason.into(),
    };
    if !matches!(
        timeout(TRANSFER_TIMEOUT, socket.send(Message::Close(Some(frame)))).await,
        Ok(Ok(()))
    ) {
        tracing::debug!(reason, "relay browser close frame was not delivered");
    }
}

/// Sends the exact suspension reason to a browser-facing socket before its session ends.
async fn close_suspended_browser(socket: &mut WebSocket, reason: &str) {
    let frame = axum::extract::ws::CloseFrame {
        code: axum::extract::ws::close_code::POLICY,
        reason: reason.to_owned().into(),
    };
    if !matches!(
        timeout(TRANSFER_TIMEOUT, socket.send(Message::Close(Some(frame)))).await,
        Ok(Ok(()))
    ) {
        tracing::warn!("relay suspended close frame was not delivered");
    }
}

/// Proxies WebSocket frames bidirectionally between `a` and `b` until
/// either side closes or errors, or the suspension sweeper ends the session.
///
/// `browser_side` names the half that faces the remote browser. Only the direction that
/// writes to that half watches `suspension`, so a suspended session ends with the exact
/// reason on the browser's socket and the daemon half is torn down with it.
async fn proxy_sockets(
    a: WebSocket,
    b: WebSocket,
    browser_side: BrowserSide,
    suspension: watch::Receiver<Option<String>>,
    deadline: tokio::time::Instant,
    #[cfg(test)] tap: Option<Arc<Mutex<Vec<u8>>>>,
) -> anyhow::Result<()> {
    let (mut a_tx, mut a_rx) = a.split();
    let (mut b_tx, mut b_rx) = b.split();

    #[cfg(test)]
    let tap_a = tap.clone();
    // `a_to_b` writes to the peer half; `b_to_a` writes to this task's own half.
    let mut peer_suspension = (browser_side == BrowserSide::Peer).then(|| suspension.clone());
    let mut own_suspension = (browser_side == BrowserSide::Own).then_some(suspension);
    let peer_deadline = (browser_side == BrowserSide::Peer).then_some(deadline);
    let own_deadline = (browser_side == BrowserSide::Own).then_some(deadline);
    let a_to_b = pump_relay_direction(
        &mut a_rx,
        &mut b_tx,
        &mut peer_suspension,
        peer_deadline,
        #[cfg(test)]
        tap_a.as_ref(),
    );
    #[cfg(test)]
    let tap_b = tap;
    let b_to_a = pump_relay_direction(
        &mut b_rx,
        &mut a_tx,
        &mut own_suspension,
        own_deadline,
        #[cfg(test)]
        tap_b.as_ref(),
    );

    tokio::select! {
        result = a_to_b => result,
        result = b_to_a => result,
    }
}

/// Pumps one direction of a relay session until either side closes, and — when this
/// direction writes to the browser half — until the suspension sweeper arms `suspension`,
/// at which point the exact reason is sent before the session is torn down.
async fn pump_relay_direction(
    rx: &mut futures_util::stream::SplitStream<WebSocket>,
    tx: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    suspension: &mut Option<watch::Receiver<Option<String>>>,
    // Set only on the direction that writes to the browser half: at the session lifetime the
    // browser is told why its socket ends instead of seeing an abnormal close.
    deadline: Option<tokio::time::Instant>,
    #[cfg(test)] tap: Option<&Arc<Mutex<Vec<u8>>>>,
) -> anyhow::Result<()> {
    use futures_util::FutureExt as _;
    // Frames are queued with `feed` and flushed only when the next frame is not already
    // waiting, so a burst of small frames costs one write rather than one per frame.
    // A burst is capped so a peer that never pauses still gets flushed and the suspension and
    // lifetime signals are still observed.
    const MAX_UNFLUSHED_FRAMES: usize = 32;
    let mut unflushed = 0usize;
    loop {
        let ready = if unflushed < MAX_UNFLUSHED_FRAMES {
            rx.next().now_or_never()
        } else {
            None
        };
        let incoming = match ready {
            Some(incoming) => incoming,
            None => {
                if unflushed > 0 {
                    timeout(TRANSFER_TIMEOUT, tx.flush()).await??;
                    unflushed = 0;
                }
                tokio::select! {
                    incoming = rx.next() => incoming,
                    reason = await_suspension(suspension) => {
                        let frame = axum::extract::ws::CloseFrame {
                            code: axum::extract::ws::close_code::POLICY,
                            reason: reason.into(),
                        };
                        timeout(TRANSFER_TIMEOUT, tx.send(Message::Close(Some(frame)))).await??;
                        return Ok(());
                    }
                    _ = async { tokio::time::sleep_until(deadline.unwrap_or_else(tokio::time::Instant::now)).await }, if deadline.is_some() => {
                        let frame = axum::extract::ws::CloseFrame {
                            code: axum::extract::ws::close_code::AWAY,
                            reason: "session lifetime exceeded".into(),
                        };
                        timeout(TRANSFER_TIMEOUT, tx.send(Message::Close(Some(frame)))).await??;
                        return Ok(());
                    }
                }
            }
        };
        let Some(msg) = incoming else {
            if unflushed > 0 {
                timeout(TRANSFER_TIMEOUT, tx.flush()).await??;
            }
            return Ok(());
        };
        let msg = msg?;
        let is_close = matches!(msg, Message::Close(_));
        #[cfg(test)]
        if let Some(tap) = tap {
            match &msg {
                Message::Binary(bytes) => tap.lock().extend_from_slice(bytes),
                Message::Text(text) => tap.lock().extend_from_slice(text.as_bytes()),
                _ => {}
            }
        }
        timeout(TRANSFER_TIMEOUT, tx.feed(msg)).await??;
        unflushed += 1;
        if is_close {
            timeout(TRANSFER_TIMEOUT, tx.flush()).await??;
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    // `FERRYX_DOWNLOADS_DIR` is process-global and the harness runs tests in parallel
    // threads, so every test that sets it must hold this guard for the whole body.
    static DOWNLOAD_DIR_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    thread_local! {
        static PROBE_ENROLLMENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    pub(super) fn probe_enrollment_transaction(path: &std::path::Path, boundary: &str) {
        if !PROBE_ENROLLMENT.get() {
            return;
        }
        // A separate process must not acquire the transaction lock while production
        // is reloading or publishing. No timing or two-reader rendezvous is involved.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "remote::relay_server::tests::test_relay_transaction_lock_probe_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("FERRYX_P23_PROBE_KEY_FILE", path)
            .output()
            .expect("run owned transaction probe process");
        assert!(
            output.status.success(),
            "cross-process transaction exclusion failed at {boundary}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn test_relay_transaction_lock_probe_child() {
        let Some(path) = std::env::var_os("FERRYX_P23_PROBE_KEY_FILE") else {
            return;
        };
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(std::path::PathBuf::from(path).with_extension("tx.lock"))
            .unwrap();
        assert!(
            matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
            "enrollment must hold a cross-process lock throughout reload and publication"
        );
    }

    #[test]
    fn test_relay_key_store_transaction_is_cross_process() {
        struct ProbeGuard;
        impl Drop for ProbeGuard {
            fn drop(&mut self) {
                PROBE_ENROLLMENT.set(false);
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("machine_keys.json");
        let first = RelayState::new_with_key_store(vec![], &path).unwrap();
        let second = RelayState::new_with_key_store(vec![], &path).unwrap();
        let auth = |seed, machine: &str| ControlAuth {
            machine_id: machine.to_owned(),
            public_key: identity(seed, machine).public_key,
            display_name: machine.to_owned(),
            enrollment_token: None,
            signature: String::new(),
            timestamp: 0,
        };
        PROBE_ENROLLMENT.set(true);
        let _probe = ProbeGuard;
        first.bind_machine_key(&auth(41, "alpha-machine")).unwrap();
        second.bind_machine_key(&auth(42, "beta-machine")).unwrap();
        assert!(second.bind_machine_key(&auth(43, "alpha-machine")).is_err());
        let restarted = RelayState::new_with_key_store(vec![], &path).unwrap();
        assert_eq!(restarted.inner.machine_public_keys.lock().len(), 2);
        restarted
            .bind_machine_key(&auth(41, "alpha-machine"))
            .unwrap();
        restarted
            .bind_machine_key(&auth(42, "beta-machine"))
            .unwrap();
        assert!(restarted
            .bind_machine_key(&auth(43, "alpha-machine"))
            .is_err());
    }

    /// A machine that enrolled itself with its account must be able to open its control
    /// tunnel on an account-enabled private relay without an operator-issued static token.
    #[test]
    fn account_enrolled_machine_binds_without_a_static_enrollment_token() {
        let tmp = tempfile::tempdir().unwrap();
        // Private relay (tokens configured) that also serves an account service. The key
        // store is a temp path so the test never touches the operator's real store.
        let state =
            RelayState::new_with_key_store(vec!["operator-token".into()], tmp.path().join("keys.json"))
                .unwrap();
        let account = test_account_state(
            tmp.path(),
            "user-1",
            "session-token",
            "owned-machine",
            "other-machine",
        );
        // The account store pins machine "owned-machine" to public key "pubkey-1".
        state.set_account_state(account);

        let enrolled = ControlAuth {
            machine_id: "owned-machine".into(),
            public_key: "pubkey-1".into(),
            display_name: "Owned Machine".into(),
            enrollment_token: None,
            signature: String::new(),
            timestamp: 0,
        };
        state
            .bind_machine_key(&enrolled)
            .expect("an account-enrolled machine must bind without a static token");
    }

    /// The account alternative must not admit a machine the account store does not hold,
    /// and must not admit an enrolled machine id presented with a different key.
    #[test]
    fn account_enrollment_admission_rejects_unknown_machine_and_mismatched_key() {
        let tmp = tempfile::tempdir().unwrap();
        let state =
            RelayState::new_with_key_store(vec!["operator-token".into()], tmp.path().join("keys.json"))
                .unwrap();
        let account = test_account_state(
            tmp.path(),
            "user-1",
            "session-token",
            "owned-machine",
            "other-machine",
        );
        state.set_account_state(account);

        let stranger = ControlAuth {
            machine_id: "never-enrolled".into(),
            public_key: "pubkey-1".into(),
            display_name: "Stranger".into(),
            enrollment_token: None,
            signature: String::new(),
            timestamp: 0,
        };
        assert!(
            state.bind_machine_key(&stranger).is_err(),
            "a machine absent from the account store must not bind"
        );

        let impostor = ControlAuth {
            machine_id: "owned-machine".into(),
            public_key: "not-the-enrolled-key".into(),
            display_name: "Impostor".into(),
            enrollment_token: None,
            signature: String::new(),
            timestamp: 0,
        };
        assert!(
            state.bind_machine_key(&impostor).is_err(),
            "an enrolled machine id with a different key must not bind"
        );
    }

    /// Without an account service the private-relay token requirement is unchanged.
    #[test]
    fn private_relay_without_an_account_service_still_requires_an_enrollment_token() {
        let tmp = tempfile::tempdir().unwrap();
        let state =
            RelayState::new_with_key_store(vec!["operator-token".into()], tmp.path().join("keys.json"))
                .unwrap();
        let auth = ControlAuth {
            machine_id: "owned-machine".into(),
            public_key: "pubkey-1".into(),
            display_name: "Owned Machine".into(),
            enrollment_token: None,
            signature: String::new(),
            timestamp: 0,
        };
        assert!(
            state.bind_machine_key(&auth).is_err(),
            "a token-only private relay must still refuse a tokenless machine"
        );

        let with_token = ControlAuth {
            enrollment_token: Some("operator-token".into()),
            ..auth
        };
        state
            .bind_machine_key(&with_token)
            .expect("the operator token must still be accepted");
    }

    /// End to end: an account-enrolled daemon with NO operator token opens its control tunnel
    /// against an account-enabled private relay, because the relay already pins its machine id
    /// to the key it enrolled. This is the gap that made `ferryx account enroll` insufficient.
    #[tokio::test]
    async fn account_enrolled_daemon_opens_its_control_tunnel_without_an_operator_token() {
        use crate::remote::relay_client::RelayClient;
        let tmp = tempfile::tempdir().unwrap();
        let state =
            RelayState::new_with_key_store(vec!["operator-token".into()], tmp.path().join("keys.json"))
                .unwrap();
        let ident = identity(23, "owned-machine");
        let account = test_account_state(
            tmp.path(),
            "user-1",
            "session-token",
            "owned-machine",
            "other-machine",
        );
        // Pin the enrolled record to the key the daemon will actually present.
        account
            .mutate(|store| {
                store
                    .machines
                    .get_mut("rec-owned")
                    .expect("rec-owned fixture")
                    .public_key = ident.public_key.clone();
                Ok(())
            })
            .expect("pin enrolled public key");
        let (base, relay) = spawn_test_relay_with_account_state(state.clone(), Some(account)).await;

        // No `.with_enrollment_token(..)` on purpose: this is the account path.
        let client = RelayClient::with_identity(&base, ident, "127.0.0.1:1".to_string());
        let control = tokio::spawn(async move { client.run().await });

        let mut bound = false;
        for _ in 0..100 {
            if state.inner.machine_public_keys.lock().contains_key("owned-machine") {
                bound = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        control.abort();
        relay.abort();
        assert!(
            bound,
            "an account-enrolled daemon must open its control tunnel without an operator token"
        );
    }

    /// The same end-to-end path must NOT admit a daemon whose machine id is absent from the
    /// account store, even though it presents a well-formed key and no token.
    #[tokio::test]
    async fn unenrolled_daemon_cannot_open_a_control_tunnel_on_a_private_relay() {
        use crate::remote::relay_client::RelayClient;
        let tmp = tempfile::tempdir().unwrap();
        let state =
            RelayState::new_with_key_store(vec!["operator-token".into()], tmp.path().join("keys.json"))
                .unwrap();
        let account = test_account_state(
            tmp.path(),
            "user-1",
            "session-token",
            "owned-machine",
            "other-machine",
        );
        let (base, relay) = spawn_test_relay_with_account_state(state.clone(), Some(account)).await;

        let client = RelayClient::with_identity(
            &base,
            identity(29, "stranger-machine"),
            "127.0.0.1:1".to_string(),
        );
        let control = tokio::spawn(async move { client.run().await });
        tokio::time::sleep(Duration::from_millis(600)).await;
        let bound = state
            .inner
            .machine_public_keys
            .lock()
            .contains_key("stranger-machine");
        control.abort();
        relay.abort();
        assert!(
            !bound,
            "a machine absent from the account store must not open a control tunnel"
        );
    }

    #[test]
    fn machine_query_admission() {
        for query in [
            "ticket=t&daemonEpoch=01",
            "ticket=t&daemonEpoch=-1",
            "ticket=t&afterSequence=18446744073709551616",
            "ticket=t&daemonEpoch=1&daemonEpoch=2",
            "ticket=t&token=secret",
            "ticket=t&afterSequence=1.0",
        ] {
            assert!(
                axum::extract::Query::<super::TerminalSocketQuery>::try_from_uri(
                    &format!("http://localhost/?{query}").parse().unwrap()
                )
                .is_err(),
                "{query}"
            );
        }
        for query in [
            "path=a&path=b",
            "token=secret",
            "includeHidden=1",
            "path=%00",
            "path=%GG",
        ] {
            assert!(super::validate_http_query("fs/directories", Some(query)).is_err());
        }
        assert!(super::validate_http_query(
            "fs/directories",
            Some("path=%2Fa%252Fb&includeHidden=true")
        )
        .is_ok());
    }
    #[test]
    fn workspace_projects_admits_its_scope_query() {
        assert_eq!(validate_http_query("workspace/projects", Some("scope=desktopLocal")), Ok(()));
        assert_eq!(
            validate_http_query("workspace/projects", Some("other=1")),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn r4_strict_utf8_query() {
        assert!(validate_http_query("fs/directories", Some("path=%2Ftmp%2F%EF%BF%BD")).is_ok());
        for query in [
            "path=%FF",
            "path=%C0%AF",
            "path=%ED%A0%80",
            "path=%E2%82",
            "path=%GG",
        ] {
            assert_eq!(
                validate_http_query("fs/directories", Some(query)),
                Err(StatusCode::BAD_REQUEST)
            );
        }
    }

    #[test]
    fn test_allowed_http_route_attach() {
        // The axum route is /host/{machine_id}/api/v1/{*path}, so host_http_handler
        // strips the /host/{machine}/api/v1 prefix and passes only the wildcard
        // remainder to allowed_http_route. Live probe on omarchy proved that passing
        // "api/v1/attach" causes production 403 while passing "attach" matches.
        assert!(allowed_http_route(&Method::GET, "attach"));
        assert!(!allowed_http_route(&Method::POST, "attach"));
        assert!(!allowed_http_route(&Method::GET, "attach/../secret"));
        assert!(!allowed_http_route(&Method::GET, "api/v1/attach"));
        assert!(allowed_http_route(&Method::GET, "capabilities"));
    }

    #[test]
    fn test_allowed_http_route_admits_push_registration() {
        assert!(allowed_http_route(&Method::GET, "push/vapid-public-key"));
        for route in ["push/subscribe", "push/unsubscribe", "push/preferences", "push/test"] {
            assert!(allowed_http_route(&Method::POST, route), "{route}");
            assert!(!allowed_http_route(&Method::GET, route), "{route} is POST only");
        }
        assert!(!allowed_http_route(&Method::POST, "push/vapid-public-key"));
    }

    #[test]
    fn test_allowed_http_route_admits_resource_reads_only() {
        assert!(allowed_http_route(&Method::GET, "resources"));
        assert!(!allowed_http_route(&Method::POST, "resources"));
        assert!(!allowed_http_route(&Method::GET, "resources/extra"));
    }

    #[test]
    fn test_allowed_http_route_admits_paired_file_read_and_paste_upload() {
        // Desktop paired clients reach these machine routes through the relay when no
        // direct adapter is up; a refusal here surfaces as a bodyless 403.
        assert!(allowed_http_route(&Method::POST, "workspace/file-read"));
        assert!(allowed_http_route(&Method::POST, "workspace/paste-upload"));
        assert!(!allowed_http_route(&Method::GET, "workspace/file-read"));
        assert!(!allowed_http_route(&Method::DELETE, "workspace/paste-upload"));
        assert!(!allowed_http_route(&Method::POST, "workspace/file-read/extra"));
        assert!(!allowed_http_route(&Method::POST, "workspace/../file-read"));
    }

    #[test]
    fn test_allowed_http_route_admits_workspace_sidebar_read_only() {
        assert!(allowed_http_route(&Method::GET, "workspace/sidebar"));
        assert!(validate_http_query("workspace/sidebar", None).is_ok());
        for method in [Method::POST, Method::DELETE, Method::PUT] {
            assert!(
                !allowed_http_route(&method, "workspace/sidebar"),
                "{method} stays refused"
            );
        }
        assert!(!allowed_http_route(&Method::GET, "workspace/sidebar/extra"));
        assert!(!allowed_http_route(&Method::GET, "workspace/../sidebar"));
    }

    #[test]
    fn paste_upload_chunk_fits_the_forwarded_body_cap() {
        // host_http_handler bounds non-reference-chat bodies at 64 KiB.
        let request = crate::remote::machine_protocol::PasteUploadChunkRequest {
            request_id: "3941b9de-b16d-4d9a-ae0a-118f90fd91f4".into(),
            upload_id: "3941b9de-b16d-4d9a-ae0a-118f90fd91f5".into(),
            file_name: "x".repeat(255),
            chunk_index: u32::MAX,
            total_chunks: u32::MAX,
            data: base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                vec![0u8; crate::paired_host::upload::UPLOAD_CHUNK_BYTES],
            ),
            offset: Some(u64::MAX),
            total_bytes: Some(u64::MAX),
        };
        assert!(serde_json::to_vec(&request).unwrap().len() < 64 * 1024);
    }

    #[tokio::test]
    async fn r4_preference_route_not_intercepted_by_websocket() {
        let (base, server) = spawn_test_relay().await;
        let outcome = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
            let response = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .get(format!(
                    "{}/host/offline/api/v1/terminal/preferences",
                    base.replace("ws://", "http://")
                ))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }))
        .await;
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    #[tokio::test]
    async fn r4_browser_discovery_routes_not_intercepted_by_websocket() {
        let (base, server) = spawn_test_relay().await;
        let outcome = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
            let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap();
            let http_base = base.replace("ws://", "http://");

            // GET /host/offline/api/v1/browser/sessions must reach host_http_handler and return NOT_FOUND (offline machine)
            // rather than hitting browser_screencast_handler (which would return BAD_REQUEST or UNAUTHORIZED)
            let resp_sessions = client
                .get(format!("{http_base}/host/offline/api/v1/browser/sessions?workspaceId=ws1&worktreeSlug=main"))
                .send().await.unwrap();
            assert_eq!(resp_sessions.status(), StatusCode::NOT_FOUND);

            // GET /host/offline/api/v1/browser/identify must also reach host_http_handler
            let resp_identify = client
                .get(format!("{http_base}/host/offline/api/v1/browser/identify?workspaceId=ws1&worktreeSlug=main"))
                .send().await.unwrap();
            assert_eq!(resp_identify.status(), StatusCode::NOT_FOUND);
        })).await;
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }
    #[test]
    fn r4_sustained_admission_and_cancel_cleanup() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.block_on(async {
            let state = test_state(vec![]);
            let (base, server) = spawn_test_relay_with_state(state.clone()).await;
            let (generation, mut notices, _grants) = state.register_control_channel("load".into());
            // Exhaust the proxy IP's unauthenticated budget before redeeming
            // valid sessions. Invalid guesses stay limited; issued sessions work.
            for _ in 0..30 {
                assert_socket_rejected(&base, "/tunnel/data/missing", StatusCode::NOT_FOUND).await;
            }
            assert_socket_rejected(&base, "/tunnel/data/missing", StatusCode::TOO_MANY_REQUESTS).await;
            for window in 0..2 {
                for request in 0..30 {
                    let http = proxy_http(&state, "load", Some(generation), Method::GET,
                        "/api/v1/fs/directories?path=%2Ftmp", HeaderMap::new(), Vec::new());
                    let peer = async {
                        let notice = notices.recv().await.unwrap();
                        let (mut data, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{}", notice.session_id)).await.unwrap();
                        let bytes = data.next().await.unwrap().unwrap().into_data();
                        assert!(bytes.starts_with(b"GET /api/v1/fs/directories?path=%2Ftmp HTTP/1.1\r\n"));
                        data.send(TMessage::Binary(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}".to_vec().into())).await.unwrap();
                    };
                    let (response, ()) = timeout(Duration::from_secs(5), async { tokio::join!(http, peer) }).await.unwrap();
                    assert_eq!(response.unwrap().status(), StatusCode::OK, "window={window} request={request}");
                    assert!(state.inner.pending_sessions.lock().is_empty());
                }
                assert!(state.inner.pending_sessions.lock().is_empty());
            }
            let mut waiting = Vec::new();
            for _ in 0..MAX_PENDING_SESSIONS_PER_MACHINE {
                let mut request = Box::pin(state.open_session_channel("load", Some(generation)));
                tokio::select! {
                    result = &mut request => panic!("unexpected completion: {:?}", result.err()),
                    notice = timeout(Duration::from_secs(5), notices.recv()) => { notice.unwrap().unwrap(); }
                }
                waiting.push(request);
            }
            assert_eq!(state.open_session_channel("load", Some(generation)).await.err(), Some(StatusCode::SERVICE_UNAVAILABLE));
            drop(waiting);
            assert!(state.inner.pending_sessions.lock().is_empty());
            state.unregister_control_channel("load", generation);
            server.abort();
            assert!(server.await.unwrap_err().is_cancelled());
            println!("R4 admission: 60 issued data-channel requests bypass exhausted guess budget; invalid guesses/429; per-machine waiting cap/503; cancellation registry=0");
        })
        }));
        drop(runtime);
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    type TestDataSocket = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    /// Reads one proxied request off a daemon-side data socket and answers it.
    async fn answer_one(data: &mut TestDataSocket, expect_prefix: &[u8]) {
        let bytes = timeout(Duration::from_secs(5), data.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .into_data();
        assert!(bytes.starts_with(expect_prefix), "{:?}", String::from_utf8_lossy(&bytes));
        assert!(
            !String::from_utf8_lossy(&bytes).to_ascii_lowercase().contains("connection: close"),
            "relay must request keep-alive"
        );
        data.send(TMessage::Binary(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}".to_vec().into(),
        ))
        .await
        .unwrap();
    }

    async fn get_via_relay(state: &RelayState, machine: &str, generation: u64) -> StatusCode {
        proxy_http(state, machine, Some(generation), Method::GET, "/api/v1/capabilities",
            HeaderMap::new(), Vec::new()).await.unwrap().status()
    }

    // rs-input-screen-r22-1
    #[tokio::test]
    async fn a_forwarded_reference_chat_submit_whose_answer_is_lost_reports_outcome_unknown() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (generation, mut notices, _grants) =
            state.register_control_channel("submit-host".into());
        let path = "/api/v1/reference-chat/sess-1/submit?hostId=h";
        let http = proxy_http(&state, "submit-host", Some(generation), Method::POST, path,
            HeaderMap::new(), br#"{"requestId":"req-1","text":"hi"}"#.to_vec());
        let peer = async {
            let notice = notices.recv().await.unwrap();
            let (mut data, _) = tokio_tungstenite::connect_async(format!(
                "{base}/tunnel/data/{}",
                notice.session_id
            ))
            .await
            .unwrap();
            let request = data.next().await.unwrap().unwrap().into_data();
            assert!(request.starts_with(b"POST /api/v1/reference-chat/sess-1/submit"));
            // The host received the submit, then the channel drops before any answer.
            data.close(None).await.unwrap();
        };
        let (response, ()) =
            timeout(Duration::from_secs(10), async { tokio::join!(http, peer) }).await.unwrap();
        let response = response.expect("a lost answer to a forwarded submit is not a bare 502");
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body: serde_json::Value = serde_json::from_slice(
            &to_bytes(response.into_body(), 4096).await.unwrap(),
        )
        .unwrap();
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], "OPERATION_OUTCOME_UNKNOWN");
        assert_eq!(body["error"]["retryable"], false);
        assert_eq!(body["requestId"], "req-1");
        state.unregister_control_channel("submit-host", generation);
        server.abort();
    }

    #[tokio::test]
    async fn a_forwarded_agent_control_send_whose_answer_is_lost_reports_outcome_unknown() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (generation, mut notices, _grants) =
            state.register_control_channel("agent-host".into());
        let path = "/api/v1/agent-control/sessions/s-1/send";
        let http = proxy_http(
            &state,
            "agent-host",
            Some(generation),
            Method::POST,
            path,
            HeaderMap::new(),
            br#"{"text":"hi","requestId":"agent-req-1"}"#.to_vec(),
        );
        let peer = async {
            let notice = notices.recv().await.unwrap();
            let (mut data, _) = tokio_tungstenite::connect_async(format!(
                "{base}/tunnel/data/{}",
                notice.session_id
            ))
            .await
            .unwrap();
            let request = data.next().await.unwrap().unwrap().into_data();
            assert!(request.starts_with(b"POST /api/v1/agent-control/sessions/s-1/send"));
            data.close(None).await.unwrap();
        };
        let (response, ()) = timeout(Duration::from_secs(10), async { tokio::join!(http, peer) })
            .await
            .unwrap();
        let response = response.expect("a lost agent-control send is not a bare 502");
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(body["error"]["code"], "OPERATION_OUTCOME_UNKNOWN");
        assert_eq!(body["error"]["retryable"], false);
        assert_eq!(body["requestId"], "agent-req-1");
        state.unregister_control_channel("agent-host", generation);
        server.abort();
    }

    #[test]
    fn agent_control_and_supervisor_routes_join_the_allowlist_without_loosening_it() {
        assert!(allowed_http_route(&Method::GET, "agent-control/sessions"));
        assert!(allowed_http_route(&Method::GET, "agent-control/events"));
        assert!(allowed_http_route(
            &Method::GET,
            "agent-control/sessions/s-1"
        ));
        assert!(allowed_http_route(
            &Method::GET,
            "agent-control/sessions/s-1/wait"
        ));
        assert!(allowed_http_route(
            &Method::POST,
            "agent-control/sessions/s-1/send"
        ));
        assert!(allowed_http_route(
            &Method::POST,
            "agent-control/sessions/s-1/answer"
        ));
        assert!(allowed_http_route(
            &Method::POST,
            "agent-control/sessions/s-1/stop"
        ));
        assert!(allowed_http_route(&Method::GET, "supervisor"));
        assert!(allowed_http_route(&Method::POST, "supervisor/pause"));
        assert!(allowed_http_route(&Method::POST, "supervisor/start"));
        assert!(allowed_http_route(&Method::POST, "supervisor/request"));
        assert!(allowed_http_route(&Method::GET, "supervisor/conversation"));
        assert!(allowed_http_route(&Method::GET, "supervisor/memory"));
        assert!(allowed_http_route(&Method::POST, "supervisor/memory"));
        assert!(
            validate_http_query("supervisor/conversation", Some("projectId=p&after=3")).is_ok()
        );
        assert!(validate_http_query("supervisor/conversation", Some("actor=x")).is_err());
        assert!(validate_http_query("supervisor/request", Some("actor=x")).is_err());
        for (method, path) in [
            (Method::GET, "reference-chat/session-1"),
            (Method::GET, "reference-chat/x/secret"),
            (Method::GET, "workspace/file-read"),
            (Method::POST, "admin"),
            (Method::DELETE, "agent-control/sessions/s-1"),
            (Method::GET, "agent-control/sessions/s-1/send"),
            (Method::DELETE, "supervisor"),
            (Method::POST, "supervisor"),
            (Method::GET, "supervisor/pause"),
            (Method::GET, "supervisor/start"),
            (Method::POST, "supervisor/x/y"),
            (Method::GET, "supervisor/x/y"),
            (Method::POST, "supervisor/lane-1/pause"),
            (Method::GET, "supervisor/request"),
            (Method::DELETE, "supervisor/memory"),
            (Method::POST, "supervisor/conversation"),
            (Method::POST, "supervisor/request/x"),
        ] {
            assert!(
                !allowed_http_route(&method, path),
                "{method} {path} stays refused"
            );
        }
        assert!(validate_http_query("agent-control/events", Some("after=3")).is_ok());
        assert!(validate_http_query("agent-control/events", Some("secret=1")).is_err());
        assert!(validate_http_query(
            "agent-control/sessions/s-1/wait",
            Some("until=idle&after=1&timeoutMs=1000&revision=rev")
        )
        .is_ok());
        assert!(!agent_control_mutation(
            &Method::GET,
            "/api/v1/agent-control/events"
        ));
        assert!(!agent_control_mutation(
            &Method::GET,
            "/api/v1/agent-control/sessions/s-1/wait?timeoutMs=1000"
        ));
        assert!(agent_control_mutation(
            &Method::POST,
            "/api/v1/agent-control/sessions/s-1/answer"
        ));
        assert!(reference_chat_mutation(
            &Method::POST,
            "/api/v1/reference-chat/sess-1/submit"
        ));
    }

    async fn reference_chat_partial_response_case(method: Method, suffix: &str, partial: &[u8]) {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (generation, mut notices, _grants) =
            state.register_control_channel("r23-partial-host".into());
        let path = format!("/api/v1/reference-chat/sess-1/{suffix}?hostId=h");
        let mutation = method == Method::POST;
        let expected_prefix = format!("{method} {path} HTTP/1.1\r\n");
        let http = proxy_http(&state, "r23-partial-host", Some(generation), method, &path,
            HeaderMap::new(), br#"{"requestId":"req-r23-partial","params":{}}"#.to_vec());
        let peer = async {
            let notice = notices.recv().await.unwrap();
            let (mut data, _) = tokio_tungstenite::connect_async(format!(
                "{base}/tunnel/data/{}", notice.session_id,
            )).await.unwrap();
            let request = data.next().await.unwrap().unwrap().into_data();
            assert!(request.starts_with(expected_prefix.as_bytes()));
            data.send(TMessage::Binary(partial.to_vec().into())).await.unwrap();
            data.close(None).await.unwrap();
        };
        let (response, ()) = timeout(Duration::from_secs(10), async {
            tokio::join!(http, peer)
        }).await.unwrap();
        if mutation {
            let response = response.expect("a truncated mutation response is typed uncertainty");
            assert_eq!(response.status(), StatusCode::CONFLICT);
            let body: serde_json::Value = serde_json::from_slice(
                &to_bytes(response.into_body(), 4096).await.unwrap(),
            ).unwrap();
            assert_eq!(body["ok"], false);
            assert_eq!(body["error"]["code"], "OPERATION_OUTCOME_UNKNOWN");
            assert_eq!(body["error"]["retryable"], false);
            assert_eq!(body["requestId"], "req-r23-partial");
        } else {
            assert_eq!(response.err(), Some(StatusCode::BAD_GATEWAY));
        }
        assert!(notices.try_recv().is_err(), "a partially answered request is never replayed");
        assert!(state.inner.pending_sessions.lock().is_empty());
        state.unregister_control_channel("r23-partial-host", generation);
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn r23_truncated_reference_chat_mutations_report_outcome_unknown() {
        for suffix in ["submit", "stop", "answer"] {
            for partial in [
                b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n".as_slice(),
                b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{\"ok\":true".as_slice(),
            ] {
                reference_chat_partial_response_case(Method::POST, suffix, partial).await;
            }
        }
    }

    #[tokio::test]
    async fn r23_truncated_reference_chat_read_remains_bad_gateway() {
        reference_chat_partial_response_case(Method::GET, "screen",
            b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{\"text\":",
        ).await;
    }

    /// rs-files-routes-8: a staged-file preview is base64 of up to ATTACHMENT_MAX_FILE_BYTES,
    /// so the relay must return a reference-chat response well past the generic 8 MiB bound.
    #[tokio::test]
    async fn a_reference_chat_file_preview_larger_than_the_generic_cap_is_relayed() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (generation, mut notices, _grants) =
            state.register_control_channel("preview-host".into());
        let body = vec![b'A'; 11 * 1024 * 1024];
        let path = "/api/v1/reference-chat/sess-1/files/att-1?hostId=h";
        let http = proxy_http(&state, "preview-host", Some(generation), Method::GET, path,
            HeaderMap::new(), Vec::new());
        let peer = async {
            let notice = notices.recv().await.unwrap();
            let (mut data, _) = tokio_tungstenite::connect_async(format!(
                "{base}/tunnel/data/{}",
                notice.session_id
            ))
            .await
            .unwrap();
            let request = data.next().await.unwrap().unwrap().into_data();
            assert!(request.starts_with(b"GET /api/v1/reference-chat/sess-1/files/att-1"));
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            // The relay may hang up mid-body when it refuses the response; that refusal is what
            // the status assertion below reports, so a send error here only ends the upload.
            let _ = data.send(TMessage::Binary(head.into_bytes().into())).await;
            for chunk in body.chunks(MAX_MESSAGE_SIZE) {
                if data.send(TMessage::Binary(chunk.to_vec().into())).await.is_err() {
                    break;
                }
            }
            data
        };
        let (response, _data) =
            timeout(Duration::from_secs(30), async { tokio::join!(http, peer) }).await.unwrap();
        let status = response.map(|response| response.status());
        assert_eq!(
            status,
            Ok(StatusCode::OK),
            "an 11 MB reference-chat preview must be relayed, not refused"
        );
        state.unregister_control_channel("preview-host", generation);
        server.abort();
    }


    #[tokio::test]
    async fn data_channel_is_reused_across_http_requests() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (generation, mut notices, _grants) = state.register_control_channel("pool".into());
        let first = async {
            let notice = notices.recv().await.unwrap();
            let (mut data, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{}", notice.session_id)).await.unwrap();
            answer_one(&mut data, b"GET /api/v1/capabilities HTTP/1.1\r\n").await;
            data
        };
        let (status, mut data) = tokio::join!(get_via_relay(&state, "pool", generation), first);
        assert_eq!(status, StatusCode::OK);
        for _ in 0..3 {
            let (status, ()) = tokio::join!(
                get_via_relay(&state, "pool", generation),
                answer_one(&mut data, b"GET /api/v1/capabilities HTTP/1.1\r\n")
            );
            assert_eq!(status, StatusCode::OK);
        }
        assert!(notices.try_recv().is_err(), "reused requests must not allocate new data channels");
        assert!(state.inner.pending_sessions.lock().is_empty());
        state.unregister_control_channel("pool", generation);
        assert!(state.inner.data_pool.lock().get("pool").is_none_or(Vec::is_empty));
        server.abort();
    }

    #[tokio::test]
    async fn dead_pooled_channel_falls_back_and_mutations_never_reuse() {
        async fn fresh(
            base: &str,
            notices: &mut mpsc::Receiver<IncomingSessionNotice>,
            prefix: &[u8],
        ) -> TestDataSocket {
            let notice = notices.recv().await.unwrap();
            let (mut data, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{}", notice.session_id)).await.unwrap();
            answer_one(&mut data, prefix).await;
            data
        }
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (generation, mut notices, _grants) = state.register_control_channel("pool".into());
        let (status, mut data) = tokio::join!(
            get_via_relay(&state, "pool", generation),
            fresh(&base, &mut notices, b"GET /api/v1/capabilities HTTP/1.1\r\n")
        );
        assert_eq!(status, StatusCode::OK);
        // The daemon side of the pooled channel goes away while idle.
        data.close(None).await.unwrap();
        let (status, _replacement) = tokio::join!(
            get_via_relay(&state, "pool", generation),
            fresh(&base, &mut notices, b"GET /api/v1/capabilities HTTP/1.1\r\n")
        );
        assert_eq!(status, StatusCode::OK, "a dead idle channel must be replaced transparently");
        assert_eq!(state.inner.data_pool.lock()["pool"].len(), 1);
        // A POST must never be sent on an idle channel it might have to replay.
        let post = proxy_http(&state, "pool", Some(generation), Method::POST, "/api/v1/sessions",
            HeaderMap::new(), b"{}".to_vec());
        let (response, _second) = tokio::join!(post, fresh(&base, &mut notices, b"POST /api/v1/sessions HTTP/1.1\r\n"));
        assert_eq!(response.unwrap().status(), StatusCode::OK);
        assert_eq!(state.inner.data_pool.lock()["pool"].len(), 2);
        // A reconnected control generation must not inherit the old channels.
        let (_newer, _rx, _g) = state.register_control_channel("pool".into());
        assert!(state.take_pooled_data("pool", None).is_none());
        server.abort();
    }

    #[test]
    fn chunked_transfer_coding_is_matched_case_insensitively() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: Chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n";
        let parsed = parse_http_response(raw, false, false).unwrap().unwrap();
        assert_eq!(parsed.status(), StatusCode::OK);
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding:  CHUNKED \r\n\r\n0\r\n\r\n";
        assert!(parse_http_response(raw, false, false).unwrap().is_some());
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n0\r\n\r\n";
        assert_eq!(parse_http_response(raw, false, false).err(), Some(StatusCode::BAD_GATEWAY));
    }

    #[test]
    fn informational_responses_are_skipped_but_bounded() {
        let final_response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        let mut one = b"HTTP/1.1 100 Continue\r\n\r\n".to_vec();
        one.extend_from_slice(final_response);
        let parsed = parse_http_response(&one, false, false).unwrap().unwrap();
        assert_eq!(parsed.status(), StatusCode::OK);
        let mut many = b"HTTP/1.1 100 Continue\r\n\r\n".repeat(50);
        many.extend_from_slice(final_response);
        assert_eq!(
            parse_http_response(&many, false, false).err(),
            Some(StatusCode::BAD_GATEWAY)
        );
    }

    #[tokio::test]
    async fn a_silent_pooled_channel_falls_through_to_a_fresh_one() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (generation, mut notices, _grants) = state.register_control_channel("pool".into());
        let first = async {
            let notice = notices.recv().await.unwrap();
            let (mut data, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{}", notice.session_id)).await.unwrap();
            answer_one(&mut data, b"GET /api/v1/capabilities HTTP/1.1\r\n").await;
            data
        };
        let (status, mut silent) = tokio::join!(get_via_relay(&state, "pool", generation), first);
        assert_eq!(status, StatusCode::OK);
        // The pooled channel stays open but never answers (a NAT dropped it): the request
        // reaches it, then the relay must give up quickly and open a fresh channel.
        let request = get_via_relay(&state, "pool", generation);
        tokio::pin!(request);
        let seen = tokio::select! {
            status = &mut request => panic!("answered before reaching the pooled channel: {status}"),
            frame = silent.next() => frame.unwrap().unwrap(),
        };
        assert!(seen.into_data().starts_with(b"GET /api/v1/capabilities HTTP/1.1\r\n"));
        tokio::time::pause();
        let notice = tokio::select! {
            status = &mut request => panic!("no fresh channel was opened: {status}"),
            notice = notices.recv() => notice.unwrap(),
        };
        tokio::time::resume();
        let fresh = async {
            let (mut data, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{}", notice.session_id)).await.unwrap();
            answer_one(&mut data, b"GET /api/v1/capabilities HTTP/1.1\r\n").await;
            data
        };
        let (status, _fresh) = tokio::join!(request, fresh);
        assert_eq!(status, StatusCode::OK);
        drop(silent);
        server.abort();
    }

    #[test]
    fn only_exactly_framed_keep_alive_responses_are_reusable() {
        assert!(response_is_reusable(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}", false));
        assert!(response_is_reusable(b"HTTP/1.1 204 No Content\r\n\r\n", false));
        assert!(response_is_reusable(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n", true));
        assert!(!response_is_reusable(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 2\r\n\r\n{}", false));
        assert!(!response_is_reusable(b"HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\n{}", false));
        assert!(!response_is_reusable(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n", false));
        assert!(!response_is_reusable(b"HTTP/1.1 200 OK\r\n\r\nbody", false));
        assert!(!response_is_reusable(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}extra", false));
    }

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

    async fn spawn_test_relay_with_state(
        state: RelayState,
    ) -> (String, tokio::task::JoinHandle<()>) {
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
        state.register_device_token("browser-machine", "test-device-token");
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer test-device-token".parse().unwrap());
        let response = socket_ticket_handler(
            State(state.clone()),
            AxumPath("browser-machine".into()),
            None,
            headers,
            Json(serde_json::json!({"target": target})),
        )
        .await
        .unwrap();
        serde_json::from_slice(
            &to_bytes(response.into_body(), MAX_MESSAGE_SIZE)
                .await
                .unwrap(),
        )
        .unwrap()
    }

    async fn ticket_status(base: &str, machine: &str, token: &str) -> StatusCode {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .post(format!(
                "{}/host/{machine}/api/v1/socket-ticket",
                base.replace("ws://", "http://")
            ))
            .bearer_auth(token)
            .header("content-type", "application/json")
            .body(r#"{"target":"/api/v1/events"}"#)
            .send()
            .await
            .unwrap()
            .status()
    }

    async fn answer_token_authority(
        control: &mut TestSocket,
        base: &str,
        token: &str,
        status: StatusCode,
    ) {
        let notice: IncomingSessionNotice = receive_json(control).await;
        let (mut data, _) =
            tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{}", notice.session_id))
                .await
                .unwrap();
        let frame = timeout(Duration::from_secs(5), data.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let raw = String::from_utf8(frame.into_data().to_vec()).unwrap();
        assert!(raw.starts_with("GET /api/v1/devices HTTP/1.1\r\n"));
        assert!(raw.to_ascii_lowercase().contains(&format!(
            "authorization: bearer {}\r\n",
            token.to_ascii_lowercase()
        )));
        data.send(TMessage::Binary(
            format!(
                "HTTP/1.1 {} {}\r\nContent-Length: 0\r\n\r\n",
                status.as_u16(),
                status.canonical_reason().unwrap()
            )
            .into_bytes()
            .into(),
        ))
        .await
        .unwrap();
        // The relay keeps a keep-alive data channel open for reuse, so the test
        // daemon hangs up here instead of waiting for the relay to close it.
        data.close(None).await.unwrap();
    }

    fn ticket_query(
        ticket: &str,
    ) -> Result<Query<SocketQuery>, axum::extract::rejection::QueryRejection> {
        Ok(Query(SocketQuery {
            ticket: Some(ticket.into()),
        }))
    }

    async fn register_security_pin(socket: &mut TestSocket, machine: &str) {
        let registration = security_registration(machine);
        socket
            .send(TMessage::Text(
                serde_json::to_string(&registration).unwrap().into(),
            ))
            .await
            .unwrap();
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
            expires_at: current_time_secs() + 300,
        }
    }

    async fn security_claim(state: &RelayState, pin: &str) -> StatusCode {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/pair/exchange")
            .body(Body::from(
                serde_json::json!({"pin": pin, "deviceName": "security-client"}).to_string(),
            ))
            .unwrap();
        // Supply the accepted peer address, not an untrusted forwarding header.
        let peer = Some(axum::Extension(ConnectInfo(
            "192.0.2.1:12345".parse().unwrap(),
        )));
        match timeout(
            Duration::from_secs(5),
            pair_exchange_handler(State(state.clone()), peer, request),
        )
        .await
        .unwrap()
        {
            Ok(response) => response.status(),
            Err(status) => status,
        }
    }

    async fn assert_socket_rejected(base: &str, path: &str, status: StatusCode) {
        let error = timeout(
            Duration::from_secs(5),
            tokio_tungstenite::connect_async(format!("{base}{path}")),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(
            matches!(error, tokio_tungstenite::tungstenite::Error::Http(response) if response.status() == status)
        );
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
        let (mut attacker, rejection) =
            authenticate(&base, &identity(32, "machine_alpha"), false, false).await;
        assert!(!rejection.success);
        // Authentication happens after HTTP 101, so rejection is a protocol error, not HTTP 401.
        assert!(rejection
            .error
            .unwrap()
            .contains("Machine ID already claimed"));
        assert!(matches!(
            timeout(Duration::from_secs(5), attacker.next())
                .await
                .unwrap(),
            Some(Ok(TMessage::Close(_)))
        ));
        assert_eq!(
            state.inner.machine_public_keys.lock()["machine_alpha"],
            owner.public_key
        );
        assert_eq!(
            state.inner.control_channels.lock()["machine_alpha"].generation,
            generation
        );
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Ready
        );
        let notice = allocate(&mut original).await;
        assert!(state
            .inner
            .pending_sessions
            .lock()
            .contains_key(&notice.session_id));
        server.abort();
    }

    async fn token_control(base: &str, machine_id: &str) -> TestSocket {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = format!("{base}/tunnel/control").into_client_request().unwrap();
        request
            .headers_mut()
            .insert("authorization", "Bearer test-machine-token".parse().unwrap());
        request
            .headers_mut()
            .insert("x-ferryx-machine-id", machine_id.parse().unwrap());
        tokio_tungstenite::connect_async(request).await.unwrap().0
    }

    /// Waits until the relay has processed every frame sent before this point: a Ping's Pong
    /// is written by the same control loop, in order.
    async fn control_barrier(socket: &mut TestSocket) {
        socket.send(TMessage::Ping(b"barrier".to_vec().into())).await.unwrap();
        loop {
            match timeout(Duration::from_secs(5), socket.next()).await.unwrap() {
                Some(Ok(TMessage::Pong(payload))) if payload.as_ref() == b"barrier" => return,
                Some(Ok(_)) => continue,
                other => panic!("control socket ended before the barrier: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn a_second_token_daemon_cannot_steal_a_live_alias() {
        let state = test_state(vec!["test-machine-token".into()]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let mut first = token_control(&base, "machine-a").await;
        control_barrier(&mut first).await;
        let first_generation = state.inner.control_channels.lock()["machine-a"].generation;
        // A second daemon sharing the operator token announces the same machine id, then
        // registers a PIN under it.
        let mut second = token_control(&base, "machine-a").await;
        control_barrier(&mut second).await;
        let mut registration = security_registration("machine-a");
        registration.pin = "654321".into();
        second
            .send(TMessage::Text(serde_json::to_string(&registration).unwrap().into()))
            .await
            .unwrap();
        let ack: RegisterPairingPinAck = receive_json(&mut second).await;
        assert_ne!(ack.status, "ready");
        assert_eq!(
            state.inner.control_channels.lock()["machine-a"].generation,
            first_generation,
            "the alias must stay routed to the daemon that holds it"
        );
        assert!(!state.inner.pairings.lock().contains_key("654321"));
        drop(first);
        server.abort();
    }

    #[tokio::test]
    async fn an_ed25519_daemon_cannot_register_a_pin_for_another_machine() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (_victim, auth) = authenticate(&base, &identity(34, "machine_victim"), false, false).await;
        assert!(auth.success);
        let victim_generation = state.inner.control_channels.lock()["machine_victim"].generation;
        let (mut attacker, auth) =
            authenticate(&base, &identity(35, "machine_attacker"), false, false).await;
        assert!(auth.success);
        attacker
            .send(TMessage::Text(
                serde_json::to_string(&security_registration("machine_victim")).unwrap().into(),
            ))
            .await
            .unwrap();
        let ack: RegisterPairingPinAck = receive_json(&mut attacker).await;
        assert_eq!(ack.status, "Machine ID does not match this control identity");
        assert_eq!(
            state.inner.control_channels.lock()["machine_victim"].generation,
            victim_generation
        );
        assert!(state.inner.pairings.lock().is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn a_busy_allocation_is_answered_without_dropping_the_control_socket() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (mut daemon, auth) = authenticate(&base, &identity(36, "busy_machine"), false, false).await;
        assert!(auth.success);
        for _ in 0..MAX_PENDING_SESSIONS_PER_MACHINE {
            allocate(&mut daemon).await;
        }
        daemon
            .send(TMessage::Text(r#"{"type":"AllocateSession"}"#.into()))
            .await
            .unwrap();
        let rejected: serde_json::Value = receive_json(&mut daemon).await;
        assert_eq!(rejected["type"], "AllocateSessionRejected");
        assert_eq!(rejected["reason"], "busy");
        // The control socket survives and keeps its registration.
        control_barrier(&mut daemon).await;
        assert!(state.inner.control_channels.lock().contains_key("busy_machine"));
        server.abort();
    }

    fn grant_envelope() -> crate::remote::account_protocol::AccountGrantOfferEnvelope {
        crate::remote::account_protocol::AccountGrantOfferEnvelope {
            machine_id: "grant_machine".into(),
            enrollment_epoch: "1".into(),
            sealed: "c2VhbGVk".into(),
        }
    }

    #[tokio::test]
    async fn concurrent_grant_deliveries_each_get_their_own_ack() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (mut daemon, auth) = authenticate(&base, &identity(37, "grant_machine"), false, false).await;
        assert!(auth.success);
        let first_state = state.clone();
        let first = tokio::spawn(async move {
            first_state.deliver_grant_offer("grant_machine", grant_envelope()).await
        });
        let delivery: AccountGrantOfferDelivery = receive_json(&mut daemon).await;
        assert_eq!(delivery.machine_id, "grant_machine");
        // A second grant is queued while the first is unacknowledged; it must not be sent
        // yet, so the daemon's first ack cannot be mistaken for it.
        let second_state = state.clone();
        let second = tokio::spawn(async move {
            second_state.deliver_grant_offer("grant_machine", grant_envelope()).await
        });
        let ack = |id: &str| {
            TMessage::Text(
                serde_json::to_string(&AccountGrantOfferDelivered {
                    grant_id: Some(id.into()),
                    status: "ready".into(),
                })
                .unwrap()
                .into(),
            )
        };
        daemon.send(ack("grant-1")).await.unwrap();
        assert_eq!(first.await.unwrap().unwrap().grant_id.as_deref(), Some("grant-1"));
        let _: AccountGrantOfferDelivery = receive_json(&mut daemon).await;
        daemon.send(ack("grant-2")).await.unwrap();
        assert_eq!(second.await.unwrap().unwrap().grant_id.as_deref(), Some("grant-2"));
        server.abort();
    }

    #[tokio::test]
    async fn an_abandoned_grant_ack_slot_does_not_capture_the_next_delivery() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (mut daemon, auth) = authenticate(&base, &identity(38, "grant_machine"), false, false).await;
        assert!(auth.success);
        let first_state = state.clone();
        let first = tokio::spawn(async move {
            first_state.deliver_grant_offer("grant_machine", grant_envelope()).await
        });
        let _: AccountGrantOfferDelivery = receive_json(&mut daemon).await;
        // The first caller gives up (as its 10 s timeout would) without any ack.
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        let second_state = state.clone();
        let second = tokio::spawn(async move {
            second_state.deliver_grant_offer("grant_machine", grant_envelope()).await
        });
        // The second delivery is forwarded once the abandoned slot is released.
        let _: AccountGrantOfferDelivery = receive_json(&mut daemon).await;
        daemon
            .send(TMessage::Text(
                serde_json::to_string(&AccountGrantOfferDelivered {
                    grant_id: Some("grant-2".into()),
                    status: "ready".into(),
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        assert_eq!(second.await.unwrap().unwrap().grant_id.as_deref(), Some("grant-2"));
        server.abort();
    }

    #[tokio::test]
    async fn test_security_fleet_pin_brute_force_lockout() {
        let state = test_state(vec![]);
        let (generation, mut notices, _grants) = state.register_control_channel("machine_alpha".into());
        assert!(state
            .register_pairing(
                "machine_alpha",
                generation,
                security_registration("machine_alpha")
            )
            .is_ok());
        for pin in ["000001", "000002", "000003", "000004", "000005"] {
            assert_eq!(security_claim(&state, pin).await, StatusCode::NOT_FOUND);
        }
        // A locked-out source is told it is rate limited, not that its code was wrong.
        assert_eq!(
            security_claim(&state, "123456").await,
            StatusCode::TOO_MANY_REQUESTS
        );
        let admission = state.inner.pairing_admission.lock();
        let tracker = &admission[&"192.0.2.1".parse::<IpAddr>().unwrap()];
        assert_eq!(tracker.failures, 5);
        assert!(tracker.locked_until.is_some());
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Ready
        );
        assert!(state.inner.pending_sessions.lock().is_empty());
        assert!(matches!(
            notices.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn test_security_expired_pin_claim_rejected() {
        let state = test_state(vec![]);
        let (generation, mut notices, _grants) = state.register_control_channel("machine_alpha".into());
        let mut expired = security_registration("machine_alpha");
        expired.expires_at = current_time_secs() - 10;
        assert!(state
            .register_pairing("machine_alpha", generation, expired)
            .is_err());
        assert_eq!(
            security_claim(&state, "123456").await,
            StatusCode::NOT_FOUND
        );
        assert!(state
            .register_pairing(
                "machine_alpha",
                generation,
                security_registration("machine_alpha")
            )
            .is_ok());
        // Exercise claim-time expiration as well as registration-time rejection, without waiting.
        state
            .inner
            .pairings
            .lock()
            .get_mut("123456")
            .unwrap()
            .registration
            .expires_at = current_time_secs() - 10;
        assert_eq!(
            security_claim(&state, "123456").await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Ready
        );
        assert!(state.inner.pending_sessions.lock().is_empty());
        assert!(matches!(
            notices.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn test_security_role_swap_prevention() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (mut daemon, auth) =
            authenticate(&base, &identity(33, "machine_alpha"), false, false).await;
        assert!(auth.success);
        register_security_pin(&mut daemon, "machine_alpha").await;
        for path in [
            "/host/machine_alpha/api/v1/events",
            "/host/machine_alpha/api/v1/terminal/t1",
        ] {
            assert_socket_rejected(&base, path, StatusCode::UNAUTHORIZED).await;
        }
        let (mut client, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/control"))
            .await
            .unwrap();
        let _: ControlChallenge = receive_json(&mut client).await;
        // Trigger explicit rejection, rather than waiting for authentication timeout.
        client
            .send(TMessage::Text(
                r#"{"machineId":"public-client","publicKey":"","timestamp":0}"#.into(),
            ))
            .await
            .unwrap();
        let rejection: ControlAuthResponse = receive_json(&mut client).await;
        assert!(!rejection.success);
        assert!(rejection.error.is_some());
        assert!(matches!(
            timeout(Duration::from_secs(5), client.next())
                .await
                .unwrap(),
            Some(Ok(TMessage::Close(_)))
        ));
        assert_socket_rejected(
            &base,
            "/tunnel/data/unissued-session",
            StatusCode::NOT_FOUND,
        )
        .await;
        assert_eq!(state.inner.control_channels.lock().len(), 1);
        assert!(!state
            .inner
            .machine_public_keys
            .lock()
            .contains_key("public-client"));
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
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let request = client
            .post(format!(
                "{}/api/v1/pair/exchange",
                base.replace("ws://", "http://")
            ))
            .body(r#"{"pin":"123456","deviceName":"isolated-client"}"#)
            .send();
        let daemon = async {
            let notice: IncomingSessionNotice = receive_json(&mut daemon_1).await;
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
            assert_eq!(body["code"], "security-pairing-secret");
            assert_eq!(body["deviceName"], "isolated-client");
            data.send(TMessage::Binary(
                b"HTTP/1.1 200 OK\r\nContent-Length: 29\r\n\r\n{\"token\":\"test-device-token\"}"
                    .to_vec()
                    .into(),
            ))
            .await
            .unwrap();
            data.close(None).await.unwrap();
        };
        let (response, ()) = timeout(Duration::from_secs(5), async {
            tokio::join!(request, daemon)
        })
        .await
        .unwrap();
        let response = response.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.text().await.unwrap(),
            r#"{"token":"test-device-token"}"#
        );
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Consumed
        );
        for target in ["/api/v1/events", "/api/v1/terminal/t1"] {
            let response = client
                .post(format!(
                    "{}/host/daemon_1/api/v1/socket-ticket",
                    base.replace("ws://", "http://")
                ))
                .header("authorization", "Bearer test-device-token")
                .header("content-type", "application/json")
                .body(serde_json::json!({"target": target}).to_string())
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let ticket: SocketTicketResponse =
                serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
            assert_socket_rejected(
                &base,
                &format!("/host/daemon_2{target}?ticket={}", ticket.ticket),
                StatusCode::UNAUTHORIZED,
            )
            .await;
        }
        // HTTP completion establishes that all synchronous dispatch decisions finished.
        assert!(matches!(
            unexpected.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        state
            .inner
            .control_channels
            .lock()
            .get_mut("daemon_2")
            .unwrap()
            .tx = original_sender;
        allocate(&mut daemon_2).await; // The unaffected daemon remains usable.
        server.abort();
    }

    #[tokio::test]
    async fn test_socket_ticket_cold_cache_rejects_never_issued_bearer() {
        let state = test_state(vec![]);
        let (base, relay) = spawn_test_relay_with_state(state).await;
        let (mut control, auth) =
            authenticate(&base, &identity(40, "authority-machine"), false, false).await;
        assert!(auth.success);
        let request = ticket_status(&base, "authority-machine", "never-issued");
        let authority = answer_token_authority(
            &mut control,
            &base,
            "never-issued",
            StatusCode::UNAUTHORIZED,
        );
        let (status, ()) = tokio::join!(request, authority);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        relay.abort();
    }

    #[tokio::test]
    async fn test_socket_ticket_gateway_confirmed_bearer_is_granted_and_cached() {
        let state = test_state(vec![]);
        let (base, relay) = spawn_test_relay_with_state(state.clone()).await;
        let (mut control, auth) =
            authenticate(&base, &identity(41, "authority-machine"), false, false).await;
        assert!(auth.success);
        let request = ticket_status(&base, "authority-machine", "valid-device-one");
        let authority =
            answer_token_authority(&mut control, &base, "valid-device-one", StatusCode::OK);
        let (status, ()) = tokio::join!(request, authority);
        assert_eq!(status, StatusCode::OK);
        assert!(state.has_fresh_device_token("authority-machine", "valid-device-one"));
        assert_eq!(
            ticket_status(&base, "authority-machine", "valid-device-one").await,
            StatusCode::OK
        );
        relay.abort();
    }

    #[tokio::test]
    async fn test_socket_ticket_expired_cache_rechecks_gateway_and_rejects_revoked_bearer() {
        let state = test_state(vec![]);
        let (base, relay) = spawn_test_relay_with_state(state.clone()).await;
        let (mut control, auth) =
            authenticate(&base, &identity(42, "authority-machine"), false, false).await;
        assert!(auth.success);
        state.register_device_token("authority-machine", "revoked-device");
        state
            .paired_tokens
            .lock()
            .get_mut(&("authority-machine".into(), "revoked-device".into()))
            .unwrap()
            .validated_at = Instant::now() - DEVICE_TOKEN_CACHE_TTL;
        let request = ticket_status(&base, "authority-machine", "revoked-device");
        let authority = answer_token_authority(
            &mut control,
            &base,
            "revoked-device",
            StatusCode::UNAUTHORIZED,
        );
        let (status, ()) = tokio::join!(request, authority);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(!state.has_fresh_device_token("authority-machine", "revoked-device"));
        relay.abort();
    }

    #[tokio::test]
    async fn a_refused_device_token_is_not_probed_again_and_repeated_refusals_lock_out() {
        let state = test_state(vec![]);
        let (base, relay) = spawn_test_relay_with_state(state.clone()).await;
        let (mut control, auth) =
            authenticate(&base, &identity(44, "authority-machine"), false, false).await;
        assert!(auth.success);
        let request = ticket_status(&base, "authority-machine", "refused-device");
        let authority =
            answer_token_authority(&mut control, &base, "refused-device", StatusCode::UNAUTHORIZED);
        let (status, ()) = tokio::join!(request, authority);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        // The same bearer is refused from the negative cache. A probe would open a session the
        // test never answers, so the request would time out instead of returning 401.
        for _ in 0..3 {
            assert_eq!(
                ticket_status(&base, "authority-machine", "refused-device").await,
                StatusCode::UNAUTHORIZED
            );
        }
        assert!(state.inner.pending_sessions.lock().is_empty());
        // Every refusal counted toward the per-IP lockout (1 probed + 3 cached = 4); the
        // fifth locks the address and the sixth is refused before any probe.
        assert_eq!(
            ticket_status(&base, "authority-machine", "refused-device").await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            ticket_status(&base, "authority-machine", "another-device").await,
            StatusCode::TOO_MANY_REQUESTS
        );
        relay.abort();
    }

    #[tokio::test]
    async fn test_socket_ticket_cache_keeps_two_valid_tokens_for_one_machine() {
        let state = test_state(vec![]);
        let (base, relay) = spawn_test_relay_with_state(state.clone()).await;
        let (mut control, auth) =
            authenticate(&base, &identity(43, "authority-machine"), false, false).await;
        assert!(auth.success);
        for token in ["valid-device-one", "valid-device-two"] {
            let request = ticket_status(&base, "authority-machine", token);
            let authority = answer_token_authority(&mut control, &base, token, StatusCode::OK);
            let (status, ()) = tokio::join!(request, authority);
            assert_eq!(status, StatusCode::OK);
        }
        for token in ["valid-device-one", "valid-device-two"] {
            assert_eq!(
                ticket_status(&base, "authority-machine", token).await,
                StatusCode::OK
            );
            assert!(state.has_fresh_device_token("authority-machine", token));
        }
        relay.abort();
    }

    #[test]
    fn a_malformed_socket_query_is_a_bad_request_not_an_auth_failure() {
        assert_eq!(socket_query_rejection(Some("ticket=t&cols=080")), StatusCode::BAD_REQUEST);
        assert_eq!(socket_query_rejection(Some("ticket=t&rows=70000")), StatusCode::BAD_REQUEST);
        assert_eq!(socket_query_rejection(Some("ticket=t&unknown=1")), StatusCode::BAD_REQUEST);
        // A missing ticket or a credential-carrying key stays an authorization failure.
        assert_eq!(socket_query_rejection(None), StatusCode::UNAUTHORIZED);
        assert_eq!(socket_query_rejection(Some("cols=80")), StatusCode::UNAUTHORIZED);
        assert_eq!(
            socket_query_rejection(Some("ticket=t&access_token=permanent")),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(socket_query_rejection(Some("token=permanent")), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_terminal_socket_with_a_noncanonical_dimension_answers_400() {
        let (base, server) = spawn_test_relay().await;
        assert_socket_rejected(
            &base,
            "/host/browser-machine/api/v1/terminal/t1?ticket=t&cols=080",
            StatusCode::BAD_REQUEST,
        )
        .await;
        server.abort();
    }

    #[test]
    fn every_device_route_trims_the_bearer_the_same_way() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer  padded-token ".parse().unwrap());
        assert_eq!(device_bearer(&headers), Some("padded-token"));
        headers.insert("authorization", "Bearer    ".parse().unwrap());
        assert_eq!(device_bearer(&headers), None);
        headers.insert("authorization", "Basic abc".parse().unwrap());
        assert_eq!(device_bearer(&headers), None);
    }

    #[tokio::test]
    async fn a_socket_ticket_accepts_a_bearer_with_surrounding_whitespace() {
        let state = test_state(vec![]);
        let (_generation, _notices, _grants) =
            state.register_control_channel("browser-machine".into());
        state.register_device_token("browser-machine", "padded-token");
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer padded-token ".parse().unwrap());
        let response = socket_ticket_handler(
            State(state.clone()),
            AxumPath("browser-machine".into()),
            None,
            headers,
            Json(serde_json::json!({"target": "/api/v1/events"})),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_relay_socket_ticket_issuance_and_single_use() {
        let state = test_state(vec![]);
        let (_generation, _control, _grants) = state.register_control_channel("browser-machine".into());
        let ticket = issue_test_ticket(&state, "/api/v1/events").await;
        assert_eq!(
            uuid::Uuid::parse_str(&ticket.ticket)
                .unwrap()
                .get_version_num(),
            4
        );
        assert!(ticket.expires_at > current_time_secs());
        assert!(ticket.expires_at <= current_time_secs() + 30);
        assert_eq!(
            consume_socket_ticket(
                &state,
                ticket_query(&ticket.ticket),
                None,
                "browser-machine",
                "/api/v1/events"
            ),
            Ok("test-device-token".into())
        );
        assert_eq!(
            consume_socket_ticket(
                &state,
                ticket_query(&ticket.ticket),
                None,
                "browser-machine",
                "/api/v1/events"
            ),
            Err(StatusCode::UNAUTHORIZED)
        );
        for (machine, target) in [
            ("other", "/api/v1/events"),
            ("browser-machine", "/api/v1/terminal/t1"),
        ] {
            let ticket = issue_test_ticket(&state, "/api/v1/events").await;
            assert_eq!(
                consume_socket_ticket(&state, ticket_query(&ticket.ticket), None, machine, target),
                Err(StatusCode::UNAUTHORIZED)
            );
            assert!(!state
                .inner
                .pending_socket_tickets
                .lock()
                .contains_key(&ticket.ticket));
        }
    }

    #[tokio::test]
    async fn test_relay_socket_ticket_expired_rejected() {
        let state = test_state(vec![]);
        let (_generation, _control, _grants) = state.register_control_channel("browser-machine".into());
        let ticket = issue_test_ticket(&state, "/api/v1/events").await;
        state
            .inner
            .pending_socket_tickets
            .lock()
            .get_mut(&ticket.ticket)
            .unwrap()
            .3 = current_time_secs() - 1;
        assert_eq!(
            consume_socket_ticket(
                &state,
                ticket_query(&ticket.ticket),
                None,
                "browser-machine",
                "/api/v1/events"
            ),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[tokio::test]
    async fn a11_expired_ticket_wire_boundary() {
        let state = test_state(vec![]);
        let (_generation, _control, _grants) = state.register_control_channel("browser-machine".into());
        let ticket = issue_test_ticket(&state, "/api/v1/events").await;
        // Inject time's exact boundary, never wait for wall-clock expiry.
        state
            .inner
            .pending_socket_tickets
            .lock()
            .get_mut(&ticket.ticket)
            .unwrap()
            .3 = current_time_secs();
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let outcome = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
            assert_socket_rejected(
                &base,
                &format!(
                    "/host/browser-machine/api/v1/events?ticket={}",
                    ticket.ticket
                ),
                StatusCode::UNAUTHORIZED,
            )
            .await;
            assert!(!state
                .inner
                .pending_socket_tickets
                .lock()
                .contains_key(&ticket.ticket));
            println!("A11 expired ticket exact boundary: actual WS401; single-use record consumed");
        }))
        .await;
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    /// `claim_pairing` validates against a control generation and then releases the
    /// lock; dispatch happens afterwards and picks the then-current channel. If the
    /// owner reconnects inside that window, the replacement must not be handed a claim
    /// it never authorized, so dispatch re-checks the generation the claim carried.
    #[tokio::test]
    async fn test_relay_dispatch_rejects_a_replaced_control_channel() {
        let state = test_state(vec![]);
        let machine = "dispatch-window";
        let (tx, mut rx) = mpsc::channel(4);
        state
            .inner
            .control_channels
            .lock()
            .insert(machine.to_owned(), ControlChannel { generation: 5, tx });

        // A claim carrying a superseded generation must be refused outright.
        assert_eq!(
            state
                .open_session_channel(machine, Some(4))
                .await
                .err(),
            Some(StatusCode::GONE),
            "dispatch must refuse a generation the current control channel never held"
        );

        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert!(state.inner.pending_sessions.lock().is_empty());
        // Observe the exact dispatch boundary, then cancel the pending request.
        let mut dispatch = Box::pin(state.open_session_channel(machine, Some(5)));
        let notice = tokio::select! {
            result = &mut dispatch => panic!("dispatch returned before its data half: {:?}", result.err()),
            notice = timeout(Duration::from_secs(5), rx.recv()) => notice.unwrap().unwrap(),
        };
        assert!(state
            .inner
            .pending_sessions
            .lock()
            .contains_key(&notice.session_id));
        // Dropping the owning future (not a Pin reference) runs SessionGuard.
        drop(dispatch);
        assert!(state.inner.pending_sessions.lock().is_empty());
    }

    #[test]
    fn test_relay_pairing_claim_uses_relay_generation_not_client_attempt_counter() {
        // relay_client.rs starts each coordinator's pairing-attempt counter at 0 and
        // bumps it per attempt, so every machine's FIRST pairing sends attempt 1. The
        // relay allocates control generations from a relay-wide counter that session
        // allocation also advances. Comparing those two numbers made an ordinary second
        // machine's first pairing fail, so the claim must use the relay-stamped value.
        let state = test_state(vec![]);

        // A first machine occupies the relay, advancing the relay-wide counter.
        let (first_tx, _first_rx) = mpsc::channel(1);
        let first_generation = 1;
        state.inner.control_channels.lock().insert(
            "machine-one".to_owned(),
            ControlChannel {
                generation: first_generation,
                tx: first_tx,
            },
        );

        // A second machine connects later, so its control generation is NOT 1.
        let second_generation = 9;
        let (second_tx, _second_rx) = mpsc::channel(1);
        state.inner.control_channels.lock().insert(
            "machine-two".to_owned(),
            ControlChannel {
                generation: second_generation,
                tx: second_tx,
            },
        );

        // Its very first pairing attempt is client attempt 1.
        state
            .register_pairing(
                "machine-two",
                second_generation,
                RegisterPairingPin {
                    generation: Some(1),
                    machine_id: "machine-two".into(),
                    pin: "778899".into(),
                    pairing_token: "machine-two-first-pairing".into(),
                    expires_at: current_time_secs() + 120,
                },
            )
            .unwrap();

        let request = PublicPairExchangeRequest {
            pin: Some("778899".into()),
            code: None,
            pairing_token: None,
            device_name: "second-device".into(),
            installation_id: None,
        };
        let claim = state
            .claim_pairing("127.0.0.1".parse().unwrap(), &request, None)
            .expect("an ordinary first pairing from a second machine must succeed");
        assert_eq!(claim.machine_id, "machine-two");

        // An omitted client attempt number must not become a bypass either: the claim
        // is gated on the relay's own generation, so replacing the owner still revokes.
        state
            .register_pairing(
                "machine-one",
                first_generation,
                RegisterPairingPin {
                    generation: None,
                    machine_id: "machine-one".into(),
                    pin: "112233".into(),
                    pairing_token: "machine-one-null-generation".into(),
                    expires_at: current_time_secs() + 120,
                },
            )
            .unwrap();
        let (replacement_tx, _replacement_rx) = mpsc::channel(1);
        state.inner.control_channels.lock().insert(
            "machine-one".to_owned(),
            ControlChannel {
                generation: first_generation + 100,
                tx: replacement_tx,
            },
        );
        let stale_request = PublicPairExchangeRequest {
            pin: Some("112233".into()),
            code: None,
            pairing_token: None,
            device_name: "stale-device".into(),
            installation_id: None,
        };
        assert_eq!(
            state
                .claim_pairing("127.0.0.4".parse().unwrap(), &stale_request, None)
                .err(),
            Some(StatusCode::NOT_FOUND),
            "an omitted client generation must not bypass the relay generation check"
        );
    }

    #[test]
    fn test_relay_pairing_claim_requires_the_live_control_identity() {
        // The exchange hands back credentials attributed to the registering machine.
        // That attribution is only sound while the registering machine is still the
        // live control identity for the generation it registered under.
        let state = test_state(vec![]);
        let machine = "exchange-machine";
        let (tx, _rx) = mpsc::channel(1);
        state
            .inner
            .control_channels
            .lock()
            .insert(machine.to_owned(), ControlChannel { generation: 3, tx });

        let registration = RegisterPairingPin {
            generation: Some(3),
            machine_id: machine.into(),
            pin: "525354".into(),
            pairing_token: "exchange-pairing-token".into(),
            expires_at: current_time_secs() + 120,
        };
        state.register_pairing(machine, 3, registration).unwrap();

        let request = PublicPairExchangeRequest {
            pin: Some("525354".into()),
            code: None,
            pairing_token: None,
            device_name: "test-device".into(),
            installation_id: None,
        };
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // The registering control channel is replaced before the code is exchanged.
        let (new_tx, _new_rx) = mpsc::channel(1);
        state.inner.control_channels.lock().insert(
            machine.to_owned(),
            ControlChannel {
                generation: 4,
                tx: new_tx,
            },
        );
        assert_eq!(
            state.claim_pairing(ip, &request, None).err(),
            Some(StatusCode::NOT_FOUND),
            "a pairing registered by a superseded control identity must not be claimable"
        );

        // And with no control channel at all, the identity cannot be vouched for.
        state.inner.control_channels.lock().remove(machine);
        assert_eq!(
            state
                .claim_pairing("127.0.0.2".parse().unwrap(), &request, None)
                .err(),
            Some(StatusCode::NOT_FOUND),
            "a pairing for a departed control identity must not be claimable"
        );

        // Restoring the exact registering generation makes it claimable again, so the
        // guard is about identity liveness rather than blanket refusal.
        let (restored_tx, _restored_rx) = mpsc::channel(1);
        state.inner.control_channels.lock().insert(
            machine.to_owned(),
            ControlChannel {
                generation: 3,
                tx: restored_tx,
            },
        );
        let claim = state
            .claim_pairing("127.0.0.3".parse().unwrap(), &request, None)
            .expect("live control identity must be able to complete the exchange");
        assert_eq!(claim.machine_id, machine);
    }

    #[test]
    fn session_kind_selects_the_splice_lifetime() {
        assert_eq!(SessionKind::Http.lifetime(), Duration::from_secs(3600));
        assert!(SessionKind::RemoteDesktop.lifetime() > Duration::from_secs(3600));
        assert_eq!(SessionKind::default(), SessionKind::Http);

        let state = test_state(vec![]);
        let machine = "kind-machine";
        let (tx, _rx) = mpsc::channel(4);
        state
            .inner
            .control_channels
            .lock()
            .insert(machine.to_owned(), ControlChannel { generation: 1, tx });
        assert!(state.issue_session_of_kind(machine, "rd", Some(1), true, SessionKind::RemoteDesktop));
        assert!(state.issue_session(machine, "http", Some(1), true));
        // A plain session is parsed by the relay and can never be given the long lifetime.
        assert!(state.issue_session_of_kind(machine, "plain", Some(1), false, SessionKind::RemoteDesktop));
        assert_eq!(state.session_lifetime("rd"), SessionKind::RemoteDesktop.lifetime());
        assert_eq!(state.session_lifetime("http"), SessionKind::Http.lifetime());
        assert_eq!(state.session_lifetime("plain"), SessionKind::Http.lifetime());
        assert_eq!(state.session_lifetime("unknown"), SessionKind::Http.lifetime());
    }

    #[test]
    fn attach_session_kind_is_optional_on_the_wire() {
        // An older client sends no kind and an older relay echoes none: both read as Http.
        let old: AttachSessionRequest =
            serde_json::from_str(r#"{"machineId":"m"}"#).expect("old request");
        assert_eq!(old.session_kind, SessionKind::Http);
        let rd: AttachSessionRequest =
            serde_json::from_str(r#"{"machineId":"m","sessionKind":"remoteDesktop"}"#)
                .expect("rd request");
        assert_eq!(rd.session_kind, SessionKind::RemoteDesktop);
        let old_reply: AttachSessionResponse =
            serde_json::from_str(r#"{"sessionId":"s","machineId":"m","opaque":true}"#)
                .expect("old reply");
        assert_eq!(old_reply.session_kind, SessionKind::Http);
    }

    #[test]
    fn test_relay_stream_half_is_bound_to_the_allocating_control_generation() {
        // A session is authorized by one control channel. If the owner reconnects
        // (new generation), a half offered against the old authorization must be
        // refused rather than proxied into the new session.
        let state = test_state(vec![]);
        let machine = "generation-machine";
        let (tx, _rx) = mpsc::channel(4);
        state
            .inner
            .control_channels
            .lock()
            .insert(machine.to_owned(), ControlChannel { generation: 7, tx });

        assert!(state.issue_session(machine, "session-gen-7", Some(7), false));
        // Same generation still attaches.
        assert!(state.reserve_half("session-gen-7", HalfKind::Data).is_ok());

        assert!(state.issue_session(machine, "session-stale", Some(7), false));
        // The daemon reconnects: same machine, new control generation.
        let (new_tx, _new_rx) = mpsc::channel(4);
        state.inner.control_channels.lock().insert(
            machine.to_owned(),
            ControlChannel {
                generation: 8,
                tx: new_tx,
            },
        );
        assert_eq!(
            state.reserve_half("session-stale", HalfKind::Data).err(),
            Some(StatusCode::GONE),
            "a half from a superseded control generation must not attach"
        );

        // The owner disconnecting entirely also invalidates its pending sessions.
        assert!(state.issue_session(machine, "session-orphan", Some(8), false));
        state.inner.control_channels.lock().remove(machine);
        assert_eq!(
            state.reserve_half("session-orphan", HalfKind::Data).err(),
            Some(StatusCode::GONE),
            "a half for a departed owner must not attach"
        );
    }

    #[test]
    fn test_relay_key_store_merges_concurrent_enrollments_and_rejects_corrupt_records() {
        // Two relay processes share one ownership file. Each loaded the store before the
        // other enrolled, so a blind write of an in-memory snapshot would drop the peer's
        // record. Enrollments must merge instead.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("machine_keys.json");

        let first = RelayState::new_with_key_store(vec![], &path).unwrap();
        let second = RelayState::new_with_key_store(vec![], &path).unwrap();

        let alpha = identity(41, "alpha-machine");
        let beta = identity(42, "beta-machine");
        let auth = |id: &crate::remote::auth::MachineIdentity, machine: &str| ControlAuth {
            machine_id: machine.to_owned(),
            public_key: id.public_key.clone(),
            display_name: machine.to_owned(),
            enrollment_token: None,
            signature: String::new(),
            timestamp: 0,
        };

        first
            .bind_machine_key(&auth(&alpha, "alpha-machine"))
            .unwrap();
        second
            .bind_machine_key(&auth(&beta, "beta-machine"))
            .unwrap();

        let persisted: HashMap<String, String> =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(
            persisted.contains_key("alpha-machine"),
            "a second relay must not drop the first relay's enrollment"
        );
        assert!(persisted.contains_key("beta-machine"));

        // A record that cannot authenticate anything is an unusable authorization
        // decision, so loading it must fail loudly rather than silently.
        let corrupt = dir.path().join("corrupt.json");
        std::fs::write(&corrupt, br#"{"ghost-machine":"not-a-real-key"}"#).unwrap();
        assert!(
            RelayState::new_with_key_store(vec![], &corrupt).is_err(),
            "an invalid persisted public key must be rejected on load"
        );
    }

    #[test]
    fn test_relay_register_pairing_is_idempotent_for_identical_retries() {
        // A lost ACK makes the daemon re-send the SAME registration. That retry must
        // report success instead of "already registered", must not duplicate the entry,
        // and must not let another machine claim the same code.
        let state = test_state(vec![]);
        let machine = "retry-machine";
        let (tx, _rx) = mpsc::channel(1);
        state
            .inner
            .control_channels
            .lock()
            .insert(machine.to_owned(), ControlChannel { generation: 1, tx });

        let registration = RegisterPairingPin {
            generation: Some(1),
            machine_id: machine.into(),
            pin: "246813".into(),
            pairing_token: "retry-pairing-secret".into(),
            expires_at: current_time_secs() + 120,
        };

        assert!(state
            .register_pairing(machine, 1, registration.clone())
            .is_ok());
        // The identical retry is satisfied, not rejected.
        assert!(
            state
                .register_pairing(machine, 1, registration.clone())
                .is_ok(),
            "an identical re-registration must be idempotent"
        );
        // Exactly one entry exists for that PIN.
        assert_eq!(
            state
                .inner
                .pairings
                .lock()
                .values()
                .filter(|p| p.registration.pin == "246813")
                .count(),
            1
        );

        // A DIFFERENT machine presenting the same PIN is still refused.
        let (other_tx, _other_rx) = mpsc::channel(1);
        state.inner.control_channels.lock().insert(
            "other-machine".to_owned(),
            ControlChannel {
                generation: 1,
                tx: other_tx,
            },
        );
        let mut hijack = registration.clone();
        hijack.machine_id = "other-machine".into();
        hijack.pairing_token = "other-pairing-secret".into();
        assert!(
            state.register_pairing("other-machine", 1, hijack).is_err(),
            "a different machine must not claim an active pairing code"
        );
    }

    #[test]
    fn test_relay_register_pairing_caps_lease_lifetime() {
        // A 6-digit numeric code is only safe as a short lease, so a client-supplied
        // far-future expiry must be refused rather than trusted.
        let state = test_state(vec![]);
        let machine = "lease-machine";
        // Hold the receiver so the channel stays open for the duration of the test.
        let (tx, _rx) = mpsc::channel(1);
        state
            .inner
            .control_channels
            .lock()
            .insert(machine.to_owned(), ControlChannel { generation: 1, tx });

        let registration = |expires_at: u64| RegisterPairingPin {
            generation: Some(1),
            machine_id: machine.into(),
            pin: "654321".into(),
            pairing_token: "lease-pairing-secret".into(),
            expires_at,
        };

        // A day-long lease is rejected outright.
        assert!(state
            .register_pairing(machine, 1, registration(current_time_secs() + 86_400))
            .is_err());
        // Exceeding the cap by one second is still rejected: the bound is the contract.
        assert!(state
            .register_pairing(
                machine,
                1,
                registration(current_time_secs() + MAX_PAIRING_LEASE + 1)
            )
            .is_err());
        // A lease inside the cap is accepted, so the guard is not simply refusing everything.
        assert!(state
            .register_pairing(
                machine,
                1,
                registration(current_time_secs() + MAX_PAIRING_LEASE - 5)
            )
            .is_ok());
    }

    #[tokio::test]
    async fn the_relay_carries_an_account_attach_only_as_ciphertext() {
        use crate::remote::attach_crypto::WebSocketByteStream;
        use crate::remote::session_transport::{establish_session, SessionTransport};
        use base64::{engine::general_purpose::STANDARD, Engine as _};

        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (generation, _notices, _grants) = state.register_control_channel("opaque-load".into());
        let session = "opaque-e2e";
        assert!(state.issue_session("opaque-load", session, Some(generation), true));
        let recorded = state.tap_session_traffic(session);

        let machine = {
            let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
            crate::remote::attach_identity::AttachIdentity {
                public_key: STANDARD.encode(x25519_dalek::PublicKey::from(&secret).as_bytes()),
                private_key: STANDARD.encode(secret.to_bytes()),
            }
        };
        let client_dir = tempfile::tempdir().unwrap();
        let device_public = crate::remote::attach_client::load_or_generate_client_attach_identity(
            client_dir.path(),
        )
        .expect("client identity")
        .public_key;

        // The daemon half: the same responder `relay_client::handle_session` runs, reached
        // through the relay's data route.
        let daemon = tokio::spawn({
            let base = base.clone();
            let machine = machine.clone();
            async move {
                let (ws, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{session}"))
                    .await
                    .unwrap();
                let expected = STANDARD.decode(&device_public).expect("device key");
                let transport = establish_session(
                    WebSocketByteStream::new(ws),
                    true,
                    Some(&machine),
                    "opaque-load",
                    session,
                    "3",
                    move |key: &[u8; 32]| key.as_slice() == expected.as_slice(),
                )
                .await
                .expect("attached session");
                let SessionTransport::Attached(mut secure) = transport else {
                    panic!("expected an attached transport");
                };
                let received = secure.recv_frame().await.expect("decrypted frame");
                secure
                    .send_frame(b"FERRYX_E2EE_REPLY terminal output")
                    .await
                    .expect("daemon reply");
                received
            }
        });

        // The account client half: encrypts before anything leaves the process.
        let mut secure = crate::paired_host::attach::attach_relay_session(
            &base,
            session,
            "opaque-load",
            &machine.public_key,
            "3",
            client_dir.path(),
        )
        .await
        .expect("account attach handshake");
        let sentinel = b"FERRYX_E2EE_SENTINEL typing keystrokes".to_vec();
        secure.send_frame(&sentinel).await.expect("send");

        let decrypted = timeout(Duration::from_secs(10), daemon)
            .await
            .expect("daemon replied in time")
            .expect("daemon task");
        assert_eq!(decrypted, sentinel, "only the daemon could read this");

        let reply = timeout(Duration::from_secs(10), secure.recv_frame())
            .await
            .expect("client received reply in time")
            .expect("client reply");
        assert_eq!(reply, b"FERRYX_E2EE_REPLY terminal output");

        let spliced = String::from_utf8_lossy(&recorded.lock()).to_string();
        assert!(
            !spliced.contains("FERRYX_E2EE_SENTINEL"),
            "the spliced copy must never contain terminal plaintext"
        );
        assert!(
            !spliced.contains("keystrokes") && !spliced.contains("terminal output"),
            "no fragment of the payload may survive in the spliced copy"
        );
        assert!(
            !recorded.lock().is_empty(),
            "the relay must have forwarded spliced frames"
        );
        server.abort();
    }

    fn e2e_attach_identity() -> crate::remote::attach_identity::AttachIdentity {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        crate::remote::attach_identity::AttachIdentity {
            public_key: STANDARD.encode(x25519_dalek::PublicKey::from(&secret).as_bytes()),
            private_key: STANDARD.encode(secret.to_bytes()),
        }
    }

    /// Allocates one `?e2e=1` attach session with `bearer` and returns the parsed answer.
    async fn allocate_e2e(http: &str, bearer: &str, machine: &str) -> AttachSessionResponse {
        let response = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .post(format!("{http}/api/v1/attach/session?e2e=1"))
            .bearer_auth(bearer)
            .header("X-Ferryx-Client-Build", "web-test-e2e")
            .json(&serde_json::json!({ "machineId": machine }))
            .send()
            .await
            .expect("allocation request");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[RELAY_E2E_HEADER], "1");
        let allocated: AttachSessionResponse = response.json().await.expect("allocation body");
        assert!(allocated.opaque, "an e2e allocation must be opaque");
        assert_eq!(allocated.machine_id, machine);
        allocated
    }

    /// The daemon half of an e2e session: the responder `relay_client::handle_session` runs,
    /// reached through the relay's data route, carrying decrypted bytes to `gateway`.
    fn spawn_e2e_daemon(
        base: String,
        session: String,
        machine_id: &'static str,
        machine: crate::remote::attach_identity::AttachIdentity,
        device_public: String,
        gateway: tokio::io::DuplexStream,
    ) -> tokio::task::JoinHandle<()> {
        use crate::remote::attach_crypto::WebSocketByteStream;
        use crate::remote::session_transport::{
            establish_session, proxy_secure_to_gateway, SessionTransport,
        };
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        tokio::spawn(async move {
            let (ws, _) =
                tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{session}"))
                    .await
                    .expect("daemon data half");
            let expected = STANDARD.decode(&device_public).expect("device key");
            let transport = establish_session(
                WebSocketByteStream::new(ws),
                true,
                Some(&machine),
                machine_id,
                &session,
                "7",
                move |key: &[u8; 32]| key.as_slice() == expected.as_slice(),
            )
            .await
            .expect("attached session");
            let SessionTransport::Attached(secure) = transport else {
                panic!("expected an attached transport");
            };
            let _ = proxy_secure_to_gateway(secure, gateway).await;
        })
    }

    fn http_response_complete(raw: &[u8]) -> bool {
        let text = String::from_utf8_lossy(raw);
        let Some(split) = text.find("\r\n\r\n") else {
            return false;
        };
        let length = text[..split]
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        raw.len() >= split + 4 + length
    }

    #[tokio::test]
    async fn an_e2e_host_proxy_request_never_reaches_the_relay_in_plaintext() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        const MACHINE: &str = "e2e-machine";
        const TOKEN: &str = "e2e-device-token";

        // No account state: a self-hosted relay must still allocate with a device token.
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let http = base.replace("ws://", "http://");
        let (_generation, mut notices, _grants) = state.register_control_channel(MACHINE.into());
        state.register_device_token(MACHINE, TOKEN);

        let machine = e2e_attach_identity();
        let client_dir = tempfile::tempdir().unwrap();
        let device_public = crate::remote::attach_client::load_or_generate_client_attach_identity(
            client_dir.path(),
        )
        .expect("client identity")
        .public_key;

        // HTTP over one encrypted session.
        let allocated = allocate_e2e(&http, TOKEN, MACHINE).await;
        let notice = timeout(Duration::from_secs(5), notices.recv())
            .await
            .expect("control notice in time")
            .expect("control notice");
        assert_eq!(notice.session_id, allocated.session_id);
        assert!(notice.opaque, "the daemon must be told to attach securely");
        let http_tap = state.tap_session_traffic(&allocated.session_id);

        let (mut gateway_peer, gateway_local) = tokio::io::duplex(64 * 1024);
        let daemon = spawn_e2e_daemon(
            base.clone(),
            allocated.session_id.clone(),
            MACHINE,
            machine.clone(),
            device_public.clone(),
            gateway_local,
        );
        let gateway = tokio::spawn(async move {
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = gateway_peer.read(&mut buffer).await.expect("gateway read");
                assert!(read > 0, "gateway closed before the request head");
                request.extend_from_slice(&buffer[..read]);
            }
            let text = String::from_utf8(request).expect("plaintext request at the gateway");
            let marker = text
                .split_once("workspaceId=")
                .and_then(|(_, rest)| rest.split(' ').next())
                .expect("marker in the request line")
                .to_string();
            let body = format!(r#"{{"workspaceId":"{marker}"}}"#);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            gateway_peer
                .write_all(response.as_bytes())
                .await
                .expect("gateway write");
            (text, gateway_peer)
        });

        let mut secure = crate::paired_host::attach::attach_relay_session(
            &base,
            &allocated.session_id,
            MACHINE,
            &machine.public_key,
            "7",
            client_dir.path(),
        )
        .await
        .expect("client attach handshake");
        secure
            .send_frame(
                format!(
                    "GET /api/v1/sessions?workspaceId=MARKER_E2E_HTTP_7f3a HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: 0\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .expect("send request");
        let mut response = Vec::new();
        while !http_response_complete(&response) {
            let frame = timeout(Duration::from_secs(10), secure.recv_frame())
                .await
                .expect("response frame in time")
                .expect("response frame");
            response.extend_from_slice(&frame);
        }
        let response = String::from_utf8(response).expect("utf-8 response");
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("MARKER_E2E_HTTP_7f3a"));
        let (seen, _gateway_peer) = timeout(Duration::from_secs(5), gateway)
            .await
            .expect("gateway in time")
            .expect("gateway task");
        assert!(seen.starts_with("GET /api/v1/sessions?workspaceId=MARKER_E2E_HTTP_7f3a HTTP/1.1\r\n"));

        // A WebSocket-style stream gets its own fresh encrypted session.
        let socket_allocated = allocate_e2e(&http, TOKEN, MACHINE).await;
        assert_ne!(socket_allocated.session_id, allocated.session_id);
        let socket_notice = timeout(Duration::from_secs(5), notices.recv())
            .await
            .expect("socket notice in time")
            .expect("socket notice");
        assert_eq!(socket_notice.session_id, socket_allocated.session_id);
        assert!(socket_notice.opaque);
        let socket_tap = state.tap_session_traffic(&socket_allocated.session_id);

        let (mut echo_peer, echo_local) = tokio::io::duplex(64 * 1024);
        let socket_daemon = spawn_e2e_daemon(
            base.clone(),
            socket_allocated.session_id.clone(),
            MACHINE,
            machine.clone(),
            device_public,
            echo_local,
        );
        let echo = tokio::spawn(async move {
            let mut buffer = [0u8; 4096];
            loop {
                let read = echo_peer.read(&mut buffer).await.unwrap_or(0);
                if read == 0 {
                    return;
                }
                if echo_peer.write_all(&buffer[..read]).await.is_err() {
                    return;
                }
            }
        });
        let mut socket = crate::paired_host::attach::attach_relay_session(
            &base,
            &socket_allocated.session_id,
            MACHINE,
            &machine.public_key,
            "7",
            client_dir.path(),
        )
        .await
        .expect("socket attach handshake");
        let payload = b"terminal input: echo MARKER_E2E\n".to_vec();
        socket.send_frame(&payload).await.expect("send payload");
        let mut echoed = Vec::new();
        while echoed.len() < payload.len() {
            let frame = timeout(Duration::from_secs(10), socket.recv_frame())
                .await
                .expect("echo frame in time")
                .expect("echo frame");
            echoed.extend_from_slice(&frame);
        }
        assert_eq!(echoed, payload);

        for (name, tap) in [("http", &http_tap), ("socket", &socket_tap)] {
            let spliced = tap.lock().clone();
            assert!(!spliced.is_empty(), "the relay must have spliced the {name} session");
            let spliced = String::from_utf8_lossy(&spliced);
            assert!(
                !spliced.contains("MARKER_E2E"),
                "the {name} session must never cross the relay in plaintext"
            );
            assert!(
                !spliced.contains(TOKEN),
                "the bearer inside the {name} session must never cross the relay in plaintext"
            );
        }
        echo.abort();
        daemon.abort();
        socket_daemon.abort();
        server.abort();
    }

    #[tokio::test]
    async fn a_legacy_client_without_e2e_still_works_and_logs_deprecation() {
        let state = test_state(vec![]);
        let (_generation, mut notices, _grants) = state.register_control_channel("m".into());
        let device_token = "legacy-device-token";
        state.register_device_token("m", device_token);
        let (base, server) = spawn_test_relay_with_account_state(state.clone(), None).await;
        let http_base = base.replace("ws://", "http://");
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();

        let mut request = tokio::spawn({
            let client = client.clone();
            let url = format!("{http_base}/host/m/api/v1/health");
            async move {
                client
                    .get(url)
                    .bearer_auth(device_token)
                    .header("X-Ferryx-Client-Build", "web-0.9.0-legacy")
                    .send()
                    .await
            }
        });
        let (served, settled) = serve_machine_sessions_until(
            &base,
            &mut notices,
            &mut request,
            "GET /api/v1/health HTTP/1.1",
            StatusCode::OK,
            br#"{"status":"healthy"}"#,
        )
        .await;
        assert_eq!(served, 1);
        let response = settled
            .expect("request task must not panic")
            .expect("plaintext proxy request must still answer");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(state.inner.plaintext_deprecations.load(Ordering::Relaxed), 1);
        assert_eq!(
            state.inner.last_plaintext_build.lock().as_deref(),
            Some("web-0.9.0-legacy")
        );

        let ticket = client
            .post(format!("{http_base}/host/m/api/v1/socket-ticket"))
            .bearer_auth(device_token)
            .header("X-Ferryx-Client-Build", "web-0.9.0-legacy")
            .json(&serde_json::json!({ "target": "/api/v1/events" }))
            .send()
            .await
            .expect("socket-ticket request");
        assert_eq!(ticket.status(), StatusCode::OK);
        let issued: SocketTicketResponse = ticket.json().await.expect("ticket body");
        assert!(!issued.ticket.is_empty());
        assert_eq!(state.inner.plaintext_deprecations.load(Ordering::Relaxed), 2);
        assert_eq!(
            state.inner.last_plaintext_build.lock().as_deref(),
            Some("web-0.9.0-legacy")
        );
        server.abort();
    }

    #[tokio::test]
    async fn a_relay_requiring_e2e_refuses_plaintext_with_a_typed_error() {
        let state = test_state(vec![]);
        state.set_require_e2e(true);
        let (_generation, mut notices, _grants) =
            state.register_control_channel("strict-machine".into());
        let device_token = "strict-device-token";
        state.register_device_token("strict-machine", device_token);
        let (base, server) = spawn_test_relay_with_account_state(state.clone(), None).await;
        let http_base = base.replace("ws://", "http://");
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        let plain = timeout(
            Duration::from_secs(5),
            client
                .get(format!("{http_base}/host/strict-machine/api/v1/health"))
                .bearer_auth(device_token)
                .send(),
        )
        .await
        .expect("a refusal must not wait on the machine")
        .expect("plaintext request");
        assert_eq!(plain.status(), StatusCode::UPGRADE_REQUIRED);
        let body: serde_json::Value = plain.json().await.unwrap();
        assert_eq!(body["error"]["code"], "E2E_REQUIRED");
        assert_eq!(body["error"]["retryable"], false);

        let ticket = timeout(
            Duration::from_secs(5),
            client
                .post(format!("{http_base}/host/strict-machine/api/v1/socket-ticket"))
                .bearer_auth(device_token)
                .json(&serde_json::json!({ "target": "/api/v1/events" }))
                .send(),
        )
        .await
        .expect("a refusal must not wait on the machine")
        .expect("socket-ticket request");
        assert_eq!(ticket.status(), StatusCode::UPGRADE_REQUIRED);
        let body: serde_json::Value = ticket.json().await.unwrap();
        assert_eq!(body["error"]["code"], "E2E_REQUIRED");

        // A WebSocket route answers the same 426 instead of upgrading.
        assert_socket_rejected(
            &base,
            "/host/strict-machine/api/v1/events?ticket=never-issued",
            StatusCode::UPGRADE_REQUIRED,
        )
        .await;
        assert!(
            notices.try_recv().is_err(),
            "a refused plaintext request must never open a data channel"
        );
        assert_eq!(state.inner.plaintext_deprecations.load(Ordering::Relaxed), 0);

        // Only `e2e=1` is admissible on the allocation route.
        let bad = client
            .post(format!("{http_base}/api/v1/attach/session?e2e=0"))
            .bearer_auth(device_token)
            .json(&serde_json::json!({ "machineId": "strict-machine" }))
            .send()
            .await
            .unwrap();
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        let unknown = client
            .post(format!("{http_base}/api/v1/attach/session?e2e=1&x=1"))
            .bearer_auth(device_token)
            .json(&serde_json::json!({ "machineId": "strict-machine" }))
            .send()
            .await
            .unwrap();
        assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
        assert_eq!(unknown.headers()[RELAY_E2E_HEADER], "1");
        // The account-only allocation (no e2e query) is not marked.
        let plain = client
            .post(format!("{http_base}/api/v1/attach/session"))
            .bearer_auth(device_token)
            .json(&serde_json::json!({ "machineId": "strict-machine" }))
            .send()
            .await
            .unwrap();
        assert!(plain.headers().get(RELAY_E2E_HEADER).is_none());
        let stranger = client
            .post(format!("{http_base}/api/v1/attach/session?e2e=1"))
            .bearer_auth("not-a-device-token")
            .json(&serde_json::json!({ "machineId": "strict-machine" }))
            .send();
        // An uncached token is probed on the machine; answer the probe 401.
        let (stranger, ()) = tokio::join!(stranger, async {
            let (line, mut data) = next_machine_request(&base, &mut notices).await;
            assert_eq!(line, DEVICE_PROBE_REQUEST_LINE);
            reply_http(&mut data, StatusCode::UNAUTHORIZED, b"").await;
        });
        let stranger = stranger.unwrap();
        assert_eq!(stranger.status(), StatusCode::UNAUTHORIZED);
        // A current relay marks even its refusal, so the client knows the token was revoked.
        assert_eq!(stranger.headers()[RELAY_E2E_HEADER], "1");
        assert!(stranger.headers()["access-control-expose-headers"]
            .to_str()
            .unwrap()
            .contains(RELAY_E2E_HEADER));
        let body: serde_json::Value = stranger.json().await.unwrap();
        assert_eq!(body["code"], "UNAUTHORIZED");

        // The encrypted allocation still succeeds.
        let allocated = allocate_e2e(&http_base, device_token, "strict-machine").await;
        let notice = timeout(Duration::from_secs(5), notices.recv())
            .await
            .expect("control notice in time")
            .expect("control notice");
        assert_eq!(notice.session_id, allocated.session_id);
        assert!(notice.opaque);
        server.abort();
    }

    #[tokio::test]
    async fn an_opaque_session_splices_frames_the_relay_never_parses() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (generation, _notices, _grants) = state.register_control_channel("opaque-load".into());
        let session = "opaque-session-1";
        assert!(state.issue_session("opaque-load", session, Some(generation), true));

        let (mut daemon, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{session}"))
            .await
            .unwrap();
        let (mut client, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/opaque/{session}"))
            .await
            .unwrap();

        // The client's bytes reach the daemon verbatim: the relay splices and never parses.
        let sentinel = b"FERRYX_E2EE_SENTINEL".to_vec();
        client.send(TMessage::Binary(sentinel.clone().into())).await.unwrap();
        let seen = timeout(Duration::from_secs(5), daemon.next()).await.unwrap().unwrap().unwrap();
        assert_eq!(seen.into_data().as_ref(), sentinel.as_slice());

        // The daemon's reply travels back the same way.
        let reply = b"FERRYX_E2EE_REPLY".to_vec();
        daemon.send(TMessage::Binary(reply.clone().into())).await.unwrap();
        let back = timeout(Duration::from_secs(5), client.next()).await.unwrap().unwrap().unwrap();
        assert_eq!(back.into_data().as_ref(), reply.as_slice());

        // A plain session is never served over the opaque route, and an opaque session is
        // never served over the plain client route.
        let plain = "plain-session-1";
        assert!(state.issue_session("opaque-load", plain, Some(generation), false));
        assert!(tokio_tungstenite::connect_async(format!("{base}/tunnel/opaque/{plain}"))
            .await
            .is_err());
        assert!(tokio_tungstenite::connect_async(format!("{base}/tunnel/client/{session}"))
            .await
            .is_err());
        server.abort();
    }

    #[tokio::test]
    async fn a_sealed_grant_reaches_the_daemon_control_socket() {
        let state = test_state(vec![]);
        let probe = state.clone();
        let (base, server) = spawn_test_relay_with_state(state).await;
        let owner = identity(1, "grant-machine");
        let (mut socket, response) =
            authenticate_with_token(&base, &owner, false, false, None).await;
        assert!(response.success);

        let envelope = crate::remote::account_protocol::AccountGrantOfferEnvelope {
            machine_id: owner.machine_id.clone(),
            enrollment_epoch: "7".into(),
            sealed: "c2VhbGVkLWdyYW50".into(),
        };
        let machine = owner.machine_id.clone();
        let deliver = tokio::spawn(async move { probe.deliver_grant_offer(&machine, envelope).await });

        // The daemon receives the sealed blob exactly as the account sealed it.
        let delivered: AccountGrantOfferDelivery = receive_json(&mut socket).await;
        assert_eq!(delivered.machine_id, owner.machine_id);
        assert_eq!(delivered.envelope.sealed, "c2VhbGVkLWdyYW50");
        socket
            .send(TMessage::Text(
                serde_json::to_string(&AccountGrantOfferDelivered {
                    grant_id: Some("grant-1".into()),
                    status: "ready".into(),
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();

        let ack = deliver.await.unwrap().unwrap();
        assert_eq!(ack.status, "ready");
        assert_eq!(ack.grant_id.as_deref(), Some("grant-1"));

        // A machine with no live control socket cannot be handed a grant.
        let orphan = test_state(vec![]);
        assert!(orphan
            .deliver_grant_offer(
                "no-such-machine",
                crate::remote::account_protocol::AccountGrantOfferEnvelope {
                    machine_id: "no-such-machine".into(),
                    enrollment_epoch: "7".into(),
                    sealed: "c2VhbGVk".into(),
                },
            )
            .await
            .is_err());
        server.abort();
    }

    #[tokio::test]
    async fn a_grant_for_the_announced_machine_id_reaches_a_token_keyed_control_socket() {
        // A daemon with FERRYX_MACHINE_TOKEN registers under its bearer token and announces its
        // account machine id (x-ferryx-machine-id); the account service delivers by that id.
        let state = test_state(vec![]);
        let (generation, _notices, mut grants) =
            state.register_control_channel("operator-token".into());
        state.bind_control_alias("operator-token", "enrolled-machine".into());

        let probe = state.clone();
        let deliver = tokio::spawn(async move {
            probe
                .deliver_grant_offer(
                    "enrolled-machine",
                    crate::remote::account_protocol::AccountGrantOfferEnvelope {
                        machine_id: "enrolled-machine".into(),
                        enrollment_epoch: "1".into(),
                        sealed: "c2VhbGVk".into(),
                    },
                )
                .await
        });
        let request = grants.recv().await.expect("grant reaches the token-keyed socket");
        assert_eq!(request.delivery.machine_id, "enrolled-machine");
        request
            .ack
            .send(AccountGrantOfferDelivered {
                grant_id: Some("grant-1".into()),
                status: "ready".into(),
            })
            .unwrap();
        assert_eq!(deliver.await.unwrap().unwrap().status, "ready");

        // Closing the socket retires the alias with it: no stale grant route survives.
        state.unregister_control_channel("operator-token", generation);
        assert!(!state.inner.grant_channels.lock().contains_key("enrolled-machine"));
        assert!(!state.inner.control_channels.lock().contains_key("enrolled-machine"));
    }

    #[tokio::test]
    async fn an_opaque_allocation_tells_the_daemon_to_attach_securely() {
        let state = test_state(vec![]);
        let machine = "opaque-machine";
        let (tx, mut rx) = mpsc::channel(4);
        state
            .inner
            .control_channels
            .lock()
            .insert(machine.to_owned(), ControlChannel { generation: 1, tx });

        assert!(state.issue_session(machine, "session-opaque", Some(1), true));
        let notice = rx.try_recv().expect("opaque notice");
        assert_eq!(notice.session_id, "session-opaque");
        assert!(notice.opaque, "the daemon must be told to attach securely");

        assert!(state.issue_session(machine, "session-plain", Some(1), false));
        assert!(!rx.try_recv().expect("plain notice").opaque);
    }

    #[tokio::test]
    async fn test_relay_browser_ws_terminal_bridge_success() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // Retain the state handle so the test can seed the device-token cache: the
        // bridge under test is the WebSocket upgrade, not ticket authorization.
        let state = test_state(vec!["test-machine-token".to_string()]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (mut control, auth) =
            authenticate(&base, &identity(12, "browser-machine"), false, false).await;
        assert!(auth.success);
        // An acknowledged control operation is the registration barrier.
        allocate(&mut control).await;
        state.register_device_token("browser-machine", "test-device-token");
        let response = reqwest::Client::new()
            .post(format!(
                "{}/host/browser-machine/api/v1/socket-ticket",
                base.replace("ws://", "http://")
            ))
            .header("authorization", "Bearer test-device-token")
            .timeout(Duration::from_secs(5))
            .header("content-type", "application/json")
            .body(r#"{"target":"/api/v1/terminal/t1"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let ticket: SocketTicketResponse =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        for query in [
            String::new(),
            "?token=permanent".into(),
            format!("?ticket={}&access_token=permanent", ticket.ticket),
        ] {
            let error = tokio_tungstenite::connect_async(format!(
                "{base}/host/browser-machine/api/v1/terminal/t1{query}"
            ))
            .await
            .unwrap_err();
            assert!(
                matches!(error, tokio_tungstenite::tungstenite::Error::Http(r) if r.status() == StatusCode::UNAUTHORIZED)
            );
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let gateway = async {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_hdr_async(
                tcp,
                crate::remote::InspectHandshake(|request: &tokio_tungstenite::tungstenite::handshake::server::Request| {
                    assert_eq!(request.uri().path(), "/api/v1/terminal/t1");
                    assert_eq!(
                        request.uri().query(),
                        Some("daemonEpoch=9007199254740993&afterSequence=18446744073709551615")
                    );
                    assert_eq!(
                        request.headers()["authorization"],
                        "Bearer test-device-token"
                    );
                }),
            )
            .await
            .unwrap();
            for expected in [
                TMessage::Text("input".into()),
                TMessage::Binary(vec![0, 1, 255].into()),
                TMessage::Ping(vec![7].into()),
            ] {
                assert_eq!(ws.next().await.unwrap().unwrap(), expected);
                ws.send(match expected {
                    TMessage::Ping(bytes) => TMessage::Pong(bytes),
                    other => other,
                })
                .await
                .unwrap();
            }
            // Consume any forwarded pong before the close frame.
            loop {
                match ws.next().await.unwrap().unwrap() {
                    TMessage::Close(_) => {
                        ws.flush().await.unwrap();
                        break;
                    }
                    TMessage::Pong(_) => {}
                    other => panic!("unexpected frame: {other:?}"),
                }
            }
        };
        let daemon = async {
            let notice: IncomingSessionNotice = receive_json(&mut control).await;
            let (mut data, _) = tokio_tungstenite::connect_async(format!(
                "{base}/tunnel/data/{}",
                notice.session_id
            ))
            .await
            .unwrap();
            let tcp = tokio::net::TcpStream::connect(address).await.unwrap();
            let (mut read, mut write) = tcp.into_split();
            let mut buffer = [0; 8192];
            loop {
                tokio::select! {
                    frame = data.next() => match frame {
                        Some(Ok(TMessage::Binary(bytes))) => write.write_all(&bytes).await.unwrap(),
                        Some(Ok(TMessage::Close(_))) => { data.flush().await.unwrap(); break; },
                        None => break,
                        Some(Ok(_)) => {},
                        Some(Err(error)) => panic!("data channel failed: {error}"),
                    },
                    count = read.read(&mut buffer) => {
                        let count = count.unwrap();
                        if count == 0 { data.close(None).await.unwrap(); break; }
                        data.send(TMessage::Binary(buffer[..count].to_vec().into())).await.unwrap();
                    }
                }
            }
        };
        let browser = async {
            let url = format!(
                "{base}/host/browser-machine/api/v1/terminal/t1?ticket={}&daemonEpoch=9007199254740993&afterSequence=18446744073709551615",
                ticket.ticket
            );
            let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
            let error = tokio_tungstenite::connect_async(&url).await.unwrap_err();
            assert!(
                matches!(error, tokio_tungstenite::tungstenite::Error::Http(r) if r.status() == StatusCode::UNAUTHORIZED)
            );
            for frame in [
                TMessage::Text("input".into()),
                TMessage::Binary(vec![0, 1, 255].into()),
                TMessage::Ping(vec![7].into()),
            ] {
                ws.send(frame.clone()).await.unwrap();
                let expected = match frame {
                    TMessage::Ping(bytes) => TMessage::Pong(bytes),
                    other => other,
                };
                assert_eq!(ws.next().await.unwrap().unwrap(), expected);
            }
            ws.close(None).await.unwrap();
            let mut observed_close = false;
            while let Some(frame) = ws.next().await {
                if matches!(frame.unwrap(), TMessage::Close(_)) {
                    observed_close = true;
                    break;
                }
            }
            assert!(observed_close, "browser EOF is not a close acknowledgement");
            eprintln!("Q4 browser close acknowledgement observed=true");
        };
        timeout(Duration::from_secs(5), async {
            tokio::join!(browser, daemon, gateway);
        })
        .await
        .unwrap();
        server.abort();
    }

    #[test]
    fn coordinator_pairs_through_relay_to_real_gateway() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.block_on(real_gateway_fixture())
        }));
        drop(runtime); // Joins all Axum upgrade/connection and reverse-channel tasks.
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    async fn real_gateway_fixture() {
        use crate::remote::{relay_client::RelayClient, state::RemoteGatewayState};
        let (base, relay) = spawn_test_relay().await;
        let terminal = Arc::new(crate::terminal::TerminalService::new(
            Arc::new(crate::terminal::PtyManager::new()),
            Arc::new(crate::terminal::TerminalOutputHub::default()),
        ));
        // Retain handles so the test can spawn a PTY on the SAME service and registry the
        // gateway serves, which is what makes the later attachment genuinely end-to-end.
        let terminal_handle = Arc::clone(&terminal);
        let registry = crate::worktree::WorkspaceRegistry::new();
        let registry_handle = registry.clone();
        let state = Arc::new(RemoteGatewayState::new_with_paths(
            terminal, registry, None, None,
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway_addr = listener.local_addr().unwrap();
        let router = crate::remote::server::create_remote_router(state.clone());
        let gateway = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = RelayClient::with_identity(
            &base,
            identity(19, "real-gateway"),
            gateway_addr.to_string(),
        )
        .with_enrollment_token("test-machine-token")
        .with_auth_manager((*state.auth_manager).clone());
        let coordinator = client.pairing_coordinator();
        let control = tokio::spawn(async move { client.run().await });
        let workspace = tempfile::tempdir().unwrap();
        let outcome = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
            let session = coordinator
                .generate_pairing(Duration::from_secs(60))
                .await
                .unwrap();
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap();
            let url = format!("{}/api/v1/pair/exchange", base.replace("ws://", "http://"));
            let response = http
                .post(&url)
                .header("content-type", "application/json")
                .body(
                    serde_json::json!({
                        "code": session.pairing_token, "deviceName": "real browser"
                    })
                    .to_string(),
                )
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body: serde_json::Value =
                serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
            assert!(body["machineId"].as_str().is_some_and(|id| !id.is_empty()));
            assert!(body["displayName"]
                .as_str()
                .is_some_and(|name| !name.is_empty()));
            let device = state
                .auth_manager
                .validate_token(body["token"].as_str().unwrap())
                .unwrap();
            assert_eq!(device.name, "real browser");
            // Given a completed exchange, only its issued device token may get tickets.
            for (token, expected) in [
                ("unissued-device-token", StatusCode::UNAUTHORIZED),
                (body["token"].as_str().unwrap(), StatusCode::OK),
            ] {
                // When requesting a socket ticket through the public HTTP surface.
                let ticket = http
                    .post(format!(
                        "{}/host/real-gateway/api/v1/socket-ticket",
                        base.replace("ws://", "http://")
                    ))
                    .bearer_auth(token)
                    .header("content-type", "application/json")
                    .body(r#"{"target":"/api/v1/events"}"#)
                    .send()
                    .await
                    .unwrap();
                // Then issuance reflects the authenticated device identity.
                assert_eq!(ticket.status(), expected);
            }
            assert!(state
                .auth_manager
                .exchange_pairing_code(&session.pairing_token, "replay")
                .is_err());

            // The auditor's F10 requirement: issuance alone is not end-to-end. Carry the
            // paired device credential through an actual authenticated terminal attachment
            // over the relay and prove real bytes flow from the real gateway's PTY.
            let device_token = body["token"].as_str().unwrap().to_owned();
            // Spawn a real PTY on the real gateway's terminal service, in a real worktree.

            registry_handle
                .register("e2e-workspace", workspace.path())
                .expect("register workspace");
            let manager = registry_handle.manager("e2e-workspace").expect("manager");
            let mut command = portable_pty::CommandBuilder::new(if cfg!(windows) {
                "cmd.exe"
            } else {
                "/bin/sh"
            });
            command.cwd(workspace.path());
            #[cfg(windows)]
            command.args(["/D", "/Q"]);

            let (session_id, _pty_rx) = terminal_handle
                .spawn_in_worktree(command, 80, 24, &manager, workspace.path())
                .expect("real gateway PTY session");

            let ticket = http
                .post(format!(
                    "{}/host/real-gateway/api/v1/socket-ticket",
                    base.replace("ws://", "http://")
                ))
                .bearer_auth(&device_token)
                .header("content-type", "application/json")
                .body(format!(r#"{{"target":"/api/v1/terminal/{session_id}"}}"#))
                .send()
                .await
                .unwrap();
            assert_eq!(ticket.status(), StatusCode::OK);
            let ticket: SocketTicketResponse =
                serde_json::from_slice(&ticket.bytes().await.unwrap()).unwrap();

            // Declare desktop selection as a different session to test that unselected sessions are rejected.
            state.set_active_selection(crate::remote::RemoteActiveDesktopSelection {
                workspace_id: None,
                worktree_slug: None,
                worktree_label: None,
                session_id: Some("different-session".into()),
                ..Default::default()
            });

            // The gateway attaches only the active desktop session. A valid ticket for a
            // non-selected session therefore carries no terminal data: the relay completes the
            // upgrade before it learns the upstream verdict, so the stream is torn down instead
            // of serving output. Assert no payload is delivered rather than asserting on the
            // handshake result, which is a relay-proxy implementation detail.
            {
                use futures_util::StreamExt;
                if let Ok((mut refused, _)) = tokio_tungstenite::connect_async(format!(
                    "{base}/host/real-gateway/api/v1/terminal/{session_id}?ticket={}",
                    ticket.ticket
                ))
                .await
                {
                    let delivered = timeout(Duration::from_secs(5), refused.next())
                        .await
                        .unwrap();
                    assert!(
                        matches!(
                            delivered,
                            None | Some(Err(_)) | Some(Ok(TMessage::Close(_)))
                        ),
                        "non-selected terminal must close, not merely remain silent: {delivered:?}"
                    );
                }
            }

            // Declare the desktop selection exactly as the real desktop does.
            state.set_active_selection(crate::remote::RemoteActiveDesktopSelection {
                workspace_id: None,
                worktree_slug: None,
                worktree_label: None,
                session_id: Some(session_id.clone()),
                ..Default::default()
            });

            // The first ticket was consumed by the refused attempt, so mint a fresh one.
            let ticket = http
                .post(format!(
                    "{}/host/real-gateway/api/v1/socket-ticket",
                    base.replace("ws://", "http://")
                ))
                .bearer_auth(&device_token)
                .header("content-type", "application/json")
                .body(format!(r#"{{"target":"/api/v1/terminal/{session_id}"}}"#))
                .send()
                .await
                .unwrap();
            assert_eq!(ticket.status(), StatusCode::OK);
            let ticket: SocketTicketResponse =
                serde_json::from_slice(&ticket.bytes().await.unwrap()).unwrap();

            // A single-use ticket authorizes the upgrade; the permanent token never
            // appears in the WebSocket URL.
            let (mut terminal_socket, _) = tokio_tungstenite::connect_async(format!(
                "{base}/host/real-gateway/api/v1/terminal/{session_id}?ticket={}",
                ticket.ticket
            ))
            .await
            .expect("authenticated terminal attachment over relay");

            // Drive the real PTY and await its echo instead of sleeping: the shell must
            // return the marker we wrote through the relay-proxied socket.
            use futures_util::{SinkExt, StreamExt};
            #[cfg(not(windows))]
            terminal_socket
                .send(tokio_tungstenite::tungstenite::Message::Binary(
                    b"printf 'ferryx_%s_marker\\n' executed\n".to_vec().into(),
                ))
                .await
                .unwrap();

            let echoed = timeout(Duration::from_secs(20), async {
                let mut seen = String::new();
                while let Some(Ok(message)) = terminal_socket.next().await {
                    match message {
                        tokio_tungstenite::tungstenite::Message::Binary(bytes) => {
                            seen.push_str(&String::from_utf8_lossy(&bytes));

                            #[cfg(windows)]
                            if seen.contains("\x1b[6n") {
                                // ConPTY asks the terminal client for its cursor before
                                // starting the shell. Answer the real terminal query.
                                terminal_socket
                                    .send(TMessage::Binary(b"\x1b[1;1R".to_vec().into()))
                                    .await
                                    .unwrap();
                                terminal_socket
                                    .send(TMessage::Binary(
                                        b"set Q4_WORD=executed\recho ferryx_%Q4_WORD%_marker\r"
                                            .to_vec()
                                            .into(),
                                    ))
                                    .await
                                    .unwrap();
                                seen = seen.replace("\x1b[6n", "");
                            }
                        }
                        tokio_tungstenite::tungstenite::Message::Text(text) => {
                            seen.push_str(&text);
                        }
                        _ => {}
                    }
                    if seen.contains("ferryx_executed_marker") {
                        return true;
                    }
                }
                false
            })
            .await
            .expect("terminal output arrived before the deadline");
            assert!(
                echoed,
                "real PTY output must traverse the relay to the paired browser"
            );

            // A single-use ticket must not authorize a second attachment.
            assert!(tokio_tungstenite::connect_async(format!(
                "{base}/host/real-gateway/api/v1/terminal/{session_id}?ticket={}",
                ticket.ticket
            ))
            .await
            .is_err());

            terminal_socket.close(None).await.unwrap();
        }))
        .await;
        for id in terminal_handle.list_sessions() {
            terminal_handle
                .close_session(&id)
                .await
                .expect("close owned real PTY");
        }
        assert!(terminal_handle.list_sessions().is_empty());
        control.abort();
        gateway.abort();
        relay.abort();
        assert!(control.await.unwrap_err().is_cancelled());
        assert!(gateway.await.unwrap_err().is_cancelled());
        assert!(relay.await.unwrap_err().is_cancelled());
        workspace.close().unwrap();
        println!("R4 PTY cleanup: owned sessions explicitly closed; listeners/reverse joined; private workspace removed");
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
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
                "pairing-secret-one",
                "/api/v1/pair/exchange",
                serde_json::json!({"pin":"123456"}),
            ),
            (
                "234567",
                "pairing-secret-two",
                "/host/pair-machine/api/v1/pair/exchange",
                serde_json::json!({"pairingToken":"pairing-secret-two"}),
            ),
            (
                "345678",
                "pairing-secret-three",
                "/api/v1/pair/exchange",
                serde_json::json!({"code":"345678"}),
            ),
            (
                "456789",
                "pairing-secret-four",
                "/api/v1/pair/exchange",
                serde_json::json!({"code":"pairing-secret-four"}),
            ),
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
            payload["installationId"] = "test-inst-uuid-1234".into();
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
                assert_eq!(body["installationId"], "test-inst-uuid-1234");
                let body = br#"{"token":"device-token"}"#;
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Daemon: yes\r\nContent-Length: {}\r\n\r\n", body.len());
                data.send(TMessage::Binary(response.into_bytes().into()))
                    .await
                    .unwrap();
                data.send(TMessage::Binary(body.to_vec().into()))
                    .await
                    .unwrap();
                // The relay keeps the keep-alive channel for reuse; hang up cleanly.
                data.close(None).await.unwrap();
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
                    StatusCode::TOO_MANY_REQUESTS
                }
            );
            if attempt == 5 {
                let retry_after: u64 = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse().ok())
                    .expect("a lockout names when to retry");
                assert!((1..=ADMISSION_WINDOW.as_secs()).contains(&retry_after));
                let body: serde_json::Value = response.json().await.unwrap();
                assert_eq!(body["code"], "pairing_rate_limited");
            }
        }
        server.abort();
    }

    #[test]
    fn rejects_registration_when_capability_format_is_invalid() {
        // Given an authenticated machine and malformed capabilities.
        for token in [
            "short",
            "abcdefghijklmnop_",
            "abcdefghijklmnop/",
            "éabcdefghijklmnop",
            &"a".repeat(65),
        ] {
            let state = test_state(vec![]);
            let (generation, _notices, _grants) = state.register_control_channel("machine".into());
            let mut registration = security_registration("machine");
            registration.pairing_token = token.into();
            // When registering the pairing capability.
            let result = state.register_pairing("machine", generation, registration);
            // Then the registration is rejected.
            assert!(result.is_err(), "accepted {token}");
        }
    }

    #[tokio::test]
    async fn routes_session_when_path_is_colon_scoped() {
        // Given a valid scoped session path and an offline machine.
        let state = test_state(vec![]);
        let path = "session/workspace:session-1";
        let request = Request::builder()
            .uri(format!("/host/machine/api/v1/{path}"))
            .body(Body::empty())
            .unwrap();
        // When proxying the session request.
        let result = host_http_handler(
            State(state),
            AxumPath(("machine".into(), path.into())),
            None,
            request,
        )
        .await;
        // Then routing succeeds and the offline machine is reported, not forbidden.
        assert_eq!(result.unwrap().status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn a_reference_chat_read_admits_every_field_the_host_declares() {
        // The host's own `ReferenceChatReadQuery` declares these; every reference-chat read
        // route deserializes it, so the relay must not refuse a query the host accepts.
        let query = "hostId=local&ownerId=owner&epoch=1&backendSessionId=s&providerSessionId=p\
                     &registryId=codex&limit=50&cursor=3&cursorStream=codex-transcript:abc";
        for suffix in ["history", "screen", "prompt", "files/attachment-1"] {
            let path = format!("reference-chat/session-1/{suffix}");
            assert!(
                validate_http_query(&path, Some(query)).is_ok(),
                "{path} must admit every field the host's read query declares"
            );
            assert!(
                validate_http_query(&path, Some("repoPath=/etc/passwd")).is_err(),
                "{path} must still refuse a field the contract does not define"
            );
        }
    }

    /// Widening the allowlist changes the response SHAPE, never the admission decisions.
    #[test]
    fn the_reference_chat_allowlist_still_refuses_what_it_always_refused() {
        let path = "reference-chat/session-1/screen";
        // A field the contract does not define.
        assert_eq!(
            validate_http_query(path, Some("repoPath=/etc/passwd")),
            Err(StatusCode::BAD_REQUEST)
        );
        // A field with no `=`.
        assert_eq!(
            validate_http_query(path, Some("ownerId=owner&epoch")),
            Err(StatusCode::BAD_REQUEST)
        );
        // A bad `%` escape.
        assert_eq!(
            validate_http_query(path, Some("ownerId=%GG")),
            Err(StatusCode::BAD_REQUEST)
        );
        // The same field twice.
        assert_eq!(
            validate_http_query(path, Some("ownerId=a&ownerId=b")),
            Err(StatusCode::BAD_REQUEST)
        );
        // An oversized query.
        let oversized = format!("ownerId={}", "a".repeat(17 * 1024));
        assert_eq!(
            validate_http_query(path, Some(&oversized)),
            Err(StatusCode::BAD_REQUEST)
        );
        // A control character in a value.
        assert_eq!(
            validate_http_query(path, Some("ownerId=%00")),
            Err(StatusCode::BAD_REQUEST)
        );
        // The frozen route table still denies everything outside it.
        assert!(!allowed_http_route(&Method::GET, "reference-chat/session-1"));
        assert!(!allowed_http_route(
            &Method::GET,
            "reference-chat/session-1/secret"
        ));
    }

    #[tokio::test]
    async fn a_refused_query_answers_with_the_machine_envelope() {
        // A bare `StatusCode` error becomes a bodyless 400, which is what a caller on this
        // surface must never see.
        let state = test_state(vec![]);
        let path = "reference-chat/session-1/screen";
        let request = Request::builder()
            .uri(format!("/host/machine/api/v1/{path}?repoPath=/etc/passwd"))
            .body(Body::empty())
            .unwrap();
        let response = host_http_handler(
            State(state),
            AxumPath(("machine".into(), path.into())),
            None,
            request,
        )
        .await
        .expect("a refused query is a response, not a bare status");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("the refusal carries a JSON envelope");
        assert_eq!(parsed["error"]["code"].as_str(), Some("INVALID_REQUEST"));
        assert_eq!(parsed["error"]["retryable"].as_bool(), Some(false));
        assert!(parsed["error"]["message"].as_str().is_some_and(|m| !m.is_empty()));
    }

    /// F-5: the machine envelope is the reference-chat batch's contract only. Every other
    /// route keeps the parent's bodyless 400, which is the shape the machine's own handlers
    /// still answer for the same malformed query.
    #[tokio::test]
    async fn a_refused_query_outside_reference_chat_stays_a_bodyless_400() {
        let state = test_state(vec![]);
        let path = "fs/directories";
        let request = Request::builder()
            .uri(format!("/host/machine/api/v1/{path}?repoPath=/etc/passwd"))
            .body(Body::empty())
            .unwrap();
        let result = host_http_handler(
            State(state),
            AxumPath(("machine".into(), path.into())),
            None,
            request,
        )
        .await;
        let Err(status) = result else {
            panic!("a non-reference-chat refusal must not carry the machine envelope");
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn accepts_target_when_session_is_colon_scoped() {
        // Given a workspace-scoped terminal session.
        let target = "/api/v1/terminal/workspace:session-1";
        // When checking the socket target.
        let valid = valid_socket_target(target);
        // Then the target is accepted.
        assert!(valid);
    }

    #[test]
    fn test_phase4_relay_valid_socket_target_allows_browser() {
        assert!(valid_socket_target("/api/v1/browser/b1"));
        assert!(valid_socket_target("/api/v1/browser/b-test_123.valid"));
        assert!(!valid_socket_target("/api/v1/browser/"));
        assert!(!valid_socket_target("/api/v1/browser/has/slash"));
    }

    #[test]
    fn protects_origin_when_gateway_sends_sensitive_headers() {
        // Given a gateway response that attempts to mutate shared-origin state.
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nCache-Control: public\r\nPragma: cache\r\nClear-Site-Data: *\r\nSet-Cookie: sid=secret\r\nService-Worker-Allowed: /\r\n\r\n";
        // When filtering the gateway response.
        let response = parse_http_response(raw, false, false).unwrap().unwrap();
        // Then caching and origin-mutating headers are blocked.
        assert_eq!(response.headers()["cache-control"], "no-store, private");
        assert_eq!(response.headers()["pragma"], "no-cache");
        for name in ["clear-site-data", "set-cookie", "service-worker-allowed"] {
            assert!(!response.headers().contains_key(name));
        }
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
        loop {
            let frame = timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            // Keepalive replies (tungstenite's automatic Pong plus the relay's own) carry no JSON.
            if matches!(frame, TMessage::Ping(_) | TMessage::Pong(_)) {
                continue;
            }
            return serde_json::from_str(frame.to_text().unwrap()).unwrap();
        }
    }

    async fn authenticate(
        base: &str,
        identity: &crate::remote::auth::MachineIdentity,
        wrong_signature: bool,
        stale: bool,
    ) -> (TestSocket, ControlAuthResponse) {
        authenticate_with_token(
            base,
            identity,
            wrong_signature,
            stale,
            Some("test-machine-token"),
        )
        .await
    }

    /// A control signature must be bound to the relay that issued the challenge.
    ///
    /// With a constant audience shared by every deployment, a signature captured by
    /// one relay authenticates the same machine on another. The relay announces its
    /// audience and requires it in the transcript, so a signature minted for a
    /// different relay is refused even though nonce, timestamp and key are valid.
    #[tokio::test]
    async fn a_control_signature_minted_for_another_relay_is_refused() {
        let identity = identity(9, "audience-machine");
        let store = tempfile::tempdir().unwrap();
        // This relay identifies itself as a specific origin rather than the shared
        // legacy constant.
        let state = RelayState::new_with_audience(
            vec!["enrollment".into()],
            store.path().join("keys.json"),
            "relay.example.test",
        )
        .unwrap();
        let (base, _running) = spawn_test_relay_with_state(state).await;

        let (mut socket, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/control"))
            .await
            .unwrap();
        let challenge: ControlChallenge = receive_json(&mut socket).await;
        assert_eq!(
            challenge.audience.as_deref(),
            Some("relay.example.test"),
            "the relay must announce the audience it will verify"
        );

        // An attacker replays a signature minted against a DIFFERENT relay, reusing
        // this relay's live nonce and timestamp.
        let auth = ControlAuth {
            enrollment_token: Some("enrollment".into()),
            machine_id: identity.machine_id.clone(),
            display_name: identity.display_name.clone(),
            public_key: identity.public_key.clone(),
            timestamp: challenge.timestamp,
            signature: crate::remote::auth::sign_control_challenge(
                &identity,
                "relay.attacker.test",
                &challenge.nonce,
                challenge.timestamp,
            )
            .unwrap(),
        };
        socket
            .send(TMessage::Text(serde_json::to_string(&auth).unwrap().into()))
            .await
            .unwrap();
        let response: ControlAuthResponse = receive_json(&mut socket).await;
        assert!(
            !response.success,
            "a signature minted for another relay must not authenticate here"
        );

        // The same machine authenticates when it signs THIS relay's audience.
        let (mut socket, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/control"))
            .await
            .unwrap();
        let challenge: ControlChallenge = receive_json(&mut socket).await;
        let auth = ControlAuth {
            enrollment_token: Some("enrollment".into()),
            machine_id: identity.machine_id.clone(),
            display_name: identity.display_name.clone(),
            public_key: identity.public_key.clone(),
            timestamp: challenge.timestamp,
            signature: crate::remote::auth::sign_control_challenge(
                &identity,
                challenge.audience.as_deref().unwrap(),
                &challenge.nonce,
                challenge.timestamp,
            )
            .unwrap(),
        };
        socket
            .send(TMessage::Text(serde_json::to_string(&auth).unwrap().into()))
            .await
            .unwrap();
        let response: ControlAuthResponse = receive_json(&mut socket).await;
        assert!(
            response.success,
            "signing the announced audience must authenticate"
        );
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
        assert!(verify_control_challenge(
            &owner.public_key,
            "machine-a",
            "relay",
            "nonce",
            42,
            &signature
        ));
        for (machine, audience, nonce, timestamp) in [
            ("machine-b", "relay", "nonce", 42),
            ("machine-a", "gateway", "nonce", 42),
            ("machine-a", "relay", "different", 42),
            ("machine-a", "relay", "nonce", 43),
        ] {
            assert!(!verify_control_challenge(
                &owner.public_key,
                machine,
                audience,
                nonce,
                timestamp,
                &signature
            ));
        }
        let legacy = sign_challenge(&owner, "nonce", 42).unwrap();
        assert!(!verify_control_challenge(
            &owner.public_key,
            "machine-a",
            "relay",
            "nonce",
            42,
            &legacy
        ));
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
        let (mut socket, response) =
            authenticate_with_token(&base, &owner, false, false, Some("enroll")).await;
        assert!(response.success);
        allocate(&mut socket).await;
        socket.close(None).await.unwrap();
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());

        let state = RelayState::new_with_key_store(vec!["enroll".into()], &path).unwrap();
        let (base, server) = spawn_test_relay_with_state(state).await;
        let (mut socket, response) =
            authenticate_with_token(&base, &owner, false, false, None).await;
        assert!(response.success);
        allocate(&mut socket).await;
        let impostor = identity(2, "durable-machine");
        let (_, response) =
            authenticate_with_token(&base, &impostor, false, false, Some("enroll")).await;
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
            enrollment_token: None,
            machine_id: "machine".into(),
            display_name: "test".into(),
            public_key: identity(1, "machine").public_key,
            signature: String::new(),
            timestamp: 0,
        };
        assert!(state.bind_machine_key(&auth).is_err());
        assert!(state.inner.machine_public_keys.lock().is_empty());
    }

    #[tokio::test]
    async fn a_server_side_bind_failure_does_not_count_toward_the_ip_lockout() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("blocked");
        let state = RelayState::new_with_key_store(vec![], blocked.join("keys.json")).unwrap();
        // The key store's directory cannot be created, so every bind fails on the relay's side.
        std::fs::write(&blocked, b"not a directory").unwrap();
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        for _ in 0..6 {
            let (_, response) = authenticate(&base, &identity(39, "disk-machine"), false, false).await;
            assert!(!response.success);
            assert!(response.error.unwrap().starts_with("Failed to "));
        }
        {
            let admission = state.inner.admission.lock();
            let tracker = &admission[&ip];
            assert_eq!(tracker.failures, 0);
            assert!(tracker.locked_until.is_none());
        }
        // A bad signature still counts.
        let (_, response) = authenticate(&base, &identity(39, "disk-machine"), true, false).await;
        assert!(!response.success);
        assert_eq!(state.inner.admission.lock()[&ip].failures, 1);
        server.abort();
    }

    #[tokio::test]
    async fn a_keepalive_before_control_auth_does_not_fail_the_handshake() {
        let (base, server) = spawn_test_relay().await;
        let identity = identity(45, "pinging-machine");
        let (mut socket, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/control"))
            .await
            .unwrap();
        let challenge: ControlChallenge = receive_json(&mut socket).await;
        socket.send(TMessage::Ping(b"early".to_vec().into())).await.unwrap();
        socket.send(TMessage::Pong(b"early".to_vec().into())).await.unwrap();
        let auth = ControlAuth {
            enrollment_token: Some("test-machine-token".into()),
            machine_id: identity.machine_id.clone(),
            display_name: identity.display_name.clone(),
            public_key: identity.public_key.clone(),
            timestamp: challenge.timestamp,
            signature: crate::remote::auth::sign_control_challenge(
                &identity,
                "relay",
                &challenge.nonce,
                challenge.timestamp,
            )
            .unwrap(),
        };
        socket
            .send(TMessage::Text(serde_json::to_string(&auth).unwrap().into()))
            .await
            .unwrap();
        // The relay answers the Ping with a Pong; skip it to reach the auth response.
        let response = loop {
            match timeout(Duration::from_secs(5), socket.next()).await.unwrap().unwrap().unwrap() {
                TMessage::Text(text) => break serde_json::from_str::<ControlAuthResponse>(&text).unwrap(),
                _ => continue,
            }
        };
        assert!(response.success, "{:?}", response.error);
        server.abort();
    }

    #[tokio::test]
    async fn a_silent_control_socket_is_closed_and_unregistered() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (mut daemon, auth) = authenticate(&base, &identity(46, "silent_machine"), false, false).await;
        assert!(auth.success);
        control_barrier(&mut daemon).await;
        // The socket task owns the grant receiver; it is dropped only after the task has
        // unregistered, so the sender closing is the signal that the registration is gone.
        let grant = state.inner.grant_channels.lock()["silent_machine"].clone();
        // Paused time auto-advances to the idle deadline once the relay waits on it.
        tokio::time::pause();
        assert_close(&mut daemon, 1001, "control channel idle").await;
        tokio::time::resume();
        timeout(Duration::from_secs(5), grant.closed()).await.unwrap();
        assert!(!state.inner.control_channels.lock().contains_key("silent_machine"));
        server.abort();
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

    #[test]
    fn the_sweeper_prunes_expired_pairing_admission_trackers() {
        let state = test_state(vec![]);
        let ip: IpAddr = "192.0.2.9".parse().unwrap();
        let expired = Instant::now()
            .checked_sub(ADMISSION_WINDOW + Duration::from_secs(1))
            .unwrap();
        for map in [&state.inner.admission, &state.inner.pairing_admission] {
            map.lock().insert(
                ip,
                AttemptTracker {
                    started: expired,
                    attempts: 3,
                    failures: 3,
                    locked_until: None,
                },
            );
        }
        // Admitting another address no longer scans the map; the sweeper owns pruning.
        assert!(state.admit("192.0.2.10".parse().unwrap(), Instant::now()));
        assert!(state.inner.admission.lock().contains_key(&ip));
        state.sweep_expired_sessions();
        assert!(!state.inner.admission.lock().contains_key(&ip));
        assert!(!state.inner.pairing_admission.lock().contains_key(&ip));
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
    /// Reads past keepalive frames to the next frame and asserts it is the expected close.
    /// No timeout of its own: callers on paused time rely on auto-advance to reach the relay's
    /// deadline, and a real-time caller wraps the call in one.
    async fn assert_close(socket: &mut TestSocket, code: u16, reason: &str) {
        loop {
            match socket.next().await {
                Some(Ok(TMessage::Ping(_) | TMessage::Pong(_))) => continue,
                Some(Ok(TMessage::Close(Some(close)))) => {
                    assert_eq!(u16::from(close.code), code);
                    assert_eq!(close.reason.as_str(), reason);
                    return;
                }
                other => panic!("expected close {code} {reason:?}, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn a_half_whose_machine_never_connects_is_closed_with_a_reason() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (_generation, mut notices, _grants) = state.register_control_channel("lonely".into());
        assert!(state.notify_incoming_session("lonely", "lonely-session"));
        notices.try_recv().unwrap();
        let (mut client, _) =
            tokio_tungstenite::connect_async(format!("{base}/tunnel/client/lonely-session"))
                .await
                .unwrap();
        // Paused time auto-advances to the pairing deadline once every task is idle.
        tokio::time::pause();
        assert_close(&mut client, 1013, "machine did not connect").await;
        server.abort();
    }

    #[tokio::test]
    async fn a_session_past_its_lifetime_tells_the_browser_why() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (_generation, mut notices, _grants) = state.register_control_channel("aging".into());
        assert!(state.notify_incoming_session("aging", "aging-session"));
        notices.try_recv().unwrap();
        let (mut client, _) =
            tokio_tungstenite::connect_async(format!("{base}/tunnel/client/aging-session"))
                .await
                .unwrap();
        let (mut data, _) =
            tokio_tungstenite::connect_async(format!("{base}/tunnel/data/aging-session"))
                .await
                .unwrap();
        // The splice is live once a frame crosses it.
        client.send(TMessage::Text("ping".into())).await.unwrap();
        assert_eq!(data.next().await.unwrap().unwrap(), TMessage::Text("ping".into()));
        tokio::time::pause();
        assert_close(&mut client, 1001, "session lifetime exceeded").await;
        server.abort();
    }

    #[tokio::test]
    async fn a_browser_socket_whose_host_goes_away_is_closed_with_a_reason() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (_generation, mut notices, _grants) =
            state.register_control_channel("browser-machine".into());
        let ticket = issue_test_ticket(&state, "/api/v1/events").await;
        let browser = tokio::spawn({
            let base = base.clone();
            async move {
                tokio_tungstenite::connect_async(format!(
                    "{base}/host/browser-machine/api/v1/events?ticket={}",
                    ticket.ticket
                ))
                .await
                .unwrap()
                .0
            }
        });
        let notice = timeout(Duration::from_secs(5), notices.recv()).await.unwrap().unwrap();
        let (mut data, _) =
            tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{}", notice.session_id))
                .await
                .unwrap();
        let mut browser = browser.await.unwrap();
        // The relay's inner WebSocket upgrade reaches the host; the host then goes away.
        let upgrade = timeout(Duration::from_secs(5), data.next()).await.unwrap().unwrap().unwrap();
        assert!(upgrade.into_data().starts_with(b"GET /api/v1/events HTTP/1.1\r\n"));
        data.close(None).await.unwrap();
        timeout(Duration::from_secs(5), assert_close(&mut browser, 1011, "host disconnected"))
            .await
            .unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn a_burst_of_frames_crosses_the_splice_intact_and_in_order() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (_generation, mut notices, _grants) = state.register_control_channel("burst".into());
        assert!(state.notify_incoming_session("burst", "burst-session"));
        notices.try_recv().unwrap();
        let (mut client, _) =
            tokio_tungstenite::connect_async(format!("{base}/tunnel/client/burst-session"))
                .await
                .unwrap();
        let (mut data, _) =
            tokio_tungstenite::connect_async(format!("{base}/tunnel/data/burst-session"))
                .await
                .unwrap();
        // More frames than one unflushed batch, queued before any is read.
        for n in 0..200u32 {
            client.feed(TMessage::Binary(n.to_be_bytes().to_vec().into())).await.unwrap();
        }
        client.flush().await.unwrap();
        for n in 0..200u32 {
            let frame = timeout(Duration::from_secs(5), data.next()).await.unwrap().unwrap().unwrap();
            assert_eq!(frame.into_data().as_ref(), n.to_be_bytes().as_slice());
        }
        // A lone frame after the burst is flushed without another frame behind it.
        client.send(TMessage::Text("tail".into())).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(5), data.next()).await.unwrap().unwrap().unwrap(),
            TMessage::Text("tail".into())
        );
        server.abort();
    }

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
        let (old, _old_rx, _grants) = state.register_control_channel("tok".into());
        let (new, mut rx, _grants) = state.register_control_channel("tok".into());
        state.unregister_control_channel("tok", old);
        assert!(!state.issue_session("tok", "stale", Some(old), false));
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
        // The paired "session" above is active and no longer charges the waiting budget.
        for n in 0..MAX_PENDING_SESSIONS_PER_MACHINE {
            assert!(state.notify_incoming_session("tok", &format!("session-{n}")));
            rx.try_recv().unwrap();
        }
        assert!(!state.notify_incoming_session("tok", "overflow"));
        state.unregister_control_channel("tok", new);
        assert!(!state.notify_incoming_session("tok", "no-control"));
        drop((waiting, handoff));
    }

    #[test]
    fn paired_sessions_do_not_consume_the_waiting_budget() {
        let state = test_state(vec![]);
        let mut receivers = Vec::new();
        let mut generations = HashMap::new();
        for machine in 0..=(MAX_PENDING_SESSIONS / MAX_PENDING_SESSIONS_PER_MACHINE) {
            let name = format!("machine-{machine}");
            let (generation, rx, grants) = state.register_control_channel(name.clone());
            generations.insert(name, generation);
            receivers.push((rx, grants));
        }
        // Fill machine-0 with live (paired) sessions well past the per-machine waiting cap.
        for n in 0..(MAX_PENDING_SESSIONS_PER_MACHINE * 2) {
            let id = format!("live-{n}");
            assert_eq!(
                state.try_issue_session_of_kind("machine-0", &id, None, false, SessionKind::Http),
                Ok(())
            );
            state.inner.pending_sessions.lock().get_mut(&id).unwrap().active = true;
            receivers[0].0.try_recv().unwrap();
        }
        // Waiting sessions are capped per machine with a typed reason.
        for n in 0..MAX_PENDING_SESSIONS_PER_MACHINE {
            assert!(state.notify_incoming_session("machine-0", &format!("wait-{n}")));
        }
        assert_eq!(
            state.try_issue_session_of_kind("machine-0", "over", None, false, SessionKind::Http),
            Err(IssueRejection::Busy)
        );
        // Another machine is unaffected by machine-0's budget.
        assert!(state.notify_incoming_session("machine-1", "other"));
        assert_eq!(
            state.try_issue_session_of_kind("missing", "x", None, false, SessionKind::Http),
            Err(IssueRejection::Offline)
        );
        assert_eq!(
            state.try_issue_session_of_kind(
                "machine-1",
                "stale",
                Some(generations["machine-1"] + 1_000),
                false,
                SessionKind::Http
            ),
            Err(IssueRejection::StaleGeneration)
        );
    }

    #[tokio::test]
    async fn test_relay_cors_preflight_and_headers() {
        let (base, server) = spawn_test_relay().await;
        let http_base = base.replace("ws://", "http://");
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        for (endpoint, send_origin) in [
            ("/api/v1/pair/exchange", false),
            ("/api/v1/pair/exchange", true),
            ("/host/m1/api/v1/workspace/state", false),
            ("/host/m1/api/v1/workspace/state", true),
        ] {
            let url = format!("{http_base}{endpoint}");
            let mut req = client.request(reqwest::Method::OPTIONS, &url);
            if send_origin {
                req = req.header("Origin", "https://relay.ferryx.dev");
                req = req.header("Access-Control-Request-Method", "POST");
                req = req.header(
                    "Access-Control-Request-Headers",
                    "authorization, content-type",
                );
            }
            let res = req.send().await.expect("OPTIONS request should succeed");
            assert_eq!(
                res.status(),
                reqwest::StatusCode::OK,
                "OPTIONS {endpoint} must return 200 OK"
            );
            assert_eq!(
                res.headers()
                    .get("access-control-allow-origin")
                    .and_then(|v| v.to_str().ok()),
                Some("*")
            );
            assert_eq!(
                res.headers()
                    .get("access-control-allow-methods")
                    .and_then(|v| v.to_str().ok()),
                Some("GET, POST, PUT, DELETE, OPTIONS")
            );
            assert_eq!(
                res.headers()
                    .get("access-control-allow-headers")
                    .and_then(|v| v.to_str().ok()),
                Some("Authorization, Content-Type, X-Device-Name, X-Requested-With, Accept, X-Ferryx-Client-Build")
            );
            assert_eq!(
                res.headers()
                    .get("access-control-max-age")
                    .and_then(|v| v.to_str().ok()),
                Some("86400")
            );
        }

        // Verify actual requests also carry the CORS headers
        let res = client
            .post(format!("{http_base}/api/v1/pair/exchange"))
            .body("{}")
            .send()
            .await
            .expect("POST request should complete");
        assert_eq!(
            res.headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("*")
        );
        assert_eq!(
            res.headers()
                .get("access-control-allow-methods")
                .and_then(|v| v.to_str().ok()),
            Some("GET, POST, PUT, DELETE, OPTIONS")
        );
        assert_eq!(
            res.headers()
                .get("access-control-allow-headers")
                .and_then(|v| v.to_str().ok()),
            Some("Authorization, Content-Type, X-Device-Name, X-Requested-With, Accept, X-Ferryx-Client-Build")
        );
        assert_eq!(
            res.headers()
                .get("access-control-max-age")
                .and_then(|v| v.to_str().ok()),
            Some("86400")
        );

        server.abort();
    }

    /// The account and grant routers are merged into the relay router *after*
    /// `Router::layer` is called, and axum only wraps the routes that exist at
    /// that moment. If the CORS middleware misses them, a browser client served
    /// from another origin (the desktop app, or the remote client hosted by the
    /// app) has every account request blocked by the preflight, so this check
    /// must fail whenever the merged routers stop receiving the layer.
    #[tokio::test]
    async fn test_merged_account_and_grant_routes_carry_cors_headers() {
        let state = test_state(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let account = test_account_state(
            tmp.path(),
            "user-1",
            "session-token",
            "owned-machine",
            "other-machine",
        );
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        for endpoint in [
            "/api/account/v1/login/request",
            "/api/account/v1/login/consume",
            "/api/account/v1/machines",
            "/api/account/v1/machines/enroll",
            "/api/v1/attach/grant",
        ] {
            let res = client
                .request(reqwest::Method::OPTIONS, format!("{http_base}{endpoint}"))
                .header("Origin", "tauri://localhost")
                .header("Access-Control-Request-Method", "POST")
                .header(
                    "Access-Control-Request-Headers",
                    "authorization, content-type",
                )
                .send()
                .await
                .expect("preflight request should complete");
            assert_eq!(
                res.status(),
                reqwest::StatusCode::OK,
                "OPTIONS {endpoint} must be answered by the CORS middleware"
            );
            assert_eq!(
                res.headers()
                    .get("access-control-allow-origin")
                    .and_then(|value| value.to_str().ok()),
                Some("*"),
                "OPTIONS {endpoint} must carry access-control-allow-origin"
            );
            let allow_headers = res
                .headers()
                .get("access-control-allow-headers")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_ascii_lowercase();
            assert!(
                allow_headers.contains("content-type"),
                "OPTIONS {endpoint} must allow the content-type request header"
            );
        }

        // A real (non-preflight) account response must carry the header too,
        // otherwise the browser discards a response the server did produce.
        let res = client
            .post(format!("{http_base}/api/account/v1/login/request"))
            .header("Origin", "tauri://localhost")
            .json(&serde_json::json!({ "email": "not-an-email" }))
            .send()
            .await
            .expect("login request should complete");
        assert_eq!(res.status(), reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(
            res.headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("*"),
            "an account response must carry access-control-allow-origin"
        );

        server.abort();
    }

    #[tokio::test]
    async fn test_p01_regression_pair_exchange_5xx_failure_allows_retry() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let owner = identity(41, "p01_daemon_5xx");
        let (mut daemon_control, auth) = authenticate(&base, &owner, false, false).await;
        assert!(auth.success);
        register_security_pin(&mut daemon_control, "p01_daemon_5xx").await;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        // 1. First attempt: client claims PIN, downstream daemon responds with 500
        let request1 = client
            .post(format!(
                "{}/api/v1/pair/exchange",
                base.replace("ws://", "http://")
            ))
            .body(r#"{"pin":"123456","deviceName":"retry-client"}"#)
            .send();

        let daemon_handle_500 = async {
            let notice: IncomingSessionNotice = receive_json(&mut daemon_control).await;
            let (mut data, _) = tokio_tungstenite::connect_async(format!(
                "{base}/tunnel/data/{}",
                notice.session_id
            ))
            .await
            .unwrap();
            let _frame = timeout(Duration::from_secs(5), data.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            data.send(TMessage::Binary(
                b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec()
                    .into(),
            ))
            .await
            .unwrap();
            let _ = timeout(Duration::from_secs(5), data.next()).await;
        };

        let (res1, ()) = timeout(Duration::from_secs(5), async {
            tokio::join!(request1, daemon_handle_500)
        })
        .await
        .unwrap();
        let res1 = res1.unwrap();
        assert_eq!(res1.status(), reqwest::StatusCode::INTERNAL_SERVER_ERROR);

        // Pre-fix code leaves the registration in Claimed; post-fix rolls back to Ready.
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Ready,
            "registration must roll back to Ready after downstream 5xx failure"
        );

        // 2. Retry with the SAME PIN: downstream daemon succeeds with 200 OK
        let request2 = client
            .post(format!(
                "{}/api/v1/pair/exchange",
                base.replace("ws://", "http://")
            ))
            .body(r#"{"pin":"123456","deviceName":"retry-client"}"#)
            .send();

        let base_clone = base.clone();
        let daemon_task = tokio::spawn(async move {
            let notice: IncomingSessionNotice = receive_json(&mut daemon_control).await;
            let (mut data, _) = tokio_tungstenite::connect_async(format!(
                "{base_clone}/tunnel/data/{}",
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
            data.send(TMessage::Binary(
                b"HTTP/1.1 200 OK\r\nContent-Length: 29\r\nConnection: close\r\n\r\n{\"token\":\"test-device-token\"}"
                    .to_vec()
                    .into(),
            ))
            .await
            .unwrap();
            let _ = timeout(Duration::from_secs(5), data.next()).await;
        });

        let res2 = request2.await.unwrap();
        assert_eq!(res2.status(), reqwest::StatusCode::OK);
        assert_eq!(
            res2.text().await.unwrap(),
            r#"{"token":"test-device-token"}"#
        );
        daemon_task.await.unwrap();
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Consumed
        );

        server.abort();
    }

    #[tokio::test]
    async fn a_host_accepted_exchange_with_an_unreadable_answer_still_spends_the_code() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let owner = identity(43, "p01_daemon_garbled");
        let (mut daemon_control, auth) = authenticate(&base, &owner, false, false).await;
        assert!(auth.success);
        register_security_pin(&mut daemon_control, "p01_daemon_garbled").await;
        let claim = security_claim(&state, "123456");
        let host = async {
            let notice: IncomingSessionNotice = receive_json(&mut daemon_control).await;
            let (mut data, _) =
                tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{}", notice.session_id))
                    .await
                    .unwrap();
            let _request = timeout(Duration::from_secs(5), data.next()).await.unwrap();
            // The host consumed the code and answered 2xx, but the body is not a token.
            data.send(TMessage::Binary(
                b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot json"
                    .to_vec()
                    .into(),
            ))
            .await
            .unwrap();
            let _ = timeout(Duration::from_secs(5), data.next()).await;
        };
        let (status, ()) = tokio::join!(claim, host);
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Consumed,
            "a code the host already consumed must not be offered again"
        );
        server.abort();
    }

    #[tokio::test]
    async fn test_p01_regression_pair_exchange_transport_error_allows_retry() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let owner = identity(42, "p01_daemon_transport");
        let (mut daemon_control, auth) = authenticate(&base, &owner, false, false).await;
        assert!(auth.success);
        register_security_pin(&mut daemon_control, "p01_daemon_transport").await;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        // 1. First attempt: downstream daemon closes data connection without sending HTTP response
        let request1 = client
            .post(format!(
                "{}/api/v1/pair/exchange",
                base.replace("ws://", "http://")
            ))
            .body(r#"{"pin":"123456","deviceName":"retry-client"}"#)
            .send();

        let daemon_handle_disconnect = async {
            let notice: IncomingSessionNotice = receive_json(&mut daemon_control).await;
            let (data, _) = tokio_tungstenite::connect_async(format!(
                "{base}/tunnel/data/{}",
                notice.session_id
            ))
            .await
            .unwrap();
            drop(data);
        };

        let (res1, ()) = timeout(Duration::from_secs(5), async {
            tokio::join!(request1, daemon_handle_disconnect)
        })
        .await
        .unwrap();
        let res1 = res1.unwrap();
        assert_eq!(res1.status(), reqwest::StatusCode::BAD_GATEWAY);

        // Pre-fix code leaves the registration in Claimed; post-fix rolls back to Ready.
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Ready,
            "registration must roll back to Ready after transport error"
        );

        // 2. Retry with the SAME PIN: downstream daemon succeeds with 200 OK
        let request2 = client
            .post(format!(
                "{}/api/v1/pair/exchange",
                base.replace("ws://", "http://")
            ))
            .body(r#"{"pin":"123456","deviceName":"retry-client"}"#)
            .send();

        let base_clone = base.clone();
        let daemon_task = tokio::spawn(async move {
            let notice: IncomingSessionNotice = receive_json(&mut daemon_control).await;
            let (mut data, _) = tokio_tungstenite::connect_async(format!(
                "{base_clone}/tunnel/data/{}",
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
            data.send(TMessage::Binary(
                b"HTTP/1.1 200 OK\r\nContent-Length: 29\r\n\r\n{\"token\":\"test-device-token\"}"
                    .to_vec()
                    .into(),
            ))
            .await
            .unwrap();
            let _ = timeout(Duration::from_secs(5), data.next()).await;
        });

        let res2 = request2.await.unwrap();
        assert_eq!(res2.status(), reqwest::StatusCode::OK);
        assert_eq!(
            res2.text().await.unwrap(),
            r#"{"token":"test-device-token"}"#
        );
        daemon_task.await.unwrap();
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Consumed
        );

        server.abort();
    }

    #[tokio::test]
    async fn test_p01_edge_concurrent_double_claim_exactly_one_wins() {
        let state = test_state(vec![]);
        let (generation, _notices, _grants) = state.register_control_channel("machine_alpha".into());
        assert!(state
            .register_pairing(
                "machine_alpha",
                generation,
                security_registration("machine_alpha"),
            )
            .is_ok());

        let req1 = PublicPairExchangeRequest {
            pin: Some("123456".into()),
            code: None,
            pairing_token: None,
            device_name: "client-one".into(),
            installation_id: None,
        };
        let req2 = PublicPairExchangeRequest {
            pin: Some("123456".into()),
            code: None,
            pairing_token: None,
            device_name: "client-two".into(),
            installation_id: None,
        };

        let ip1: IpAddr = "192.0.2.10".parse().unwrap();
        let ip2: IpAddr = "192.0.2.20".parse().unwrap();

        let state1 = state.clone();
        let state2 = state.clone();

        let (claim1, claim2) = tokio::join!(
            tokio::spawn(async move { state1.claim_pairing(ip1, &req1, None) }),
            tokio::spawn(async move { state2.claim_pairing(ip2, &req2, None) })
        );

        let res1 = claim1.unwrap();
        let res2 = claim2.unwrap();

        let one_won = (res1.is_ok() && res2.is_err()) || (res1.is_err() && res2.is_ok());
        assert!(
            one_won,
            "exactly one claim must win when two requests claim the same PIN concurrently"
        );

        let err = match (&res1, &res2) {
            (Err(e), Ok(_)) | (Ok(_), Err(e)) => *e,
            _ => panic!("exactly one claim must win"),
        };
        assert_eq!(err, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_p01_edge_rollback_must_not_resurrect_consumed() {
        let state = test_state(vec![]);
        let (generation, _notices, _grants) = state.register_control_channel("machine_alpha".into());
        let reg = security_registration("machine_alpha");
        assert!(state
            .register_pairing("machine_alpha", generation, reg.clone())
            .is_ok());

        let req = PublicPairExchangeRequest {
            pin: Some("123456".into()),
            code: None,
            pairing_token: None,
            device_name: "client-consumed".into(),
            installation_id: None,
        };
        let ip: IpAddr = "192.0.2.30".parse().unwrap();

        let mut claim = state
            .claim_pairing(ip, &req, None)
            .expect("claim should succeed");
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Claimed
        );

        let claim_fence = claim.claim_fence;
        // Downstream succeeds: commit marks registration as Consumed
        claim.commit();
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Consumed
        );

        // Attempt rollback (e.g. from an errant stale guard or duplicate error path)
        state.rollback_pairing("123456", &reg.pairing_token, generation, claim_fence);
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Consumed,
            "rollback must never resurrect a Consumed registration back to Ready"
        );

        // A consumed registration cannot be claimed again
        assert_eq!(
            state.claim_pairing(ip, &req, None).err(),
            Some(StatusCode::NOT_FOUND)
        );
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Consumed
        );
    }

    #[tokio::test]
    async fn test_p01_edge_lease_expiry_enforced_after_rollback() {
        let state = test_state(vec![]);
        let (generation, _notices, _grants) = state.register_control_channel("machine_alpha".into());
        let reg = security_registration("machine_alpha");
        assert!(state
            .register_pairing("machine_alpha", generation, reg.clone())
            .is_ok());

        let req = PublicPairExchangeRequest {
            pin: Some("123456".into()),
            code: None,
            pairing_token: None,
            device_name: "client-expiry".into(),
            installation_id: None,
        };
        let ip: IpAddr = "192.0.2.40".parse().unwrap();

        // 1. Initial claim succeeds, downstream fails, rollback to Ready
        let mut claim = state
            .claim_pairing(ip, &req, None)
            .expect("first claim should succeed");
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Claimed
        );
        claim.rollback();
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Ready
        );

        // 2. Lease expires: inject clock advance by setting expires_at into the past (no sleep)
        state
            .inner
            .pairings
            .lock()
            .get_mut("123456")
            .unwrap()
            .registration
            .expires_at = current_time_secs() - 10;

        // 3. Retry after rollback MUST be rejected because lease expired
        let ip2: IpAddr = "192.0.2.41".parse().unwrap();
        assert_eq!(
            state.claim_pairing(ip2, &req, None).err(),
            Some(StatusCode::NOT_FOUND),
            "claim must be rejected when lease expiry is reached after rollback"
        );
    }

    #[tokio::test]
    async fn test_p01_edge_claim_rollback_when_already_expired_marks_expired() {
        let state = test_state(vec![]);
        let (generation, _notices, _grants) = state.register_control_channel("machine_alpha".into());
        let reg = security_registration("machine_alpha");
        assert!(state
            .register_pairing("machine_alpha", generation, reg.clone())
            .is_ok());

        let req = PublicPairExchangeRequest {
            pin: Some("123456".into()),
            code: None,
            pairing_token: None,
            device_name: "client-in-flight-expiry".into(),
            installation_id: None,
        };
        let ip: IpAddr = "192.0.2.50".parse().unwrap();

        let mut claim = state
            .claim_pairing(ip, &req, None)
            .expect("claim should succeed");
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Claimed
        );

        // Registration lease expires while downstream exchange was in flight
        state
            .inner
            .pairings
            .lock()
            .get_mut("123456")
            .unwrap()
            .registration
            .expires_at = current_time_secs() - 10;

        // Downstream fails, triggering rollback
        claim.rollback();

        // Expired registration must not become Ready
        assert_ne!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Ready,
            "expired registration must not be rolled back to Ready"
        );

        // Attempting to claim again is rejected
        assert_eq!(
            state.claim_pairing(ip, &req, None).err(),
            Some(StatusCode::NOT_FOUND)
        );
    }

    #[tokio::test]
    async fn test_p01_edge_claim_lease_timeout_allows_reclaim() {
        let state = test_state(vec![]);
        let (generation, _notices, _grants) = state.register_control_channel("machine_alpha".into());
        let reg = security_registration("machine_alpha");
        assert!(state
            .register_pairing("machine_alpha", generation, reg.clone())
            .is_ok());

        let req = PublicPairExchangeRequest {
            pin: Some("123456".into()),
            code: None,
            pairing_token: None,
            device_name: "client-abandoned".into(),
            installation_id: None,
        };
        let ip1: IpAddr = "192.0.2.60".parse().unwrap();
        let mut claim1 = state
            .claim_pairing(ip1, &req, None)
            .expect("first claim should succeed");
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Claimed
        );

        // Disarm guard so dropping claim1 doesn't invoke rollback,
        // simulating an abandoned in-flight claim lease (e.g. hung worker).
        claim1.disarm();
        drop(claim1);

        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Claimed
        );

        // While claim lease is still active, another claim is refused
        let ip2: IpAddr = "192.0.2.61".parse().unwrap();
        assert_eq!(
            state.claim_pairing(ip2, &req, None).err(),
            Some(StatusCode::NOT_FOUND),
            "concurrent claim must be refused while temporary lease is active"
        );

        // Inject clock / lease expiration: expire the claim lease
        state
            .inner
            .pairings
            .lock()
            .get_mut("123456")
            .unwrap()
            .claim_expires_at = Some(Instant::now() - Duration::from_secs(1));

        // Now that the temporary claim lease has expired, a subsequent claim succeeds
        let claim2 = state
            .claim_pairing(ip2, &req, None)
            .expect("claim should succeed after claim lease expiry");
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Claimed
        );
        assert_ne!(claim2.claim_fence, 0);
    }

    #[tokio::test]
    async fn test_p01_edge_stale_claim_fence_cannot_rollback_or_consume_newer_claim() {
        let state = test_state(vec![]);
        let (generation, _notices, _grants) = state.register_control_channel("machine_alpha".into());
        let reg = security_registration("machine_alpha");
        assert!(state
            .register_pairing("machine_alpha", generation, reg.clone())
            .is_ok());

        let req = PublicPairExchangeRequest {
            pin: Some("123456".into()),
            code: None,
            pairing_token: None,
            device_name: "client-fencing".into(),
            installation_id: None,
        };
        let ip1: IpAddr = "192.0.2.70".parse().unwrap();
        let mut claim1 = state
            .claim_pairing(ip1, &req, None)
            .expect("first claim succeeds");
        claim1.disarm();
        let stale_fence = claim1.claim_fence;
        drop(claim1);

        // Expire first claim lease and re-claim
        state
            .inner
            .pairings
            .lock()
            .get_mut("123456")
            .unwrap()
            .claim_expires_at = Some(Instant::now() - Duration::from_secs(1));

        let ip2: IpAddr = "192.0.2.71".parse().unwrap();
        let mut claim2 = state
            .claim_pairing(ip2, &req, None)
            .expect("second claim succeeds");
        let active_fence = claim2.claim_fence;
        assert_ne!(stale_fence, active_fence);

        // Stale claim tries to roll back:
        state.rollback_pairing("123456", &reg.pairing_token, generation, stale_fence);
        // State must still be Claimed under the active fence!
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Claimed
        );
        assert_eq!(
            state.inner.pairings.lock()["123456"].claim_fence,
            active_fence
        );

        // Stale claim tries to consume:
        state.consume_pairing("123456", &reg.pairing_token, generation, stale_fence);
        // State must still be Claimed under active fence, NOT consumed!
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Claimed
        );

        // Active claim commits:
        claim2.commit();
        assert_eq!(
            state.inner.pairings.lock()["123456"].state,
            PairingState::Consumed
        );
    }

    #[tokio::test]
    async fn test_relay_504_stage_discriminator_data_pairing_timeout() {
        tokio::time::pause();
        let state = test_state(vec![]);
        let (generation, mut notices, _grants) =
            state.register_control_channel("machine_stage_1".into());

        let http_fut = proxy_http(
            &state,
            "machine_stage_1",
            Some(generation),
            Method::GET,
            "/api/v1/capabilities",
            HeaderMap::new(),
            Vec::new(),
        );
        tokio::pin!(http_fut);

        let notice = tokio::select! {
            res = &mut http_fut => panic!("http_fut completed prematurely: {:?}", res.map(|r| r.status())),
            n = notices.recv() => n.expect("session notice must be pushed"),
        };
        assert!(!notice.session_id.is_empty());

        tokio::time::advance(SESSION_PAIRING_TIMEOUT + Duration::from_secs(1)).await;

        let res = http_fut
            .await
            .expect("proxy_http returns Ok(Response) for stage timeout");
        assert_eq!(res.status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(
            res.headers()
                .get(RELAY_STAGE_HEADER)
                .and_then(|v| v.to_str().ok()),
            Some(RELAY_STAGE_DATA_PAIRING_TIMEOUT)
        );
        state.unregister_control_channel("machine_stage_1", generation);
    }

    #[tokio::test]
    async fn a_waiter_retired_by_the_sweeper_reports_the_data_pairing_timeout() {
        let state = test_state(vec![]);
        let (generation, mut notices, _grants) =
            state.register_control_channel("machine_stage_swept".into());
        let http_fut = proxy_http(
            &state,
            "machine_stage_swept",
            Some(generation),
            Method::GET,
            "/api/v1/capabilities",
            HeaderMap::new(),
            Vec::new(),
        );
        tokio::pin!(http_fut);
        let notice = tokio::select! {
            res = &mut http_fut => panic!("completed prematurely: {:?}", res.map(|r| r.status())),
            n = notices.recv() => n.expect("session notice"),
        };
        // The sweeper retires the entry (as it does at SESSION_PAIRING_TIMEOUT) before the
        // waiter's own timer fires.
        state.inner.pending_sessions.lock().remove(&notice.session_id);
        let res = http_fut.await.expect("stage timeout is a response");
        assert_eq!(res.status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(
            res.headers().get(RELAY_STAGE_HEADER).and_then(|v| v.to_str().ok()),
            Some(RELAY_STAGE_DATA_PAIRING_TIMEOUT)
        );
    }

    #[tokio::test]
    async fn an_offline_machine_answers_404_with_a_typed_envelope() {
        let state = test_state(vec![]);
        let response = proxy_http(
            &state,
            "nobody-home",
            None,
            Method::GET,
            "/api/v1/capabilities",
            HeaderMap::new(),
            Vec::new(),
        )
        .await
        .expect("a relay refusal is a typed response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body: serde_json::Value = serde_json::from_slice(
            &to_bytes(response.into_body(), MAX_MESSAGE_SIZE).await.unwrap(),
        )
        .unwrap();
        assert_eq!(body["error"]["code"], "MACHINE_OFFLINE");
        assert!(relay_refusal(StatusCode::GONE).is_some());
        assert_eq!(relay_refusal(StatusCode::SERVICE_UNAVAILABLE).unwrap().status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(relay_refusal(StatusCode::BAD_GATEWAY).is_none());
    }

    #[tokio::test]
    async fn a_budget_spent_before_any_channel_attached_reports_the_pairing_stage() {
        tokio::time::pause();
        let state = test_state(vec![]);
        let (generation, mut notices, _grants) =
            state.register_control_channel("machine_stage_budget".into());
        // A short agent-control wait's budget runs out before the pairing timeout, while the
        // machine never attaches a data channel.
        let path = "/api/v1/agent-control/sessions/s1/wait?timeoutMs=1000";
        let budget = agent_control_wait_budget(path).expect("wait path has its own budget");
        assert!(budget < SESSION_PAIRING_TIMEOUT, "the budget must expire before pairing does");
        let http_fut = proxy_http(
            &state,
            "machine_stage_budget",
            Some(generation),
            Method::GET,
            path,
            HeaderMap::new(),
            Vec::new(),
        );
        tokio::pin!(http_fut);
        tokio::select! {
            res = &mut http_fut => panic!("completed prematurely: {:?}", res.map(|r| r.status())),
            n = notices.recv() => { n.expect("session notice"); }
        }
        let res = http_fut.await.expect("stage timeout is a response");
        assert_eq!(res.status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(
            res.headers().get(RELAY_STAGE_HEADER).and_then(|v| v.to_str().ok()),
            Some(RELAY_STAGE_DATA_PAIRING_TIMEOUT)
        );
    }

    #[tokio::test]
    async fn test_relay_504_stage_discriminator_upstream_transfer_timeout() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_state(state.clone()).await;
        let (generation, mut notices, _grants) =
            state.register_control_channel("machine_stage_2".into());

        let http_fut = proxy_http(
            &state,
            "machine_stage_2",
            Some(generation),
            Method::GET,
            "/api/v1/workspace/projects",
            HeaderMap::new(),
            Vec::new(),
        );
        tokio::pin!(http_fut);

        let notice = tokio::select! {
            res = &mut http_fut => panic!("http_fut completed prematurely: {:?}", res.map(|r| r.status())),
            n = notices.recv() => n.expect("session notice must be pushed"),
        };
        let (mut data_socket, _) = tokio_tungstenite::connect_async(format!(
            "{base}/tunnel/data/{}",
            notice.session_id
        ))
        .await
        .expect("data socket connection must succeed");

        let msg = tokio::select! {
            res = &mut http_fut => panic!("http_fut completed prematurely: {:?}", res.map(|r| r.status())),
            frame = data_socket.next() => frame.expect("data socket stream active").expect("frame ok"),
        };
        assert!(msg
            .into_data()
            .starts_with(b"GET /api/v1/workspace/projects HTTP/1.1\r\n"));

        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(41)).await;

        let res = http_fut
            .await
            .expect("proxy_http returns Ok(Response) for stage timeout");
        assert_eq!(res.status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(
            res.headers()
                .get(RELAY_STAGE_HEADER)
                .and_then(|v| v.to_str().ok()),
            Some(RELAY_STAGE_UPSTREAM_TRANSFER_TIMEOUT)
        );

        state.unregister_control_channel("machine_stage_2", generation);
        server.abort();
    }

    #[tokio::test]
    async fn test_install_script_and_download_cli_routes() {
        let _guard = DOWNLOAD_DIR_ENV_LOCK.lock().await;
        let state = RelayState::new(vec!["test-machine-token".to_string()]);
        let (url, _handle) = spawn_test_relay_with_state(state).await;
        let http_url = url.replace("ws://", "http://");

        let client = reqwest::Client::new();
        let res = client.get(format!("{http_url}/install.sh")).send().await.unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        let script = res.text().await.unwrap();
        assert!(
            script.contains("Ferryx CLI Installer"),
            "install.sh must be the ferryx installer script, got: {script}"
        );

        let ps1_res = client.get(format!("{http_url}/install.ps1")).send().await.unwrap();
        assert_eq!(ps1_res.status(), reqwest::StatusCode::OK);
        let ps1_script = ps1_res.text().await.unwrap();
        assert!(
            ps1_script.contains("ferryx-cli-windows-amd64.exe"),
            "install.ps1 must reference the windows artifact, got: {ps1_script}"
        );

        // Stage an artifact so the download route must serve bytes rather than redirect.
        let staging = tempfile::tempdir().expect("staging dir");
        let artifact_bytes = b"\x7fELF-ferryx-cli-test-bytes";
        std::fs::write(staging.path().join("ferryx-cli-linux-amd64"), artifact_bytes)
            .expect("stage artifact");
        // SAFETY: this test owns the variable for the duration of the request; no other
        // test in this module reads FERRYX_DOWNLOADS_DIR.
        unsafe {
            std::env::set_var("FERRYX_DOWNLOADS_DIR", staging.path());
        }

        let staged = client
            .get(format!("{http_url}/download/ferryx-cli-linux-amd64"))
            .send()
            .await
            .expect("download request");
        assert_eq!(staged.status(), reqwest::StatusCode::OK);
        assert_eq!(
            staged.headers().get("content-disposition").and_then(|v| v.to_str().ok()),
            Some("attachment; filename=\"ferryx-cli-linux-amd64\""),
            "the saved file keeps its artifact name (and .exe on Windows)"
        );
        assert_eq!(
            staged.headers().get("content-length").and_then(|v| v.to_str().ok()),
            Some(artifact_bytes.len().to_string().as_str())
        );
        let served = staged.bytes().await.expect("download body");
        assert_eq!(
            served.as_ref(),
            artifact_bytes.as_slice(),
            "download route must serve the staged artifact bytes"
        );

        unsafe {
            std::env::remove_var("FERRYX_DOWNLOADS_DIR");
        }

        // Path traversal must be rejected rather than proxied.
        let rejected = client
            .get(format!("{http_url}/download/..%2F..%2Fetc%2Fpasswd"))
            .send()
            .await
            .expect("traversal request");
        assert_eq!(rejected.status(), reqwest::StatusCode::BAD_REQUEST);

        // A file that exists in the staging dir must still be refused by name.
        std::fs::write(staging.path().join("private-notes.txt"), b"secret")
            .expect("stage secret");
        unsafe {
            std::env::set_var("FERRYX_DOWNLOADS_DIR", staging.path());
        }
        let secret = client
            .get(format!("{http_url}/download/private-notes.txt"))
            .send()
            .await
            .expect("secret request");
        assert_eq!(
            secret.status(),
            reqwest::StatusCode::BAD_REQUEST,
            "non-whitelisted artifact names must be refused even when the file exists"
        );
        unsafe {
            std::env::remove_var("FERRYX_DOWNLOADS_DIR");
        }
    }

    #[cfg(unix)]
    async fn spawn_test_download_relay_with_dirs(
        dirs: Vec<std::path::PathBuf>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let router = download_artifact_router(dirs);

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

        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn a_symlinked_artifact_is_not_served() {
        let outside_dir = tempfile::tempdir().expect("outside dir");
        let staging = tempfile::tempdir().expect("staging dir");
        let secret_bytes = b"secret-payload-should-not-be-served";
        let target_file = outside_dir.path().join("real-file");
        std::fs::write(&target_file, secret_bytes).expect("write target file");

        let symlink_path = staging.path().join("ferryx-cli-linux-amd64");
        std::os::unix::fs::symlink(&target_file, &symlink_path).expect("create symlink");

        let (http_url, handle) =
            spawn_test_download_relay_with_dirs(vec![staging.path().to_path_buf()]).await;

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        let symlink_res = client
            .get(format!("{http_url}/download/ferryx-cli-linux-amd64"))
            .send()
            .await
            .expect("download request");
        assert_ne!(
            symlink_res.status(),
            reqwest::StatusCode::OK,
            "symlink artifact must not return 200 OK"
        );
        assert_eq!(
            symlink_res.status(),
            reqwest::StatusCode::TEMPORARY_REDIRECT,
            "symlink artifact must trigger 307 redirect to GitHub fallback"
        );

        handle.abort();
        assert!(handle.await.unwrap_err().is_cancelled());

        // Remove the symlink before writing the regular file so we do not overwrite into the symlink target.
        std::fs::remove_file(&symlink_path).expect("remove symlink");
        let real_artifact_bytes = b"\x7fELF-ferryx-cli-linux-amd64-real-bytes";
        std::fs::write(&symlink_path, real_artifact_bytes).expect("write regular artifact");

        let (real_http_url, real_handle) =
            spawn_test_download_relay_with_dirs(vec![staging.path().to_path_buf()]).await;

        let real_res = client
            .get(format!("{real_http_url}/download/ferryx-cli-linux-amd64"))
            .send()
            .await
            .expect("download request for regular file");
        assert_eq!(real_res.status(), reqwest::StatusCode::OK);
        let served_bytes = real_res.bytes().await.expect("download body");
        assert_eq!(
            served_bytes.as_ref(),
            real_artifact_bytes.as_slice(),
            "download route must serve genuine regular file bytes"
        );

        real_handle.abort();
        assert!(real_handle.await.unwrap_err().is_cancelled());

        // Direct shared function fallback assertion: two owned candidate dirs
        // (dir 1 has symlink, dir 2 has regular file) proves continuation through candidates.
        let fallback_symlink_dir = tempfile::tempdir().expect("fallback symlink dir");
        let fallback_regular_dir = tempfile::tempdir().expect("fallback regular dir");
        let fallback_secret = fallback_symlink_dir.path().join("secret-target");
        std::fs::write(&fallback_secret, b"fallback-secret-payload").expect("write secret");
        let fallback_symlink = fallback_symlink_dir.path().join("ferryx-cli-linux-amd64");
        std::os::unix::fs::symlink(&fallback_secret, &fallback_symlink).expect("symlink");
        let fallback_real_file = fallback_regular_dir.path().join("ferryx-cli-linux-amd64");
        let fallback_real_bytes = b"\x7fELF-fallback-real-binary";
        std::fs::write(&fallback_real_file, fallback_real_bytes).expect("write regular");

        let fallback_res = download_cli_artifact_from_candidate_dirs(
            "ferryx-cli-linux-amd64",
            vec![
                fallback_symlink_dir.path().to_path_buf(),
                fallback_regular_dir.path().to_path_buf(),
            ],
        )
        .await;
        assert_eq!(
            fallback_res.status(),
            StatusCode::OK,
            "must fall through from symlink directory to valid regular file directory"
        );
        let fallback_body = axum::body::to_bytes(fallback_res.into_body(), 64 * 1024)
            .await
            .expect("read body");
        assert_eq!(
            fallback_body.as_ref(),
            fallback_real_bytes.as_slice(),
            "must serve regular file bytes from second candidate directory"
        );
    }

    #[tokio::test]
    async fn a_multi_chunk_artifact_is_streamed_whole() {
        let staging = tempfile::tempdir().expect("staging dir");
        let artifact: Vec<u8> = (0..(200 * 1024u32)).map(|n| (n % 251) as u8).collect();
        std::fs::write(staging.path().join("ferryx-cli-windows-amd64.exe"), &artifact).unwrap();
        let response = download_cli_artifact_from_candidate_dirs(
            "ferryx-cli-windows-amd64.exe",
            vec![staging.path().to_path_buf()],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_DISPOSITION],
            "attachment; filename=\"ferryx-cli-windows-amd64.exe\""
        );
        assert_eq!(response.headers()[header::CONTENT_LENGTH], artifact.len().to_string());
        let body = axum::body::to_bytes(response.into_body(), artifact.len() + 1).await.unwrap();
        assert_eq!(body.as_ref(), artifact.as_slice());
    }

    async fn spawn_test_relay_with_account_state(
        state: RelayState,
        account_state: Option<Arc<crate::account::service::AccountState>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let router = relay_router_with_account(state, None, account_state);

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

    /// `/healthz` is JSON (not the SPA fallback) and reports which failover role it serves.
    #[tokio::test]
    async fn healthz_reports_primary_and_standby_roles() {
        let temp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            temp.path().join("mail"),
        ));
        let account = Arc::new(crate::account::service::AccountState::new(
            temp.path().join("account"),
            "https://relay.test",
            mailer,
        ));
        let (base, handle) =
            spawn_test_relay_with_account_state(RelayState::new(vec![]), Some(account)).await;
        let url = format!("{}/healthz", base.replacen("ws://", "http://", 1));
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let primary = client.get(&url).send().await.unwrap();
        assert_eq!(primary.status(), StatusCode::OK);
        let body: serde_json::Value = primary.json().await.unwrap();
        assert_eq!(body, serde_json::json!({ "ok": true, "role": "primary" }));

        std::fs::create_dir_all(temp.path().join("account")).unwrap();
        std::fs::write(
            temp.path()
                .join("account")
                .join(crate::account::store::store_sqlite::STANDBY_MARKER_FILENAME),
            b"",
        )
        .unwrap();
        let standby: serde_json::Value =
            client.get(&url).send().await.unwrap().json().await.unwrap();
        assert_eq!(standby["role"], "standby");
        handle.abort();
    }

    /// During a failover the commercial standby must keep admitting enrolled machines: the
    /// entitlement check evaluates without persisting, while account writes such as a new
    /// enrollment are still refused with the typed read-only error.
    #[tokio::test]
    async fn commercial_standby_accepts_enrolled_machine_control_bind_and_entitlement_check() {
        let tmp = tempfile::tempdir().unwrap();
        // Seed the primary's data (user, session, enrolled machine "owned-machine"/"pubkey-1").
        let _seeded = test_account_state(
            tmp.path(),
            "user-1",
            "session-token",
            "owned-machine",
            "other-machine",
        );
        // The same data directory, served as a commercial read-only standby.
        std::fs::write(
            tmp.path()
                .join("account")
                .join(crate::account::store::store_sqlite::STANDBY_MARKER_FILENAME),
            b"",
        )
        .unwrap();
        let account = Arc::new(
            crate::account::service::AccountState::new(
                tmp.path().join("account"),
                "https://relay.test",
                Arc::new(crate::account::mailer::FileMailer::with_dir(
                    tmp.path().join("mail"),
                )),
            )
            .with_deployment_mode(crate::account::origin::DeploymentMode::Commercial),
        );
        assert!(account.is_read_only());

        let now = crate::account::billing::routes::billing_now();
        let entitlement =
            crate::account::billing::routes::entitlement_for_user(&account, "user-1", now)
                .expect("a standby must evaluate entitlement without writing");
        assert_ne!(
            entitlement.status,
            crate::account::billing::entitlement::EntitlementStatus::Stopped
        );

        let state = RelayState::new_with_key_store(vec![], tmp.path().join("keys.json")).unwrap();
        state.set_account_state(account.clone());
        let enrolled = ControlAuth {
            machine_id: "owned-machine".into(),
            public_key: "pubkey-1".into(),
            display_name: "Owned Machine".into(),
            enrollment_token: None,
            signature: String::new(),
            timestamp: 0,
        };
        state
            .bind_machine_key(&enrolled)
            .expect("an enrolled machine must bind its control tunnel on a commercial standby");

        let (base, handle) =
            spawn_test_relay_with_account_state(state, Some(account)).await;
        let http = base.replacen("ws://", "http://", 1);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let enroll = client
            .post(format!("{http}/api/account/v1/machines/enroll"))
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(enroll.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: serde_json::Value = enroll.json().await.unwrap();
        assert_eq!(body["code"], "ACCOUNT_STORE_READ_ONLY");
        handle.abort();
    }

    /// Owner of the foreign machine in [`test_account_state`]. It needs its own `users` row
    /// because `machines.user_id` is `REFERENCES users(user_id)` and the store enables
    /// `PRAGMA foreign_keys = ON` (store_sqlite/schema.rs:11-12, 65-84).
    const OTHER_MACHINE_OWNER_ID: &str = "another-user";

    fn test_account_state(
        temp_dir: &std::path::Path,
        user_id: &str,
        session_token: &str,
        owned_machine_id: &str,
        other_machine_id: &str,
    ) -> Arc<crate::account::service::AccountState> {
        let mail_dir = temp_dir.join("mail");
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(mail_dir));
        let account = Arc::new(crate::account::service::AccountState::new(
            temp_dir.join("account"),
            "https://relay.test",
            mailer,
        ));
        let now = crate::account::store::now_secs();
        account
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    crate::account::store::UserRecord {
                        user_id: user_id.into(),
                        email: "test@example.com".into(),
                        created_at: now,
                    },
                );
                store.sessions.insert(
                    crate::account::store::token_hash(session_token),
                    crate::account::store::SessionRecord {
                        user_id: user_id.into(),
                        expires_at: now + 3600,
                    },
                );
                // The foreign machine below is owned by a different user, so that user must exist
                // before the machine row is written (foreign key) and its email must differ from
                // the primary user's (unique index on users.email).
                store.users.insert(
                    OTHER_MACHINE_OWNER_ID.into(),
                    crate::account::store::UserRecord {
                        user_id: OTHER_MACHINE_OWNER_ID.into(),
                        email: "other-owner@example.com".into(),
                        created_at: now,
                    },
                );
                store.machines.insert(
                    "rec-owned".into(),
                    crate::account::store::MachineRecord {
                        machine_record_id: "rec-owned".into(),
                        owner_user_id: user_id.into(),
                        machine_id: owned_machine_id.into(),
                        display_name: "Owned Machine".into(),
                        public_key: "pubkey-1".into(),
                        attach_public_key: "attachkey-1".into(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                store.machines.insert(
                    "rec-other".into(),
                    crate::account::store::MachineRecord {
                        machine_record_id: "rec-other".into(),
                        owner_user_id: OTHER_MACHINE_OWNER_ID.into(),
                        machine_id: other_machine_id.into(),
                        display_name: "Other Machine".into(),
                        public_key: "pubkey-2".into(),
                        attach_public_key: "attachkey-2".into(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                Ok(())
            })
            .expect("setup test account store");
        account
    }

    #[tokio::test]
    async fn test_attach_session_success_for_owned_machine() {
        let state = test_state(vec![]);
        let (_generation, mut notices, _grants) =
            state.register_control_channel("owned-machine".into());
        let tmp = tempfile::tempdir().unwrap();
        let token = "valid-token";
        let account =
            test_account_state(tmp.path(), "user-1", token, "owned-machine", "other-machine");
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = client
            .post(format!("{http_base}/api/v1/attach/session"))
            .bearer_auth(token)
            .json(&serde_json::json!({ "machineId": "owned-machine" }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        let session_id = body["sessionId"].as_str().expect("sessionId in response");
        assert!(!session_id.is_empty());
        assert_eq!(body["machineId"], "owned-machine");
        assert_eq!(body["opaque"], true);

        // Control channel received exactly one notice with opaque: true and that id
        let notice = notices.try_recv().expect("must receive exactly one notice");
        assert_eq!(notice.session_id, session_id);
        assert!(notice.opaque);
        assert!(matches!(
            notices.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));

        // The id minted this way is takeable as HalfKind::Opaque and NOT as a plain client half
        assert!(matches!(
            state.reserve_half(session_id, HalfKind::Client),
            Err(StatusCode::NOT_FOUND)
        ));
        assert!(state.reserve_half(session_id, HalfKind::Opaque).is_ok());

        server.abort();
    }

    #[tokio::test]
    async fn test_attach_session_rejects_other_accounts_machine() {
        let state = test_state(vec![]);
        let (_generation, mut notices, _grants) =
            state.register_control_channel("other-machine".into());
        let tmp = tempfile::tempdir().unwrap();
        let token = "valid-token";
        let account =
            test_account_state(tmp.path(), "user-1", token, "owned-machine", "other-machine");
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = client
            .post(format!("{http_base}/api/v1/attach/session"))
            .bearer_auth(token)
            .json(&serde_json::json!({ "machineId": "other-machine" }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["code"], "MACHINE_NOT_FOUND");

        // Nothing was pushed to the control channel
        assert!(matches!(
            notices.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));

        server.abort();
    }

    #[tokio::test]
    async fn test_attach_session_rejects_missing_or_invalid_bearer() {
        let state = test_state(vec![]);
        let (_generation, mut notices, _grants) =
            state.register_control_channel("owned-machine".into());
        let tmp = tempfile::tempdir().unwrap();
        let token = "valid-token";
        let account =
            test_account_state(tmp.path(), "user-1", token, "owned-machine", "other-machine");
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        // Missing bearer
        let response = client
            .post(format!("{http_base}/api/v1/attach/session"))
            .json(&serde_json::json!({ "machineId": "owned-machine" }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["code"], "UNAUTHORIZED");

        // Invalid bearer
        let response_invalid = client
            .post(format!("{http_base}/api/v1/attach/session"))
            .bearer_auth("bad-token")
            .json(&serde_json::json!({ "machineId": "owned-machine" }))
            .send()
            .await
            .unwrap();
        assert_eq!(response_invalid.status(), StatusCode::UNAUTHORIZED);
        let body_invalid: serde_json::Value = response_invalid.json().await.unwrap();
        assert_eq!(body_invalid["code"], "UNAUTHORIZED");

        // Nothing was pushed
        assert!(matches!(
            notices.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));

        server.abort();
    }

    #[tokio::test]
    async fn test_attach_session_rejects_offline_machine() {
        let state = test_state(vec![]);
        // Do NOT register a control channel for owned-machine (it is offline)
        let tmp = tempfile::tempdir().unwrap();
        let token = "valid-token";
        let account =
            test_account_state(tmp.path(), "user-1", token, "owned-machine", "other-machine");
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = client
            .post(format!("{http_base}/api/v1/attach/session"))
            .bearer_auth(token)
            .json(&serde_json::json!({ "machineId": "owned-machine" }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["code"], "MACHINE_OFFLINE");

        server.abort();
    }

    #[tokio::test]
    async fn test_attach_session_rejects_when_relay_is_not_account_origin() {
        let state = test_state(vec![]);
        let (base, server) = spawn_test_relay_with_account_state(state.clone(), None).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = client
            .post(format!("{http_base}/api/v1/attach/session"))
            .bearer_auth("any-token")
            .json(&serde_json::json!({ "machineId": "some-machine" }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        server.abort();
    }

    #[tokio::test]
    async fn test_attach_session_resolves_aliased_machine_channel() {
        let state = test_state(vec![]);
        let (_generation, mut notices, _grants) =
            state.register_control_channel("token-alpha".into());
        state.bind_control_alias("token-alpha", "owned-machine".into());

        let tmp = tempfile::tempdir().unwrap();
        let token = "valid-token";
        let account =
            test_account_state(tmp.path(), "user-1", token, "owned-machine", "other-machine");
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = client
            .post(format!("{http_base}/api/v1/attach/session"))
            .bearer_auth(token)
            .json(&serde_json::json!({ "machineId": "owned-machine" }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        let session_id = body["sessionId"].as_str().expect("sessionId in response");

        let notice = notices.try_recv().expect("must receive notice via alias");
        assert_eq!(notice.session_id, session_id);
        assert!(notice.opaque);

        server.abort();
    }

    #[tokio::test]
    async fn test_unregister_control_channel_interleaving_preserves_reconnected_grant_channel() {
        let state = test_state(vec![]);
        let (gen_1, _notices_1, _grants_1) =
            state.register_control_channel("reconnect-machine".into());

        // Concurrency interleaving regression test:
        // When unregister_control_channel drops `control_channels` before acquiring `grant_channels`,
        // a daemon reconnect can slip into that window, registering generation N+1.
        // The unregister loop then resumes and evicts the newly registered grant channel by name.
        //
        // Force the interleaving: hold `grant_channels` lock so the unregister task cannot proceed
        // past the gap between drop(control_channels) and grant_channels.lock().
        let mut grant_hold = state.inner.grant_channels.lock();

        let unregister_handle = {
            let state = state.clone();
            std::thread::spawn(move || {
                state.unregister_control_channel("reconnect-machine", gen_1);
            })
        };

        // If the defect is present (drop(channels) before grant_channels lock),
        // Thread A drops `control_channels`, allowing us to acquire it and observe
        // that "reconnect-machine" was removed from `control_channels` while Thread A
        // is suspended waiting for `grant_channels`.
        let mut defect_gap_detected = false;
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(30) {
            if let Some(mut controls) = state.inner.control_channels.try_lock() {
                if !controls.contains_key("reconnect-machine") {
                    defect_gap_detected = true;
                    // Reconnect occurs inside the gap: registers generation N+1 in both maps
                    let (tx2, _rx2) = tokio::sync::mpsc::channel(100);
                    let (gtx2, _grx2) = tokio::sync::mpsc::channel(1);
                    controls.insert(
                        "reconnect-machine".into(),
                        ControlChannel {
                            generation: gen_1 + 1,
                            tx: tx2,
                        },
                    );
                    grant_hold.insert("reconnect-machine".into(), gtx2);
                    break;
                }
            }
            std::thread::yield_now();
        }

        // Release grant_channels so Thread A can proceed
        drop(grant_hold);
        unregister_handle.join().expect("unregister thread panicked");

        if !defect_gap_detected {
            // Under the fix: control_channels was never dropped before grant_channels was acquired;
            // Thread A held both locks atomically so no gap existed. Simulate reconnect now:
            let (_gen_2, _notices_2, _grants_2) =
                state.register_control_channel("reconnect-machine".into());
        }

        // In the buggy code: Thread A woke up and removed "reconnect-machine" from grant_channels,
        // leaving the machine without a usable grant channel (404 NOT_FOUND).
        // In the fixed code: the machine still has a usable grant channel.
        let grant_present = state
            .inner
            .grant_channels
            .lock()
            .contains_key("reconnect-machine");
        assert!(
            grant_present,
            "reconnected grant channel must not be evicted by stale unregister"
        );
    }

    #[tokio::test]
    async fn test_unregister_control_channel_preserves_already_reconnected_grant_channel() {
        let state = test_state(vec![]);
        let (gen_1, _notices_1, _grants_1) =
            state.register_control_channel("already-reconnected".into());
        let (gen_2, _notices_2, _grants_2) =
            state.register_control_channel("already-reconnected".into());
        assert_eq!(gen_2, gen_1 + 1);

        state.unregister_control_channel("already-reconnected", gen_1);

        assert!(
            state
                .inner
                .grant_channels
                .lock()
                .contains_key("already-reconnected"),
            "grant channel for generation N+1 must survive unregister of generation N"
        );
    }

    #[tokio::test]
    async fn test_account_state_liveness_probe_lifecycle_and_channel_tracking() {
        let state = test_state(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let token = "valid-token";
        let account =
            test_account_state(tmp.path(), "user-1", token, "owned-machine", "other-machine");

        // Make owned-machine record stale in store
        let now = crate::account::store::now_secs();
        let stale_time = now.saturating_sub(
            crate::account::service::MACHINE_ONLINE_FRESHNESS_WINDOW_SECS + 300,
        );
        account
            .mutate(|store| {
                if let Some(m) = store.machines.get_mut("rec-owned") {
                    m.last_seen_at = stale_time;
                }
                Ok(())
            })
            .expect("make machine stale");

        // 1. Before setting account state on relay: stale machine has no probe and is offline
        let rec = crate::account::service::enrolled_machine_by_id(&account, "owned-machine").expect("find owned-machine");
        let view0 = account.machine_view(&rec, now);
        assert!(!view0.online, "stale machine without probe must be offline");

        // 2. Set account state on relay: installs liveness probe. Still offline because channel not registered yet.
        state.set_account_state(account.clone());
        let view1 = account.machine_view(&rec, now);
        assert!(!view1.online, "unregistered machine must remain offline");

        // Setting the same Arc again must retain the probe (ptr_eq guard)
        state.set_account_state(account.clone());
        let view1_repeat = account.machine_view(&rec, now);
        assert!(!view1_repeat.online, "probe must still be functional after setting same Arc");

        // 3. Register live direct control channel: becomes online
        let (gen, rx, _grants) = state.register_control_channel("owned-machine".into());
        let view2 = account.machine_view(&rec, now);
        assert!(view2.online, "machine with live direct channel must be online");

        // Repeat set_account_state with same Arc when channel is LIVE: probe must NOT be cleared
        state.set_account_state(account.clone());
        let view2_repeat = account.machine_view(&rec, now);
        assert!(
            view2_repeat.online,
            "probe must remain functional and online after re-setting same Arc with live channel"
        );

        // 4. Closed receiver: channel sender is closed, becomes offline
        drop(rx);
        let view3 = account.machine_view(&rec, now);
        assert!(!view3.online, "machine with closed channel receiver must report offline");

        // 5. Register alias: machine registered under token-alpha, aliased to owned-machine -> online
        let (gen_alpha, _rx_alpha, _grants_alpha) =
            state.register_control_channel("token-alpha".into());
        state.bind_control_alias("token-alpha", "owned-machine".into());
        let view4 = account.machine_view(&rec, now);
        assert!(view4.online, "machine with live aliased channel must be online");

        // 6. Generation unregister on token-alpha: removes channel and alias -> offline
        state.unregister_control_channel("token-alpha", gen_alpha);
        let view5 = account.machine_view(&rec, now);
        assert!(!view5.online, "unregistered alias channel must report offline");

        // Clean up the dead gen 1 direct channel as well
        state.unregister_control_channel("owned-machine", gen);

        // 7. Re-register live channel, then test account replacement detaches old probe
        let (_gen_live, _rx_live, _grants_live) =
            state.register_control_channel("owned-machine".into());
        let view6_live = account.machine_view(&rec, now);
        assert!(view6_live.online, "re-registered live channel must report online");

        let tmp2 = tempfile::tempdir().unwrap();
        let account2 =
            test_account_state(tmp2.path(), "user-2", "token-2", "owned-machine", "other-machine");
        account2
            .mutate(|store| {
                if let Some(m) = store.machines.get_mut("rec-owned") {
                    m.last_seen_at = stale_time;
                }
                Ok(())
            })
            .expect("make machine stale in account2");
        state.set_account_state(account2.clone());

        // Old account must have its probe cleared by replacement and now report offline despite live channel
        let view_old_after_replace = account.machine_view(&rec, now);
        assert!(
            !view_old_after_replace.online,
            "old account must lose its probe on replacement and report offline despite live channel"
        );

        // New account received the probe and reports online for the live channel
        let rec2 = crate::account::service::enrolled_machine_by_id(&account2, "owned-machine").expect("find owned-machine in account2");
        let view_new = account2.machine_view(&rec2, now);
        assert!(
            view_new.online,
            "new replacement account must receive probe and report online"
        );

        // 8. Test clear_account_state detaches probe from active account -> offline
        state.clear_account_state();
        let view7 = account2.machine_view(&rec2, now);
        assert!(!view7.online, "detached account must lose probe and report offline");
    }

    /// Request line the relay's bearer probe uses before it consults account authorization.
    const DEVICE_PROBE_REQUEST_LINE: &str = "GET /api/v1/devices HTTP/1.1";

    /// Serves one machine-side tunnel session and returns the proxied request line with the
    /// paired data socket. Both waits are bounded, so the fixture can never hang.
    async fn next_machine_request(
        base: &str,
        notices: &mut mpsc::Receiver<IncomingSessionNotice>,
    ) -> (String, TestSocket) {
        let notice: IncomingSessionNotice = timeout(Duration::from_secs(5), notices.recv())
            .await
            .expect("timed out waiting for a machine session notice")
            .expect("control channel closed before a machine session notice arrived");
        assert!(!notice.opaque);
        let (mut data, _) = tokio_tungstenite::connect_async(format!(
            "{base}/tunnel/data/{}",
            notice.session_id
        ))
        .await
        .expect("machine data channel must connect");
        let frame = timeout(Duration::from_secs(5), data.next())
            .await
            .expect("timed out waiting for the proxied request")
            .expect("the data channel ended before the proxied request")
            .expect("the data channel failed before the proxied request");
        let raw = String::from_utf8(frame.into_data().to_vec()).expect("request bytes");
        let line = raw
            .lines()
            .next()
            .expect("the proxied request must carry a request line")
            .to_string();
        (line, data)
    }

    /// Answers a proxied HTTP request and asserts the relay releases the data half within 5s.
    /// The response carries `Connection: close` so the relay never parks the channel in its
    /// keep-alive pool; it drops that socket without a close handshake once it has parsed the
    /// response, so only boundedness is asserted here rather than the shape of that close.
    async fn reply_http(data: &mut TestSocket, status: StatusCode, body: &[u8]) {
        let head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Unknown"),
            body.len()
        );
        data.send(TMessage::Binary(head.into_bytes().into()))
            .await
            .expect("send response head");
        if !body.is_empty() {
            data.send(TMessage::Binary(body.to_vec().into()))
                .await
                .expect("send response body");
        }
        let tail = timeout(Duration::from_secs(5), data.next()).await;
        assert!(
            tail.is_ok(),
            "the relay must release the data half within 5s of the response"
        );
    }

    /// Serves machine-side sessions until the spawned client request settles, classifying each
    /// proxied request by its request line: the relay's bearer probe
    /// ([`DEVICE_PROBE_REQUEST_LINE`]) is answered 401 so an account bearer falls through to
    /// account authorization, and any other request is asserted to be `forwarded_request_line`
    /// and answered with the scripted status/body.
    ///
    /// Terminating on the request itself - never on a sleep, a poll or a fixed session count -
    /// keeps the fixture deterministic on both paths: with the authorization gate intact the
    /// request settles right after the probe and no further session exists, while with the gate
    /// removed the forwarded request is still answered, so the caller's status assertion fails
    /// on the intended evidence instead of on a client timeout.
    async fn serve_machine_sessions_until<F>(
        base: &str,
        notices: &mut mpsc::Receiver<IncomingSessionNotice>,
        client_request: &mut F,
        forwarded_request_line: &str,
        forwarded_status: StatusCode,
        forwarded_body: &[u8],
    ) -> (usize, F::Output)
    where
        F: std::future::Future + Unpin,
    {
        let mut served = 0usize;
        loop {
            tokio::select! {
                settled = &mut *client_request => return (served, settled),
                session = next_machine_request(base, notices) => {
                    let (line, mut data) = session;
                    if line == DEVICE_PROBE_REQUEST_LINE {
                        reply_http(&mut data, StatusCode::UNAUTHORIZED, b"").await;
                    } else {
                        assert_eq!(
                            line, forwarded_request_line,
                            "unexpected request line on the machine side"
                        );
                        reply_http(&mut data, forwarded_status, forwarded_body).await;
                    }
                    served += 1;
                }
            }
        }
    }

    #[tokio::test]
    async fn host_proxy_rejects_foreign_account() {
        let state = test_state(vec![]);
        let (_generation, mut notices, _grants) =
            state.register_control_channel("other-machine".into());
        let tmp = tempfile::tempdir().unwrap();
        let token = "valid-token";
        let account =
            test_account_state(tmp.path(), "user-1", token, "owned-machine", "other-machine");
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        // Spawned so the fixture below can stop the moment the request settles.
        let mut request = tokio::spawn({
            let client = client.clone();
            let url = format!("{http_base}/host/other-machine/api/v1/health");
            async move { client.get(url).bearer_auth(token).send().await }
        });

        let (served, settled) = serve_machine_sessions_until(
            &base,
            &mut notices,
            &mut request,
            "GET /api/v1/health HTTP/1.1",
            StatusCode::OK,
            br#"{"status":"healthy"}"#,
        )
        .await;
        assert_eq!(
            served, 0,
            "an account session bearer must never reach another account's machine, not even as a device probe"
        );
        let response = settled
            .expect("request task must not panic")
            .expect("host proxy request must return a response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["code"], "MACHINE_NOT_FOUND");

        server.abort();
    }

    #[tokio::test]
    async fn host_proxy_rejects_anonymous_on_account_relay() {
        let state = test_state(vec![]);
        let (_generation, mut notices, _grants) =
            state.register_control_channel("owned-machine".into());
        let tmp = tempfile::tempdir().unwrap();
        let token = "valid-token";
        let account =
            test_account_state(tmp.path(), "user-1", token, "owned-machine", "other-machine");
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();

        // 1. Missing Authorization header completely -> 401 UNAUTHORIZED, with no machine
        //    session at all: the gate rejects before it opens one.
        let mut request = tokio::spawn({
            let client = client.clone();
            let url = format!("{http_base}/host/owned-machine/api/v1/health");
            async move { client.get(url).send().await }
        });
        let (served, settled) = serve_machine_sessions_until(
            &base,
            &mut notices,
            &mut request,
            "GET /api/v1/health HTTP/1.1",
            StatusCode::OK,
            br#"{"status":"healthy"}"#,
        )
        .await;
        assert_eq!(
            served, 0,
            "a request without a bearer must be rejected before any machine session"
        );
        let response = settled
            .expect("request task must not panic")
            .expect("anonymous host proxy request must return a response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["code"], "UNAUTHORIZED");

        // 2. Invalid/unknown bearer token (neither device token nor account session) -> 401
        //    UNAUTHORIZED. The bearer may only ever reach the machine as the device probe.
        let mut request_bad = tokio::spawn({
            let client = client.clone();
            let url = format!("{http_base}/host/owned-machine/api/v1/health");
            async move { client.get(url).bearer_auth("bad-token").send().await }
        });
        let (served_bad, settled_bad) = serve_machine_sessions_until(
            &base,
            &mut notices,
            &mut request_bad,
            "GET /api/v1/health HTTP/1.1",
            StatusCode::OK,
            br#"{"status":"healthy"}"#,
        )
        .await;
        assert_eq!(
            served_bad, 1,
            "an unknown bearer may only reach the device probe, never a forwarded request"
        );
        let response_bad = settled_bad
            .expect("request task must not panic")
            .expect("host proxy request must return a response");
        assert_eq!(response_bad.status(), StatusCode::UNAUTHORIZED);
        let body_bad: serde_json::Value = response_bad.json().await.unwrap();
        assert_eq!(body_bad["code"], "UNAUTHORIZED");

        server.abort();
    }

    #[tokio::test]
    async fn host_proxy_allows_owner() {
        let state = test_state(vec![]);
        let (_generation, mut notices, _grants) =
            state.register_control_channel("owned-machine".into());
        let tmp = tempfile::tempdir().unwrap();
        let token = "valid-token";
        let account =
            test_account_state(tmp.path(), "user-1", token, "owned-machine", "other-machine");
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let mut request = tokio::spawn({
            let client = client.clone();
            let url = format!("{http_base}/host/owned-machine/api/v1/health");
            async move { client.get(url).bearer_auth(token).send().await }
        });

        // The owner path serves only the authorized forward (an account session is never used
        // as a device probe); the served count is deliberately not asserted here so a gate
        // mutation keeps the reject tests as the only RED evidence.
        let (_served, settled) = serve_machine_sessions_until(
            &base,
            &mut notices,
            &mut request,
            "GET /api/v1/health HTTP/1.1",
            StatusCode::OK,
            br#"{"status":"healthy"}"#,
        )
        .await;
        let response = settled
            .expect("request task must not panic")
            .expect("host proxy request must return a response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["status"], "healthy");

        server.abort();
    }

    #[tokio::test]
    async fn host_proxy_allows_authorized_device_token() {
        let state = test_state(vec![]);
        let (_generation, mut notices, _grants) =
            state.register_control_channel("owned-machine".into());
        let device_token = "valid-paired-device-token";
        // Pre-register fresh cached device token in relay state (as happens after pair/exchange or authorize_device_token)
        state.register_device_token("owned-machine", device_token);

        let tmp = tempfile::tempdir().unwrap();
        // Account state has user-1, but no account session token matches the device token
        let account =
            test_account_state(tmp.path(), "user-1", "user-session-token", "owned-machine", "other-machine");
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        // A cached device token needs no probe: the forwarded request is the only session.
        let mut request = tokio::spawn({
            let client = client.clone();
            let url = format!("{http_base}/host/owned-machine/api/v1/health");
            async move { client.get(url).bearer_auth(device_token).send().await }
        });

        let (served, settled) = serve_machine_sessions_until(
            &base,
            &mut notices,
            &mut request,
            "GET /api/v1/health HTTP/1.1",
            StatusCode::OK,
            br#"{"status":"healthy","paired":true}"#,
        )
        .await;
        assert_eq!(
            served, 1,
            "a cached device token must authorize without a device probe"
        );
        let response = settled
            .expect("request task must not panic")
            .expect("host proxy request must return a response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["status"], "healthy");
        assert_eq!(body["paired"], true);

        server.abort();
    }

    #[tokio::test]
    async fn host_proxy_device_token_on_offline_machine_is_409_not_401() {
        // No control channel for owned-machine: the device-token check cannot reach the host.
        let state = test_state(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let account = test_account_state(
            tmp.path(),
            "user-1",
            "user-session-token",
            "owned-machine",
            "other-machine",
        );
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let response = client
            .get(format!("{http_base}/host/owned-machine/api/v1/capabilities"))
            .bearer_auth("paired-device-token")
            .send()
            .await
            .unwrap();

        // An unconfirmed token is not a revoked one: a 401 here would make clients drop it.
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["code"], "MACHINE_OFFLINE");

        server.abort();
    }

    #[tokio::test]
    async fn host_proxy_device_token_check_failure_is_502_not_401() {
        let state = test_state(vec![]);
        let (_generation, mut notices, _grants) =
            state.register_control_channel("owned-machine".into());
        let tmp = tempfile::tempdir().unwrap();
        let account = test_account_state(
            tmp.path(),
            "user-1",
            "user-session-token",
            "owned-machine",
            "other-machine",
        );
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let request = tokio::spawn({
            let client = client.clone();
            let url = format!("{http_base}/host/owned-machine/api/v1/capabilities");
            async move { client.get(url).bearer_auth("paired-device-token").send().await }
        });

        // The host answers the device check with neither a grant nor a refusal.
        let (line, mut data) = next_machine_request(&base, &mut notices).await;
        assert_eq!(line, DEVICE_PROBE_REQUEST_LINE);
        reply_http(&mut data, StatusCode::INTERNAL_SERVER_ERROR, b"").await;

        let response = timeout(Duration::from_secs(5), request)
            .await
            .expect("host proxy request must settle")
            .expect("request task must not panic")
            .expect("host proxy request must return a response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["code"], "HOST_UNAVAILABLE");

        server.abort();
    }

    #[tokio::test]
    async fn stopped_owner_attach_session_is_402() {
        let state = test_state(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let token = "user-stopped-session-token";
        let user_id = "user-stopped-attach";
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let account = Arc::new(
            crate::account::service::AccountState::new(
                tmp.path().join("account"),
                "https://relay.test",
                mailer,
            )
            .with_deployment_mode(crate::account::origin::DeploymentMode::Commercial),
        );
        let now = crate::account::store::now_secs();
        account
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    crate::account::store::UserRecord {
                        user_id: user_id.into(),
                        email: "stopped-attach@example.com".into(),
                        created_at: now,
                    },
                );
                store.sessions.insert(
                    crate::account::store::token_hash(token),
                    crate::account::store::SessionRecord {
                        user_id: user_id.into(),
                        expires_at: now + 3600,
                    },
                );
                store.machines.insert(
                    "rec-owned".into(),
                    crate::account::store::MachineRecord {
                        machine_record_id: "rec-owned".into(),
                        owner_user_id: user_id.into(),
                        machine_id: "owned-machine".into(),
                        display_name: "Owned Machine".into(),
                        public_key: "pubkey-1".into(),
                        attach_public_key: "attachkey-1".into(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                // More machines owned by the user create an active over_limit violation on Free
                // (FREE_HOSTS + 1 > FREE_HOSTS).
                store.machines.insert(
                    "rec-owned-2".into(),
                    crate::account::store::MachineRecord {
                        machine_record_id: "rec-owned-2".into(),
                        owner_user_id: user_id.into(),
                        machine_id: "owned-machine-2".into(),
                        display_name: "Owned Machine 2".into(),
                        public_key: "pubkey-2".into(),
                        attach_public_key: "attachkey-2".into(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                seed_free_over_limit_fillers(store, user_id, now);
                store.billing_states.insert(
                    user_id.into(),
                    crate::account::store::BillingStateRecord {
                        owner_key: user_id.into(),
                        grace_started_at: Some(
                            now.saturating_sub(crate::account::billing::entitlement::GRACE_SECS + 1000),
                        ),
                        stopped_at: Some(now.saturating_sub(100)),
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .unwrap();

        // Assert computed status before attach call is legitimately Stopped
        let eval_before =
            crate::account::billing::routes::entitlement_for_user(&account, user_id, now)
                .expect("entitlement evaluation");
        assert_eq!(
            eval_before.status,
            crate::account::billing::entitlement::EntitlementStatus::Stopped,
            "computed status before attach session must be Stopped"
        );
        assert!(
            !eval_before.remote_allowed,
            "remote access must not be allowed when stopped"
        );

        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();

        let attach_body = serde_json::json!({
            "machineId": "owned-machine"
        });

        let response = client
            .post(format!("{http_base}/api/v1/attach/session"))
            .bearer_auth(token)
            .json(&attach_body)
            .send()
            .await
            .expect("attach session request must respond");

        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["code"], "REMOTE_SUSPENDED");

        server.abort();
    }

    #[tokio::test]
    async fn stopped_owner_control_tunnel_refused() {
        let state = test_state(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let user_id = "user-stopped-control";
        let token = "user-stopped-ctrl-token";
        let identity = identity(101, "owned-stopped-ctrl");
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let account = Arc::new(
            crate::account::service::AccountState::new(
                tmp.path().join("account"),
                "https://relay.test",
                mailer,
            )
            .with_deployment_mode(crate::account::origin::DeploymentMode::Commercial),
        );
        let now = crate::account::store::now_secs();
        account
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    crate::account::store::UserRecord {
                        user_id: user_id.into(),
                        email: "stopped-ctrl@example.com".into(),
                        created_at: now,
                    },
                );
                store.sessions.insert(
                    crate::account::store::token_hash(token),
                    crate::account::store::SessionRecord {
                        user_id: user_id.into(),
                        expires_at: now + 3600,
                    },
                );
                store.machines.insert(
                    "rec-stopped-ctrl".into(),
                    crate::account::store::MachineRecord {
                        machine_record_id: "rec-stopped-ctrl".into(),
                        owner_user_id: user_id.into(),
                        machine_id: identity.machine_id.clone(),
                        display_name: identity.display_name.clone(),
                        public_key: identity.public_key.clone(),
                        attach_public_key: identity.public_key.clone(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                // More machines owned by the user create an active over_limit violation on Free
                // (FREE_HOSTS + 1 > FREE_HOSTS).
                store.machines.insert(
                    "rec-stopped-ctrl-2".into(),
                    crate::account::store::MachineRecord {
                        machine_record_id: "rec-stopped-ctrl-2".into(),
                        owner_user_id: user_id.into(),
                        machine_id: "stopped-ctrl-machine-2".into(),
                        display_name: "Stopped Ctrl Machine 2".into(),
                        public_key: "pubkey-ctrl-2".into(),
                        attach_public_key: "attachkey-ctrl-2".into(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                seed_free_over_limit_fillers(store, user_id, now);
                store.billing_states.insert(
                    user_id.into(),
                    crate::account::store::BillingStateRecord {
                        owner_key: user_id.into(),
                        grace_started_at: Some(
                            now.saturating_sub(crate::account::billing::entitlement::GRACE_SECS + 1000),
                        ),
                        stopped_at: Some(now.saturating_sub(100)),
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .unwrap();

        // Assert computed status before control connection is legitimately Stopped
        let eval_before =
            crate::account::billing::routes::entitlement_for_user(&account, user_id, now)
                .expect("entitlement evaluation");
        assert_eq!(
            eval_before.status,
            crate::account::billing::entitlement::EntitlementStatus::Stopped,
            "computed status before control socket connect must be Stopped"
        );
        assert!(
            !eval_before.remote_allowed,
            "remote access must not be allowed when stopped"
        );

        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;

        let (mut socket, response) =
            authenticate_with_token(&base, &identity, false, false, None).await;

        assert!(
            !response.success,
            "control authentication must fail for a stopped account owner"
        );
        assert_eq!(response.error.as_deref(), Some("REMOTE_SUSPENDED"));

        let close_frame = timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("server must close control socket within 5s")
            .expect("socket stream item must exist")
            .expect("socket read must not error");

        match close_frame {
            TMessage::Close(Some(frame)) => {
                assert_eq!(frame.reason.as_str(), "REMOTE_SUSPENDED");
                assert_eq!(u16::from(frame.code), 1008);
            }
            other => panic!("expected close frame with REMOTE_SUSPENDED reason, got {other:?}"),
        }

        server.abort();
    }

    #[tokio::test]
    async fn static_token_machine_unaffected() {
        let state = test_state(vec!["test-machine-token".into()]);
        let (base, server) = spawn_test_relay_with_account_state(state.clone(), None).await;
        let identity = identity(102, "static-token-machine");

        let (_socket, response) = authenticate_with_token(
            &base,
            &identity,
            false,
            false,
            Some("test-machine-token"),
        )
        .await;

        assert!(
            response.success,
            "static token machine must authenticate successfully without account billing gate"
        );
        assert!(response.error.is_none());

        server.abort();
    }

    #[tokio::test]
    async fn selfhost_never_checks() {
        let state = test_state(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let user_id = "user-selfhost";
        let token = "user-selfhost-token";
        let identity = identity(103, "selfhost-ctrl-machine");
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let account = Arc::new(
            crate::account::service::AccountState::new(
                tmp.path().join("account"),
                "https://relay.test",
                mailer,
            )
            .with_deployment_mode(crate::account::origin::DeploymentMode::SelfHost),
        );
        let now = crate::account::store::now_secs();
        account
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    crate::account::store::UserRecord {
                        user_id: user_id.into(),
                        email: "selfhost@example.com".into(),
                        created_at: now,
                    },
                );
                store.sessions.insert(
                    crate::account::store::token_hash(token),
                    crate::account::store::SessionRecord {
                        user_id: user_id.into(),
                        expires_at: now + 3600,
                    },
                );
                store.machines.insert(
                    "rec-selfhost".into(),
                    crate::account::store::MachineRecord {
                        machine_record_id: "rec-selfhost".into(),
                        owner_user_id: user_id.into(),
                        machine_id: identity.machine_id.clone(),
                        display_name: identity.display_name.clone(),
                        public_key: identity.public_key.clone(),
                        attach_public_key: identity.public_key.clone(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                // Mark stopped state in store; SelfHost must ignore it
                store.billing_states.insert(
                    user_id.into(),
                    crate::account::store::BillingStateRecord {
                        owner_key: user_id.into(),
                        grace_started_at: Some(now - 700_000),
                        stopped_at: Some(now - 100),
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .unwrap();

        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;

        let (_socket, response) =
            authenticate_with_token(&base, &identity, false, false, None).await;

        assert!(
            response.success,
            "selfhost relay must never refuse control socket on billing state"
        );
        assert!(response.error.is_none());

        server.abort();
    }

    #[tokio::test]
    async fn billing_evaluation_failure_refuses_control_tunnel() {
        let state = test_state(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let user_id = "user-outage";
        let identity = identity(104, "outage-machine");
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let account = Arc::new(
            crate::account::service::AccountState::new(
                tmp.path().join("account"),
                "https://relay.test",
                mailer,
            )
            .with_deployment_mode(crate::account::origin::DeploymentMode::Commercial),
        );
        let now = crate::account::store::now_secs();
        account
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    crate::account::store::UserRecord {
                        user_id: user_id.into(),
                        email: "outage@example.com".into(),
                        created_at: now,
                    },
                );
                store.machines.insert(
                    "rec-outage".into(),
                    crate::account::store::MachineRecord {
                        machine_record_id: "rec-outage".into(),
                        owner_user_id: user_id.into(),
                        machine_id: identity.machine_id.clone(),
                        display_name: identity.display_name.clone(),
                        public_key: identity.public_key.clone(),
                        attach_public_key: identity.public_key.clone(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                Ok(())
            })
            .unwrap();

        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account.clone())).await;

        // Simulate billing store outage by corrupting the database file
        let db_path = tmp
            .path()
            .join("account")
            .join(crate::account::store::store_sqlite::STORE_SQLITE_FILENAME);
        assert!(
            db_path.exists(),
            "store database file must exist at {db_path:?} before corruption"
        );
        let wal_path = tmp
            .path()
            .join("account")
            .join(format!("{}-wal", crate::account::store::store_sqlite::STORE_SQLITE_FILENAME));
        if wal_path.exists() {
            let _ = std::fs::remove_file(&wal_path);
        }
        let shm_path = tmp
            .path()
            .join("account")
            .join(format!("{}-shm", crate::account::store::store_sqlite::STORE_SQLITE_FILENAME));
        if shm_path.exists() {
            let _ = std::fs::remove_file(&shm_path);
        }
        std::fs::write(&db_path, b"corrupted sqlite header garbage data").unwrap();

        let (_socket, response) =
            authenticate_with_token(&base, &identity, false, false, None).await;

        assert!(
            !response.success,
            "control authentication must fail when billing entitlement evaluation fails"
        );
        let err = response.error.expect("error message must be present");
        assert!(
            err.contains("ACCOUNT_ENTITLEMENT_UNAVAILABLE"),
            "error must indicate entitlement evaluation failure, got: {err}"
        );

        server.abort();
    }

    #[tokio::test]
    async fn stopped_owner_device_token_proxy_refused() {
        let state = test_state(vec![]);
        let (_generation, mut notices, _grants) =
            state.register_control_channel("owned-machine".into());
        let device_token = "valid-stopped-device-token";
        state.register_device_token("owned-machine", device_token);

        let tmp = tempfile::tempdir().unwrap();
        let user_id = "user-stopped-dev";
        let session_token = "user-session-token";
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let account = Arc::new(
            crate::account::service::AccountState::new(
                tmp.path().join("account"),
                "https://relay.test",
                mailer,
            )
            .with_deployment_mode(crate::account::origin::DeploymentMode::Commercial),
        );
        let now = crate::account::store::now_secs();
        account
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    crate::account::store::UserRecord {
                        user_id: user_id.into(),
                        email: "stopped-dev@example.com".into(),
                        created_at: now,
                    },
                );
                store.sessions.insert(
                    crate::account::store::token_hash(session_token),
                    crate::account::store::SessionRecord {
                        user_id: user_id.into(),
                        expires_at: now + 3600,
                    },
                );
                store.machines.insert(
                    "rec-owned".into(),
                    crate::account::store::MachineRecord {
                        machine_record_id: "rec-owned".into(),
                        owner_user_id: user_id.into(),
                        machine_id: "owned-machine".into(),
                        display_name: "Owned Machine".into(),
                        public_key: "pubkey-1".into(),
                        attach_public_key: "attachkey-1".into(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                // More machines create an active over_limit violation on Free (FREE_HOSTS + 1 > FREE_HOSTS).
                store.machines.insert(
                    "rec-owned-2".into(),
                    crate::account::store::MachineRecord {
                        machine_record_id: "rec-owned-2".into(),
                        owner_user_id: user_id.into(),
                        machine_id: "owned-machine-2".into(),
                        display_name: "Owned Machine 2".into(),
                        public_key: "pubkey-2".into(),
                        attach_public_key: "attachkey-2".into(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                seed_free_over_limit_fillers(store, user_id, now);
                store.billing_states.insert(
                    user_id.into(),
                    crate::account::store::BillingStateRecord {
                        owner_key: user_id.into(),
                        grace_started_at: Some(
                            now.saturating_sub(crate::account::billing::entitlement::GRACE_SECS + 1000),
                        ),
                        stopped_at: Some(now.saturating_sub(100)),
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .unwrap();

        // Assert computed status before proxy call is legitimately Stopped
        let eval_before =
            crate::account::billing::routes::entitlement_for_user(&account, user_id, now)
                .expect("entitlement evaluation");
        assert_eq!(
            eval_before.status,
            crate::account::billing::entitlement::EntitlementStatus::Stopped,
            "computed status before proxy request must be Stopped"
        );
        assert!(
            !eval_before.remote_allowed,
            "remote access must not be allowed when stopped"
        );

        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account)).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();

        let mut request = tokio::spawn({
            let client = client.clone();
            let url = format!("{http_base}/host/owned-machine/api/v1/health");
            async move { client.get(url).bearer_auth(device_token).send().await }
        });

        // The request must be rejected by the relay gate before reaching the machine
        let (served, settled) = serve_machine_sessions_until(
            &base,
            &mut notices,
            &mut request,
            "GET /api/v1/health HTTP/1.1",
            StatusCode::OK,
            br#"{"status":"healthy"}"#,
        )
        .await;
        assert_eq!(
            served, 0,
            "a valid device token for a stopped account must be rejected before opening any machine session"
        );
        let response = settled
            .expect("request task must not panic")
            .expect("proxy request must return a response");
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["code"], "REMOTE_SUSPENDED");
        let details = body.get("details").expect("details must be present");
        assert_eq!(details["status"], "stopped");
        assert_eq!(details["plan"], "free");
        assert_eq!(details["stoppedAt"], now.saturating_sub(100));

        server.abort();
    }

    #[tokio::test]
    async fn static_token_device_token_proxy_unaffected() {
        let state = test_state(vec![]);
        let (_generation, mut notices, _grants) =
            state.register_control_channel("static-machine".into());
        let device_token = "valid-static-device-token";
        state.register_device_token("static-machine", device_token);

        // Standalone relay without account state
        let (base, server) = spawn_test_relay_with_account_state(state.clone(), None).await;
        let http_base = base.replace("ws://", "http://");

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();

        let mut request = tokio::spawn({
            let client = client.clone();
            let url = format!("{http_base}/host/static-machine/api/v1/health");
            async move { client.get(url).bearer_auth(device_token).send().await }
        });

        let (served, settled) = serve_machine_sessions_until(
            &base,
            &mut notices,
            &mut request,
            "GET /api/v1/health HTTP/1.1",
            StatusCode::OK,
            br#"{"status":"healthy","static":true}"#,
        )
        .await;
        assert_eq!(
            served, 1,
            "a cached device token for a static machine must authorize without account checks"
        );
        let response = settled
            .expect("request task must not panic")
            .expect("proxy request must return a response");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["status"], "healthy");
        assert_eq!(body["static"], true);

        server.abort();
    }

    /// A commercial account state whose store backs the suspension tests.
    fn test_suspension_account(
        temp_dir: &std::path::Path,
    ) -> Arc<crate::account::service::AccountState> {
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            temp_dir.join("mail"),
        ));
        Arc::new(
            crate::account::service::AccountState::new(
                temp_dir.join("account"),
                "https://relay.test",
                mailer,
            )
            .with_deployment_mode(crate::account::origin::DeploymentMode::Commercial),
        )
    }

    /// Seeds `FREE_HOSTS - 1` extra machines for `user_id`. Added to a fixture that already
    /// holds two machines, the owner is one machine over the Free limit.
    fn seed_free_over_limit_fillers(
        store: &mut crate::account::store::AccountStore,
        user_id: &str,
        now: u64,
    ) {
        for index in 0..crate::account::billing::entitlement::FREE_HOSTS - 1 {
            let id = format!("filler-{user_id}-{index}");
            store.machines.insert(
                format!("rec-{id}"),
                crate::account::store::MachineRecord {
                    machine_record_id: format!("rec-{id}"),
                    owner_user_id: user_id.into(),
                    machine_id: id.clone(),
                    display_name: id.clone(),
                    public_key: format!("pubkey-{id}"),
                    attach_public_key: format!("attachkey-{id}"),
                    relay_origin: "https://relay.test".into(),
                    platform: "linux".into(),
                    enrollment_epoch: 1,
                    enrolled_at: now,
                    last_seen_at: now,
                },
            );
        }
    }

    /// Enrolls `machine_count` machines for `user_id` — the first carrying `public_key` and
    /// `machine_id` — and seeds the owner's grace start when one is given.
    ///
    /// `FREE_HOSTS + 1` machines on the Free plan are over its limit, so an owner seeded with a
    /// grace start is a real violation rather than a hypothetical one.
    fn seed_suspension_owner(
        account: &Arc<crate::account::service::AccountState>,
        user_id: &str,
        session_token: &str,
        machine_id: &str,
        machine_public_key: &str,
        grace_started_at: Option<u64>,
        machine_count: usize,
    ) {
        let now = crate::account::store::now_secs();
        account
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    crate::account::store::UserRecord {
                        user_id: user_id.into(),
                        email: format!("{user_id}@example.com"),
                        created_at: now,
                    },
                );
                store.sessions.insert(
                    crate::account::store::token_hash(session_token),
                    crate::account::store::SessionRecord {
                        user_id: user_id.into(),
                        expires_at: now + 3600,
                    },
                );
                for index in 0..machine_count {
                    let id = if index == 0 {
                        machine_id.to_string()
                    } else {
                        format!("{machine_id}-{index}")
                    };
                    let public_key = if index == 0 {
                        machine_public_key.to_string()
                    } else {
                        format!("pubkey-{id}")
                    };
                    store.machines.insert(
                        format!("rec-{id}"),
                        crate::account::store::MachineRecord {
                            machine_record_id: format!("rec-{id}"),
                            owner_user_id: user_id.into(),
                            machine_id: id.clone(),
                            display_name: id.clone(),
                            public_key,
                            attach_public_key: format!("attachkey-{id}"),
                            relay_origin: "https://relay.test".into(),
                            platform: "linux".into(),
                            enrollment_epoch: 1,
                            enrolled_at: now,
                            last_seen_at: now,
                        },
                    );
                }
                if let Some(grace_started_at) = grace_started_at {
                    store.billing_states.insert(
                        user_id.into(),
                        crate::account::store::BillingStateRecord {
                            owner_key: user_id.into(),
                            grace_started_at: Some(grace_started_at),
                            stopped_at: None,
                            last_notice: None,
                        },
                    );
                }
                Ok(())
            })
            .expect("seed the suspension fixture");
    }

    /// Authenticates a control tunnel, allocates a session and pairs both halves, proving the
    /// splice is live before the test makes the assertion it actually cares about.
    ///
    /// The daemon half connects first, so it owns the splice and the browser half is the peer.
    async fn open_live_session(
        base: &str,
        identity: &crate::remote::auth::MachineIdentity,
    ) -> (TestSocket, TestSocket, TestSocket, IncomingSessionNotice) {
        let (mut control, response) =
            authenticate_with_token(base, identity, false, false, None).await;
        assert!(
            response.success,
            "the control tunnel must authenticate: {:?}",
            response.error
        );
        let notice = allocate(&mut control).await;
        let (mut data, _) = tokio_tungstenite::connect_async(format!(
            "{base}/tunnel/data/{}",
            notice.session_id
        ))
        .await
        .unwrap();
        let (mut client, _) = tokio_tungstenite::connect_async(format!(
            "{base}/tunnel/client/{}",
            notice.session_id
        ))
        .await
        .unwrap();
        client.send(TMessage::Text("live".into())).await.unwrap();
        let frame = timeout(Duration::from_secs(5), data.next())
            .await
            .expect("the paired halves must forward a frame")
            .expect("the daemon half must stay open")
            .expect("the frame read must not error");
        assert_eq!(frame.to_text().unwrap(), "live");
        (control, client, data, notice)
    }

    #[tokio::test]
    async fn a_suspended_owner_is_refused_before_the_host_mints_a_device() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(vec![]);
        let machine_id = "suspended-pair-machine";
        let now = crate::account::store::now_secs();
        let account = test_suspension_account(tmp.path());
        seed_suspension_owner(
            &account,
            "user-suspended-pair",
            "user-suspended-pair-token",
            machine_id,
            &identity(202, machine_id).public_key,
            Some(now.saturating_sub(crate::account::billing::entitlement::GRACE_SECS + 1000)),
            crate::account::billing::entitlement::FREE_HOSTS as usize + 1,
        );
        let entitlement = crate::account::billing::routes::entitlement_for_user(
            &account,
            "user-suspended-pair",
            crate::account::billing::routes::billing_now(),
        )
        .unwrap();
        assert!(!entitlement.remote_allowed, "the fixture owner must be stopped");
        state.set_account_state(account);
        let (generation, mut notices, _grants) = state.register_control_channel(machine_id.into());
        state
            .register_pairing(machine_id, generation, security_registration(machine_id))
            .unwrap();
        assert_eq!(security_claim(&state, "123456").await, StatusCode::PAYMENT_REQUIRED);
        // The exchange never reached the host: no data channel was requested.
        assert!(matches!(notices.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
        assert_eq!(state.inner.pairings.lock()["123456"].state, PairingState::Ready);
    }

    #[test]
    fn one_unreadable_machine_or_owner_does_not_abort_the_suspension_sweep() {
        let candidates: Vec<String> =
            ["broken-machine", "stopped-machine", "broken-owner-machine"]
                .into_iter()
                .map(String::from)
                .collect();
        let targets = suspension_targets_with(
            &candidates,
            |machine| match machine {
                "broken-machine" => Err(crate::account::service::ApiError::internal("corrupt row")),
                "stopped-machine" => Ok(Some((machine.into(), "stopped-owner".into()))),
                _ => Ok(Some((machine.into(), "broken-owner".into()))),
            },
            |owner| match owner {
                "stopped-owner" => Ok(true),
                _ => Err(crate::account::service::ApiError::internal("unreadable billing")),
            },
        );
        assert_eq!(targets.owners, HashSet::from(["stopped-owner".to_string()]));
        assert_eq!(targets.machines, HashSet::from(["stopped-machine".to_string()]));
    }

    #[tokio::test]
    async fn suspension_closes_live_sessions_with_reason() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(vec![]);
        let user_id = "user-suspension";
        let machine_id = "suspension-machine";
        let identity = identity(201, machine_id);
        let now = crate::account::store::now_secs();
        let account = test_suspension_account(tmp.path());
        // Grace starts now, so the tunnel below is admitted at the real clock and only the
        // injected clock is past the deadline: the same store, a later decision.
        seed_suspension_owner(
            &account,
            user_id,
            "user-suspension-token",
            machine_id,
            &identity.public_key,
            Some(now),
            crate::account::billing::entitlement::FREE_HOSTS as usize + 1,
        );
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account.clone())).await;

        let (mut control, mut client, _data, _notice) = open_live_session(&base, &identity).await;

        let virtual_now = now + crate::account::billing::entitlement::GRACE_SECS;
        let sweep = state
            .sweep_suspended_owners(virtual_now)
            .await
            .expect("the sweep must evaluate the store");
        assert_eq!(
            sweep.stopped_owners, 1,
            "the injected clock is past the grace deadline"
        );
        assert_eq!(sweep.closed_sessions, 1);
        assert_eq!(sweep.closed_controls, 1);

        // Prearmed and bounded: the browser half must be told exactly why, and the daemon's
        // control tunnel must end for the same reason instead of lingering.
        let browser_close = timeout(Duration::from_secs(5), client.next())
            .await
            .expect("the browser half must close")
            .expect("the browser half must yield a frame")
            .expect("the browser half must not error");
        match browser_close {
            TMessage::Close(Some(frame)) => {
                assert_eq!(frame.reason.as_str(), "REMOTE_SUSPENDED");
                assert_eq!(u16::from(frame.code), 1008);
            }
            other => panic!("expected REMOTE_SUSPENDED on the browser half, got {other:?}"),
        }
        let control_close = timeout(Duration::from_secs(5), control.next())
            .await
            .expect("the control tunnel must close")
            .expect("the control tunnel must yield a frame")
            .expect("the control tunnel must not error");
        match control_close {
            TMessage::Close(Some(frame)) => assert_eq!(frame.reason.as_str(), "REMOTE_SUSPENDED"),
            other => panic!("expected REMOTE_SUSPENDED on the control tunnel, got {other:?}"),
        }

        // A stop is a state flag: the machine record and the account survive it.
        assert!(
            crate::account::service::owner_stopped_at(&account, user_id).is_some(),
            "the stop must be persisted through the billing store"
        );
        assert!(
            crate::account::service::try_enrolled_machine_by_id(&account, machine_id)
                .expect("store read")
                .is_some(),
            "the machine record must survive the stop"
        );

        server.abort();
    }

    #[tokio::test]
    async fn suspension_skips_healthy_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(vec![]);
        let account = test_suspension_account(tmp.path());
        let now = crate::account::store::now_secs();
        let stopped_id = "stopped-owner-machine";
        let stopped_identity = identity(202, stopped_id);
        seed_suspension_owner(
            &account,
            "user-stopped-owner",
            "stopped-owner-token",
            stopped_id,
            &stopped_identity.public_key,
            Some(now),
            crate::account::billing::entitlement::FREE_HOSTS as usize + 1,
        );
        let healthy_id = "healthy-owner-machine";
        let healthy_identity = identity(203, healthy_id);
        seed_suspension_owner(
            &account,
            "user-healthy-owner",
            "healthy-owner-token",
            healthy_id,
            &healthy_identity.public_key,
            None,
            1,
        );
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account.clone())).await;

        let (mut stopped_control, mut stopped_client, _stopped_data, _) =
            open_live_session(&base, &stopped_identity).await;
        let (mut healthy_control, mut healthy_client, mut healthy_data, _) =
            open_live_session(&base, &healthy_identity).await;

        let virtual_now = now + crate::account::billing::entitlement::GRACE_SECS;
        let sweep = state
            .sweep_suspended_owners(virtual_now)
            .await
            .expect("the sweep must evaluate the store");
        assert_eq!(
            sweep.stopped_owners, 1,
            "only the owner past the deadline is stopped"
        );
        assert_eq!(
            sweep.closed_sessions, 1,
            "the healthy owner's session must survive the pass"
        );
        assert_eq!(sweep.closed_controls, 1);

        let stopped_close = timeout(Duration::from_secs(5), stopped_client.next())
            .await
            .expect("the stopped owner's browser half must close")
            .expect("the stopped owner's browser half must yield a frame")
            .expect("the stopped owner's browser half must not error");
        match stopped_close {
            TMessage::Close(Some(frame)) => assert_eq!(frame.reason.as_str(), "REMOTE_SUSPENDED"),
            other => panic!("expected REMOTE_SUSPENDED on the stopped owner, got {other:?}"),
        }
        let stopped_control_close = timeout(Duration::from_secs(5), stopped_control.next())
            .await
            .expect("the stopped owner's control tunnel must close")
            .expect("the stopped owner's control tunnel must yield a frame")
            .expect("the stopped owner's control tunnel must not error");
        assert!(matches!(
            stopped_control_close,
            TMessage::Close(Some(_))
        ));

        // The healthy owner keeps working: a frame still crosses the live splice and the
        // control tunnel still allocates a session.
        healthy_client
            .send(TMessage::Text("still-live".into()))
            .await
            .unwrap();
        let frame = timeout(Duration::from_secs(5), healthy_data.next())
            .await
            .expect("the healthy session must still forward frames")
            .expect("the healthy daemon half must stay open")
            .expect("the healthy frame read must not error");
        assert_eq!(frame.to_text().unwrap(), "still-live");
        let notice = allocate(&mut healthy_control).await;
        assert!(!notice.session_id.is_empty());

        assert!(
            crate::account::service::owner_stopped_at(&account, "user-healthy-owner").is_none(),
            "a healthy owner must not be marked stopped"
        );
        assert!(
            crate::account::service::owner_stopped_at(&account, "user-stopped-owner").is_some(),
            "the stopped owner must be marked stopped"
        );

        server.abort();
    }

    #[tokio::test]
    async fn resolved_owner_reconnects() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(vec![]);
        let user_id = "user-resolved";
        let machine_id = "resolved-machine";
        let identity = identity(204, machine_id);
        let now = crate::account::store::now_secs();
        let account = test_suspension_account(tmp.path());
        // The violation starts now, so the tunnel and session below are admitted while the
        // owner is still inside the grace window.
        seed_suspension_owner(
            &account,
            user_id,
            "user-resolved-token",
            machine_id,
            &identity.public_key,
            Some(now),
            crate::account::billing::entitlement::FREE_HOSTS as usize + 1,
        );
        let (base, server) =
            spawn_test_relay_with_account_state(state.clone(), Some(account.clone())).await;

        let (mut control, mut client, _data, _notice) = open_live_session(&base, &identity).await;

        // Record that the deadline has passed — the same store change the passage of time
        // makes in production — while the tunnel and the session stay live.
        account
            .mutate(|store| {
                store.billing_states.insert(
                    user_id.into(),
                    crate::account::store::BillingStateRecord {
                        owner_key: user_id.into(),
                        grace_started_at: Some(
                            now.saturating_sub(crate::account::billing::entitlement::GRACE_SECS + 30),
                        ),
                        stopped_at: None,
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .expect("record the end of the grace period");

        // The admission gate already refuses a new tunnel for the stopped owner...
        let (_refused, response) = authenticate_with_token(&base, &identity, false, false, None).await;
        assert!(
            !response.success,
            "a stopped owner must not open a control tunnel"
        );
        assert_eq!(response.error.as_deref(), Some("REMOTE_SUSPENDED"));

        // ...and the sweeper ends the tunnel and the session that predate the stop.
        let sweep = state
            .sweep_suspended_owners(now)
            .await
            .expect("the sweep must evaluate the store");
        assert_eq!(
            sweep.stopped_owners, 1,
            "the real clock is past the grace deadline"
        );
        assert_eq!(sweep.closed_sessions, 1);
        assert_eq!(sweep.closed_controls, 1);
        let browser_close = timeout(Duration::from_secs(5), client.next())
            .await
            .expect("the browser half must close")
            .expect("the browser half must yield a frame")
            .expect("the browser half must not error");
        match browser_close {
            TMessage::Close(Some(frame)) => assert_eq!(frame.reason.as_str(), "REMOTE_SUSPENDED"),
            other => panic!("expected REMOTE_SUSPENDED on the browser half, got {other:?}"),
        }
        assert!(matches!(
            timeout(Duration::from_secs(5), control.next())
                .await
                .expect("the control tunnel must close")
                .expect("the control tunnel must yield a frame")
                .expect("the control tunnel must not error"),
            TMessage::Close(Some(_))
        ));
        assert!(
            crate::account::service::owner_stopped_at(&account, user_id).is_some(),
            "the sweep persists the stop through the billing store"
        );

        // Resolution: the owner upgrades to Pro, so every machine fits the plan limit.
        account
            .mutate(|store| {
                store.subscriptions.insert(
                    "sub-resolved".into(),
                    crate::account::store::SubscriptionRecord {
                        subscription_id: "sub-resolved".into(),
                        owner_user_id: user_id.into(),
                        org_id: None,
                        plan_key: "pro_monthly".into(),
                        seats: 0,
                        host_packs: 0,
                        kind: crate::account::billing::routes::KIND_BASE.into(),
                        status: "active".into(),
                        ends_at: None,
                        ls_customer_id: Some("cust-resolved".into()),
                        ls_updated_at: now,
                        manage_url: None,
                    },
                );
                Ok(())
            })
            .expect("record the upgrade");

        // The owner reconnects: admission and a fresh live session both work again.
        let (_control, _client, _data, notice) = open_live_session(&base, &identity).await;
        assert!(!notice.session_id.is_empty());
        let sweep = state
            .sweep_suspended_owners(now)
            .await
            .expect("the sweep must evaluate the store");
        assert_eq!(sweep.stopped_owners, 0, "a resolved owner is not stopped");
        assert_eq!(sweep.closed_sessions, 0);
        assert!(
            crate::account::service::owner_stopped_at(&account, user_id).is_none(),
            "resolution clears the persisted stop"
        );

        server.abort();
    }

    #[tokio::test]
    async fn suspension_sweeper_ends_a_session_on_its_own_tick() {
        // Virtual time: the sweeper's own interval fires without a wall-clock sleep.
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(vec![]);
        let machine_id = "sweeper-tick-machine";
        let identity = identity(205, machine_id);
        let now = crate::account::store::now_secs();
        let account = test_suspension_account(tmp.path());
        seed_suspension_owner(
            &account,
            "user-sweeper-tick",
            "sweeper-tick-token",
            machine_id,
            &identity.public_key,
            Some(now.saturating_sub(crate::account::billing::entitlement::GRACE_SECS + 30)),
            crate::account::billing::entitlement::FREE_HOSTS as usize + 1,
        );
        state.set_account_state(account);

        let (generation, _notices, _grants) = state.register_control_channel(machine_id.to_string());
        let session_id = "sweeper-tick-session";
        assert!(
            state.issue_session(machine_id, session_id, Some(generation), false),
            "the control channel must accept a session"
        );
        let mut suspension = state.session_suspension_receiver(session_id);

        tokio::time::pause();
        let sweeper = spawn_suspension_sweeper_with_interval(state.clone(), SUSPENSION_SWEEP_INTERVAL);
        // Let the spawned task register its sleep with the timer wheel before time moves.
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        tokio::time::advance(SUSPENSION_SWEEP_INTERVAL).await;
        tokio::time::resume();

        let reason = timeout(Duration::from_secs(5), next_suspension_reason(&mut suspension))
            .await
            .expect("the sweeper must end the session on its own interval");
        assert_eq!(reason, "REMOTE_SUSPENDED");
        sweeper.abort();
    }

    #[tokio::test]
    async fn authenticated_control_socket_client_initiated_close_handshake() {
        use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
        use tokio_tungstenite::tungstenite::protocol::CloseFrame;

        let state = test_state(vec![]);
        let (base, relay) = spawn_test_relay_with_state(state.clone()).await;
        let machine_id = "close-handshake-machine";
        let machine_ident = identity(206, machine_id);

        let (mut control, auth) = authenticate(&base, &machine_ident, false, false).await;
        assert!(auth.success, "control channel must authenticate successfully");

        let close_frame = CloseFrame {
            code: CloseCode::Normal,
            reason: "probe clean close".into(),
        };
        control
            .send(TMessage::Close(Some(close_frame)))
            .await
            .expect("send client close frame");

        let mut observed_close = None;
        let read_close = async {
            while let Some(msg) = control.next().await {
                match msg {
                    Ok(TMessage::Close(frame)) => {
                        observed_close = frame;
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => panic!("unexpected error waiting for close handshake: {error}"),
                }
            }
        };
        timeout(Duration::from_secs(5), read_close)
            .await
            .expect("server must complete the WebSocket close handshake within timeout");

        let frame = observed_close.expect("server must return an echoed close frame");
        assert_eq!(frame.code, CloseCode::Normal);
        assert_eq!(frame.reason.as_str(), "probe clean close");

        relay.abort();
    }
}
