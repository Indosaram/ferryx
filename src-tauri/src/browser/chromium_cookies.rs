use std::fs;
use std::path::{Path, PathBuf};

use aes::Aes128;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockDecryptMut, KeyIvInit};
use pbkdf2::pbkdf2_hmac;
use rusqlite::OpenFlags;
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::browser::cookies::{is_google_cookie, ImportedCookie};
use crate::browser::BrowserError;

pub const CHROMIUM_OSCRYPT_IV: [u8; 16] = [0x20; 16];
pub const CHROMIUM_OSCRYPT_SALT: &[u8] = b"saltysalt";
pub const CHROMIUM_PBKDF2_ITERATIONS_MACOS: u32 = 1003;
pub const CHROMIUM_PBKDF2_ITERATIONS_LINUX: u32 = 1;
pub const CHROMIUM_LINUX_V10_PASSWORD: &[u8] = b"peanuts";
pub const CHROME_EPOCH_OFFSET_MICROS: i64 = 11_644_473_600 * 1_000_000;

type Aes128CbcDec = cbc::Decryptor<Aes128>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromiumCookieReadResult {
    pub cookies: Vec<ImportedCookie>,
    pub unsupported_v11_count: usize,
    pub decrypt_failed_count: usize,
    pub google_excluded_count: usize,
    pub invalid_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    RawKey([u8; 16]),
    Password {
        password: Vec<u8>,
        iterations: u32,
    },
    None,
}

impl KeySource {
    pub fn resolve_key(&self) -> Option<[u8; 16]> {
        match self {
            KeySource::RawKey(key) => Some(*key),
            KeySource::Password {
                password,
                iterations,
            } => Some(derive_key_aes128(password, *iterations)),
            KeySource::None => None,
        }
    }
}

pub fn derive_key_aes128(password: &[u8], iterations: u32) -> [u8; 16] {
    let mut key = [0u8; 16];
    pbkdf2_hmac::<Sha1>(password, CHROMIUM_OSCRYPT_SALT, iterations, &mut key);
    key
}

pub fn chrome_time_to_unix(expires_utc: i64) -> Option<i64> {
    if expires_utc <= 0 {
        return None;
    }
    let micros_since_unix = expires_utc - CHROME_EPOCH_OFFSET_MICROS;
    if micros_since_unix <= 0 {
        None
    } else {
        Some(micros_since_unix / 1_000_000)
    }
}

pub fn unix_to_chrome_time(unix_seconds: i64) -> i64 {
    (unix_seconds * 1_000_000) + CHROME_EPOCH_OFFSET_MICROS
}

pub fn decrypt_chromium_value(
    encrypted: &[u8],
    key: &[u8; 16],
    expected_host_key: Option<&str>,
) -> Result<String, BrowserError> {
    if encrypted.len() < 3 {
        return Err(BrowserError::CookieImport(
            "encrypted value too short for version prefix".into(),
        ));
    }

    let prefix = &encrypted[..3];
    if prefix != b"v10" && prefix != b"v11" {
        return Err(BrowserError::CookieImport(format!(
            "unsupported encryption version prefix: {:?}",
            String::from_utf8_lossy(prefix)
        )));
    }

    let ciphertext = &encrypted[3..];
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(16) {
        return Err(BrowserError::CookieImport(
            "ciphertext length must be a non-zero multiple of 16 bytes".into(),
        ));
    }

    let mut buffer = ciphertext.to_vec();
    let decryptor = Aes128CbcDec::new(key.into(), &CHROMIUM_OSCRYPT_IV.into());
    let decrypted_slice = decryptor
        .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        .map_err(|_| BrowserError::CookieImport("invalid PKCS#7 padding during decryption".into()))?;

    let payload = if let Some(host_key) = expected_host_key {
        if decrypted_slice.len() < 32 {
            return Err(BrowserError::CookieImport(
                "decrypted plaintext shorter than expected 32-byte host_key hash".into(),
            ));
        }
        let (hash_prefix, actual_plaintext) = decrypted_slice.split_at(32);
        let expected_hash = Sha256::digest(host_key.as_bytes());
        if hash_prefix != expected_hash.as_slice() {
            return Err(BrowserError::CookieImport(
                "decrypted plaintext host_key SHA256 prefix mismatch".into(),
            ));
        }
        actual_plaintext
    } else {
        decrypted_slice
    };

    String::from_utf8(payload.to_vec())
        .map_err(|e| BrowserError::CookieImport(format!("decrypted cookie value is not UTF-8: {e}")))
}

pub fn find_chromium_cookie_db_path(profile_dir: &Path) -> Option<PathBuf> {
    let network_path = profile_dir.join("Network").join("Cookies");
    if network_path.is_file() {
        return Some(network_path);
    }
    let direct_path = profile_dir.join("Cookies");
    if direct_path.is_file() {
        return Some(direct_path);
    }
    None
}

pub fn map_chromium_samesite(samesite: i64) -> Option<String> {
    match samesite {
        1 => Some("lax".to_string()),
        2 => Some("strict".to_string()),
        0 => Some("none".to_string()),
        _ => None,
    }
}

pub fn copy_cookie_db_to_temp(source_db_path: &Path) -> Result<(tempfile::TempDir, PathBuf), BrowserError> {
    let temp_dir = tempfile::Builder::new()
        .prefix("ferryx-cookies-")
        .tempdir()
        .map_err(|e| BrowserError::CookieImport(format!("failed to create temp directory for cookie DB: {e}")))?;

    let file_name = source_db_path
        .file_name()
        .ok_or_else(|| BrowserError::CookieImport("invalid cookie DB file path".into()))?;

    let target_db_path = temp_dir.path().join(file_name);
    fs::copy(source_db_path, &target_db_path).map_err(|e| {
        BrowserError::CookieImport(format!(
            "failed to copy cookie DB from {} to {}: {e}",
            source_db_path.display(),
            target_db_path.display()
        ))
    })?;

    for extension in &["wal", "shm", "journal"] {
        let sidecar_filename = format!("{}-{}", file_name.to_string_lossy(), extension);
        if let Some(parent) = source_db_path.parent() {
            let sidecar_source = parent.join(&sidecar_filename);
            let sidecar_dest = temp_dir.path().join(&sidecar_filename);
            match fs::copy(&sidecar_source, &sidecar_dest) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(BrowserError::CookieImport(format!(
                        "failed to copy cookie sidecar {} to {}: {e}",
                        sidecar_source.display(),
                        sidecar_dest.display()
                    )));
                }
            }
        }
    }

    Ok((temp_dir, target_db_path))
}

pub fn read_chromium_cookie_db(
    db_path: &Path,
    key_source: &KeySource,
) -> Result<ChromiumCookieReadResult, BrowserError> {
    let (_temp_dir, isolated_db_path) = copy_cookie_db_to_temp(db_path)?;

    let conn = rusqlite::Connection::open_with_flags(
        &isolated_db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| BrowserError::CookieImport(format!("failed to open cookie SQLite DB: {e}")))?;

    let meta_version: i64 = match conn.query_row(
        "SELECT value FROM meta WHERE key = 'version'",
        [],
        |row| {
            if let Ok(int_val) = row.get::<_, i64>(0) {
                Ok(int_val)
            } else if let Ok(str_val) = row.get::<_, String>(0) {
                str_val
                    .trim()
                    .parse::<i64>()
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
            } else {
                row.get(0)
            }
        },
    ) {
        Ok(v) => v,
        Err(rusqlite::Error::QueryReturnedNoRows) => 0,
        Err(e) => {
            return Err(BrowserError::CookieImport(format!(
                "failed to parse chromium cookie DB meta version: {e}"
            )));
        }
    };

    let verify_host_hash = meta_version >= 24;
    let resolved_key = key_source.resolve_key();

    let mut stmt = conn
        .prepare(
            "SELECT host_key, name, value, encrypted_value, path, expires_utc, is_secure, is_httponly, samesite FROM cookies",
        )
        .map_err(|e| BrowserError::CookieImport(format!("failed to prepare cookies SELECT query: {e}")))?;

    let mut rows = stmt
        .query([])
        .map_err(|e| BrowserError::CookieImport(format!("failed to execute cookies query: {e}")))?;

    let mut cookies = Vec::new();
    let mut unsupported_v11_count = 0usize;
    let mut decrypt_failed_count = 0usize;
    let mut google_excluded_count = 0usize;
    let mut invalid_count = 0usize;

    while let Some(row) = rows
        .next()
        .map_err(|e| BrowserError::CookieImport(format!("error reading cookies row: {e}")))?
    {
        let host_key: String = row.get(0).unwrap_or_default();
        let name: String = row.get(1).unwrap_or_default();
        let plaintext_value: String = row.get(2).unwrap_or_default();
        let encrypted_value: Vec<u8> = row.get(3).unwrap_or_default();
        let path: String = row.get(4).unwrap_or_else(|_| "/".into());
        let expires_utc: i64 = row.get(5).unwrap_or(0);
        let is_secure: i64 = row.get(6).unwrap_or(0);
        let is_httponly: i64 = row.get(7).unwrap_or(0);
        let samesite: i64 = row.get(8).unwrap_or(-1);

        let clean_name = name.trim().to_string();
        if clean_name.is_empty() {
            invalid_count += 1;
            continue;
        }

        let domain_opt = if host_key.is_empty() {
            None
        } else {
            Some(host_key.clone())
        };

        if is_google_cookie(domain_opt.as_deref(), &clean_name) {
            google_excluded_count += 1;
            continue;
        }

        let value = if !encrypted_value.is_empty() {
            if encrypted_value.starts_with(b"v10") || encrypted_value.starts_with(b"v11") {
                match resolved_key {
                    Some(ref key) => {
                        let expected_host = if verify_host_hash {
                            Some(host_key.as_str())
                        } else {
                            None
                        };
                        match decrypt_chromium_value(&encrypted_value, key, expected_host) {
                            Ok(decrypted) => decrypted,
                            Err(_) => {
                                // `v11` rows are keyed by the OS keyring, not the key this
                                // source holds (Linux libsecret/kwallet), so a failure means
                                // the key is unavailable, not that the data is corrupt.
                                if encrypted_value.starts_with(b"v11") {
                                    unsupported_v11_count += 1;
                                } else {
                                    decrypt_failed_count += 1;
                                }
                                continue;
                            }
                        }
                    }
                    None => {
                        if encrypted_value.starts_with(b"v11") {
                            unsupported_v11_count += 1;
                        } else {
                            decrypt_failed_count += 1;
                        }
                        continue;
                    }
                }
            } else {
                decrypt_failed_count += 1;
                continue;
            }
        } else {
            plaintext_value
        };

        let cookie = ImportedCookie {
            name: clean_name,
            value,
            domain: domain_opt,
            path: if path.starts_with('/') {
                path
            } else {
                format!("/{path}")
            },
            secure: is_secure != 0,
            http_only: is_httponly != 0,
            expires_unix: chrome_time_to_unix(expires_utc),
            same_site: map_chromium_samesite(samesite),
        };

        cookies.push(cookie);
    }

    Ok(ChromiumCookieReadResult {
        cookies,
        unsupported_v11_count,
        decrypt_failed_count,
        google_excluded_count,
        invalid_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::Aes128;
    use cbc::cipher::block_padding::Pkcs7;
    use cbc::cipher::{BlockEncryptMut, KeyIvInit};

    type Aes128CbcEnc = cbc::Encryptor<Aes128>;

    fn synthetic_encrypt(plaintext: &[u8], key: &[u8; 16], prefix: &[u8]) -> Vec<u8> {
        let encryptor = Aes128CbcEnc::new(key.into(), &CHROMIUM_OSCRYPT_IV.into());
        let pad_len = 16 - (plaintext.len() % 16);
        let total_len = plaintext.len() + pad_len;
        let mut buffer = vec![0u8; total_len];
        buffer[..plaintext.len()].copy_from_slice(plaintext);
        let ciphertext_slice = encryptor
            .encrypt_padded_mut::<Pkcs7>(&mut buffer, plaintext.len())
            .expect("encryption failed in test helper");

        let mut res = Vec::with_capacity(prefix.len() + ciphertext_slice.len());
        res.extend_from_slice(prefix);
        res.extend_from_slice(ciphertext_slice);
        res
    }

    #[test]
    fn test_chrome_time_to_unix() {
        assert_eq!(chrome_time_to_unix(0), None);
        assert_eq!(chrome_time_to_unix(-100), None);
        assert_eq!(chrome_time_to_unix(CHROME_EPOCH_OFFSET_MICROS), None);
        assert_eq!(
            chrome_time_to_unix(CHROME_EPOCH_OFFSET_MICROS + 1_000_000),
            Some(1)
        );

        let fixed_chrome_micros = 13_000_000_000_000_000i64;
        let expected_unix = (fixed_chrome_micros - CHROME_EPOCH_OFFSET_MICROS) / 1_000_000;
        assert_eq!(expected_unix, 1_355_526_400);
        assert_eq!(chrome_time_to_unix(fixed_chrome_micros), Some(1_355_526_400));
    }

    #[test]
    fn test_pbkdf2_derivation_deterministic() {
        let key_macos = derive_key_aes128(b"test-password", CHROMIUM_PBKDF2_ITERATIONS_MACOS);
        let key_linux = derive_key_aes128(CHROMIUM_LINUX_V10_PASSWORD, CHROMIUM_PBKDF2_ITERATIONS_LINUX);
        assert_eq!(key_macos.len(), 16);
        assert_eq!(key_linux.len(), 16);
        assert_ne!(key_macos, key_linux);

        assert_eq!(
            key_macos,
            derive_key_aes128(b"test-password", CHROMIUM_PBKDF2_ITERATIONS_MACOS)
        );
    }

    #[test]
    fn test_decrypt_v10_v11_roundtrip() {
        let key = derive_key_aes128(b"secret", 1003);
        let secret_value = "session_token_xyz_12345";

        let encrypted_v10 = synthetic_encrypt(secret_value.as_bytes(), &key, b"v10");
        let decrypted_v10 = decrypt_chromium_value(&encrypted_v10, &key, None)
            .expect("v10 decryption should succeed");
        assert_eq!(decrypted_v10, secret_value);

        let encrypted_v11 = synthetic_encrypt(secret_value.as_bytes(), &key, b"v11");
        let decrypted_v11 = decrypt_chromium_value(&encrypted_v11, &key, None)
            .expect("v11 decryption should succeed");
        assert_eq!(decrypted_v11, secret_value);
    }

    #[test]
    fn test_decrypt_v24_host_key_hash_verification() {
        let key = derive_key_aes128(b"secret", 1003);
        let host_key = ".example.com";
        let secret_value = "cookie_value_v24";

        let mut plaintext = Vec::new();
        let hash = Sha256::digest(host_key.as_bytes());
        plaintext.extend_from_slice(hash.as_slice());
        plaintext.extend_from_slice(secret_value.as_bytes());

        let encrypted = synthetic_encrypt(&plaintext, &key, b"v10");

        let decrypted = decrypt_chromium_value(&encrypted, &key, Some(host_key))
            .expect("v24 decryption with matching host_key should succeed");
        assert_eq!(decrypted, secret_value);

        let err = decrypt_chromium_value(&encrypted, &key, Some(".other.com"))
            .expect_err("v24 decryption with wrong host_key must fail");
        assert!(format!("{err:?}").contains("host_key SHA256 prefix mismatch"));
    }

    #[test]
    fn test_decrypt_bad_padding_returns_structured_error() {
        let key = derive_key_aes128(b"secret", 1003);
        let mut corrupted = synthetic_encrypt(b"some value", &key, b"v10");
        let last_idx = corrupted.len() - 1;
        corrupted[last_idx] ^= 0xFF;

        let err = decrypt_chromium_value(&corrupted, &key, None)
            .expect_err("corrupted padding must return error");
        assert!(format!("{err:?}").contains("invalid PKCS#7 padding"));
    }

    #[test]
    fn test_decrypt_unsupported_prefix_returns_structured_error() {
        let key = [0u8; 16];
        let encrypted = b"v20some_windows_app_bound_data_here";
        let err = decrypt_chromium_value(encrypted, &key, None)
            .expect_err("v20 prefix must return error");
        assert!(format!("{err:?}").contains("unsupported encryption version prefix"));
    }

    #[test]
    fn test_read_chromium_cookie_db_fixture() {
        let temp_dir = tempfile::Builder::new()
            .prefix("test-cookie-db-")
            .tempdir()
            .expect("create tempdir");
        let db_path = temp_dir.path().join("Cookies");

        let conn = rusqlite::Connection::open(&db_path).expect("open sqlite");
        conn.execute(
            "CREATE TABLE meta(key LONGVARCHAR NOT NULL UNIQUE PRIMARY KEY, value LONGVARCHAR);",
            [],
        )
        .expect("create meta");
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('version', '24');",
            [],
        )
        .expect("insert meta version");

        conn.execute(
            "CREATE TABLE cookies(
                host_key TEXT NOT NULL,
                name TEXT NOT NULL,
                value TEXT NOT NULL,
                encrypted_value BLOB NOT NULL,
                path TEXT NOT NULL,
                expires_utc INTEGER NOT NULL,
                is_secure INTEGER NOT NULL,
                is_httponly INTEGER NOT NULL,
                samesite INTEGER NOT NULL
            );",
            [],
        )
        .expect("create cookies table");

        let key = derive_key_aes128(b"testpass", 1003);

        let host1 = ".example.com";
        let name1 = "auth_token";
        let val1 = "secret_session_token_123";
        let mut p1 = Vec::new();
        p1.extend_from_slice(Sha256::digest(host1.as_bytes()).as_slice());
        p1.extend_from_slice(val1.as_bytes());
        let enc1 = synthetic_encrypt(&p1, &key, b"v10");
        let expires1 = unix_to_chrome_time(1_750_000_000);

        conn.execute(
            "INSERT INTO cookies VALUES (?1, ?2, '', ?3, '/app', ?4, 1, 1, 1);",
            rusqlite::params![host1, name1, enc1, expires1],
        )
        .expect("insert row 1");

        let host2 = "example.org";
        let name2 = "preferences";
        let val2 = "theme=dark";
        conn.execute(
            "INSERT INTO cookies VALUES (?1, ?2, ?3, X'', '/', 0, 0, 0, 0);",
            rusqlite::params![host2, name2, val2],
        )
        .expect("insert row 2");

        let host3 = ".google.com";
        let name3 = "SID";
        let val3 = "google_auth_val";
        conn.execute(
            "INSERT INTO cookies VALUES (?1, ?2, ?3, X'', '/', 0, 1, 1, 1);",
            rusqlite::params![host3, name3, val3],
        )
        .expect("insert row 3");

        conn.execute(
            "INSERT INTO cookies VALUES ('.example.com', '', 'empty_name_val', X'', '/', 0, 0, 0, 0);",
            [],
        )
        .expect("insert row 4");

        let res = read_chromium_cookie_db(&db_path, &KeySource::RawKey(key))
            .expect("read_chromium_cookie_db should succeed");

        assert_eq!(res.cookies.len(), 2);
        assert_eq!(res.google_excluded_count, 1);
        assert_eq!(res.invalid_count, 1);
        assert_eq!(res.unsupported_v11_count, 0);
        assert_eq!(res.decrypt_failed_count, 0);

        let c1 = &res.cookies[0];
        assert_eq!(c1.name, name1);
        assert_eq!(c1.value, val1);
        assert_eq!(c1.domain.as_deref(), Some(host1));
        assert_eq!(c1.path, "/app");
        assert!(c1.secure);
        assert!(c1.http_only);
        assert_eq!(c1.expires_unix, Some(1_750_000_000));
        assert_eq!(c1.same_site.as_deref(), Some("lax"));

        let c2 = &res.cookies[1];
        assert_eq!(c2.name, name2);
        assert_eq!(c2.value, val2);
        assert_eq!(c2.domain.as_deref(), Some(host2));
        assert_eq!(c2.path, "/");
        assert!(!c2.secure);
        assert!(!c2.http_only);
        assert_eq!(c2.expires_unix, None);
        assert_eq!(c2.same_site.as_deref(), Some("none"));
    }

    #[test]
    fn test_read_chromium_cookie_db_without_key_counts_unsupported_v11() {
        let temp_dir = tempfile::Builder::new()
            .prefix("test-cookie-db-nokey-")
            .tempdir()
            .expect("create tempdir");
        let db_path = temp_dir.path().join("Cookies");

        let conn = rusqlite::Connection::open(&db_path).expect("open sqlite");
        conn.execute("CREATE TABLE meta(key TEXT, value TEXT);", [])
            .expect("create meta");
        conn.execute(
            "CREATE TABLE cookies(
                host_key TEXT, name TEXT, value TEXT, encrypted_value BLOB, path TEXT,
                expires_utc INTEGER, is_secure INTEGER, is_httponly INTEGER, samesite INTEGER
            );",
            [],
        )
        .expect("create cookies");

        let dummy_v11_cipher = b"v11_sixteen_bytes!";
        conn.execute(
            "INSERT INTO cookies VALUES ('.example.com', 'v11_cookie', '', ?1, '/', 0, 0, 0, 0);",
            rusqlite::params![dummy_v11_cipher.as_slice()],
        )
        .expect("insert v11");

        let res = read_chromium_cookie_db(&db_path, &KeySource::None)
            .expect("read should succeed and tally unsupported");

        assert_eq!(res.cookies.len(), 0);
        assert_eq!(res.unsupported_v11_count, 1);
        assert_eq!(res.decrypt_failed_count, 0);
    }

    #[test]
    fn test_read_chromium_cookie_db_v11_with_wrong_key_counts_unsupported_v11() {
        let temp_dir = tempfile::Builder::new()
            .prefix("test-cookie-db-v11-wrongkey-")
            .tempdir()
            .expect("create tempdir");
        let db_path = temp_dir.path().join("Cookies");
        let conn = rusqlite::Connection::open(&db_path).expect("open sqlite");
        conn.execute("CREATE TABLE meta(key TEXT, value TEXT);", [])
            .expect("create meta");
        conn.execute(
            "CREATE TABLE cookies(
                host_key TEXT, name TEXT, value TEXT, encrypted_value BLOB, path TEXT,
                expires_utc INTEGER, is_secure INTEGER, is_httponly INTEGER, samesite INTEGER
            );",
            [],
        )
        .expect("create cookies");
        // A v11 row this source cannot open (keyring-keyed on Linux) must be tallied as an
        // unsupported v11 row. Malformed ciphertext makes the failure deterministic, where a
        // wrong key could by chance still decrypt to valid padding.
        let peanuts_key = derive_key_aes128(CHROMIUM_LINUX_V10_PASSWORD, 1);
        let v11: &[u8] = b"v11-not-a-block";
        let v10: &[u8] = b"v10-not-a-block";
        conn.execute(
            "INSERT INTO cookies VALUES ('.example.com', 'k11', '', ?1, '/', 0, 0, 0, 0);",
            rusqlite::params![v11],
        )
        .expect("insert v11");
        conn.execute(
            "INSERT INTO cookies VALUES ('.example.com', 'k10', '', ?1, '/', 0, 0, 0, 0);",
            rusqlite::params![v10],
        )
        .expect("insert v10");

        let res = read_chromium_cookie_db(&db_path, &KeySource::RawKey(peanuts_key))
            .expect("read tallies instead of failing");

        assert!(res.cookies.is_empty());
        assert_eq!(res.unsupported_v11_count, 1, "v11 needs the OS keyring key");
        assert_eq!(res.decrypt_failed_count, 1, "a failing v10 row is a decrypt failure");
    }

    #[test]
    fn test_copy_cookie_db_to_temp_sidecar_handling() {
        let temp_dir = tempfile::Builder::new()
            .prefix("test-sidecar-")
            .tempdir()
            .expect("create tempdir");
        let db_path = temp_dir.path().join("Cookies");
        fs::write(&db_path, b"SQLite format 3\0fake_db").expect("write fake db");

        let wal_path = temp_dir.path().join("Cookies-wal");
        fs::write(&wal_path, b"fake_wal_content").expect("write fake wal");

        let (_isolated_dir, isolated_db) = copy_cookie_db_to_temp(&db_path)
            .expect("copy with existing wal should succeed");
        let isolated_wal = isolated_db.parent().expect("parent").join("Cookies-wal");
        assert!(isolated_wal.is_file());
        assert_eq!(
            fs::read(&isolated_wal).expect("read isolated wal"),
            b"fake_wal_content"
        );
    }
}
