use std::path::{Path, PathBuf};

use crate::browser::chromium_cookies::{
    find_chromium_cookie_db_path, read_chromium_cookie_db, ChromiumCookieReadResult, KeySource,
};
#[cfg(target_os = "linux")]
use crate::browser::chromium_cookies::{
    CHROMIUM_LINUX_V10_PASSWORD, CHROMIUM_PBKDF2_ITERATIONS_LINUX,
};
#[cfg(target_os = "macos")]
use crate::browser::chromium_cookies::CHROMIUM_PBKDF2_ITERATIONS_MACOS;
use crate::browser::cookies::{
    resolve_installed_browser_profile_dir, InstalledBrowserKind,
};
use crate::browser::BrowserError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstalledCookieImportFailureReason {
    NoProfile,
    KeychainDenied,
    NoCookies,
    UnsupportedPlatform,
    CookieImportFailed,
}

impl InstalledCookieImportFailureReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoProfile => "no-profile",
            Self::KeychainDenied => "keychain-denied",
            Self::NoCookies => "no-cookies",
            Self::UnsupportedPlatform => "unsupported-platform",
            Self::CookieImportFailed => "cookie-import-failed",
        }
    }
}

impl std::fmt::Display for InstalledCookieImportFailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledCookieImportError {
    pub reason: InstalledCookieImportFailureReason,
    pub message: String,
}

impl InstalledCookieImportError {
    pub fn new(reason: InstalledCookieImportFailureReason, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
        }
    }

    pub fn reason(&self) -> InstalledCookieImportFailureReason {
        self.reason
    }
}

impl std::fmt::Display for InstalledCookieImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.reason.as_str(), self.message)
    }
}

impl std::error::Error for InstalledCookieImportError {}

impl From<InstalledCookieImportError> for BrowserError {
    fn from(err: InstalledCookieImportError) -> Self {
        BrowserError::CookieImport(err.to_string())
    }
}

pub fn cookie_import_error(
    reason: InstalledCookieImportFailureReason,
    message: impl Into<String>,
) -> InstalledCookieImportError {
    InstalledCookieImportError::new(reason, message)
}

pub fn resolve_platform_key_source(
    kind: InstalledBrowserKind,
) -> Result<KeySource, InstalledCookieImportError> {
    #[cfg(target_os = "macos")]
    {
        resolve_macos_keychain_key_source(kind)
    }

    #[cfg(target_os = "linux")]
    {
        let _ = kind;
        Ok(KeySource::Password {
            password: CHROMIUM_LINUX_V10_PASSWORD.to_vec(),
            iterations: CHROMIUM_PBKDF2_ITERATIONS_LINUX,
        })
    }

    #[cfg(target_os = "windows")]
    {
        let _ = kind;
        Err(cookie_import_error(
            InstalledCookieImportFailureReason::UnsupportedPlatform,
            "Windows Chrome 127+ uses app-bound encryption (v20) which cannot be imported by external applications",
        ))
    }

    #[cfg(all(
        not(target_os = "macos"),
        not(target_os = "linux"),
        not(target_os = "windows")
    ))]
    {
        let _ = kind;
        Err(cookie_import_error(
            InstalledCookieImportFailureReason::UnsupportedPlatform,
            "installed profile cookie import is not supported on this platform",
        ))
    }
}

#[cfg(target_os = "macos")]
fn resolve_macos_keychain_key_source(
    kind: InstalledBrowserKind,
) -> Result<KeySource, InstalledCookieImportError> {
    let (service, account) = match kind {
        InstalledBrowserKind::Chrome => ("Chrome Safe Storage", "Chrome"),
        InstalledBrowserKind::Edge => ("Microsoft Edge Safe Storage", "Microsoft Edge"),
    };

    let password = security_framework::passwords::get_generic_password(service, account)
        .map_err(|err| {
            cookie_import_error(
                InstalledCookieImportFailureReason::KeychainDenied,
                format!("Keychain access for {service} failed: {err}"),
            )
        })?;

    Ok(KeySource::Password {
        password,
        iterations: CHROMIUM_PBKDF2_ITERATIONS_MACOS,
    })
}

pub fn import_installed_browser_cookies_sync(
    kind: InstalledBrowserKind,
    profile_name: Option<&str>,
    home_dir: Option<&Path>,
) -> Result<ChromiumCookieReadResult, InstalledCookieImportError> {
    let profile_dir: PathBuf = resolve_installed_browser_profile_dir(kind, profile_name, home_dir)
        .map_err(|err| {
            cookie_import_error(InstalledCookieImportFailureReason::NoProfile, err.to_string())
        })?;
    if !profile_dir.exists() {
        return Err(cookie_import_error(
            InstalledCookieImportFailureReason::NoProfile,
            format!("profile directory does not exist at {}", profile_dir.display()),
        ));
    }

    let cookie_db_path = find_chromium_cookie_db_path(&profile_dir).ok_or_else(|| {
        cookie_import_error(
            InstalledCookieImportFailureReason::NoProfile,
            format!(
                "cookie database (Cookies) not found in profile directory {}",
                profile_dir.display()
            ),
        )
    })?;

    let key_source = resolve_platform_key_source(kind)?;
    let result = read_chromium_cookie_db(&cookie_db_path, &key_source).map_err(|err| {
        cookie_import_error(
            InstalledCookieImportFailureReason::CookieImportFailed,
            err.to_string(),
        )
    })?;

    finalize_installed_cookie_read(result)
}

/// Applies the post-read acceptance rules to a profile database read. Google exclusion is owned
/// by `read_chromium_cookie_db`, which already drops and counts those rows.
fn finalize_installed_cookie_read(
    result: ChromiumCookieReadResult,
) -> Result<ChromiumCookieReadResult, InstalledCookieImportError> {
    if result.cookies.is_empty() {
        if result.unsupported_v11_count > 0
            && result.decrypt_failed_count == 0
            && result.google_excluded_count == 0
        {
            return Err(cookie_import_error(
                InstalledCookieImportFailureReason::NoCookies,
                format!(
                    "no cookies imported: all {} cookies use unsupported v11 encryption",
                    result.unsupported_v11_count
                ),
            ));
        }
        return Err(cookie_import_error(
            InstalledCookieImportFailureReason::NoCookies,
            "no valid non-Google cookies found in profile database",
        ));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::cookies::{filter_google_cookies, ImportedCookie};
    use tempfile::Builder;

    #[test]
    fn test_finalize_does_not_recount_google_exclusions_from_reader() {
        // The reader already excluded one Google row and counted it. A Google-domain cookie that
        // reaches finalize (it cannot in production) must not be filtered or counted a second time.
        let read = ChromiumCookieReadResult {
            cookies: vec![
                ImportedCookie {
                    name: "session".into(),
                    value: "abc".into(),
                    domain: Some(".github.com".into()),
                    path: "/".into(),
                    secure: true,
                    http_only: true,
                    expires_unix: None,
                    same_site: None,
                },
                ImportedCookie {
                    name: "SID".into(),
                    value: "xyz".into(),
                    domain: Some(".google.com".into()),
                    path: "/".into(),
                    secure: true,
                    http_only: true,
                    expires_unix: None,
                    same_site: None,
                },
            ],
            unsupported_v11_count: 0,
            decrypt_failed_count: 0,
            google_excluded_count: 1,
            invalid_count: 0,
        };

        let finalized = finalize_installed_cookie_read(read).expect("non-empty read is accepted");
        assert_eq!(finalized.google_excluded_count, 1);
        assert_eq!(finalized.cookies.len(), 2);
    }

    #[test]
    fn test_failure_reason_as_str() {
        assert_eq!(
            InstalledCookieImportFailureReason::NoProfile.as_str(),
            "no-profile"
        );
        assert_eq!(
            InstalledCookieImportFailureReason::KeychainDenied.as_str(),
            "keychain-denied"
        );
        assert_eq!(
            InstalledCookieImportFailureReason::NoCookies.as_str(),
            "no-cookies"
        );
        assert_eq!(
            InstalledCookieImportFailureReason::UnsupportedPlatform.as_str(),
            "unsupported-platform"
        );
        assert_eq!(
            InstalledCookieImportFailureReason::CookieImportFailed.as_str(),
            "cookie-import-failed"
        );
    }

    #[test]
    fn test_cookie_import_error_formatting() {
        let err = cookie_import_error(
            InstalledCookieImportFailureReason::NoProfile,
            "directory missing",
        );
        let msg = err.to_string();
        assert!(msg.contains("no-profile: directory missing"));
        assert_eq!(err.reason(), InstalledCookieImportFailureReason::NoProfile);
        let browser_err: BrowserError = err.into();
        assert!(browser_err.to_string().contains("no-profile: directory missing"));
    }

    #[test]
    fn test_import_nonexistent_profile_returns_no_profile() {
        let temp_dir = Builder::new().prefix("test-fake-home-").tempdir().unwrap();
        let fake_home = temp_dir.path();

        let err = import_installed_browser_cookies_sync(
            InstalledBrowserKind::Chrome,
            Some("NonExistentProfile"),
            Some(fake_home),
        )
        .expect_err("should fail when profile directory does not exist");

        assert_eq!(err.reason(), InstalledCookieImportFailureReason::NoProfile);
        let msg = err.to_string();
        assert!(
            msg.contains("no-profile"),
            "expected no-profile failure reason, got: {msg}"
        );
    }

    #[test]
    fn test_import_without_home_dir_refuses_instead_of_using_literal_tilde() {
        let err = import_installed_browser_cookies_sync(InstalledBrowserKind::Chrome, None, None)
            .expect_err("unknown home directory must be refused");

        assert_eq!(err.reason(), InstalledCookieImportFailureReason::NoProfile);
        assert!(
            err.message.contains("home directory unknown"),
            "expected home-directory refusal, got: {err}"
        );
    }

    #[test]
    fn test_import_missing_cookie_db_returns_no_profile() {
        let temp_dir = Builder::new().prefix("test-fake-home-").tempdir().unwrap();
        let fake_home = temp_dir.path();

        let profile_dir = resolve_installed_browser_profile_dir(
            InstalledBrowserKind::Chrome,
            Some("Default"),
            Some(fake_home),
        )
        .expect("home directory is provided");
        std::fs::create_dir_all(&profile_dir).unwrap();

        let err = import_installed_browser_cookies_sync(
            InstalledBrowserKind::Chrome,
            Some("Default"),
            Some(fake_home),
        )
        .expect_err("should fail when cookie db is missing");

        assert_eq!(err.reason(), InstalledCookieImportFailureReason::NoProfile);
        let msg = err.to_string();
        assert!(
            msg.contains("no-profile"),
            "expected no-profile failure reason when db missing, got: {msg}"
        );
    }

    #[test]
    fn test_import_empty_cookie_db_returns_no_cookies() {
        #[cfg(target_os = "linux")]
        {
            use rusqlite::Connection;
            let temp_dir = Builder::new().prefix("test-fake-home-").tempdir().unwrap();
            let fake_home = temp_dir.path();

            let profile_dir = resolve_installed_browser_profile_dir(
                InstalledBrowserKind::Chrome,
                Some("Default"),
                Some(fake_home),
            )
            .expect("home directory is provided");
            std::fs::create_dir_all(&profile_dir).unwrap();
            let db_path = profile_dir.join("Cookies");

            let conn = Connection::open(&db_path).unwrap();
            conn.execute("CREATE TABLE meta(key TEXT, value TEXT);", [])
                .unwrap();
            conn.execute(
                "CREATE TABLE cookies(
                    host_key TEXT, name TEXT, value TEXT, encrypted_value BLOB, path TEXT,
                    expires_utc INTEGER, is_secure INTEGER, is_httponly INTEGER, samesite INTEGER
                );",
                [],
            )
            .unwrap();

            let err = import_installed_browser_cookies_sync(
                InstalledBrowserKind::Chrome,
                Some("Default"),
                Some(fake_home),
            )
            .expect_err("should return no-cookies for empty db");

            assert_eq!(err.reason(), InstalledCookieImportFailureReason::NoCookies);
            assert!(err.to_string().contains("no-cookies"));
        }
    }

    #[test]
    fn test_filter_google_cookies_in_installed_import() {
        let cookies = vec![
            ImportedCookie {
                name: "session".into(),
                value: "abc".into(),
                domain: Some(".github.com".into()),
                path: "/".into(),
                secure: true,
                http_only: true,
                expires_unix: None,
                same_site: None,
            },
            ImportedCookie {
                name: "SID".into(),
                value: "xyz".into(),
                domain: Some(".google.com".into()),
                path: "/".into(),
                secure: true,
                http_only: true,
                expires_unix: None,
                same_site: None,
            },
        ];

        let filtered = filter_google_cookies(cookies);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].domain.as_deref(), Some(".github.com"));
    }

    #[test]
    fn test_platform_refusal_on_windows_or_synthetic_key() {
        #[cfg(target_os = "windows")]
        {
            let res = resolve_platform_key_source(InstalledBrowserKind::Chrome);
            let err = res.expect_err("Windows must return structured refusal");
            assert_eq!(err.reason(), InstalledCookieImportFailureReason::UnsupportedPlatform);
            assert!(err.to_string().contains("unsupported-platform"));
        }

        #[cfg(target_os = "linux")]
        {
            let res = resolve_platform_key_source(InstalledBrowserKind::Chrome).unwrap();
            assert_eq!(
                res,
                KeySource::Password {
                    password: CHROMIUM_LINUX_V10_PASSWORD.to_vec(),
                    iterations: CHROMIUM_PBKDF2_ITERATIONS_LINUX,
                }
            );
        }
    }
}

