//! Host side of `POST /api/v1/direct/offer`: answer a signed direct (P2P) offer,
//! then punch, accept one pinned QUIC connection and bridge it to the local gateway.
//!
//! Order is load-bearing: bearer auth, Machine+Control scope, identity, structural
//! and replay checks, independent trust lookup and signature verification all run
//! BEFORE any DNS or UDP work. The answer is returned before punching starts, since
//! the client cannot punch until it holds the answer.
use super::server::{authenticate_machine_request, load_gateway_identity, machine_error, Refusal};
use crate::paired_host::direct::{generate_ephemeral_cert, DirectConnectionAnswer};
use crate::paired_host::direct_trust::{trusted_key, DirectTrustStore};
use crate::paired_host::direct_wire::{
    answer_session_id, check_offer_at, unix_now_ms, DirectAnswerEnvelope, DirectOfferEnvelope,
    WireError, MAX_DIRECT_BODY_BYTES,
};
use crate::remote::auth::{DeviceAccessScope, DeviceInfo, DevicePermission};
use crate::remote::state::RemoteGatewayState;
use axum::{
    body::Bytes,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

#[path = "direct_host_task.rs"]
mod host_task;

#[derive(Debug, Clone)]
pub struct DirectHostConfig {
    /// `host:port` STUN servers (non-443), resolved only after the offer verifies.
    pub stun_servers: Vec<String>,
    /// Budget for DNS + STUN before the answer is sent.
    pub stun_deadline: Duration,
    /// Budget for punch + QUIC accept after the answer is sent.
    pub connect_deadline: Duration,
    /// Concurrent negotiations plus live bridged connections.
    pub max_sessions: usize,
    /// Replay-window clock; injectable for tests.
    pub now_ms: fn() -> u64,
    /// Operator-pinned account public key used to verify account peer records on a trust miss.
    /// `None` disables account auto-trust (no key pinned, or `FERRYX_DIRECT_AUTO_TRUST=0`).
    pub auto_trust_account_key: Option<String>,
    /// How long a failed account attestation for one source machine is remembered.
    pub auto_trust_negative_ttl: Duration,
}

impl Default for DirectHostConfig {
    fn default() -> Self {
        Self {
            stun_servers: vec!["stun.l.google.com:19302".into(), "stun.cloudflare.com:3478".into()],
            stun_deadline: Duration::from_secs(3),
            connect_deadline: Duration::from_secs(3),
            max_sessions: 8,
            now_ms: unix_now_ms,
            auto_trust_account_key: crate::paired_host::direct_trust::auto_trust_enabled()
                .then(crate::remote::direct_lease::pinned_account_public_key)
                .flatten(),
            auto_trust_negative_ttl: Duration::from_secs(60),
        }
    }
}

/// Upper bound on remembered attestation misses; expired entries are pruned first.
const MAX_AUTO_TRUST_MISSES: usize = 1024;

/// Owns every host direct task. Dropping it aborts the JoinSet, which drops each
/// task's UDP socket, QUIC endpoint and bridge.
pub struct DirectHostManager {
    config: DirectHostConfig,
    permits: Arc<Semaphore>,
    tasks: parking_lot::Mutex<JoinSet<()>>,
    udp_binds: AtomicUsize,
    cancellation_tx: tokio::sync::broadcast::Sender<()>,
    /// Source machine id -> time of the last failed (or in-flight) account attestation.
    auto_trust_misses: parking_lot::Mutex<std::collections::HashMap<String, Instant>>,
    auto_trust_fetches: AtomicUsize,
}

impl DirectHostManager {
    pub fn new(config: DirectHostConfig) -> Self {
        let (cancellation_tx, _) = tokio::sync::broadcast::channel(16);
        Self {
            permits: Arc::new(Semaphore::new(config.max_sessions)),
            config,
            tasks: parking_lot::Mutex::new(JoinSet::new()),
            udp_binds: AtomicUsize::new(0),
            cancellation_tx,
            auto_trust_misses: parking_lot::Mutex::new(std::collections::HashMap::new()),
            auto_trust_fetches: AtomicUsize::new(0),
        }
    }

    /// Cancel all active account-gated bridges immediately (e.g. on lease expiry or 402).
    pub fn cancel_account_bridges(&self) {
        let _ = self.cancellation_tx.send(());
    }

    /// Subscribe to cancellation signal for account-gated bridges.
    pub fn subscribe_cancellation(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.cancellation_tx.subscribe()
    }

    /// UDP sockets ever bound; proves rejected offers never touch UDP.
    pub fn udp_binds(&self) -> usize {
        self.udp_binds.load(Ordering::Acquire)
    }

    pub fn active_tasks(&self) -> usize {
        let mut tasks = self.tasks.lock();
        while tasks.try_join_next().is_some() {}
        tasks.len()
    }

    /// Account peer-record fetches ever started; proves the negative cache bounds them.
    pub fn auto_trust_fetches(&self) -> usize {
        self.auto_trust_fetches.load(Ordering::Acquire)
    }

    /// Claims the right to attest `source` now. `false` while a recent miss (or an in-flight
    /// attempt) for the same source is remembered, so a flood of offers cannot hammer the account.
    fn claim_auto_trust(&self, source: &str, now: Instant) -> bool {
        let ttl = self.config.auto_trust_negative_ttl;
        let mut misses = self.auto_trust_misses.lock();
        if misses.get(source).is_some_and(|at| now.duration_since(*at) < ttl) {
            return false;
        }
        if misses.len() >= MAX_AUTO_TRUST_MISSES {
            misses.retain(|_, at| now.duration_since(*at) < ttl);
            if misses.len() >= MAX_AUTO_TRUST_MISSES {
                return false;
            }
        }
        misses.insert(source.to_owned(), now);
        true
    }

    fn clear_auto_trust_miss(&self, source: &str) {
        self.auto_trust_misses.lock().remove(source);
    }
}

/// The process-wide host manager. Shared as an `Arc` so the lease cancellation watch can hold it
/// for the lifetime of the process.
pub(crate) fn global_manager() -> Arc<DirectHostManager> {
    static MANAGER: OnceLock<Arc<DirectHostManager>> = OnceLock::new();
    Arc::clone(
        MANAGER.get_or_init(|| Arc::new(DirectHostManager::new(DirectHostConfig::default()))),
    )
}

/// Account-lease wiring for the direct path, started once by the remote-server startup.
///
/// The renewal loop acquires the first lease immediately and refreshes it every
/// [`crate::remote::direct_lease::DEFAULT_RENEWAL_INTERVAL`]; the cancellation watch turns a
/// revocation or a real expiry into a stop for every live account-gated bridge, and those bridges
/// release their permits as they end. Non-account hosts pass
/// [`crate::remote::direct_lease::HostLeaseWiring::none`] and start nothing here, so LAN,
/// static-token and selfhost direct trust paths are untouched.
pub(crate) fn start_account_lease_gate(
    state: &Arc<RemoteGatewayState>,
    manager: Arc<DirectHostManager>,
    wiring: crate::remote::direct_lease::HostLeaseWiring,
) {
    if !state.begin_host_lease_wiring() {
        return;
    }
    state.set_host_lease_required(wiring.required);

    let Some(context) = wiring.context else {
        if wiring.required {
            tracing::warn!(
                "account-enrolled host has no verifiable lease pin ({}); direct offers stay \
                 refused until the account public key is configured",
                crate::remote::direct_lease::ACCOUNT_PUBLIC_KEY_ENV
            );
        }
        return;
    };

    crate::remote::direct_lease::spawn_lease_cancellation_watch(
        Arc::clone(&state.host_lease),
        Arc::clone(&context.now_fn),
        Arc::new(move || manager.cancel_account_bridges()),
    );

    let lease_state = Arc::clone(&state.host_lease);
    tokio::spawn(async move {
        crate::remote::direct_lease::run_host_lease_renewal(lease_state, context).await
    });
}

pub(super) async fn direct_offer_handler(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let manager = global_manager();
    handle_offer(state, &headers, &body, &manager).await
}

/// Loopback gateway listener the bridge dials; never a remote-supplied address.
fn local_gateway_addr(state: &RemoteGatewayState) -> Option<SocketAddr> {
    let bound = state.bound_address.read().clone()?;
    let addr: SocketAddr = bound.parse().ok()?;
    (addr.ip().is_loopback() && addr.port() != 0).then_some(addr)
}

/// Token validation may persist auth state, so it runs on a blocking worker.
async fn authorize(state: &Arc<RemoteGatewayState>, headers: &HeaderMap) -> Result<DeviceInfo, Refusal> {
    let (state, headers) = (Arc::clone(state), headers.clone());
    let device = crate::ipc::run_blocking(move || {
        Ok(authenticate_machine_request(&state, &headers).map_err(|response| *response))
    })
        .await
        .map_err(|_| machine_error(StatusCode::SERVICE_UNAVAILABLE, "MACHINE_SERVICE_UNAVAILABLE"))??;
    if device.access_scope != DeviceAccessScope::Machine || device.permission != DevicePermission::Control {
        return Err(machine_error(StatusCode::FORBIDDEN, "FORBIDDEN").into());
    }
    Ok(device)
}

pub async fn handle_offer(
    state: Arc<RemoteGatewayState>,
    headers: &HeaderMap,
    body: &[u8],
    manager: &DirectHostManager,
) -> Response {
    match negotiate(state, headers, body, manager).await {
        Ok(answer) => ([(header::CACHE_CONTROL, "no-store")], Json(answer)).into_response(),
        Err(response) => response.into(),
    }
}

async fn negotiate(
    state: Arc<RemoteGatewayState>,
    headers: &HeaderMap,
    body: &[u8],
    manager: &DirectHostManager,
) -> Result<DirectAnswerEnvelope, Refusal> {
    authorize(&state, headers).await?;
    let bad_request = || machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST");
    if body.len() > MAX_DIRECT_BODY_BYTES {
        return Err(machine_error(StatusCode::PAYLOAD_TOO_LARGE, "PAYLOAD_TOO_LARGE").into());
    }
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| bad_request())?;
    // Unknown/legacy protocol versions simply get no direct path.
    if value.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err(machine_error(StatusCode::NOT_FOUND, "DIRECT_UNSUPPORTED").into());
    }
    let envelope: DirectOfferEnvelope = serde_json::from_value(value).map_err(|_| bad_request())?;

    let identity = load_gateway_identity(Arc::clone(&state)).await?;
    let forbidden = |code| machine_error(StatusCode::FORBIDDEN, code);
    let source = match check_offer_at(&envelope, &identity.machine_id, (manager.config.now_ms)()) {
        Ok(source) => source,
        Err(WireError::Malformed) => return Err(bad_request().into()),
        Err(WireError::WrongTarget) => return Err(forbidden("DIRECT_WRONG_TARGET").into()),
        Err(WireError::Expired) => return Err(forbidden("DIRECT_OFFER_EXPIRED").into()),
        Err(WireError::BadSignature) => return Err(forbidden("DIRECT_BAD_SIGNATURE").into()),
    };
    // Trust comes only from the local store; nothing the relay forwards can add it.
    let unavailable = || machine_error(StatusCode::SERVICE_UNAVAILABLE, "DIRECT_UNAVAILABLE");
    let store_dir = match &state.identity_dir {
        Some(dir) => dir.clone(),
        None => crate::remote::auth::canonical_identity_dir().map_err(|_| unavailable())?,
    };
    // Whether this host is account-enrolled is decided once at startup off the reactor; the
    // reactor must not stat the identity dir on every offer.
    let host_lease_required = state.host_lease_required();

    if host_lease_required {
        let now_secs = (manager.config.now_ms)() / 1000;
        if state.host_lease.get_valid_lease(now_secs).is_none() {
            return Err(machine_error(StatusCode::PAYMENT_REQUIRED, "REMOTE_SUSPENDED").into());
        }
    }

    let key = match trusted_key(DirectTrustStore::at(&store_dir), source.clone()).await {
        Some(key) => key,
        None => account_auto_trust(&state, manager, &store_dir, &identity, &source)
            .await
            .ok_or_else(|| forbidden("DIRECT_UNTRUSTED"))?,
    };
    envelope.offer.verify(&key).map_err(|_| forbidden("DIRECT_BAD_SIGNATURE"))?;

    let gateway = local_gateway_addr(&state).ok_or_else(unavailable)?;
    let seed: [u8; 32] = STANDARD
        .decode(&identity.private_key)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(unavailable)?;
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
    let permit = Arc::clone(&manager.permits)
        .try_acquire_owned()
        .map_err(|_| machine_error(StatusCode::TOO_MANY_REQUESTS, "CAPACITY_EXCEEDED"))?;

    manager.udp_binds.fetch_add(1, Ordering::AcqRel);
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await.map_err(|_| unavailable())?;
    let public_endpoint = tokio::time::timeout(
        manager.config.stun_deadline,
        host_task::discover(&socket, &manager.config.stun_servers),
    )
    .await
    .ok()
    .flatten()
    .ok_or_else(|| machine_error(StatusCode::SERVICE_UNAVAILABLE, "DIRECT_STUN_FAILED"))?;
    // A token revoked during STUN must not start a punch task.
    authorize(&state, headers).await?;
    let cert = generate_ephemeral_cert().map_err(|_| unavailable())?;
    let nonce: [u8; 32] = rand::random();
    let answer = DirectConnectionAnswer::new(
        answer_session_id(&envelope.offer),
        public_endpoint,
        cert.cert_der.clone(),
        nonce,
        &signing_key,
    );

    // If this host is account-enrolled, ensure a valid lease is held before and after STUN.
    // Also attach the cancellation receiver so lease expiry drops this bridge immediately.
    let cancellation = if host_lease_required {
        let now_secs = (manager.config.now_ms)() / 1000;
        if state.host_lease.get_valid_lease(now_secs).is_none() {
            return Err(machine_error(StatusCode::PAYMENT_REQUIRED, "REMOTE_SUSPENDED").into());
        }
        Some(manager.subscribe_cancellation())
    } else {
        None
    };

    // The task is spawned, never awaited: the answer must reach the client first.
    let job = host_task::HostJob {
        socket,
        cert,
        local_nonce: nonce,
        offer: envelope.offer,
        gateway,
        deadline: manager.config.connect_deadline,
        cancellation,
    };
    let mut tasks = manager.tasks.lock();
    while tasks.try_join_next().is_some() {}
    tasks.spawn(async move {
        let _permit = permit;
        job.run().await;
    });
    Ok(DirectAnswerEnvelope { version: 1, answer })
}

/// Host-side account auto-trust for a client with no local pin.
///
/// Only an account-gated host holding a valid lease attempts it, and only a record whose
/// `ownerKey` equals that lease's owner is pinned: a machine of another owner never becomes
/// trusted. A manual pin is never consulted here (the caller only gets here on a miss) and
/// `provision_from_account` refuses to override one or a revoked id. Bounded by the attestation
/// timeout, rate-limited per source by the manager's negative cache; every failure is `None`.
async fn account_auto_trust(
    state: &Arc<RemoteGatewayState>,
    manager: &DirectHostManager,
    store_dir: &std::path::Path,
    identity: &crate::remote::auth::MachineIdentity,
    source: &str,
) -> Option<ed25519_dalek::VerifyingKey> {
    if !crate::paired_host::direct_trust::auto_trust_enabled() || !state.host_lease_required() {
        return None;
    }
    let account_public_key = match manager.config.auto_trust_account_key.clone() {
        Some(key) => key,
        None => {
            let dir = store_dir.to_path_buf();
            crate::ipc::run_blocking(move || {
                Ok(crate::remote::direct_lease::pinned_account_public_key_in(Some(&dir)))
            })
            .await
            .ok()
            .flatten()?
        }
    };
    let account_public_key = account_public_key.as_str();
    let lease = state
        .host_lease
        .get_valid_lease((manager.config.now_ms)() / 1000)?;
    if !manager.claim_auto_trust(source, Instant::now()) {
        tracing::debug!(source, "account direct trust recently failed; not retrying yet");
        return None;
    }
    let enrollment_dir = store_dir.to_path_buf();
    let enrollment = crate::ipc::run_blocking(move || {
        Ok(crate::remote::direct_lease::load_host_enrollment(Some(&enrollment_dir)))
    })
    .await
    .ok()
    .flatten()?;
    manager.auto_trust_fetches.fetch_add(1, Ordering::AcqRel);
    let http = crate::paired_host::direct_autotrust::account_http();
    let attestor = crate::paired_host::direct_autotrust::Attestor {
        http: &http,
        account_origin: &enrollment.account_origin,
        own: identity,
        account_public_key,
    };
    match crate::paired_host::direct_autotrust::attest_and_pin(
        DirectTrustStore::at(store_dir),
        &attestor,
        source,
        Some(&lease.owner_key),
        crate::remote::direct_lease::unix_now_secs(),
    )
    .await
    {
        Ok(key) => {
            manager.clear_auto_trust_miss(source);
            tracing::info!(source, "direct trust pinned from account record");
            Some(key)
        }
        Err(error) => {
            tracing::info!(source, %error, "account direct trust refused; staying on relay");
            None
        }
    }
}

#[cfg(test)]
#[path = "direct_api_tests.rs"]
mod tests;
