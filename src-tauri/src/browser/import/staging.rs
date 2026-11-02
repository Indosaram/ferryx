//! JSON-lines staging files: the hand-off from an import to the bookmark, history and
//! password stores.
//!
//! Contract (the stores' ingesters rely on it):
//! - Directory: `<data>/browser-import/<profile>/pending/`.
//! - One file per import run and data type: `<type>-<unix millis>-<run>-<pid>.jsonl`, `<type>`
//!   one of `bookmarks`, `history`, `passwords`, `<run>` a zero-padded per-process counter so
//!   names sort in write order. A file appears only complete: it is written as
//!   `….jsonl.partial` and renamed when its type finishes, so an ingester never sees a torn file
//!   and ignores `.partial` files (a crash leaves one behind; it is safe to delete).
//! - One JSON object per line, camelCase: [`StagedRecord`] = `{"source": "<browser name>", …}`
//!   flattened with [`ImportedBookmark`], [`ImportedHistoryEntry`] or [`ImportedPassword`].
//! - The ingester (`super::ingest`) wipes and deletes a file once it has read it, stored or
//!   not. Password files hold plaintext secrets: they are created owner-only (0600) on Unix.

use super::{
    ImportDataType, ImportedBookmark, ImportedHistoryEntry, ImportedPassword, SinkError,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

pub const PENDING_DIR: &str = "pending";
pub const PARTIAL_SUFFIX: &str = ".partial";

/// Distinguishes two runs started in the same millisecond.
static RUN_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `<data>/browser-import/<profile>/pending`.
pub fn pending_dir(data_dir: &Path, profile: &str) -> PathBuf {
    data_dir.join("browser-import").join(profile).join(PENDING_DIR)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedRecord<T> {
    pub source: String,
    #[serde(flatten)]
    pub record: T,
}

struct OpenFile {
    partial: PathBuf,
    writer: BufWriter<File>,
    count: usize,
}

/// Writes the records of one import run; one file per data type.
pub struct StagingWriter {
    dir: PathBuf,
    source: String,
    run_tag: String,
    files: Mutex<Vec<(ImportDataType, OpenFile)>>,
}

fn create_file(path: &Path, secret: bool) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = secret;
    options.open(path)
}

fn failed(path: &Path, error: impl std::fmt::Display) -> SinkError {
    SinkError::Failed(format!("{}: {error}", path.display()))
}

impl StagingWriter {
    pub fn new(dir: PathBuf, source: impl Into<String>) -> Self {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let run = RUN_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            dir,
            source: source.into(),
            run_tag: format!("{millis}-{run:08}-{}", std::process::id()),
            files: Mutex::new(Vec::new()),
        }
    }

    fn final_path(&self, data_type: ImportDataType) -> PathBuf {
        self.dir.join(format!("{}-{}.jsonl", data_type.as_str(), self.run_tag))
    }

    fn write<T: Serialize>(&self, data_type: ImportDataType, record: T) -> Result<(), SinkError> {
        let line = serde_json::to_string(&StagedRecord { source: self.source.clone(), record })
            .map_err(|error| SinkError::Failed(error.to_string()))?;
        let mut files = self.files.lock();
        if !files.iter().any(|(kind, _)| *kind == data_type) {
            std::fs::create_dir_all(&self.dir).map_err(|error| failed(&self.dir, error))?;
            let partial = PathBuf::from(format!(
                "{}{PARTIAL_SUFFIX}",
                self.final_path(data_type).display()
            ));
            let file = create_file(&partial, data_type == ImportDataType::Passwords)
                .map_err(|error| failed(&partial, error))?;
            files.push((data_type, OpenFile { partial, writer: BufWriter::new(file), count: 0 }));
        }
        let (_, open) = files
            .iter_mut()
            .find(|(kind, _)| *kind == data_type)
            .ok_or_else(|| SinkError::Failed("staging file missing".into()))?;
        writeln!(open.writer, "{line}").map_err(|error| failed(&open.partial, error))?;
        open.count += 1;
        Ok(())
    }

    pub fn add_bookmark(&self, bookmark: ImportedBookmark) -> Result<(), SinkError> {
        self.write(ImportDataType::Bookmarks, bookmark)
    }

    pub fn add_history(&self, entry: ImportedHistoryEntry) -> Result<(), SinkError> {
        self.write(ImportDataType::History, entry)
    }

    pub fn add_password(&self, password: ImportedPassword) -> Result<(), SinkError> {
        self.write(ImportDataType::Passwords, password)
    }

    /// Publishes the file of `data_type` (renames it into place); returns its record count.
    pub fn finish(&self, data_type: ImportDataType) -> Result<usize, SinkError> {
        let open = {
            let mut files = self.files.lock();
            match files.iter().position(|(kind, _)| *kind == data_type) {
                Some(index) => files.remove(index).1,
                None => return Ok(0),
            }
        };
        let OpenFile { partial, writer, count } = open;
        let file = writer.into_inner().map_err(|error| failed(&partial, error.error()))?;
        file.sync_all().map_err(|error| failed(&partial, error))?;
        drop(file);
        let target = self.final_path(data_type);
        if let Err(error) = std::fs::rename(&partial, &target) {
            discard_file(&partial);
            return Err(failed(&target, error));
        }
        Ok(count)
    }

    /// Wipes and deletes the unfinished file of `data_type` (a cancelled or failed type).
    pub fn discard(&self, data_type: ImportDataType) {
        let mut files = self.files.lock();
        if let Some(index) = files.iter().position(|(kind, _)| *kind == data_type) {
            let (_, open) = files.remove(index);
            drop(open.writer);
            discard_file(&open.partial);
        }
    }
}

fn discard_file(path: &Path) {
    if let Err(error) = super::ingest::shred(path) {
        tracing::warn!(path = %path.display(), %error, "could not remove staging file");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn a_type_appears_only_when_finished_with_one_record_per_line() {
        let root = tempfile::tempdir().unwrap();
        let dir = pending_dir(root.path(), "default");
        let writer = StagingWriter::new(dir.clone(), "Chrome");
        writer
            .add_bookmark(ImportedBookmark {
                title: "Rust".into(),
                url: "https://rust-lang.org/".into(),
                folder_path: vec!["Bookmarks bar".into()],
            })
            .unwrap();
        writer
            .add_bookmark(ImportedBookmark {
                title: "Docs".into(),
                url: "https://docs.rs/".into(),
                folder_path: vec![],
            })
            .unwrap();
        assert!(files_in(&dir).iter().all(|name| name.ends_with(PARTIAL_SUFFIX)));

        assert_eq!(writer.finish(ImportDataType::Bookmarks).unwrap(), 2);
        let names = files_in(&dir);
        assert_eq!(names.len(), 1);
        assert!(names[0].starts_with("bookmarks-") && names[0].ends_with(".jsonl"));

        let text = std::fs::read_to_string(dir.join(&names[0])).unwrap();
        let records: Vec<StagedRecord<ImportedBookmark>> =
            text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].source, "Chrome");
        assert_eq!(records[0].record.folder_path, vec!["Bookmarks bar".to_string()]);
        let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(first["folderPath"][0], "Bookmarks bar");
        assert_eq!(first["url"], "https://rust-lang.org/");
    }

    #[test]
    fn a_type_with_no_records_writes_nothing_and_a_discarded_type_leaves_nothing() {
        let root = tempfile::tempdir().unwrap();
        let dir = pending_dir(root.path(), "work");
        let writer = StagingWriter::new(dir.clone(), "Firefox");
        assert_eq!(writer.finish(ImportDataType::History).unwrap(), 0);
        writer
            .add_history(ImportedHistoryEntry {
                url: "https://a.test/".into(),
                title: "A".into(),
                visited_at_unix: 1_700_000_000.5,
                visit_count: 3,
            })
            .unwrap();
        writer.discard(ImportDataType::History);
        assert!(files_in(&dir).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn password_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let dir = pending_dir(root.path(), "default");
        let writer = StagingWriter::new(dir.clone(), "Bitwarden JSON");
        writer
            .add_password(ImportedPassword {
                origin_url: "https://a.test".into(),
                action_url: String::new(),
                username: "me".into(),
                password: "hunter2".into(),
            })
            .unwrap();
        writer.finish(ImportDataType::Passwords).unwrap();
        let name = files_in(&dir).pop().unwrap();
        let mode = std::fs::metadata(dir.join(name)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
