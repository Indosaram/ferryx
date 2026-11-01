//! Versioned AES-256-GCM record envelopes and zeroizing key material for the password vault.
//!
//! Ported from Maho (Project-Maho/maho-browser@c2830251, maho/crates/maho-core/src/vault_crypto
//! and the envelope types of maho/crates/maho-types/src/vault.rs) under the owner's reuse grant.
//! Ferryx keeps the record envelope, its validation and the zeroizing key; the passphrase KDF
//! and key wrapping are replaced by the OS keystore (see `keystore`), which holds the key itself.

use std::fmt;

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce, Tag};
use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub const NONCE_LEN: usize = 12;
pub const TAG_LEN: usize = 16;
pub const KEY_LEN: usize = 32;
pub const MAX_AAD_LEN: usize = 4096;
pub const MAX_VAULT_ENVELOPE_NONCE_LEN: usize = 64;
pub const MAX_VAULT_ENVELOPE_CIPHERTEXT_LEN: usize = 16 * 1024 * 1024;
pub const MAX_VAULT_ENVELOPE_TAG_LEN: usize = 64;
pub const MAX_PLAINTEXT_LEN: usize = MAX_VAULT_ENVELOPE_CIPHERTEXT_LEN;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VaultCryptoError {
    #[error("secure random generation failed")]
    Randomness,
    #[error("Vault encryption failed")]
    Encryption,
    #[error("Vault authentication failed")]
    AuthenticationFailed,
    #[error("invalid Vault ciphertext envelope")]
    InvalidEnvelope,
    #[error("Vault cryptographic input exceeds its supported size")]
    InputTooLarge,
    #[error("ciphertext key version {envelope} does not match key version {key}")]
    KeyVersionMismatch { envelope: u32, key: u32 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VaultKeyVersion(u32);

impl VaultKeyVersion {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VaultSchemaVersion {
    V1,
    /// Only ever built in process (never deserialized); rejected by every crypto entry point.
    Unsupported(u16),
}

impl Serialize for VaultSchemaVersion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::V1 => 1_u16.serialize(serializer),
            Self::Unsupported(found) => Err(serde::ser::Error::custom(format!(
                "unsupported Vault schema version {found}"
            ))),
        }
    }
}

impl<'de> Deserialize<'de> for VaultSchemaVersion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u16::deserialize(deserializer)? {
            1 => Ok(Self::V1),
            found => Err(serde::de::Error::custom(format!(
                "unsupported Vault schema version {found}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum VaultCipherAlgorithm {
    #[serde(rename = "aes_256_gcm")]
    Aes256Gcm,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VaultCiphertextPurpose {
    VaultItem,
    TotpSeed,
    PasskeyPrivateKey,
}

struct BoundedBytesVisitor<const MAX: usize>;

impl<'de, const MAX: usize> Visitor<'de> for BoundedBytesVisitor<MAX> {
    type Value = Vec<u8>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "at most {MAX} bytes")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut bytes = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX));
        while let Some(byte) = sequence.next_element()? {
            if bytes.len() == MAX {
                return Err(serde::de::Error::invalid_length(MAX + 1, &self));
            }
            bytes.push(byte);
        }
        Ok(bytes)
    }
}

fn bounded<'de, D, const MAX: usize>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_seq(BoundedBytesVisitor::<MAX>)
}

fn deserialize_nonce<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    bounded::<D, MAX_VAULT_ENVELOPE_NONCE_LEN>(deserializer)
}

fn deserialize_ciphertext<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<u8>, D::Error> {
    bounded::<D, MAX_VAULT_ENVELOPE_CIPHERTEXT_LEN>(deserializer)
}

fn deserialize_tag<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    bounded::<D, MAX_VAULT_ENVELOPE_TAG_LEN>(deserializer)
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VaultCiphertextEnvelope {
    pub schema_version: VaultSchemaVersion,
    pub algorithm: VaultCipherAlgorithm,
    pub purpose: VaultCiphertextPurpose,
    pub key_version: VaultKeyVersion,
    #[serde(deserialize_with = "deserialize_nonce")]
    pub nonce: Vec<u8>,
    #[serde(deserialize_with = "deserialize_ciphertext")]
    pub ciphertext: Vec<u8>,
    #[serde(deserialize_with = "deserialize_tag")]
    pub tag: Vec<u8>,
}

impl fmt::Debug for VaultCiphertextEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VaultCiphertextEnvelope")
            .field("schema_version", &self.schema_version)
            .field("algorithm", &self.algorithm)
            .field("purpose", &self.purpose)
            .field("key_version", &self.key_version)
            .field("nonce", &"[REDACTED]")
            .field("ciphertext", &"[REDACTED]")
            .field("tag", &"[REDACTED]")
            .finish()
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct VaultKey {
    bytes: [u8; KEY_LEN],
    #[zeroize(skip)]
    version: VaultKeyVersion,
    #[cfg(test)]
    #[zeroize(skip)]
    probe: Option<ZeroizeProbe>,
}

impl fmt::Debug for VaultKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VaultKey")
            .field("version", &self.version)
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

impl VaultKey {
    pub fn generate(version: VaultKeyVersion) -> Result<Self, VaultCryptoError> {
        let mut bytes = Zeroizing::new([0; KEY_LEN]);
        rand::RngCore::try_fill_bytes(&mut rand::rngs::OsRng, bytes.as_mut())
            .map_err(|_| VaultCryptoError::Randomness)?;
        Ok(Self::from_bytes(*bytes, version))
    }

    /// Rebuilds a key the OS keystore returned; `None` when it is not exactly [`KEY_LEN`] bytes.
    pub fn from_slice(bytes: &[u8], version: VaultKeyVersion) -> Option<Self> {
        let array: [u8; KEY_LEN] = bytes.try_into().ok()?;
        let mut array = Zeroizing::new(array);
        Some(Self::from_bytes(*array.as_mut(), version))
    }

    pub const fn version(&self) -> VaultKeyVersion {
        self.version
    }

    /// The raw key, only for handing it to the OS keystore.
    pub fn expose(&self) -> &[u8; KEY_LEN] {
        &self.bytes
    }

    fn from_bytes(bytes: [u8; KEY_LEN], version: VaultKeyVersion) -> Self {
        Self {
            bytes,
            version,
            #[cfg(test)]
            probe: None,
        }
    }
}

#[cfg(test)]
pub(super) struct ZeroizeProbe(pub std::sync::Arc<std::sync::atomic::AtomicBool>);

#[cfg(test)]
impl Drop for ZeroizeProbe {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
impl VaultKey {
    pub(super) fn generate_with_probe(
        version: VaultKeyVersion,
        zeroized: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<Self, VaultCryptoError> {
        let mut key = Self::generate(version)?;
        key.probe = Some(ZeroizeProbe(zeroized));
        Ok(key)
    }
}

pub fn encrypt_record(
    plaintext: &[u8],
    key: &VaultKey,
    purpose: VaultCiphertextPurpose,
    aad: &[u8],
) -> Result<VaultCiphertextEnvelope, VaultCryptoError> {
    if plaintext.len() > MAX_PLAINTEXT_LEN {
        return Err(VaultCryptoError::InputTooLarge);
    }
    aad_len(aad)?;
    let mut buffer = Zeroizing::new(plaintext.to_vec());
    let nonce = random_nonce()?;
    let metadata_aad = record_aad(purpose, key.version, aad);
    let cipher = Aes256Gcm::new_from_slice(&key.bytes).map_err(|_| VaultCryptoError::Encryption)?;
    let tag = cipher
        .encrypt_in_place_detached(Nonce::from_slice(&nonce), &metadata_aad, buffer.as_mut_slice())
        .map_err(|_| VaultCryptoError::Encryption)?;
    Ok(VaultCiphertextEnvelope {
        schema_version: VaultSchemaVersion::V1,
        algorithm: VaultCipherAlgorithm::Aes256Gcm,
        purpose,
        key_version: key.version,
        nonce: nonce.to_vec(),
        ciphertext: buffer.to_vec(),
        tag: tag.to_vec(),
    })
}

pub fn decrypt_record(
    envelope: &VaultCiphertextEnvelope,
    key: &VaultKey,
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>, VaultCryptoError> {
    validate_record_envelope(envelope, aad)?;
    if envelope.key_version != key.version {
        return Err(VaultCryptoError::KeyVersionMismatch {
            envelope: envelope.key_version.value(),
            key: key.version.value(),
        });
    }
    let metadata_aad = record_aad(envelope.purpose, envelope.key_version, aad);
    let mut plaintext = Zeroizing::new(envelope.ciphertext.clone());
    let cipher =
        Aes256Gcm::new_from_slice(&key.bytes).map_err(|_| VaultCryptoError::InvalidEnvelope)?;
    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(&envelope.nonce),
            &metadata_aad,
            plaintext.as_mut_slice(),
            Tag::from_slice(&envelope.tag),
        )
        .map_err(|_| VaultCryptoError::AuthenticationFailed)?;
    Ok(plaintext)
}

fn validate_record_envelope(
    envelope: &VaultCiphertextEnvelope,
    aad: &[u8],
) -> Result<(), VaultCryptoError> {
    if let VaultSchemaVersion::Unsupported(_) = envelope.schema_version {
        return Err(VaultCryptoError::InvalidEnvelope);
    }
    aad_len(aad)?;
    if envelope.nonce.len() > MAX_VAULT_ENVELOPE_NONCE_LEN
        || envelope.ciphertext.len() > MAX_VAULT_ENVELOPE_CIPHERTEXT_LEN
        || envelope.tag.len() > MAX_VAULT_ENVELOPE_TAG_LEN
    {
        return Err(VaultCryptoError::InputTooLarge);
    }
    if envelope.nonce.len() != NONCE_LEN || envelope.tag.len() != TAG_LEN {
        return Err(VaultCryptoError::InvalidEnvelope);
    }
    Ok(())
}

fn aad_len(aad: &[u8]) -> Result<(), VaultCryptoError> {
    if aad.len() > MAX_AAD_LEN {
        return Err(VaultCryptoError::InputTooLarge);
    }
    Ok(())
}

fn random_nonce() -> Result<[u8; NONCE_LEN], VaultCryptoError> {
    let mut nonce = [0; NONCE_LEN];
    rand::RngCore::try_fill_bytes(&mut rand::rngs::OsRng, &mut nonce)
        .map_err(|_| VaultCryptoError::Randomness)?;
    Ok(nonce)
}

fn record_aad(purpose: VaultCiphertextPurpose, key_version: VaultKeyVersion, aad: &[u8]) -> Vec<u8> {
    let purpose_byte = match purpose {
        VaultCiphertextPurpose::VaultItem => 1,
        VaultCiphertextPurpose::TotpSeed => 2,
        VaultCiphertextPurpose::PasskeyPrivateKey => 3,
    };
    let prefix: &[u8] = b"maho-vault-record-v1";
    let mut contextual = Vec::with_capacity(prefix.len() + 1 + 4 + 8 + aad.len());
    contextual.extend_from_slice(prefix);
    contextual.push(purpose_byte);
    contextual.extend_from_slice(&key_version.value().to_be_bytes());
    contextual.extend_from_slice(&u64::try_from(aad.len()).unwrap_or(u64::MAX).to_be_bytes());
    contextual.extend_from_slice(aad);
    contextual
}

#[cfg(test)]
#[path = "crypto_tests.rs"]
mod tests;
