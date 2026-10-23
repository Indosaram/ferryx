//! Account-attested automatic direct trust (both sides).
//!
//! On a direct-trust miss, a machine enrolled with an account asks that account service for the
//! peer's signed record (`GET /api/account/v1/machines/{id}/peer-record`, authenticated by this
//! machine's enrolled identity key), verifies it against the operator-pinned account public key
//! (`FERRYX_RELAY_ACCOUNT_PUBLIC_KEY`), and pins the attested key with
//! [`DirectTrustStore::provision_from_account`]. Everything is best-effort and bounded: any
//! failure leaves no pin and the pair stays on the relay. `FERRYX_DIRECT_AUTO_TRUST=0` disables it.
use std::time::Duration;

use ed25519_dalek::VerifyingKey;

use super::direct_trust::{parse_verifying_key, DirectTrustStore, TrustError};
use crate::account::peer_record::{
    peer_record_auth_nonce, verify_peer_record, PEER_AUTH_MACHINE_HEADER, PEER_AUTH_SIG_HEADER,
    PEER_AUTH_TS_HEADER,
};
use crate::remote::auth::{sign_challenge, MachineIdentity};

/// Whole budget for one fetch + verify + pin.
pub const AUTO_TRUST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RECORD_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AutoTrustError {
    #[error("account request failed: {0}")]
    Transport(String),
    #[error("account refused the peer record with status {0}")]
    Status(u16),
    #[error("peer record failed verification")]
    Invalid,
    #[error("peer record owner does not match this machine's owner")]
    OwnerMismatch,
    #[error("peer record could not be pinned: {0}")]
    Pin(TrustError),
    #[error("account attestation timed out")]
    Timeout,
}

/// What a machine needs to ask its account service about a peer.
pub struct Attestor<'a> {
    pub http: &'a reqwest::Client,
    pub account_origin: &'a str,
    pub own: &'a MachineIdentity,
    pub account_public_key: &'a str,
}

/// Fetches `target`'s signed peer record, signing the request as `own`.
pub async fn fetch_peer_record(
    attestor: &Attestor<'_>,
    target: &str,
    now_secs: u64,
) -> Result<String, AutoTrustError> {
    let origin = attestor.account_origin.trim_end_matches('/');
    let mut url = reqwest::Url::parse(origin).map_err(|e| AutoTrustError::Transport(e.to_string()))?;
    url.path_segments_mut()
        .map_err(|_| AutoTrustError::Transport("account origin cannot be a base".into()))?
        .extend(["api", "account", "v1", "machines", target, "peer-record"]);
    let nonce = peer_record_auth_nonce(&attestor.own.machine_id, target);
    let sig = sign_challenge(attestor.own, &nonce, now_secs).map_err(AutoTrustError::Transport)?;
    let response = attestor
        .http
        .get(url)
        .header(PEER_AUTH_MACHINE_HEADER, &attestor.own.machine_id)
        .header(PEER_AUTH_TS_HEADER, now_secs.to_string())
        .header(PEER_AUTH_SIG_HEADER, sig)
        .send()
        .await
        .map_err(|e| AutoTrustError::Transport(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AutoTrustError::Status(response.status().as_u16()));
    }
    let bytes = response.bytes().await.map_err(|e| AutoTrustError::Transport(e.to_string()))?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(AutoTrustError::Invalid);
    }
    let body: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| AutoTrustError::Invalid)?;
    body.get("record")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or(AutoTrustError::Invalid)
}

/// Fetches, verifies and pins `target`'s account-attested key; returns the pinned key.
///
/// `required_owner`, when set, must equal the record's `ownerKey` (the host passes its own
/// lease owner so a record for another owner's machine is never pinned). Bounded by
/// [`AUTO_TRUST_TIMEOUT`].
pub async fn attest_and_pin(
    store: DirectTrustStore,
    attestor: &Attestor<'_>,
    target: &str,
    required_owner: Option<&str>,
    now_secs: u64,
) -> Result<VerifyingKey, AutoTrustError> {
    tokio::time::timeout(AUTO_TRUST_TIMEOUT, async {
        let record = fetch_peer_record(attestor, target, now_secs).await?;
        let claims = verify_peer_record(attestor.account_public_key, target, &record, now_secs)
            .ok_or(AutoTrustError::Invalid)?;
        if let Some(owner) = required_owner {
            if claims.owner_key != owner {
                return Err(AutoTrustError::OwnerMismatch);
            }
        }
        let key = parse_verifying_key(&claims.public_key).map_err(|_| AutoTrustError::Invalid)?;
        super::direct_trust::provision_from_account(
            store,
            target.to_owned(),
            claims.public_key,
            claims.enrollment_epoch,
        )
        .await
        .map_err(AutoTrustError::Pin)?;
        Ok(key)
    })
    .await
    .map_err(|_| AutoTrustError::Timeout)?
}

/// Shared HTTP client for account attestation requests (bounded per request).
pub fn account_http() -> reqwest::Client {
    static HTTP: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(AUTO_TRUST_TIMEOUT)
            .build()
            .unwrap_or_default()
    })
    .clone()
}
