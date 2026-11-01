//! Chromium autofill (Web Data) SQLite parser.
//!
//! Ports `maho-chromium/browser/importer/chromium_autofill_parser.cc` to Rust.
//! Reads the `Web Data` SQLite database from a Chromium profile directory and
//! extracts autofill form field entries.

use std::path::Path;

use rusqlite::Connection;

use crate::{AutofillEntry, ImportError, ImportResult};

/// Parses Chromium autofill data from a profile directory.
///
/// The `profile_dir` should contain a file named `Web Data`.
pub fn parse_chromium_autofill(profile_dir: &Path) -> ImportResult<Vec<AutofillEntry>> {
    let web_data_path = profile_dir.join("Web Data");

    if !web_data_path.exists() {
        return Err(ImportError::FileNotFound(
            web_data_path.display().to_string(),
        ));
    }

    let conn = open_readonly(&web_data_path)?;

    let mut entries = Vec::new();

    // At least one expected table must exist; otherwise the file is likely corrupt.
    let has_autofill = table_exists(&conn, "autofill");
    let has_addresses = table_exists(&conn, "local_addresses");
    let has_cards = table_exists(&conn, "local_credit_cards");
    if !has_autofill && !has_addresses && !has_cards {
        return Err(ImportError::Parse(
            "no autofill tables found in Web Data".into(),
        ));
    }

    // Parse the autofill table (name/value pairs with usage counts)
    if has_autofill {
        let mut stmt = conn
            .prepare(
                "SELECT name, value, count, date_created, date_last_used \
                 FROM autofill ORDER BY count DESC",
            )
            .map_err(|e| ImportError::Parse(format!("preparing autofill query: {e}")))?;

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
            .map_err(|e| ImportError::Parse(format!("querying autofill: {e}")))?;

        for row in rows {
            let entry =
                row.map_err(|e| ImportError::Parse(format!("reading autofill row: {e}")))?;
            if !entry.field_name.is_empty() && !entry.value.is_empty() {
                entries.push(entry);
            }
        }
    }

    // Parse local_addresses table
    parse_table_fields(&conn, "local_addresses", ADDRESS_EXCLUDES, &mut entries)?;

    // Parse local_credit_cards table (excluding encrypted number)
    parse_table_fields(&conn, "local_credit_cards", CARD_EXCLUDES, &mut entries)?;

    Ok(entries)
}

const ADDRESS_EXCLUDES: &[&str] = &[
    "guid",
    "use_count",
    "use_date",
    "date_modified",
    "language_code",
    "date_created",
    "record_type",
    "id",
];

const CARD_EXCLUDES: &[&str] = &[
    "guid",
    "use_count",
    "use_date",
    "date_modified",
    "card_number_encrypted",
    "billing_address_id",
    "id",
];

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

fn parse_table_fields(
    conn: &Connection,
    table_name: &str,
    exclude_cols: &[&str],
    entries: &mut Vec<AutofillEntry>,
) -> ImportResult<()> {
    if !table_exists(conn, table_name) {
        return Ok(());
    }

    // Get column names via PRAGMA
    let mut pragma_stmt = conn
        .prepare(&format!("PRAGMA table_info({table_name})"))
        .map_err(|e| ImportError::Parse(format!("pragma for {table_name}: {e}")))?;

    let columns: Vec<String> = pragma_stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| ImportError::Parse(format!("reading columns for {table_name}: {e}")))?
        .filter_map(|r| r.ok())
        .filter(|col| !exclude_cols.contains(&col.as_str()))
        .collect();

    if columns.is_empty() {
        return Ok(());
    }

    let col_list = columns.join(", ");
    let query = format!("SELECT {col_list} FROM {table_name}");

    let mut stmt = conn
        .prepare(&query)
        .map_err(|e| ImportError::Parse(format!("preparing query for {table_name}: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            let mut fields = Vec::new();
            for (i, col_name) in columns.iter().enumerate() {
                if let Ok(value) = row.get::<_, String>(i) {
                    if !value.is_empty() {
                        fields.push((col_name.clone(), value));
                    }
                }
            }
            Ok(fields)
        })
        .map_err(|e| ImportError::Parse(format!("querying {table_name}: {e}")))?;

    for row in rows {
        let fields =
            row.map_err(|e| ImportError::Parse(format!("reading row from {table_name}: {e}")))?;
        for (name, value) in fields {
            entries.push(AutofillEntry {
                field_name: name,
                value,
                times_used: 0,
                first_used: 0,
                last_used: 0,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ferryx-import-test-chromium-autofill-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn create_autofill_db(dir: &Path) {
        let db_path = dir.join("Web Data");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE autofill (
                name TEXT NOT NULL,
                value TEXT NOT NULL,
                count INTEGER DEFAULT 1,
                date_created INTEGER DEFAULT 0,
                date_last_used INTEGER DEFAULT 0
            );
            INSERT INTO autofill (name, value, count, date_created, date_last_used)
                VALUES ('email', 'user@example.com', 10, 1700000000, 1700500000);
            INSERT INTO autofill (name, value, count, date_created, date_last_used)
                VALUES ('name', 'John Doe', 5, 1699000000, 1700400000);
            INSERT INTO autofill (name, value, count) VALUES ('', '', 1);",
        )
        .unwrap();
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let dir = create_test_dir("missing");
        let err = parse_chromium_autofill(&dir).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)), "got {err:?}");
    }

    #[test]
    fn corrupted_db_returns_error() {
        let dir = create_test_dir("corrupted");
        std::fs::write(dir.join("Web Data"), b"not a sqlite file").unwrap();
        let err = parse_chromium_autofill(&dir).unwrap_err();
        assert!(
            matches!(err, ImportError::Io(_) | ImportError::Parse(_)),
            "got {err:?}",
        );
    }

    #[test]
    fn parses_valid_autofill_db() {
        let dir = create_test_dir("valid");
        create_autofill_db(&dir);

        let entries = parse_chromium_autofill(&dir).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].field_name, "email");
        assert_eq!(entries[0].value, "user@example.com");
        assert_eq!(entries[0].times_used, 10);
        assert_eq!(entries[1].field_name, "name");
        assert_eq!(entries[1].value, "John Doe");
    }

    #[test]
    fn parses_address_table() {
        let dir = create_test_dir("addresses");
        let db_path = dir.join("Web Data");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE local_addresses (
                id INTEGER PRIMARY KEY,
                guid TEXT,
                first_name TEXT,
                last_name TEXT,
                city TEXT,
                use_count INTEGER,
                use_date INTEGER
            );
            INSERT INTO local_addresses (guid, first_name, last_name, city, use_count, use_date)
            VALUES ('abc', 'Jane', 'Smith', 'Seoul', 5, 0);",
        )
        .unwrap();

        let entries = parse_chromium_autofill(&dir).unwrap();
        // first_name, last_name, city (guid/use_count/use_date/id excluded)
        assert_eq!(entries.len(), 3);
        let names: Vec<&str> = entries.iter().map(|e| e.field_name.as_str()).collect();
        assert!(names.contains(&"first_name"));
        assert!(names.contains(&"last_name"));
        assert!(names.contains(&"city"));
    }
}
