use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

const JOURNAL_FILE: &str = "split_operations.json";
const LOCK_FILE: &str = "split_operations.lock";

/// A durable snapshot of one local split operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitJournalEntry {
    pub request_id: String,
    pub fingerprint: String,
    pub expires_at_unix_ms: u64,
    pub session_id: Option<String>,
    pub cancel_requested: bool,
    pub tombstone: bool,
    #[serde(default)]
    pub outcome: Option<crate::daemon::protocol::SplitOperationResult>,
}

#[derive(Debug, thiserror::Error)]
pub enum SplitJournalError {
    #[error("split journal I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("split journal entry is corrupt: {0}")]
    CorruptEntry(String),
    #[error("invalid split request id")]
    InvalidRequestId,
    #[error("split request identity was reused with different parameters")]
    Conflict,
    #[error("split journal lock poisoned")]
    LockPoisoned,
}

/// Synchronous durable journal scoped to one workspace directory.
///
/// All methods perform blocking filesystem I/O. Async callers must execute them
/// through `crate::ipc::run_blocking` rather than on a Tokio runtime thread.
pub struct SplitJournal {
    dir: PathBuf,
    process_lock: Mutex<()>,
}

impl SplitJournal {
    /// Creates or loads the journal in a workspace-scoped directory.
    pub fn open(dir: &Path) -> Result<Self, SplitJournalError> {
        fs::create_dir_all(dir)?;
        let journal = Self {
            dir: dir.to_path_buf(),
            process_lock: Mutex::new(()),
        };
        {
            let _guard = journal.lock()?;
            journal.read_entries()?;
        }
        Ok(journal)
    }

    /// Reserves before creation under the same cross-process lock as publication.
    /// `false` means the caller must reconcile, never spawn a replacement.
    pub fn begin(&self, entry: &SplitJournalEntry) -> Result<bool, SplitJournalError> {
        validate_request_id(&entry.request_id)?;
        let _guard = self.lock()?;
        let mut entries = self.read_entries()?;
        if let Some(previous) = entries.get(&entry.request_id) {
            if !previous.fingerprint.is_empty() && previous.fingerprint != entry.fingerprint {
                return Err(SplitJournalError::Conflict);
            }
            return Ok(false);
        }
        entries.insert(entry.request_id.clone(), entry.clone());
        self.write_entries(&entries)?;
        Ok(true)
    }

    /// Atomically records one entry without losing other entries or concurrent updates.
    pub fn upsert(&self, entry: &SplitJournalEntry) -> Result<(), SplitJournalError> {
        validate_request_id(&entry.request_id)?;
        let _guard = self.lock()?;
        let mut entries = self.read_entries()?;
        if entries.get(&entry.request_id).is_some_and(|previous|
            !previous.fingerprint.is_empty() && previous.fingerprint != entry.fingerprint)
        {
            return Err(SplitJournalError::Conflict);
        }
        if entries
            .get(&entry.request_id)
            .is_some_and(|existing| existing.tombstone || existing.cancel_requested)
        {
            if let Some(existing) = entries.get_mut(&entry.request_id) {
                if existing.session_id.is_none() { existing.session_id = entry.session_id.clone(); }
            }
            self.write_entries(&entries)?;
            return Ok(());
        }
        entries.insert(entry.request_id.clone(), entry.clone());
        self.write_entries(&entries)
    }

    pub fn load(&self, request_id: &str) -> Result<Option<SplitJournalEntry>, SplitJournalError> {
        validate_request_id(request_id)?;
        let _guard = self.lock()?;
        Ok(self.read_entries()?.remove(request_id))
    }

    pub fn list(&self) -> Result<Vec<SplitJournalEntry>, SplitJournalError> {
        let _guard = self.lock()?;
        Ok(self.read_entries()?.into_values().collect())
    }

    /// Marks a request non-creatable, retaining the fingerprint and any outcome.
    pub fn tombstone(
        &self,
        request_id: &str,
        fingerprint: &str,
    ) -> Result<(), SplitJournalError> {
        validate_request_id(request_id)?;
        let _guard = self.lock()?;
        let mut entries = self.read_entries()?;
        let entry = entries
            .entry(request_id.to_owned())
            .or_insert_with(|| SplitJournalEntry {
                request_id: request_id.to_owned(),
                fingerprint: fingerprint.to_owned(),
                expires_at_unix_ms: 0,
                session_id: None,
                cancel_requested: true,
                tombstone: true,
                outcome: Some(crate::daemon::protocol::SplitOperationResult::Cancelled),
            });
        entry.cancel_requested = true;
        entry.tombstone = true;
        entry.outcome = Some(crate::daemon::protocol::SplitOperationResult::Cancelled);
        self.write_entries(&entries)
    }

    /// Compacts expired terminal records without erasing the non-creation fence.
    pub fn purge_expired(&self, now_unix_ms: u64) -> Result<usize, SplitJournalError> {
        let _guard = self.lock()?;
        let mut entries = self.read_entries()?;
        let mut removed = 0;
        for entry in entries.values_mut() {
            if entry.expires_at_unix_ms <= now_unix_ms && !entry.tombstone
                && (entry.cancel_requested || entry.session_id.is_some())
            {
                entry.tombstone = true;
                removed += 1;
            }
        }
        if removed != 0 {
            self.write_entries(&entries)?;
        }
        Ok(removed)
    }

    fn lock(&self) -> Result<JournalGuard<'_>, SplitJournalError> {
        let process_guard = self
            .process_lock
            .lock()
            .map_err(|_| SplitJournalError::LockPoisoned)?;
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.dir.join(LOCK_FILE))?;
        lock_file.lock()?;
        Ok(JournalGuard {
            _process_guard: process_guard,
            lock_file,
        })
    }

    fn read_entries(&self) -> Result<BTreeMap<String, SplitJournalEntry>, SplitJournalError> {
        let path = self.dir.join(JOURNAL_FILE);
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let records: Vec<SplitJournalEntry> = serde_json::from_slice(&bytes)
            .map_err(|error| SplitJournalError::CorruptEntry(error.to_string()))?;
        let mut entries = BTreeMap::new();
        for entry in records {
            validate_request_id(&entry.request_id)?;
            if entries.insert(entry.request_id.clone(), entry).is_some() {
                return Err(SplitJournalError::CorruptEntry(
                    "duplicate request id".to_owned(),
                ));
            }
        }
        Ok(entries)
    }

    fn write_entries(
        &self,
        entries: &BTreeMap<String, SplitJournalEntry>,
    ) -> Result<(), SplitJournalError> {
        let path = self.dir.join(JOURNAL_FILE);
        let temp = self.dir.join(format!(
            ".{JOURNAL_FILE}.tmp-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("writer")
        ));
        let bytes = serde_json::to_vec(&entries.values().collect::<Vec<_>>())
            .map_err(|error| SplitJournalError::CorruptEntry(error.to_string()))?;
        let result = (|| -> Result<(), std::io::Error> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temp, &path)?;
            #[cfg(unix)]
            File::open(&self.dir)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(SplitJournalError::Io)
    }
}

struct JournalGuard<'a> {
    _process_guard: MutexGuard<'a, ()>,
    lock_file: File,
}

impl Drop for JournalGuard<'_> {
    fn drop(&mut self) {
        let _ = self.lock_file.unlock();
    }
}

fn validate_request_id(request_id: &str) -> Result<(), SplitJournalError> {
    if request_id.trim().is_empty()
        || request_id.len() > 256
        || request_id.chars().any(char::is_control)
    {
        return Err(SplitJournalError::InvalidRequestId);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn entry(id: &str, expiry: u64, session_id: Option<&str>) -> SplitJournalEntry {
        SplitJournalEntry {
            request_id: id.to_owned(),
            fingerprint: format!("fingerprint-{id}"),
            expires_at_unix_ms: expiry,
            session_id: session_id.map(str::to_owned),
            cancel_requested: false,
            tombstone: false,
            outcome: None,
        }
    }

    #[test]
    fn local_split_reliability_round_trips_entries_after_reopen() {
        let dir = tempdir().expect("temporary journal directory");
        let expected = entry("request-1", 500, Some("session-1"));
        SplitJournal::open(dir.path())
            .expect("open")
            .upsert(&expected)
            .expect("persist entry");
        let reopened = SplitJournal::open(dir.path()).expect("reopen");
        assert_eq!(reopened.load("request-1").expect("load"), Some(expected));
    }

    #[test]
    fn local_split_reliability_tombstone_is_idempotent_and_blocks_creation() {
        let dir = tempdir().expect("temporary journal directory");
        let journal = SplitJournal::open(dir.path()).expect("open");
        journal.tombstone("request-2", "fingerprint-request-2").expect("tombstone");
        journal.tombstone("request-2", "fingerprint-request-2").expect("repeat tombstone");
        let cancelled = journal.load("request-2").expect("load").expect("record");
        assert!(cancelled.cancel_requested && cancelled.tombstone);
        journal
            .upsert(&entry("request-2", 900, None))
            .expect("retry cannot overwrite tombstone");
        let still_cancelled = journal.load("request-2").expect("load").expect("record");
        assert!(still_cancelled.tombstone && still_cancelled.cancel_requested);
    }

    #[test]
    fn local_split_reliability_expired_records_keep_creation_fences() {
        let dir = tempdir().expect("temporary journal directory");
        let journal = SplitJournal::open(dir.path()).expect("open");
        journal.upsert(&entry("pending", 10, None)).expect("pending");
        journal.upsert(&entry("created", 10, Some("s1"))).expect("created");
        journal.upsert(&entry("future", 100, Some("s2"))).expect("future");
        journal.tombstone("cancelled", "fp").expect("cancelled");
        assert_eq!(journal.purge_expired(10).expect("purge"), 1);
        let remaining = journal.list().expect("list");
        assert!(remaining.iter().any(|item| item.request_id == "pending"));
        assert!(remaining.iter().any(|item| item.request_id == "future"));
        assert!(remaining.iter().any(|item| item.request_id == "created" && item.tombstone));
        assert!(remaining.iter().any(|item| item.request_id == "cancelled" && item.tombstone));
    }

    #[test]
    fn local_split_reliability_reservation_survives_reopen_without_second_creation() {
        let dir = tempdir().unwrap();
        let journal = SplitJournal::open(dir.path()).unwrap();
        let pending = entry("reserved", 900, None);
        assert!(journal.begin(&pending).unwrap());
        drop(journal);
        let journal = SplitJournal::open(dir.path()).unwrap();
        assert!(!journal.begin(&pending).unwrap());
        let mut conflicting = pending;
        conflicting.fingerprint = "different".into();
        assert!(matches!(journal.begin(&conflicting), Err(SplitJournalError::Conflict)));
    }

    #[test]
    fn local_split_reliability_late_publication_retains_cancelled_child_for_cleanup() {
        let dir = tempdir().expect("temporary journal directory");
        let journal = SplitJournal::open(dir.path()).expect("open");
        let pending = entry("late-child", 900, None);
        assert!(journal.begin(&pending).expect("reserve"));
        journal.tombstone("late-child", &pending.fingerprint).expect("cancel");

        journal.upsert(&entry("late-child", 900, Some("owned-child")))
            .expect("publish after cancellation");

        let record = SplitJournal::open(dir.path()).expect("reopen")
            .load("late-child").expect("load").expect("record");
        assert_eq!(record.session_id.as_deref(), Some("owned-child"));
        assert!(record.cancel_requested && record.tombstone);
        assert_eq!(record.outcome, Some(crate::daemon::protocol::SplitOperationResult::Cancelled));
        assert!(!journal.begin(&pending).expect("cancelled reservation"));
    }

    #[test]
    fn corrupt_journal_surfaces_typed_error() {
        let dir = tempdir().expect("temporary journal directory");
        fs::write(dir.path().join(JOURNAL_FILE), b"{not-json").expect("write corruption");
        assert!(matches!(
            SplitJournal::open(dir.path()),
            Err(SplitJournalError::CorruptEntry(_))
        ));
    }
}
