//! Chromium `Login Data` SQLite parser with v10 password decryption.
//!
//! Reads the `logins` table from a Chromium profile's `Login Data` file,
//! decrypts each `password_value` blob using the provided AES-128 key, and
//! returns `PasswordEntry` structs.

use std::path::Path;

use crate::decrypt::chromium_keychain::{decrypt_v10, ChromiumKey};
use crate::{ImportError, ImportResult, PasswordEntry};

/// Parses Chromium passwords from a profile directory.
///
/// Copies `Login Data` to a temp file (avoids SQLite locking against a running
/// browser), queries the `logins` table, and decrypts password blobs using the
/// provided key. Rows that fail decryption are silently skipped.
///
/// # Arguments
/// * `profile_dir` — path to the Chromium profile containing `Login Data`
/// * `key` — pre-derived AES-128 key (from [`super::super::decrypt::derive_aes_key`])
pub fn parse_chromium_passwords(
    profile_dir: &Path,
    key: &ChromiumKey,
) -> ImportResult<Vec<PasswordEntry>> {
    parse_chromium_passwords_with(profile_dir, |blob| decrypt_v10(blob, key).ok())
}

/// Like [`parse_chromium_passwords`] with a caller-supplied decryptor (several candidate keys
/// on Linux, AES-GCM on Windows). Rows the decryptor returns `None` for are skipped.
pub fn parse_chromium_passwords_with(
    profile_dir: &Path,
    decrypt: impl Fn(&[u8]) -> Option<String>,
) -> ImportResult<Vec<PasswordEntry>> {
    let login_data_path = profile_dir.join("Login Data");

    if !login_data_path.exists() {
        return Err(ImportError::FileNotFound(
            login_data_path.display().to_string(),
        ));
    }

    // A private copy: a running browser holds Login Data open.
    let conn = crate::sqlite_snapshot::open_snapshot(&login_data_path)?;

    // Verify table exists.
    let has_table: bool = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='logins'")
        .and_then(|mut s| s.query_row([], |_| Ok(true)))
        .unwrap_or(false);

    if !has_table {
        return Err(ImportError::Parse(
            "Login Data missing 'logins' table".into(),
        ));
    }

    let mut stmt = conn
        .prepare(
            "SELECT origin_url, action_url, username_value, password_value \
             FROM logins",
        )
        .map_err(|e| ImportError::Parse(format!("preparing logins query: {e}")))?;

    let mut results = Vec::new();

    let rows = stmt
        .query_map([], |row| {
            let origin: String = row.get(0)?;
            let action: String = row.get(1)?;
            let username: String = row.get(2)?;
            let blob: Vec<u8> = row.get(3)?;
            Ok((origin, action, username, blob))
        })
        .map_err(|e| ImportError::Parse(format!("querying logins: {e}")))?;

    for row in rows {
        let (origin, action, username, blob) =
            row.map_err(|e| ImportError::Parse(format!("reading login row: {e}")))?;

        if blob.is_empty() {
            continue;
        }

        let Some(password) = decrypt(&blob) else {
            continue;
        };

        if password.is_empty() {
            continue;
        }

        results.push(PasswordEntry {
            origin_url: origin,
            action_url: action,
            username,
            password,
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::tempdir;

    /// Helper: create a minimal Login Data SQLite database.
    fn create_test_logins_db(dir: &Path, entries: &[(&str, &str, &str, &[u8])]) {
        let db_path = dir.join("Login Data");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE logins (
                origin_url TEXT NOT NULL,
                action_url TEXT NOT NULL,
                username_value TEXT NOT NULL,
                password_value BLOB NOT NULL
            )",
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "INSERT INTO logins (origin_url, action_url, username_value, password_value) \
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .unwrap();

        for &(origin, action, user, blob) in entries {
            stmt.execute(rusqlite::params![origin, action, user, blob])
                .unwrap();
        }
    }

    /// Known test vector from C++ unit test.
    const TEST_KEY_BYTES: [u8; 16] = [0x42; 16];
    const TEST_CIPHERTEXT: &[u8] = &[
        b'v', b'1', b'0', 0x9c, 0x31, 0x37, 0xcc, 0x50, 0xf8, 0xd5, 0x93, 0x0e, 0xe3, 0x7a, 0x1e,
        0x63, 0x0c, 0xce, 0x75,
    ];

    fn test_key() -> ChromiumKey {
        // Use raw construction via derive_aes_key? No — use known key directly.
        // We replicate the struct here for testing.
        crate::decrypt::chromium_keychain::tests_support::make_key(TEST_KEY_BYTES)
    }

    #[test]
    fn missing_db_returns_file_not_found() {
        let dir = tempdir().unwrap();
        let err = parse_chromium_passwords(dir.path(), &test_key()).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)));
    }

    #[test]
    fn parses_minimal_login_data() {
        let dir = tempdir().unwrap();
        create_test_logins_db(
            dir.path(),
            &[(
                "https://example.com",
                "https://example.com/login",
                "user_0",
                TEST_CIPHERTEXT,
            )],
        );

        let entries = parse_chromium_passwords(dir.path(), &test_key()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].origin_url, "https://example.com");
        assert_eq!(entries[0].username, "user_0");
        assert_eq!(entries[0].password, "hunter2");
    }

    #[test]
    fn skips_undecryptable_rows() {
        let dir = tempdir().unwrap();
        // "v20" prefix + 16 zero bytes — wrong prefix, should be skipped.
        let mut bad_blob_vec = vec![b'v', b'2', b'0'];
        bad_blob_vec.extend_from_slice(&[0x00; 16]);
        let bad_blob: &[u8] = &bad_blob_vec;
        create_test_logins_db(
            dir.path(),
            &[
                (
                    "https://good.com",
                    "https://good.com/login",
                    "alice",
                    TEST_CIPHERTEXT,
                ),
                ("https://bad.com", "https://bad.com/login", "bob", bad_blob),
            ],
        );

        let entries = parse_chromium_passwords(dir.path(), &test_key()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].username, "alice");
    }

    #[test]
    fn empty_db_returns_empty_vec() {
        let dir = tempdir().unwrap();
        create_test_logins_db(dir.path(), &[]);
        let entries = parse_chromium_passwords(dir.path(), &test_key()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn missing_logins_table_returns_parse_error() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("Login Data");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE other (x TEXT)").unwrap();
        drop(conn);

        let err = parse_chromium_passwords(dir.path(), &test_key()).unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)));
    }
}
