//! Cookie import worker.
//!
//! Reads cookies from the source browser and writes them to the destination.
//!
//! For Chromium-derived browsers, v10-encrypted cookies are transparently
//! decrypted via macOS Keychain (`<Browser> Safe Storage` entry). v20 cookies
//! (Chrome 117+ App-Bound Encryption) are skipped outside Windows — they
//! require an extra Win32 DPAPI hop / per-app entitlement that is out of scope
//! here. Every encrypted cookie that cannot be decrypted is counted and
//! reported once per import as a `Summary` line ("N cookies need sign-in
//! again"), so the loss is visible instead of silent.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::orchestrator::{ImportDestination, ImportProgress, ImportType};
use crate::parsers;
use crate::parsers::chromium::cookies::{ChromiumCookieEntry, EncryptionVersion};
use crate::{BrowserType, DetectedBrowser, ImportResult};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::decrypt::chromium_keychain::decrypt_cookie_v10;
#[cfg(target_os = "macos")]
use crate::decrypt::chromium_keychain::ChromiumKey;

#[cfg(target_os = "macos")]
use super::{fetch_keychain_key_cancellable, keychain_service_account};
use super::{is_cancelled, send_update, ImportWorker};

pub struct CookieWorker;

impl ImportWorker for CookieWorker {
    fn run(
        &self,
        browser: &DetectedBrowser,
        destination: &dyn ImportDestination,
        cancelled: &Arc<AtomicBool>,
        progress: &Sender<ImportProgress>,
    ) -> ImportResult<u32> {
        let profile_dir = browser.profile_dir();

        let mut count = 0u32;

        match browser.browser_type {
            BrowserType::Safari => {
                // Safari stores cookies in the proprietary binarycookies format
                // (plaintext values — no Keychain decryption needed).
                let cookies = parsers::safari::cookies::parse_safari_cookies(profile_dir)?;
                for cookie in &cookies {
                    if is_cancelled(cancelled) {
                        break;
                    }
                    if cookie.value.is_empty() || cookie.host.is_empty() {
                        continue;
                    }
                    if !destination.add_cookie(cookie) {
                        continue;
                    }
                    count += 1;
                    if count.is_multiple_of(500) {
                        send_update(progress, ImportType::Cookies, count, "Importing cookies...");
                    }
                }
            }
            BrowserType::Firefox | BrowserType::Zen => {
                let cookies = parsers::firefox::cookies::parse_firefox_cookies(profile_dir)?;
                for cookie in &cookies {
                    if is_cancelled(cancelled) {
                        break;
                    }
                    if cookie.value.is_empty() || cookie.host.is_empty() {
                        continue;
                    }
                    if !destination.add_cookie(cookie) {
                        continue;
                    }
                    count += 1;
                    if count.is_multiple_of(500) {
                        send_update(progress, ImportType::Cookies, count, "Importing cookies...");
                    }
                }
            }
            BrowserType::Chrome
            | BrowserType::Arc
            | BrowserType::Brave
            | BrowserType::Edge
            | BrowserType::Vivaldi
            | BrowserType::Opera => {
                let entries = parsers::chromium::cookies::parse_chromium_cookies(profile_dir)?;

                #[cfg(target_os = "macos")]
                let decryption_key: Option<ChromiumKey> = keychain_service_account(
                    browser.browser_type,
                )
                .and_then(|(service, account)| {
                    fetch_keychain_key_cancellable(
                        service,
                        account,
                        None,
                        Some(cancelled),
                    )
                });

                #[cfg(target_os = "linux")]
                let linux_keys = {
                    let browser_name = match browser.browser_type {
                        BrowserType::Chrome => "Chrome",
                        BrowserType::Brave => "Brave",
                        BrowserType::Edge => "Edge",
                        BrowserType::Vivaldi => "Vivaldi",
                        BrowserType::Opera => "Opera",
                        _ => "Chromium",
                    };
                    crate::decrypt::linux_keyring::fetch_linux_keys(browser_name)
                };

                #[cfg(target_os = "windows")]
                let local_state = find_local_state(profile_dir);
                // `v10` cookies use the DPAPI key; `v20` (app-bound) ones the key only the
                // browser's elevation service releases. Each is fetched once, if needed.
                #[cfg(target_os = "windows")]
                let windows_v10 = local_state.as_deref().and_then(|path| {
                    crate::decrypt::win_app_bound::WindowsAppBoundDecryptor::for_v10(path).ok()
                });
                #[cfg(target_os = "windows")]
                let windows_v20 = std::cell::OnceCell::new();
                #[cfg(target_os = "windows")]
                let windows_decryptor_v20 = || {
                    windows_v20
                        .get_or_init(|| {
                            let browser_name = match browser.browser_type {
                                BrowserType::Chrome => "Chrome",
                                BrowserType::Brave => "Brave",
                                BrowserType::Edge => "Edge",
                                BrowserType::Vivaldi => "Vivaldi",
                                BrowserType::Opera => "Opera",
                                _ => "Chromium",
                            };
                            local_state.as_deref().and_then(|path| {
                                crate::decrypt::win_app_bound::WindowsAppBoundDecryptor::for_browser(
                                    browser_name,
                                    path,
                                )
                                .ok()
                            })
                        })
                        .as_ref()
                };

                let decrypt = |entry: &ChromiumCookieEntry| -> Option<String> {
                    match entry.encryption_version {
                        EncryptionVersion::V10 => {
                            #[cfg(target_os = "macos")]
                            {
                                decryption_key.as_ref().and_then(|key| {
                                    decrypt_cookie_v10(&entry.encrypted_value, key, entry.database_version, &entry.cookie.host).ok()
                                })
                            }
                            #[cfg(target_os = "linux")]
                            {
                                linux_keys.iter().find_map(|key| {
                                    decrypt_cookie_v10(&entry.encrypted_value, key, entry.database_version, &entry.cookie.host).ok()
                                })
                            }
                            #[cfg(target_os = "windows")]
                            {
                                windows_v10.as_ref().and_then(|dec| {
                                    dec.decrypt_bound_cookie(&entry.encrypted_value[3..], entry.database_version, &entry.cookie.host).ok()
                                })
                            }
                            #[cfg(not(any(
                                target_os = "macos",
                                target_os = "linux",
                                target_os = "windows"
                            )))]
                            {
                                None
                            }
                        }
                        EncryptionVersion::V20 => {
                            #[cfg(target_os = "windows")]
                            {
                                windows_decryptor_v20().and_then(|dec| {
                                    dec.decrypt_bound_cookie(&entry.encrypted_value[3..], entry.database_version, &entry.cookie.host).ok()
                                })
                            }
                            #[cfg(not(target_os = "windows"))]
                            {
                                None
                            }
                        }
                        _ => None,
                    }
                };

                let tally =
                    import_chromium_entries(&entries, decrypt, destination, cancelled, progress);
                count = tally.imported;
                send_sign_in_summary(progress, tally.needs_sign_in);
            }
        }
        Ok(count)
    }
}

/// Outcome of writing parsed Chromium cookies to the destination.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ChromiumCookieTally {
    imported: u32,
    /// Encrypted cookies that could not be decrypted (on macOS/Linux this
    /// includes every App-Bound `v20` cookie). Their sites need sign-in again.
    needs_sign_in: u32,
}

fn import_chromium_entries(
    entries: &[ChromiumCookieEntry],
    decrypt: impl Fn(&ChromiumCookieEntry) -> Option<String>,
    destination: &dyn ImportDestination,
    cancelled: &AtomicBool,
    progress: &Sender<ImportProgress>,
) -> ChromiumCookieTally {
    let mut tally = ChromiumCookieTally::default();
    for entry in entries {
        if is_cancelled(cancelled) {
            break;
        }
        let c = &entry.cookie;
        if c.host.is_empty() {
            continue;
        }

        let decrypted_value: Option<String> = if !c.value.is_empty() {
            Some(c.value.clone())
        } else if !entry.encrypted_value.is_empty() {
            decrypt(entry)
        } else {
            None
        };

        let Some(value) = decrypted_value else {
            if !entry.encrypted_value.is_empty() {
                tally.needs_sign_in += 1;
            }
            continue;
        };
        let cookie = crate::CookieEntry { value, ..c.clone() };
        if !destination.add_cookie(&cookie) {
            continue;
        }
        tally.imported += 1;
        if tally.imported.is_multiple_of(500) {
            send_update(
                progress,
                ImportType::Cookies,
                tally.imported,
                "Importing cookies...",
            );
        }
    }
    tally
}

/// The per-import summary line for cookies that were dropped, or `None` when
/// every cookie carried over.
pub fn cookies_need_sign_in_summary(dropped: u32) -> Option<String> {
    match dropped {
        0 => None,
        1 => Some("1 cookie needs sign-in again".to_string()),
        n => Some(format!("{n} cookies need sign-in again")),
    }
}

fn send_sign_in_summary(progress: &Sender<ImportProgress>, dropped: u32) {
    if let Some(message) = cookies_need_sign_in_summary(dropped) {
        let _ = progress.send(ImportProgress::Summary {
            import_type: ImportType::Cookies,
            count: dropped,
            message,
        });
    }
}

#[cfg(target_os = "windows")]
fn find_local_state(start_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut current = start_dir.to_path_buf();
    for _ in 0..4 {
        let candidate = current.join("Local State");
        if candidate.exists() {
            return Some(candidate);
        }
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::tests::MockDestination;
    use std::sync::mpsc;

    #[test]
    fn test_cookie_worker_nonexistent_safari() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Safari,
            display_name: "Safari".to_string(),
            profile_path: std::path::PathBuf::from("/nonexistent/Library/Safari/Bookmarks.plist"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        let result = CookieWorker.run(&browser, &*dest, &cancelled, &tx);
        assert!(result.is_err());
    }

    #[test]
    fn test_cookie_destination_interaction() {
        let dest = Arc::new(MockDestination::new());

        let cookie = |host: &str, name: &str, value: &str, path: &str| crate::CookieEntry {
            host: host.into(),
            name: name.into(),
            value: value.into(),
            path: path.into(),
            ..Default::default()
        };
        dest.add_cookie(&cookie(".example.com", "session", "abc123", "/"));
        dest.add_cookie(&cookie(".test.org", "pref", "dark", "/settings"));

        let cookies = dest.cookies.lock().unwrap();
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].host, ".example.com");
        assert_eq!(cookies[0].name, "session");
        assert_eq!(cookies[1].host, ".test.org");
    }

    #[test]
    fn test_cookie_worker_nonexistent_firefox() {
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

        let result = CookieWorker.run(&browser, &*dest, &cancelled, &tx);
        assert!(result.is_err());
    }

    fn chromium_entry(host: &str, value: &str, encrypted: &[u8]) -> ChromiumCookieEntry {
        ChromiumCookieEntry {
            cookie: crate::CookieEntry {
                host: host.to_string(),
                name: "n".to_string(),
                value: value.to_string(),
                path: "/".to_string(),
                ..Default::default()
            },
            encrypted_value: encrypted.to_vec(),
            encryption_version: parsers::chromium::cookies::detect_encryption_version(encrypted),
            database_version: 24,
        }
    }

    #[test]
    fn undecryptable_cookies_are_counted_and_summarized() {
        let dest = MockDestination::new();
        let cancelled = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel();
        let entries = vec![
            chromium_entry(".plain.test", "plain", b""),
            chromium_entry(".v10ok.test", "", b"v10ok"),
            chromium_entry(".v10bad.test", "", b"v10bad"),
            chromium_entry(".v20.test", "", b"v20abound"),
            chromium_entry(".v20b.test", "", b"v20abound"),
            // Neither a value nor ciphertext: nothing to sign back into.
            chromium_entry(".empty.test", "", b""),
            // No host: skipped before decryption.
            chromium_entry("", "", b"v20abound"),
        ];
        // Mirrors macOS/Linux: v10 decrypts when the key matches, v20 never does.
        let decrypt = |e: &ChromiumCookieEntry| -> Option<String> {
            (e.encrypted_value.as_slice() == b"v10ok").then(|| "decrypted".to_string())
        };

        let tally = import_chromium_entries(&entries, decrypt, &dest, &cancelled, &tx);
        send_sign_in_summary(&tx, tally.needs_sign_in);

        assert_eq!(
            tally,
            ChromiumCookieTally {
                imported: 2,
                needs_sign_in: 3
            }
        );
        let hosts: Vec<String> = dest.cookies.lock().unwrap().iter().map(|c| c.host.clone()).collect();
        assert_eq!(hosts, vec![".plain.test", ".v10ok.test"]);
        let msgs: Vec<_> = rx.try_iter().collect();
        assert_eq!(
            msgs,
            vec![ImportProgress::Summary {
                import_type: ImportType::Cookies,
                count: 3,
                message: "3 cookies need sign-in again".to_string(),
            }]
        );
    }

    #[test]
    fn no_summary_when_every_cookie_carries_over() {
        let (tx, rx) = mpsc::channel();
        send_sign_in_summary(&tx, 0);
        assert!(rx.try_iter().next().is_none());
        assert_eq!(cookies_need_sign_in_summary(0), None);
        assert_eq!(
            cookies_need_sign_in_summary(1).as_deref(),
            Some("1 cookie needs sign-in again")
        );
        assert_eq!(
            cookies_need_sign_in_summary(42).as_deref(),
            Some("42 cookies need sign-in again")
        );
    }

    #[test]
    fn cancelled_import_stops_counting() {
        let dest = MockDestination::new();
        let cancelled = AtomicBool::new(true);
        let (tx, _rx) = mpsc::channel();
        let entries = vec![chromium_entry(".v20.test", "", b"v20abound")];
        let tally = import_chromium_entries(&entries, |_| None, &dest, &cancelled, &tx);
        assert_eq!(tally, ChromiumCookieTally::default());
    }
}
