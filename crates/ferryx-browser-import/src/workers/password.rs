//! Password import worker.
//!
//! Sources, per browser family:
//! - **Safari**: a user-provided passwords CSV (`Safari → File → Export →
//!   Passwords`), since Safari credentials live in the iCloud Keychain behind an
//!   ACL no third-party app can read.
//! - **Chromium** (Chrome/Arc/Brave/Edge/Vivaldi/Opera): the `Login Data`
//!   SQLite `logins` table, `v10`-AES-128-CBC encrypted. The key is derived from
//!   the browser's `<Browser> Safe Storage` login-keychain generic password
//!   (macOS) or Secret Service / kwallet entry (Linux). This is the classic
//!   consent-prompt/ACL model — readable by other apps, unlike Safari.
//! - **Firefox/Zen**: `logins.json` + NSS `key4.db`, decrypted in-crate via the
//!   pure-Rust NSS path with an empty master password (the common case).
//!
//! ## Safety
//! Password values are never logged or placed in error strings.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::orchestrator::{ImportDestination, ImportProgress, ImportType};
use crate::parsers::chromium::passwords::parse_chromium_passwords_with;
use crate::parsers::firefox::passwords::parse_firefox_passwords;
use crate::parsers::safari::passwords_csv::parse_safari_passwords_csv;
use crate::{BrowserType, DetectedBrowser, ImportError, ImportResult, PasswordEntry};

#[cfg(target_os = "macos")]
use crate::decrypt::chromium_keychain::ChromiumKey;

#[cfg(target_os = "macos")]
use super::{fetch_keychain_key_cancellable, keychain_service_account};
use super::{is_cancelled, send_update, ImportWorker};

pub struct PasswordWorker {
    csv_path: Option<PathBuf>,
}

impl PasswordWorker {
    pub fn new(csv_path: Option<PathBuf>) -> Self {
        Self { csv_path }
    }
}

impl ImportWorker for PasswordWorker {
    fn run(
        &self,
        browser: &DetectedBrowser,
        destination: &dyn ImportDestination,
        cancelled: &Arc<AtomicBool>,
        progress: &Sender<ImportProgress>,
    ) -> ImportResult<u32> {
        let entries: Vec<PasswordEntry> = match browser.browser_type {
            BrowserType::Safari => {
                let Some(csv_path) = self.csv_path.as_ref() else {
                    return Err(ImportError::FileNotFound(
                        "no Safari passwords CSV was provided".into(),
                    ));
                };
                parse_safari_passwords_csv(csv_path)?
            }
            BrowserType::Chrome
            | BrowserType::Arc
            | BrowserType::Brave
            | BrowserType::Edge
            | BrowserType::Vivaldi
            | BrowserType::Opera => {
                let profile_dir = browser.profile_dir();

                if !profile_dir.join("Login Data").exists() {
                    return Ok(0);
                }

                chromium_passwords(browser.browser_type, profile_dir, cancelled)?
            }
            BrowserType::Firefox | BrowserType::Zen => {
                let profile_dir = browser.profile_dir();
                parse_firefox_passwords(profile_dir, "")?
            }
        };

        let mut count = 0u32;
        for entry in entries {
            if is_cancelled(cancelled) {
                break;
            }
            if entry.password.is_empty() || entry.origin_url.is_empty() {
                continue;
            }
            if !destination.add_password(entry) {
                return Err(ImportError::Io("password destination rejected the import".into()));
            }
            count += 1;
            if count.is_multiple_of(100) {
                send_update(
                    progress,
                    ImportType::Passwords,
                    count,
                    "Importing passwords...",
                );
            }
        }
        Ok(count)
    }
}

fn browser_name(browser_type: BrowserType) -> &'static str {
    match browser_type {
        BrowserType::Chrome => "Chrome",
        BrowserType::Brave => "Brave",
        BrowserType::Edge => "Edge",
        BrowserType::Vivaldi => "Vivaldi",
        BrowserType::Opera => "Opera",
        _ => "Chromium",
    }
}

/// Reads `Login Data` with the platform's key. A key that cannot be obtained is an error
/// (`KeyUnavailable`), not an empty import.
#[cfg(target_os = "macos")]
fn chromium_passwords(
    browser_type: BrowserType,
    profile_dir: &std::path::Path,
    cancelled: &std::sync::atomic::AtomicBool,
) -> ImportResult<Vec<PasswordEntry>> {
    let key: ChromiumKey = keychain_service_account(browser_type)
        .and_then(|(service, account)| {
            fetch_keychain_key_cancellable(service, account, None, Some(cancelled))
        })
        .ok_or_else(|| {
            ImportError::KeyUnavailable(format!("{} Safe Storage", browser_name(browser_type)))
        })?;
    parse_chromium_passwords_with(profile_dir, |blob| {
        crate::decrypt::chromium_keychain::decrypt_v10(blob, &key).ok()
    })
}

#[cfg(target_os = "linux")]
fn chromium_passwords(
    browser_type: BrowserType,
    profile_dir: &std::path::Path,
    _cancelled: &std::sync::atomic::AtomicBool,
) -> ImportResult<Vec<PasswordEntry>> {
    let keys = crate::decrypt::linux_keyring::fetch_linux_keys(browser_name(browser_type));
    parse_chromium_passwords_with(profile_dir, |blob| {
        keys.iter()
            .find_map(|key| crate::decrypt::chromium_keychain::decrypt_v10(blob, key).ok())
    })
}

/// Windows: `Login Data` passwords are `v10`, AES-256-GCM under the DPAPI-protected key in
/// `Local State`. A `v20` (app-bound) password, if a browser writes one, needs the elevation
/// service's key, fetched only when such a row exists.
#[cfg(target_os = "windows")]
fn chromium_passwords(
    browser_type: BrowserType,
    profile_dir: &std::path::Path,
    _cancelled: &std::sync::atomic::AtomicBool,
) -> ImportResult<Vec<PasswordEntry>> {
    let name = browser_name(browser_type);
    let local_state = profile_dir
        .ancestors()
        .take(4)
        .map(|dir| dir.join("Local State"))
        .find(|path| path.exists())
        .ok_or_else(|| ImportError::FileNotFound(profile_dir.join("../Local State").display().to_string()))?;
    use crate::decrypt::win_app_bound::WindowsAppBoundDecryptor;
    let v10 = WindowsAppBoundDecryptor::for_v10(&local_state).map_err(ImportError::KeyUnavailable)?;
    let v20 = std::cell::OnceCell::new();
    parse_chromium_passwords_with(profile_dir, |blob| {
        let payload = blob.get(3..)?;
        if blob.starts_with(b"v20") {
            let decryptor = v20.get_or_init(|| WindowsAppBoundDecryptor::for_browser(name, &local_state).ok());
            decryptor.as_ref()?.decrypt_cookie(payload).ok()
        } else {
            v10.decrypt_cookie(payload).ok()
        }
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn chromium_passwords(
    _browser_type: BrowserType,
    _profile_dir: &std::path::Path,
    _cancelled: &std::sync::atomic::AtomicBool,
) -> ImportResult<Vec<PasswordEntry>> {
    Err(ImportError::UnsupportedScheme("Chromium password import on this platform".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::tests::MockDestination;
    use std::sync::mpsc;

    fn safari_browser(profile: &str) -> DetectedBrowser {
        DetectedBrowser {
            browser_type: BrowserType::Safari,
            display_name: "Safari".to_string(),
            profile_path: std::path::PathBuf::from(profile),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        }
    }

    #[test]
    fn review_destination_rejection_is_not_reported_as_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("passwords.csv");
        std::fs::write(&path, "Title,URL,Username,Password\nExample,https://example.com,user,secret\n").unwrap();
        let mut destination = MockDestination::new();
        destination.reject_passwords = true;
        let (tx, _rx) = mpsc::channel();
        let result = PasswordWorker::new(Some(path)).run(
            &safari_browser("/dev/null"), &destination,
            &Arc::new(AtomicBool::new(false)), &tx,
        );
        assert!(result.is_err(), "destination rejected the password, got {result:?}");
        assert!(destination.passwords.lock().unwrap().is_empty());
    }

    #[test]
    fn imports_passwords_from_csv_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("safari-passwords.csv");
        std::fs::write(
            &path,
            "Title,URL,Username,Password,Notes,OTPAuth\n\
             Apple,https://apple.com,user@example.com,hunter2,,\n\
             GitHub,https://github.com,octocat,s3cr3t,,\n",
        )
        .unwrap();

        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::channel();

        let worker = PasswordWorker::new(Some(path));
        let count = worker
            .run(&safari_browser("/dev/null"), &*dest, &cancelled, &tx)
            .unwrap();

        assert_eq!(count, 2);
        let passwords = dest.passwords.lock().unwrap();
        assert_eq!(passwords.len(), 2);
        assert_eq!(passwords[0].origin_url, "https://apple.com");
        assert_eq!(passwords[0].username, "user@example.com");
        assert_eq!(passwords[1].origin_url, "https://github.com");
    }

    #[test]
    fn missing_csv_path_is_error() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::channel();

        let worker = PasswordWorker::new(None);
        let result = worker.run(&safari_browser("/dev/null"), &*dest, &cancelled, &tx);
        assert!(matches!(result, Err(ImportError::FileNotFound(_))));
    }

    #[test]
    fn chromium_without_login_data_is_noop() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Chrome,
            display_name: "Chrome".to_string(),
            profile_path: std::path::PathBuf::from("/nonexistent/Default/Preferences"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        let worker = PasswordWorker::new(None);
        let count = worker.run(&browser, &*dest, &cancelled, &tx).unwrap();
        assert_eq!(count, 0);
        assert!(dest.passwords.lock().unwrap().is_empty());
    }

    #[test]
    fn imports_firefox_passwords_via_nss() {
        let (_temp, profile, _master_key) =
            crate::decrypt::firefox_nss::tests_support::create_test_profile(
                "",
                &["alice", "s3cr3t", "https://example.com"],
            );

        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Firefox,
            display_name: "Firefox".to_string(),
            profile_path: profile.join("prefs.js"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        let worker = PasswordWorker::new(None);
        let count = worker.run(&browser, &*dest, &cancelled, &tx).unwrap();

        assert_eq!(count, 1);
        let passwords = dest.passwords.lock().unwrap();
        assert_eq!(passwords.len(), 1);
        assert_eq!(passwords[0].origin_url, "https://example.com");
        assert_eq!(passwords[0].username, "alice");
    }

    #[test]
    fn firefox_missing_profile_is_error() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Firefox,
            display_name: "Firefox".to_string(),
            profile_path: std::path::PathBuf::from("/nonexistent/Default/prefs.js"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        let worker = PasswordWorker::new(None);
        let result = worker.run(&browser, &*dest, &cancelled, &tx);
        assert!(matches!(result, Err(ImportError::FileNotFound(_))));
    }
}
