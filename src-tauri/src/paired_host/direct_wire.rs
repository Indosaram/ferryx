//! Wire contract for `POST /api/v1/direct/offer` (machine bearer auth required).
//!
//! The relay is untrusted, so identity binding lives inside the signed session id:
//! - offer   `session_id = "fxd1|<source machine>|<target machine>|<uuid v4>"`,
//!   signed by the source machine's `identity.json` key (role Initiator);
//! - answer  `session_id = "<offer session_id>|<base64url sha256(offer transcript)>"`,
//!   signed by the target machine's key, so it binds the initiator cert + nonce.
//!
//! Machine ids never contain `|` (see `direct_trust::validate_machine_id`).
use super::direct::{
    compute_offer_transcript, DirectConnectionAnswer, DirectConnectionOffer, DirectRole,
};
use super::direct_trust::validate_machine_id;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DIRECT_OFFER_PATH: [&str; 2] = ["direct", "offer"];
pub const MAX_DIRECT_BODY_BYTES: usize = 16 * 1024;
pub const MAX_DIRECT_CERT_BYTES: usize = 4 * 1024;
/// Signed `issued_at_ms` must be within this window of the verifier's clock.
pub const MAX_OFFER_SKEW_MS: u64 = 60_000;
const PREFIX: &str = "fxd1";

pub fn unix_now_ms() -> u64 {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    d.as_secs()
        .saturating_mul(1000)
        .saturating_add(u64::from(d.subsec_millis()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectOfferEnvelope {
    pub version: u32,
    pub source_machine_id: String,
    pub target_machine_id: String,
    pub offer: DirectConnectionOffer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectAnswerEnvelope {
    pub version: u32,
    pub answer: DirectConnectionAnswer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("malformed direct envelope")]
    Malformed,
    #[error("direct envelope is addressed to another machine")]
    WrongTarget,
    #[error("direct offer timestamp is outside the accepted window")]
    Expired,
    #[error("direct peer signature is not from the trusted machine key")]
    BadSignature,
}

pub fn offer_session_id(source: &str, target: &str) -> String {
    format!("{PREFIX}|{source}|{target}|{}", uuid::Uuid::new_v4())
}

pub fn offer_digest(offer: &DirectConnectionOffer) -> String {
    let transcript = compute_offer_transcript(
        &offer.session_id,
        offer.public_endpoint,
        &offer.cert_der,
        &offer.punch_nonce,
        offer.role,
        offer.issued_at_ms,
    );
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        Sha256::digest(transcript),
    )
}

pub fn endpoint_is_routable(endpoint: std::net::SocketAddrV4) -> bool {
    let ip = endpoint.ip();
    endpoint.port() != 0 && !ip.is_unspecified() && !ip.is_multicast() && !ip.is_broadcast()
}

pub fn answer_session_id(offer: &DirectConnectionOffer) -> String {
    format!("{}|{}", offer.session_id, offer_digest(offer))
}

/// Structural checks the host runs BEFORE trust lookup or any UDP work.
/// Returns the source machine id whose trusted key must verify the offer.
pub fn check_offer(envelope: &DirectOfferEnvelope, own_machine_id: &str) -> Result<String, WireError> {
    check_offer_at(envelope, own_machine_id, unix_now_ms())
}

pub fn check_offer_at(
    envelope: &DirectOfferEnvelope,
    own_machine_id: &str,
    now_ms: u64,
) -> Result<String, WireError> {
    let offer = &envelope.offer;
    if offer.issued_at_ms.abs_diff(now_ms) > MAX_OFFER_SKEW_MS {
        return Err(WireError::Expired);
    }
    if envelope.version != 1
        || offer.role != DirectRole::Initiator
        || offer.cert_der.is_empty()
        || offer.cert_der.len() > MAX_DIRECT_CERT_BYTES
        || !endpoint_is_routable(offer.public_endpoint)
        || validate_machine_id(&envelope.source_machine_id).is_err()
    {
        return Err(WireError::Malformed);
    }
    let parts: Vec<&str> = offer.session_id.split('|').collect();
    let [PREFIX, source, target, nonce] = parts.as_slice() else {
        return Err(WireError::Malformed);
    };
    if *source != envelope.source_machine_id
        || uuid::Uuid::parse_str(nonce).map_or(true, |u| u.get_version_num() != 4)
    {
        return Err(WireError::Malformed);
    }
    if *target != envelope.target_machine_id || *target != own_machine_id {
        return Err(WireError::WrongTarget);
    }
    Ok(envelope.source_machine_id.clone())
}

/// Client-side verification of the host answer against the pending offer.
pub fn check_answer(
    pending: &DirectConnectionOffer,
    envelope: &DirectAnswerEnvelope,
    trusted_host_key: &ed25519_dalek::VerifyingKey,
) -> Result<(), WireError> {
    let answer = &envelope.answer;
    if envelope.version != 1
        || answer.cert_der.is_empty()
        || answer.cert_der.len() > MAX_DIRECT_CERT_BYTES
        || !endpoint_is_routable(answer.public_endpoint)
        || answer.session_id != answer_session_id(pending)
    {
        return Err(WireError::Malformed);
    }
    answer
        .verify(trusted_host_key)
        .map_err(|_| WireError::BadSignature)
}

#[cfg(test)]
#[path = "direct_wire_tests.rs"]
mod tests;
