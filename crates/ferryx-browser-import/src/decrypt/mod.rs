//! Password decryption for imported browser data.
//!
//! Phase 3a: Chromium-family macOS Keychain path (AES-128-CBC via PBKDF2).
//! Phase 3b: Firefox NSS pure-Rust decryption (PBKDF2 + AES-256-CBC / 3DES-CBC).

pub mod asn1;
pub mod chromium_keychain;
pub mod firefox_nss;

pub use chromium_keychain::{
    decrypt_v10, derive_aes_key, ChromiumKey, DecryptError, KeychainError, KeychainProvider,
};

pub use firefox_nss::{decrypt_nss_value, unlock_nss_master_key, NssError, NssMasterKey};

#[cfg(target_os = "macos")]
pub use chromium_keychain::MacOsKeychain;

pub mod linux_keyring;
#[cfg(target_os = "linux")]
pub use linux_keyring::fetch_linux_keys;

#[cfg(any(target_os = "windows", test))]
pub mod win_app_bound;
#[cfg(target_os = "windows")]
pub use win_app_bound::WindowsAppBoundDecryptor;
