//! Windows: the vault key protected with DPAPI for the current user (CryptProtectData), kept in
//! `<vault dir>/key-<account>.dpapi`. Only this Windows account on this machine can unprotect it.

use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use zeroize::Zeroizing;

use super::{KeyStore, KeystoreError};

/// Optional DPAPI entropy: ties the blob to this use.
const ENTROPY: &[u8] = b"ferryx.browser.vault.v1";

pub(super) struct Dpapi {
    dir: PathBuf,
}

impl Dpapi {
    pub(super) fn new(dir: &Path) -> Self {
        Self { dir: dir.to_path_buf() }
    }

    fn path(&self, account: &str) -> PathBuf {
        self.dir.join(format!("key-{account}.dpapi"))
    }
}

fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr().cast_mut() }
}

/// Copies and frees a blob DPAPI allocated.
fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if out.pbData.is_null() {
        return Vec::new();
    }
    // SAFETY: DPAPI returned `cbData` bytes at `pbData`, allocated with LocalAlloc.
    let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    // SAFETY: the buffer came from DPAPI and is freed exactly once.
    unsafe { LocalFree(out.pbData.cast()) };
    bytes
}

fn protect(secret: &[u8]) -> Result<Vec<u8>, KeystoreError> {
    let input = blob(secret);
    let entropy = blob(ENTROPY);
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: every pointer is valid for the call; DPAPI only reads the inputs.
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            &entropy,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    };
    if ok == 0 {
        return Err(KeystoreError::Failed(format!(
            "CryptProtectData failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(take(out))
}

fn unprotect(protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, KeystoreError> {
    let input = blob(protected);
    let entropy = blob(ENTROPY);
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: every pointer is valid for the call; DPAPI only reads the inputs.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            &entropy,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    };
    if ok == 0 {
        return Err(KeystoreError::Failed(format!(
            "CryptUnprotectData failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(Zeroizing::new(take(out)))
}

impl KeyStore for Dpapi {
    fn backend(&self) -> &'static str {
        "dpapi"
    }

    fn load(&self, account: &str) -> Result<Option<Vec<u8>>, KeystoreError> {
        match std::fs::read(self.path(account)) {
            Ok(protected) => Ok(Some(unprotect(&protected)?.to_vec())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(KeystoreError::Failed(error.to_string())),
        }
    }

    fn store(&self, account: &str, secret: &[u8]) -> Result<(), KeystoreError> {
        let protected = protect(secret)?;
        std::fs::create_dir_all(&self.dir).map_err(|error| KeystoreError::Failed(error.to_string()))?;
        let path = self.path(account);
        let staging = path.with_extension("dpapi.tmp");
        std::fs::write(&staging, protected)
            .and_then(|()| std::fs::rename(&staging, &path))
            .map_err(|error| KeystoreError::Failed(error.to_string()))
    }

    fn delete(&self, account: &str) -> Result<(), KeystoreError> {
        match std::fs::remove_file(self.path(account)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(KeystoreError::Failed(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpapi_round_trips_and_rejects_a_damaged_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = Dpapi::new(dir.path());
        assert_eq!(store.load("k1").unwrap(), None);
        store.store("k1", &[7; 32]).unwrap();
        assert_eq!(store.load("k1").unwrap(), Some(vec![7; 32]));
        let mut damaged = std::fs::read(store.path("k1")).unwrap();
        let last = damaged.len() - 1;
        damaged[last] ^= 0xff;
        std::fs::write(store.path("k1"), damaged).unwrap();
        assert!(matches!(store.load("k1"), Err(KeystoreError::Failed(_))));
        store.delete("k1").unwrap();
        assert_eq!(store.load("k1").unwrap(), None);
    }
}
