use axum::{
    extract::State,
    http::StatusCode,
    routing::post,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::remote::account_protocol::AccountGrantOfferEnvelope;

/// Separates this protocol from Ed25519 signatures on other payloads a relay might verify.
pub const GRANT_SUBMISSION_DOMAIN: &[u8] = b"ferryx relay grant submission v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantSubmission {
    pub machine_id: String,
    pub enrollment_epoch: String,
    pub envelope: AccountGrantOfferEnvelope,
    pub signature: String,
}

/// The exact bytes the account service signs and the relay verifies. Domain separation keeps a
/// signature from being replayed into another protocol.
pub fn submission_signing_input(submission: &GrantSubmission) -> Vec<u8> {
    let mut input = GRANT_SUBMISSION_DOMAIN.to_vec();
    input.extend_from_slice(submission.machine_id.as_bytes());
    input.extend_from_slice(b"\0");
    input.extend_from_slice(submission.enrollment_epoch.as_bytes());
    input.extend_from_slice(b"\0");
    input.extend_from_slice(submission.envelope.sealed.as_bytes());
    input
}

/// The account service's Ed25519 public key, pinned by the relay operator. Without it the relay
/// refuses every submission: an unpinned relay is not an account gateway.
pub fn verify_grant_signature(
    account_public_key: &Option<String>,
    submission: &GrantSubmission,
) -> Result<(), StatusCode> {
    let Some(pinned) = account_public_key else {
        return Err(StatusCode::FORBIDDEN);
    };
    let pinned = STANDARD
        .decode(pinned)
        .ok()
        .and_then(|bytes| {
            let key: [u8; 32] = bytes.as_slice().try_into().ok()?;
            VerifyingKey::from_bytes(&key).ok()
        });
    let Some(pinned) = pinned else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let Ok(signature) = STANDARD.decode(&submission.signature) else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let Ok(signature) = Signature::from_slice(&signature) else {
        return Err(StatusCode::BAD_REQUEST);
    };
    pinned
        .verify(&submission_signing_input(submission), &signature)
        .map_err(|_| StatusCode::FORBIDDEN)
}

#[derive(Clone)]
pub struct GrantGate {
    pub account_public_key: Option<String>,
    pub delivery: crate::remote::relay_server::RelayState,
}

impl GrantGate {
    pub async fn accept(
        &self,
        submission: GrantSubmission,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        verify_grant_signature(&self.account_public_key, &submission)?;
        let delivered = self
            .delivery
            .deliver_grant_offer(&submission.machine_id, submission.envelope)
            .await?;
        Ok(Json(serde_json::json!({
            "accepted": true,
            "delivered": delivered,
        })))
    }
}

pub fn grant_gate_router(gate: GrantGate) -> Router {
    Router::new()
        .route(
            "/api/v1/attach/grant",
            post(accept_grant),
        )
        .with_state(gate)
}

async fn accept_grant(
    State(gate): State<GrantGate>,
    Json(submission): Json<GrantSubmission>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    gate.accept(submission).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use ed25519_dalek::{Signer, SigningKey};

    fn submission() -> GrantSubmission {
        GrantSubmission {
            machine_id: "grant-machine".into(),
            enrollment_epoch: "4".into(),
            envelope: crate::remote::account_protocol::AccountGrantOfferEnvelope {
                machine_id: "grant-machine".into(),
                enrollment_epoch: "4".into(),
                sealed: STANDARD.encode(b"sealed-bytes"),
            },
            signature: String::new(),
        }
    }

    #[test]
    fn a_pinned_account_key_is_the_only_way_in() {
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let mut submission = submission();
        submission.signature = STANDARD
            .encode(signing.sign(&submission_signing_input(&submission)).to_bytes());

        let pinned = Some(STANDARD.encode(signing.verifying_key().as_bytes()));
        assert_eq!(
            verify_grant_signature(&pinned, &submission),
            Ok(()),
            "the pinned account key verifies its own signature"
        );
        assert_eq!(
            verify_grant_signature(&None, &submission),
            Err(StatusCode::FORBIDDEN),
            "an unpinned relay refuses every submission"
        );
    }

    #[test]
    fn a_tampered_or_foreign_submission_is_refused() {
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let mut base = submission();
        base.signature = STANDARD
            .encode(signing.sign(&submission_signing_input(&base)).to_bytes());
        let pinned = Some(STANDARD.encode(signing.verifying_key().as_bytes()));

        let mut renamed = base.clone();
        renamed.machine_id = "other-machine".into();
        assert_eq!(
            verify_grant_signature(&pinned, &renamed),
            Err(StatusCode::FORBIDDEN)
        );

        let mut swapped = submission();
        swapped.signature = base.signature.clone();
        swapped.envelope.sealed = STANDARD.encode(b"swapped-sealed-bytes");
        assert_eq!(
            verify_grant_signature(&pinned, &swapped),
            Err(StatusCode::FORBIDDEN)
        );

        let other = SigningKey::from_bytes(&[9u8; 32]);
        let mut foreign = submission();
        foreign.signature = STANDARD
            .encode(other.sign(&submission_signing_input(&foreign)).to_bytes());
        assert_eq!(
            verify_grant_signature(&pinned, &foreign),
            Err(StatusCode::FORBIDDEN)
        );
    }
}
