//! Firefox autofill parser — reads `formhistory.sqlite` (moz_formhistory table).
//!
//! Firefox stores form autocomplete data in `formhistory.sqlite` with fields:
//! fieldname, value, timesUsed, firstUsed, lastUsed (microseconds since epoch).

use std::path::Path;

use rusqlite::Connection;

use crate::{AutofillEntry, ImportError, ImportResult};

pub fn parse_firefox_autofill(profile_dir: &Path) -> ImportResult<Vec<AutofillEntry>> {
    let db_path = profile_dir.join("formhistory.sqlite");

    if !db_path.exists() {
        return Err(ImportError::FileNotFound(db_path.display().to_string()));
    }

    let conn = open_readonly(&db_path)?;

    if !table_exists(&conn, "moz_formhistory") {
        return Err(ImportError::Parse("moz_formhistory table not found".into()));
    }

    let mut stmt = conn
        .prepare(
            "SELECT fieldname, value, timesUsed, firstUsed, lastUsed \
             FROM moz_formhistory \
             ORDER BY timesUsed DESC",
        )
        .map_err(|e| ImportError::Parse(format!("prepare: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(AutofillEntry {
                field_name: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                value: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                times_used: row.get::<_, Option<i32>>(2)?.unwrap_or(0),
                first_used: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                last_used: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
            })
        })
        .map_err(|e| ImportError::Parse(format!("query: {e}")))?;

    let mut results = Vec::new();
    for row_result in rows {
        let entry = row_result.map_err(|e| ImportError::Parse(format!("row: {e}")))?;
        if !entry.field_name.is_empty() && !entry.value.is_empty() {
            results.push(entry);
        }
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

    fn create_formhistory_db(dir: &Path) {
        let db_path = dir.join("formhistory.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_formhistory (
                 id INTEGER PRIMARY KEY,
                 fieldname TEXT, value TEXT, timesUsed INTEGER,
                 firstUsed INTEGER, lastUsed INTEGER
             );
             INSERT INTO moz_formhistory (fieldname, value, timesUsed, firstUsed, lastUsed)
                 VALUES ('email', 'user@example.com', 15, 1700000000000000, 1718000000000000);
             INSERT INTO moz_formhistory (fieldname, value, timesUsed, firstUsed, lastUsed)
                 VALUES ('username', 'jdoe', 3, 1710000000000000, 1715000000000000);
             -- empty value should be filtered
             INSERT INTO moz_formhistory (fieldname, value, timesUsed, firstUsed, lastUsed)
                 VALUES ('search', '', 1, 0, 0);
             -- empty fieldname should be filtered
             INSERT INTO moz_formhistory (fieldname, value, timesUsed, firstUsed, lastUsed)
                 VALUES ('', 'orphan', 1, 0, 0);",
        )
        .unwrap();
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-autofill-missing");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let err = parse_firefox_autofill(&tmp).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)));
    }

    #[test]
    fn corrupted_db_returns_error() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-autofill-corrupt");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("formhistory.sqlite"), b"nope").unwrap();
        let err = parse_firefox_autofill(&tmp).unwrap_err();
        assert!(
            matches!(err, ImportError::Parse(_) | ImportError::Io(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn missing_table_returns_parse_error() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-autofill-notable");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("formhistory.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE other (id INTEGER);")
            .unwrap();
        drop(conn);

        let err = parse_firefox_autofill(&tmp).unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)));
    }

    #[test]
    fn parses_valid_autofill() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-autofill-valid");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        create_formhistory_db(&tmp);

        let entries = parse_firefox_autofill(&tmp).unwrap();
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].field_name, "email");
        assert_eq!(entries[0].value, "user@example.com");
        assert_eq!(entries[0].times_used, 15);
        assert_eq!(entries[0].first_used, 1700000000000000);
        assert_eq!(entries[0].last_used, 1718000000000000);

        assert_eq!(entries[1].field_name, "username");
        assert_eq!(entries[1].value, "jdoe");
    }
}
