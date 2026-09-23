use base64::{engine::general_purpose::STANDARD, Engine as _};
use blake2::{Blake2b512, Digest};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use rand::RngCore;
use x25519_dalek::{PublicKey, StaticSecret};

pub const SEALED_OFFER_PREFIX: &str = "ferryx-offer-v1";
pub const SEALED_OFFER_HEADER: usize = 32 + 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealError {
    BadKeyEncoding,
    ShortSealed,
    DecryptFailed,
}

impl std::fmt::Display for SealError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadKeyEncoding => write!(formatter, "SEALED_OFFER_KEY_INVALID"),
            Self::ShortSealed => write!(formatter, "SEALED_OFFER_TRUNCATED"),
            Self::DecryptFailed => write!(formatter, "SEALED_OFFER_DECRYPT_FAILED"),
        }
    }
}

fn associated_data(machine_id: &str, enrollment_epoch: &str) -> String {
    format!("{SEALED_OFFER_PREFIX}:{machine_id}:{enrollment_epoch}")
}

fn derive_key(shared: &[u8], ephemeral_public: &[u8], recipient_public: &[u8]) -> [u8; 32] {
    let mut hasher = Blake2b512::new();
    hasher.update(shared);
    hasher.update(ephemeral_public);
    hasher.update(recipient_public);
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest[..32]);
    key
}

fn decode_key(value: &str) -> Result<[u8; 32], SealError> {
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| SealError::BadKeyEncoding)?;
    bytes.try_into().map_err(|_| SealError::BadKeyEncoding)
}

/// Seals `plaintext` so only the holder of `recipient_attach_public_key` can open it.
/// The relay carrying the result sees an ephemeral public key, a nonce, and ciphertext.
pub fn seal_offer(
    recipient_attach_public_key: &str,
    machine_id: &str,
    enrollment_epoch: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>, SealError> {
    let recipient = PublicKey::from(decode_key(recipient_attach_public_key)?);
    let ephemeral = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let ephemeral_public = PublicKey::from(&ephemeral);
    let shared = ephemeral.diffie_hellman(&recipient);
    let key = derive_key(
        shared.as_bytes(),
        ephemeral_public.as_bytes(),
        recipient.as_bytes(),
    );

    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let cipher = ChaCha20Poly1305::new_from_slice(&key).map_err(|_| SealError::BadKeyEncoding)?;
    let aad = associated_data(machine_id, enrollment_epoch);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| SealError::DecryptFailed)?;

    let mut sealed = Vec::with_capacity(SEALED_OFFER_HEADER + ciphertext.len());
    sealed.extend_from_slice(ephemeral_public.as_bytes());
    sealed.extend_from_slice(&nonce_bytes);
    sealed.extend_from_slice(&ciphertext);
    Ok(sealed)
}

pub fn open_offer(
    recipient_attach_identity: &super::attach_identity::AttachIdentity,
    machine_id: &str,
    enrollment_epoch: &str,
    sealed: &[u8],
) -> Result<Vec<u8>, SealError> {
    if sealed.len() <= SEALED_OFFER_HEADER {
        return Err(SealError::ShortSealed);
    }
    let recipient_private = decode_key(&recipient_attach_identity.private_key)?;
    let recipient_public = decode_key(&recipient_attach_identity.public_key)?;
    let ephemeral_public: [u8; 32] = sealed[..32]
        .try_into()
        .map_err(|_| SealError::ShortSealed)?;
    let nonce_bytes = &sealed[32..SEALED_OFFER_HEADER];
    let ciphertext = &sealed[SEALED_OFFER_HEADER..];

    let secret = StaticSecret::from(recipient_private);
    let shared = secret.diffie_hellman(&PublicKey::from(ephemeral_public));
    let key = derive_key(&shared.to_bytes(), &ephemeral_public, &recipient_public);
    let cipher = ChaCha20Poly1305::new_from_slice(&key).map_err(|_| SealError::BadKeyEncoding)?;
    let aad = associated_data(machine_id, enrollment_epoch);
    cipher
        .decrypt(
            Nonce::from_slice(nonce_bytes),
            Payload {
                msg: ciphertext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| SealError::DecryptFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::attach_identity::AttachIdentity;

    fn attach_identity() -> AttachIdentity {
        let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        AttachIdentity {
            public_key: STANDARD.encode(PublicKey::from(&secret).as_bytes()),
            private_key: STANDARD.encode(secret.to_bytes()),
        }
    }

    const PAIRING_TOKEN: &str = "7160b3a6f454dc9b89f5d00a906d8b3c457383046aff54332f6d7c466e8eb30e";

    #[test]
    fn sealed_offer_round_trips_and_hides_the_token() {
        let daemon = attach_identity();
        let sealed = seal_offer(&daemon.public_key, "machine-1", "3", PAIRING_TOKEN.as_bytes())
            .expect("seal");
        let opened = open_offer(&daemon, "machine-1", "3", &sealed).expect("open");
        assert_eq!(opened, PAIRING_TOKEN.as_bytes());
        assert!(
            !String::from_utf8_lossy(&sealed).contains(PAIRING_TOKEN),
            "the relay payload must not carry the token in the clear"
        );
    }

    #[test]
    fn sealed_offer_rejects_a_different_recipient() {
        let daemon = attach_identity();
        let other = attach_identity();
        let sealed = seal_offer(&daemon.public_key, "machine-1", "3", b"payload").expect("seal");
        assert_eq!(
            open_offer(&other, "machine-1", "3", &sealed).expect_err("wrong key"),
            SealError::DecryptFailed
        );
    }

    #[test]
    fn sealed_offer_rejects_tampering_and_context_changes() {
        let daemon = attach_identity();
        let sealed = seal_offer(&daemon.public_key, "machine-1", "3", b"payload").expect("seal");

        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        assert_eq!(
            open_offer(&daemon, "machine-1", "3", &tampered).expect_err("tampered"),
            SealError::DecryptFailed
        );

        assert_eq!(
            open_offer(&daemon, "machine-2", "3", &sealed).expect_err("other machine"),
            SealError::DecryptFailed
        );
        assert_eq!(
            open_offer(&daemon, "machine-1", "4", &sealed).expect_err("other epoch"),
            SealError::DecryptFailed
        );
        assert_eq!(
            open_offer(&daemon, "machine-1", "3", &sealed[..SEALED_OFFER_HEADER]).expect_err("short"),
            SealError::ShortSealed
        );
    }
}
