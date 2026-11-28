//! MachineClient hooks for the direct path: pick the request base, report direct
//! transport loss, and start the background negotiator once trust is known.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use url::Url;

const UNPINNED_KICK_TTL: Duration = Duration::from_secs(60);
const KICK_CACHE_MAX_ENTRIES: usize = 256;

#[derive(Clone, Copy, Debug)]
enum KickState {
    InFlight,
    UnpinnedUntil(Instant),
}

static KICK_STATE: LazyLock<Mutex<HashMap<(String, u64), KickState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Claims a `kick()` slot for `(host_id, generation)`. Returns `false` if a
/// lookup is already in flight or if this generation recently resolved as unpinned.
fn try_claim_kick(host_id: &str, generation: u64, now: Instant) -> bool {
    let mut state = KICK_STATE.lock();
    let key = (host_id.to_owned(), generation);
    if let Some(entry) = state.get(&key) {
        match *entry {
            KickState::InFlight => return false,
            KickState::UnpinnedUntil(until) if now < until => return false,
            KickState::UnpinnedUntil(_) => {}
        }
    }
    if state.len() >= KICK_CACHE_MAX_ENTRIES && !state.contains_key(&key) {
        state.retain(|_, entry| match *entry {
            KickState::InFlight => true,
            KickState::UnpinnedUntil(until) => now < until,
        });
        if state.len() >= KICK_CACHE_MAX_ENTRIES {
            state.retain(|_, entry| matches!(*entry, KickState::InFlight));
        }
    }
    state.insert(key, KickState::InFlight);
    true
}

struct KickClaimGuard {
    key: (String, u64),
    unpinned_until: Option<Instant>,
}

impl KickClaimGuard {
    fn mark_unpinned(&mut self, until: Instant) {
        self.unpinned_until = Some(until);
    }
}

impl Drop for KickClaimGuard {
    fn drop(&mut self) {
        let mut state = KICK_STATE.lock();
        if let Some(until) = self.unpinned_until {
            state.insert(self.key.clone(), KickState::UnpinnedUntil(until));
        } else if matches!(state.get(&self.key), Some(KickState::InFlight)) {
            state.remove(&self.key);
        }
    }
}

use super::client::ClientError;
use super::direct_connect::{ensure_direct as start_direct, is_running, DirectAttempt};
use super::direct_route::{RouteState, GLOBAL_DIRECT_ROUTES};
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

/// Gives this process's direct worker a bounded head start before choosing a route.
/// Only a locally pinned (manual or account-attested) host key permits waiting;
/// account discovery still runs in the background on a trust miss.
pub(crate) async fn ensure_direct(
    http: &reqwest::Client,
    host: &HostView,
    lease: &CredentialLease,
    budget: Duration,
) {
    if GLOBAL_DIRECT_ROUTES.direct_for(&host.host_id).is_some() || lease.token().is_err() {
        return;
    }
    // A host already parked in a retry backoff will not be promoted inside this budget: the
    // worker emits nothing while it sleeps and `begin_attempt` refuses to claim. Return at once
    // so the caller goes straight to the relay instead of burning the whole budget.
    if GLOBAL_DIRECT_ROUTES.in_backoff(&host.host_id, std::time::Instant::now()) {
        return;
    }
    // Subscribe before kick: even an immediately promoted or failed attempt is observed.
    let mut events = GLOBAL_DIRECT_ROUTES.subscribe();
    let mut cancelled = lease.cancellation();
    kick(http, host, lease);
    let Some(dir) = identity_dir() else {
        return;
    };
    let wait = async {
        if trusted_key(DirectTrustStore::at(&dir), host.machine_id.clone())
            .await
            .is_none()
        {
            return;
        }
        loop {
            if GLOBAL_DIRECT_ROUTES.direct_for(&host.host_id).is_some() {
                return;
            }
            match events.recv().await {
                Ok(event)
                    if event.host_id == host.host_id
                        && !matches!(event.state, RouteState::Attempting) =>
                {
                    return
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                _ => {}
            }
        }
    };
    tokio::select! { biased;
        _ = cancelled.changed() => {}
        _ = tokio::time::timeout(budget, wait) => {}
    }
}

#[cfg(test)]
tokio::task_local! {
    // Replace only transport negotiation; trust loading, promotion events and client IO stay real.
    static TEST_ATTEMPT: (std::path::PathBuf, tokio::sync::mpsc::UnboundedSender<Option<DirectAttempt>>);
}

fn identity_dir() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    if let Ok(dir) = TEST_ATTEMPT.try_with(|(dir, _)| dir.clone()) {
        return Some(dir);
    }
    crate::remote::auth::canonical_identity_dir().ok()
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
    let Some(dir) = identity_dir() else {
        return;
    };
    #[cfg(test)]
    let test_attempt = TEST_ATTEMPT.try_with(|(_, sender)| sender.clone()).ok();
    #[cfg(test)]
    let bypass_kick_cache = test_attempt.is_some();
    #[cfg(not(test))]
    let bypass_kick_cache = false;
    let claim = if !bypass_kick_cache {
        if !try_claim_kick(&host.host_id, generation, Instant::now()) {
            return;
        }
        Some(KickClaimGuard {
            key: (host.host_id.clone(), generation),
            unpinned_until: None,
        })
    } else {
        None
    };
    let http = http.clone();
    let host = host.clone();
    tokio::spawn(async move {
        let mut claim = claim;
        let attempt = load_attempt(dir, &host, bearer).await;
        if *cancelled.borrow_and_update() {
            return;
        }
        #[cfg(test)]
        if let Some(sender) = test_attempt {
            let _ = sender.send(attempt);
            return;
        }
        let Some(attempt) = attempt else {
            if let Some(guard) = claim.as_mut() {
                guard.mark_unpinned(Instant::now() + UNPINNED_KICK_TTL);
            }
            return;
        };
        drop(claim);
        let cancelled = async move {
            // borrow_and_update first: a flag already set before subscribe counts.
            while !*cancelled.borrow_and_update() {
                if cancelled.changed().await.is_err() {
                    return;
                }
            }
        };
        start_direct(&GLOBAL_DIRECT_ROUTES, http, attempt, generation, cancelled);
    });
}

async fn load_attempt(
    dir: std::path::PathBuf,
    host: &HostView,
    bearer: String,
) -> Option<DirectAttempt> {
    // Read direct-trust.json once; a pinned host proceeds immediately without
    // waiting on account availability.
    let pinned = trusted_key(DirectTrustStore::at(&dir), host.machine_id.clone()).await;
    if pinned.is_some() {
        return load_attempt_with_pinned(dir, host, bearer, pinned, None).await;
    }
    let http = super::direct_autotrust::account_http();
    let account_key = if super::direct_trust::auto_trust_enabled() {
        crate::remote::direct_lease::ensure_account_public_key(Some(&dir), &http)
            .await
            .ok()
    } else {
        None
    };
    load_attempt_with_pinned(
        dir,
        host,
        bearer,
        None,
        account_key.as_deref().map(|key| (&http, key)),
    )
    .await
}

/// [`load_attempt`] against an explicit identity dir. `auto_trust` carries the account HTTP
/// client and the operator-pinned account public key; `None` disables account auto-pinning.
///
/// On a trust miss, a client enrolled with an account fetches the host's account-signed peer
/// record from the account origin it enrolled with, verifies it, pins it and continues. Every
/// failure (not enrolled, no pin, network, bad record) is logged and leaves the route on the relay.
#[cfg(test)]
pub(crate) async fn load_attempt_in(
    dir: std::path::PathBuf,
    host: &HostView,
    bearer: String,
    auto_trust: Option<(&reqwest::Client, &str)>,
) -> Option<DirectAttempt> {
    let pinned = trusted_key(DirectTrustStore::at(&dir), host.machine_id.clone()).await;
    load_attempt_with_pinned(dir, host, bearer, pinned, auto_trust).await
}

async fn load_attempt_with_pinned(
    dir: std::path::PathBuf,
    host: &HostView,
    bearer: String,
    pinned: Option<ed25519_dalek::VerifyingKey>,
    auto_trust: Option<(&reqwest::Client, &str)>,
) -> Option<DirectAttempt> {
    if pinned.is_none() && auto_trust.is_none() {
        return None;
    }
    let identity_dir = dir.clone();
    let identity = crate::ipc::run_blocking(move || {
        crate::remote::auth::load_or_generate_machine_identity(&identity_dir)
            .map_err(crate::ipc::IpcError::internal)
    })
    .await
    .ok()?;
    let host_key = match pinned {
        Some(key) => key,
        None => {
            let (http, account_public_key) = auto_trust?;
            let enrollment_dir = dir.clone();
            let enrollment = crate::ipc::run_blocking(move || {
                Ok(crate::remote::direct_lease::load_host_enrollment(Some(&enrollment_dir)))
            })
            .await
            .ok()
            .flatten()?;
            let attestor = super::direct_autotrust::Attestor {
                http,
                account_origin: &enrollment.account_origin,
                own: &identity,
                account_public_key,
            };
            match super::direct_autotrust::attest_and_pin(
                DirectTrustStore::at(&dir),
                &attestor,
                &host.machine_id,
                None,
                crate::remote::direct_lease::unix_now_secs(),
            )
            .await
            {
                Ok(key) => {
                    tracing::info!(host = %host.machine_id, "direct trust pinned from account record");
                    key
                }
                Err(error) => {
                    tracing::info!(host = %host.machine_id, %error, "no account direct trust; staying on relay");
                    return None;
                }
            }
        }
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::peer_record::{sign_peer_record, test_support::*};
    use crate::account::enroll_client::{AccountEnrollmentRecord, ACCOUNT_ENROLLMENT_FILE};
    use crate::paired_host::inventory::{AuthStatus, GrantScope};
    use crate::scoped_contracts::Epoch;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use std::time::Duration;

    const LIMIT: Duration = Duration::from_secs(15);

    struct DesktopFixture {
        root: tempfile::TempDir,
        service: crate::paired_host::service::PairedHostService,
        host: HostView,
        origin: String,
        server: tokio::task::JoinHandle<()>,
    }

    impl DesktopFixture {
        async fn new(pin: Option<super::super::direct_trust::PinSource>) -> Self {
            use crate::paired_host::inventory::{Inventory, Pairing};
            use axum::{
                routing::{get, post},
                Json, Router,
            };

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let relay_origin = origin.clone();
            let (root, host) = crate::ipc::run_blocking(move || {
                let root = tempfile::tempdir().unwrap();
                let dir = root.path().join("remote");
                crate::remote::auth::load_or_generate_machine_identity(&dir).unwrap();
                if let Some(source) = pin {
                    let store = DirectTrustStore::at(&dir);
                    let key = identity("rd-host", 31).public_key;
                    match source {
                        super::super::direct_trust::PinSource::Manual => {
                            store.provision("rd-host", &key, false).unwrap();
                        }
                        super::super::direct_trust::PinSource::Account => {
                            store.provision_from_account("rd-host", &key, 1).unwrap();
                        }
                    }
                }
                let mut inventory =
                    Inventory::open_test_loopback(&root.path().join("inventory")).unwrap();
                let host = inventory
                    .pair(&Pairing {
                        relay_origin,
                        machine_id: "rd-host".into(),
                        display_label: "rd-host".into(),
                        grant_scope: GrantScope::Machine,
                        device_token: "rd-bearer".into(),
                    })
                    .unwrap();
                Ok((root, host))
            })
            .await
            .unwrap();
            let relay_path = "/host/rd-host/api/v1/remote-desktop/pair";
            let router = Router::new()
                .route(
                    relay_path,
                    post(|| async { Json(serde_json::json!({ "transport": "relay" })) }),
                )
                .route(
                    "/api/v1/remote-desktop/pair",
                    post(|headers: axum::http::HeaderMap| async move {
                        assert_eq!(headers["authorization"], "Bearer rd-bearer");
                        Json(serde_json::json!({ "transport": "direct" }))
                    }),
                )
                .route(
                    "/api/v1/remote-desktop/stream",
                    get(
                        |headers: axum::http::HeaderMap,
                         ws: axum::extract::ws::WebSocketUpgrade| async move {
                            assert_eq!(headers["authorization"], "Bearer rd-bearer");
                            ws.on_upgrade(|mut socket| async move {
                                socket
                                    .send(axum::extract::ws::Message::Binary(vec![7, 8, 9].into()))
                                    .await
                                    .unwrap();
                            })
                        },
                    ),
                );
            let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
            let service = crate::paired_host::service::PairedHostService::open_test_loopback(
                root.path().join("inventory"),
            );
            assert!(GLOBAL_DIRECT_ROUTES.direct_for(&host.host_id).is_none());
            Self {
                root,
                service,
                host,
                origin,
                server,
            }
        }

        fn promote(&self, attempt: DirectAttempt) {
            assert_eq!(attempt.host_id, self.host.host_id);
            assert_eq!(attempt.host_machine_id, self.host.machine_id);
            assert_eq!(attempt.bearer, "rd-bearer");
            assert_eq!(
                attempt.host_key,
                super::super::direct_trust::parse_verifying_key(
                    &identity("rd-host", 31).public_key
                )
                .unwrap()
            );
            let epoch = GLOBAL_DIRECT_ROUTES
                .begin_attempt(&self.host.host_id, std::time::Instant::now())
                .unwrap();
            assert!(GLOBAL_DIRECT_ROUTES.publish(
                &self.host.host_id,
                epoch,
                self.origin.clone(),
                super::super::direct_route::DirectGuard::default(),
            ));
        }

        async fn close(self) {
            GLOBAL_DIRECT_ROUTES.reset(&self.host.host_id);
            self.server.abort();
            assert!(self.server.await.unwrap_err().is_cancelled());
            crate::ipc::run_blocking(move || {
                self.root.close().unwrap();
                Ok(())
            })
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn fresh_remote_desktop_pair_starts_and_uses_direct_for_manual_and_account_pins() {
        use crate::paired_host::{client::MachineClient, direct_trust::PinSource};
        for pin in [PinSource::Manual, PinSource::Account] {
            let fixture = DesktopFixture::new(Some(pin)).await;
            let (sender, mut attempts) = tokio::sync::mpsc::unbounded_channel();
            TEST_ATTEMPT
                .scope((fixture.root.path().join("remote"), sender), async {
                    let client = MachineClient::new();
                    let pair = client.remote_desktop_pair(
                        &fixture.service,
                        fixture.host.host_id.clone(),
                        fixture.host.generation,
                    );
                    let promote = async {
                        fixture.promote(attempts.recv().await.unwrap().expect("trusted attempt"));
                    };
                    let (body, ()) =
                        tokio::time::timeout(LIMIT, async { tokio::join!(pair, promote) })
                            .await
                            .expect("pair observes direct promotion");
                    let body: serde_json::Value = serde_json::from_slice(&body.unwrap()).unwrap();
                    assert_eq!(body["transport"], "direct");
                })
                .await;
            fixture.close().await;
        }
    }

    #[tokio::test]
    async fn fresh_remote_desktop_stream_starts_and_uses_direct() {
        use crate::paired_host::{client::MachineClient, direct_trust::PinSource};
        use futures_util::StreamExt;
        let fixture = DesktopFixture::new(Some(PinSource::Manual)).await;
        let (sender, mut attempts) = tokio::sync::mpsc::unbounded_channel();
        TEST_ATTEMPT
            .scope((fixture.root.path().join("remote"), sender), async {
                let client = MachineClient::new();
                let stream = client.remote_desktop_stream(
                    &fixture.service,
                    fixture.host.host_id.clone(),
                    fixture.host.generation,
                );
                let promote = async {
                    fixture.promote(attempts.recv().await.unwrap().expect("trusted attempt"));
                };
                tokio::time::timeout(LIMIT, async {
                    let (socket, ()) = tokio::join!(stream, promote);
                    let frame = socket.unwrap().next().await.unwrap().unwrap();
                    assert_eq!(frame.into_data().as_ref(), &[7, 8, 9]);
                })
                .await
                .expect("stream observes direct promotion and carries bytes");
            })
            .await;
        fixture.close().await;
    }

    #[tokio::test]
    async fn untrusted_remote_desktop_pair_uses_relay_without_a_promotion_wait() {
        use crate::paired_host::client::MachineClient;
        let fixture = DesktopFixture::new(None).await;
        let (sender, mut attempts) = tokio::sync::mpsc::unbounded_channel();
        TEST_ATTEMPT
            .scope((fixture.root.path().join("remote"), sender), async {
                let (host, lease) = fixture
                    .service
                    .capture_operation(fixture.host.host_id.clone(), fixture.host.generation)
                    .await
                    .unwrap();
                // An unconditional wait would exceed this watchdog. No network negotiation is
                // faked as successful, and no wall-clock latency threshold can make this flaky.
                tokio::time::timeout(
                    LIMIT,
                    ensure_direct(&http(), &host, &lease, Duration::from_secs(60)),
                )
                .await
                .expect("no trusted host key means no promotion wait");
                assert!(tokio::time::timeout(LIMIT, attempts.recv())
                    .await
                    .unwrap()
                    .unwrap()
                    .is_none());
                let body = tokio::time::timeout(
                    LIMIT,
                    MachineClient::new().remote_desktop_pair(
                        &fixture.service,
                        host.host_id.clone(),
                        host.generation,
                    ),
                )
                .await
                .unwrap()
                .unwrap();
                let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["transport"], "relay");
                assert!(tokio::time::timeout(LIMIT, attempts.recv())
                    .await
                    .unwrap()
                    .unwrap()
                    .is_none());
                assert!(GLOBAL_DIRECT_ROUTES.direct_for(&host.host_id).is_none());
            })
            .await;
        fixture.close().await;
    }

    #[tokio::test]
    async fn direct_promotion_budget_expires_without_a_route() {
        use crate::paired_host::direct_trust::PinSource;
        let fixture = DesktopFixture::new(Some(PinSource::Manual)).await;
        let (sender, mut attempts) = tokio::sync::mpsc::unbounded_channel();
        TEST_ATTEMPT
            .scope((fixture.root.path().join("remote"), sender), async {
                let (host, lease) = fixture
                    .service
                    .capture_operation(fixture.host.host_id.clone(), fixture.host.generation)
                    .await
                    .unwrap();
                let http = http();
                let wait = ensure_direct(&http, &host, &lease, Duration::from_secs(3));
                let expire = async {
                    attempts
                        .recv()
                        .await
                        .unwrap()
                        .expect("trusted attempt started");
                    // Time is the behavior under test; advance only after its start signal.
                    tokio::time::pause();
                    tokio::time::advance(Duration::from_secs(3)).await;
                };
                tokio::time::timeout(LIMIT, async { tokio::join!(wait, expire) })
                    .await
                    .expect("unreachable direct path stays bounded");
                tokio::time::resume();
                assert!(request_base(&host).unwrap().direct_epoch.is_none());
                assert!(GLOBAL_DIRECT_ROUTES.direct_for(&host.host_id).is_none());
            })
            .await;
        fixture.close().await;
    }

    #[tokio::test]
    async fn a_host_in_backoff_does_not_consume_the_promotion_budget() {
        use crate::paired_host::direct_trust::PinSource;
        let fixture = DesktopFixture::new(Some(PinSource::Manual)).await;
        let (sender, mut attempts) = tokio::sync::mpsc::unbounded_channel();
        // Park the host where a failed attempt leaves it: inside a retry window, not attempting.
        let epoch = GLOBAL_DIRECT_ROUTES
            .begin_attempt(&fixture.host.host_id, std::time::Instant::now())
            .expect("cold host may attempt");
        GLOBAL_DIRECT_ROUTES.fail(&fixture.host.host_id, epoch, std::time::Instant::now());
        assert!(GLOBAL_DIRECT_ROUTES.in_backoff(&fixture.host.host_id, std::time::Instant::now()));
        TEST_ATTEMPT
            .scope((fixture.root.path().join("remote"), sender), async {
                let (host, lease) = fixture
                    .service
                    .capture_operation(fixture.host.host_id.clone(), fixture.host.generation)
                    .await
                    .unwrap();
                // A 600 s budget would blow this watchdog if the wait were still entered.
                tokio::time::timeout(
                    LIMIT,
                    ensure_direct(&http(), &host, &lease, Duration::from_secs(600)),
                )
                .await
                .expect("a host in backoff must not wait for a promotion it cannot get");
                // The early return precedes `kick`, so no attempt was started at all.
                assert!(matches!(
                    attempts.try_recv(),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty)
                ));
            })
            .await;
        fixture.close().await;
    }

    #[tokio::test]
    async fn published_direct_route_needs_no_trust_lookup_or_negotiation() {
        use crate::paired_host::{client::MachineClient, direct_route::DirectGuard};
        let fixture = DesktopFixture::new(None).await;
        let (sender, mut attempts) = tokio::sync::mpsc::unbounded_channel();
        let epoch = GLOBAL_DIRECT_ROUTES
            .begin_attempt(&fixture.host.host_id, std::time::Instant::now())
            .unwrap();
        assert!(GLOBAL_DIRECT_ROUTES.publish(
            &fixture.host.host_id,
            epoch,
            fixture.origin.clone(),
            DirectGuard::default(),
        ));
        TEST_ATTEMPT
            .scope((fixture.root.path().join("missing"), sender), async {
                let body = tokio::time::timeout(
                    LIMIT,
                    MachineClient::new().remote_desktop_pair(
                        &fixture.service,
                        fixture.host.host_id.clone(),
                        fixture.host.generation,
                    ),
                )
                .await
                .unwrap()
                .unwrap();
                let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["transport"], "direct");
                assert!(matches!(
                    attempts.try_recv(),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty)
                ));
            })
            .await;
        fixture.close().await;
    }

    fn host_view(machine_id: &str) -> HostView {
        HostView {
            host_id: "host-view-1".into(),
            relay_origin: "https://relay.test".into(),
            machine_id: machine_id.into(),
            display_label: "host".into(),
            grant_scope: GrantScope::Machine,
            generation: Epoch(1),
            auth_status: AuthStatus::Paired,
            online: true,
            ssh_host_id: None,
            device_id: None,
            unavailable_reason: None,
            published_origins: Vec::new(),
        }
    }

    /// A client data dir with an identity and an account enrollment pointing at `origin`.
    fn client_dir(origin: &str) -> (tempfile::TempDir, crate::remote::auth::MachineIdentity) {
        let dir = tempfile::tempdir().unwrap();
        let identity = crate::remote::auth::load_or_generate_machine_identity(dir.path()).unwrap();
        let record = AccountEnrollmentRecord {
            account_id: "acct-1".into(),
            machine_record_id: format!("rec-{}", identity.machine_id),
            account_origin: origin.into(),
            relay_origin: "https://relay.test".into(),
            enrollment_epoch: "1".into(),
            enrolled_at: 1,
        };
        std::fs::write(
            dir.path().join(ACCOUNT_ENROLLMENT_FILE),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        (dir, identity)
    }

    async fn attempt(
        dir: &std::path::Path,
        host: &HostView,
        account_key: Option<&str>,
    ) -> Option<DirectAttempt> {
        let http = http();
        tokio::time::timeout(
            LIMIT,
            load_attempt_in(dir.to_path_buf(), host, "bearer".into(), account_key.map(|k| (&http, k))),
        )
        .await
        .expect("bounded attempt")
    }

    #[tokio::test]
    async fn client_valid_account_record_pins_host_and_attempt_proceeds() {
        let host = identity("host-m", 31);
        // Enrollment origin is only known after the account starts; the identity must exist first.
        let probe = tempfile::tempdir().unwrap();
        let client = crate::remote::auth::load_or_generate_machine_identity(probe.path()).unwrap();
        let fx = spawn_account(&[
            ("user-1", &client.machine_id, &client.public_key),
            ("user-1", "host-m", &host.public_key),
        ])
        .await;
        let (dir, _) = client_dir(&fx.origin);
        std::fs::copy(probe.path().join("identity.json"), dir.path().join("identity.json")).unwrap();

        let store = DirectTrustStore::at(dir.path());
        assert_eq!(store.trusted_key("host-m").unwrap(), None);
        let attempt = attempt(dir.path(), &host_view("host-m"), Some(&fx.account_public_key))
            .await
            .expect("an account-attested host key lets the direct attempt proceed");
        let expected = super::super::direct_trust::parse_verifying_key(&host.public_key).unwrap();
        assert_eq!(attempt.host_key, expected);
        assert_eq!(attempt.own_machine_id, client.machine_id);
        assert_eq!(store.trusted_key("host-m").unwrap(), Some(expected), "the key is pinned");
    }

    #[tokio::test]
    async fn client_forged_or_unsigned_record_leaves_no_pin() {
        let host = identity("host-m", 31);
        let account = ed25519_dalek::SigningKey::from_bytes(&[41; 32]);
        let account_public = STANDARD.encode(account.verifying_key().to_bytes());
        let forger = ed25519_dalek::SigningKey::from_bytes(&[42; 32]);
        let now = crate::remote::direct_lease::unix_now_secs();
        let forged =
            sign_peer_record(&forger, "host-m", &host.public_key, "user-1", 1, now + 600).unwrap();
        let wrong_machine =
            sign_peer_record(&account, "host-other", &host.public_key, "user-1", 1, now + 600).unwrap();
        for record in [forged, "unsigned-garbage".to_string(), wrong_machine] {
            let (origin, stub) = spawn_record_stub(record).await;
            let (dir, _) = client_dir(&origin);
            assert!(attempt(dir.path(), &host_view("host-m"), Some(&account_public)).await.is_none());
            assert_eq!(DirectTrustStore::at(dir.path()).trusted_key("host-m").unwrap(), None);
            stub.abort();
        }
    }

    #[tokio::test]
    async fn client_without_enrollment_or_opt_out_stays_on_relay() {
        let host = identity("host-m", 31);
        let account = ed25519_dalek::SigningKey::from_bytes(&[41; 32]);
        let account_public = STANDARD.encode(account.verifying_key().to_bytes());
        let now = crate::remote::direct_lease::unix_now_secs();
        let record = sign_peer_record(&account, "host-m", &host.public_key, "u", 1, now + 600).unwrap();
        let (origin, stub) = spawn_record_stub(record).await;
        // Opt-out (no account key passed): nothing is fetched or pinned.
        let (dir, _) = client_dir(&origin);
        assert!(attempt(dir.path(), &host_view("host-m"), None).await.is_none());
        assert_eq!(DirectTrustStore::at(dir.path()).trusted_key("host-m").unwrap(), None);
        // Not account-enrolled: skipped silently.
        let bare = tempfile::tempdir().unwrap();
        assert!(attempt(bare.path(), &host_view("host-m"), Some(&account_public)).await.is_none());
        assert_eq!(DirectTrustStore::at(bare.path()).trusted_key("host-m").unwrap(), None);
        stub.abort();
    }

    #[test]
    fn kick_negative_cache_deduplicates_in_flight_and_unpinned_hosts_per_generation() {
        let host = "https://relay.test/host/unpinned-cache-test";
        let now = Instant::now();
        assert!(try_claim_kick(host, 1, now));
        // In-flight deduplication suppresses concurrent kicks for the same generation.
        assert!(!try_claim_kick(host, 1, now));
        // A newer generation is not suppressed by an older generation's claim.
        assert!(try_claim_kick(host, 2, now));
        let mut guard = KickClaimGuard {
            key: (host.to_owned(), 1),
            unpinned_until: None,
        };
        guard.mark_unpinned(now + UNPINNED_KICK_TTL);
        drop(guard);
        assert!(!try_claim_kick(host, 1, now + Duration::from_secs(30)));
        assert!(try_claim_kick(host, 1, now + UNPINNED_KICK_TTL));
        // Clean up test keys.
        let mut state = KICK_STATE.lock();
        state.remove(&(host.to_owned(), 1));
        state.remove(&(host.to_owned(), 2));
    }
}

