//! Opens another browser's SQLite database through a private copy.
//!
//! A running browser keeps its databases open (Windows Chromium even holds `Cookies`
//! exclusively) and writes recent changes to the `-wal` file first. Reading the live file in
//! place would either fail or miss those changes, so the database and its sidecars are copied
//! into a temporary directory and the copy is opened read-only.

use std::ops::Deref;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::{ImportError, ImportResult};

const SIDECARS: [&str; 3] = ["-wal", "-shm", "-journal"];

/// A read-only connection to a copy of a database. The copy is deleted on drop.
pub struct Snapshot {
    // Declared before `_dir` so the connection closes before its directory is removed.
    conn: Connection,
    _dir: tempfile::TempDir,
}

impl Deref for Snapshot {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        &self.conn
    }
}

/// Maps an I/O error on a source file to the import error the UI can act on.
pub fn io_error(path: &Path, error: &std::io::Error) -> ImportError {
    let shown = path.display().to_string();
    match error.kind() {
        std::io::ErrorKind::NotFound => ImportError::FileNotFound(shown),
        std::io::ErrorKind::PermissionDenied => ImportError::PermissionDenied(shown),
        _ if is_sharing_violation(error) => ImportError::Locked(shown),
        _ => ImportError::Io(format!("{shown}: {error}")),
    }
}

/// ERROR_SHARING_VIOLATION (32) / ERROR_LOCK_VIOLATION (33): another process holds the file.
fn is_sharing_violation(error: &std::io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33))
}

/// Copies `path` (and any `-wal`/`-shm`/`-journal` next to it) and opens the copy read-only.
pub fn open_snapshot(path: &Path) -> ImportResult<Snapshot> {
    let file_name = path
        .file_name()
        .ok_or_else(|| ImportError::Io(format!("not a file path: {}", path.display())))?;
    let dir = tempfile::Builder::new()
        .prefix("ferryx-import-")
        .tempdir()
        .map_err(|e| ImportError::Io(format!("creating temp dir: {e}")))?;
    let copy = dir.path().join(file_name);
    std::fs::copy(path, &copy).map_err(|e| io_error(path, &e))?;
    for suffix in SIDECARS {
        let mut name = file_name.to_os_string();
        name.push(suffix);
        let source = path.with_file_name(&name);
        match std::fs::copy(&source, dir.path().join(&name)) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(&source, &e)),
        }
    }
    let conn = Connection::open_with_flags(
        &copy,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| ImportError::Io(format!("{}: {e}", path.display())))?;
    Ok(Snapshot { conn, _dir: dir })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rows_that_are_still_in_the_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("History");
        let writer = Connection::open(&path).unwrap();
        writer
            .execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
                 CREATE TABLE t(v TEXT); INSERT INTO t VALUES ('in-wal');",
            )
            .unwrap();
        // The writer stays open, as a running browser would: the row lives only in History-wal.
        assert!(dir.path().join("History-wal").exists());

        let snapshot = open_snapshot(&path).unwrap();
        let value: String = snapshot.query_row("SELECT v FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(value, "in-wal");
        drop(writer);
    }

    #[test]
    fn missing_database_is_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let err = open_snapshot(&dir.path().join("Cookies")).err().unwrap();
        assert!(matches!(err, ImportError::FileNotFound(_)), "{err:?}");
    }
}
