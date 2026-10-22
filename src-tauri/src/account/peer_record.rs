//! Account-attested direct (P2P) peer records.
//!
//! The account service signs a short-lived statement that an enrolled machine's identity key is
//! `publicKey` and that it belongs to `ownerKey`. Two machines of the same owner use it to pin
//! each other for the direct path without the manual `ferryx direct-trust` exchange.
//!
//! ```text
//! signing input = b"ferryx-account-direct-peer-v1" || 0x00 || payload
//! payload       = compact UTF-8 JSON, keys in exactly this order:
//!                 {"v":1,"machineId":"..","publicKey":"<base64 ed25519>","ownerKey":"..",
//!                  "enrollmentEpoch":<n>,"expiresAt":<unix seconds>}
//! envelope      = base64(payload) || "." || base64(signature over the signing input)
//! ```
//!
//! The domain separator differs from leases (`ferryx-account-lease-v1`) and grant submissions,
//! so no other account signature can be replayed as a peer record. `enrollmentEpoch` is the
//! account store's per-machine `enrollment_epoch`; a trust store only accepts a key change for a
//! strictly newer epoch.
//!
//! Trust-root caveat: in production the account service runs inside `ferryx-relay`, so the
//! account signing key is not independent of the relay host. Whoever controls the relay host's
//! account key can attest any key for any machine. Users who do not accept that keep manual pins
//! (which always win) or set `FERRYX_DIRECT_AUTO_TRUST=0`.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

pub const PEER_RECORD_VERSION: u32 = 1;
pub const PEER_RECORD_SIGNING_DOMAIN: &[u8] = b"ferryx-account-direct-peer-v1";
/// Short on purpose: a record only seeds a local pin, it is never a long-lived credential.
pub const PEER_RECORD_TTL_SECS: u64 = 86_400;

/// Field order is part of the signed payload; never reorder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PeerRecordClaims {
    pub v: u32,
    pub machine_id: String,
    pub public_key: String,
    pub owner_key: String,
    pub enrollment_epoch: u64,
    pub expires_at: u64,
}

fn signing_input(payload: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(PEER_RECORD_SIGNING_DOMAIN.len() + 1 + payload.len());
    input.extend_from_slice(PEER_RECORD_SIGNING_DOMAIN);
    input.push(0);
    input.extend_from_slice(payload);
    input
}

pub fn sign_peer_record(
    key: &SigningKey,
    machine_id: &str,
    public_key: &str,
    owner_key: &str,
    enrollment_epoch: u64,
    expires_at: u64,
) -> Result<String, String> {
    let claims = PeerRecordClaims {
        v: PEER_RECORD_VERSION,
        machine_id: machine_id.to_string(),
        public_key: public_key.to_string(),
        owner_key: owner_key.to_string(),
        enrollment_epoch,
        expires_at,
    };
    let payload =
        serde_json::to_vec(&claims).map_err(|error| format!("peer record payload: {error}"))?;
    let signature = key.sign(&signing_input(&payload));
    Ok(format!(
        "{}.{}",
        STANDARD.encode(&payload),
        STANDARD.encode(signature.to_bytes())
    ))
}

/// Returns the claims only for a record this account key signed for `machine_id`, with a valid
/// Ed25519 `publicKey`, version 1, and `expiresAt > now_secs`.
pub fn verify_peer_record(
    account_public_key: &str,
    machine_id: &str,
    record: &str,
    now_secs: u64,
) -> Option<PeerRecordClaims> {
    let (payload_b64, signature_b64) = record.split_once('.')?;
    let payload = STANDARD.decode(payload_b64).ok()?;
    let signature = Signature::from_slice(&STANDARD.decode(signature_b64).ok()?).ok()?;
    let key_bytes: [u8; 32] = STANDARD
        .decode(account_public_key.trim())
        .ok()?
        .try_into()
        .ok()?;
    VerifyingKey::from_bytes(&key_bytes)
        .ok()?
        .verify(&signing_input(&payload), &signature)
        .ok()?;
    let claims: PeerRecordClaims = serde_json::from_slice(&payload).ok()?;
    if claims.v != PEER_RECORD_VERSION
        || claims.machine_id != machine_id
        || claims.expires_at <= now_secs
        || !crate::remote::auth::is_valid_public_key(&claims.public_key)
    {
        return None;
    }
    Some(claims)
}

/// The nonce a machine signs (as `"{nonce}:{ts}"`, the same scheme as the lease request) to ask
/// for `target`'s record. Binding caller and target keeps a captured lease-request signature
/// from being replayed here, and one peer-record signature from being reused for another target.
pub fn peer_record_auth_nonce(caller_machine_id: &str, target_machine_id: &str) -> String {
    format!("ferryx-account-direct-peer-v1:{caller_machine_id}:{target_machine_id}")
}

pub const PEER_AUTH_MACHINE_HEADER: &str = "x-ferryx-machine-id";
pub const PEER_AUTH_TS_HEADER: &str = "x-ferryx-machine-ts";
pub const PEER_AUTH_SIG_HEADER: &str = "x-ferryx-machine-sig";

/// Test-only account service and identities shared by the route, client and host tests.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::account::store::{AccountStore, MachineRecord, UserRecord};
    use crate::remote::auth::MachineIdentity;
    use std::sync::Arc;

    pub(crate) fn identity(machine_id: &str, seed: u8) -> MachineIdentity {
        let key = SigningKey::from_bytes(&[seed; 32]);
        MachineIdentity {
            machine_id: machine_id.to_string(),
            display_name: machine_id.to_string(),
            public_key: STANDARD.encode(key.verifying_key().to_bytes()),
            private_key: STANDARD.encode(key.to_bytes()),
        }
    }

    pub(crate) struct AccountFixture {
        pub origin: String,
        pub account_public_key: String,
        pub signing_key: Arc<SigningKey>,
        server: tokio::task::JoinHandle<()>,
        _tmp: tempfile::TempDir,
    }

    impl Drop for AccountFixture {
        fn drop(&mut self) {
            self.server.abort();
        }
    }

    /// A real account router on loopback, seeded with `(owner user id, machine id, public key)`.
    pub(crate) async fn spawn_account(machines: &[(&str, &str, &str)]) -> AccountFixture {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = AccountStore::load(tmp.path()).unwrap();
        for (index, (owner, machine_id, public_key)) in machines.iter().enumerate() {
            store.users.entry(owner.to_string()).or_insert_with(|| UserRecord {
                user_id: owner.to_string(),
                email: format!("{owner}@test.local"),
                created_at: 1,
            });
            store.machines.insert(
                format!("rec-{machine_id}"),
                MachineRecord {
                    machine_record_id: format!("rec-{machine_id}"),
                    owner_user_id: owner.to_string(),
                    machine_id: machine_id.to_string(),
                    display_name: machine_id.to_string(),
                    public_key: public_key.to_string(),
                    attach_public_key: String::new(),
                    relay_origin: "https://relay.test".to_string(),
                    platform: "test".to_string(),
                    enrollment_epoch: 1 + index as u64,
                    enrolled_at: 1,
                    last_seen_at: 0,
                },
            );
        }
        store.save(tmp.path()).unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(tmp.path().join("mail")));
        let state = Arc::new(crate::account::service::AccountState::new(
            tmp.path(),
            "https://relay.test",
            mailer,
        ));
        let signing_key = state.signing_key().unwrap();
        let account_public_key = state.account_public_key();
        let router = crate::account::service::router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        AccountFixture {
            origin: format!("http://{addr}"),
            account_public_key,
            signing_key,
            server,
            _tmp: tmp,
        }
    }

    /// An "account" that answers every peer-record request with `record` (forgery fixtures).
    pub(crate) async fn spawn_record_stub(record: String) -> (String, tokio::task::JoinHandle<()>) {
        let router = axum::Router::new().route(
            "/api/account/v1/machines/{machine_id}/peer-record",
            axum::routing::get(move || {
                let record = record.clone();
                async move { axum::Json(serde_json::json!({ "record": record, "expiresAt": 0 })) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        (format!("http://{addr}"), handle)
    }

    pub(crate) fn http() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap()
    }
}

#[cfg(test)]
mod route_tests {
    use super::test_support::*;
    use super::*;
    use crate::account::store::now_secs;
    use std::time::Duration;

    const LIMIT: Duration = Duration::from_secs(10);

    async fn request(
        origin: &str,
        target: &str,
        caller: Option<(&crate::remote::auth::MachineIdentity, &str)>,
    ) -> (u16, serde_json::Value) {
        let mut builder = http().get(format!("{origin}/api/account/v1/machines/{target}/peer-record"));
        if let Some((identity, nonce_target)) = caller {
            let ts = now_secs();
            let nonce = peer_record_auth_nonce(&identity.machine_id, nonce_target);
            let sig = crate::remote::auth::sign_challenge(identity, &nonce, ts).unwrap();
            builder = builder
                .header(PEER_AUTH_MACHINE_HEADER, &identity.machine_id)
                .header(PEER_AUTH_TS_HEADER, ts.to_string())
                .header(PEER_AUTH_SIG_HEADER, sig);
        }
        let response = tokio::time::timeout(LIMIT, builder.send()).await.expect("bounded").unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(serde_json::Value::Null))
    }

    #[tokio::test]
    async fn peer_record_route_enforces_machine_auth_and_owner() {
        let (a, b, c) = (identity("mach-a", 21), identity("mach-b", 22), identity("mach-c", 23));
        let fx = spawn_account(&[
            ("user-1", "mach-a", &a.public_key),
            ("user-1", "mach-b", &b.public_key),
            ("user-2", "mach-c", &c.public_key),
        ])
        .await;

        // 401: no machine auth at all.
        let (status, body) = request(&fx.origin, "mach-b", None).await;
        assert_eq!(status, 401, "{body}");
        assert_eq!(body["code"], "MACHINE_AUTH_REQUIRED");

        // 401: a signature by a key that is not the caller's enrolled key.
        let impostor = identity("mach-a", 99);
        let (status, body) = request(&fx.origin, "mach-b", Some((&impostor, "mach-b"))).await;
        assert_eq!(status, 401, "{body}");
        assert_eq!(body["code"], "MACHINE_SIGNATURE_INVALID");

        // 401: a valid signature bound to another target is not reusable.
        let (status, _) = request(&fx.origin, "mach-b", Some((&a, "mach-c"))).await;
        assert_eq!(status, 401);

        // 401: an unknown caller machine.
        let stranger = identity("mach-x", 24);
        let (status, _) = request(&fx.origin, "mach-b", Some((&stranger, "mach-b"))).await;
        assert_eq!(status, 401);

        // 403: cross-owner.
        let (status, body) = request(&fx.origin, "mach-c", Some((&a, "mach-c"))).await;
        assert_eq!(status, 403, "{body}");
        assert_eq!(body["code"], "PEER_OWNER_MISMATCH");

        // 404: unknown target.
        let (status, _) = request(&fx.origin, "mach-zz", Some((&a, "mach-zz"))).await;
        assert_eq!(status, 404);

        // 200: same owner; the record verifies against the account key and carries b's key.
        let (status, body) = request(&fx.origin, "mach-b", Some((&a, "mach-b"))).await;
        assert_eq!(status, 200, "{body}");
        let record = body["record"].as_str().unwrap();
        let claims = verify_peer_record(&fx.account_public_key, "mach-b", record, now_secs()).unwrap();
        assert_eq!(claims.public_key, b.public_key);
        assert_eq!(claims.owner_key, "user-1");
        assert_eq!(claims.enrollment_epoch, 2);
        assert!(claims.expires_at <= now_secs() + PEER_RECORD_TTL_SECS);
        assert!(verify_peer_record(&fx.account_public_key, "mach-a", record, now_secs()).is_none());
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;

    fn account() -> SigningKey {
        SigningKey::from_bytes(&[11; 32])
    }

    fn b64(key: &SigningKey) -> String {
        STANDARD.encode(key.verifying_key().to_bytes())
    }

    fn peer_key() -> String {
        b64(&SigningKey::from_bytes(&[12; 32]))
    }

    #[test]
    fn peer_record_round_trips() {
        let record =
            sign_peer_record(&account(), "m-1", &peer_key(), "org:t", 3, NOW + 60).unwrap();
        let claims = verify_peer_record(&b64(&account()), "m-1", &record, NOW).unwrap();
        assert_eq!(
            claims,
            PeerRecordClaims {
                v: 1,
                machine_id: "m-1".into(),
                public_key: peer_key(),
                owner_key: "org:t".into(),
                enrollment_epoch: 3,
                expires_at: NOW + 60,
            }
        );
    }

    #[test]
    fn peer_record_rejects_lease_and_grant_domain_signatures() {
        let key = account();
        // A lease is a different domain: its envelope never verifies as a peer record.
        let lease = crate::account::billing::lease::sign_lease(&key, "m-1", "u", NOW + 60).unwrap();
        assert!(verify_peer_record(&b64(&key), "m-1", &lease, NOW).is_none());

        // The same payload bytes signed under the lease or grant domain are rejected too.
        let claims = PeerRecordClaims {
            v: 1,
            machine_id: "m-1".into(),
            public_key: peer_key(),
            owner_key: "u".into(),
            enrollment_epoch: 1,
            expires_at: NOW + 60,
        };
        let payload = serde_json::to_vec(&claims).unwrap();
        for domain in [
            crate::account::billing::lease::LEASE_SIGNING_DOMAIN,
            crate::remote::account_grants::GRANT_SUBMISSION_DOMAIN,
        ] {
            let mut input = domain.to_vec();
            input.push(0);
            input.extend_from_slice(&payload);
            let forged = format!(
                "{}.{}",
                STANDARD.encode(&payload),
                STANDARD.encode(key.sign(&input).to_bytes())
            );
            assert!(verify_peer_record(&b64(&key), "m-1", &forged, NOW).is_none());
        }
        // A peer record never verifies as a lease.
        let record = sign_peer_record(&key, "m-1", &peer_key(), "u", 1, NOW + 60).unwrap();
        assert!(crate::account::billing::lease::verify_lease(&b64(&key), "m-1", &record).is_none());
    }

    #[test]
    fn peer_record_rejects_other_machine_id() {
        let record = sign_peer_record(&account(), "m-1", &peer_key(), "u", 1, NOW + 60).unwrap();
        assert!(verify_peer_record(&b64(&account()), "m-2", &record, NOW).is_none());
    }

    #[test]
    fn peer_record_rejects_expired_and_foreign_signer() {
        let record = sign_peer_record(&account(), "m-1", &peer_key(), "u", 1, NOW).unwrap();
        assert!(verify_peer_record(&b64(&account()), "m-1", &record, NOW).is_none());
        assert!(verify_peer_record(&b64(&account()), "m-1", &record, NOW - 1).is_some());
        let other = SigningKey::from_bytes(&[13; 32]);
        assert!(verify_peer_record(&b64(&other), "m-1", &record, NOW - 1).is_none());
        let bad_key = sign_peer_record(&account(), "m-1", "not-a-key", "u", 1, NOW + 60).unwrap();
        assert!(verify_peer_record(&b64(&account()), "m-1", &bad_key, NOW).is_none());
    }
}
