//! Safari password CSV import (`File → Export → Passwords`).
//!
//! ## Why CSV and not direct Keychain read
//!
//! Safari does not persist saved passwords in any store a third-party app can
//! read. They live in the login / iCloud Keychain, and each item carries an
//! access-control list (ACL) that scopes decryption to Safari and the system
//! `AuthenticationServices` stack. The `keychain-access-groups` entitlement only
//! grants an app access to keychain access groups it is *provisioned* for
//! (its own group, or shared App Groups) — it cannot join Apple's private
//! `com.apple.safari` / WebKit access groups. So even a fully entitled Maho
//! build cannot call `SecItemCopyMatching` and get Safari's items back: the
//! item ACL, not the app entitlement, gates access. This is by design and is
//! why every established importer (Firefox, Chrome, KeePass, 1Password) imports
//! Safari passwords from the user-exported CSV instead of the Keychain.
//!
//! The exported CSV is a plain, unencrypted RFC-4180 file with a fixed header:
//!
//! ```text
//! Title,URL,Username,Password,Notes,OTPAuth
//! ```
//!
//! Only `URL`, `Username` and `Password` map onto [`PasswordEntry`]; `Title`,
//! `Notes` and `OTPAuth` have no destination field and are dropped (matching
//! Firefox's Safari migrator, which also drops them).
//!
//! ## Safety
//!
//! - Never `unwrap()`s file contents; every fallible step returns
//!   [`ImportError`].
//! - Password values are never logged, echoed, or included in error strings.

use std::path::Path;

use crate::parsers::csv::parse_csv_records;
use crate::{ImportError, ImportResult, PasswordEntry};

/// Parse a Safari-exported passwords CSV into [`PasswordEntry`] values.
///
/// The `URL` column becomes [`PasswordEntry::origin_url`]; `action_url` is left
/// empty (Safari does not export a form-submit URL). Rows without a usable
/// password are skipped. Per INV-1 an empty result is reported as an error, not
/// `Ok(vec![])`.
///
/// # Arguments
/// * `csv_path` — path to the CSV the user exported from Safari and selected.
pub fn parse_safari_passwords_csv(csv_path: &Path) -> ImportResult<Vec<PasswordEntry>> {
    if !csv_path.exists() {
        return Err(ImportError::FileNotFound(csv_path.display().to_string()));
    }

    let content = std::fs::read_to_string(csv_path)
        .map_err(|e| ImportError::Io(format!("reading passwords CSV: {e}")))?;

    parse_safari_passwords_csv_str(&content)
}

/// Parse Safari passwords CSV from an in-memory string.
///
/// Split out from [`parse_safari_passwords_csv`] so tests exercise the parser
/// without touching the filesystem.
fn parse_safari_passwords_csv_str(content: &str) -> ImportResult<Vec<PasswordEntry>> {
    let records = parse_csv_records(content)?;

    let Some((header, rows)) = records.split_first() else {
        return Err(ImportError::Parse("passwords CSV is empty".into()));
    };

    let columns = ColumnIndices::from_header(header)?;

    let mut results = Vec::new();
    for row in rows {
        // A record produced solely by a trailing newline is empty; skip it.
        if row.iter().all(|f| f.is_empty()) {
            continue;
        }

        let password = columns.get(row, columns.password);
        // A credential with no password is not usable; skip it (also skips the
        // header-only case and stray blank rows).
        if password.is_empty() {
            continue;
        }

        results.push(PasswordEntry {
            origin_url: columns.get(row, columns.url).to_string(),
            action_url: String::new(),
            username: columns.get(row, columns.username).to_string(),
            password: password.to_string(),
        });
    }

    if results.is_empty() {
        // INV-1: a parser that extracts zero records fails loudly.
        return Err(ImportError::Parse(
            "no password entries found in CSV".into(),
        ));
    }

    Ok(results)
}

/// Resolved positions of the columns Maho consumes.
struct ColumnIndices {
    url: usize,
    username: usize,
    password: usize,
}

impl ColumnIndices {
    /// Locate the `URL`, `Username`, and `Password` columns by name
    /// (case-insensitive, whitespace-trimmed). Safari's header is fixed, but
    /// resolving by name tolerates column reordering and stray BOMs.
    fn from_header(header: &[String]) -> ImportResult<Self> {
        let find = |name: &str| {
            header.iter().position(|h| {
                h.trim_start_matches('\u{feff}')
                    .trim()
                    .eq_ignore_ascii_case(name)
            })
        };

        let url = find("URL");
        let username = find("Username");
        let password = find("Password");

        match (url, username, password) {
            (Some(url), Some(username), Some(password)) => Ok(Self {
                url,
                username,
                password,
            }),
            _ => Err(ImportError::Parse(
                "CSV header missing required URL/Username/Password columns".into(),
            )),
        }
    }

    /// Fetch a field by index, returning `""` for short rows (graceful — never
    /// panics on ragged input).
    fn get<'a>(&self, row: &'a [String], idx: usize) -> &'a str {
        row.get(idx).map(String::as_str).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_safari_export() {
        let csv = "Title,URL,Username,Password,Notes,OTPAuth\n\
                   Apple,https://apple.com,user@example.com,hunter2,,\n\
                   GitHub,https://github.com,octocat,s3cr3t,my note,otpauth://totp?secret=ABC\n";
        let entries = parse_safari_passwords_csv_str(csv).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].origin_url, "https://apple.com");
        assert_eq!(entries[0].username, "user@example.com");
        assert_eq!(entries[0].password, "hunter2");
        assert_eq!(entries[0].action_url, "");
        assert_eq!(entries[1].origin_url, "https://github.com");
        assert_eq!(entries[1].username, "octocat");
        assert_eq!(entries[1].password, "s3cr3t");
    }

    #[test]
    fn parses_quoted_fields_with_embedded_commas() {
        let csv = "Title,URL,Username,Password,Notes,OTPAuth\n\
                   \"Bank, N.A.\",\"https://bank.example\",\"a,b@x.com\",\"p,a,ss\",\"note, with comma\",\n";
        let entries = parse_safari_passwords_csv_str(csv).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].origin_url, "https://bank.example");
        assert_eq!(entries[0].username, "a,b@x.com");
        assert_eq!(entries[0].password, "p,a,ss");
    }

    #[test]
    fn parses_escaped_double_quotes() {
        let csv = "Title,URL,Username,Password,Notes,OTPAuth\n\
                   \"Say \"\"hi\"\"\",https://ex.com,user,\"pa\"\"ss\",,\n";
        let entries = parse_safari_passwords_csv_str(csv).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].password, "pa\"ss");
    }

    #[test]
    fn parses_quoted_field_with_embedded_newline() {
        let csv = "Title,URL,Username,Password,Notes,OTPAuth\n\
                   Multi,https://ex.com,user,pw,\"line one\nline two\",\n";
        let entries = parse_safari_passwords_csv_str(csv).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].origin_url, "https://ex.com");
        assert_eq!(entries[0].password, "pw");
    }

    #[test]
    fn handles_crlf_line_endings() {
        let csv = "Title,URL,Username,Password,Notes,OTPAuth\r\n\
                   Apple,https://apple.com,user,pw,,\r\n";
        let entries = parse_safari_passwords_csv_str(csv).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].username, "user");
        assert_eq!(entries[0].password, "pw");
    }

    #[test]
    fn tolerates_bom_and_case_insensitive_reordered_header() {
        let csv = "\u{feff}password,username,url,title\n\
                   pw,user,https://ex.com,Example\n";
        let entries = parse_safari_passwords_csv_str(csv).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].origin_url, "https://ex.com");
        assert_eq!(entries[0].username, "user");
        assert_eq!(entries[0].password, "pw");
    }

    #[test]
    fn skips_rows_without_password() {
        let csv = "Title,URL,Username,Password,Notes,OTPAuth\n\
                   NoPass,https://ex.com,user,,,\n\
                   HasPass,https://ok.com,user2,pw,,\n";
        let entries = parse_safari_passwords_csv_str(csv).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].origin_url, "https://ok.com");
    }

    #[test]
    fn skips_blank_rows() {
        let csv = "Title,URL,Username,Password,Notes,OTPAuth\n\
                   \n\
                   Apple,https://apple.com,user,pw,,\n\
                   \n";
        let entries = parse_safari_passwords_csv_str(csv).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn tolerates_ragged_short_rows() {
        // Row shorter than the header must not panic; missing cols read as "".
        let csv = "Title,URL,Username,Password,Notes,OTPAuth\n\
                   Apple,https://apple.com,user,pw\n";
        let entries = parse_safari_passwords_csv_str(csv).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].password, "pw");
    }

    #[test]
    fn empty_content_is_error() {
        let err = parse_safari_passwords_csv_str("").unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)));
    }

    #[test]
    fn header_only_is_error() {
        let csv = "Title,URL,Username,Password,Notes,OTPAuth\n";
        let err = parse_safari_passwords_csv_str(csv).unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)));
    }

    #[test]
    fn missing_required_columns_is_error() {
        // Malformed header lacking Password.
        let csv = "Title,URL,Username,Notes\n\
                   Apple,https://apple.com,user,note\n";
        let err = parse_safari_passwords_csv_str(csv).unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)));
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.csv");
        let err = parse_safari_passwords_csv(&path).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)));
    }

    #[test]
    fn reads_from_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("safari-passwords.csv");
        std::fs::write(
            &path,
            "Title,URL,Username,Password,Notes,OTPAuth\n\
             Apple,https://apple.com,user,pw,,\n",
        )
        .unwrap();
        let entries = parse_safari_passwords_csv(&path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].origin_url, "https://apple.com");
    }

    #[test]
    fn error_messages_never_leak_password_values() {
        // A malformed body still must not echo any field values.
        let csv = "Title,URL,Username,Notes\nx,y,z,supersecretvalue\n";
        let err = parse_safari_passwords_csv_str(csv).unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("supersecretvalue"));
    }
}
