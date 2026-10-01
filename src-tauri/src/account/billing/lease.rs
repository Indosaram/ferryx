//! Signed host entitlement leases (plan todo 7; the host side is plan todo 10).
//!
//! The account service issues a lease after the host machine proves possession of its enrolled
//! Ed25519 key, and the host then verifies it offline. The exact bytes are:
//!
//! ```text
//! signing input = b"ferryx-account-lease-v1" || 0x00 || payload
//! payload       = compact UTF-8 JSON, no whitespace, keys in exactly this order:
//!                 {"v":1,"machineId":"<machine id>","ownerKey":"<user id or org:id>","expiresAt":<unix seconds>}
//! envelope      = base64(payload) || "." || base64(ed25519 signature over the signing input)
//! ```
//!
//! The domain separator plus the explicit JSON field names bind `machineId`, `ownerKey` and
//! `expiresAt` by name - nothing is concatenated positionally - so an account signature over a
//! lease can never be replayed as another account-signed message and a lease minted for one
//! machine can never authenticate another.
//!
//! A verifier must decode the envelope, require `v == 1`, require `machineId` to be the machine it
//! verifies for, check the signature against the pinned account public key over the signing input,
//! and compare `expiresAt` with its own clock. [`verify_lease`] does all of that and, with
//! [`LeaseClaims`], is the only helper the direct path needs.
//!
//! `expiresAt` is always `min(now + LEASE_TTL_SECS, graceEndsAt)`: a host inside a grace window can
//! never hold a lease that outlives the suspension deadline, and a stopped owner gets no lease at
//! all (`402 REMOTE_SUSPENDED` in the route layer).
//!
//! Field order inside [`LeaseClaims`] is part of the signed payload and must never be changed.
//! Serialization is `serde_json` compact output over declared struct fields: no map ordering, no
//! float encoding, no whitespace.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

/// How long a freshly issued lease is valid without renewal. The host renews every 6 hours and
/// tolerates network failures until this expiry.
pub const LEASE_TTL_SECS: u64 = 86_400;

/// Protocol version carried inside the signed payload; a verifier rejects anything else.
pub const LEASE_VERSION: u32 = 1;

/// Domain separator so an account signature over a lease can never be replayed as a grant or any
/// other account-signed payload.
pub const LEASE_SIGNING_DOMAIN: &[u8] = b"ferryx-account-lease-v1";

/// The exact claims covered by the account signature. Field order is part of the payload bytes,
/// so it must never be reordered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaseClaims {
    pub v: u32,
    pub machine_id: String,
    /// Pool that granted the lease: the bare user id, or `org:<id>` for a team pool.
    pub owner_key: String,
    pub expires_at: u64,
}

/// Lease deadline: the 24 hour TTL, capped by the grace deadline when the owner is inside one.
///
/// The seven day full stop is the user-facing invariant, so this is a hard `min`: while a violation
/// is active (`grace_ends_at` is `Some` and still in the future) the direct path may never hold an
/// authorization that outlives the suspension deadline. A stopped owner is refused by the route
/// before any expiry is computed, so an already-expired lease is never minted.
pub fn lease_expiry(now: u64, grace_ends_at: Option<u64>) -> u64 {
    let ttl_expiry = now.saturating_add(LEASE_TTL_SECS);
    match grace_ends_at {
        Some(grace_ends_at) => ttl_expiry.min(grace_ends_at),
        None => ttl_expiry,
    }
}

/// Bytes the account service signs for `payload` (see the module contract).
fn lease_signing_input(payload: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(LEASE_SIGNING_DOMAIN.len() + 1 + payload.len());
    input.extend_from_slice(LEASE_SIGNING_DOMAIN);
    input.push(0);
    input.extend_from_slice(payload);
    input
}

/// Mints a lease for `machine_id` owned by `owner_key`, valid until `expires_at`.
pub fn sign_lease(
    key: &SigningKey,
    machine_id: &str,
    owner_key: &str,
    expires_at: u64,
) -> Result<String, String> {
    let claims = LeaseClaims {
        v: LEASE_VERSION,
        machine_id: machine_id.to_string(),
        owner_key: owner_key.to_string(),
        expires_at,
    };
    let payload = serde_json::to_vec(&claims).map_err(|error| format!("lease payload: {error}"))?;
    let signature = key.sign(&lease_signing_input(&payload));
    Ok(format!(
        "{}.{}",
        STANDARD.encode(&payload),
        STANDARD.encode(signature.to_bytes())
    ))
}

/// Verifies a lease against the pinned account public key and returns its claims.
///
/// Returns `None` for anything that is not a lease this account signed for `machine_id`:
/// a malformed envelope, a bad signature, a wrong version, or a lease minted for another machine.
/// Callers still compare [`LeaseClaims::expires_at`] with their own clock.
pub fn verify_lease(
    account_public_key: &str,
    machine_id: &str,
    lease: &str,
) -> Option<LeaseClaims> {
    let (payload_b64, signature_b64) = lease.split_once('.')?;
    let payload = STANDARD.decode(payload_b64).ok()?;
    let signature_bytes = STANDARD.decode(signature_b64).ok()?;
    let signature = Signature::from_slice(&signature_bytes).ok()?;
    let key_bytes: [u8; 32] = STANDARD
        .decode(account_public_key.trim())
        .ok()?
        .try_into()
        .ok()?;
    let key = VerifyingKey::from_bytes(&key_bytes).ok()?;
    key.verify(&lease_signing_input(&payload), &signature)
        .ok()?;

    let claims: LeaseClaims = serde_json::from_slice(&payload).ok()?;
    if claims.v != LEASE_VERSION || claims.machine_id != machine_id {
        return None;
    }
    Some(claims)
}

#[cfg(test)]
mod lease_tests {
    use super::*;

    fn test_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn public_key_b64(key: &SigningKey) -> String {
        STANDARD.encode(key.verifying_key().as_bytes())
    }

    #[test]
    fn lease_expiry_caps_the_ttl_at_the_grace_deadline() {
        assert_eq!(lease_expiry(1_000, None), 1_000 + LEASE_TTL_SECS);
        assert_eq!(
            lease_expiry(1_000, Some(1_000 + LEASE_TTL_SECS + 60)),
            1_000 + LEASE_TTL_SECS
        );
        assert_eq!(lease_expiry(1_000, Some(1_300)), 1_300);
        assert_eq!(lease_expiry(u64::MAX, None), u64::MAX);
    }

    #[test]
    fn lease_expiry_boundary_is_the_exact_deadline() {
        let now = 1_000;
        // A deadline equal to the TTL is not a cap: both candidates are the same instant.
        assert_eq!(
            lease_expiry(now, Some(now + LEASE_TTL_SECS)),
            now + LEASE_TTL_SECS
        );
        // One second nearer than the TTL, the deadline wins by exactly one second.
        assert_eq!(
            lease_expiry(now, Some(now + LEASE_TTL_SECS - 1)),
            now + LEASE_TTL_SECS - 1
        );
        // The tightest cap still lands on the deadline, never past it.
        assert_eq!(lease_expiry(now, Some(now + 1)), now + 1);
        // A deadline already in the past stays in the past; routes refuse stopped owners first.
        assert_eq!(lease_expiry(now, Some(now - 7)), now - 7);
    }

    #[test]
    fn signed_lease_round_trips() {
        let key = test_key();
        let lease = sign_lease(&key, "machine-1", "org:team-1", 500_000).expect("sign lease");
        let claims = verify_lease(&public_key_b64(&key), "machine-1", &lease).expect("verify");
        assert_eq!(
            claims,
            LeaseClaims {
                v: LEASE_VERSION,
                machine_id: "machine-1".to_string(),
                owner_key: "org:team-1".to_string(),
                expires_at: 500_000,
            }
        );
    }

    #[test]
    fn tampered_and_foreign_leases_are_rejected() {
        let key = test_key();
        let other = SigningKey::from_bytes(&[9u8; 32]);
        let lease = sign_lease(&key, "machine-1", "user-1", 500_000).expect("sign lease");

        assert!(verify_lease(&public_key_b64(&other), "machine-1", &lease).is_none());
        assert!(verify_lease(&public_key_b64(&key), "machine-2", &lease).is_none());

        let (payload, signature) = lease.split_once('.').expect("envelope");
        let mut flipped = payload.to_string();
        let last = flipped.pop().expect("payload");
        flipped.push(if last == 'A' { 'B' } else { 'A' });
        let tampered = format!("{flipped}.{signature}");
        assert!(verify_lease(&public_key_b64(&key), "machine-1", &tampered).is_none());

        let mut flipped_signature = signature.to_string();
        let last = flipped_signature.pop().expect("signature");
        flipped_signature.push(if last == 'A' { 'B' } else { 'A' });
        let tampered = format!("{payload}.{flipped_signature}");
        assert!(verify_lease(&public_key_b64(&key), "machine-1", &tampered).is_none());

        assert!(verify_lease(&public_key_b64(&key), "machine-1", "not-a-lease").is_none());
        assert!(verify_lease("not-base64", "machine-1", &lease).is_none());
    }
}
