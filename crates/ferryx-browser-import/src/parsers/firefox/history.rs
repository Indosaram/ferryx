//! Firefox history parser — reads `places.sqlite` (moz_historyvisits + moz_places).
//!
//! Ports `maho-chromium/browser/importer/firefox_history_parser.cc`.

use std::path::Path;
use crate::{HistoryEntry, ImportError, ImportResult};

const MAX_HISTORY_ROWS: u32 = 100_000;

pub fn parse_firefox_history(profile_dir: &Path) -> ImportResult<Vec<HistoryEntry>> {
    let db_path = profile_dir.join("places.sqlite");

    if !db_path.exists() {
        return Err(ImportError::FileNotFound(db_path.display().to_string()));
    }

    let conn = open_readonly(&db_path)?;

    let mut stmt = conn
        .prepare(
            "SELECT p.url, p.title, p.visit_count, MAX(h.visit_date) \
             FROM moz_places p \
             JOIN moz_historyvisits h ON p.id = h.place_id \
             WHERE p.url NOT LIKE 'place:%' \
               AND p.url NOT LIKE 'about:%' \
             GROUP BY p.id \
             ORDER BY MAX(h.visit_date) DESC \
             LIMIT ?1",
        )
        .map_err(|e| ImportError::Parse(format!("prepare: {e}")))?;

    let rows = stmt
        .query_map([MAX_HISTORY_ROWS], |row| {
            let url: String = row.get(0)?;
            let title: Option<String> = row.get(1)?;
            let visit_count: u32 = row.get(2)?;
            let visit_date_us: i64 = row.get::<_, Option<i64>>(3)?.unwrap_or(0);
            Ok(HistoryEntry {
                url,
                title: title.unwrap_or_default(),
                visit_count,
                // Firefox stores microseconds since epoch; convert to seconds
                visit_time: visit_date_us as f64 / 1_000_000.0,
            })
        })
        .map_err(|e| ImportError::Parse(format!("query: {e}")))?;

    let mut results = Vec::new();
    for row_result in rows {
        match row_result {
            Ok(entry) => {
                if !entry.url.is_empty() {
                    results.push(entry);
                }
            }
            Err(e) => return Err(ImportError::Parse(format!("row: {e}"))),
        }
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
    use std::fs;

    fn create_history_db(dir: &Path) {
        let db_path = dir.join("places.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_places (
                 id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER
             );
             CREATE TABLE moz_historyvisits (
                 id INTEGER PRIMARY KEY, place_id INTEGER, visit_date INTEGER
             );
             INSERT INTO moz_places VALUES (1, 'https://example.com', 'Example', 3);
             INSERT INTO moz_places VALUES (2, 'https://rust-lang.org', 'Rust', 1);
             INSERT INTO moz_places VALUES (3, 'place:sort=8', NULL, 1);
             INSERT INTO moz_places VALUES (4, 'about:config', NULL, 1);
             INSERT INTO moz_historyvisits VALUES (1, 1, 1718000000000000);
             INSERT INTO moz_historyvisits VALUES (2, 1, 1718100000000000);
             INSERT INTO moz_historyvisits VALUES (3, 2, 1717000000000000);
             INSERT INTO moz_historyvisits VALUES (4, 3, 1717000000000000);
             INSERT INTO moz_historyvisits VALUES (5, 4, 1717000000000000);",
        )
        .unwrap();
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-hist-missing");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let err = parse_firefox_history(&tmp).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)));
    }

    #[test]
    fn corrupted_db_returns_error() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-hist-corrupt");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("places.sqlite"), b"garbage").unwrap();
        let err = parse_firefox_history(&tmp).unwrap_err();
        assert!(
            matches!(err, ImportError::Parse(_) | ImportError::Io(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn parses_valid_history() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-hist-valid");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        create_history_db(&tmp);

        let entries = parse_firefox_history(&tmp).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "https://example.com");
        assert_eq!(entries[0].title, "Example");
        assert_eq!(entries[0].visit_count, 3);
        assert!((entries[0].visit_time - 1718100000.0).abs() < 1.0);

        assert_eq!(entries[1].url, "https://rust-lang.org");
    }

    #[test]
    fn filters_place_and_about_urls() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-hist-filter");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        create_history_db(&tmp);

        let entries = parse_firefox_history(&tmp).unwrap();
        for e in &entries {
            assert!(!e.url.starts_with("place:"));
            assert!(!e.url.starts_with("about:"));
        }
    }
}
