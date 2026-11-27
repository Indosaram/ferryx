//! Durable admission intent log for new PTY sessions.
//!
//! This is the *counted-before-dispatch* record. A reservation is appended and `sync_data`d
//! before the caller is allowed to ask a keeper to spawn anything, so a daemon crash can never
//! leave a child that the restarting daemon has no record of. The invariant this file exists to
//! uphold is one-directional on purpose:
//!
//! > **The bytes reach the disk before the request leaves the process.**
//!
//! The complement — "this reservation is definitely dead, drop it" — is deliberately *not* a
//! property of this store. Nothing here infers death from absence, from a timeout, or from a
//! failed read. A reservation leaves the reserved state in exactly two ways:
//!
//! - [`ReservationState::Bound`]: the keeper acknowledged the spawn, so no unanswered request
//!   remains for this key. From there [`ReservationState::Ended`] may release the slot.
//! - [`ReservationState::Abandoned`]: reachable **only** through
//!   [`AdmissionStore::abandon_before_dispatch`], whose caller asserts that no byte of the request
//!   ever reached a keeper.
//!
//! **Nothing the keeper can say today justifies the second one after dispatch.** A correlated
//! `ERROR` reply bounds no future: the keeper's `spawns` map is a live-session hint that is pruned
//! when a session goes away, so a duplicate `Spawn` carrying the same request id misses the map,
//! falls through the keeper's existence check, and forks a second child — possibly on a later
//! connection, long after the refusal was read. The keeper's in-memory `requestId` tombstone is
//! in-memory only (lost on keeper restart), bounded (8192 ids, so absence proves nothing), and
//! signalled by a message prefix rather than a typed field; it is a positive oracle at best and
//! never a durable one.
//!
//! **The logical spawn request id lives here too.** A spawn that will be dispatched to the keeper
//! records [`ReservationState::ReservedWithRequest`] instead of a plain
//! [`ReservationState::Reserved`], so the one id a retried `SPAWN` must repeat is on the disk
//! before the request can leave the process, and survives the crash that makes the retry
//! necessary. A plain `Reserved` is what older logs hold and what a direct PTY (no keeper, nothing
//! to retry) records.
//!
//! Everything else stays counted: an unresolved reservation is a leak, which costs capacity;
//! releasing one that can still spawn is an over-admission, which breaks the hard limit. This file
//! always chooses the leak.
//!
//! ## Format
//!
//! `MAGIC` (8 bytes) followed by frames of `len: u32 LE || crc32(payload): u32 LE || payload`,
//! where the payload is one JSON [`Entry`]. Folding is last-record-wins per key, so the log is
//! append-only and every transition is a new frame.
//!
//! ## Corruption is terminal, not recoverable
//!
//! `open` fails closed on *any* damage: a short header, a short body, an implausible length, a
//! checksum mismatch, an undecodable payload, a bad magic, or a file past `MAX_STORE_BYTES`. It
//! never truncates a tail and never repairs in place. A torn region can hide *several*
//! reservations, so the true count after a tear is unknown, and the only safe count is "refuse
//! new creates" — [`StoreError::Corrupt`] is the caller's signal to stop admitting, not to guess.
//!
//! ## Durability of directory entries
//!
//! `sync_data` on a record only flushes that record. It does not make a *newly created* file's
//! directory entry durable. So the file and its parent directory entry are made durable once, in
//! [`AdmissionStore::open`], before any reservation exists; per-record appends then only ever
//! extend an already-durable inode and `sync_data` is sufficient. Compaction is the one operation
//! that creates a new inode, so it syncs the file, renames, and syncs the directory.
//!
//! ## Single writer
//!
//! `open` creates the log with `create_new`, so two processes opening the same path concurrently
//! cannot both believe they are the writer. The daemon instance lock is what guarantees this in
//! production; the flag is the belt to that suspenders.

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::PtyError;

/// Identifies the log and its on-disk layout version. Bumping this refuses older files rather
/// than misreading them.
const MAGIC: &[u8; 8] = b"FXADMIT\x01";

/// `len` + `crc32`.
const FRAME_HEADER_BYTES: usize = 8;

/// One frame payload is a small JSON object; anything larger is corruption, not a record.
const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

/// A log bigger than this is refused rather than loaded. A store this size holds far more
/// reservations than any admission limit, so it means the file is not the log we wrote.
const MAX_STORE_BYTES: usize = 8 * 1024 * 1024;

/// Where one reservation currently stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservationState {
    Reserved,
    /// A reservation that also carries the **logical spawn request id**.
    ///
    /// The id is minted once per logical spawn, written here before the request can leave the
    /// process, and repeated on every retry of that same spawn: the keeper's request-id tombstone
    /// can only retire a duplicate `SPAWN` if the retry presents the same id. A plain [`Self::Reserved`]
    /// is what older logs and callers that dispatch nothing (a direct PTY, a replay-only hub) hold.
    ReservedWithRequest { request_id: String },
    Bound { session_id: String },
    /// The session ended; sound because `Bound` means no unanswered request remains to fork.
    Ended { session_id: String },
    /// The reservation is provably dead because the request **never left this process**.
    ///
    /// This is the only state that releases a slot, and it is sound only under that precondition:
    /// once a byte has been written to the keeper socket, a later `ERROR`, timeout or
    /// `UNKNOWN_SESSION` does not bound the future, because a delayed duplicate `SPAWN` carrying
    /// the same request can still fork. Callers reach this state only through
    /// [`AdmissionGate::abandon_before_dispatch`], whose contract is that no dispatch happened.
    Abandoned { detail: String },
}

impl ReservationState {
    pub fn occupies_slot(&self) -> bool {
        matches!(
            self,
            Self::Reserved | Self::ReservedWithRequest { .. } | Self::Bound { .. }
        )
    }

    /// Charged, and no keeper acknowledgement has adopted it yet: the spawn may still be in flight.
    pub fn is_reserved(&self) -> bool {
        matches!(self, Self::Reserved | Self::ReservedWithRequest { .. })
    }

    /// The logical spawn request id this record carries, when it carries one.
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::ReservedWithRequest { request_id } => Some(request_id.as_str()),
            _ => None,
        }
    }
}

/// One folded record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub key: String,
    pub state: ReservationState,
}

/// Why the log could not be opened, appended to, or compacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// A filesystem operation failed. `op` names it so a caller can log something actionable.
    Io { op: &'static str, message: String },
    /// The file does not carry this log's magic. Not ours; refuse it.
    BadMagic,
    /// A length is impossible: zero, past `MAX_PAYLOAD_BYTES`, or the file is past
    /// `MAX_STORE_BYTES`. Treated as corruption so the caller refuses admission.
    Implausible { offset: usize, detail: &'static str },
    /// The log is damaged. It is *not* repaired or truncated: the true reservation count is
    /// unknown, so the only safe response is to stop admitting.
    Corrupt { offset: usize, detail: &'static str },
    /// A record could not be encoded. Should be unreachable; refusing is safer than writing junk.
    Encode(String),
    /// A prior append failed after the file may have been partly written, so the tail of the log
    /// is unknown. No further record may be appended and admission must stop.
    Poisoned,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { op, message } => write!(f, "ADMISSION_STORE_IO: {op}: {message}"),
            Self::BadMagic => write!(f, "ADMISSION_STORE_BAD_MAGIC"),
            Self::Implausible { offset, detail } => {
                write!(f, "ADMISSION_STORE_IMPLAUSIBLE at {offset}: {detail}")
            }
            Self::Corrupt { offset, detail } => {
                write!(f, "ADMISSION_STORE_CORRUPT at {offset}: {detail}")
            }
            Self::Encode(message) => write!(f, "ADMISSION_STORE_ENCODE: {message}"),
            Self::Poisoned => write!(f, "ADMISSION_STORE_POISONED"),
        }
    }
}

impl std::error::Error for StoreError {}

/// The append handle plus how many bytes it has written, so callers can observe growth.
#[derive(Debug)]
struct AppendHandle {
    file: std::fs::File,
    bytes: u64,
    /// Compaction is attempted once `bytes` reaches this. It starts at the store's threshold and
    /// moves with failures: a successful compaction resets it, and a failed one doubles it, so a
    /// persistent error (`ENOSPC`, a permissions change on the tmp path) cannot turn every
    /// subsequent append into a full rewrite of the folded log.
    next_compact_bytes: u64,
}

/// The durable reservation log.
#[derive(Debug)]
pub struct AdmissionStore {
    path: PathBuf,
    append: Mutex<AppendHandle>,
    /// Folded view of every frame ever appended: last state per key.
    entries: RwLock<BTreeMap<String, ReservationState>>,
    /// Set when an append failed after the file may have been partly written, or when compaction
    /// detached the append handle from the path. Either way the tail is no longer trustworthy.
    poisoned: AtomicBool,
    /// `append` compacts once the log grows past this many bytes.
    compact_threshold: u64,
}

impl AdmissionStore {
    /// Opens (or creates) the log at `path`, failing closed on any damage.
    ///
    /// Creating the file, writing its magic, and syncing the parent directory all happen here,
    /// before a single reservation exists. That is what lets [`Self::append`] rely on `sync_data`
    /// alone: it only ever extends an inode whose directory entry is already durable.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let parent = path.parent().map(Path::to_path_buf);
        if let Some(parent) = &parent {
            std::fs::create_dir_all(parent).map_err(|e| io_err("create_dir", e))?;
        }

        // Bound the read before allocating: an oversized or hostile log must be rejected without
        // pulling it into memory. `take(MAX + 1)` also covers a file that grows after the stat.
        let existing = match std::fs::File::open(path) {
            Ok(file) => {
                let len = file.metadata().map_err(|e| io_err("metadata", e))?.len();
                if len > MAX_STORE_BYTES as u64 {
                    return Err(StoreError::Implausible {
                        offset: 0,
                        detail: "log exceeds MAX_STORE_BYTES",
                    });
                }
                let mut bytes = Vec::with_capacity(len as usize);
                file.take(MAX_STORE_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| io_err("read", e))?;
                Some(bytes)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(io_err("read", error)),
        };

        let entries = match existing {
            Some(bytes) => {
                if bytes.len() > MAX_STORE_BYTES {
                    return Err(StoreError::Implausible {
                        offset: 0,
                        detail: "log exceeds MAX_STORE_BYTES",
                    });
                }
                decode(&bytes)?
            }
            None => {
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(|e| io_err("create", e))?;
                file.write_all(MAGIC).map_err(|e| io_err("write_magic", e))?;
                file.sync_all().map_err(|e| io_err("sync_magic", e))?;
                drop(file);
                // The new directory entry is durable before any record can refer to it.
                sync_dir(parent.as_deref())?;
                BTreeMap::new()
            }
        };

        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|e| io_err("open_append", e))?;
        let bytes = file.metadata().map_err(|e| io_err("metadata", e))?.len();

        let compact_threshold = (MAX_STORE_BYTES / 2) as u64;
        Ok(Self {
            path: path.to_path_buf(),
            append: Mutex::new(AppendHandle {
                file,
                bytes,
                next_compact_bytes: compact_threshold,
            }),
            entries: RwLock::new(entries),
            poisoned: AtomicBool::new(false),
            compact_threshold,
        })
    }

    /// Appends one transition and returns only after it is on the disk.
    ///
    /// **The caller may dispatch a spawn against `key` only after this returns `Ok`.** The append
    /// mutex serialises whole frames, so concurrent callers cannot interleave halves of a record
    /// and cannot lose each other: every append is an independent extension of an `O_APPEND`
    /// inode, never a read-modify-write of the file.
    ///
    /// On failure the in-memory view is left unchanged and the store is poisoned, because the
    /// file may now end in a partial frame and no later append could make it valid again.
    pub fn append(&self, key: &str, state: ReservationState) -> Result<(), StoreError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(StoreError::Poisoned);
        }
        let payload = self.encode(key, &state)?;
        let mut handle = self.append.lock();
        // Re-check under the lock: a failure in another thread poisons before we write.
        if self.poisoned.load(Ordering::Acquire) {
            return Err(StoreError::Poisoned);
        }
        let mut header = [0u8; FRAME_HEADER_BYTES];
        header[0..4].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        header[4..8].copy_from_slice(&crc32(&payload).to_le_bytes());
        let frame_len = (FRAME_HEADER_BYTES + payload.len()) as u64;
        // Fail closed *before* growing the log past the ceiling. An append that crossed it would
        // produce exactly the file this store's own `open` refuses, so the writer must stop here
        // instead of writing it and only finding out on the next start.
        if handle.bytes.saturating_add(frame_len) > MAX_STORE_BYTES as u64 {
            self.poisoned.store(true, Ordering::Release);
            return Err(StoreError::Implausible {
                offset: handle.bytes as usize,
                detail: "append would grow the log past MAX_STORE_BYTES",
            });
        }

        let written = handle
            .file
            .write_all(&header)
            .map_err(|e| io_err("append_header", e))
            .and_then(|()| {
                handle
                    .file
                    .write_all(&payload)
                    .map_err(|e| io_err("append_body", e))
            })
            .and_then(|()| handle.file.sync_data().map_err(|e| io_err("sync_data", e)));

        if let Err(error) = written {
            self.poisoned.store(true, Ordering::Release);
            return Err(error);
        }
        handle.bytes += frame_len;
        // Publish while still holding the append mutex (append -> entries, the same order as
        // `compact`), so a concurrent compaction can never snapshot a view that omits this
        // already-fsynced frame.
        self.entries.write().insert(key.to_string(), state);
        if handle.bytes >= handle.next_compact_bytes {
            // The append itself is durable, so a failed compaction is not this call's error: the
            // original log is intact and the tail still parses. The one failure that does matter
            // is the compacted image being too large to open, because then the writer would go on
            // producing a file its own next `open` rejects -- the invariant this store exists to
            // keep. Fail closed (refuse new creates) rather than grow past the ceiling.
            if let Err(error) = self.compact_locked(&mut handle) {
                if matches!(error, StoreError::Implausible { .. }) {
                    self.poisoned.store(true, Ordering::Release);
                } else {
                    // Back off instead of retrying the whole rewrite on the very next append: the
                    // guard condition would otherwise stay true forever and every create would
                    // pay a failed `create` + `write` + `sync` + `rename` sequence.
                    handle.next_compact_bytes = handle
                        .bytes
                        .saturating_mul(2)
                        .max(handle.bytes.saturating_add(4096));
                }
            }
        }
        Ok(())
    }

    /// Every live folded record, sorted by key.
    ///
    /// Compaction drops `Ended` / `Abandoned` keys, so a terminal session's key is absent here
    /// after a compaction even though the log mentioned it. Callers must not read this as "the log
    /// never saw it".
    pub fn entries(&self) -> Vec<Entry> {
        self.entries
            .read()
            .iter()
            .map(|(key, state)| Entry {
                key: key.clone(),
                state: state.clone(),
            })
            .collect()
    }

    /// The reservations that are still counted: exactly the reserved ones.
    pub fn pending(&self) -> Vec<Entry> {
        self.entries
            .read()
            .iter()
            .filter(|(_, state)| state.is_reserved())
            .map(|(key, state)| Entry {
                key: key.clone(),
                state: state.clone(),
            })
            .collect()
    }

    /// How many reservations admission must still charge against the limit.
    pub fn pending_count(&self) -> usize {
        self.entries
            .read()
            .values()
            .filter(|state| state.is_reserved())
            .count()
    }

    /// The folded state of one live key, if the log still holds it.
    ///
    /// `None` means "not live", not "never mentioned": compaction drops terminal keys.
    pub fn state_of(&self, key: &str) -> Option<ReservationState> {
        self.entries.read().get(key).cloned()
    }

    /// Durably records that `key` can never spawn, because the request never left this process.
    pub fn abandon_before_dispatch(&self, key: &str, detail: &str) -> Result<(), StoreError> {
        self.append(
            key,
            ReservationState::Abandoned {
                detail: detail.to_string(),
            },
        )
    }

    /// Records that the reservation became the live session `key`.
    pub fn bind_reservation(&self, key: &str) -> Result<(), StoreError> {
        self.append(
            key,
            ReservationState::Bound {
                session_id: key.to_string(),
            },
        )
    }

    pub fn end_session(&self, key: &str) -> Result<(), StoreError> {
        self.append(
            key,
            ReservationState::Ended {
                session_id: key.to_string(),
            },
        )
    }

    /// Whether a prior append left the tail unknown. While set, callers must refuse new creates.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned.load(Ordering::Acquire)
    }

    /// Bytes the current handle has appended since the file was created or last compacted.
    pub fn durable_bytes(&self) -> u64 {
        self.append.lock().bytes
    }

    /// The byte count at which the next append attempts compaction. Test-only: production reads it
    /// only through `append`, and the backoff it encodes is observable solely from outside.
    #[cfg(test)]
    fn next_compact_bytes(&self) -> u64 {
        self.append.lock().next_compact_bytes
    }

    /// Lowers the threshold compaction resets to, so a test can force compaction without filling
    /// the log. Sets the current handle's trigger too, since a fresh handle starts at the field.
    #[cfg(test)]
    fn set_compact_threshold(&mut self, bytes: u64) {
        self.compact_threshold = bytes;
        self.append.get_mut().next_compact_bytes = bytes;
    }

    /// Forces the handle's byte count, so the ceiling gate can be exercised without writing
    /// `MAX_STORE_BYTES` to the disk.
    #[cfg(test)]
    fn set_append_bytes(&self, bytes: u64) {
        self.append.lock().bytes = bytes;
    }

    /// Rewrites the log as one frame per folded key, dropping superseded history.
    ///
    /// Holds the append mutex for the whole operation, so no append can land in the gap between
    /// the rewrite and the handle swap. Failures before the rename leave the original log and the
    /// original handle untouched; a failure to reopen after the rename poisons the store, because
    /// the old handle would otherwise keep appending to an unlinked inode and those records would
    /// vanish on restart.
    pub fn compact(&self) -> Result<(), StoreError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(StoreError::Poisoned);
        }
        let mut handle = self.append.lock();
        self.compact_locked(&mut handle)
    }

    /// Compaction body; the caller holds the append mutex. Terminal (`Ended` / `Abandoned`)
    /// entries are dropped so ordinary session churn cannot grow the log without bound.
    fn compact_locked(&self, handle: &mut AppendHandle) -> Result<(), StoreError> {
        let folded: BTreeMap<String, ReservationState> = self
            .entries
            .read()
            .iter()
            .filter(|(_, state)| {
                !matches!(
                    state,
                    ReservationState::Ended { .. } | ReservationState::Abandoned { .. }
                )
            })
            .map(|(key, state)| (key.clone(), state.clone()))
            .collect();
        let mut bytes = Vec::with_capacity(MAGIC.len());
        bytes.extend_from_slice(MAGIC);
        for (key, state) in &folded {
            let payload = self.encode(key, state)?;
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&crc32(&payload).to_le_bytes());
            bytes.extend_from_slice(&payload);
        }
        if bytes.len() > MAX_STORE_BYTES {
            return Err(StoreError::Implausible {
                offset: 0,
                detail: "compacted log exceeds MAX_STORE_BYTES",
            });
        }

        let tmp = self
            .path
            .with_extension(format!("tmp.{}", std::process::id()));
        {
            let mut file = std::fs::File::create(&tmp).map_err(|e| io_err("create_tmp", e))?;
            file.write_all(&bytes)
                .map_err(|e| io_err("write_tmp", e))?;
            file.sync_all().map_err(|e| io_err("sync_tmp", e))?;
        }
        if let Err(error) = std::fs::rename(&tmp, &self.path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(io_err("rename", error));
        }

        // The old handle now points at an unlinked inode: until this succeeds, appends would be
        // lost, so a failure here is a poisoned store rather than a retryable error.
        match std::fs::OpenOptions::new().append(true).open(&self.path) {
            Ok(file) => {
                handle.file = file;
                handle.bytes = bytes.len() as u64;
            }
            Err(error) => {
                self.poisoned.store(true, Ordering::Release);
                return Err(io_err("reopen_after_compact", error));
            }
        }
        // Keep the in-memory view equal to what a restart would load.
        *self.entries.write() = folded;
        // The log is small again, so the next append may compact at the configured threshold.
        handle.next_compact_bytes = self.compact_threshold;

        // The rename created a new directory entry; make it durable. The handle is already
        // correct, so this failure costs durability of the rename, not the log's contents.
        let parent = self.path.parent().map(Path::to_path_buf);
        sync_dir(parent.as_deref())
    }

    fn encode(&self, key: &str, state: &ReservationState) -> Result<Vec<u8>, StoreError> {
        let entry = Entry {
            key: key.to_string(),
            state: state.clone(),
        };
        let payload = serde_json::to_vec(&entry).map_err(|e| StoreError::Encode(e.to_string()))?;
        if payload.is_empty() || payload.len() > MAX_PAYLOAD_BYTES {
            return Err(StoreError::Implausible {
                offset: 0,
                detail: "record payload past MAX_PAYLOAD_BYTES",
            });
        }
        Ok(payload)
    }
}

fn io_err(op: &'static str, error: std::io::Error) -> StoreError {
    StoreError::Io {
        op,
        message: error.to_string(),
    }
}

/// The budget's ceiling, copied so a refusal names the same limit the budget enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionCeiling {
    pub limit: usize,
    pub budget_bytes: usize,
    pub min_session_bytes: usize,
}

impl AdmissionCeiling {
    pub fn from_budget(budget: &super::scrollback_budget::ScrollbackBudget) -> Self {
        Self {
            limit: budget.admission_limit(),
            budget_bytes: budget.budget_bytes(),
            min_session_bytes: budget.min_session_bytes(),
        }
    }

    fn capacity_error(&self) -> PtyError {
        PtyError::SessionCapacityExhausted {
            limit: self.limit,
            budget_bytes: self.budget_bytes,
            min_session_bytes: self.min_session_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReservationOutcome {
    /// Charged and durable: the caller may now dispatch.
    Reserved,
    /// Already charged, so a retry is never denied by the cap it already occupies.
    AlreadyCharged,
}

#[derive(Debug)]
pub enum AdmissionRefusal {
    Capacity(PtyError),
    StoreUnusable(StoreError),
}

impl AdmissionRefusal {
    pub fn limit(&self) -> Option<usize> {
        match self {
            Self::Capacity(PtyError::SessionCapacityExhausted { limit, .. }) => Some(*limit),
            _ => None,
        }
    }

    pub fn is_store_unusable(&self) -> bool {
        matches!(self, Self::StoreUnusable(_))
    }
}

impl std::fmt::Display for AdmissionRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Capacity(error) => write!(f, "{error}"),
            Self::StoreUnusable(error) => write!(f, "ADMISSION_REFUSED_STORE_UNUSABLE: {error}"),
        }
    }
}

impl std::error::Error for AdmissionRefusal {}

impl From<StoreError> for AdmissionRefusal {
    fn from(error: StoreError) -> Self {
        Self::StoreUnusable(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecoveredAccounting {
    pub counted: usize,
    pub bound_reservations: usize,
}

/// Which session ids occupy an admission slot. A set, not a counter: see the module header.
#[derive(Debug)]
pub struct AdmissionLedger {
    store: AdmissionStore,
    charged: RwLock<BTreeSet<String>>,
    /// Serialises check-then-append across concurrent creates.
    reserve_gate: Mutex<()>,
}

impl AdmissionLedger {
    /// Opens the ledger, seeding the charge from the log; corruption is returned, never papered
    /// over, because a caller that continues would admit against an unknown reservation count.
    pub fn open(path: &Path) -> Result<Arc<Self>, StoreError> {
        let store = AdmissionStore::open(path)?;
        let charged: BTreeSet<String> = store
            .entries()
            .into_iter()
            .filter(|entry| entry.state.occupies_slot())
            .map(|entry| entry.key)
            .collect();
        Ok(Arc::new(Self {
            store,
            charged: RwLock::new(charged),
            reserve_gate: Mutex::new(()),
        }))
    }

    pub fn store(&self) -> &AdmissionStore {
        &self.store
    }

    pub fn charged(&self) -> usize {
        self.charged.read().len()
    }

    pub fn pending(&self) -> usize {
        self.store.pending_count()
    }

    pub fn is_charged(&self, session_id: &str) -> bool {
        self.charged.read().contains(session_id)
    }

    /// Charges recovered sessions once each, never consulting the ceiling: a restart must not
    /// deny the running sessions it is recovering.
    pub fn note_recovered_sessions(
        &self,
        session_ids: &[String],
    ) -> Result<RecoveredAccounting, StoreError> {
        self.reject_if_poisoned()?;
        let mut accounting = RecoveredAccounting::default();
        for session_id in session_ids {
            if self.charged.write().insert(session_id.clone()) {
                accounting.counted += 1;
            }
            // Binding is separate from counting: a reservation already seeded from the log is not
            // newly charged, but it must still be bound or its slot could never be released.
            //
            // `Bound` is also the only state a reopen seeds a charge from, and the only state that
            // releases a slot later. So a recovered session must be bound even when it has no frame
            // at all: leaving it unrecorded would keep its charge in this process only, and the
            // next startup would seed nothing from the log and charge the same pane a second time.
            let bindable = match self.store.state_of(session_id) {
                // No frame at all: the session is running now, which is what `Bound` attests.
                None => true,
                // A reservation that was never acknowledged is adopted here, as before.
                Some(state) => state.is_reserved(),
            };
            if bindable && self.store.bind_reservation(session_id).is_ok() {
                accounting.bound_reservations += 1;
            }
        }
        Ok(accounting)
    }

    /// Charges one slot durably; the caller may dispatch only after `Ok`.
    pub fn reserve(
        &self,
        session_id: &str,
        ceiling: AdmissionCeiling,
    ) -> Result<ReservationOutcome, AdmissionRefusal> {
        self.reserve_with_request(session_id, None, ceiling)
    }

    /// [`Self::reserve`] for a spawn that will be dispatched to the keeper: `request_id` is the
    /// **logical** spawn id, minted once per spawn and reused on every retry, and it is durable
    /// before this returns `Ok`.
    ///
    /// A retry of an already-charged session keeps the id the first attempt recorded. It is never
    /// replaced with a new one, because a second id would let a delayed duplicate of the first
    /// `SPAWN` fork unretired; a plain [`ReservationState::Reserved`] is upgraded in place when an
    /// id arrives late, which is the one write that improves durability without adding a charge.
    pub fn reserve_with_request(
        &self,
        session_id: &str,
        request_id: Option<&str>,
        ceiling: AdmissionCeiling,
    ) -> Result<ReservationOutcome, AdmissionRefusal> {
        let _gate = self.reserve_gate.lock();
        self.reject_if_poisoned().map_err(AdmissionRefusal::StoreUnusable)?;
        if self.is_charged(session_id) {
            if let Some(request_id) = request_id {
                if self.store.state_of(session_id) == Some(ReservationState::Reserved) {
                    self.store
                        .append(
                            session_id,
                            ReservationState::ReservedWithRequest {
                                request_id: request_id.to_string(),
                            },
                        )
                        .map_err(AdmissionRefusal::StoreUnusable)?;
                }
            }
            return Ok(ReservationOutcome::AlreadyCharged);
        }
        if self.charged() >= ceiling.limit {
            return Err(AdmissionRefusal::Capacity(ceiling.capacity_error()));
        }
        let state = match request_id {
            Some(request_id) => ReservationState::ReservedWithRequest {
                request_id: request_id.to_string(),
            },
            None => ReservationState::Reserved,
        };
        self.store
            .append(session_id, state)
            .map_err(AdmissionRefusal::StoreUnusable)?;
        self.charged.write().insert(session_id.to_string());
        Ok(ReservationOutcome::Reserved)
    }

    /// The logical spawn request id durably recorded for `session_id`, when it has one.
    ///
    /// This is the value a retried `SPAWN` must repeat. It is `None` for a session reserved
    /// without one (a direct PTY), for one that never reached the log, and after the reservation
    /// is bound, ended or abandoned.
    pub fn request_id_for(&self, session_id: &str) -> Option<String> {
        self.store
            .state_of(session_id)
            .and_then(|state| state.request_id().map(str::to_string))
    }

    pub fn bind(&self, session_id: &str) -> Result<(), StoreError> {
        if matches!(
            self.store.state_of(session_id),
            Some(ReservationState::Bound { .. })
        ) {
            return Ok(());
        }
        self.store.bind_reservation(session_id)?;
        self.charged.write().insert(session_id.to_string());
        Ok(())
    }

    /// A session ended: its slot is released only from a state that proves no request is pending.
    pub fn end_session(&self, session_id: &str) -> Result<(), StoreError> {
        match self.store.state_of(session_id) {
            Some(ReservationState::Bound { .. }) => self.store.end_session(session_id)?,
            // Still reserved (with or without a request id): the spawn was never acknowledged, so
            // the request may still land. The charge stays, and capacity leaks rather than
            // over-admitting.
            Some(ReservationState::Reserved | ReservationState::ReservedWithRequest { .. }) => {
                return Ok(())
            }
            Some(ReservationState::Ended { .. } | ReservationState::Abandoned { .. }) | None => {
                return Ok(())
            }
        }
        self.charged.write().remove(session_id);
        Ok(())
    }

    /// Releases a slot, valid **only** when the request never left this process. A refusal, a
    /// timeout or an `UNKNOWN_SESSION` after dispatch leaves it charged, deliberately.
    pub fn abandon_before_dispatch(
        &self,
        session_id: &str,
        detail: &str,
    ) -> Result<(), StoreError> {
        self.store.abandon_before_dispatch(session_id, detail)?;
        self.charged.write().remove(session_id);
        Ok(())
    }

    fn reject_if_poisoned(&self) -> Result<(), StoreError> {
        if self.store.is_poisoned() {
            return Err(StoreError::Poisoned);
        }
        Ok(())
    }
}

pub const ADMISSION_FILE_NAME: &str = "pty-admission.log";

/// Where this runtime's admission log lives.
///
/// It sits beside the durable session state (`OwnershipStore`), not in the app bundle: a daemon
/// restarted after a crash finds the reservations of the attempt it died in.
pub fn admission_path_for(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(ADMISSION_FILE_NAME)
}

/// Opens the ledger; the error is the caller's signal to refuse admission, not to retry.
pub fn open_admission(path: &Path) -> Result<Arc<AdmissionLedger>, StoreError> {
    AdmissionLedger::open(path)
}

/// Flushes a directory entry. Unix needs the directory itself opened and synced; Windows has no
/// directory handle to sync, and the rename's own flush is what the platform offers.
fn sync_dir(parent: Option<&Path>) -> Result<(), StoreError> {
    #[cfg(unix)]
    if let Some(parent) = parent {
        let dir = std::fs::File::open(parent).map_err(|e| io_err("open_dir", e))?;
        dir.sync_all().map_err(|e| io_err("sync_dir", e))?;
    }
    let _ = parent;
    Ok(())
}

/// Folds the whole log, failing closed on any damage. Never truncates, never skips a bad frame:
/// the frames after a tear are unreachable, so a tear means the count is unknown.
fn decode(bytes: &[u8]) -> Result<BTreeMap<String, ReservationState>, StoreError> {
    if bytes.len() < MAGIC.len() {
        return Err(StoreError::Corrupt {
            offset: 0,
            detail: "truncated magic",
        });
    }
    if &bytes[..MAGIC.len()] != MAGIC {
        return Err(StoreError::BadMagic);
    }

    let mut folded = BTreeMap::new();
    let mut offset = MAGIC.len();
    while offset < bytes.len() {
        if bytes.len() - offset < FRAME_HEADER_BYTES {
            return Err(StoreError::Corrupt {
                offset,
                detail: "truncated frame header",
            });
        }
        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&bytes[offset..offset + 4]);
        let len = u32::from_le_bytes(len_bytes) as usize;
        let mut crc_bytes = [0u8; 4];
        crc_bytes.copy_from_slice(&bytes[offset + 4..offset + 8]);
        let expected = u32::from_le_bytes(crc_bytes);
        offset += FRAME_HEADER_BYTES;

        if len == 0 || len > MAX_PAYLOAD_BYTES {
            return Err(StoreError::Implausible {
                offset,
                detail: "frame length outside 1..=MAX_PAYLOAD_BYTES",
            });
        }
        if bytes.len() - offset < len {
            return Err(StoreError::Corrupt {
                offset,
                detail: "truncated frame body",
            });
        }
        let payload = &bytes[offset..offset + len];
        if crc32(payload) != expected {
            return Err(StoreError::Corrupt {
                offset,
                detail: "frame checksum mismatch",
            });
        }
        let entry: Entry = serde_json::from_slice(payload).map_err(|_| StoreError::Corrupt {
            offset,
            detail: "undecodable frame",
        })?;
        folded.insert(entry.key, entry.state);
        offset += len;
    }
    Ok(folded)
}

const fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0usize;
    while index < 256 {
        let mut crc = index as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
}

static CRC32_TABLE: [u32; 256] = crc32_table();

/// CRC-32/ISO-HDLC. In-file so the store adds no dependency.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        let index = ((crc ^ *byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[index];
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn temp_path(name: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(name);
        (dir, path)
    }

    fn framed(key: &str, state: &ReservationState) -> Vec<u8> {
        let payload = serde_json::to_vec(&Entry {
            key: key.to_string(),
            state: state.clone(),
        })
        .expect("encode");
        let mut out = Vec::new();
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&crc32(&payload).to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    fn write_log(path: &Path, frames: &[Vec<u8>]) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        for frame in frames {
            bytes.extend_from_slice(frame);
        }
        std::fs::write(path, bytes).expect("write log");
    }

    #[test]
    fn open_creates_missing_log_and_persists_the_first_reservation() {
        let (_dir, path) = temp_path("admission.log");
        {
            let store = AdmissionStore::open(&path).expect("open");
            assert_eq!(store.pending_count(), 0);
            store
                .append("req-1", ReservationState::Reserved)
                .expect("append");
            assert_eq!(store.pending_count(), 1);
        }
        let reopened = AdmissionStore::open(&path).expect("reopen");
        assert_eq!(reopened.pending_count(), 1);
        assert_eq!(
            reopened.state_of("req-1"),
            Some(ReservationState::Reserved)
        );
    }

    #[test]
    fn an_unresolved_reservation_survives_a_crash_and_stays_counted() {
        let (_dir, path) = temp_path("admission.log");
        let store = AdmissionStore::open(&path).expect("open");
        // No transition is ever recorded: this is the daemon dying mid-spawn.
        store
            .append("req-unknown", ReservationState::Reserved)
            .expect("append");
        // Deliberately not dropped, not unlinked, no cleanup: the bytes are the evidence.
        drop(store);

        let restarted = AdmissionStore::open(&path).expect("reopen");
        assert_eq!(restarted.pending_count(), 1, "uncertainty stays counted");
        assert!(restarted.pending().iter().any(|e| e.key == "req-unknown"));
    }

    #[test]
    fn only_adoption_removes_a_slot_from_the_pending_count() {
        let (_dir, path) = temp_path("admission.log");
        let store = AdmissionStore::open(&path).expect("open");
        store
            .append("req-1", ReservationState::Reserved)
            .expect("reserve");
        store
            .append("req-2", ReservationState::Reserved)
            .expect("reserve");
        store
            .append(
                "req-2",
                ReservationState::Bound {
                    session_id: "sess-2".to_string(),
                },
            )
            .expect("bind");
        assert_eq!(store.pending_count(), 1);
        drop(store);

        let reopened = AdmissionStore::open(&path).expect("reopen");
        assert_eq!(reopened.pending_count(), 1, "the unbound slot stays counted");
        assert!(reopened.pending().iter().any(|e| e.key == "req-1"));
    }

    #[test]
    fn bound_key_stops_being_pending_and_keeps_its_session_id() {
        let (_dir, path) = temp_path("admission.log");
        let store = AdmissionStore::open(&path).expect("open");
        store
            .append("req-1", ReservationState::Reserved)
            .expect("reserve");
        store
            .append(
                "req-1",
                ReservationState::Bound {
                    session_id: "sess-9".to_string(),
                },
            )
            .expect("bind");
        assert_eq!(store.pending_count(), 0);
        drop(store);

        let reopened = AdmissionStore::open(&path).expect("reopen");
        assert_eq!(
            reopened.state_of("req-1"),
            Some(ReservationState::Bound {
                session_id: "sess-9".to_string()
            })
        );
    }

    #[test]
    fn last_state_wins_per_key() {
        let (_dir, path) = temp_path("admission.log");
        let store = AdmissionStore::open(&path).expect("open");
        for _ in 0..3 {
            store
                .append("req-1", ReservationState::Reserved)
                .expect("reserve");
        }
        assert_eq!(store.pending_count(), 1, "repeats fold onto one key");
        assert_eq!(store.entries().len(), 1);
    }

    /// The case the previous design got wrong: many reservists, one limit. The store's job is that
    /// no append is lost, so the caller's atomic check-and-increment sees every reservation.
    #[test]
    fn concurrent_appends_lose_no_records() {
        let (_dir, path) = temp_path("admission.log");
        let store = Arc::new(AdmissionStore::open(&path).expect("open"));
        let mut handles = Vec::new();
        for index in 0..32 {
            let store = Arc::clone(&store);
            handles.push(std::thread::spawn(move || {
                store
                    .append(&format!("req-{index}"), ReservationState::Reserved)
                    .expect("append");
            }));
        }
        for handle in handles {
            handle.join().expect("join");
        }
        assert_eq!(store.pending_count(), 32);
        drop(store);

        let reopened = AdmissionStore::open(&path).expect("reopen");
        assert_eq!(reopened.pending_count(), 32, "no record lost on disk");
        assert_eq!(reopened.entries().len(), 32);
    }

    #[test]
    fn a_torn_body_fails_closed_and_is_never_truncated() {
        let (_dir, path) = temp_path("admission.log");
        let mut frame = framed("req-1", &ReservationState::Reserved);
        frame.truncate(frame.len() - 3);
        write_log(&path, &[frame]);
        let before = std::fs::read(&path).expect("read");

        let error = AdmissionStore::open(&path).expect_err("a tear must refuse to open");
        assert!(matches!(error, StoreError::Corrupt { .. }), "{error}");

        let after = std::fs::read(&path).expect("read");
        assert_eq!(before, after, "open must not repair a damaged log");
    }

    #[test]
    fn a_short_header_fails_closed() {
        let (_dir, path) = temp_path("admission.log");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&[1, 2, 3]);
        std::fs::write(&path, bytes).expect("write");

        let error = AdmissionStore::open(&path).expect_err("short header must refuse");
        assert!(matches!(error, StoreError::Corrupt { .. }), "{error}");
    }

    #[test]
    fn a_checksum_mismatch_fails_closed() {
        let (_dir, path) = temp_path("admission.log");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        let frame = framed("req-1", &ReservationState::Reserved);
        let mut corrupted = frame.clone();
        let last = corrupted.len() - 1;
        corrupted[last] ^= 0xFF;
        bytes.extend_from_slice(&corrupted);
        std::fs::write(&path, bytes).expect("write");

        let error = AdmissionStore::open(&path).expect_err("bad checksum must refuse");
        assert!(matches!(error, StoreError::Corrupt { .. }), "{error}");
    }

    #[test]
    fn a_bad_magic_fails_closed() {
        let (_dir, path) = temp_path("admission.log");
        std::fs::write(&path, b"NOTALOG\x01\0\0\0\0").expect("write");
        assert_eq!(
            AdmissionStore::open(&path).expect_err("foreign file"),
            StoreError::BadMagic
        );
    }

    #[test]
    fn an_impossible_length_fails_closed() {
        let (_dir, path) = temp_path("admission.log");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"{}");
        std::fs::write(&path, bytes).expect("write");

        let error = AdmissionStore::open(&path).expect_err("impossible length must refuse");
        assert!(matches!(error, StoreError::Implausible { .. }), "{error}");
    }

    #[test]
    fn a_damaged_frame_before_the_tail_hides_the_successor_and_still_refuses() {
        let (_dir, path) = temp_path("admission.log");
        let good = framed("req-1", &ReservationState::Reserved);
        let mut corrupted = good.clone();
        corrupted[FRAME_HEADER_BYTES] ^= 0xFF;
        // A second reservation sits after the damage; it must not be silently adopted either.
        write_log(
            &path,
            &[corrupted, framed("req-2", &ReservationState::Reserved)],
        );

        let error = AdmissionStore::open(&path).expect_err("damage refuses wholly");
        assert!(matches!(error, StoreError::Corrupt { .. }), "{error}");
    }

    #[test]
    fn an_oversized_record_is_refused_without_entering_the_log() {
        let (_dir, path) = temp_path("admission.log");
        let store = AdmissionStore::open(&path).expect("open");
        let huge = "k".repeat(MAX_PAYLOAD_BYTES + 1);
        let error = store
            .append(&huge, ReservationState::Reserved)
            .expect_err("oversized key must refuse");
        assert!(matches!(error, StoreError::Implausible { .. }), "{error}");
        assert_eq!(store.pending_count(), 0);
        assert!(!store.is_poisoned(), "a refused record leaves the log writable");
    }

    #[test]
    fn compact_folds_history_and_keeps_the_count() {
        let (_dir, path) = temp_path("admission.log");
        let store = AdmissionStore::open(&path).expect("open");
        for index in 0..4 {
            store
                .append(&format!("req-{index}"), ReservationState::Reserved)
                .expect("reserve");
        }
        store
            .append(
                "req-0",
                ReservationState::Bound {
                    session_id: "sess-0".to_string(),
                },
            )
            .expect("bind");
        let before = store.durable_bytes();
        store.compact().expect("compact");
        assert!(
            store.durable_bytes() < before,
            "compaction drops superseded frames"
        );
        assert_eq!(store.pending_count(), 3);

        drop(store);
        let reopened = AdmissionStore::open(&path).expect("reopen");
        assert_eq!(reopened.pending_count(), 3);
        assert_eq!(reopened.entries().len(), 4);
    }

    #[test]
    fn compaction_drops_terminal_entries_and_append_auto_compacts() {
        let (_dir, path) = temp_path("admission.log");
        let mut store = AdmissionStore::open(&path).expect("open");
        store.set_compact_threshold(512);
        store
            .append("live", ReservationState::Reserved)
            .expect("live");
        for index in 0..64 {
            let key = format!("churn-{index}");
            store.append(&key, ReservationState::Reserved).expect("r");
            store
                .append(
                    &key,
                    ReservationState::Ended {
                        session_id: format!("s-{index}"),
                    },
                )
                .expect("end");
        }
        assert!(store.durable_bytes() < 1024, "churn stays bounded");
        assert!(store.state_of("live").is_some());
        drop(store);
        let reopened = AdmissionStore::open(&path).expect("reopen");
        assert_eq!(reopened.pending_count(), 1);
    }

    #[test]
    fn a_failed_compaction_backs_off_instead_of_rewriting_on_every_append() {
        // R2-F2b-01: `append` retried the whole `create` + `write` + `sync` + `rename` sequence on
        // *every* subsequent append while the guard condition stayed true. A persistent ENOSPC on
        // the staging path turned each create into a failed rewrite of the folded log.
        let (_dir, path) = temp_path("admission.log");
        let mut store = AdmissionStore::open(&path).expect("open");
        store.set_compact_threshold(0);
        // A directory where compaction's staging file goes: `File::create` fails with a persistent
        // error that is not `Implausible`, which is the shape of the case this guards.
        let blocked = path.with_extension(format!("tmp.{}", std::process::id()));
        std::fs::create_dir_all(&blocked).expect("block the staging path");

        store
            .append("req-0", ReservationState::Reserved)
            .expect("append");
        let after_first = store.next_compact_bytes();
        assert!(
            after_first > 0,
            "a failed compaction must raise the next attempt's threshold"
        );
        assert!(
            !store.is_poisoned(),
            "a retryable failure must not poison the store"
        );

        // The next append sits below the new threshold, so no compaction is attempted at all.
        let before = store.durable_bytes();
        store
            .append("req-1", ReservationState::Reserved)
            .expect("append");
        assert!(store.durable_bytes() > before, "the append itself still lands");
        assert_eq!(
            store.next_compact_bytes(),
            after_first,
            "a skipped attempt leaves the backoff where it was"
        );

        // Clearing the obstruction lets compaction succeed, and success resets the threshold.
        std::fs::remove_dir_all(&blocked).expect("clear the staging path");
        store.compact().expect("compaction succeeds once the path is usable");
        assert_eq!(
            store.next_compact_bytes(),
            store.compact_threshold,
            "a successful compaction resets the backoff"
        );
        assert_eq!(store.pending_count(), 2);
    }

    #[test]
    fn an_append_that_would_cross_the_ceiling_poisons_before_writing() {
        // R2-F2b-01: nothing forced a poison at the ceiling, so a store whose compactions kept
        // failing could still write a file its own next `open` rejects.
        let (_dir, path) = temp_path("admission.log");
        let store = AdmissionStore::open(&path).expect("open");
        store.set_append_bytes(MAX_STORE_BYTES as u64 - 1);

        let error = store
            .append("req-ceiling", ReservationState::Reserved)
            .expect_err("an append past the ceiling must be refused");
        assert!(matches!(error, StoreError::Implausible { .. }), "{error}");
        assert!(
            store.is_poisoned(),
            "the writer must refuse rather than produce a log `open` rejects"
        );
        assert_eq!(store.pending_count(), 0, "the refused record is not folded in");
        AdmissionStore::open(&path).expect("what reached the disk is still openable");
    }

    #[test]
    fn open_rejects_an_oversized_log_without_decoding() {
        let (_dir, path) = temp_path("admission.log");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(MAX_STORE_BYTES as u64 + 1).expect("len");
        drop(file);
        assert!(matches!(
            AdmissionStore::open(&path),
            Err(StoreError::Implausible { .. })
        ));
    }

    #[test]
    fn appends_after_compaction_survive_a_reload() {
        let (_dir, path) = temp_path("admission.log");
        let store = AdmissionStore::open(&path).expect("open");
        store
            .append("req-1", ReservationState::Reserved)
            .expect("reserve");
        store.compact().expect("compact");
        store
            .append("req-2", ReservationState::Reserved)
            .expect("append after compact");
        drop(store);

        let reopened = AdmissionStore::open(&path).expect("reopen");
        assert_eq!(reopened.pending_count(), 2);
        assert!(reopened.state_of("req-2").is_some());
    }

    #[test]
    fn crc32_matches_the_reference_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    fn ceiling(limit: usize) -> AdmissionCeiling {
        AdmissionCeiling {
            limit,
            budget_bytes: 1024 * 1024,
            min_session_bytes: 1024,
        }
    }

    #[test]
    fn a_recovered_session_is_counted_once_across_repeated_startups() {
        let (_dir, path) = temp_path("admission.log");
        let recovered = vec!["pane-a".to_string(), "pane-b".to_string()];

        let first = AdmissionLedger::open(&path).expect("open");
        let accounting = first
            .note_recovered_sessions(&recovered)
            .expect("recover");
        assert_eq!(accounting.counted, 2);
        assert_eq!(first.charged(), 2);
        drop(first);

        let second = AdmissionLedger::open(&path).expect("reopen");
        let accounting = second
            .note_recovered_sessions(&recovered)
            .expect("recover again");
        assert_eq!(accounting.counted, 0, "a repeated startup must not charge twice");
        assert_eq!(second.charged(), 2);
    }

    #[test]
    fn a_running_session_is_recovered_past_a_full_ceiling() {
        let (_dir, path) = temp_path("admission.log");
        let ledger = AdmissionLedger::open(&path).expect("open");
        ledger
            .reserve("pane-a", ceiling(1))
            .expect("first slot");
        assert!(ledger.reserve("pane-b", ceiling(1)).is_err());

        // The keeper already runs both, so neither may be denied by the cap they help fill.
        let accounting = ledger
            .note_recovered_sessions(&["pane-a".to_string(), "pane-b".to_string()])
            .expect("recover");
        assert_eq!(accounting.counted, 1);
        assert_eq!(ledger.charged(), 2);
    }

    #[test]
    fn a_reservation_outstanding_at_restart_stays_charged() {
        let (_dir, path) = temp_path("admission.log");
        let ledger = AdmissionLedger::open(&path).expect("open");
        ledger.reserve("pane-a", ceiling(4)).expect("reserve");
        drop(ledger);

        // Nothing proved the request died, so a reopen must not free the slot: freeing it would
        // admit a session that a delayed duplicate may already have forked.
        let reopened = AdmissionLedger::open(&path).expect("reopen");
        assert!(reopened.is_charged("pane-a"));
    }

    #[test]
    fn a_bound_session_releases_its_slot_and_an_unanswered_one_does_not() {
        let (_dir, path) = temp_path("admission.log");
        let ledger = AdmissionLedger::open(&path).expect("open");
        ledger.reserve("pane-bound", ceiling(4)).expect("reserve");
        ledger.bind("pane-bound").expect("bind");
        ledger.end_session("pane-bound").expect("end");
        assert!(!ledger.is_charged("pane-bound"));

        ledger.reserve("pane-pending", ceiling(4)).expect("reserve");
        ledger.end_session("pane-pending").expect("end");
        assert!(
            ledger.is_charged("pane-pending"),
            "an unacknowledged reservation may still fork, so it keeps its slot"
        );
    }

    #[test]
    fn abandoning_before_dispatch_frees_the_slot() {
        let (_dir, path) = temp_path("admission.log");
        let ledger = AdmissionLedger::open(&path).expect("open");
        ledger.reserve("pane-a", ceiling(1)).expect("reserve");
        ledger
            .abandon_before_dispatch("pane-a", "refused before dispatch")
            .expect("abandon");
        assert_eq!(ledger.charged(), 0);
        ledger.reserve("pane-b", ceiling(1)).expect("the freed slot admits");
    }

    #[test]
    fn a_retry_of_a_charged_session_is_never_denied_by_its_own_slot() {
        let (_dir, path) = temp_path("admission.log");
        let ledger = AdmissionLedger::open(&path).expect("open");
        assert_eq!(
            ledger.reserve("pane-a", ceiling(1)).expect("reserve"),
            ReservationOutcome::Reserved
        );
        assert_eq!(
            ledger.reserve("pane-a", ceiling(1)).expect("retry"),
            ReservationOutcome::AlreadyCharged
        );
        assert_eq!(ledger.charged(), 1);
    }

    #[test]
    fn a_corrupt_log_refuses_admission_instead_of_starting_from_zero() {
        let (_dir, path) = temp_path("admission.log");
        AdmissionStore::open(&path)
            .expect("open")
            .append("pane-a", ReservationState::Reserved)
            .expect("reserve");
        let mut raw = std::fs::read(&path).expect("read");
        let last = raw.len() - 1;
        raw[last] ^= 0xff;
        std::fs::write(&path, &raw).expect("damage");

        assert!(
            AdmissionLedger::open(&path).is_err(),
            "an unreadable log hides the true count, so the caller must fail closed"
        );
    }

    #[test]
    fn admission_path_for_places_the_log_beside_the_runtime_state() {
        let dir = Path::new("/tmp/ferryx-runtime");
        assert_eq!(
            admission_path_for(dir),
            dir.join(ADMISSION_FILE_NAME),
            "the log must live in the runtime dir the daemon already reconciles"
        );
    }

    #[test]
    fn a_request_id_round_trips_through_a_reopen() {
        let (_dir, path) = temp_path("admission.log");
        let ledger = AdmissionLedger::open(&path).expect("open");
        ledger
            .reserve_with_request("pane-a", Some("spawn-7"), ceiling(4))
            .expect("reserve");
        assert_eq!(ledger.request_id_for("pane-a").as_deref(), Some("spawn-7"));
        drop(ledger);

        // The id has to survive the crash it exists for: a retry after restart must be able to
        // present the same logical spawn id to the keeper.
        let reopened = AdmissionLedger::open(&path).expect("reopen");
        assert_eq!(reopened.request_id_for("pane-a").as_deref(), Some("spawn-7"));
        assert!(reopened.is_charged("pane-a"), "the slot is still unanswered");
        assert_eq!(reopened.pending(), 1);
    }

    #[test]
    fn a_retry_keeps_the_first_request_id_and_never_charges_twice() {
        let (_dir, path) = temp_path("admission.log");
        let ledger = AdmissionLedger::open(&path).expect("open");
        ledger
            .reserve_with_request("pane-a", Some("spawn-first"), ceiling(1))
            .expect("reserve");
        // A retry that mints a second id would let a delayed duplicate of the first fork.
        assert_eq!(
            ledger
                .reserve_with_request("pane-a", Some("spawn-second"), ceiling(1))
                .expect("retry"),
            ReservationOutcome::AlreadyCharged
        );
        assert_eq!(ledger.request_id_for("pane-a").as_deref(), Some("spawn-first"));
        assert_eq!(ledger.charged(), 1);
    }

    #[test]
    fn a_plain_reservation_is_upgraded_in_place_when_an_id_arrives() {
        let (_dir, path) = temp_path("admission.log");
        let ledger = AdmissionLedger::open(&path).expect("open");
        ledger.reserve("pane-a", ceiling(4)).expect("reserve");
        assert_eq!(ledger.request_id_for("pane-a"), None);

        assert_eq!(
            ledger
                .reserve_with_request("pane-a", Some("spawn-late"), ceiling(4))
                .expect("upgrade"),
            ReservationOutcome::AlreadyCharged
        );
        assert_eq!(ledger.request_id_for("pane-a").as_deref(), Some("spawn-late"));
        assert_eq!(ledger.charged(), 1, "upgrading the record adds no charge");
        drop(ledger);

        let reopened = AdmissionLedger::open(&path).expect("reopen");
        assert_eq!(reopened.request_id_for("pane-a").as_deref(), Some("spawn-late"));
    }

    #[test]
    fn an_unanswered_reservation_with_a_request_id_keeps_its_slot_after_a_restart() {
        let (_dir, path) = temp_path("admission.log");
        let ledger = AdmissionLedger::open(&path).expect("open");
        ledger
            .reserve_with_request("pane-a", Some("spawn-1"), ceiling(4))
            .expect("reserve");
        // A session end cannot release a reservation that still carries an unanswered request.
        ledger.end_session("pane-a").expect("end");
        assert!(ledger.is_charged("pane-a"));
        drop(ledger);

        let reopened = AdmissionLedger::open(&path).expect("reopen");
        assert!(reopened.is_charged("pane-a"));
        assert_eq!(reopened.request_id_for("pane-a").as_deref(), Some("spawn-1"));
    }

    #[test]
    fn a_bound_reservation_no_longer_reports_a_request_id() {
        let (_dir, path) = temp_path("admission.log");
        let ledger = AdmissionLedger::open(&path).expect("open");
        ledger
            .reserve_with_request("pane-a", Some("spawn-1"), ceiling(4))
            .expect("reserve");
        ledger.bind("pane-a").expect("bind");
        assert_eq!(
            ledger.request_id_for("pane-a"),
            None,
            "an acknowledged spawn has no retry to protect"
        );
        // Bound still occupies a slot; only ending the session frees it.
        assert!(ledger.is_charged("pane-a"));
        ledger.end_session("pane-a").expect("end");
        assert!(!ledger.is_charged("pane-a"));
    }

    #[test]
    fn a_legacy_log_without_request_ids_still_decodes() {
        // A log written before request ids existed: plain `Reserved` frames only.
        let (_dir, path) = temp_path("admission.log");
        write_log(
            &path,
            &[
                framed("pane-a", &ReservationState::Reserved),
                framed(
                    "pane-b",
                    &ReservationState::Bound {
                        session_id: "pane-b".to_string(),
                    },
                ),
            ],
        );

        let ledger = AdmissionLedger::open(&path).expect("a legacy log still opens");
        assert_eq!(ledger.request_id_for("pane-a"), None);
        assert!(ledger.is_charged("pane-a"));
        assert_eq!(ledger.pending(), 1);
        assert_eq!(ledger.charged(), 2, "bound sessions keep their slot until they end");
        // The next write upgrades it in place rather than adding a second charge.
        ledger
            .reserve_with_request("pane-a", Some("spawn-new"), ceiling(4))
            .expect("upgrade a legacy reservation");
        assert_eq!(ledger.request_id_for("pane-a").as_deref(), Some("spawn-new"));
        assert_eq!(ledger.charged(), 2);
    }
}
