//! Chromium password decryption via macOS Keychain.
//!
//! On macOS, Chromium-family browsers store an encryption password in the
//! system Keychain under a per-browser service name (e.g. "Chrome Safe Storage").
//! The password is fed through PBKDF2-HMAC-SHA1 (salt="saltysalt", 1003 iterations)
//! to derive a 16-byte AES-128 key. Login passwords in the `Login Data` SQLite
//! database are prefixed with `v10` followed by AES-128-CBC ciphertext (IV = 16
//! spaces, PKCS#7 padded).

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use hmac::Hmac;
use sha1::Sha1;
use zeroize::{Zeroize, Zeroizing};

// ---------- Error types ----------

/// Errors from Keychain access.
#[derive(Clone, Debug, thiserror::Error)]
pub enum KeychainError {
    #[error("keychain access denied")]
    AccessDenied,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("system: {0}")]
    System(String),
}

/// Errors from AES-128-CBC decryption.
#[derive(Clone, Debug, thiserror::Error)]
pub enum DecryptError {
    #[error("missing v10 prefix")]
    MissingPrefix,
    #[error("invalid ciphertext length")]
    InvalidLength,
    #[error("AES failed")]
    AesFailed,
    #[error("invalid UTF-8")]
    InvalidUtf8,
    #[error("invalid padding")]
    InvalidPadding,
    #[error("cookie host binding mismatch")]
    HostBindingMismatch,
}

// ---------- Keychain provider trait ----------

/// Abstraction over macOS Keychain access for testability.
pub trait KeychainProvider {
    /// Retrieves the raw password bytes for a given service/account pair.
    fn get_password(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Zeroizing<Vec<u8>>, KeychainError>;
}

/// Real macOS Keychain implementation using `security-framework`.
#[cfg(target_os = "macos")]
pub struct MacOsKeychain;

#[cfg(target_os = "macos")]
impl KeychainProvider for MacOsKeychain {
    fn get_password(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Zeroizing<Vec<u8>>, KeychainError> {
        use security_framework::passwords::get_generic_password;

        match get_generic_password(service, account) {
            Ok(pw) => Ok(Zeroizing::new(pw.to_vec())),
            Err(e) => {
                let code = e.code();
                if code == -25293 {
                    // errSecAuthFailed
                    Err(KeychainError::AccessDenied)
                } else if code == -25300 {
                    // errSecItemNotFound
                    Err(KeychainError::NotFound(format!(
                        "service={service}, account={account}"
                    )))
                } else {
                    Err(KeychainError::System(e.to_string()))
                }
            }
        }
    }
}

/// Mock Keychain for tests.
#[cfg(test)]
pub struct MockKeychain {
    pub password: Vec<u8>,
}

#[cfg(test)]
impl KeychainProvider for MockKeychain {
    fn get_password(
        &self,
        _service: &str,
        _account: &str,
    ) -> Result<Zeroizing<Vec<u8>>, KeychainError> {
        Ok(Zeroizing::new(self.password.clone()))
    }
}

// ---------- ChromiumKey ----------

/// AES-128 key derived from Keychain password. Zeroized on drop.
#[derive(Zeroize)]
#[zeroize(drop)]
pub struct ChromiumKey([u8; 16]);

impl ChromiumKey {
    /// Returns a reference to the raw key bytes.
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

// ---------- KDF ----------

const PBKDF2_SALT: &[u8] = b"saltysalt";
const PBKDF2_ITERATIONS: u32 = 1003;
const AES_KEY_LEN: usize = 16;

/// Derives the AES-128 key from a Keychain password using PBKDF2-HMAC-SHA1.
pub fn derive_aes_key(keychain_password: &[u8]) -> ChromiumKey {
    derive_aes_key_with_iterations(keychain_password, PBKDF2_ITERATIONS)
}

/// PBKDF2 rounds Chromium uses on Linux (`os_crypt_linux.cc`); macOS uses 1003.
pub const PBKDF2_ITERATIONS_LINUX: u32 = 1;

/// Derives the AES-128 key with an explicit round count (Linux Chromium uses one round).
pub fn derive_aes_key_with_iterations(password: &[u8], iterations: u32) -> ChromiumKey {
    let mut key = [0u8; AES_KEY_LEN];
    pbkdf2::pbkdf2::<Hmac<Sha1>>(password, PBKDF2_SALT, iterations, &mut key)
        .expect("HMAC can be initialized with any key length");
    ChromiumKey(key)
}

// ---------- AES-128-CBC decrypt ----------

const V10_PREFIX: &[u8; 3] = b"v10";
/// Linux: `v11` blobs use the keyring (Secret Service / KWallet) key, `v10` the fixed one.
const V11_PREFIX: &[u8; 3] = b"v11";
const AES_BLOCK_SIZE: usize = 16;
/// IV: 16 space characters (0x20).
const IV: [u8; 16] = [0x20; 16];

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

/// Decrypts a v10-prefixed Chromium password blob.
///
/// Input: `"v10"` (3 bytes) + AES-128-CBC ciphertext (PKCS#7 padded).
/// Returns the decrypted password as a UTF-8 string.
pub fn decrypt_v10(ciphertext: &[u8], key: &ChromiumKey) -> Result<String, DecryptError> {
    if ciphertext.len() < 3 {
        return Err(DecryptError::MissingPrefix);
    }

    if &ciphertext[..3] != V10_PREFIX && &ciphertext[..3] != V11_PREFIX {
        return Err(DecryptError::MissingPrefix);
    }

    let encrypted = &ciphertext[3..];
    if encrypted.is_empty() || !encrypted.len().is_multiple_of(AES_BLOCK_SIZE) {
        return Err(DecryptError::InvalidLength);
    }

    // Decrypt in-place on a copy.
    let mut buf = Zeroizing::new(encrypted.to_vec());
    let decrypted = Aes128CbcDec::new(key.0.as_ref().into(), &IV.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| DecryptError::InvalidPadding)?;

    String::from_utf8(decrypted.to_vec()).map_err(|_| DecryptError::InvalidUtf8)
}

/// Decrypts a v10-prefixed Chromium **cookie** blob.
///
/// Cookie database versions 24 and later prefix plaintext with SHA-256 of
/// the exact host_key. Verify that binding before stripping it. Earlier
/// database versions contain only the cookie value, regardless of its length.
pub fn decrypt_cookie_v10(
    ciphertext: &[u8],
    key: &ChromiumKey,
    database_version: i64,
    host: &str,
) -> Result<String, DecryptError> {
    if ciphertext.len() < 3 {
        return Err(DecryptError::MissingPrefix);
    }
    if &ciphertext[..3] != V10_PREFIX && &ciphertext[..3] != V11_PREFIX {
        return Err(DecryptError::MissingPrefix);
    }
    let encrypted = &ciphertext[3..];
    if encrypted.is_empty() || !encrypted.len().is_multiple_of(AES_BLOCK_SIZE) {
        return Err(DecryptError::InvalidLength);
    }
    let mut buf = Zeroizing::new(encrypted.to_vec());
    let decrypted = Aes128CbcDec::new(key.0.as_ref().into(), &IV.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| DecryptError::InvalidPadding)?;

    let value = if database_version >= 24 {
        use sha2::{Digest, Sha256};
        let expected = Sha256::digest(host.as_bytes());
        if decrypted.get(..32) != Some(expected.as_slice()) {
            return Err(DecryptError::HostBindingMismatch);
        }
        &decrypted[32..]
    } else {
        decrypted
    };
    String::from_utf8(value.to_vec()).map_err(|_| DecryptError::InvalidUtf8)
}

// ---------- Convenience: fetch + derive ----------

/// Fetches the Keychain password for a browser and derives the AES key.
pub fn fetch_and_derive_key(
    provider: &dyn KeychainProvider,
    service: &str,
    account: &str,
) -> Result<ChromiumKey, KeychainError> {
    let password = provider.get_password(service, account)?;
    Ok(derive_aes_key(&password))
}

// ---------- Test support ----------

/// Test-only helpers for constructing keys without Keychain access.
#[doc(hidden)]
pub mod tests_support {
    use super::ChromiumKey;

    /// Creates a `ChromiumKey` from raw bytes (test-only).
    pub fn make_key(bytes: [u8; 16]) -> ChromiumKey {
        ChromiumKey(bytes)
    }
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    /// Known test vector matching the C++ unit test:
    /// Key: 0x42 * 16 ("BBBBBBBBBBBBBBBB")
    /// IV: 0x20 * 16
    /// Plaintext: "hunter2"
    const TEST_KEY_BYTES: [u8; 16] = [0x42; 16];

    /// v10 + AES-128-CBC(key=0x42*16, iv=0x20*16, plaintext="hunter2" PKCS7)
    const TEST_CIPHERTEXT: &[u8] = &[
        b'v', b'1', b'0', 0x9c, 0x31, 0x37, 0xcc, 0x50, 0xf8, 0xd5, 0x93, 0x0e, 0xe3, 0x7a, 0x1e,
        0x63, 0x0c, 0xce, 0x75,
    ];

    fn test_key() -> ChromiumKey {
        ChromiumKey(TEST_KEY_BYTES)
    }

    #[test]
    fn review_legacy_cookie_preserves_long_values() {
        use aes::cipher::BlockEncryptMut;
        for len in [32, 40] {
            let plain = vec![b'A'; len];
            let mut buf = vec![0; len + 16];
            buf[..len].copy_from_slice(&plain);
            let encrypted = cbc::Encryptor::<aes::Aes128>::new(TEST_KEY_BYTES.as_ref().into(), &IV.into())
                .encrypt_padded_mut::<Pkcs7>(&mut buf, len).unwrap();
            let mut blob = b"v10".to_vec();
            blob.extend_from_slice(encrypted);
            assert_eq!(decrypt_cookie_v10(&blob, &test_key(), 23, ".example.com").unwrap(), "A".repeat(len));
        }
    }

    #[test]
    fn review_bound_cookie_verifies_exact_host() {
        use aes::cipher::BlockEncryptMut;
        use sha2::{Digest, Sha256};
        let mut plain = Sha256::digest(b".example.com").to_vec();
        plain.extend_from_slice(b"session-value");
        let mut buf = vec![0; plain.len() + 16];
        buf[..plain.len()].copy_from_slice(&plain);
        let encrypted = cbc::Encryptor::<aes::Aes128>::new(TEST_KEY_BYTES.as_ref().into(), &IV.into())
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plain.len()).unwrap();
        let mut blob = b"v10".to_vec();
        blob.extend_from_slice(encrypted);
        assert_eq!(decrypt_cookie_v10(&blob, &test_key(), 24, ".example.com").unwrap(), "session-value");
        assert!(matches!(decrypt_cookie_v10(&blob, &test_key(), 24, "example.com"), Err(DecryptError::HostBindingMismatch)));
        assert!(matches!(decrypt_cookie_v10(TEST_CIPHERTEXT, &test_key(), 24, ".example.com"), Err(DecryptError::HostBindingMismatch)));
    }

    #[test]
    fn decrypt_v10_known_vector() {
        let result = decrypt_v10(TEST_CIPHERTEXT, &test_key()).unwrap();
        assert_eq!(result, "hunter2");
    }

    #[test]
    fn decrypt_v10_roundtrip() {
        use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

        let plaintext = b"my$ecretP@ss!";
        let key = ChromiumKey([0xAA; 16]);

        // Encrypt: allocate buffer with space for padding.
        let mut buf = vec![0u8; plaintext.len() + 16];
        buf[..plaintext.len()].copy_from_slice(plaintext);
        let encrypted = Aes128CbcEnc::new(key.0.as_ref().into(), &IV.into())
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .unwrap();

        // Prepend v10 prefix
        let mut blob = Vec::with_capacity(3 + encrypted.len());
        blob.extend_from_slice(b"v10");
        blob.extend_from_slice(encrypted);

        // Decrypt
        let decrypted = decrypt_v10(&blob, &key).unwrap();
        assert_eq!(decrypted, "my$ecretP@ss!");
    }

    #[test]
    fn decrypt_v10_rejects_wrong_prefix() {
        let mut bad = TEST_CIPHERTEXT.to_vec();
        bad[1] = b'2'; // "v20"
        let err = decrypt_v10(&bad, &test_key()).unwrap_err();
        assert!(matches!(err, DecryptError::MissingPrefix));
    }

    #[test]
    fn decrypt_v10_rejects_no_prefix() {
        let bad = &[
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12,
        ];
        let err = decrypt_v10(bad, &test_key()).unwrap_err();
        assert!(matches!(err, DecryptError::MissingPrefix));
    }

    #[test]
    fn decrypt_v10_rejects_short_ciphertext() {
        let bad = &[b'v', b'1', b'0', 0x00, 0x01, 0x02, 0x03, 0x04];
        let err = decrypt_v10(bad, &test_key()).unwrap_err();
        assert!(matches!(err, DecryptError::InvalidLength));
    }

    #[test]
    fn decrypt_v10_rejects_empty_after_prefix() {
        let bad = b"v10";
        let err = decrypt_v10(bad, &test_key()).unwrap_err();
        // Only 3 bytes total — prefix check passes but then encrypted is empty
        assert!(matches!(err, DecryptError::InvalidLength));
    }

    #[test]
    fn derive_aes_key_deterministic() {
        let key1 = derive_aes_key(b"peanuts");
        let key2 = derive_aes_key(b"peanuts");
        assert_eq!(key1.0, key2.0);
    }

    #[test]
    fn linux_round_count_derives_a_different_key() {
        // python3 -c "import hashlib; print(hashlib.pbkdf2_hmac('sha1', b'peanuts', b'saltysalt', 1, 16).hex())"
        let key = derive_aes_key_with_iterations(b"peanuts", PBKDF2_ITERATIONS_LINUX);
        let expected: [u8; 16] = [
            0xfd, 0x62, 0x1f, 0xe5, 0xa2, 0xb4, 0x02, 0x53, 0x9d, 0xfa, 0x14, 0x7c, 0xa9, 0x27,
            0x27, 0x78,
        ];
        assert_eq!(key.0, expected);
    }

    #[test]
    fn derive_aes_key_known_vector() {
        // python3 -c "import hashlib; print(hashlib.pbkdf2_hmac('sha1', b'peanuts', b'saltysalt', 1003, 16).hex())"
        // Result: d9a09d499b4e1b7461f28e67972c6dbd
        let key = derive_aes_key(b"peanuts");
        let expected: [u8; 16] = [
            0xd9, 0xa0, 0x9d, 0x49, 0x9b, 0x4e, 0x1b, 0x74, 0x61, 0xf2, 0x8e, 0x67, 0x97, 0x2c,
            0x6d, 0xbd,
        ];
        assert_eq!(key.0, expected);
    }

    #[test]
    fn mock_keychain_works() {
        let mock = MockKeychain {
            password: b"testpassword".to_vec(),
        };
        let pw = mock.get_password("Test Service", "Test").unwrap();
        assert_eq!(&pw[..], b"testpassword");
    }
}
