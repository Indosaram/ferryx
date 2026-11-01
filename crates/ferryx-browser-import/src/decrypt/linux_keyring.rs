// Copyright 2026 The Maho Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Linux Chromium cookie/password keys.
//!
//! Linux Chromium encrypts `v10` blobs with a fixed password (`peanuts`) and `v11` blobs with
//! a secret it keeps in the desktop keyring (Secret Service, or KWallet on KDE). Both keys are
//! PBKDF2-SHA1 with ONE round (`os_crypt_linux.cc`), unlike macOS's 1003.

use crate::decrypt::chromium_keychain::ChromiumKey;

/// The keys to try, in order: the keyring key (for `v11`) when the keyring has one, then the
/// fixed `v10` key. AES-CBC with a wrong key fails its padding or UTF-8 check, so trying both
/// per blob is safe.
pub struct LinuxChromiumKeys {
    pub keyring: Option<ChromiumKey>,
    pub fixed: ChromiumKey,
}

impl LinuxChromiumKeys {
    pub fn iter(&self) -> impl Iterator<Item = &ChromiumKey> {
        self.keyring.iter().chain(std::iter::once(&self.fixed))
    }
}

/// Keyring service/account names Chromium-family browsers store their secret under.
pub fn keyring_names(browser_name: &str) -> (&'static str, &'static str) {
    match browser_name {
        "Chrome" | "google-chrome" => ("Chrome Safe Storage", "Chrome"),
        "Brave" | "brave-browser" => ("Brave Safe Storage", "Brave"),
        "Edge" | "microsoft-edge" => ("Microsoft Edge Safe Storage", "Microsoft Edge"),
        "Vivaldi" | "vivaldi-stable" => ("Vivaldi Safe Storage", "Vivaldi"),
        "Opera" | "opera" => ("Opera Safe Storage", "Opera"),
        _ => ("Chromium Safe Storage", "Chromium"),
    }
}

/// The fixed `v10` key of Linux Chromium.
pub fn fixed_key() -> ChromiumKey {
    use crate::decrypt::chromium_keychain::{derive_aes_key_with_iterations, PBKDF2_ITERATIONS_LINUX};
    derive_aes_key_with_iterations(b"peanuts", PBKDF2_ITERATIONS_LINUX)
}

/// Reads the browser's keyring secret (blocking D-Bus / a `kwallet-query` subprocess).
#[cfg(target_os = "linux")]
pub fn fetch_linux_keys(browser_name: &str) -> LinuxChromiumKeys {
    use crate::decrypt::chromium_keychain::{derive_aes_key_with_iterations, PBKDF2_ITERATIONS_LINUX};
    let (service_name, account_name) = keyring_names(browser_name);
    let secret = secret_service_secret(service_name)
        .or_else(|| kwallet_secret(service_name, account_name));
    LinuxChromiumKeys {
        keyring: secret.map(|s| derive_aes_key_with_iterations(&s, PBKDF2_ITERATIONS_LINUX)),
        fixed: fixed_key(),
    }
}

#[cfg(target_os = "linux")]
fn secret_service_secret(service_name: &str) -> Option<zeroize::Zeroizing<Vec<u8>>> {
    use secret_service::blocking::SecretService;
    use secret_service::EncryptionType;
    let ss = SecretService::connect(EncryptionType::Dh).ok()?;
    let items = ss
        .search_items(std::collections::HashMap::from([("application", service_name)]))
        .ok()?;
    let secret = items
        .unlocked
        .into_iter()
        .chain(items.locked)
        .find_map(|item| {
            let _ = item.unlock();
            item.get_secret().ok().filter(|secret| !secret.is_empty())
        })
        .map(zeroize::Zeroizing::new);
    secret
}

#[cfg(target_os = "linux")]
fn kwallet_secret(service_name: &str, account_name: &str) -> Option<zeroize::Zeroizing<Vec<u8>>> {
    let folder = format!("{account_name} Keys");
    let output = std::process::Command::new("kwallet-query")
        .args(["-r", service_name, "-f", &folder, "kdewallet"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then(|| zeroize::Zeroizing::new(text.into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_key_is_one_round_of_peanuts() {
        // python3 -c "import hashlib; print(hashlib.pbkdf2_hmac('sha1', b'peanuts', b'saltysalt', 1, 16).hex())"
        assert_eq!(
            fixed_key().as_bytes(),
            &[0xfd, 0x62, 0x1f, 0xe5, 0xa2, 0xb4, 0x02, 0x53, 0x9d, 0xfa, 0x14, 0x7c, 0xa9, 0x27, 0x27, 0x78]
        );
    }

    #[test]
    fn keyring_key_is_tried_before_the_fixed_key() {
        let keys = LinuxChromiumKeys {
            keyring: Some(crate::decrypt::chromium_keychain::tests_support::make_key([1; 16])),
            fixed: fixed_key(),
        };
        let order: Vec<u8> = keys.iter().map(|k| k.as_bytes()[0]).collect();
        assert_eq!(order, vec![1, 0xfd]);
    }
}
