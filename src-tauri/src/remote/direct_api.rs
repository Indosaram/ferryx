//! Host side of `POST /api/v1/direct/offer`: answer a signed direct (P2P) offer,
//! then punch, accept one pinned QUIC connection and bridge it to the local gateway.
//!
//! Order is load-bearing: bearer auth, Machine+Control scope, identity, structural
//! and replay checks, independent trust lookup and signature verification all run
//! BEFORE any DNS or UDP work. The answer is returned before punching starts, since
//! the client cannot punch until it holds the answer.
use super::server::{authenticate_machine_request, load_gateway_identity, machine_error};
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
use std::time::Duration;
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
}

impl Default for DirectHostConfig {
    fn default() -> Self {
        Self {
            stun_servers: vec!["stun.l.google.com:19302".into(), "stun.cloudflare.com:3478".into()],
            stun_deadline: Duration::from_secs(3),
            connect_deadline: Duration::from_secs(3),
            max_sessions: 8,
            now_ms: unix_now_ms,
        }
    }
}

/// Owns every host direct task. Dropping it aborts the JoinSet, which drops each
/// task's UDP socket, QUIC endpoint and bridge.
pub struct DirectHostManager {
    config: DirectHostConfig,
    permits: Arc<Semaphore>,
    tasks: parking_lot::Mutex<JoinSet<()>>,
    udp_binds: AtomicUsize,
    cancellation_tx: tokio::sync::broadcast::Sender<()>,
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
async fn authorize(state: &Arc<RemoteGatewayState>, headers: &HeaderMap) -> Result<DeviceInfo, Response> {
    let (state, headers) = (Arc::clone(state), headers.clone());
    let device = crate::ipc::run_blocking(move || Ok(authenticate_machine_request(&state, &headers)))
        .await
        .map_err(|_| machine_error(StatusCode::SERVICE_UNAVAILABLE, "MACHINE_SERVICE_UNAVAILABLE"))??;
    if device.access_scope != DeviceAccessScope::Machine || device.permission != DevicePermission::Control {
        return Err(machine_error(StatusCode::FORBIDDEN, "FORBIDDEN"));
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
        Err(response) => response,
    }
}

async fn negotiate(
    state: Arc<RemoteGatewayState>,
    headers: &HeaderMap,
    body: &[u8],
    manager: &DirectHostManager,
) -> Result<DirectAnswerEnvelope, Response> {
    authorize(&state, headers).await?;
    let bad_request = || machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST");
    if body.len() > MAX_DIRECT_BODY_BYTES {
        return Err(machine_error(StatusCode::PAYLOAD_TOO_LARGE, "PAYLOAD_TOO_LARGE"));
    }
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| bad_request())?;
    // Unknown/legacy protocol versions simply get no direct path.
    if value.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err(machine_error(StatusCode::NOT_FOUND, "DIRECT_UNSUPPORTED"));
    }
    let envelope: DirectOfferEnvelope = serde_json::from_value(value).map_err(|_| bad_request())?;

    let identity = load_gateway_identity(Arc::clone(&state)).await?;
    let forbidden = |code| machine_error(StatusCode::FORBIDDEN, code);
    let source = match check_offer_at(&envelope, &identity.machine_id, (manager.config.now_ms)()) {
        Ok(source) => source,
        Err(WireError::Malformed) => return Err(bad_request()),
        Err(WireError::WrongTarget) => return Err(forbidden("DIRECT_WRONG_TARGET")),
        Err(WireError::Expired) => return Err(forbidden("DIRECT_OFFER_EXPIRED")),
        Err(WireError::BadSignature) => return Err(forbidden("DIRECT_BAD_SIGNATURE")),
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
            return Err(machine_error(StatusCode::PAYMENT_REQUIRED, "REMOTE_SUSPENDED"));
        }
    }

    let key = trusted_key(DirectTrustStore::at(&store_dir), source)
        .await
        .ok_or_else(|| forbidden("DIRECT_UNTRUSTED"))?;
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
            return Err(machine_error(StatusCode::PAYMENT_REQUIRED, "REMOTE_SUSPENDED"));
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

#[cfg(test)]
#[path = "direct_api_tests.rs"]
mod tests;
