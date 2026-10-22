//! MachineClient hooks for the direct path: pick the request base, report direct
//! transport loss, and start the background negotiator once trust is known.

use url::Url;

use super::client::ClientError;
use super::direct_connect::{ensure_direct, is_running, DirectAttempt};
use super::direct_route::GLOBAL_DIRECT_ROUTES;
use super::direct_trust::{trusted_key, DirectTrustStore};
use super::inventory::{CredentialLease, HostView};

/// Base URL for one request/stream plus the direct epoch it used, if any.
/// Callers must reuse one base for a ticket and the socket it authorizes.
pub(crate) struct RequestBase {
    pub url: Url,
    pub direct_epoch: Option<u64>,
}

pub(crate) fn request_base(host: &HostView) -> Result<RequestBase, ClientError> {
    let invalid = || ClientError::local("INVALID_REQUEST");
    if let Some((origin, epoch)) = GLOBAL_DIRECT_ROUTES.direct_for(&host.host_id) {
        if let Ok(url) = Url::parse(&origin) {
            return Ok(RequestBase { url, direct_epoch: Some(epoch) });
        }
    }
    Ok(RequestBase {
        url: Url::parse(&crate::remote::relay_client::live_relay_url(&host.host_id))
            .map_err(|_| invalid())?,
        direct_epoch: None,
    })
}

/// A request over the direct adapter failed at the transport level: fall back.
pub(crate) fn report_direct_loss(host: &HostView, epoch: u64) {
    GLOBAL_DIRECT_ROUTES.mark_disconnected(&host.host_id, epoch, std::time::Instant::now());
}

/// Transport-level failures (not host answers) that justify leaving the direct path.
pub(crate) fn is_transport_failure(error: &ClientError) -> bool {
    error.machine_error.is_none() && matches!(error.code.as_str(), "HOST_UNAVAILABLE" | "TIMEOUT")
}

/// Starts direct negotiation in the background after the relay confirmed the
/// host's machine identity. Without a locally provisioned host key nothing
/// starts and every request stays on the relay. Never blocks the caller.
pub(crate) fn kick(http: &reqwest::Client, host: &HostView, lease: &CredentialLease) {
    let generation = lease.generation().0;
    if is_running(&host.host_id, generation) {
        return;
    }
    // Subscribe before any await so a revocation during trust loading is seen.
    let mut cancelled = lease.cancellation();
    let Ok(bearer) = lease.token().map(str::to_owned) else {
        return;
    };
    let http = http.clone();
    let host = host.clone();
    tokio::spawn(async move {
        let Some(attempt) = load_attempt(&host, bearer).await else {
            return;
        };
        if *cancelled.borrow_and_update() {
            return;
        }
        let cancelled = async move {
            // borrow_and_update first: a flag already set before subscribe counts.
            while !*cancelled.borrow_and_update() {
                if cancelled.changed().await.is_err() {
                    return;
                }
            }
        };
        ensure_direct(&GLOBAL_DIRECT_ROUTES, http, attempt, generation, cancelled);
    });
}

async fn load_attempt(host: &HostView, bearer: String) -> Option<DirectAttempt> {
    let dir = crate::remote::auth::canonical_identity_dir().ok()?;
    let host_key = trusted_key(DirectTrustStore::at(&dir), host.machine_id.clone()).await?;
    let identity = crate::ipc::run_blocking(move || {
        crate::remote::auth::load_or_generate_machine_identity(&dir)
            .map_err(crate::ipc::IpcError::internal)
    })
    .await
    .ok()?;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let seed: [u8; 32] = STANDARD.decode(&identity.private_key).ok()?.try_into().ok()?;
    Some(DirectAttempt {
        host_id: host.host_id.clone(),
        base_url: host.host_id.clone(),
        bearer,
        own_machine_id: identity.machine_id,
        host_machine_id: host.machine_id.clone(),
        signing_key: ed25519_dalek::SigningKey::from_bytes(&seed),
        host_key,
    })
}
