//! Ported from Maho (Project-Maho/maho-browser@c2830251,
//! maho/crates/maho-core/src/vault_crypto/tests.rs): the record-envelope cases. The KDF and
//! key-wrapping cases are not ported because Ferryx keeps the key in the OS keystore.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::json;

use super::*;

const SENTINEL: &[u8] = b"S3NTINEL-ferryx-vault";
const RECORD_AAD: &[u8] = b"vault-item:018f47a6";

fn vault_key() -> VaultKey {
    VaultKey::generate(VaultKeyVersion::new(1)).unwrap()
}

fn encrypt(key: &VaultKey) -> VaultCiphertextEnvelope {
    encrypt_record(SENTINEL, key, VaultCiphertextPurpose::VaultItem, RECORD_AAD).unwrap()
}

#[test]
fn aes_gcm_record_round_trip_preserves_plaintext() {
    let key = vault_key();
    let encrypted = encrypt(&key);
    assert_ne!(encrypted.ciphertext.as_slice(), SENTINEL);
    assert_eq!(decrypt_record(&encrypted, &key, RECORD_AAD).unwrap().as_slice(), SENTINEL);
}

#[test]
fn aes_gcm_record_rejects_tampered_ciphertext_and_wrong_aad() {
    let key = vault_key();
    let mut tampered = encrypt(&key);
    tampered.ciphertext[0] ^= 1;
    assert_eq!(
        decrypt_record(&tampered, &key, RECORD_AAD),
        Err(VaultCryptoError::AuthenticationFailed)
    );
    let encrypted = encrypt(&key);
    assert_eq!(
        decrypt_record(&encrypted, &key, b"vault-item:other"),
        Err(VaultCryptoError::AuthenticationFailed)
    );
}

#[test]
fn record_rejects_wrong_key_nonce_tag_purpose_and_key_version() {
    let key = vault_key();
    let wrong_key = vault_key();
    let encrypted = encrypt(&key);

    assert_eq!(
        decrypt_record(&encrypted, &wrong_key, RECORD_AAD),
        Err(VaultCryptoError::AuthenticationFailed)
    );
    let mut wrong_nonce = encrypted.clone();
    wrong_nonce.nonce[0] ^= 1;
    assert!(decrypt_record(&wrong_nonce, &key, RECORD_AAD).is_err());
    let mut wrong_tag = encrypted.clone();
    wrong_tag.tag[0] ^= 1;
    assert!(decrypt_record(&wrong_tag, &key, RECORD_AAD).is_err());
    let mut wrong_purpose = encrypted.clone();
    wrong_purpose.purpose = VaultCiphertextPurpose::TotpSeed;
    assert!(decrypt_record(&wrong_purpose, &key, RECORD_AAD).is_err());
    let mut wrong_version = encrypted;
    wrong_version.key_version = VaultKeyVersion::new(2);
    assert!(matches!(
        decrypt_record(&wrong_version, &key, RECORD_AAD),
        Err(VaultCryptoError::KeyVersionMismatch { envelope: 2, key: 1 })
    ));
}

#[test]
fn record_rejects_malformed_lengths_and_unsupported_schema_version() {
    let key = vault_key();
    let encrypted = encrypt(&key);

    let mut short_nonce = encrypted.clone();
    short_nonce.nonce.pop();
    assert_eq!(decrypt_record(&short_nonce, &key, RECORD_AAD), Err(VaultCryptoError::InvalidEnvelope));
    let mut short_tag = encrypted.clone();
    short_tag.tag.pop();
    assert_eq!(decrypt_record(&short_tag, &key, RECORD_AAD), Err(VaultCryptoError::InvalidEnvelope));

    let mut in_process = encrypted.clone();
    in_process.schema_version = VaultSchemaVersion::Unsupported(99);
    assert_eq!(decrypt_record(&in_process, &key, RECORD_AAD), Err(VaultCryptoError::InvalidEnvelope));

    let mut serialized = serde_json::to_value(encrypted).unwrap();
    serialized["schemaVersion"] = json!(99);
    assert!(serde_json::from_value::<VaultCiphertextEnvelope>(serialized).is_err());
}

#[test]
fn record_crypto_rejects_oversized_inputs_before_processing() {
    let key = vault_key();
    let oversized_aad = vec![0; MAX_AAD_LEN + 1];
    let mut oversized_envelope = encrypt(&key);
    oversized_envelope.ciphertext = vec![0; MAX_PLAINTEXT_LEN + 1];

    assert_eq!(
        encrypt_record(&vec![0; MAX_PLAINTEXT_LEN + 1], &key, VaultCiphertextPurpose::VaultItem, RECORD_AAD),
        Err(VaultCryptoError::InputTooLarge)
    );
    assert_eq!(
        encrypt_record(SENTINEL, &key, VaultCiphertextPurpose::VaultItem, &oversized_aad),
        Err(VaultCryptoError::InputTooLarge)
    );
    assert_eq!(
        decrypt_record(&oversized_envelope, &key, RECORD_AAD),
        Err(VaultCryptoError::InputTooLarge)
    );
}

#[test]
fn secure_nonce_generation_is_unique_for_repeated_encryptions() {
    let key = vault_key();
    let first = encrypt(&key);
    let second = encrypt(&key);
    assert_ne!(first.nonce, vec![0; NONCE_LEN]);
    assert_ne!(first.nonce, second.nonce);
    assert_ne!(first.ciphertext, second.ciphertext);
}

#[test]
fn key_debug_and_envelope_debug_do_not_expose_secrets() {
    let key = vault_key();
    let encrypted = encrypt(&key);
    let debug = format!("{key:?} {encrypted:?}");
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains("S3NTINEL"));
    let raw = format!("{:?}", key.expose());
    assert!(!debug.contains(&raw));
}

#[test]
fn key_from_slice_requires_exactly_32_bytes() {
    let key = vault_key();
    let restored = VaultKey::from_slice(key.expose(), key.version()).unwrap();
    assert_eq!(decrypt_record(&encrypt(&key), &restored, RECORD_AAD).unwrap().as_slice(), SENTINEL);
    assert!(VaultKey::from_slice(&[0; 31], VaultKeyVersion::new(1)).is_none());
    assert!(VaultKey::from_slice(&[0; 33], VaultKeyVersion::new(1)).is_none());
}

#[test]
fn vault_key_drop_runs_zeroization_instrumentation() {
    let zeroized = Arc::new(AtomicBool::new(false));
    let key = VaultKey::generate_with_probe(VaultKeyVersion::new(1), Arc::clone(&zeroized)).unwrap();
    drop(key);
    assert!(zeroized.load(Ordering::SeqCst));
}
