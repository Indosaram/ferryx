//! Client-side direct (STUN + punched QUIC) negotiation and background retry.
//!
//! Security: the relay only carries the signed offer/answer and is untrusted. The
//! answer must verify against the independently provisioned host key, and the
//! resulting QUIC connection is exposed only on a loopback listener, so the host
//! still enforces machine bearer auth on every request.

use std::collections::HashMap;
use std::future::Future;
use std::net::{SocketAddr, SocketAddrV4};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use ed25519_dalek::{SigningKey, VerifyingKey};
use parking_lot::Mutex;
use url::Url;

use super::direct::bridge::bridge_tcp_listener_to_quic;
use super::direct::cert::generate_ephemeral_cert;
use super::direct::offer::{DirectConnectionOffer, DirectRole};
use super::direct::punch::execute_nonce_punch;
use super::direct::quic::create_quic_endpoint;
use super::direct::stun::discover_public_endpoint;
use super::direct_route::{DirectGuard, DirectRoutes, RETRY_BASE, RETRY_MAX};
use super::direct_wire::{
    check_answer, offer_session_id, DirectAnswerEnvelope, DirectOfferEnvelope, DIRECT_OFFER_PATH,
    MAX_DIRECT_BODY_BYTES,
};

/// Whole negotiation budget: STUN, offer round trip, punch and QUIC handshake.
pub const DIRECT_ATTEMPT_BUDGET: Duration = Duration::from_secs(3);
const PUNCH_BUDGET: Duration = Duration::from_millis(1200);
const QUIC_SERVER_NAME: &str = "ferryx-direct";
pub const DEFAULT_STUN_SERVERS: [&str; 2] = ["stun.l.google.com:19302", "stun.cloudflare.com:3478"];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DirectConnectError {
    #[error("host does not offer direct connections")]
    Unsupported,
    #[error("direct answer failed verification")]
    Untrusted,
    #[error("direct transport failed: {0}")]
    Transport(String),
}

fn transport(err: impl std::fmt::Display) -> DirectConnectError {
    DirectConnectError::Transport(err.to_string())
}

/// Everything one attempt needs; the caller resolves trust. A caller without a
/// provisioned host key must not start a worker (stay on relay).
#[derive(Clone)]
pub struct DirectAttempt {
    pub host_id: String,
    /// Relay (or host) base URL; the offer goes to `<base>/api/v1/direct/offer`.
    pub base_url: String,
    pub bearer: String,
    pub own_machine_id: String,
    pub host_machine_id: String,
    pub signing_key: SigningKey,
    pub host_key: VerifyingKey,
}

struct Worker {
    token: u64,
    generation: u64,
    handle: tokio::task::AbortHandle,
}

static WORKERS: LazyLock<Mutex<HashMap<String, Worker>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_WORKER_TOKEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Lock order is always WORKERS -> routes, and route teardown happens under the
/// WORKERS lock, so a retiring worker can never reset a successor's route.
fn retire_if_owner(routes: &DirectRoutes, host_id: &str, token: u64) {
    let mut workers = WORKERS.lock();
    if workers.get(host_id).is_some_and(|w| w.token == token) {
        workers.remove(host_id);
        routes.reset(host_id);
    }
}

/// Starts the per-host background worker if none is running. It keeps retrying
/// with backoff, publishes verified routes, re-punches after loss, and exits
/// (resetting the route) when `cancelled` resolves, e.g. credential revocation.
pub fn ensure_direct<C>(
    routes: &'static DirectRoutes,
    http: reqwest::Client,
    attempt: DirectAttempt,
    generation: u64,
    cancelled: C,
) where
    C: Future<Output = ()> + Send + 'static,
{
    let mut workers = WORKERS.lock();
    match workers.get(&attempt.host_id) {
        Some(w) if w.generation >= generation && !w.handle.is_finished() => return,
        Some(_) => {
            // A newer credential generation supersedes the old worker and route.
            if let Some(old) = workers.remove(&attempt.host_id) {
                old.handle.abort();
            }
            routes.reset(&attempt.host_id);
        }
        None => {}
    }
    let token = NEXT_WORKER_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let host_id = attempt.host_id.clone();
    // The task cannot observe WORKERS before this insert: it needs the lock we hold.
    let task = tokio::spawn(async move {
        tokio::select! {
            biased;
            _ = cancelled => {}
            _ = run_worker(routes, &http, &attempt) => {}
        }
        retire_if_owner(routes, &attempt.host_id, token);
    });
    workers.insert(host_id, Worker { token, generation, handle: task.abort_handle() });
}

pub fn is_running(host_id: &str, generation: u64) -> bool {
    WORKERS
        .lock()
        .get(host_id)
        .is_some_and(|w| w.generation >= generation && !w.handle.is_finished())
}

/// Stops the worker for `generation` (or older) and drops its direct route.
/// A worker for a newer generation is left untouched.
pub fn stop_direct(routes: &DirectRoutes, host_id: &str, generation: u64) {
    let mut workers = WORKERS.lock();
    if workers.get(host_id).is_some_and(|w| w.generation > generation) {
        return;
    }
    if let Some(worker) = workers.remove(host_id) {
        worker.handle.abort();
    }
    // Under the lock: an aborted worker's late cleanup sees a different (or no)
    // owner token and leaves any successor untouched.
    routes.reset(host_id);
}

async fn run_worker(routes: &DirectRoutes, http: &reqwest::Client, attempt: &DirectAttempt) {
    let host_id = attempt.host_id.as_str();
    let mut failures: u32 = 0;
    loop {
        let Some(epoch) = routes.begin_attempt(host_id, Instant::now()) else {
            tokio::time::sleep(RETRY_BASE).await;
            continue;
        };
        match tokio::time::timeout(DIRECT_ATTEMPT_BUDGET, negotiate(http, attempt)).await {
            Ok(Ok((origin, conn, guard))) => {
                if !routes.publish(host_id, epoch, origin, guard) {
                    continue;
                }
                failures = 0;
                let reason = conn.closed().await;
                tracing::info!(host_id, %reason, "direct path lost; falling back to relay");
                routes.mark_disconnected(host_id, epoch, Instant::now());
            }
            Ok(Err(DirectConnectError::Unsupported)) => {
                tracing::info!(host_id, "host has no direct route; staying on relay");
                routes.fail(host_id, epoch, Instant::now());
                failures = failures.max(6);
            }
            Ok(Err(err)) => {
                tracing::info!(host_id, %err, "direct attempt failed; staying on relay");
                routes.fail(host_id, epoch, Instant::now());
            }
            Err(_) => {
                tracing::info!(host_id, "direct attempt timed out; staying on relay");
                routes.fail(host_id, epoch, Instant::now());
            }
        }
        failures = failures.saturating_add(1);
        tokio::time::sleep(worker_backoff(failures)).await;
    }
}

pub(crate) fn worker_backoff(failures: u32) -> Duration {
    let shift = failures.saturating_sub(1).min(6);
    (RETRY_BASE * (1u32 << shift)).min(RETRY_MAX)
}

pub(crate) fn offer_url(base_url: &str) -> Result<Url, DirectConnectError> {
    let mut url = Url::parse(base_url).map_err(transport)?;
    url.path_segments_mut()
        .map_err(|_| transport("base url cannot be a base"))?
        .pop_if_empty()
        .extend(["api", "v1"])
        .extend(DIRECT_OFFER_PATH);
    Ok(url)
}

async fn stun_servers() -> Vec<SocketAddr> {
    let mut out = Vec::new();
    for name in DEFAULT_STUN_SERVERS {
        if let Ok(addrs) = tokio::net::lookup_host(name).await {
            out.extend(addrs.filter(SocketAddr::is_ipv4));
        }
    }
    out
}

async fn negotiate(
    http: &reqwest::Client,
    attempt: &DirectAttempt,
) -> Result<(String, quinn::Connection, DirectGuard), DirectConnectError> {
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await.map_err(transport)?;
    let public_endpoint = discover_public_endpoint(&socket, &stun_servers().await)
        .await
        .map_err(transport)?;
    let cert = generate_ephemeral_cert().map_err(transport)?;
    let punch_nonce: [u8; 32] = rand::random();
    let envelope = DirectOfferEnvelope {
        version: 1,
        source_machine_id: attempt.own_machine_id.clone(),
        target_machine_id: attempt.host_machine_id.clone(),
        offer: DirectConnectionOffer::new(
            offer_session_id(&attempt.own_machine_id, &attempt.host_machine_id),
            public_endpoint,
            cert.cert_der.clone(),
            punch_nonce,
            DirectRole::Initiator,
            &attempt.signing_key,
        ),
    };

    let answer = post_offer(http, attempt, &envelope).await?;
    check_answer(&envelope.offer, &answer, &attempt.host_key)
        .map_err(|_| DirectConnectError::Untrusted)?;
    let peer: SocketAddrV4 = answer.answer.public_endpoint;
    execute_nonce_punch(&socket, peer, punch_nonce, answer.answer.punch_nonce, PUNCH_BUDGET)
        .await
        .map_err(transport)?;

    let endpoint = create_quic_endpoint(
        socket.into_std().map_err(transport)?,
        &cert,
        &answer.answer.cert_der,
        false,
    )
    .map_err(transport)?;
    let conn = endpoint
        .connect(SocketAddr::V4(peer), QUIC_SERVER_NAME)
        .map_err(transport)?
        .await
        .map_err(transport)?;

    // Loopback only: the adapter must never be reachable from the network.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(transport)?;
    let origin = format!("http://{}", listener.local_addr().map_err(transport)?);
    let bridge = bridge_tcp_listener_to_quic(listener, conn.clone());
    let mut guard = DirectGuard::new(conn.clone(), endpoint);
    guard.track(bridge.abort_handle());
    Ok((origin, conn, guard))
}

pub(crate) async fn post_offer(
    http: &reqwest::Client,
    attempt: &DirectAttempt,
    envelope: &DirectOfferEnvelope,
) -> Result<DirectAnswerEnvelope, DirectConnectError> {
    let mut response = http
        .post(offer_url(&attempt.base_url)?)
        .bearer_auth(&attempt.bearer)
        .json(envelope)
        .send()
        .await
        .map_err(transport)?;
    let status = response.status();
    // Older hosts/relays reject the unknown route: treat as "no direct", not an error.
    if matches!(status.as_u16(), 403..=405) {
        return Err(DirectConnectError::Unsupported);
    }
    if !status.is_success() {
        return Err(transport(format!("offer rejected: {status}")));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport)? {
        if body.len() + chunk.len() > MAX_DIRECT_BODY_BYTES {
            return Err(DirectConnectError::Untrusted);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| DirectConnectError::Untrusted)
}

#[cfg(test)]
#[path = "direct_connect_tests.rs"]
mod tests;
