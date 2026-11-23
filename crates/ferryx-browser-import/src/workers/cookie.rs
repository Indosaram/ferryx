//! Cookie import worker.
//!
//! Reads cookies from the source browser and writes them to the destination.
//!
//! For Chromium-derived browsers, v10-encrypted cookies are transparently
//! decrypted via macOS Keychain (`<Browser> Safe Storage` entry). v20 cookies
//! (Chrome 117+ App-Bound Encryption) need the browser's elevation service on
//! Windows, which refuses any caller but the browser itself: those rows are
//! reported with their reason (`app-bound-key-refused`) and never applied,
//! while the same profile's v10 rows still import. Every encrypted cookie that
//! cannot be decrypted is counted and reported once per import as a `Summary`
//! line ("N cookies need sign-in again"), so the loss is visible instead of
//! silent.

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
                // `v20` is fetched at most once. An error here is a measured refusal from the
                // elevation service, NOT a reason to reach for the `v10` key (which cannot open
                // a `v20` row): it is reported with the row count instead.
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
                            local_state
                                .as_deref()
                                .ok_or_else(|| {
                                    crate::decrypt::win_app_bound::AppBoundKeyError::Unavailable(
                                        "no Local State above the profile directory".into(),
                                    )
                                })
                                .and_then(|path| {
                                    crate::decrypt::win_app_bound::WindowsAppBoundDecryptor::for_browser(
                                        browser_name,
                                        path,
                                    )
                                })
                        })
                        .as_ref()
                        .map_err(|error| error.clone())
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
                                windows_decryptor_v20().ok().and_then(|dec| {
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
                // An app-bound key the service refused is why the `v20` rows were dropped: report
                // it as seen/not applied instead of leaving a bare "needs sign-in" count. The
                // `v10` rows above were still imported while that key was fetched, so the refusal
                // never costs them.
                #[cfg(target_os = "windows")]
                if tally.v20_unapplied > 0 {
                    if let Err(refusal) = windows_decryptor_v20() {
                        let _ = progress.send(ImportProgress::Blocked {
                            import_type: ImportType::Cookies,
                            code: refusal.code().to_string(),
                            message: refusal.to_string(),
                            rows: tally.v20_unapplied,
                            hresult: refusal.hresult(),
                        });
                    }
                }
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
    /// The subset of `needs_sign_in` that were App-Bound (`v20`) rows: these are the ones an
    /// app-bound key refusal is responsible for, so they are the count reported with it.
    v20_unapplied: u32,
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
                if entry.encryption_version == EncryptionVersion::V20 {
                    tally.v20_unapplied += 1;
                }
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
                needs_sign_in: 3,
                v20_unapplied: 2,
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

    /// End to end on a real profile layout: a temp `Local State` holding an APPB key and a temp
    /// `Network/Cookies` database with v10 and v20 rows. The elevation service answers "refused"
    /// (the measured `0x8004A007`), so the v20 rows must be reported once with that reason and
    /// left unapplied, while the v10 rows still arrive.
    #[cfg(windows)]
    #[test]
    fn a_refused_app_bound_key_reports_the_v20_rows_and_still_imports_the_v10_ones() {
        use crate::decrypt::win_app_bound::{AppBoundKeyError, COM_DECRYPT_LOCK, COM_DECRYPT_OVERRIDE};
        use base64::Engine as _;

        let _guard = COM_DECRYPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let key = [0x2Bu8; 32];
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("Default");
        let network = profile.join("Network");
        std::fs::create_dir_all(&network).unwrap();

        // Local State: the v10 DPAPI key (real, user-context) and an APPB key the service refuses.
        let mut dpapi = b"DPAPI".to_vec();
        dpapi.extend_from_slice(&dpapi_protect(&key));
        let mut app_bound = b"APPB".to_vec();
        app_bound.extend_from_slice(b"sealed-app-bound");
        let local_state = serde_json::json!({
            "os_crypt": {
                "encrypted_key": base64::engine::general_purpose::STANDARD.encode(&dpapi),
                "app_bound_encrypted_key": base64::engine::general_purpose::STANDARD.encode(&app_bound),
            }
        });
        std::fs::write(dir.path().join("Local State"), local_state.to_string()).unwrap();

        // Cookies: one v10 row (decrypts), two v20 rows (refused), one plaintext row.
        let conn = rusqlite::Connection::open(network.join("Cookies")).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (key TEXT NOT NULL UNIQUE, value TEXT NOT NULL);
             INSERT INTO meta (key, value) VALUES ('version', '24');
             CREATE TABLE cookies (
                 host_key TEXT NOT NULL, name TEXT NOT NULL, value TEXT NOT NULL DEFAULT '',
                 encrypted_value BLOB DEFAULT x'', path TEXT NOT NULL DEFAULT '/',
                 expires_utc INTEGER NOT NULL DEFAULT 0, is_secure INTEGER NOT NULL DEFAULT 0,
                 is_httponly INTEGER NOT NULL DEFAULT 0, samesite INTEGER NOT NULL DEFAULT -1,
                 source_scheme INTEGER NOT NULL DEFAULT 0);",
        )
        .unwrap();
        let expires = (4_000_000_000i64 + 11_644_473_600) * 1_000_000;
        let insert = |host: &str, name: &str, value: &str, encrypted: &[u8]| {
            conn.execute(
                "INSERT INTO cookies (host_key, name, value, encrypted_value, expires_utc, is_secure, is_httponly, samesite)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, 1, 1)",
                rusqlite::params![host, name, value, encrypted, expires],
            )
            .unwrap();
        };
        insert(".v10.test", "v10", "", &v10_cookie(&key, ".v10.test", "v10-value"));
        insert(".v20a.test", "a", "", b"v20sealed-ciphertext-a");
        insert(".v20b.test", "b", "", b"v20sealed-ciphertext-b");
        insert(".plain.test", "plain", "plain-value", b"");
        drop(conn);

        *COM_DECRYPT_OVERRIDE.lock().unwrap() = Some(|_browser, payload| {
            assert_eq!(payload, b"sealed-app-bound", "the APPB prefix is stripped before COM");
            Err(AppBoundKeyError::Refused { hresult: 0x8004_A007 })
        });

        let dest = MockDestination::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let browser = DetectedBrowser {
            browser_type: BrowserType::Chrome,
            display_name: "Google Chrome".into(),
            profile_path: profile,
            services_supported: crate::ImportServices::COOKIES,
            requires_full_disk_access: false,
        };
        let outcome = CookieWorker.run(&browser, &dest, &cancelled, &tx);
        *COM_DECRYPT_OVERRIDE.lock().unwrap() = None;

        let imported = outcome.expect("the worker returns its count");
        assert_eq!(imported, 2, "the v10 and plaintext rows still import");
        let hosts: Vec<String> =
            dest.cookies.lock().unwrap().iter().map(|c| c.host.clone()).collect();
        assert_eq!(hosts, vec![".v10.test", ".plain.test"], "v20 rows are not applied");
        assert_eq!(
            dest.cookies.lock().unwrap()[0].value, "v10-value",
            "the v10 row really decrypted"
        );

        let msgs: Vec<_> = rx.try_iter().collect();
        let blocked = msgs
            .iter()
            .find_map(|m| match m {
                ImportProgress::Blocked { code, rows, hresult, message, .. } => {
                    Some((code.clone(), *rows, *hresult, message.clone()))
                }
                _ => None,
            })
            .expect("the refusal is reported, not swallowed");
        assert_eq!(blocked.0, "app-bound-key-refused");
        assert_eq!(blocked.1, 2, "both v20 rows are reported as seen and not applied");
        assert_eq!(blocked.2, Some(0x8004_A007), "the measured HRESULT travels with it");
        assert!(blocked.3.contains("0x8004A007"), "{}", blocked.3);

        // The synthetic-import report: what the importer saw, what it reported, what it applied.
        println!(
            "SYNTHETIC_IMPORT {}",
            serde_json::json!({
                "source": "temp Local State with an APPB key + temp Network/Cookies with v20 rows",
                "v20_seen": blocked.1,
                "reported_reason": blocked.0,
                "reported_hresult": format!("0x{:08X}", blocked.2.unwrap()),
                "v20_not_applied": blocked.1,
                "applied_hosts": hosts,
                "imported_count": imported,
            })
        );

        assert!(
            msgs.iter().any(|m| matches!(
                m, ImportProgress::Summary { count: 2, message, .. }
                    if message == "2 cookies need sign-in again"
            )),
            "{msgs:?}"
        );
    }

    #[cfg(windows)]
    fn dpapi_protect(plain: &[u8]) -> Vec<u8> {
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
        let input = CRYPT_INTEGER_BLOB {
            cbData: plain.len() as u32,
            pbData: plain.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        unsafe {
            CryptProtectData(&input, None, None, None, None, 0, &mut out).unwrap();
            let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out.pbData as *mut std::ffi::c_void)));
            bytes
        }
    }

    /// A `v10` AES-256-GCM cookie blob, host-bound for database version 24.
    #[cfg(windows)]
    fn v10_cookie(key: &[u8], host: &str, value: &str) -> Vec<u8> {
        use aes_gcm::aead::{Aead, KeyInit};
        use aes_gcm::{Aes256Gcm, Nonce};
        use sha2::{Digest, Sha256};
        let iv = [0x31u8; 12];
        let mut plain = Sha256::digest(host.as_bytes()).to_vec();
        plain.extend_from_slice(value.as_bytes());
        let cipher = Aes256Gcm::new_from_slice(key).unwrap();
        let mut blob = b"v10".to_vec();
        blob.extend_from_slice(&iv);
        blob.extend(cipher.encrypt(Nonce::from_slice(&iv), plain.as_slice()).unwrap());
        blob
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
