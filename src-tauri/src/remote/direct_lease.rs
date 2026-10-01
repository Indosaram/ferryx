//! Host side of the account entitlement lease (plan todo 10; the account side is todo 7).
//!
//! The account service issues a short-lived, account-signed lease to an enrolled machine
//! (`account::billing::lease`). This module holds the host's copy of that lease, renews it against
//! the account service, and turns a refusal, a revocation or the lease's own expiry into an
//! authoritative "stop the account direct path" signal. The admission and bridge wiring lives in
//! `remote::direct_api`; this module owns the state machine and the HTTP conversation.
//!
//! Contract:
//! - A lease is accepted only when [`verify_lease`] accepts it against the operator-pinned account
//!   key *and* this machine, and only while it has not expired. Signature, issuer, machine and
//!   expiry are all mandatory; an unverifiable response never becomes the host's lease.
//! - Renewal is account-only. A host with no enrollment record never starts the loop and keeps its
//!   LAN / static-token / selfhost direct trust paths exactly as they were.
//! - Network failures (and non-402 refusals) keep the current lease until its own expiry. A
//!   `402 REMOTE_SUSPENDED` revokes immediately.
//! - Expiry timers are generation-aware: a timer armed by an older lease can never revoke a newer
//!   one, and the cancellation watch cancels only while no valid lease remains.
//! - Nothing here deletes credentials, stops PTYs, or restarts the daemon: only the account-scoped
//!   direct transport stops, and a new connection is possible as soon as a lease is held again.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::account::billing::lease::{verify_lease, LeaseClaims};
use crate::account::enroll_client::{AccountEnrollmentRecord, ACCOUNT_ENROLLMENT_FILE};
use crate::remote::auth::{sign_challenge, MachineIdentity};

/// The host renews every six hours, so a 24 hour lease never lapses while the account is
/// reachable. A six hour refresh interval is a refresh cadence only: it never extends the lease's
/// expiry and never becomes an expiry timer.
pub const DEFAULT_RENEWAL_INTERVAL: Duration = Duration::from_secs(6 * 3600);

/// Retry cadence after a refusal or a network error. The current lease stays authoritative until
/// its own expiry; this only bounds how quickly a recovered account is picked up again.
pub const RENEWAL_RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// Operator-pinned account-service Ed25519 public key. The commercial relay already pins this same
/// key under this name to authenticate account-signed payloads, and an enrolled host pins it here
/// to verify the leases that key signs. Without the pin a lease can never be verified, so an
/// enrolled host holds none and the direct path stays refused (fail closed).
pub const ACCOUNT_PUBLIC_KEY_ENV: &str = "FERRYX_RELAY_ACCOUNT_PUBLIC_KEY";

/// Budget for one lease request, matching the enrollment client's HTTP budget.
pub const ACCOUNT_LEASE_HTTP_TIMEOUT: Duration = Duration::from_secs(15);

pub fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostLeaseRecord {
    pub raw_lease: String,
    pub claims: LeaseClaims,
    pub acquired_at: u64,
}

#[derive(Debug, Clone)]
pub struct HostLeaseState {
    current: Arc<RwLock<Option<HostLeaseRecord>>>,
    generation: Arc<AtomicU64>,
    cancellation_notify: Arc<Notify>,
}

impl Default for HostLeaseState {
    fn default() -> Self {
        Self::new()
    }
}

impl HostLeaseState {
    pub fn new() -> Self {
        Self {
            current: Arc::new(RwLock::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
            cancellation_notify: Arc::new(Notify::new()),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn cancellation_notify(&self) -> Arc<Notify> {
        Arc::clone(&self.cancellation_notify)
    }

    pub fn get_valid_lease(&self, now_secs: u64) -> Option<LeaseClaims> {
        let guard = self.current.read();
        if let Some(record) = guard.as_ref() {
            if record.claims.expires_at > now_secs {
                return Some(record.claims.clone());
            }
        }
        None
    }

    /// Installs a lease after verifying it against the pinned account key for `machine_id`.
    ///
    /// Returns `Err("LEASE_VERIFICATION_FAILED")` for a tampered envelope, a lease minted for
    /// another machine or another issuer, and `Err("LEASE_EXPIRED")` for a lease that is already
    /// past its expiry.
    pub fn set_lease(
        &self,
        account_public_key: &str,
        machine_id: &str,
        raw_lease: &str,
        now_secs: u64,
    ) -> Result<LeaseClaims, &'static str> {
        let claims = verify_lease(account_public_key, machine_id, raw_lease)
            .ok_or("LEASE_VERIFICATION_FAILED")?;

        if claims.expires_at <= now_secs {
            return Err("LEASE_EXPIRED");
        }

        let new_gen = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let mut guard = self.current.write();
        *guard = Some(HostLeaseRecord {
            raw_lease: raw_lease.to_string(),
            claims: claims.clone(),
            acquired_at: now_secs,
        });

        let expires_at = claims.expires_at;
        let state_clone = self.clone();
        tokio::spawn(async move {
            let delay_secs = expires_at.saturating_sub(now_secs);
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
            state_clone.expire_if_generation(new_gen, expires_at);
        });

        Ok(claims)
    }

    /// Drops the current lease immediately (a `402 REMOTE_SUSPENDED` from the account service)
    /// and wakes the cancellation watch.
    pub fn revoke(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let mut guard = self.current.write();
        let had_lease = guard.take().is_some();
        drop(guard);

        if had_lease {
            // `notify_one` (not `notify_waiters`) so a revoke that lands just before the watch
            // re-registers still wakes it exactly once.
            self.cancellation_notify.notify_one();
        }
    }

    /// Generation-aware expiry: only the timer armed by the currently held lease may drop it, so
    /// an old timer can never revoke a lease that was renewed after the timer was armed.
    pub fn expire_if_generation(&self, gen: u64, expected_expiry: u64) -> bool {
        if self.generation.load(Ordering::Acquire) != gen {
            return false;
        }

        let mut guard = self.current.write();
        if self.generation.load(Ordering::Acquire) != gen {
            return false;
        }

        if let Some(record) = guard.as_ref() {
            if record.claims.expires_at <= expected_expiry {
                guard.take();
                drop(guard);
                self.cancellation_notify.notify_one();
                return true;
            }
        }
        false
    }
}

/// Requests a lease for this enrolled machine. The signed message is exactly
/// `"{machine_id}:{ts}"`, the bytes `remote::auth::verify_machine_signature` checks on the
/// account side (the account route only trusts the machine's enrolled Ed25519 key).
pub async fn request_lease_from_account(
    http: &reqwest::Client,
    account_origin: &str,
    machine: &MachineIdentity,
    now_secs: u64,
) -> Result<Result<(String, u64), LeaseRefusal>, reqwest::Error> {
    let origin = account_origin.trim_end_matches('/');
    let url = format!("{origin}/api/account/v1/billing/lease");

    let sig = match sign_challenge(machine, &machine.machine_id, now_secs) {
        Ok(signature) => signature,
        Err(_) => {
            return Ok(Err(LeaseRefusal::ClientError(
                "Failed to sign machine challenge".to_string(),
            )))
        }
    };

    let body = serde_json::json!({
        "machineId": machine.machine_id,
        "ts": now_secs,
        "sig": sig,
    });

    let resp = http.post(&url).json(&body).send().await?;
    let status = resp.status();

    if status.as_u16() == 402 {
        return Ok(Err(LeaseRefusal::RemoteSuspended));
    }

    if !status.is_success() {
        return Ok(Err(LeaseRefusal::ServerError(status.as_u16())));
    }

    let payload: serde_json::Value = resp.json().await?;
    let lease_str = payload["lease"].as_str().unwrap_or("").to_string();
    let expires_at = payload["expiresAt"].as_u64().unwrap_or(0);

    Ok(Ok((lease_str, expires_at)))
}

#[derive(Debug, PartialEq, Eq)]
pub enum LeaseRefusal {
    RemoteSuspended,
    ServerError(u16),
    ClientError(String),
}

/// Outcome of one renewal attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseAttempt {
    /// The account returned a lease this host could verify; it is now the authoritative one.
    Renewed,
    /// No new lease. The current lease (if any) stays authoritative until its own expiry.
    Kept,
    /// The account refused remote access (`402 REMOTE_SUSPENDED`): the lease was revoked.
    Revoked,
}

/// Everything a renewal needs. `now_fn` is injectable so tests never touch the process clock and
/// never mutate process-wide environment state.
#[derive(Clone)]
pub struct HostLeaseContext {
    pub enrollment: Option<AccountEnrollmentRecord>,
    pub machine: Option<MachineIdentity>,
    pub account_public_key: Option<String>,
    pub http: reqwest::Client,
    pub now_fn: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl HostLeaseContext {
    pub fn is_account_enrolled(&self) -> bool {
        self.enrollment.is_some() && self.machine.is_some() && self.account_public_key.is_some()
    }
}

/// One renewal attempt: the exact call the renewal loop makes, so a 402 or a network error
/// exercises the production path in tests too.
pub async fn renew_host_lease(state: &HostLeaseState, context: &HostLeaseContext) -> LeaseAttempt {
    let (Some(enrollment), Some(machine), Some(account_public_key)) = (
        context.enrollment.as_ref(),
        context.machine.as_ref(),
        context.account_public_key.as_deref(),
    ) else {
        return LeaseAttempt::Kept;
    };

    let now_secs = (context.now_fn)();
    match request_lease_from_account(&context.http, &enrollment.account_origin, machine, now_secs)
        .await
    {
        Ok(Ok((raw_lease, _expires_at))) => {
            match state.set_lease(account_public_key, &machine.machine_id, &raw_lease, now_secs) {
                Ok(_) => LeaseAttempt::Renewed,
                Err(reason) => {
                    tracing::warn!(
                        reason,
                        "account lease failed local verification; keeping the current lease until expiry"
                    );
                    LeaseAttempt::Kept
                }
            }
        }
        Ok(Err(LeaseRefusal::RemoteSuspended)) => {
            state.revoke();
            LeaseAttempt::Revoked
        }
        Ok(Err(refusal)) => {
            tracing::warn!(
                ?refusal,
                "account lease renewal refused; keeping the current lease until expiry"
            );
            LeaseAttempt::Kept
        }
        Err(error) => {
            tracing::warn!(
                %error,
                "account lease renewal unreachable; keeping the current lease until expiry"
            );
            LeaseAttempt::Kept
        }
    }
}

/// Renewal loop: acquires the first lease immediately, refreshes every
/// [`DEFAULT_RENEWAL_INTERVAL`], and retries every [`RENEWAL_RETRY_INTERVAL`] after a refusal or
/// a network error. A refused host keeps retrying, so resolving the account restores the direct
/// path without restarting the daemon.
pub async fn run_host_lease_renewal(state: Arc<HostLeaseState>, context: HostLeaseContext) {
    loop {
        let attempt = renew_host_lease(&state, &context).await;
        let interval = match attempt {
            LeaseAttempt::Renewed => DEFAULT_RENEWAL_INTERVAL,
            LeaseAttempt::Kept | LeaseAttempt::Revoked => RENEWAL_RETRY_INTERVAL,
        };
        tokio::time::sleep(interval).await;
    }
}

/// Turns lease revocation or real expiry into the host's stop signal for live account bridges.
///
/// The watch cancels only while no valid lease remains, so a renewal that lands first is never
/// torn down by a stale wake-up. Bridges for non-account trust paths do not subscribe to this
/// signal at all, so a billing-scope cancellation never aborts them.
pub fn spawn_lease_cancellation_watch(
    lease: Arc<HostLeaseState>,
    now_fn: Arc<dyn Fn() -> u64 + Send + Sync>,
    cancel: Arc<dyn Fn() + Send + Sync>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let notify = lease.cancellation_notify();
        loop {
            if lease.get_valid_lease(now_fn()).is_none() {
                cancel();
            }
            notify.notified().await;
        }
    })
}

/// `true` when this host carries an account-relay enrollment record.
///
/// Existence alone decides the gate: a damaged record still means "this host was enrolled", so a
/// corrupt file can never silently reopen the account direct path.
///
/// Blocking (filesystem): call through `crate::ipc::run_blocking`.
pub fn host_account_enrolled(identity_dir: Option<&Path>) -> bool {
    enrollment_path(identity_dir).is_some_and(|path| path.exists())
}

/// Blocking: the enrollment record the renewal loop signs requests for, when it parses.
pub fn load_host_enrollment(identity_dir: Option<&Path>) -> Option<AccountEnrollmentRecord> {
    let path = enrollment_path(identity_dir)?;
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Blocking: the operator-pinned account-service signing key.
pub fn pinned_account_public_key() -> Option<String> {
    std::env::var(ACCOUNT_PUBLIC_KEY_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// The gateway's own identity dir wins; otherwise the canonical remote dir that
/// `enroll_machine` writes to.
fn enrollment_path(identity_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = identity_dir {
        let own = dir.join(ACCOUNT_ENROLLMENT_FILE);
        if own.exists() {
            return Some(own);
        }
    }
    crate::account::enroll_client::enrollment_record_path().filter(|path| path.exists())
}

/// Account-lease wiring resolved once at remote-server startup, off the reactor.
pub struct HostLeaseWiring {
    /// The account direct path must hold a valid signed lease.
    pub required: bool,
    /// Present only when this host can actually renew: enrollment record, machine identity and
    /// the operator-pinned account key all resolved.
    pub context: Option<HostLeaseContext>,
}

impl HostLeaseWiring {
    /// No account gate: LAN, static-token and selfhost hosts keep their direct trust paths.
    pub fn none() -> Self {
        Self {
            required: false,
            context: None,
        }
    }

    /// Blocking resolution, intended for `crate::ipc::run_blocking`.
    ///
    /// Reads the enrollment record (and, only when enrolled, the machine identity). Hosts without
    /// an enrollment record never touch the machine identity here.
    pub fn resolve(identity_dir: Option<&Path>) -> Self {
        if !host_account_enrolled(identity_dir) {
            return Self::none();
        }
        let context = Self::build_context(identity_dir);
        Self {
            required: true,
            context,
        }
    }

    fn build_context(identity_dir: Option<&Path>) -> Option<HostLeaseContext> {
        let dir = identity_dir
            .map(Path::to_path_buf)
            .or_else(|| crate::remote::auth::canonical_identity_dir().ok())?;
        let enrollment = load_host_enrollment(identity_dir)?;
        let machine = crate::remote::auth::load_or_generate_machine_identity(&dir).ok()?;
        let account_public_key = pinned_account_public_key()?;
        let http = reqwest::Client::builder()
            .timeout(ACCOUNT_LEASE_HTTP_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Some(HostLeaseContext {
            enrollment: Some(enrollment),
            machine: Some(machine),
            account_public_key: Some(account_public_key),
            http,
            now_fn: Arc::new(unix_now_secs),
        })
    }
}
