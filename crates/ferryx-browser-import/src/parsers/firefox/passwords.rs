//! Firefox `logins.json` parser with NSS password decryption.

use std::path::Path;

use serde::Deserialize;

use crate::decrypt::firefox_nss::{decrypt_nss_value, unlock_nss_master_key, NssError};
use crate::{ImportError, ImportResult, PasswordEntry};

#[derive(Deserialize)]
struct LoginsFile {
    logins: Vec<LoginEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginEntry {
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    form_submit_url: Option<String>,
    #[serde(default)]
    encrypted_username: String,
    #[serde(default)]
    encrypted_password: String,
}

/// Parses and decrypts Firefox passwords from a profile directory.
///
/// Reads `key4.db` to extract the master decryption key, then decrypts
/// each entry in `logins.json`. Entries that fail individual decryption
/// are silently skipped.
pub fn parse_firefox_passwords(
    profile_dir: &Path,
    master_password: &str,
) -> ImportResult<Vec<PasswordEntry>> {
    let logins_path = profile_dir.join("logins.json");
    if !logins_path.exists() {
        return Err(ImportError::FileNotFound(logins_path.display().to_string()));
    }

    let master_key = unlock_nss_master_key(profile_dir, master_password).map_err(|e| match e {
        NssError::KeyDbMissing => ImportError::FileNotFound("key4.db".into()),
        NssError::WrongMasterPassword => ImportError::Parse("master password incorrect".into()),
        NssError::UnsupportedVersion => ImportError::UnsupportedScheme(e.to_string()),
        _ => ImportError::Parse(e.to_string()),
    })?;

    let json_bytes = std::fs::read_to_string(&logins_path)
        .map_err(|e| ImportError::Io(format!("reading logins.json: {e}")))?;

    let logins_file: LoginsFile = serde_json::from_str(&json_bytes)
        .map_err(|e| ImportError::Parse(format!("logins.json: {e}")))?;

    let mut results = Vec::new();

    for entry in &logins_file.logins {
        if entry.encrypted_username.is_empty() && entry.encrypted_password.is_empty() {
            continue;
        }

        let username = match decrypt_nss_value(&entry.encrypted_username, &master_key) {
            Ok(u) => u,
            Err(_) => continue, // Skip entries that fail decryption (non-fatal).
        };

        let password = match decrypt_nss_value(&entry.encrypted_password, &master_key) {
            Ok(p) => p,
            Err(_) => continue,
        };

        results.push(PasswordEntry {
            origin_url: entry.hostname.clone(),
            action_url: entry.form_submit_url.clone().unwrap_or_default(),
            username,
            password,
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decrypt::firefox_nss::tests_support::create_test_profile;

    #[test]
    fn missing_logins_json_returns_file_not_found() {
        let temp = tempfile::tempdir().unwrap();
        // Create key4.db but no logins.json.
        std::fs::File::create(temp.path().join("key4.db")).unwrap();
        let err = parse_firefox_passwords(temp.path(), "").unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)));
    }

    #[test]
    fn parses_valid_fixture() {
        let (_temp, profile, _master_key) =
            create_test_profile("", &["user1", "pass1", "https://example.com"]);
        let entries = parse_firefox_passwords(&profile, "").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].username, "user1");
        assert_eq!(entries[0].password, "pass1");
        assert_eq!(entries[0].origin_url, "https://example.com");
    }

    #[test]
    fn wrong_master_password_returns_error() {
        let (_temp, profile, _) = create_test_profile("correctpw", &["u", "p", "https://x.com"]);
        let err = parse_firefox_passwords(&profile, "wrongpw").unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)));
    }
}
