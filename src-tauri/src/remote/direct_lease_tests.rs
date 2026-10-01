use super::*;
use crate::account::billing::lease::sign_lease;
use crate::account::enroll_client::AccountEnrollmentRecord;
use crate::remote::auth::{load_or_generate_machine_identity, MachineIdentity};
use crate::remote::direct_lease::{
    host_account_enrolled, renew_host_lease, request_lease_from_account,
    spawn_lease_cancellation_watch, unix_now_secs, HostLeaseContext, HostLeaseState, HostLeaseWiring,
    LeaseAttempt,
};
use ed25519_dalek::SigningKey;

fn test_account_key() -> SigningKey {
    SigningKey::from_bytes(&[42; 32])
}

fn test_wrong_key() -> SigningKey {
    SigningKey::from_bytes(&[99; 32])
}

fn account_public_key_b64(key: &SigningKey) -> String {
    STANDARD.encode(key.verifying_key().to_bytes())
}

fn machine_identity(machine_id: &str) -> MachineIdentity {
    let key = SigningKey::from_bytes(&[5; 32]);
    MachineIdentity {
        machine_id: machine_id.to_string(),
        display_name: "lease fixture".to_string(),
        public_key: STANDARD.encode(key.verifying_key().to_bytes()),
        private_key: STANDARD.encode(key.to_bytes()),
    }
}

fn enrollment_record(account_origin: &str) -> AccountEnrollmentRecord {
    AccountEnrollmentRecord {
        account_id: "acct-1".to_string(),
        machine_record_id: "machine-record-1".to_string(),
        account_origin: account_origin.to_string(),
        relay_origin: "https://relay.test".to_string(),
        enrollment_epoch: "1".to_string(),
        enrolled_at: 1,
    }
}

fn write_enrollment(fx: &Fixture, account_origin: &str) {
    std::fs::write(
        fx._dir
            .path()
            .join(crate::account::enroll_client::ACCOUNT_ENROLLMENT_FILE),
        serde_json::to_vec(&enrollment_record(account_origin)).expect("enrollment json"),
    )
    .expect("write enrollment");
}

fn lease_http() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("lease http client")
}

fn lease_context(
    account_origin: &str,
    machine: MachineIdentity,
    account_public_key: &str,
    now_secs: u64,
) -> HostLeaseContext {
    HostLeaseContext {
        enrollment: Some(enrollment_record(account_origin)),
        machine: Some(machine),
        account_public_key: Some(account_public_key.to_string()),
        http: lease_http(),
        now_fn: Arc::new(move || now_secs),
    }
}

/// A loopback origin nothing listens on: the connection is refused, never answered.
fn unreachable_origin() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let addr = listener.local_addr().expect("probe address");
    drop(listener);
    format!("http://{addr}")
}

async fn lease_402_stub() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind lease stub");
    let addr = listener.local_addr().expect("stub address");
    let app = axum::Router::new().route(
        "/api/account/v1/billing/lease",
        axum::routing::post(|| async { StatusCode::PAYMENT_REQUIRED }),
    );
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, handle)
}

#[tokio::test]
async fn non_account_direct_unaffected() {
    let fx = fixture(true);
    assert!(
        !host_account_enrolled(Some(fx._dir.path())),
        "no enrollment record means no account lease gate"
    );
    assert!(!fx.state.host_lease_required());

    let headers = bearer(&fx.state, DeviceAccessScope::Machine);
    let body = offer_body(&client_key(), &fx.host_id, 1);
    let (status, text, binds) = post(&fx, headers, body, fixed_now).await;
    assert_eq!((status, binds), (503, 0));
    assert!(text.contains("DIRECT_UNAVAILABLE"), "{text}");
    assert!(
        !text.contains("REMOTE_SUSPENDED"),
        "a LAN/selfhost host without any account lease is never refused for billing: {text}"
    );
}

#[tokio::test]
async fn direct_offer_refused_without_lease() {
    let fx = fixture(true);
    write_enrollment(&fx, &unreachable_origin());
    assert!(host_account_enrolled(Some(fx._dir.path())));
    // The startup path resolves this once and stores it; the refusal must hold for an enrolled
    // host whose lease was never obtained.
    fx.state.set_host_lease_required(true);
    assert!(fx.state.host_lease.get_valid_lease(NOW / 1000).is_none());

    let headers = bearer(&fx.state, DeviceAccessScope::Machine);
    let body = offer_body(&client_key(), &fx.host_id, 1);
    let (status, text, binds) = post(&fx, headers, body, fixed_now).await;
    assert_eq!((status, binds), (402, 0));
    assert!(text.contains("REMOTE_SUSPENDED"), "{text}");
}

#[tokio::test]
async fn direct_offer_accepted_with_valid_lease_before_and_after_stun() {
    let fx = fixture(true);
    write_enrollment(&fx, &unreachable_origin());
    fx.state.set_host_lease_required(true);

    let acct_key = test_account_key();
    let pub_key_str = account_public_key_b64(&acct_key);
    let now_secs = NOW / 1000;
    let lease = sign_lease(&acct_key, &fx.host_id, "user-123", now_secs + 3600).unwrap();

    fx.state
        .host_lease
        .set_lease(&pub_key_str, &fx.host_id, &lease, now_secs)
        .expect("valid lease set");

    let headers = bearer(&fx.state, DeviceAccessScope::Machine);
    let body = offer_body(&client_key(), &fx.host_id, 1);
    let (status, text, binds) = post(&fx, headers, body, fixed_now).await;
    assert_eq!((status, binds), (503, 0));
    assert!(text.contains("DIRECT_UNAVAILABLE"), "{text}");
}

#[tokio::test]
async fn lease_verification_rejects_tampering_wrong_machine_wrong_issuer() {
    let host_lease = HostLeaseState::new();
    let acct_key = test_account_key();
    let pub_key_str = account_public_key_b64(&acct_key);
    let now_secs = 1_000_000;

    let valid_lease = sign_lease(&acct_key, "mach-1", "user-1", now_secs + 3600).unwrap();
    assert!(host_lease
        .set_lease(&pub_key_str, "mach-1", &valid_lease, now_secs)
        .is_ok());

    let tampered = format!("{}tampered", valid_lease);
    assert_eq!(
        host_lease.set_lease(&pub_key_str, "mach-1", &tampered, now_secs),
        Err("LEASE_VERIFICATION_FAILED")
    );

    assert_eq!(
        host_lease.set_lease(&pub_key_str, "wrong-mach", &valid_lease, now_secs),
        Err("LEASE_VERIFICATION_FAILED")
    );

    let wrong_key = test_wrong_key();
    let lease_wrong_signer = sign_lease(&wrong_key, "mach-1", "user-1", now_secs + 3600).unwrap();
    assert_eq!(
        host_lease.set_lease(&pub_key_str, "mach-1", &lease_wrong_signer, now_secs),
        Err("LEASE_VERIFICATION_FAILED")
    );

    let expired_lease = sign_lease(&acct_key, "mach-1", "user-1", now_secs - 10).unwrap();
    assert_eq!(
        host_lease.set_lease(&pub_key_str, "mach-1", &expired_lease, now_secs),
        Err("LEASE_EXPIRED")
    );
}

#[tokio::test]
async fn generation_aware_expiry_timer_cannot_revoke_newer_lease() {
    let host_lease = HostLeaseState::new();
    let acct_key = test_account_key();
    let pub_key_str = account_public_key_b64(&acct_key);

    let now_secs = 1000;
    let expiry_old = now_secs + 500;
    let lease_old = sign_lease(&acct_key, "mach-1", "user-1", expiry_old).unwrap();
    host_lease
        .set_lease(&pub_key_str, "mach-1", &lease_old, now_secs)
        .unwrap();

    let gen1 = host_lease.generation();

    let expiry_new = now_secs + 5000;
    let lease_new = sign_lease(&acct_key, "mach-1", "user-1", expiry_new).unwrap();
    host_lease
        .set_lease(&pub_key_str, "mach-1", &lease_new, now_secs)
        .unwrap();

    let gen2 = host_lease.generation();
    assert!(gen2 > gen1);

    let expired = host_lease.expire_if_generation(gen1, expiry_old);
    assert!(!expired, "old timer should not expire newer generation");

    let current = host_lease.get_valid_lease(expiry_old + 10);
    assert!(current.is_some(), "newer lease must remain valid");
    assert_eq!(current.unwrap().expires_at, expiry_new);
}

type LeaseBodySlot = Arc<parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<serde_json::Value>>>>;

async fn capture_lease_body(
    axum::extract::State(slot): axum::extract::State<LeaseBodySlot>,
    body: Bytes,
) -> axum::Json<serde_json::Value> {
    let parsed = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    if let Some(sender) = slot.lock().take() {
        let _ = sender.send(parsed);
    }
    axum::Json(serde_json::json!({ "lease": "", "expiresAt": 0 }))
}

/// The signed request bytes must be exactly the machine challenge the account route verifies.
#[tokio::test]
async fn lease_request_signature_verifies_as_a_machine_challenge() {
    let machine = machine_identity("mach-1");
    let now_secs = 1_700_000_000;
    let (body_tx, body_rx) = tokio::sync::oneshot::channel::<serde_json::Value>();
    let captured: LeaseBodySlot = Arc::new(parking_lot::Mutex::new(Some(body_tx)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind capture stub");
    let addr = listener.local_addr().expect("capture address");
    let app = axum::Router::new()
        .route(
            "/api/account/v1/billing/lease",
            axum::routing::post(capture_lease_body),
        )
        .with_state(Arc::clone(&captured));
    let stub = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let result = request_lease_from_account(
        &lease_http(),
        &format!("http://{addr}"),
        &machine,
        now_secs,
    )
    .await
    .expect("the stub answers");
    assert!(result.is_ok(), "a 200 with a lease body is not a refusal");

    let body = tokio::time::timeout(Duration::from_secs(5), body_rx)
        .await
        .expect("the account stub received the lease request")
        .expect("body captured");
    stub.abort();

    assert_eq!(body["machineId"], machine.machine_id);
    assert_eq!(body["ts"], now_secs);
    let signature = body["sig"].as_str().expect("signature").to_string();
    assert!(
        crate::remote::auth::verify_machine_signature(
            &machine.public_key,
            &machine.machine_id,
            now_secs,
            &signature,
        ),
        "the signed bytes must be exactly {{machine_id}}:{{ts}}"
    );
    assert!(
        !crate::remote::auth::verify_machine_signature(
            &machine.public_key,
            "another-machine",
            now_secs,
            &signature,
        ),
        "the signature must bind this machine id"
    );
}

#[tokio::test]
async fn lease_402_stops_direct_worker() {
    let fx = fixture(true);
    let acct_key = test_account_key();
    let account_public_key = account_public_key_b64(&acct_key);
    let machine = load_or_generate_machine_identity(fx._dir.path()).expect("machine identity");
    assert_eq!(
        machine.machine_id, fx.host_id,
        "the renewal must sign as the enrolled machine"
    );

    let now_secs = NOW / 1000;
    let lease = sign_lease(&acct_key, &fx.host_id, "user-1", now_secs + 3600).unwrap();
    fx.state
        .host_lease
        .set_lease(&account_public_key, &fx.host_id, &lease, now_secs)
        .expect("valid lease");
    assert!(fx.state.host_lease.get_valid_lease(now_secs).is_some());

    let manager = Arc::new(DirectHostManager::new(DirectHostConfig::default()));

    // The live account-gated worker: the same permit + cancellation pair `negotiate` spawns.
    let account_cancellation = manager.subscribe_cancellation();
    let account_permit = Arc::clone(&manager.permits)
        .try_acquire_owned()
        .expect("permit acquired");
    assert_eq!(
        manager.permits.available_permits(),
        manager.config.max_sessions - 1
    );
    let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel::<()>();
    let account_worker = tokio::spawn(async move {
        let _permit = account_permit;
        let mut cancellation = account_cancellation;
        let _ = cancellation.recv().await;
        let _ = stopped_tx.send(());
    });

    // A worker for a non-account trust path subscribes to nothing and must survive the cancel.
    let other_permit = Arc::clone(&manager.permits)
        .try_acquire_owned()
        .expect("permit acquired");
    let non_account_worker = tokio::spawn(async move {
        let _permit = other_permit;
        std::future::pending::<()>().await
    });

    let (stub_addr, stub) = lease_402_stub().await;
    let context = lease_context(
        &format!("http://{stub_addr}"),
        machine,
        &account_public_key,
        now_secs,
    );
    crate::remote::direct_api::start_account_lease_gate(
        &fx.state,
        Arc::clone(&manager),
        HostLeaseWiring {
            required: true,
            context: Some(context),
        },
    );

    // Only the production chain runs from here: the renewal loop's first attempt is a 402, which
    // revokes the lease and must reach the live bridge through the cancellation watch.
    let stopped = tokio::time::timeout(Duration::from_secs(5), stopped_rx)
        .await
        .expect("a 402 must stop the live direct worker within the bound");
    assert!(stopped.is_ok(), "the worker observed the cancellation");

    tokio::time::timeout(Duration::from_secs(5), account_worker)
        .await
        .expect("the cancelled worker ends")
        .expect("worker joined");
    assert_eq!(
        manager.permits.available_permits(),
        manager.config.max_sessions - 1,
        "the cancelled worker released its permit; the untouched non-account worker keeps its own"
    );
    assert!(
        !non_account_worker.is_finished(),
        "a billing-scope cancellation must not stop non-account direct bridges"
    );
    assert!(
        fx.state.host_lease.get_valid_lease(now_secs).is_none(),
        "a 402 revokes the lease"
    );
    assert!(fx.state.host_lease_required());

    // Admission follows the revocation: the next offer is refused with REMOTE_SUSPENDED.
    let headers = bearer(&fx.state, DeviceAccessScope::Machine);
    let body = offer_body(&client_key(), &fx.host_id, 1);
    let (status, text, binds) = post(&fx, headers, body, fixed_now).await;
    assert_eq!((status, binds), (402, 0));
    assert!(text.contains("REMOTE_SUSPENDED"), "{text}");

    stub.abort();
    non_account_worker.abort();
    let _ = non_account_worker.await;
    assert_eq!(
        manager.permits.available_permits(),
        manager.config.max_sessions
    );
}

/// Offline expiry must stop a live bridge and return its permit, driven only by the held lease.
#[tokio::test]
async fn lease_expiry_cancels_live_bridge_and_releases_permit() {
    let host_lease = Arc::new(HostLeaseState::new());
    let acct_key = test_account_key();
    let account_public_key = account_public_key_b64(&acct_key);
    let now_secs = unix_now_secs();

    let manager = Arc::new(DirectHostManager::new(DirectHostConfig::default()));
    let mut cancellation = manager.subscribe_cancellation();
    let permit = Arc::clone(&manager.permits)
        .try_acquire_owned()
        .expect("permit acquired");
    let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel::<()>();
    let worker = tokio::spawn(async move {
        let _permit = permit;
        let _ = cancellation.recv().await;
        let _ = stopped_tx.send(());
    });

    let lease = sign_lease(&acct_key, "mach-expiry", "user-1", now_secs + 1).unwrap();
    host_lease
        .set_lease(&account_public_key, "mach-expiry", &lease, now_secs)
        .expect("valid lease");

    let cancel_manager = Arc::clone(&manager);
    let watch = spawn_lease_cancellation_watch(
        Arc::clone(&host_lease),
        Arc::new(unix_now_secs),
        Arc::new(move || cancel_manager.cancel_account_bridges()),
    );

    let stopped = tokio::time::timeout(Duration::from_secs(10), stopped_rx)
        .await
        .expect("lease expiry must stop the live bridge within the bound");
    assert!(stopped.is_ok(), "the worker observed the expiry");
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("the expired worker ends")
        .expect("worker joined");
    assert_eq!(
        manager.permits.available_permits(),
        manager.config.max_sessions
    );
    assert!(host_lease.get_valid_lease(now_secs + 2).is_none());

    watch.abort();
}

#[tokio::test]
async fn network_error_keeps_lease_until_expiry() {
    let host_lease = HostLeaseState::new();
    let acct_key = test_account_key();
    let account_public_key = account_public_key_b64(&acct_key);
    let now_secs = 1000;
    let expiry = now_secs + 3600;

    let lease = sign_lease(&acct_key, "mach-1", "user-1", expiry).unwrap();
    host_lease
        .set_lease(&account_public_key, "mach-1", &lease, now_secs)
        .expect("valid lease");

    let context = lease_context(
        &unreachable_origin(),
        machine_identity("mach-1"),
        &account_public_key,
        now_secs,
    );
    assert_eq!(
        renew_host_lease(&host_lease, &context).await,
        LeaseAttempt::Kept,
        "a network error must not revoke the lease"
    );
    assert!(host_lease.get_valid_lease(now_secs).is_some());

    assert!(host_lease.get_valid_lease(now_secs + 100).is_some());
    assert!(host_lease.get_valid_lease(now_secs + 3599).is_some());
    assert!(host_lease.get_valid_lease(now_secs + 3600).is_none());
    assert!(host_lease.get_valid_lease(now_secs + 3601).is_none());
}