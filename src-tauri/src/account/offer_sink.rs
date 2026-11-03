use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

use crate::remote::account_protocol::{
    AccountGrantOffer, AccountGrantOfferEnvelope, AccountGrantScope,
};
use crate::remote::attach_identity::AttachIdentity;
use crate::remote::auth::{AuthManager, DeviceAccessScope, DevicePermission};
use crate::remote::sealed_offer::open_offer;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountGrantOfferAck {
    pub grant_id: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfferError {
    NotEnrolled,
    WrongMachine,
    WrongEpoch,
    Sealed(String),
    Malformed,
    Expired,
    GrantRefused,
}

impl OfferError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotEnrolled => "ACCOUNT_OFFER_NOT_ENROLLED",
            Self::WrongMachine => "ACCOUNT_OFFER_WRONG_MACHINE",
            Self::WrongEpoch => "ACCOUNT_OFFER_WRONG_EPOCH",
            Self::Sealed(_) => "ACCOUNT_OFFER_UNSEAL_FAILED",
            Self::Malformed => "ACCOUNT_OFFER_MALFORMED",
            Self::Expired => "ACCOUNT_OFFER_EXPIRED",
            Self::GrantRefused => "ACCOUNT_OFFER_GRANT_REFUSED",
        }
    }
}

impl std::fmt::Display for OfferError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sealed(reason) => write!(formatter, "{}: {reason}", self.code()),
            other => write!(formatter, "{}", other.code()),
        }
    }
}

fn scope_pairing(scope: AccountGrantScope) -> (DevicePermission, DeviceAccessScope) {
    match scope {
        AccountGrantScope::Machine => (DevicePermission::Control, DeviceAccessScope::Machine),
        AccountGrantScope::Mirror => (DevicePermission::Control, DeviceAccessScope::Mirror),
    }
}

pub fn open_grant_offer(    attach: &AttachIdentity,
    envelope: &AccountGrantOfferEnvelope,
    machine_id: &str,
    enrollment_epoch: &str,
    now: u64,
) -> Result<AccountGrantOffer, OfferError> {
    if envelope.machine_id != machine_id {
        return Err(OfferError::WrongMachine);
    }
    if envelope.enrollment_epoch != enrollment_epoch {
        return Err(OfferError::WrongEpoch);
    }
    let sealed = STANDARD
        .decode(&envelope.sealed)
        .map_err(|error| OfferError::Sealed(error.to_string()))?;
    let plaintext = open_offer(attach, machine_id, enrollment_epoch, &sealed)
        .map_err(|error| OfferError::Sealed(error.to_string()))?;
    let offer: AccountGrantOffer =
        serde_json::from_slice(&plaintext).map_err(|_| OfferError::Malformed)?;
    if offer.machine_id != machine_id || offer.enrollment_epoch != enrollment_epoch {
        return Err(OfferError::WrongMachine);
    }
    if offer.expires_at <= now {
        return Err(OfferError::Expired);
    }
    Ok(offer)
}

/// True when `signature` is the account service's signature over the submission that carried
/// `envelope` ([`crate::remote::account_grants::submission_signing_input`]), checked against
/// `pinned`, the account key this machine trusts. The seal only hides the offer; anyone who can
/// reach the attach key can seal one, so only this proves the account issued it.
pub fn account_signed(pinned: Option<&str>, envelope: &AccountGrantOfferEnvelope, signature: Option<&str>) -> bool {
    let (Some(pinned), Some(signature)) = (pinned, signature) else {
        return false;
    };
    let submission = crate::remote::account_grants::GrantSubmission {
        machine_id: envelope.machine_id.clone(),
        enrollment_epoch: envelope.enrollment_epoch.clone(),
        envelope: envelope.clone(),
        signature: signature.to_owned(),
    };
    crate::remote::account_grants::verify_grant_signature(&Some(pinned.to_owned()), &submission).is_ok()
}

/// Applies a sealed grant. `account_vouched` is [`account_signed`]'s verdict on the envelope:
/// only then is the offer's same-account machine marker kept; without it the grant still
/// applies, as a plain machine grant that follows the agent-control switch.
pub fn apply_grant_offer(
    auth: &AuthManager,
    attach: &AttachIdentity,
    machine_id: &str,
    enrollment_epoch: &str,
    envelope: &AccountGrantOfferEnvelope,
    account_vouched: bool,
    now: u64,
) -> Result<AccountGrantOfferAck, OfferError> {
    let offer = open_grant_offer(attach, envelope, machine_id, enrollment_epoch, now)?;
    let (permission, scope) = scope_pairing(offer.grant_scope);
    // Only a machine-scope grant can name a same-account machine; a mirror grant never does,
    // and neither does one whose account signature this machine could not verify.
    if offer.account_machine_id.is_some() && !account_vouched {
        tracing::warn!("account grant names a requester machine without a verified account signature; ignoring it");
    }
    let account_machine_id = offer
        .account_machine_id
        .clone()
        .filter(|_| scope == DeviceAccessScope::Machine && account_vouched);
    auth.register_account_machine_capability(
        &offer.pairing_token,
        permission,
        scope,
        Some(offer.device_attach_public_key.clone()),
        account_machine_id,
    )
    .map_err(|_| OfferError::GrantRefused)?;
    Ok(AccountGrantOfferAck {
        grant_id: offer.grant_id,
        status: "ready".to_string(),
    })
}

/// Applies an envelope that arrived for this machine, taking the machine id and epoch
/// from the enrollment record written at enroll time rather than from the wire, and checking
/// the relay-forwarded `account_signature` against the pinned account key
/// ([`crate::remote::direct_lease::pinned_account_public_key`]).
pub fn apply_envelope_for_this_machine(
    auth: &AuthManager,
    envelope: &AccountGrantOfferEnvelope,
    account_signature: Option<&str>,
    now: u64,
) -> Result<AccountGrantOfferAck, OfferError> {
    let record = crate::account::enroll_client::load_enrollment_record()
        .ok_or(OfferError::NotEnrolled)?;
    let attach = crate::remote::attach_identity::load_or_generate_canonical_attach_identity()
        .map_err(OfferError::Sealed)?;
    let dir = crate::remote::auth::canonical_remote_dir()
        .ok_or_else(|| OfferError::Sealed("cannot resolve identity dir".into()))?;
    let identity = crate::remote::auth::load_or_generate_machine_identity(&dir)
        .map_err(|e| OfferError::Sealed(e.to_string()))?;
    if envelope.machine_id != identity.machine_id {
        return Err(OfferError::WrongMachine);
    }
    if record.machine_record_id.is_empty() || envelope.enrollment_epoch != record.enrollment_epoch {
        return Err(OfferError::WrongEpoch);
    }
    let pinned = crate::remote::direct_lease::pinned_account_public_key();
    let vouched = account_signed(pinned.as_deref(), envelope, account_signature);
    apply_grant_offer(
        auth,
        &attach,
        &identity.machine_id,
        &record.enrollment_epoch,
        envelope,
        vouched,
        now,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::sealed_offer::seal_offer;

    fn attach_identity() -> AttachIdentity {
        let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        AttachIdentity {
            public_key: STANDARD.encode(x25519_dalek::PublicKey::from(&secret).as_bytes()),
            private_key: STANDARD.encode(secret.to_bytes()),
        }
    }

    fn auth(manager_dir: &std::path::Path) -> AuthManager {
        AuthManager::with_persistence(Some(manager_dir.join("remote-auth.json")))
    }

    fn envelope(
        attach: &AttachIdentity,
        machine_id: &str,
        epoch: &str,
        token: &str,
        scope: AccountGrantScope,
        expires_at: u64,
    ) -> AccountGrantOfferEnvelope {
        let offer = AccountGrantOffer {
            grant_id: "grant-1".into(),
            machine_id: machine_id.into(),
            enrollment_epoch: epoch.into(),
            pairing_token: token.into(),
            device_label: "MacBook".into(),
            installation_id: "install-1".into(),
            grant_scope: scope,
            expires_at,
            device_attach_public_key: STANDARD.encode([9u8; 32]),
            account_machine_id: None,
        };
        let plaintext = serde_json::to_vec(&offer).expect("serialize");
        let sealed = seal_offer(&attach.public_key, machine_id, epoch, &plaintext).expect("seal");
        AccountGrantOfferEnvelope {
            machine_id: machine_id.into(),
            enrollment_epoch: epoch.into(),
            sealed: STANDARD.encode(sealed),
        }
    }

    #[test]
    fn applied_offer_registers_a_usable_capability() {
        let dir = tempfile::tempdir().expect("temp");
        let attach = attach_identity();
        let manager = auth(dir.path());
        let token = "0f".repeat(32);
        let now = 1_700_000_000;

        let ack = apply_grant_offer(
            &manager,
            &attach,
            "machine-1",
            "2",
            &envelope(&attach, "machine-1", "2", &token, AccountGrantScope::Machine, now + 600),
            false,
            now,
        )
        .expect("apply");
        assert_eq!(ack.status, "ready");
        assert_eq!(ack.grant_id, "grant-1");

        let (_bearer, device) = manager
            .exchange_pairing_code_with_installation(&token, "MacBook", Some("install-1"))
            .expect("the registered capability redeems");
        assert_eq!(device.access_scope, DeviceAccessScope::Machine);
        assert_eq!(device.permission, DevicePermission::Control);
        let attach_key = STANDARD.encode([9u8; 32]);
        assert_eq!(
            device.attach_public_key.as_deref(),
            Some(attach_key.as_str()),
            "the grant's device attach key must land on the issued device"
        );
        assert!(
            manager.device_for_attach_key(&attach_key).is_some(),
            "the device that holds the attach key must be findable"
        );
        assert!(
            manager.device_for_attach_key("unknown-key").is_none(),
            "an unknown attach key must not resolve to a device"
        );
    }

    #[test]
    fn mirror_scope_stays_mirror() {
        let dir = tempfile::tempdir().expect("temp");
        let attach = attach_identity();
        let manager = auth(dir.path());
        let token = "1a".repeat(32);
        let now = 1_700_000_000;
        apply_grant_offer(
            &manager,
            &attach,
            "machine-1",
            "1",
            &envelope(&attach, "machine-1", "1", &token, AccountGrantScope::Mirror, now + 600),
            false,
            now,
        )
        .expect("apply");
        let (_bearer, device) = manager
            .exchange_pairing_code(&token, "Phone")
            .expect("redeem");
        assert_eq!(device.access_scope, DeviceAccessScope::Mirror);
    }

    #[test]
    fn offers_for_another_machine_epoch_or_time_are_refused() {
        let dir = tempfile::tempdir().expect("temp");
        let attach = attach_identity();
        let manager = auth(dir.path());
        let now = 1_700_000_000;

        let other = envelope(&attach, "machine-2", "1", &"2b".repeat(32), AccountGrantScope::Machine, now + 600);
        assert_eq!(
            apply_grant_offer(&manager, &attach, "machine-1", "1", &other, false, now).expect_err("machine"),
            OfferError::WrongMachine
        );

        let stale = envelope(&attach, "machine-1", "1", &"3c".repeat(32), AccountGrantScope::Machine, now + 600);
        assert_eq!(
            apply_grant_offer(&manager, &attach, "machine-1", "9", &stale, false, now).expect_err("epoch"),
            OfferError::WrongEpoch
        );

        let expired = envelope(&attach, "machine-1", "1", &"4d".repeat(32), AccountGrantScope::Machine, now - 1);
        assert_eq!(
            apply_grant_offer(&manager, &attach, "machine-1", "1", &expired, false, now).expect_err("expired"),
            OfferError::Expired
        );

        let mut tampered = envelope(&attach, "machine-1", "1", &"5e".repeat(32), AccountGrantScope::Machine, now + 600);
        let mut bytes = STANDARD.decode(&tampered.sealed).expect("decode");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        tampered.sealed = STANDARD.encode(bytes);
        assert_eq!(
            apply_grant_offer(&manager, &attach, "machine-1", "1", &tampered, false, now).expect_err("tampered"),
            OfferError::Sealed("SEALED_OFFER_DECRYPT_FAILED".into())
        );
    }

    #[test]
    fn envelope_for_this_machine_uses_the_enrollment_record() {
        let dir = tempfile::tempdir().expect("temp");
        let previous = std::env::var_os("FERRYX_DATA_DIR");
        std::env::set_var("FERRYX_DATA_DIR", dir.path());

        let remote = dir.path().join("remote");
        std::fs::create_dir_all(&remote).expect("remote dir");
        let identity = crate::remote::auth::load_or_generate_machine_identity(&remote)
            .expect("machine identity");
        let attach = crate::remote::attach_identity::load_or_generate_canonical_attach_identity()
            .expect("attach identity");
        let record = crate::account::enroll_client::AccountEnrollmentRecord {
            account_id: "acct-1".into(),
            machine_record_id: "record-1".into(),
            account_origin: "https://account.example".into(),
            relay_origin: "https://relay.ferryx.dev".into(),
            enrollment_epoch: "4".into(),
            enrolled_at: 1,
        };
        crate::remote::auth::write_private_json(
            &crate::account::enroll_client::enrollment_record_path().expect("record path"),
            &record,
        )
        .expect("write record");

        let token = "6f".repeat(32);
        let now = 1_700_000_000;
        let envelope = envelope(
            &attach,
            &identity.machine_id,
            "4",
            &token,
            AccountGrantScope::Machine,
            now + 600,
        );
        let manager = AuthManager::with_persistence(Some(dir.path().join("remote-auth.json")));
        let ack = apply_envelope_for_this_machine(&manager, &envelope, None, now).expect("apply");
        assert_eq!(ack.status, "ready");

        let (_bearer, device) = manager
            .exchange_pairing_code_with_installation(&token, "MacBook", Some("install-1"))
            .expect("redeem");
        assert_eq!(device.access_scope, DeviceAccessScope::Machine);

        if let Some(value) = previous {
            std::env::set_var("FERRYX_DATA_DIR", value);
        } else {
            std::env::remove_var("FERRYX_DATA_DIR");
        }
    }

    /// The account-vouched machine id inside the sealed offer lands on the issued device, and
    /// only for a machine-scope grant whose account signature verified: a mirror grant never
    /// carries it, and an unverified one applies without it.
    #[test]
    fn a_vouched_requester_machine_lands_on_the_device_only_for_machine_scope() {
        let dir = tempfile::tempdir().expect("temp");
        let attach = attach_identity();
        let manager = auth(dir.path());
        let now = 1_700_000_000;
        let sealed = |token: &str, scope: AccountGrantScope| {
            let offer = AccountGrantOffer {
                grant_id: "grant-peer".into(),
                machine_id: "machine-1".into(),
                enrollment_epoch: "1".into(),
                pairing_token: token.into(),
                device_label: "Standby".into(),
                installation_id: "mach-standby".into(),
                grant_scope: scope,
                expires_at: now + 600,
                device_attach_public_key: STANDARD.encode([7u8; 32]),
                account_machine_id: Some("mach-standby".into()),
            };
            let plaintext = serde_json::to_vec(&offer).expect("serialize");
            AccountGrantOfferEnvelope {
                machine_id: "machine-1".into(),
                enrollment_epoch: "1".into(),
                sealed: STANDARD.encode(seal_offer(&attach.public_key, "machine-1", "1", &plaintext).expect("seal")),
            }
        };
        let machine = "7a".repeat(32);
        apply_grant_offer(&manager, &attach, "machine-1", "1", &sealed(&machine, AccountGrantScope::Machine), true, now)
            .expect("apply machine grant");
        let (_, device) = manager
            .exchange_pairing_code_with_installation(&machine, "Standby", Some("mach-standby"))
            .expect("redeem");
        assert_eq!(device.account_machine_id.as_deref(), Some("mach-standby"));

        let mirror = "8b".repeat(32);
        apply_grant_offer(&manager, &attach, "machine-1", "1", &sealed(&mirror, AccountGrantScope::Mirror), true, now)
            .expect("apply mirror grant");
        let (_, device) = manager.exchange_pairing_code(&mirror, "Phone").expect("redeem");
        assert_eq!(device.account_machine_id, None, "a mirror grant never names a machine");

        let unverified = "9c".repeat(32);
        apply_grant_offer(&manager, &attach, "machine-1", "1", &sealed(&unverified, AccountGrantScope::Machine), false, now)
            .expect("an unverified grant still applies");
        let (_, device) = manager
            .exchange_pairing_code_with_installation(&unverified, "Standby", Some("mach-standby"))
            .expect("redeem");
        assert_eq!(device.account_machine_id, None, "an unverified offer never names a machine");
    }

    /// F2 MINOR-2: the marker is trusted only when the account's signature over the envelope
    /// verifies against the pinned account key. An offer a relay sealed itself (no or a wrong
    /// signature), one signed by another key, or one with no pinned key is not vouched for.
    #[test]
    fn only_an_offer_the_pinned_account_key_signed_is_vouched_for() {
        use ed25519_dalek::{Signer, SigningKey};
        let envelope = AccountGrantOfferEnvelope {
            machine_id: "machine-1".into(),
            enrollment_epoch: "1".into(),
            sealed: STANDARD.encode(b"sealed-offer"),
        };
        let sign = |key: &SigningKey, envelope: &AccountGrantOfferEnvelope| {
            let submission = crate::remote::account_grants::GrantSubmission {
                machine_id: envelope.machine_id.clone(),
                enrollment_epoch: envelope.enrollment_epoch.clone(),
                envelope: envelope.clone(),
                signature: String::new(),
            };
            STANDARD.encode(key.sign(&crate::remote::account_grants::submission_signing_input(&submission)).to_bytes())
        };
        let account = SigningKey::from_bytes(&[11u8; 32]);
        let pinned = STANDARD.encode(account.verifying_key().as_bytes());
        let signature = sign(&account, &envelope);

        assert!(account_signed(Some(&pinned), &envelope, Some(&signature)));
        assert!(!account_signed(Some(&pinned), &envelope, None), "an older relay sends no signature");
        assert!(!account_signed(None, &envelope, Some(&signature)), "no pinned key, nothing vouched");
        let relay = SigningKey::from_bytes(&[12u8; 32]);
        assert!(!account_signed(Some(&pinned), &envelope, Some(&sign(&relay, &envelope))), "another key");
        let mut swapped = envelope.clone();
        swapped.sealed = STANDARD.encode(b"relay-sealed-offer");
        assert!(!account_signed(Some(&pinned), &swapped, Some(&signature)), "a signature for another envelope");
    }
}
