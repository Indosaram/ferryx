use serde::{Deserialize, Serialize};

use super::machine_protocol::Platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountGrantScope {
    Mirror,
    Machine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountEnrollChallenge {
    pub nonce: String,
    pub timestamp: u64,
    pub audience: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountEnrollRequest {
    pub api_version: u32,
    pub enrollment_code: String,
    pub machine_id: String,
    pub display_name: String,
    pub public_key: String,
    pub attach_public_key: String,
    pub platform: Platform,
    pub app_version: String,
    pub nonce: String,
    pub timestamp: u64,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountEnrollResponse {
    pub account_id: String,
    pub machine_record_id: String,
    pub relay_origin: String,
    pub enrolled_at: u64,
    pub enrollment_epoch: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountMachineView {
    pub machine_record_id: String,
    pub machine_id: String,
    pub display_name: String,
    pub public_key: String,
    pub attach_public_key: String,
    pub relay_origin: String,
    pub platform: Platform,
    pub online: bool,
    pub enrollment_epoch: String,
    pub last_seen_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountGrantRequest {
    pub machine_record_id: String,
    pub enrollment_epoch: String,
    pub device_label: String,
    pub installation_id: String,
    pub grant_scope: AccountGrantScope,
    pub attach_public_key: String,
    /// Present only when the requester is itself an enrolled machine of this account (a
    /// standby pairing to the supervisor lease holder). A phone or the web app has no machine
    /// key and never sends it. The account service verifies it and vouches for the machine in
    /// the sealed offer ([`AccountGrantOffer::account_machine_id`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester_machine: Option<GrantRequesterProof>,
}

/// An enrolled machine's proof that it is the one asking for a grant: an Ed25519 signature by
/// its enrolled machine key over
/// `"{grant_requester_nonce(machine_id, machine_record_id, attach_public_key)}:{timestamp}"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantRequesterProof {
    pub machine_id: String,
    pub timestamp: u64,
    pub signature: String,
}

/// The nonce a requesting machine signs for a grant on `target_machine_record_id` for the device
/// key `attach_public_key`. The domain and both ids keep it from being replayed as a lease,
/// peer-record or other grant signature; the attach key binds it to the requester's own device,
/// so a captured proof cannot vouch for a device key the requester never saw.
pub fn grant_requester_nonce(
    requester_machine_id: &str,
    target_machine_record_id: &str,
    attach_public_key: &str,
) -> String {
    format!("ferryx-account-grant-requester-v2:{requester_machine_id}:{target_machine_record_id}:{attach_public_key}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountGrantOffer {
    pub grant_id: String,
    pub machine_id: String,
    pub enrollment_epoch: String,
    pub pairing_token: String,
    pub device_label: String,
    pub installation_id: String,
    pub grant_scope: AccountGrantScope,
    pub expires_at: u64,
    pub device_attach_public_key: String,
    /// The enrolled machine of this account the grant was issued to, set by the account
    /// service only after verifying [`AccountGrantRequest::requester_machine`]. A phone cannot
    /// add it. The seal alone does not authenticate the sender (anyone can seal to the target's
    /// attach key), so the target honours it only when the account's signature over the
    /// envelope, forwarded by the relay, verifies against the account key the machine pinned
    /// ([`crate::account::offer_sink::account_signed`]); otherwise the grant applies without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_machine_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountGrantOfferEnvelope {
    pub machine_id: String,
    pub enrollment_epoch: String,
    pub sealed: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountGrantResponse {
    pub grant_id: String,
    pub machine_id: String,
    pub relay_origin: String,
    pub pairing_token: String,
    pub machine_attach_public_key: String,
    pub grant_scope: AccountGrantScope,
    pub expires_at: u64,
    #[serde(default)]
    pub sealed_offer: Option<AccountGrantOfferEnvelope>,
}

#[cfg(test)]
mod tests {
    use super::super::auth::{
        enrollment_code_hash, sign_account_enrollment, sign_control_challenge,
        verify_account_enrollment, MachineIdentity,
    };
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn identity() -> MachineIdentity {
        let key = SigningKey::generate(&mut OsRng);
        MachineIdentity {
            machine_id: "machine-1".into(),
            display_name: "box".into(),
            public_key: STANDARD.encode(key.verifying_key().as_bytes()),
            private_key: STANDARD.encode(key.to_bytes()),
        }
    }

    #[test]
    fn account_enroll_signature_verifies() {
        let id = identity();
        let code = "enroll-code";
        let hash = enrollment_code_hash(code);
        let signature = sign_account_enrollment(&id, "https://account.example", &hash, "n", 10)
            .expect("sign");
        assert!(verify_account_enrollment(
            &id.public_key,
            &id.machine_id,
            "https://account.example",
            &hash,
            "n",
            10,
            &signature,
        ));
        let control = sign_control_challenge(&id, "https://account.example", "n", 10)
            .expect("control");
        assert!(
            !verify_account_enrollment(
                &id.public_key,
                &id.machine_id,
                "https://account.example",
                &hash,
                "n",
                10,
                &control,
            ),
            "a ferryx-control-v1 signature must not verify as enrollment"
        );
        let raw = sign_account_enrollment(&id, "https://account.example", code, "n", 10)
            .expect("raw");
        assert!(!verify_account_enrollment(
            &id.public_key,
            &id.machine_id,
            "https://account.example",
            &hash,
            "n",
            10,
            &raw,
        ));
    }
}
