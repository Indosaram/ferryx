use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

use crate::remote::account_protocol::{
    AccountGrantOffer, AccountGrantOfferEnvelope, AccountGrantScope,
};
use crate::remote::attach_identity::AttachIdentity;
use crate::remote::auth::{AuthManager, DeviceAccessScope, DevicePermission};
use crate::remote::sealed_offer::{open_offer, seal_offer};

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

pub fn apply_grant_offer(
    auth: &AuthManager,
    attach: &AttachIdentity,
    machine_id: &str,
    enrollment_epoch: &str,
    envelope: &AccountGrantOfferEnvelope,
    now: u64,
) -> Result<AccountGrantOfferAck, OfferError> {
    let offer = open_grant_offer(attach, envelope, machine_id, enrollment_epoch, now)?;
    let (permission, scope) = scope_pairing(offer.grant_scope);
    auth.register_scoped_pairing_capability_with_attach(
        &offer.pairing_token,
        permission,
        scope,
        Some(offer.device_attach_public_key.clone()),
    )
    .map_err(|_| OfferError::GrantRefused)?;
    Ok(AccountGrantOfferAck {
        grant_id: offer.grant_id,
        status: "ready".to_string(),
    })
}

/// Applies an envelope that arrived for this machine, taking the machine id and epoch
/// from the enrollment record written at enroll time rather than from the wire.
pub fn apply_envelope_for_this_machine(
    auth: &AuthManager,
    envelope: &AccountGrantOfferEnvelope,
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
    apply_grant_offer(
        auth,
        &attach,
        &identity.machine_id,
        &record.enrollment_epoch,
        envelope,
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
            apply_grant_offer(&manager, &attach, "machine-1", "1", &other, now).expect_err("machine"),
            OfferError::WrongMachine
        );

        let stale = envelope(&attach, "machine-1", "1", &"3c".repeat(32), AccountGrantScope::Machine, now + 600);
        assert_eq!(
            apply_grant_offer(&manager, &attach, "machine-1", "9", &stale, now).expect_err("epoch"),
            OfferError::WrongEpoch
        );

        let expired = envelope(&attach, "machine-1", "1", &"4d".repeat(32), AccountGrantScope::Machine, now - 1);
        assert_eq!(
            apply_grant_offer(&manager, &attach, "machine-1", "1", &expired, now).expect_err("expired"),
            OfferError::Expired
        );

        let mut tampered = envelope(&attach, "machine-1", "1", &"5e".repeat(32), AccountGrantScope::Machine, now + 600);
        let mut bytes = STANDARD.decode(&tampered.sealed).expect("decode");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        tampered.sealed = STANDARD.encode(bytes);
        assert_eq!(
            apply_grant_offer(&manager, &attach, "machine-1", "1", &tampered, now).expect_err("tampered"),
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
            relay_origin: "https://relay.checka.cc".into(),
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
        let ack = apply_envelope_for_this_machine(&manager, &envelope, now).expect("apply");
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
}
