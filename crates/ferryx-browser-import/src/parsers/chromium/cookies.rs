//! Chromium Cookies SQLite parser.
//!
//! Ports `maho-chromium/browser/importer/cookie_parser.cc` to Rust.
//! Reads the `Cookies` SQLite database from a Chromium profile directory.
//!
//! NOTE: Cookie decryption is NOT implemented in Phase 1 (Phase 3 — macOS
//! Keychain). Encrypted cookies are returned with the raw encrypted_value
//! in a chromium-specific wrapper. Per INV-5, we check the 3-byte prefix
//! (`v10`/`v20`) before considering decryption.

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{CookieEntry, ImportError, ImportResult};

/// Chromium epoch offset: microseconds between 1601-01-01 and 1970-01-01.
const WINDOWS_EPOCH_DELTA_MICROSECONDS: i64 = 11_644_473_600_000_000;

/// Encryption prefix detected on the cookie value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncryptionVersion {
    /// No encryption — plaintext value available.
    None,
    /// `v10` prefix — AES-128-CBC (macOS Keychain key).
    V10,
    /// `v20` prefix — App-Bound encryption (Windows).
    V20,
    /// Unknown prefix — cannot decrypt.
    Unknown,
}

/// Chromium-specific cookie with encryption metadata.
/// Wraps the shared `CookieEntry` plus encrypted value info.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChromiumCookieEntry {
    /// Standard cookie fields.
    pub cookie: CookieEntry,
    /// Raw encrypted bytes (empty if cookie was plaintext).
    pub encrypted_value: Vec<u8>,
    /// Detected encryption version from the 3-byte prefix.
    pub encryption_version: EncryptionVersion,
    /// Source database schema version; version 24 introduced host binding.
    #[serde(default)]
    pub database_version: i64,
}

fn chromium_time_to_unix_seconds(chromium_time: i64) -> i64 {
    if chromium_time <= 0 {
        return 0;
    }
    // Chromium stores microseconds since Windows epoch; convert to seconds since Unix epoch.
    (chromium_time - WINDOWS_EPOCH_DELTA_MICROSECONDS) / 1_000_000
}

/// Checks the 3-byte prefix of an encrypted cookie value (INV-5).
pub fn detect_encryption_version(data: &[u8]) -> EncryptionVersion {
    if data.len() < 3 {
        return EncryptionVersion::Unknown;
    }
    // `v11` is Linux's keyring-keyed variant of the same AES-128-CBC scheme.
    if data[0] == b'v' && data[1] == b'1' && (data[2] == b'0' || data[2] == b'1') {
        EncryptionVersion::V10
    } else if data[0] == b'v' && data[1] == b'2' && data[2] == b'0' {
        EncryptionVersion::V20
    } else {
        EncryptionVersion::Unknown
    }
}

/// Parses Chromium cookies from a profile directory.
///
/// The `profile_dir` should contain a file named `Cookies`.
///
/// Returns cookies with metadata. Encrypted cookies have `value` empty and
/// `encrypted_value` populated. Decryption is deferred to Phase 3.
pub fn parse_chromium_cookies(profile_dir: &Path) -> ImportResult<Vec<ChromiumCookieEntry>> {
    // Chromium 96+ keeps the database in `Network/`; older profiles and fixtures at the top.
    let network_path = profile_dir.join("Network").join("Cookies");
    let cookies_path = if network_path.is_file() { network_path } else { profile_dir.join("Cookies") };

    if !cookies_path.exists() {
        return Err(ImportError::FileNotFound(
            cookies_path.display().to_string(),
        ));
    }

    let conn = open_readonly(&cookies_path)?;

    if !table_exists(&conn, "cookies") {
        return Err(ImportError::Parse("cookies table not found".into()));
    }

    let database_version = if table_exists(&conn, "meta") {
        conn.query_row("SELECT value FROM meta WHERE key = 'version'", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| ImportError::Parse(format!("reading cookie database version: {e}")))?
        .parse::<i64>()
        .map_err(|e| ImportError::Parse(format!("invalid cookie database version: {e}")))?
    } else {
        0 // Legacy exports and fixtures without a meta table have no host binding.
    };

    let mut stmt = conn
        .prepare(
            "SELECT host_key, name, value, encrypted_value, path, \
             expires_utc, is_secure, is_httponly, samesite, source_scheme \
             FROM cookies WHERE expires_utc > 0",
        )
        .map_err(|e| ImportError::Parse(format!("preparing cookies query: {e}")))?;

    let mut results = Vec::new();

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Vec<u8>>(3).unwrap_or_default(),
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, bool>(6)?,
                row.get::<_, bool>(7)?,
                row.get::<_, i32>(8)?,
                row.get::<_, i32>(9)?,
            ))
        })
        .map_err(|e| ImportError::Parse(format!("querying cookies: {e}")))?;

    for row in rows {
        let (
            host_key,
            name,
            plaintext_value,
            encrypted_value,
            path,
            expires_utc_raw,
            is_secure,
            is_httponly,
            samesite,
            _source_scheme,
        ) = row.map_err(|e| ImportError::Parse(format!("reading cookie row: {e}")))?;

        let (value, encrypted_bytes, encryption_version) = if encrypted_value.is_empty() {
            (plaintext_value, Vec::new(), EncryptionVersion::None)
        } else {
            // INV-5: Check 3-byte prefix before considering decryption
            let version = detect_encryption_version(&encrypted_value);
            // Phase 1: Do NOT decrypt. Return raw encrypted bytes + empty value.
            // TODO(Phase 3): Implement decryption via macOS Keychain for V10.
            (String::new(), encrypted_value, version)
        };

        results.push(ChromiumCookieEntry {
            cookie: CookieEntry {
                host: host_key,
                name,
                value,
                path,
                expires: chromium_time_to_unix_seconds(expires_utc_raw),
                is_secure,
                is_httponly,
                same_site: samesite,
            },
            encrypted_value: encrypted_bytes,
            encryption_version,
            database_version,
        });
    }

    Ok(results)
}

/// Reads a private copy: the browser may hold the live file open or keep rows in its WAL.
fn open_readonly(path: &Path) -> ImportResult<crate::sqlite_snapshot::Snapshot> {
    crate::sqlite_snapshot::open_snapshot(path)
}

fn table_exists(conn: &Connection, table_name: &str) -> bool {
    conn.prepare(&format!(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='{table_name}'"
    ))
    .and_then(|mut stmt| stmt.query_row([], |_| Ok(())))
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ferryx-import-test-chromium-cookies-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn create_cookies_db(dir: &Path) {
        let db_path = dir.join("Cookies");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE cookies (
                host_key TEXT NOT NULL,
                name TEXT NOT NULL,
                value TEXT NOT NULL DEFAULT '',
                encrypted_value BLOB DEFAULT x'',
                path TEXT NOT NULL DEFAULT '/',
                expires_utc INTEGER NOT NULL DEFAULT 0,
                is_secure INTEGER NOT NULL DEFAULT 0,
                is_httponly INTEGER NOT NULL DEFAULT 0,
                samesite INTEGER NOT NULL DEFAULT -1,
                source_scheme INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();

        // Plaintext cookie
        conn.execute(
            "INSERT INTO cookies (host_key, name, value, encrypted_value, path, expires_utc, is_secure, is_httponly, samesite, source_scheme) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                ".example.com",
                "session",
                "abc123",
                Vec::<u8>::new(),
                "/",
                13_400_000_000_000_000i64,
                1i32,
                1i32,
                1i32,
                2i32,
            ],
        ).unwrap();

        // Encrypted cookie with v10 prefix
        let mut v10_data = b"v10".to_vec();
        v10_data.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        conn.execute(
            "INSERT INTO cookies (host_key, name, value, encrypted_value, path, expires_utc, is_secure, is_httponly, samesite, source_scheme) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                ".secure.com",
                "token",
                "",
                v10_data,
                "/api",
                13_400_000_000_000_000i64,
                1i32,
                0i32,
                0i32,
                2i32,
            ],
        ).unwrap();

        // Expired cookie (expires_utc = 0, filtered by query)
        conn.execute(
            "INSERT INTO cookies (host_key, name, value, encrypted_value, path, expires_utc, is_secure, is_httponly, samesite, source_scheme) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                ".old.com", "expired", "val", Vec::<u8>::new(), "/", 0i64, 0i32, 0i32, -1i32, 0i32,
            ],
        ).unwrap();
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let dir = create_test_dir("missing");
        let err = parse_chromium_cookies(&dir).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)), "got {err:?}");
    }

    #[test]
    fn corrupted_db_returns_error() {
        let dir = create_test_dir("corrupted");
        std::fs::write(dir.join("Cookies"), b"not a sqlite database").unwrap();
        let err = parse_chromium_cookies(&dir).unwrap_err();
        assert!(
            matches!(err, ImportError::Io(_) | ImportError::Parse(_)),
            "got {err:?}",
        );
    }

    #[test]
    fn parses_valid_cookies_db() {
        let dir = create_test_dir("valid");
        create_cookies_db(&dir);

        let entries = parse_chromium_cookies(&dir).unwrap();
        // 2 entries (expired one filtered by SQL WHERE)
        assert_eq!(entries.len(), 2);

        // Plaintext cookie
        let plain = &entries[0];
        assert_eq!(plain.cookie.host, ".example.com");
        assert_eq!(plain.cookie.name, "session");
        assert_eq!(plain.cookie.value, "abc123");
        assert!(plain.encrypted_value.is_empty());
        assert_eq!(plain.encryption_version, EncryptionVersion::None);
        assert!(plain.cookie.is_secure);
        assert!(plain.cookie.is_httponly);
        assert_eq!(plain.cookie.same_site, 1); // Lax

        // Encrypted cookie
        let enc = &entries[1];
        assert_eq!(enc.cookie.host, ".secure.com");
        assert_eq!(enc.cookie.name, "token");
        assert!(enc.cookie.value.is_empty()); // Not decrypted in Phase 1
        assert!(!enc.encrypted_value.is_empty());
        assert_eq!(enc.encryption_version, EncryptionVersion::V10);
    }

    #[test]
    fn review_cookie_parser_carries_database_version() {
        let dir = tempfile::tempdir().unwrap();
        create_cookies_db(dir.path());
        {
            let conn = Connection::open(dir.path().join("Cookies")).unwrap();
            conn.execute_batch("CREATE TABLE meta(key TEXT, value TEXT); INSERT INTO meta VALUES ('version', '24');").unwrap();
        }
        let entries = parse_chromium_cookies(dir.path()).unwrap();
        assert!(entries.iter().all(|entry| entry.database_version == 24));
    }

    #[test]
    fn detects_v20_prefix() {
        let data = b"v20\x01\x02\x03\x04";
        assert_eq!(detect_encryption_version(data), EncryptionVersion::V20);
    }

    #[test]
    fn detects_unknown_prefix() {
        let data = b"xxx\x01\x02\x03";
        assert_eq!(detect_encryption_version(data), EncryptionVersion::Unknown);
    }

    #[test]
    fn short_blob_returns_unknown() {
        let data = b"v1";
        assert_eq!(detect_encryption_version(data), EncryptionVersion::Unknown);
    }
}
