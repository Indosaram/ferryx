//! Chromium History SQLite parser.
//!
//! Ports `maho-chromium/browser/importer/chromium_history_parser.cc` to Rust.
//! Reads the `History` SQLite database from a Chromium profile directory and
//! extracts URL visit entries.

use std::collections::HashSet;
use std::path::Path;
use crate::{HistoryEntry, ImportError, ImportResult};

/// Maximum number of history rows to import.
const MAX_HISTORY_ROWS: u32 = 100_000;

/// Chromium epoch offset: microseconds between 1601-01-01 and 1970-01-01.
const WINDOWS_EPOCH_DELTA_MICROSECONDS: i64 = 11_644_473_600_000_000;

/// Converts Chromium's Windows-epoch microseconds to seconds since Unix epoch.
fn webkit_microseconds_to_seconds(webkit_us: i64) -> f64 {
    if webkit_us <= 0 {
        return 0.0;
    }
    (webkit_us - WINDOWS_EPOCH_DELTA_MICROSECONDS) as f64 / 1_000_000.0
}

/// Parses Chromium history from a profile directory.
///
/// The `profile_dir` should contain a file named `History`.
pub fn parse_chromium_history(profile_dir: &Path) -> ImportResult<Vec<HistoryEntry>> {
    let history_path = profile_dir.join("History");

    if !history_path.exists() {
        return Err(ImportError::FileNotFound(
            history_path.display().to_string(),
        ));
    }

    let conn = open_readonly(&history_path)?;

    let mut stmt = conn
        .prepare(
            "SELECT url, title, visit_count, last_visit_time \
             FROM urls \
             WHERE url NOT LIKE 'chrome://%' \
               AND url NOT LIKE 'chrome-extension://%' \
               AND url NOT LIKE 'ferryx://%' \
             ORDER BY last_visit_time DESC \
             LIMIT ?1",
        )
        .map_err(|e| ImportError::Parse(format!("preparing history query: {e}")))?;

    let mut seen_urls = HashSet::new();
    let mut results = Vec::new();

    let rows = stmt
        .query_map([MAX_HISTORY_ROWS], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| ImportError::Parse(format!("querying history: {e}")))?;

    for row in rows {
        let (url, title, visit_count, last_visit_raw) =
            row.map_err(|e| ImportError::Parse(format!("reading history row: {e}")))?;

        if url.is_empty() || seen_urls.contains(&url) {
            continue;
        }

        if url::Url::parse(&url).is_err() {
            continue;
        }

        seen_urls.insert(url.clone());

        results.push(HistoryEntry {
            url,
            title: title.unwrap_or_default(),
            visit_count,
            visit_time: webkit_microseconds_to_seconds(last_visit_raw),
        });
    }

    Ok(results)
}

/// Reads a private copy: the browser may hold the live file open or keep rows in its WAL.
fn open_readonly(path: &Path) -> ImportResult<crate::sqlite_snapshot::Snapshot> {
    crate::sqlite_snapshot::open_snapshot(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn create_test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ferryx-import-test-chromium-history-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn create_history_db(dir: &Path) {
        let db_path = dir.join("History");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE urls (
                id INTEGER PRIMARY KEY,
                url TEXT NOT NULL,
                title TEXT DEFAULT '',
                visit_count INTEGER DEFAULT 0,
                last_visit_time INTEGER DEFAULT 0
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO urls (url, title, visit_count, last_visit_time) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "https://example.com",
                "Example",
                5u32,
                13_350_000_000_000_000i64
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO urls (url, title, visit_count, last_visit_time) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "https://rust-lang.org",
                "Rust",
                3u32,
                13_349_000_000_000_000i64
            ],
        )
        .unwrap();
        // Should be filtered out (chrome:// scheme):
        conn.execute(
            "INSERT INTO urls (url, title, visit_count, last_visit_time) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "chrome://settings",
                "Settings",
                1u32,
                13_348_000_000_000_000i64
            ],
        )
        .unwrap();
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let dir = create_test_dir("missing");
        let err = parse_chromium_history(&dir).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)), "got {err:?}");
    }

    #[test]
    fn corrupted_db_returns_error() {
        let dir = create_test_dir("corrupted");
        std::fs::write(dir.join("History"), b"not a sqlite file").unwrap();
        let err = parse_chromium_history(&dir).unwrap_err();
        assert!(
            matches!(err, ImportError::Io(_) | ImportError::Parse(_)),
            "got {err:?}",
        );
    }

    #[test]
    fn parses_valid_history_db() {
        let dir = create_test_dir("valid");
        create_history_db(&dir);

        let entries = parse_chromium_history(&dir).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "https://example.com");
        assert_eq!(entries[0].title, "Example");
        assert_eq!(entries[0].visit_count, 5);
        assert!(entries[0].visit_time > 0.0);
        assert_eq!(entries[1].url, "https://rust-lang.org");
    }

    #[test]
    fn deduplicates_urls() {
        let dir = create_test_dir("dedup");
        let db_path = dir.join("History");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, last_visit_time INTEGER);",
        ).unwrap();
        conn.execute(
            "INSERT INTO urls (url, title, visit_count, last_visit_time) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["https://dup.com", "Dup1", 1u32, 13_350_000_000_000_000i64],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO urls (url, title, visit_count, last_visit_time) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["https://dup.com", "Dup2", 2u32, 13_349_000_000_000_000i64],
        )
        .unwrap();

        let entries = parse_chromium_history(&dir).unwrap();
        assert_eq!(entries.len(), 1);
    }
}
