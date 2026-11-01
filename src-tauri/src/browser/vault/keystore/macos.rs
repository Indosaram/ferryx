//! macOS: the vault key as a generic password in the user's login Keychain.

use super::{KeyStore, KeystoreError, KEY_SERVICE};

/// errSecItemNotFound.
const ITEM_NOT_FOUND: i32 = -25300;

pub(super) struct Keychain;

fn failed(error: security_framework::base::Error) -> KeystoreError {
    KeystoreError::Failed(format!("Keychain error {}: {error}", error.code()))
}

impl KeyStore for Keychain {
    fn backend(&self) -> &'static str {
        "keychain"
    }

    fn load(&self, account: &str) -> Result<Option<Vec<u8>>, KeystoreError> {
        match security_framework::passwords::get_generic_password(KEY_SERVICE, account) {
            Ok(secret) => Ok(Some(secret)),
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(None),
            Err(error) => Err(failed(error)),
        }
    }

    fn store(&self, account: &str, secret: &[u8]) -> Result<(), KeystoreError> {
        security_framework::passwords::set_generic_password(KEY_SERVICE, account, secret)
            .map_err(failed)
    }

    fn delete(&self, account: &str) -> Result<(), KeystoreError> {
        match security_framework::passwords::delete_generic_password(KEY_SERVICE, account) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(()),
            Err(error) => Err(failed(error)),
        }
    }
}
