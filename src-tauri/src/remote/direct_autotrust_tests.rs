use super::*;
use crate::account::billing::lease::sign_lease;
use crate::account::enroll_client::{AccountEnrollmentRecord, ACCOUNT_ENROLLMENT_FILE};
use crate::account::peer_record::{sign_peer_record, test_support::*};
use crate::remote::auth::load_or_generate_machine_identity;

fn client_public() -> String {
    STANDARD.encode(client_key().verifying_key().to_bytes())
}

fn enroll_host(fx: &Fixture, account_origin: &str) {
    let record = AccountEnrollmentRecord {
        account_id: "acct-1".into(),
        machine_record_id: "rec-host".into(),
        account_origin: account_origin.into(),
        relay_origin: "https://relay.test".into(),
        enrollment_epoch: "1".into(),
        enrolled_at: 1,
    };
    std::fs::write(fx._dir.path().join(ACCOUNT_ENROLLMENT_FILE), serde_json::to_vec(&record).unwrap())
        .unwrap();
}

/// Account-gated host holding a valid lease for `owner_key`, signed by `account`.
fn hold_lease(fx: &Fixture, account: &SigningKey, owner_key: &str) {
    fx.state.set_host_lease_required(true);
    let now_secs = NOW / 1000;
    let lease = sign_lease(account, &fx.host_id, owner_key, now_secs + 3600).unwrap();
    let public = STANDARD.encode(account.verifying_key().to_bytes());
    fx.state.host_lease.set_lease(&public, &fx.host_id, &lease, now_secs).expect("lease");
}

fn auto_manager(account_public_key: &str) -> DirectHostManager {
    DirectHostManager::new(DirectHostConfig {
        now_ms: fixed_now,
        auto_trust_account_key: Some(account_public_key.to_string()),
        ..DirectHostConfig::default()
    })
}

async fn post_with(fx: &Fixture, manager: &DirectHostManager) -> (u16, String) {
    let headers = bearer(&fx.state, DeviceAccessScope::Machine);
    let body = offer_body(&client_key(), &fx.host_id, 1);
    let response = tokio::time::timeout(LIMIT, handle_offer(Arc::clone(&fx.state), &headers, &body, manager))
        .await
        .expect("bounded negotiation");
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn host_accepts_account_attested_same_owner_client() {
    let fx = fixture(false);
    let host = load_or_generate_machine_identity(fx._dir.path()).unwrap();
    let account = spawn_account(&[
        ("user-1", CLIENT, &client_public()),
        ("user-1", &host.machine_id, &host.public_key),
    ])
    .await;
    enroll_host(&fx, &account.origin);
    hold_lease(&fx, &account.signing_key, "user-1");

    let manager = auto_manager(&account.account_public_key);
    let (status, text) = post_with(&fx, &manager).await;
    // Past trust and signature verification: only the missing loopback gateway stops it.
    assert_eq!(status, 503, "{text}");
    assert!(text.contains("DIRECT_UNAVAILABLE"), "{text}");
    assert_eq!(
        DirectTrustStore::at(fx._dir.path()).trusted_key(CLIENT).unwrap(),
        Some(client_key().verifying_key()),
        "the attested client key is pinned"
    );
}

#[tokio::test]
async fn host_refuses_foreign_owner_client() {
    // Real account: a cross-owner request is refused by the account itself.
    let fx = fixture(false);
    let host = load_or_generate_machine_identity(fx._dir.path()).unwrap();
    let account = spawn_account(&[
        ("user-2", CLIENT, &client_public()),
        ("user-1", &host.machine_id, &host.public_key),
    ])
    .await;
    enroll_host(&fx, &account.origin);
    hold_lease(&fx, &account.signing_key, "user-1");
    let (status, text) = post_with(&fx, &auto_manager(&account.account_public_key)).await;
    assert_eq!(status, 403, "{text}");
    assert!(text.contains("DIRECT_UNTRUSTED"), "{text}");
    assert_eq!(DirectTrustStore::at(fx._dir.path()).trusted_key(CLIENT).unwrap(), None);

    // A validly signed record for another owner (e.g. an account answering for a different
    // owner scope) is refused by the host's own ownerKey == lease owner check.
    let fx = fixture(false);
    let signer = SigningKey::from_bytes(&[77; 32]);
    let now = crate::remote::direct_lease::unix_now_secs();
    let record = sign_peer_record(&signer, CLIENT, &client_public(), "user-2", 1, now + 600).unwrap();
    let (origin, stub) = spawn_record_stub(record).await;
    enroll_host(&fx, &origin);
    hold_lease(&fx, &signer, "user-1");
    let manager = auto_manager(&STANDARD.encode(signer.verifying_key().to_bytes()));
    let (status, text) = post_with(&fx, &manager).await;
    assert_eq!(status, 403, "{text}");
    assert!(text.contains("DIRECT_UNTRUSTED"), "{text}");
    assert_eq!(DirectTrustStore::at(fx._dir.path()).trusted_key(CLIENT).unwrap(), None);

    // The negative cache keeps a flood of offers from re-fetching.
    let (status, _) = post_with(&fx, &manager).await;
    assert_eq!(status, 403);
    assert_eq!(manager.auto_trust_fetches(), 1, "a recent miss is not re-fetched");
    stub.abort();
}

#[tokio::test]
async fn host_manual_pin_mismatch_wins_over_account() {
    let fx = fixture(false);
    let host = load_or_generate_machine_identity(fx._dir.path()).unwrap();
    let account = spawn_account(&[
        ("user-1", CLIENT, &client_public()),
        ("user-1", &host.machine_id, &host.public_key),
    ])
    .await;
    enroll_host(&fx, &account.origin);
    hold_lease(&fx, &account.signing_key, "user-1");
    let other = STANDARD.encode(SigningKey::from_bytes(&[55; 32]).verifying_key().to_bytes());
    DirectTrustStore::at(fx._dir.path()).provision(CLIENT, &other, false).unwrap();

    let manager = auto_manager(&account.account_public_key);
    let (status, text) = post_with(&fx, &manager).await;
    assert_eq!(status, 403, "{text}");
    assert!(text.contains("DIRECT_BAD_SIGNATURE"), "{text}");
    assert_eq!(manager.auto_trust_fetches(), 0, "a manual pin is never second-guessed by the account");
    assert_eq!(
        DirectTrustStore::at(fx._dir.path()).list().unwrap(),
        vec![(CLIENT.to_string(), other)]
    );
}

#[tokio::test]
async fn host_without_lease_gate_never_auto_trusts() {
    let fx = fixture(false);
    let host = load_or_generate_machine_identity(fx._dir.path()).unwrap();
    let account = spawn_account(&[
        ("user-1", CLIENT, &client_public()),
        ("user-1", &host.machine_id, &host.public_key),
    ])
    .await;
    enroll_host(&fx, &account.origin);
    // Not lease-gated (non-account host): no attestation, stays untrusted.
    let manager = auto_manager(&account.account_public_key);
    let (status, text) = post_with(&fx, &manager).await;
    assert_eq!(status, 403, "{text}");
    assert!(text.contains("DIRECT_UNTRUSTED"), "{text}");
    assert_eq!(manager.auto_trust_fetches(), 0);
}
