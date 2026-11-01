//! Where the vault key lives: the OS keystore, never next to the encrypted store.
//!
//! macOS: a generic password in the login Keychain. Windows: the key protected with DPAPI
//! (CryptProtectData, current user) in a file beside the store. Linux: an item in the Secret
//! Service default collection over D-Bus (GNOME Keyring, KWallet's provider). Other targets,
//! and a Linux session without a Secret Service, report `Unavailable` with the reason; the
//! vault then refuses to start instead of keeping the key in plain text.
//!
//! Every call blocks (Keychain, DPAPI, D-Bus): run it through `crate::ipc::run_blocking`.

use std::path::Path;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Keychain service / Secret Service attribute / DPAPI description of the vault key.
pub const KEY_SERVICE: &str = "com.ferryx.app.browser-vault";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeystoreError {
    /// No OS keystore here (no Secret Service on the session bus, unsupported target).
    #[error("no OS keystore is available: {0}")]
    Unavailable(String),
    /// The keystore exists but refused or failed the operation (denied, locked, I/O).
    #[error("the OS keystore failed: {0}")]
    Failed(String),
}

/// One vault key slot in an OS keystore, named by `account` (the store's key id).
pub trait KeyStore: Send + Sync {
    /// `keychain`, `dpapi`, `secretService`, or `memory` in tests.
    fn backend(&self) -> &'static str;
    fn load(&self, account: &str) -> Result<Option<Vec<u8>>, KeystoreError>;
    fn store(&self, account: &str, secret: &[u8]) -> Result<(), KeystoreError>;
    fn delete(&self, account: &str) -> Result<(), KeystoreError>;
}

/// The platform keystore. `vault_dir` holds the DPAPI-protected key file on Windows.
pub fn platform(vault_dir: &Path) -> Box<dyn KeyStore> {
    #[cfg(target_os = "macos")]
    {
        let _ = vault_dir;
        Box::new(macos::Keychain)
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::Dpapi::new(vault_dir))
    }
    #[cfg(target_os = "linux")]
    {
        let _ = vault_dir;
        Box::new(linux::SecretService)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = vault_dir;
        Box::new(Unsupported)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
struct Unsupported;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
impl KeyStore for Unsupported {
    fn backend(&self) -> &'static str {
        "unsupported"
    }
    fn load(&self, _account: &str) -> Result<Option<Vec<u8>>, KeystoreError> {
        Err(KeystoreError::Unavailable("this platform has no supported OS keystore".into()))
    }
    fn store(&self, _account: &str, _secret: &[u8]) -> Result<(), KeystoreError> {
        Err(KeystoreError::Unavailable("this platform has no supported OS keystore".into()))
    }
    fn delete(&self, _account: &str) -> Result<(), KeystoreError> {
        Err(KeystoreError::Unavailable("this platform has no supported OS keystore".into()))
    }
}

/// An in-process keystore for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryKeyStore {
    pub slots: parking_lot::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    pub unavailable: Option<String>,
}

#[cfg(test)]
impl KeyStore for MemoryKeyStore {
    fn backend(&self) -> &'static str {
        "memory"
    }
    fn load(&self, account: &str) -> Result<Option<Vec<u8>>, KeystoreError> {
        if let Some(reason) = &self.unavailable {
            return Err(KeystoreError::Unavailable(reason.clone()));
        }
        Ok(self.slots.lock().get(account).cloned())
    }
    fn store(&self, account: &str, secret: &[u8]) -> Result<(), KeystoreError> {
        if let Some(reason) = &self.unavailable {
            return Err(KeystoreError::Unavailable(reason.clone()));
        }
        self.slots.lock().insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), KeystoreError> {
        self.slots.lock().remove(account);
        Ok(())
    }
}
