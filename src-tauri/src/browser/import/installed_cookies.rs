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
    CookieImportFailed,
}

impl InstalledCookieImportFailureReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoProfile => "no-profile",
            Self::KeychainDenied => "keychain-denied",
            Self::NoCookies => "no-cookies",
            Self::CookieImportFailed => "cookie-import-failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: {message}", .reason.as_str())]
pub struct InstalledCookieImportError {
    pub reason: InstalledCookieImportFailureReason,
    pub message: String,
}

fn failure(reason: InstalledCookieImportFailureReason, message: impl Into<String>) -> InstalledCookieImportError {
    InstalledCookieImportError { reason, message: message.into() }
}

/// The cookies read from one installed profile.
#[derive(Debug, Default)]
pub struct InstalledCookies {
    pub cookies: Vec<ImportedCookie>,
    /// Cookies that could not be decrypted, had expired, or had no name or host.
    pub skipped: usize,
    /// Google cookies, left out on purpose (Google signs the account out when they move).
    pub google_excluded: usize,
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
    let reason = match error {
        ImportError::FileNotFound(_) => InstalledCookieImportFailureReason::NoProfile,
        ImportError::KeyUnavailable(_) => InstalledCookieImportFailureReason::KeychainDenied,
        _ => InstalledCookieImportFailureReason::CookieImportFailed,
    };
    failure(reason, error.to_string())
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
    let needs_sign_in: usize = events
        .try_iter()
        .map(|event| match event {
            ImportProgress::Summary { count, .. } => count as usize,
            _ => 0,
        })
        .sum();

    let mut read = std::mem::take(&mut *collector.read.lock());
    read.skipped += needs_sign_in;
    if read.cookies.is_empty() {
        // Every encrypted cookie failing means the browser's key was not released.
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
