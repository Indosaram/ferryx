//! "Import from Chrome / Edge" in Session & Cookies: reads one installed Chrome or Edge
//! profile's cookies through the ported importer (the same parser, snapshot copy and
//! Keychain / keyring / DPAPI decryption the Import dialog uses) and hands them back for the
//! profile's open webviews. Failures carry the `reason` the settings UI already maps.

use super::cookie_sink::imported_cookie;
use super::CookieEntry;
use crate::browser::cookies::{
    is_google_cookie, resolve_installed_browser_profile_dir, ImportedCookie, InstalledBrowserKind,
};
use ferryx_browser_import::orchestrator::{ImportDestination, ImportProgress, ImportType, Orchestrator};
use ferryx_browser_import::{BrowserType, DetectedBrowser, ImportError, ImportServices};
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstalledCookieImportFailureReason {
    NoProfile,
    KeychainDenied,
    NoCookies,
    /// The browser's elevation service refused its app-bound (`v20`) key: those cookies were
    /// seen and reported but not applied. The same profile's `v10` cookies still are.
    AppBoundKeyRefused,
    CookieImportFailed,
}

impl InstalledCookieImportFailureReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoProfile => "no-profile",
            Self::KeychainDenied => "keychain-denied",
            Self::NoCookies => "no-cookies",
            Self::AppBoundKeyRefused => "app-bound-key-refused",
            Self::CookieImportFailed => "cookie-import-failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: {message}", .reason.as_str())]
pub struct InstalledCookieImportError {
    pub reason: InstalledCookieImportFailureReason,
    pub message: String,
    /// The measured COM status behind an app-bound refusal (`0x8004A007` = the elevation
    /// service's `kValidationDidNotPass` for any caller that is not the browser's own image).
    /// `None` for every other failure; it is what makes the refusal diagnosable instead of
    /// leaving "N cookies need sign-in again" as the only trace.
    pub hresult: Option<u32>,
    /// How many rows the importer named as left unapplied by this block (its "seen" count).
    /// `None` when the failure is not a block of rows (a missing profile, a refused keychain).
    pub rows: Option<usize>,
}

impl InstalledCookieImportError {
    /// The `details` of this failure's structured `{code, message, details}` IPC error.
    pub fn details(&self) -> serde_json::Value {
        let mut details = serde_json::json!({ "reason": self.reason.as_str() });
        if let Some(hresult) = self.hresult {
            details["hresult"] = serde_json::json!(format!("0x{hresult:08X}"));
        }
        if let Some(rows) = self.rows {
            details["rows"] = serde_json::json!(rows);
        }
        details
    }
}

fn failure(reason: InstalledCookieImportFailureReason, message: impl Into<String>) -> InstalledCookieImportError {
    InstalledCookieImportError { reason, message: message.into(), hresult: None, rows: None }
}

/// The cookies read from one installed profile.
#[derive(Debug, Default)]
pub struct InstalledCookies {
    pub cookies: Vec<ImportedCookie>,
    /// Cookies that could not be decrypted, had expired, or had no name or host.
    pub skipped: usize,
    /// Google cookies, left out on purpose (Google signs the account out when they move).
    pub google_excluded: usize,
    /// Why some rows could not be applied even though others were, when the importer named a
    /// reason: today the browser refusing its app-bound key. Reported alongside the imported
    /// `v10` cookies so the loss is never just a count.
    pub blocked: Option<InstalledCookieImportError>,
}

/// Collects what the ported cookie worker reads.
struct Collector {
    now_unix: i64,
    read: Mutex<InstalledCookies>,
}

impl ImportDestination for Collector {
    fn create_space(&self, _: &str, _: &str, _: &str) -> Option<String> {
        None
    }
    fn create_tab(&self, _: &str, _: &str, _: &str) -> Option<String> {
        None
    }
    fn create_tab_in_folder(&self, _: &str, _: &str, _: &str, _: &str) -> Option<String> {
        None
    }
    fn create_folder(&self, _: &str, _: &str, _: &str) -> Option<String> {
        None
    }
    fn pin_tab(&self, _: &str) {}
    fn favorite_tab(&self, _: &str) {}
    fn activate_space(&self, _: &str) {}
    fn get_active_space_id(&self) -> Option<String> {
        None
    }
    fn add_bookmark(&self, _: &str, _: &str, _: &[String]) -> Option<String> {
        None
    }
    fn add_history(&self, _: &str, _: &str, _: f64, _: u32) -> bool {
        false
    }
    fn add_cookie(&self, entry: &CookieEntry) -> bool {
        let mut read = self.read.lock();
        match imported_cookie(entry, self.now_unix) {
            None => read.skipped += 1,
            Some(cookie) if is_google_cookie(cookie.domain.as_deref(), &cookie.name) => {
                read.google_excluded += 1
            }
            Some(cookie) => {
                read.cookies.push(cookie);
                return true;
            }
        }
        false
    }
    fn add_autofill(&self, _: &str, _: &str, _: i32, _: i64, _: i64) -> bool {
        false
    }
    fn add_favicon(&self, _: &str, _: &[u8]) -> bool {
        false
    }
}

fn read_failure(error: &ImportError) -> InstalledCookieImportError {
    match error {
        ImportError::FileNotFound(_) => {
            failure(InstalledCookieImportFailureReason::NoProfile, error.to_string())
        }
        ImportError::KeyUnavailable(_) => {
            failure(InstalledCookieImportFailureReason::KeychainDenied, error.to_string())
        }
        ImportError::AppBoundKeyRefused { hresult } => InstalledCookieImportError {
            reason: InstalledCookieImportFailureReason::AppBoundKeyRefused,
            message: error.to_string(),
            hresult: Some(*hresult),
            rows: None,
        },
        _ => failure(InstalledCookieImportFailureReason::CookieImportFailed, error.to_string()),
    }
}

/// Reads the cookies of `profile_name` (default `Default`) of the installed `kind` under
/// `home_dir`. Blocking (file copies, Keychain / keyring / DPAPI).
pub fn read_installed_browser_cookies(
    kind: InstalledBrowserKind,
    profile_name: Option<&str>,
    home_dir: Option<&Path>,
) -> Result<InstalledCookies, InstalledCookieImportError> {
    use InstalledCookieImportFailureReason as Reason;
    let profile_dir = resolve_installed_browser_profile_dir(kind, profile_name, home_dir)
        .map_err(|error| failure(Reason::NoProfile, error.to_string()))?;
    if !profile_dir.is_dir() {
        return Err(failure(
            Reason::NoProfile,
            format!("profile directory does not exist at {}", profile_dir.display()),
        ));
    }
    let (browser_type, display_name) = match kind {
        InstalledBrowserKind::Chrome => (BrowserType::Chrome, "Google Chrome"),
        InstalledBrowserKind::Edge => (BrowserType::Edge, "Microsoft Edge"),
    };
    let browser = DetectedBrowser {
        browser_type,
        display_name: display_name.into(),
        profile_path: profile_dir,
        services_supported: ImportServices::COOKIES,
        requires_full_disk_access: false,
    };
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let collector = Arc::new(Collector { now_unix, read: Mutex::new(InstalledCookies::default()) });
    let orchestrator = Orchestrator::new(Arc::clone(&collector) as Arc<dyn ImportDestination>);
    let (progress, events) = std::sync::mpsc::channel();
    let outcome = orchestrator.run_type(ImportType::Cookies, &browser, &progress);
    drop(progress);
    outcome.map_err(|error| read_failure(&error))?;
    let mut needs_sign_in = 0usize;
    let mut blocked = None;
    for event in events.try_iter() {
        match event {
            ImportProgress::Summary { count, .. } => needs_sign_in += count as usize,
            // The worker names the reason its rows were dropped (`app-bound-key-refused` with the
            // measured HRESULT) before the count; keep it so the shell sees why, not just how many.
            ImportProgress::Blocked { code, message, hresult, rows, .. } => {
                blocked = Some(InstalledCookieImportError {
                    reason: match code.as_str() {
                        "app-bound-key-refused" => {
                            InstalledCookieImportFailureReason::AppBoundKeyRefused
                        }
                        _ => InstalledCookieImportFailureReason::CookieImportFailed,
                    },
                    message,
                    hresult,
                    rows: Some(rows as usize),
                });
            }
            _ => {}
        }
    }

    let mut read = std::mem::take(&mut *collector.read.lock());
    read.skipped += needs_sign_in;
    read.blocked = blocked;
    if read.cookies.is_empty() {
        // Every encrypted cookie failing means the browser's key was not released; when the
        // importer named the refusal, that reason (and its HRESULT) is the answer.
        if let Some(refusal) = read.blocked {
            return Err(refusal);
        }
        if needs_sign_in > 0 {
            return Err(failure(
                Reason::KeychainDenied,
                format!("{needs_sign_in} cookies could not be decrypted: the browser's encryption key was not available"),
            ));
        }
        return Err(failure(Reason::NoCookies, "no valid non-Google cookies found in profile database"));
    }
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome_profile(home: &Path) -> std::path::PathBuf {
        let dir = resolve_installed_browser_profile_dir(InstalledBrowserKind::Chrome, Some("Default"), Some(home))
            .expect("home directory is provided");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A `Network/Cookies` database (Chromium 96+) with plaintext rows only, so no key is needed.
    #[cfg(not(target_os = "macos"))]
    fn write_cookies(profile: &Path, rows: &[(&str, &str, &str, i64)]) {
        let network = profile.join("Network");
        std::fs::create_dir_all(&network).unwrap();
        let conn = rusqlite::Connection::open(network.join("Cookies")).unwrap();
        conn.execute_batch(
            "CREATE TABLE cookies (host_key TEXT NOT NULL, name TEXT NOT NULL, value TEXT NOT NULL DEFAULT '',
                 encrypted_value BLOB DEFAULT x'', path TEXT NOT NULL DEFAULT '/', expires_utc INTEGER NOT NULL DEFAULT 0,
                 is_secure INTEGER NOT NULL DEFAULT 0, is_httponly INTEGER NOT NULL DEFAULT 0,
                 samesite INTEGER NOT NULL DEFAULT -1, source_scheme INTEGER NOT NULL DEFAULT 0);",
        )
        .unwrap();
        for (host, name, value, expires_unix) in rows {
            // Chromium time: microseconds since 1601-01-01.
            let expires_utc = (expires_unix + 11_644_473_600) * 1_000_000;
            conn.execute(
                "INSERT INTO cookies (host_key, name, value, path, expires_utc, is_secure, is_httponly, samesite)
                 VALUES (?1, ?2, ?3, '/', ?4, 1, 1, 1)",
                rusqlite::params![host, name, value, expires_utc],
            )
            .unwrap();
        }
    }

    fn reason(result: Result<InstalledCookies, InstalledCookieImportError>) -> InstalledCookieImportFailureReason {
        result.expect_err("the read must fail").reason
    }

    #[test]
    fn a_missing_profile_or_home_or_cookie_database_is_no_profile() {
        let home = tempfile::tempdir().unwrap();
        let missing = read_installed_browser_cookies(InstalledBrowserKind::Chrome, Some("Nope"), Some(home.path()));
        assert_eq!(reason(missing), InstalledCookieImportFailureReason::NoProfile);
        let no_home = read_installed_browser_cookies(InstalledBrowserKind::Chrome, None, None);
        assert_eq!(reason(no_home), InstalledCookieImportFailureReason::NoProfile);
        chrome_profile(home.path());
        let no_db = read_installed_browser_cookies(InstalledBrowserKind::Chrome, Some("Default"), Some(home.path()));
        assert_eq!(reason(no_db), InstalledCookieImportFailureReason::NoProfile);
        let traversal = read_installed_browser_cookies(InstalledBrowserKind::Edge, Some("../x"), Some(home.path()));
        assert_eq!(reason(traversal), InstalledCookieImportFailureReason::NoProfile);
    }

    #[test]
    fn reasons_are_the_wire_strings_the_settings_ui_maps() {
        let error = failure(InstalledCookieImportFailureReason::KeychainDenied, "denied");
        assert_eq!(error.to_string(), "keychain-denied: denied");
        assert_eq!(InstalledCookieImportFailureReason::NoProfile.as_str(), "no-profile");
        assert_eq!(InstalledCookieImportFailureReason::NoCookies.as_str(), "no-cookies");
        assert_eq!(InstalledCookieImportFailureReason::CookieImportFailed.as_str(), "cookie-import-failed");
        let unreadable = read_failure(&ImportError::Parse("cookies table not found".into()));
        assert_eq!(unreadable.reason, InstalledCookieImportFailureReason::CookieImportFailed);
        let locked_key = read_failure(&ImportError::KeyUnavailable("no keychain".into()));
        assert_eq!(locked_key.reason, InstalledCookieImportFailureReason::KeychainDenied);
    }

    /// An app-bound refusal keeps its own reason and carries the measured HRESULT into the
    /// structured IPC details, so the shell can tell "the browser said no" apart from "no key
    /// was found" and from a generic failure.
    #[test]
    fn an_app_bound_refusal_is_its_own_reason_and_carries_its_hresult() {
        let refused = ImportError::AppBoundKeyRefused { hresult: 0x8004_A007 };
        let mapped = read_failure(&refused);
        assert_eq!(mapped.reason, InstalledCookieImportFailureReason::AppBoundKeyRefused);
        assert_eq!(mapped.reason.as_str(), "app-bound-key-refused");
        assert_eq!(mapped.hresult, Some(0x8004_A007));
        let details = mapped.details();
        assert_eq!(details["reason"], "app-bound-key-refused");
        assert_eq!(details["hresult"], "0x8004A007");
        assert!(mapped.to_string().starts_with("app-bound-key-refused: "), "{mapped}");
        assert!(mapped.to_string().contains("0x8004A007"), "{mapped}");

        // Every other failure keeps one reason string and no HRESULT.
        let no_profile = read_failure(&ImportError::FileNotFound("Cookies".into()));
        assert_eq!(no_profile.reason.as_str(), "no-profile");
        assert_eq!(no_profile.hresult, None);
        assert!(no_profile.details().get("hresult").is_none());
    }

    /// The live path: a profile whose app-bound key the browser refuses still imports its `v10`
    /// cookies, and the refusal is reported with its reason and HRESULT instead of only a count.
    #[cfg(windows)]
    #[test]
    fn a_refused_app_bound_key_reports_the_reason_and_still_imports_v10_cookies() {
        use base64::Engine as _;
        use ferryx_browser_import::decrypt::win_app_bound::{COM_DECRYPT_LOCK, COM_DECRYPT_OVERRIDE};
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};

        fn dpapi_protect(plain: &[u8]) -> Vec<u8> {
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

        fn v10_cookie(key: &[u8], host: &str, value: &str) -> Vec<u8> {
            use aes_gcm::aead::{Aead, KeyInit};
            use aes_gcm::{Aes256Gcm, Nonce};
            use sha2::{Digest, Sha256};
            let iv = [0x41u8; 12];
            let mut plain = Sha256::digest(host.as_bytes()).to_vec();
            plain.extend_from_slice(value.as_bytes());
            let cipher = Aes256Gcm::new_from_slice(key).unwrap();
            let mut blob = b"v10".to_vec();
            blob.extend_from_slice(&iv);
            blob.extend(cipher.encrypt(Nonce::from_slice(&iv), plain.as_slice()).unwrap());
            blob
        }

        let _guard = COM_DECRYPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let key = [0x6Du8; 32];
        let home = tempfile::tempdir().unwrap();
        let profile = chrome_profile(home.path());

        // Local State sits one level above the profile, where the worker looks for it.
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
        std::fs::write(profile.parent().unwrap().join("Local State"), local_state.to_string()).unwrap();

        let network = profile.join("Network");
        std::fs::create_dir_all(&network).unwrap();
        let conn = rusqlite::Connection::open(network.join("Cookies")).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (key TEXT NOT NULL UNIQUE, value TEXT NOT NULL);
             INSERT INTO meta (key, value) VALUES ('version', '24');
             CREATE TABLE cookies (host_key TEXT NOT NULL, name TEXT NOT NULL, value TEXT NOT NULL DEFAULT '',
                 encrypted_value BLOB DEFAULT x'', path TEXT NOT NULL DEFAULT '/', expires_utc INTEGER NOT NULL DEFAULT 0,
                 is_secure INTEGER NOT NULL DEFAULT 0, is_httponly INTEGER NOT NULL DEFAULT 0,
                 samesite INTEGER NOT NULL DEFAULT -1, source_scheme INTEGER NOT NULL DEFAULT 0);",
        )
        .unwrap();
        let expires = (4_000_000_000i64 + 11_644_473_600) * 1_000_000;
        for (host, name, encrypted) in [
            (".v10.test", "v10", v10_cookie(&key, ".v10.test", "v10-value")),
            (".v20.test", "v20", b"v20sealed-ciphertext".to_vec()),
        ] {
            conn.execute(
                "INSERT INTO cookies (host_key, name, value, encrypted_value, expires_utc, is_secure, is_httponly, samesite)
                 VALUES (?1, ?2, '', ?3, ?4, 1, 1, 1)",
                rusqlite::params![host, name, encrypted, expires],
            )
            .unwrap();
        }
        drop(conn);

        *COM_DECRYPT_OVERRIDE.lock().unwrap() = Some(|_browser, _payload| {
            Err(ferryx_browser_import::decrypt::win_app_bound::AppBoundKeyError::Refused {
                hresult: 0x8004_A007,
            })
        });
        let read = read_installed_browser_cookies(InstalledBrowserKind::Chrome, Some("Default"), Some(home.path()));
        *COM_DECRYPT_OVERRIDE.lock().unwrap() = None;

        let read = read.expect("the v10 cookie still imports despite the refusal");
        assert_eq!(read.cookies.len(), 1, "{:?}", read.cookies.iter().map(|c| c.domain.clone()).collect::<Vec<_>>());
        assert_eq!(read.cookies[0].domain.as_deref(), Some(".v10.test"));
        assert_eq!(read.cookies[0].value, "v10-value");
        assert_eq!(read.skipped, 1, "the refused v20 row is counted, not applied");

        let blocked = read.blocked.expect("the refusal is reported alongside the imported rows");
        assert_eq!(blocked.reason.as_str(), "app-bound-key-refused");
        assert_eq!(blocked.hresult, Some(0x8004_A007));
        assert_eq!(blocked.rows, Some(1), "the block names the row it cost");
        assert_eq!(blocked.details()["hresult"], "0x8004A007");
        assert_eq!(blocked.details()["rows"], 1);
    }

    // macOS fetches the browser's Keychain key before reading any row, which would prompt on a
    // developer machine with Chrome installed; the parser and filter below are platform-neutral.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn plaintext_cookies_are_read_through_the_ported_importer_without_google_or_expired_ones() {
        let home = tempfile::tempdir().unwrap();
        let profile = chrome_profile(home.path());
        let future = 4_000_000_000;
        write_cookies(
            &profile,
            &[
                (".github.com", "session", "abc", future),
                (".google.com", "SID", "xyz", future),
                (".old.example", "gone", "1", 1_000_000),
            ],
        );
        let read = read_installed_browser_cookies(InstalledBrowserKind::Chrome, Some("Default"), Some(home.path()))
            .expect("plaintext cookies import");
        assert_eq!(read.cookies.len(), 1);
        assert_eq!(read.cookies[0].domain.as_deref(), Some(".github.com"));
        assert_eq!(read.cookies[0].value, "abc");
        assert!(read.cookies[0].secure && read.cookies[0].http_only);
        assert_eq!(read.cookies[0].same_site.as_deref(), Some("lax"));
        assert_eq!(read.google_excluded, 1);
        assert_eq!(read.skipped, 1);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_profile_with_only_google_cookies_has_no_cookies() {
        let home = tempfile::tempdir().unwrap();
        let profile = chrome_profile(home.path());
        write_cookies(&profile, &[(".google.com", "SID", "xyz", 4_000_000_000)]);
        let result = read_installed_browser_cookies(InstalledBrowserKind::Chrome, Some("Default"), Some(home.path()));
        assert_eq!(reason(result), InstalledCookieImportFailureReason::NoCookies);
    }
}
