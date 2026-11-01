//! Firefox cookie parser — reads `cookies.sqlite` (moz_cookies table).
//!
//! Ports the `ParseFirefoxCookies` function from `cookie_parser.cc`.

use std::path::Path;

use rusqlite::Connection;

use crate::{CookieEntry, ImportError, ImportResult};

pub fn parse_firefox_cookies(profile_dir: &Path) -> ImportResult<Vec<CookieEntry>> {
    let db_path = profile_dir.join("cookies.sqlite");

    if !db_path.exists() {
        return Err(ImportError::FileNotFound(db_path.display().to_string()));
    }

    let conn = open_readonly(&db_path)?;

    if !table_exists(&conn, "moz_cookies") {
        return Err(ImportError::Parse("moz_cookies table not found".into()));
    }

    let mut stmt = conn
        .prepare(
            "SELECT host, name, value, path, expiry, isSecure, isHttpOnly, sameSite \
             FROM moz_cookies WHERE expiry > 0",
        )
        .map_err(|e| ImportError::Parse(format!("prepare: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(CookieEntry {
                host: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                name: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                value: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                path: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                expires: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                is_secure: row.get::<_, Option<bool>>(5)?.unwrap_or(false),
                is_httponly: row.get::<_, Option<bool>>(6)?.unwrap_or(false),
                same_site: row.get::<_, Option<i32>>(7)?.unwrap_or(-1),
            })
        })
        .map_err(|e| ImportError::Parse(format!("query: {e}")))?;

    let mut results = Vec::new();
    for row_result in rows {
        let entry = row_result.map_err(|e| ImportError::Parse(format!("row: {e}")))?;
        results.push(entry);
    }

    Ok(results)
}

/// Reads a private copy: the browser may hold the live file open or keep rows in its WAL.
fn open_readonly(path: &Path) -> ImportResult<crate::sqlite_snapshot::Snapshot> {
    crate::sqlite_snapshot::open_snapshot(path)
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.prepare(&format!(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='{table}'"
    ))
    .and_then(|mut s| s.query_row([], |_| Ok(())))
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::fs;

    fn create_cookies_db(dir: &Path) {
        let db_path = dir.join("cookies.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_cookies (
                 id INTEGER PRIMARY KEY,
                 host TEXT, name TEXT, value TEXT, path TEXT,
                 expiry INTEGER, isSecure INTEGER, isHttpOnly INTEGER, sameSite INTEGER
             );
             INSERT INTO moz_cookies (host, name, value, path, expiry, isSecure, isHttpOnly, sameSite)
                 VALUES ('.example.com', 'session_id', 'abc123', '/', 1750000000, 1, 1, 1);
             INSERT INTO moz_cookies (host, name, value, path, expiry, isSecure, isHttpOnly, sameSite)
                 VALUES ('.test.org', 'pref', 'dark', '/app', 1760000000, 0, 0, 0);
             -- expired cookie (expiry=0) should be excluded
             INSERT INTO moz_cookies (host, name, value, path, expiry, isSecure, isHttpOnly, sameSite)
                 VALUES ('.old.com', 'gone', 'x', '/', 0, 0, 0, -1);",
        )
        .unwrap();
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-cookie-missing");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let err = parse_firefox_cookies(&tmp).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)));
    }

    #[test]
    fn corrupted_db_returns_error() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-cookie-corrupt");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("cookies.sqlite"), b"not a db").unwrap();
        let err = parse_firefox_cookies(&tmp).unwrap_err();
        assert!(
            matches!(err, ImportError::Parse(_) | ImportError::Io(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn missing_table_returns_parse_error() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-cookie-notable");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("cookies.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE other (id INTEGER);")
            .unwrap();
        drop(conn);

        let err = parse_firefox_cookies(&tmp).unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)));
    }

    #[test]
    fn parses_valid_cookies() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-cookie-valid");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        create_cookies_db(&tmp);

        let entries = parse_firefox_cookies(&tmp).unwrap();
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].host, ".example.com");
        assert_eq!(entries[0].name, "session_id");
        assert_eq!(entries[0].value, "abc123");
        assert_eq!(entries[0].path, "/");
        assert_eq!(entries[0].expires, 1750000000);
        assert!(entries[0].is_secure);
        assert!(entries[0].is_httponly);
        assert_eq!(entries[0].same_site, 1);

        assert_eq!(entries[1].host, ".test.org");
        assert!(!entries[1].is_secure);
    }
}
