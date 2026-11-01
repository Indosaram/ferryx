//! Safari History.db (SQLite) parser.
//!
//! Ports the Safari history import logic to Rust.
//! Reads `<safari_dir>/History.db` and extracts visited URLs with timestamps.

use std::path::Path;

use crate::{HistoryEntry, ImportError, ImportResult};

pub fn parse_safari_history(safari_dir: &Path) -> ImportResult<Vec<HistoryEntry>> {
    let db_path = safari_dir.join("History.db");

    if !db_path.exists() {
        return Err(ImportError::FileNotFound(db_path.display().to_string()));
    }

    // Copying fails with PermissionDenied without Full Disk Access; Safari may hold the live file.
    let conn = crate::sqlite_snapshot::open_snapshot(&db_path)?;

    // Safari stores visit_time as seconds since 2001-01-01 (Core Data epoch).
    // Convert to Unix epoch by adding the offset.
    const CORE_DATA_EPOCH_OFFSET: f64 = 978_307_200.0;

    let mut stmt = conn
        .prepare(
            "SELECT hi.url, hv.title, hv.visit_time, hi.visit_count \
             FROM history_items hi \
             JOIN history_visits hv ON hi.id = hv.history_item \
             ORDER BY hv.visit_time DESC",
        )
        .map_err(|e| ImportError::Parse(format!("History.db query: {}", e)))?;

    let entries = stmt
        .query_map([], |row| {
            let url: String = row.get(0)?;
            let title: String = row.get::<_, Option<String>>(1)?.unwrap_or_default();
            let visit_time: f64 = row.get(2)?;
            let visit_count: u32 = row.get::<_, Option<u32>>(3)?.unwrap_or(1);
            Ok(HistoryEntry {
                url,
                title,
                visit_time: visit_time + CORE_DATA_EPOCH_OFFSET,
                visit_count,
            })
        })
        .map_err(|e| ImportError::Parse(format!("History.db rows: {}", e)))?;

    let mut result = Vec::new();
    for e in entries.flatten() {
        if !e.url.is_empty() {
            result.push(e);
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn create_test_db(dir: &Path) {
        let db_path = dir.join("History.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE history_items (
                id INTEGER PRIMARY KEY,
                url TEXT NOT NULL,
                visit_count INTEGER DEFAULT 1
            );
            CREATE TABLE history_visits (
                id INTEGER PRIMARY KEY,
                history_item INTEGER,
                title TEXT,
                visit_time REAL
            );
            INSERT INTO history_items (id, url, visit_count) VALUES (1, 'https://example.com', 3);
            INSERT INTO history_items (id, url, visit_count) VALUES (2, 'https://rust-lang.org', 1);
            INSERT INTO history_visits (history_item, title, visit_time)
                VALUES (1, 'Example', 700000000.0);
            INSERT INTO history_visits (history_item, title, visit_time)
                VALUES (2, 'Rust', 700001000.0);",
        )
        .unwrap();
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let err = parse_safari_history(tmp.path()).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)), "got {:?}", err);
    }

    #[test]
    fn corrupted_db_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("History.db"), b"not a sqlite db").unwrap();
        let err = parse_safari_history(tmp.path()).unwrap_err();
        assert!(
            matches!(err, ImportError::Io(_) | ImportError::Parse(_)),
            "got {:?}",
            err
        );
    }

    #[test]
    fn parses_valid_history() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_db(tmp.path());

        let entries = parse_safari_history(tmp.path()).unwrap();
        assert_eq!(entries.len(), 2);

        // Entries ordered by visit_time DESC
        assert_eq!(entries[0].url, "https://rust-lang.org");
        assert_eq!(entries[0].title, "Rust");
        assert_eq!(entries[0].visit_count, 1);
        // 700001000 + 978307200 = 1678308200
        assert!((entries[0].visit_time - 1_678_308_200.0).abs() < 0.01);

        assert_eq!(entries[1].url, "https://example.com");
        assert_eq!(entries[1].visit_count, 3);
    }

    #[test]
    fn empty_db_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("History.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE history_items (id INTEGER PRIMARY KEY, url TEXT, visit_count INTEGER);
             CREATE TABLE history_visits (id INTEGER PRIMARY KEY, history_item INTEGER, title TEXT, visit_time REAL);",
        )
        .unwrap();

        let entries = parse_safari_history(tmp.path()).unwrap();
        assert!(entries.is_empty());
    }
}
