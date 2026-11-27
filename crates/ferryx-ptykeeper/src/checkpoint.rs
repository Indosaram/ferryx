//! Checkpoint transport.
//!
//! The keeper never parses terminal state. A checkpoint is bytes the DAEMON produced (it is the
//! component that understands escape sequences); the keeper only stores and hands them back. That
//! is what keeps this crate FFI-free while still letting a successor daemon restore a screen.
//!
//! Install is atomic: a chunked upload accumulates in memory charged against the aggregate cap, and
//! only a completed upload replaces the committed checkpoint. A budget refusal preserves whatever
//! was committed before, so a failed upload never costs the successor its recovery point.
//!
//! ## One store, keyed by session
//!
//! [`CheckpointStore`] holds every session's recovery point and every in-flight upload, keyed by
//! `session_id`, because that is how the keeper uses it: one store behind the keeper's single
//! checkpoint mutex, with the session passed into every call. A caller that names a session the
//! store has never seen, or a checkpoint id with no open upload, is refused with
//! [`KeeperErrorCode::UnknownCheckpointUpload`] rather than silently served an empty answer.
//!
//! ## Accounting
//!
//! Every byte is charged through the crate's [`MemoryBudget`], which the keeper shares with this
//! store: an upload's declared size is reserved in the pending bucket at `begin`, moved to the
//! committed bucket at `commit`, and returned on every abort path. [`CheckpointStore::pending_bytes`]
//! and [`CheckpointStore::committed_bytes`] therefore read the budget's own
//! `checkpoint_pending_bytes`/`checkpoint_committed_bytes` counters instead of keeping a second
//! copy that could drift from them.
//!
//! A charge is the *allocation*, never the logical length:
//!
//! * an upload is allocated once, at its declared size, and charged before that allocation is made.
//!   Chunks then fill the buffer without ever reallocating, so there is no growth step in which an
//!   old and a new allocation coexist and no capacity appears that the budget has not admitted.
//!   An allocator that rounds the reservation up is billed the difference at `begin`, and refused
//!   there if it cannot pay, which is the only moment a capacity figure can surprise this store;
//! * a committed checkpoint is charged at the capacity the store actually keeps, not the length it
//!   was handed: a caller's buffer carrying slack is billed for that slack instead of being kept at
//!   an unbilled size;
//! * a single-frame [`CheckpointStore::install`] admits the incoming buffer at its *peak*: those
//!   bytes are allocated before the call starts and the checkpoint they replace is still live, so
//!   both are charged at once. A replacement that cannot fit at its peak is refused with the
//!   previous recovery point intact.
//!
//! Nothing is released before the allocation behind it is freed: `install`, `commit`, `abort` and
//! `forget_session` drop the superseded buffer first and return its charge second, so the
//! accounting never claims a byte is free while the allocator still owns it. The budget lock is
//! held across that free and release, which makes each operation one atomic step for every other
//! budget user: a refusal leaves the accounting exactly as it found it.
//!
//! Retrieval is not charged because it retains nothing. [`CheckpointStore::read_chunk`] copies one
//! bounded slice straight into the caller's buffer, so a download holds no store memory of its own.
//!
//! ## Where the cap is exact, and where it is only a bound
//!
//! Chunked uploads are exact: `begin` charges the declared size *before* allocating, the buffer is
//! sized once to that declared total, and no chunk may exceed it, so no chunk allocates anything.
//! The one figure that cannot be known in advance is an allocator that rounds a reservation up:
//! the overshoot is billed the instant it is measured and refused if it cannot be paid, so a
//! process may hold at most that rounding as unbilled memory for the duration of one allocation.
//!
//! Single-frame [`CheckpointStore::install`] cannot be exact, and this is a real limitation: the
//! bytes are allocated by the *caller* before this store sees them, so no charge can precede that
//! allocation. The store charges them at their peak - with no credit for the checkpoint they
//! replace - which means the accounting never under-reports, but a frame arriving when the budget
//! is already nearly full can still push the process above the cap by up to the size of that frame
//! before the refusal lands. A caller that must never exceed the cap has to bound the frame it
//! hands over, or use the chunked path, where the refusal genuinely precedes the allocation.
//!
//! ### The limits this store actually guarantees
//!
//! The byte buckets are exact, and every release is ordered after the free it pays for. Three
//! figures are *not* bounded by a constant this source can assert, and naming them is the point:
//!
//! * **Allocator rounding, inside one `begin`.** `Vec::with_capacity(declared)` may return more
//!   capacity than asked, and that capacity exists before it can be measured, so for the rest of
//!   that call the process holds `capacity - declared` unbilled bytes. The window cannot be closed
//!   earlier - the capacity is unknown until the allocation exists - and its size is the allocator's
//!   size-class granularity, not a number this crate can state. What is guaranteed is that the
//!   window is only ever the delta, because the declared size is charged first, and that a measured
//!   capacity over [`MAX_CHECKPOINT_BYTES`] is refused rather than kept.
//! * **A single-frame [`CheckpointStore::install`].** The buffer is the caller's and exists before
//!   the charge, so up to [`MAX_CHECKPOINT_BYTES`] can sit above the cap for the duration of one
//!   refusal. Bounding that frame is the caller's job; the chunked path refuses before allocating.
//! * **Retained metadata.** The session key and the two id strings a [`Checkpoint`] carries are not
//!   charged to the byte buckets. In production the keeper's per-session reservation covers them;
//!   a standalone store holds them unbilled, and `checkpoint_id` arrives from the wire with only
//!   [`crate::protocol::MAX_FRAME_PAYLOAD_BYTES`] above it.
//!
//! A caller that asks for the whole buffer instead of a chunk (cloning a [`Checkpoint`], or the
//! keeper's own fetch path) makes a full-size copy this store never sees and cannot charge. That is
//! why [`CheckpointStore::pin`] and [`CheckpointStore::read_chunk`] exist: a download that pages
//! through them allocates nothing the store has to account for.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::budget::MemoryBudget;
use crate::error::{KeeperError, KeeperErrorCode, KeeperResult};

/// Largest checkpoint one session may hold, or be uploading: 32 MiB.
///
/// A checkpoint is terminal state, not output history, so the cap is far below the replay cap. It
/// is enforced at `begin` as well as at `install`, which is what makes an oversized chunked upload
/// refusable before a byte of it is accepted.
pub const MAX_CHECKPOINT_BYTES: u64 = 32 * 1024 * 1024;

/// Largest slice one retrieval returns. A 32 MiB checkpoint therefore arrives as at least 512
/// chunks: a recovery point must fit the transport's frame cap, not the other way round.
pub const MAX_CHECKPOINT_READ_CHUNK: usize = 64 * 1024;

/// The committed recovery point for one session.
///
/// Deliberately not `Clone`: a clone would copy the whole buffer, and that copy is allocated by the
/// clone rather than admitted by the budget, so it would be up to a full checkpoint of memory this
/// store never charged. Share the store instead, as the keeper does through its checkpoint mutex.
#[derive(Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub session_id: String,
    pub checkpoint_id: String,
    /// The output offset the checkpoint covers. The only field the keeper interprets, and only to
    /// order commits and to detect a stale upload.
    pub sequence_offset: u64,
    pub bytes: Vec<u8>,
}

impl Checkpoint {
    pub fn new(
        session_id: impl Into<String>,
        checkpoint_id: impl Into<String>,
        sequence_offset: u64,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            checkpoint_id: checkpoint_id.into(),
            sequence_offset,
            bytes,
        }
    }

    /// The stored bytes. The keeper hands them back unchanged; it never inspects them.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Bytes the stored buffer actually holds allocated.
    ///
    /// Not [`Checkpoint::len`] when the caller handed over a slice of a larger buffer: the store
    /// keeps that buffer as it is, so its capacity is what the committed charge has to cover.
    pub fn allocated_bytes(&self) -> usize {
        self.bytes.capacity()
    }
}

/// What a session's committed checkpoint looks like on the wire: identity, the offset it covers and
/// how many bytes are stored. Deliberately small and byte-free - `ATTACH_ACK`, `CHECKPOINT_ACK` and
/// `CHECKPOINT_QUERY` carry this, and none of them should be able to accidentally ship 32 MiB.
///
/// `Default` is the "nothing committed" answer: an empty id, offset 0 and no bytes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckpointInfo {
    /// Empty when nothing is committed.
    pub checkpoint_id: String,
    /// The committed output offset; `0` when nothing is committed.
    pub sequence_offset: u64,
    /// Bytes the committed checkpoint holds.
    pub stored_bytes: u64,
}

impl CheckpointInfo {
    pub fn new(checkpoint_id: impl Into<String>, sequence_offset: u64, stored_bytes: u64) -> Self {
        Self {
            checkpoint_id: checkpoint_id.into(),
            sequence_offset,
            stored_bytes,
        }
    }

    /// The committed checkpoint id, or `""` when none is committed.
    pub fn checkpoint_id(&self) -> &str {
        &self.checkpoint_id
    }

    /// Bytes the committed checkpoint holds: the `storedBytes` figure this struct carries.
    pub fn bytes(&self) -> u64 {
        self.stored_bytes
    }

    /// Whether a checkpoint is committed at all.
    pub fn has_checkpoint(&self) -> bool {
        !self.checkpoint_id.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.stored_bytes == 0 && self.checkpoint_id.is_empty()
    }
}

/// An in-progress chunked upload.
///
/// Not `Clone` for the same reason as [`Checkpoint`]: the clone's bytes would never be charged.
#[derive(Debug, PartialEq, Eq)]
pub struct CheckpointUpload {
    session_id: String,
    checkpoint_id: String,
    sequence_offset: u64,
    declared_total: u64,
    /// What this upload currently costs the budget: the declared total plus every measured capacity
    /// overshoot, raised to the buffer's measured capacity. A stored figure rather than a
    /// recomputation because `Vec` may reserve more than it was asked for.
    charged_bytes: u64,
    next_chunk_index: u32,
    chunks: Vec<u8>,
    /// The epoch that opened this upload. An `ATTACH` that advances the fence retires every upload
    /// below the new epoch: those attempts belong to a daemon that can no longer commit them, and
    /// leaving them charged would strand budget no peer could ever release.
    epoch: u64,
    /// Opaque, monotonically increasing identity for this one attempt. The epoch names the daemon;
    /// the token names the attempt, which is what a teardown has to abort. Two connections on the
    /// same epoch that begin the same `checkpoint_id` are distinguishable only by this.
    upload_token: u64,
}

impl CheckpointUpload {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn checkpoint_id(&self) -> &str {
        &self.checkpoint_id
    }

    pub fn sequence_offset(&self) -> u64 {
        self.sequence_offset
    }

    /// The epoch that opened this upload.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The opaque token that identifies this attempt, and the only handle an abort may use.
    pub fn upload_token(&self) -> u64 {
        self.upload_token
    }

    pub fn received_bytes(&self) -> u64 {
        self.chunks.len() as u64
    }

    /// Bytes this upload has actually allocated. At most [`Self::declared_total`] unless the
    /// allocator overshot a reservation, and an overshoot is billed rather than tolerated.
    pub fn allocated_bytes(&self) -> u64 {
        self.chunks.capacity() as u64
    }

    pub fn declared_total(&self) -> u64 {
        self.declared_total
    }

    /// The chunk index the next `CHECKPOINT_CHUNK` must carry.
    pub fn next_chunk_index(&self) -> u32 {
        self.next_chunk_index
    }

    pub fn is_complete(&self) -> bool {
        self.received_bytes() == self.declared_total
    }

    /// Bytes this upload holds charged: the figure every release path must return.
    ///
    /// Never less than the buffer's capacity, because each growth bills the capacity it measured.
    /// Committing this figure leaves the ledger matching what the buffer really holds.
    pub fn charged_bytes(&self) -> u64 {
        self.charged_bytes
    }
}

/// How far an upload has got, as `CHECKPOINT_ACK` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointProgress {
    pub session_id: String,
    pub checkpoint_id: String,
    pub sequence_offset: u64,
    /// Bytes accepted so far.
    pub received_bytes: u64,
    /// Bytes the upload declared at `begin`.
    pub total_bytes: u64,
    pub complete: bool,
    /// The attempt's opaque token. A peer echoes it on abort so a teardown can only ever release the
    /// attempt it opened, never a later attempt that reused the same ids.
    pub upload_token: u64,
}

impl CheckpointProgress {
    fn new(upload: &CheckpointUpload) -> Self {
        Self {
            session_id: upload.session_id.clone(),
            checkpoint_id: upload.checkpoint_id.clone(),
            sequence_offset: upload.sequence_offset,
            received_bytes: upload.received_bytes(),
            total_bytes: upload.declared_total,
            complete: upload.is_complete(),
            upload_token: upload.upload_token,
        }
    }
}

/// The identity of one committed checkpoint, as a retrieval stream pinned it.
///
/// Byte-free and cheap to clone: a caller holds this between chunk requests and hands it back to
/// [`CheckpointStore::read_chunk`], which re-verifies it against what is committed right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointRef {
    pub session_id: String,
    pub checkpoint_id: String,
    /// The committed offset the stream was opened at.
    pub sequence_offset: u64,
    /// Bytes the checkpoint held when the stream was opened.
    pub total_bytes: u64,
}

impl CheckpointRef {
    pub fn new(
        session_id: impl Into<String>,
        checkpoint_id: impl Into<String>,
        sequence_offset: u64,
        total_bytes: u64,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            checkpoint_id: checkpoint_id.into(),
            sequence_offset,
            total_bytes,
        }
    }
}

/// One slice of a pinned checkpoint.
///
/// Every chunk echoes the identity of the checkpoint it was cut from - `checkpoint_id` *and*
/// `sequence_offset` - so a daemon can reject a stream that changed underneath it even when the
/// bytes it received look plausible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointChunk {
    pub checkpoint_id: String,
    pub sequence_offset: u64,
    /// Byte offset this chunk starts at.
    pub offset: u64,
    /// Bytes the whole checkpoint holds.
    pub total_bytes: u64,
    /// Bytes copied into the caller's buffer.
    pub filled: usize,
    /// Whether this chunk reached the end of the checkpoint.
    pub complete: bool,
}

impl CheckpointChunk {
    /// The offset the next request must carry. Equal to `total_bytes` once complete.
    pub fn next_offset(&self) -> u64 {
        self.offset.saturating_add(self.filled as u64)
    }
}

/// One session's committed checkpoint plus its in-flight uploads.
#[derive(Debug, Default)]
struct SessionCheckpoints {
    committed: Option<Checkpoint>,
    uploads: Vec<CheckpointUpload>,
}

/// The keeper's checkpoint state: one store for every session, keyed by session id.
#[derive(Debug)]
pub struct CheckpointStore {
    /// Shared with the keeper: the aggregate cap every checkpoint charge lands on.
    budget: Arc<Mutex<MemoryBudget>>,
    sessions: HashMap<String, SessionCheckpoints>,
    /// Source of [`CheckpointUpload::upload_token`]. Monotonic for the store's life, so a token is
    /// never handed out twice and an abort can never match a later attempt by accident.
    next_upload_token: u64,
}

impl CheckpointStore {
    /// A store that charges the keeper's aggregate budget.
    pub fn new(budget: Arc<Mutex<MemoryBudget>>) -> Self {
        Self { budget, sessions: HashMap::new(), next_upload_token: 0 }
    }

    /// The next attempt token. Saturating rather than wrapping: a token must never be reused, and a
    /// store that somehow issued `u64::MAX` of them has bigger problems than a repeated identity.
    fn issue_upload_token(&mut self) -> u64 {
        self.next_upload_token = self.next_upload_token.saturating_add(1);
        self.next_upload_token
    }

    /// Bytes held by uploads that have neither committed nor aborted: the budget's pending bucket.
    pub fn pending_bytes(&self) -> usize {
        lock(&self.budget).checkpoint_pending_bytes()
    }

    /// The ledger this store charges. Test-only: it exists so a test can assert the store and the
    /// keeper hold the *same* `Arc` rather than two equal-looking ledgers, which is the whole point of
    /// the shared accounting - equality of counters would not catch a fork.
    #[cfg(test)]
    pub(crate) fn budget_handle(&self) -> Arc<Mutex<MemoryBudget>> {
        Arc::clone(&self.budget)
    }

    /// Bytes held by committed checkpoints across every session.
    pub fn committed_bytes(&self) -> usize {
        lock(&self.budget).checkpoint_committed_bytes()
    }

    /// Sessions this store holds a recovery point or an upload for.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// The committed checkpoint, bytes included.
    pub fn checkpoint(&self, session_id: &str) -> Option<&Checkpoint> {
        self.sessions.get(session_id)?.committed.as_ref()
    }

    /// The committed checkpoint as the wire reports it. `CheckpointInfo::default()` when nothing is
    /// committed, which is the `storedBytes: 0` answer rather than an error.
    pub fn stored(&self, session_id: &str) -> CheckpointInfo {
        match self.checkpoint(session_id) {
            Some(checkpoint) => CheckpointInfo::new(
                checkpoint.checkpoint_id.clone(),
                checkpoint.sequence_offset,
                checkpoint.len() as u64,
            ),
            None => CheckpointInfo::default(),
        }
    }

    /// Pins the committed checkpoint's identity for a bounded download.
    ///
    /// Returns `None` when nothing is committed. The identity is byte-free, so opening a download
    /// never costs the budget; every later [`CheckpointStore::read_chunk`] re-checks it against what
    /// is committed at that moment and fails closed if it was replaced.
    pub fn pin(&self, session_id: &str) -> Option<CheckpointRef> {
        let checkpoint = self.checkpoint(session_id)?;
        Some(CheckpointRef::new(
            &checkpoint.session_id,
            &checkpoint.checkpoint_id,
            checkpoint.sequence_offset,
            checkpoint.len() as u64,
        ))
    }

    /// Copies one bounded slice of a pinned checkpoint into `out`.
    ///
    /// The copy goes straight into the caller's buffer, so a download allocates nothing the store
    /// has to charge and retains nothing between chunks. Identity is re-verified per call - the
    /// pinned id and offset must still be what is committed - so a checkpoint replaced mid-download
    /// fails closed instead of stitching two generations of terminal state together. Chunks carry
    /// the identity they were cut from, so a caller can check the stream even if it batches replies.
    pub fn read_chunk(
        &self,
        pinned: &CheckpointRef,
        offset: u64,
        out: &mut [u8],
    ) -> KeeperResult<CheckpointChunk> {
        let current = self.checkpoint(&pinned.session_id).ok_or_else(|| {
            KeeperError::new(
                KeeperErrorCode::StaleCheckpoint,
                format!(
                    "checkpoint {} for session {} was replaced or dropped during download",
                    pinned.checkpoint_id, pinned.session_id
                ),
            )
        })?;
        if current.checkpoint_id != pinned.checkpoint_id
            || current.sequence_offset != pinned.sequence_offset
        {
            return Err(KeeperError::at(
                KeeperErrorCode::StaleCheckpoint,
                format!(
                    "download pinned checkpoint {} at offset {}, but {} at offset {} is committed",
                    pinned.checkpoint_id, pinned.sequence_offset, current.checkpoint_id, current.sequence_offset
                ),
                current.sequence_offset,
            ));
        }
        if offset > pinned.total_bytes {
            return Err(KeeperError::new(
                KeeperErrorCode::CheckpointChunkMismatch,
                format!(
                    "checkpoint {} holds {} bytes but the download asked for offset {offset}",
                    pinned.checkpoint_id, pinned.total_bytes
                ),
            ));
        }

        let remaining = pinned.total_bytes.saturating_sub(offset) as usize;
        let limit = out.len().min(remaining).min(MAX_CHECKPOINT_READ_CHUNK);
        let start = offset as usize;
        out[..limit].copy_from_slice(&current.bytes[start..start + limit]);
        let filled = limit;
        Ok(CheckpointChunk {
            checkpoint_id: pinned.checkpoint_id.clone(),
            sequence_offset: pinned.sequence_offset,
            offset,
            total_bytes: pinned.total_bytes,
            filled,
            complete: offset.saturating_add(filled as u64) >= pinned.total_bytes,
        })
    }

    /// The open upload for `(session_id, checkpoint_id)`, if any.
    pub fn upload(&self, session_id: &str, checkpoint_id: &str) -> Option<&CheckpointUpload> {
        self.sessions
            .get(session_id)?
            .uploads
            .iter()
            .find(|upload| upload.checkpoint_id == checkpoint_id)
    }

    /// Installs a checkpoint that arrived complete in one frame.
    ///
    /// `total_bytes` must equal the bytes carried: a frame that declares one size and carries
    /// another is a mismatch, not something to reconcile silently. Every refusal here happens
    /// before the committed checkpoint is touched, so a failed install always preserves the
    /// previous recovery point.
    pub fn install(
        &mut self,
        session_id: &str,
        checkpoint_id: &str,
        sequence_offset: u64,
        total_bytes: u64,
        bytes: Vec<u8>,
    ) -> KeeperResult<CheckpointInfo> {
        // An empty id is the `Default` sentinel for "nothing committed", so a checkpoint stored
        // under it is unaddressable: `stored()` would report no checkpoint while the replay prefix
        // this install trims is already gone. Refused before any charge or trim.
        check_checkpoint_id(checkpoint_id)?;
        // The declared size is checked before the carried bytes: a size over the cap is refused
        // for that reason, whatever the frame happens to contain.
        self.check_per_checkpoint_cap(checkpoint_id, total_bytes)
            .map_err(|error| self.note_committed_offset(session_id, error))?;
        if total_bytes != bytes.len() as u64 {
            return Err(KeeperError::new(
                KeeperErrorCode::CheckpointChunkMismatch,
                format!(
                    "checkpoint {checkpoint_id} declared {total_bytes} bytes but carried {}",
                    bytes.len()
                ),
            ));
        }
        // The buffer is stored unchanged, so what the store retains is its capacity, not its
        // length. A caller that hands over a slice of a larger buffer is charged for the slack it
        // keeps alive, and one whose buffer alone breaks the per-checkpoint cap is refused instead
        // of being kept at unbilled size.
        let incoming = bytes.capacity() as u64;
        if incoming > MAX_CHECKPOINT_BYTES {
            return Err(self.note_committed_offset(
                session_id,
                KeeperError::new(
                    KeeperErrorCode::CheckpointBudgetExhausted,
                    format!(
                        "checkpoint {checkpoint_id} carries {} bytes in a {incoming} byte allocation, over the {MAX_CHECKPOINT_BYTES} byte per-checkpoint cap",
                        bytes.len()
                    ),
                ),
            ));
        }
        if let Err(stale) = self.check_stale(session_id, checkpoint_id, sequence_offset) {
            self.release_dead_uploads(session_id);
            return Err(stale);
        }

        let mut budget = lock(&self.budget);
        // The incoming buffer exists before this call starts and everything it supersedes is still
        // live, so the honest peak is the incoming allocation with no credit for the frees to come.
        // A replacement that cannot fit at that peak is refused with the old checkpoint intact.
        Self::check_peak_fit(&budget, checkpoint_id, incoming, 0)
            .map_err(|error| self.note_committed_offset(session_id, error))?;
        budget
            .try_reserve_checkpoint_chunk(incoming as usize)
            .map_err(|refusal| {
                self.note_committed_offset(
                    session_id,
                    refusal.as_error(&format!("checkpoint {checkpoint_id}")),
                )
            })?;

        let entry = self.sessions.entry(session_id.to_owned()).or_default();
        if let Some(index) = entry
            .uploads
            .iter()
            .position(|upload| upload.checkpoint_id == checkpoint_id)
        {
            // Destruct first, release second: the ledger must never call a byte free while the
            // allocation behind it is still held.
            let superseded = entry.uploads.remove(index);
            let charged = superseded.charged_bytes as usize;
            drop(superseded);
            budget.abort_checkpoint_chunk(charged);
        }
        if let Some(superseded) = entry.committed.take() {
            let charged = superseded.allocated_bytes();
            drop(superseded);
            budget.release_checkpoint_committed(charged);
        }
        // The incoming charge becomes the committed charge in one step: the bytes stayed allocated
        // throughout, only their bucket changes.
        debug_assert!(
            budget.checkpoint_pending_bytes() as u64 >= incoming,
            "install committed {incoming} bytes that were never reserved as pending"
        );
        budget.commit_checkpoint_chunk(incoming as usize);
        drop(budget);
        Ok(self.replace_committed(session_id, checkpoint_id, sequence_offset, bytes))
    }

    /// Opens a chunked upload, allocating and charging its whole declared size up front.
    ///
    /// This is one allocation of the final size, made before any chunk arrives, and chunks then fill
    /// it without ever reallocating: capacity is at least the declared total, and no chunk may
    /// exceed that total, so there is no growth step in which an old and a new buffer coexist. The
    /// peak the budget bounds is therefore the capacity of this single buffer, not a transient
    /// double allocation.
    ///
    /// The fit is judged before the buffer is built, so an upload that cannot fit allocates nothing.
    /// A retried `CHECKPOINT_INSTALL` under the same id restarts the upload instead of charging a
    /// second copy of the same declared size.
    ///
    /// Releasing the previous attempt before allocating the new buffer is what keeps the peak at one
    /// upload: holding both while the second is charged would exceed what the budget admitted. The
    /// cost is that an allocator rounding the reservation past a full budget loses the retried
    /// attempt rather than keeping it - the alternative is a window in which two buffers are live
    /// and only one is charged, which is the failure this store exists to prevent.
    pub fn begin(
        &mut self,
        session_id: &str,
        checkpoint_id: &str,
        sequence_offset: u64,
        total_bytes: u64,
        epoch: u64,
    ) -> KeeperResult<CheckpointProgress> {
        check_checkpoint_id(checkpoint_id)?;
        self.check_per_checkpoint_cap(checkpoint_id, total_bytes)
            .map_err(|error| self.note_committed_offset(session_id, error))?;
        if let Err(stale) = self.check_stale(session_id, checkpoint_id, sequence_offset) {
            self.release_dead_uploads(session_id);
            return Err(stale);
        }

        // Presence, not charge: a zero-byte retry charges nothing, and gating the removal on the
        // charge left the first attempt in place to shadow the retry - `push_chunk`, `commit` and
        // `abort` all select the *first* match, so the real upload could never take a chunk and the
        // stale metadata accumulated without bound.
        let supersedes = self.upload(session_id, checkpoint_id).is_some();
        let superseded_pending = self
            .upload(session_id, checkpoint_id)
            .map_or(0, CheckpointUpload::charged_bytes);
        let declared = total_bytes as usize;
        let mut budget = lock(&self.budget);
        // A retried begin releases the first attempt before taking the second, and nothing new is
        // allocated until that memory is gone, so one upload never holds two charges and the
        // freed bytes are only counted once they are actually free.
        Self::check_peak_fit(&budget, checkpoint_id, total_bytes, superseded_pending)
            .map_err(|error| self.note_committed_offset(session_id, error))?;
        if supersedes {
            if let Some(entry) = self.sessions.get_mut(session_id) {
                if let Some(index) = entry
                    .uploads
                    .iter()
                    .position(|upload| upload.checkpoint_id == checkpoint_id)
                {
                    // Destruct first, release second: the ledger must never call a byte free while
                    // the allocation behind it is still held.
                    let superseded = entry.uploads.remove(index);
                    let charged = superseded.charged_bytes as usize;
                    drop(superseded);
                    if charged > 0 {
                        budget.abort_checkpoint_chunk(charged);
                    }
                }
            }
        }
        // Charged before the allocation: the declared size is the lower bound on what the buffer
        // will hold, so nothing is allocated that the budget has not already admitted.
        budget
            .try_reserve_checkpoint_chunk(declared)
            .map_err(|refusal| {
                self.note_committed_offset(
                    session_id,
                    refusal.as_error(&format!("checkpoint {checkpoint_id}")),
                )
            })?;

        let chunks = Vec::with_capacity(declared);
        let capacity = chunks.capacity();
        // The allocator may round up, and the upload keeps that whole allocation for its lifetime.
        // The per-checkpoint cap is a promise about what the store holds, so it is enforced against
        // the measured capacity, exactly as `install` enforces it against the buffer it is handed.
        if capacity as u64 > MAX_CHECKPOINT_BYTES {
            drop(chunks);
            budget.abort_checkpoint_chunk(declared);
            return Err(self.note_committed_offset(
                session_id,
                KeeperError::new(
                    KeeperErrorCode::CheckpointBudgetExhausted,
                    format!(
                        "checkpoint {checkpoint_id} declared {declared} bytes but was allocated {capacity}, over the {MAX_CHECKPOINT_BYTES} byte per-checkpoint cap"
                    ),
                ),
            ));
        }
        if capacity > declared {
            // The allocator may round a reservation up. That overshoot is memory this store keeps
            // for the life of the upload, so it is billed now; if it cannot be billed the buffer is
            // dropped and the declared charge returned, because keeping it would mean holding
            // bytes no charge covers.
            let overshoot = capacity - declared;
            if let Err(refusal) = budget.try_reserve_checkpoint_chunk(overshoot) {
                drop(chunks);
                budget.abort_checkpoint_chunk(declared);
                return Err(self.note_committed_offset(
                    session_id,
                    refusal.as_error(&format!("checkpoint {checkpoint_id}")),
                ));
            }
        }
        drop(budget);

        let upload_token = self.issue_upload_token();
        let entry = self.sessions.entry(session_id.to_owned()).or_default();
        entry.uploads.push(CheckpointUpload {
            session_id: session_id.to_owned(),
            checkpoint_id: checkpoint_id.to_owned(),
            sequence_offset,
            declared_total: total_bytes,
            charged_bytes: capacity as u64,
            next_chunk_index: 0,
            chunks,
            epoch,
            upload_token,
        });
        let upload = entry.uploads.last().expect("the upload was just pushed");
        Ok(CheckpointProgress::new(upload))
    }

    /// Appends one chunk. `chunk_index` must be the next index in order and the run must not overrun
    /// the declared total: an out-of-order or oversized chunk is a typed mismatch, never a silent
    /// truncation or gap.
    pub fn push_chunk(
        &mut self,
        session_id: &str,
        checkpoint_id: &str,
        chunk_index: u32,
        data: &[u8],
    ) -> KeeperResult<CheckpointProgress> {
        let entry = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| Self::unknown_upload(session_id, checkpoint_id))?;
        let index = entry
            .uploads
            .iter()
            .position(|upload| upload.checkpoint_id == checkpoint_id)
            .ok_or_else(|| Self::unknown_upload(session_id, checkpoint_id))?;
        let upload = &mut entry.uploads[index];

        if chunk_index != upload.next_chunk_index {
            return Err(KeeperError::new(
                KeeperErrorCode::CheckpointChunkMismatch,
                format!(
                    "checkpoint {checkpoint_id} expected chunk {} but received chunk {chunk_index}",
                    upload.next_chunk_index
                ),
            ));
        }
        let after = upload.received_bytes().saturating_add(data.len() as u64);
        if after > upload.declared_total {
            return Err(KeeperError::new(
                KeeperErrorCode::CheckpointChunkMismatch,
                format!(
                    "checkpoint {checkpoint_id} declared {} bytes but chunk {chunk_index} would carry {after}",
                    upload.declared_total
                ),
            ));
        }
        Self::append_chunk(upload, data);
        Ok(CheckpointProgress::new(upload))
    }

    /// Appends one chunk into space that was already allocated and already charged.
    ///
    /// The buffer was sized to the declared total at `begin` and the caller has already refused any
    /// run past that total, so this cannot reallocate: there is no step where an old and a new
    /// allocation exist together, and no capacity appears that the budget has not admitted.
    fn append_chunk(upload: &mut CheckpointUpload, data: &[u8]) {
        debug_assert!(
            upload.chunks.len() + data.len() <= upload.chunks.capacity(),
            "a chunk must fit the allocation charged at begin"
        );
        upload.chunks.extend_from_slice(data);
        upload.next_chunk_index = upload.next_chunk_index.saturating_add(1);
    }

    /// Commits a completed upload, replacing the previous checkpoint.
    ///
    /// The superseded checkpoint is destroyed before its charge returns, and the upload's pending
    /// charge becomes the committed charge under the same guard, so the ledger never calls a byte
    /// free while the allocator still owns it and no byte is counted in two buckets.
    pub fn commit(
        &mut self,
        session_id: &str,
        checkpoint_id: &str,
        sequence_offset: u64,
    ) -> KeeperResult<CheckpointInfo> {
        let upload = self
            .upload(session_id, checkpoint_id)
            .ok_or_else(|| Self::unknown_upload(session_id, checkpoint_id))?;
        if sequence_offset != upload.sequence_offset {
            return Err(KeeperError::new(
                KeeperErrorCode::CheckpointChunkMismatch,
                format!(
                    "checkpoint {checkpoint_id} was opened at offset {} but committed at {sequence_offset}",
                    upload.sequence_offset
                ),
            ));
        }
        if !upload.is_complete() {
            return Err(KeeperError::new(
                KeeperErrorCode::CheckpointChunkMismatch,
                format!(
                    "checkpoint {checkpoint_id} is {} of {} bytes",
                    upload.received_bytes(),
                    upload.declared_total
                ),
            ));
        }
        // A newer commit may have landed while this upload was in flight; the older one is stale.
        if let Err(stale) = self.check_stale(session_id, checkpoint_id, sequence_offset) {
            self.release_dead_uploads(session_id);
            return Err(stale);
        }

        let removed = self
            .sessions
            .get_mut(session_id)
            .and_then(|entry| {
                let index = entry
                    .uploads
                    .iter()
                    .position(|upload| upload.checkpoint_id == checkpoint_id)?;
                Some(entry.uploads.remove(index))
            })
            .ok_or_else(|| Self::unknown_upload(session_id, checkpoint_id))?;

        let mut budget = lock(&self.budget);
        // The superseded checkpoint is destroyed before its charge returns: until that drop the
        // allocator still holds its bytes, so the ledger must still count them.
        if let Some(entry) = self.sessions.get_mut(session_id) {
            if let Some(superseded) = entry.committed.take() {
                let charged = superseded.allocated_bytes();
                drop(superseded);
                budget.release_checkpoint_committed(charged);
            }
        }
        // The upload's charge - its allocation, not its declared size - becomes the committed
        // charge. Nothing new is reserved: these bytes stayed allocated the whole way through.
        debug_assert!(
            budget.checkpoint_pending_bytes() as u64 >= removed.charged_bytes,
            "commit converted {} pending bytes but only {} are pending",
            removed.charged_bytes,
            budget.checkpoint_pending_bytes()
        );
        budget.commit_checkpoint_chunk(removed.charged_bytes as usize);
        drop(budget);
        Ok(self.replace_committed(session_id, checkpoint_id, sequence_offset, removed.chunks))
    }

    /// Abandons one upload attempt and returns its charge. The committed checkpoint is untouched.
    ///
    /// Matched by `upload_token`, never by `(session, checkpoint_id)`: the epoch cannot tell two
    /// attempts apart, so a teardown that matched on it aborted whichever attempt happened to be
    /// live - a successor's, when a retry had superseded the caller's own. A token only its opener
    /// knows cannot name anyone else's attempt.
    ///
    /// Idempotent: an attempt that already committed, was superseded, or was never opened reports
    /// `false` and changes nothing. A caller that retries an abort after a lost reply still sees
    /// success for its own upload.
    pub fn abort(&mut self, session_id: &str, checkpoint_id: &str, upload_token: u64) -> bool {
        let removed = self.sessions.get_mut(session_id).and_then(|entry| {
            let index = entry.uploads.iter().position(|upload| {
                upload.checkpoint_id == checkpoint_id && upload.upload_token == upload_token
            })?;
            Some(entry.uploads.remove(index))
        });
        match removed {
            Some(upload) => {
                let charged = upload.charged_bytes as usize;
                drop(upload);
                lock(&self.budget).abort_checkpoint_chunk(charged);
                true
            }
            None => false,
        }
    }

    /// Retires every upload a predecessor epoch opened, returning each charge.
    ///
    /// An `ATTACH` that advances the fence proves the previous daemon is gone, so its in-flight
    /// uploads can never commit: their `epoch` no longer verifies. Keeping them would hold budget
    /// until the session died, and a later abort from that daemon must not be the only thing that
    /// could free it. Uploads opened at `epoch` or above - the successor's own - are untouched.
    ///
    /// Returns how many attempts were retired, so a caller can tell "nothing to do" from "released".
    pub fn retire_uploads_before_epoch(&mut self, session_id: &str, epoch: u64) -> usize {
        let mut dead = Vec::new();
        if let Some(entry) = self.sessions.get_mut(session_id) {
            let mut index = 0;
            while index < entry.uploads.len() {
                if entry.uploads[index].epoch < epoch {
                    dead.push(entry.uploads.remove(index));
                } else {
                    index += 1;
                }
            }
        }
        if dead.is_empty() {
            return 0;
        }
        let retired = dead.len();
        let mut budget = lock(&self.budget);
        for upload in dead {
            let charged = upload.charged_bytes as usize;
            drop(upload);
            budget.abort_checkpoint_chunk(charged);
        }
        retired
    }

    /// Drops every upload for `session_id` whose offset can no longer be committed, returning each
    /// charge. `commit` re-checks staleness, so an upload at or before the committed offset is dead:
    /// keeping it would hold budget until the session was killed. An upload with a newer offset is
    /// left alone - a stale frame for the same id must not destroy a live attempt.
    fn release_dead_uploads(&mut self, session_id: &str) {
        let Some(committed) = self.checkpoint(session_id).map(|c| c.sequence_offset) else {
            return;
        };
        let mut dead = Vec::new();
        if let Some(entry) = self.sessions.get_mut(session_id) {
            let mut index = 0;
            while index < entry.uploads.len() {
                if entry.uploads[index].sequence_offset <= committed {
                    dead.push(entry.uploads.remove(index));
                } else {
                    index += 1;
                }
            }
        }
        if dead.is_empty() {
            return;
        }
        let mut budget = lock(&self.budget);
        for upload in dead {
            let charged = upload.charged_bytes as usize;
            drop(upload);
            budget.abort_checkpoint_chunk(charged);
        }
    }

    /// Forgets a session entirely and returns every charge it held. Used when the session is
    /// retired, so a killed session cannot leave a checkpoint charged against the budget.
    pub fn forget_session(&mut self, session_id: &str) {
        let Some(entry) = self.sessions.remove(session_id) else {
            return;
        };
        let mut budget = lock(&self.budget);
        for upload in entry.uploads {
            let charged = upload.charged_bytes as usize;
            drop(upload);
            budget.abort_checkpoint_chunk(charged);
        }
        if let Some(committed) = entry.committed {
            let charged = committed.allocated_bytes();
            drop(committed);
            budget.release_checkpoint_committed(charged);
        }
    }

    /// Whether `total_bytes` fits at the peak, after the `freed` bytes the caller is about to return.
    ///
    /// The figure is the *peak*, not the settled charge: the bytes being admitted are live before
    /// anything is released, so only the memory the caller genuinely frees first may be credited.
    /// One pure check, made while the budget lock is held: the mutation it authorises follows under
    /// the same guard, so a charge that does not fit leaves the committed checkpoint and the
    /// accounting exactly as they were.
    fn check_peak_fit(
        budget: &MemoryBudget,
        checkpoint_id: &str,
        total_bytes: u64,
        freed: u64,
    ) -> KeeperResult<()> {
        let required = total_bytes.saturating_sub(freed);
        let available = budget.available();
        if required > available {
            return Err(KeeperError::new(
                KeeperErrorCode::CheckpointBudgetExhausted,
                format!(
                    "checkpoint {checkpoint_id} needs {required} bytes at its peak, but only {available} of the {} byte keeper budget remain",
                    budget.total()
                ),
            ));
        }
        Ok(())
    }

    fn replace_committed(
        &mut self,
        session_id: &str,
        checkpoint_id: &str,
        sequence_offset: u64,
        bytes: Vec<u8>,
    ) -> CheckpointInfo {
        let stored_bytes = bytes.len() as u64;
        let checkpoint = Checkpoint::new(session_id, checkpoint_id, sequence_offset, bytes);
        let entry = self.sessions.entry(session_id.to_owned()).or_default();
        entry.committed = Some(checkpoint);
        CheckpointInfo::new(checkpoint_id, sequence_offset, stored_bytes)
    }

    fn check_per_checkpoint_cap(&self, checkpoint_id: &str, total_bytes: u64) -> KeeperResult<()> {
        if total_bytes > MAX_CHECKPOINT_BYTES {
            return Err(KeeperError::new(
                KeeperErrorCode::CheckpointBudgetExhausted,
                format!(
                    "checkpoint {checkpoint_id} declares {total_bytes} bytes, over the {MAX_CHECKPOINT_BYTES} byte per-checkpoint cap"
                ),
            ));
        }
        Ok(())
    }

    /// Attaches the committed sequence offset to a `CHECKPOINT_BUDGET_EXHAUSTED` refusal, which is
    /// what [`KeeperError`]'s `offset` field promises for that code.
    ///
    /// The caller needs it to know *where* it still is: a refusal preserves the previous checkpoint
    /// precisely so the daemon can resume from it, and without the offset the daemon would have to
    /// ask again to learn the recovery point it was just told to keep using. Absent a committed
    /// checkpoint there is no such offset, and inventing `0` would claim a recovery point that does
    /// not exist - so the field stays empty.
    ///
    /// Reads the committed checkpoint only, straight out of this store's map: it takes no lock at
    /// all, so a caller that already holds the budget guard can call it without a second acquisition
    /// and without a lock-order edge.
    fn note_committed_offset(&self, session_id: &str, mut error: KeeperError) -> KeeperError {
        if error.code != KeeperErrorCode::CheckpointBudgetExhausted || error.offset.is_some() {
            return error;
        }
        error.offset = self.checkpoint(session_id).map(|checkpoint| checkpoint.sequence_offset);
        error
    }

    /// A checkpoint at or before the committed offset can never be worth installing: the successor
    /// would restore older terminal state than it already holds.
    fn check_stale(
        &self,
        session_id: &str,
        checkpoint_id: &str,
        sequence_offset: u64,
    ) -> KeeperResult<()> {
        let Some(committed) = self.checkpoint(session_id) else {
            return Ok(());
        };
        if sequence_offset <= committed.sequence_offset {
            return Err(KeeperError::at(
                KeeperErrorCode::StaleCheckpoint,
                format!(
                    "checkpoint {checkpoint_id} is at sequence offset {sequence_offset}, but offset {} is already committed",
                    committed.sequence_offset
                ),
                committed.sequence_offset,
            ));
        }
        Ok(())
    }

    fn unknown_upload(session_id: &str, checkpoint_id: &str) -> KeeperError {
        KeeperError::new(
            KeeperErrorCode::UnknownCheckpointUpload,
            format!("no open checkpoint upload {checkpoint_id} for session {session_id}"),
        )
    }
}

/// Locks the shared budget, recovering from a poisoned guard: the accounting is plain numbers and
/// one panicking connection thread must not take the checkpoint state down with it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Refuses a checkpoint id that cannot address a stored checkpoint.
///
/// The empty string is the [`CheckpointInfo::default`] sentinel for "nothing committed", so a
/// checkpoint stored under it is invisible to every reader (`stored`, `pin`, `checkpoint_fetch_run`)
/// while the install that wrote it has already charged its bytes and trimmed the replay prefix it
/// covers. That is a recovery point nobody can ask for, produced by a frame that was accepted. The
/// refusal is typed and happens before any allocation, charge or trim.
pub fn check_checkpoint_id(checkpoint_id: &str) -> KeeperResult<()> {
    if checkpoint_id.is_empty() {
        return Err(KeeperError::new(
            KeeperErrorCode::BadFrame,
            "checkpointId must not be empty: the empty id is the \"nothing committed\" sentinel",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::BUDGET_AGGREGATE_BYTES;

    fn store() -> CheckpointStore {
        CheckpointStore::new(Arc::new(Mutex::new(MemoryBudget::new(
            BUDGET_AGGREGATE_BYTES as u64,
        ))))
    }

    fn store_with_cap(total_bytes: u64) -> CheckpointStore {
        CheckpointStore::new(Arc::new(Mutex::new(MemoryBudget::new(total_bytes))))
    }

    /// The charge an upload must hold, derived from what it measured rather than from a literal:
    /// the allocator may hand back more capacity than was asked for, and the charge follows it.
    fn expected_charge(upload: &CheckpointUpload) -> u64 {
        upload.declared_total().max(upload.allocated_bytes())
    }

    fn install(store: &mut CheckpointStore, id: &str, offset: u64, bytes: &[u8]) -> CheckpointInfo {
        store
            .install("s1", id, offset, bytes.len() as u64, bytes.to_vec())
            .expect("install")
    }

    fn begin_and_commit(
        store: &mut CheckpointStore,
        id: &str,
        offset: u64,
        bytes: &[u8],
    ) -> CheckpointInfo {
        let total = bytes.len() as u64;
        store.begin("s1", id, offset, total, 0).expect("begin");
        let (head, tail) = bytes.split_at(bytes.len() / 2);
        store.push_chunk("s1", id, 0, head).expect("chunk 0");
        store.push_chunk("s1", id, 1, tail).expect("chunk 1");
        store.commit("s1", id, offset).expect("commit")
    }

    #[test]
    fn a_single_frame_install_is_stored_at_its_declared_offset() {
        let mut store = store();
        let info = install(&mut store, "cp1", 100, b"abcdef");

        assert_eq!(info.checkpoint_id(), "cp1");
        assert_eq!(info.sequence_offset, 100);
        assert_eq!(info.stored_bytes, 6);
        assert_eq!(info.bytes(), 6);
        assert_eq!(store.stored("s1"), info);
        assert_eq!(store.checkpoint("s1").expect("committed").bytes(), &b"abcdef"[..]);
        assert_eq!(store.pending_bytes(), 0);
        assert_eq!(
            store.committed_bytes(),
            store.checkpoint("s1").expect("committed").allocated_bytes()
        );
    }

    #[test]
    fn a_chunked_upload_commits_at_its_sequence_offset() {
        let mut store = store();
        let info = begin_and_commit(&mut store, "cp1", 100, b"abcdef");

        assert_eq!(info.sequence_offset, 100);
        assert_eq!(info.stored_bytes, 6);
        assert_eq!(store.checkpoint("s1").expect("committed").bytes(), &b"abcdef"[..]);
        assert_eq!(store.pending_bytes(), 0);
        assert_eq!(
            store.committed_bytes(),
            store.checkpoint("s1").expect("committed").allocated_bytes()
        );
    }

    #[test]
    fn a_begin_charges_the_declared_size_before_any_chunk_arrives() {
        let mut store = store();
        let progress = store.begin("s1", "cp1", 10, 64, 0).expect("begin");

        assert_eq!(progress.received_bytes, 0);
        assert_eq!(progress.total_bytes, 64);
        assert!(!progress.complete);
        let upload = store.upload("s1", "cp1").expect("upload");
        assert_eq!(upload.charged_bytes(), expected_charge(upload));
        assert_eq!(store.pending_bytes() as u64, upload.charged_bytes());
        assert_eq!(store.committed_bytes(), 0);

        let progress = store.push_chunk("s1", "cp1", 0, b"abc").expect("chunk");
        assert_eq!(progress.received_bytes, 3);
        let upload = store.upload("s1", "cp1").expect("upload");
        assert_eq!(upload.charged_bytes(), expected_charge(upload));
        assert_eq!(
            store.pending_bytes() as u64,
            upload.charged_bytes(),
            "the charge follows the allocation, not the bytes received"
        );
    }

    #[test]
    fn a_checkpoint_at_or_before_the_committed_offset_is_stale() {
        let mut store = store();
        install(&mut store, "cp1", 100, b"aaaa");

        for (id, offset) in [("cp2", 100), ("cp3", 99)] {
            let error = store
                .install("s1", id, offset, 4, b"bbbb".to_vec())
                .expect_err("stale install must be refused");
            assert_eq!(error.code, KeeperErrorCode::StaleCheckpoint);
            assert_eq!(error.offset, Some(100), "the newest committed offset is reported");

            let error = store
                .begin("s1", id, offset, 4, 0)
                .expect_err("stale begin must be refused");
            assert_eq!(error.code, KeeperErrorCode::StaleCheckpoint);
        }
        assert_eq!(store.checkpoint("s1").expect("kept").bytes(), &b"aaaa"[..]);
        assert_eq!(store.pending_bytes(), 0, "a refused begin charges nothing");
    }

    #[test]
    fn a_checkpoint_over_the_per_checkpoint_cap_is_refused_and_the_commit_is_kept() {
        let mut store = store();
        install(&mut store, "cp1", 10, b"aaaa");

        let over = MAX_CHECKPOINT_BYTES + 1;
        let error = store
            .begin("s1", "cp2", 20, over, 0)
            .expect_err("the per-checkpoint cap must refuse");
        assert_eq!(error.code, KeeperErrorCode::CheckpointBudgetExhausted);
        assert!(error.message.contains("per-checkpoint cap"));

        // The install path refuses on the same cap, before it compares the carried bytes.
        let error = store
            .install("s1", "cp2", 20, over, Vec::new())
            .expect_err("an install over the cap must be refused");
        assert_eq!(error.code, KeeperErrorCode::CheckpointBudgetExhausted);

        assert_eq!(store.checkpoint("s1").expect("preserved").sequence_offset, 10);
        assert_eq!(store.pending_bytes(), 0);
        assert_eq!(store.committed_bytes(), 4);
    }

    #[test]
    fn a_budget_refusal_preserves_the_previously_committed_checkpoint() {
        // Room for the first checkpoint and nothing more.
        let mut store = store_with_cap(8);
        install(&mut store, "cp1", 10, b"aaaa");

        let error = store
            .begin("s1", "cp2", 20, 64, 0)
            .expect_err("the aggregate ceiling must refuse the second upload");
        assert_eq!(error.code, KeeperErrorCode::CheckpointBudgetExhausted);
        assert_eq!(store.checkpoint("s1").expect("preserved").bytes(), &b"aaaa"[..]);
        assert_eq!(store.pending_bytes(), 0);
        assert_eq!(store.committed_bytes(), 4);

        // Replacing the committed checkpoint with a same-sized one still fits: its own bytes are
        // returned as the new ones are charged.
        let replaced = install(&mut store, "cp2", 20, b"bbbb");
        assert_eq!(replaced.stored_bytes, 4);
        assert_eq!(store.committed_bytes(), 4);
        assert_eq!(store.checkpoint("s1").expect("replaced").sequence_offset, 20);
    }

    #[test]
    fn a_budget_refusal_names_the_offset_the_daemon_can_still_resume_from() {
        // `KeeperError::offset` promises the newest committed offset on
        // `CHECKPOINT_BUDGET_EXHAUSTED`, and a refusal preserves that checkpoint precisely so the
        // daemon can resume from it. Reporting the code without the offset would make the promise
        // false and force a second round trip to learn the recovery point the refusal just kept.
        let mut store = store_with_cap(8);
        install(&mut store, "cp1", 10, b"aaaa");

        for error in [
            store
                .begin("s1", "cp2", 20, 64, 0)
                .expect_err("the aggregate ceiling must refuse the second upload"),
            store
                .install("s1", "cp2", 20, 8, vec![b'b'; 8])
                .expect_err("the peak allocation must be refused"),
            store
                .begin("s1", "cp2", 20, MAX_CHECKPOINT_BYTES + 1, 0)
                .expect_err("the per-checkpoint cap must refuse"),
        ] {
            assert_eq!(error.code, KeeperErrorCode::CheckpointBudgetExhausted);
            assert_eq!(
                error.offset,
                Some(10),
                "a refusal must name the committed offset it kept: {}",
                error.message
            );
        }
    }

    #[test]
    fn a_budget_refusal_without_a_committed_checkpoint_claims_no_offset() {
        // Nothing is committed, so there is no recovery point to name. A zero here would be a
        // fabricated answer the daemon would resume from into a log that starts somewhere else.
        let mut store = store_with_cap(8);
        let error = store
            .begin("s1", "cp1", 20, 64, 0)
            .expect_err("the aggregate ceiling must refuse the first upload");
        assert_eq!(error.code, KeeperErrorCode::CheckpointBudgetExhausted);
        assert_eq!(error.offset, None);
    }

    #[test]
    fn a_chunk_out_of_order_is_a_mismatch_and_does_not_advance_the_upload() {
        let mut store = store();
        store.begin("s1", "cp1", 1, 6, 0).expect("begin");
        store.push_chunk("s1", "cp1", 0, b"abc").expect("chunk 0");

        let error = store
            .push_chunk("s1", "cp1", 2, b"def")
            .expect_err("chunk 2 is not next");
        assert_eq!(error.code, KeeperErrorCode::CheckpointChunkMismatch);
        assert_eq!(
            store.upload("s1", "cp1").expect("upload").received_bytes(),
            3,
            "a refused chunk is not accepted"
        );

        store.push_chunk("s1", "cp1", 1, b"def").expect("chunk 1");
        store.commit("s1", "cp1", 1).expect("commit");
    }

    #[test]
    fn a_chunk_that_overruns_the_declared_total_is_a_mismatch() {
        let mut store = store();
        store.begin("s1", "cp1", 1, 4, 0).expect("begin");
        store.push_chunk("s1", "cp1", 0, b"ab").expect("fits");

        let error = store.push_chunk("s1", "cp1", 1, b"cde").expect_err("overrun");
        assert_eq!(error.code, KeeperErrorCode::CheckpointChunkMismatch);
        assert_eq!(store.upload("s1", "cp1").expect("upload").received_bytes(), 2);
    }

    #[test]
    fn committing_an_incomplete_upload_is_refused() {
        let mut store = store();
        store.begin("s1", "cp1", 1, 8, 0).expect("begin");
        store.push_chunk("s1", "cp1", 0, b"ab").expect("partial");

        let error = store.commit("s1", "cp1", 1).expect_err("incomplete");
        assert_eq!(error.code, KeeperErrorCode::CheckpointChunkMismatch);
        assert!(store.checkpoint("s1").is_none());
    }

    #[test]
    fn committing_at_an_offset_the_upload_did_not_declare_is_a_mismatch() {
        let mut store = store();
        store.begin("s1", "cp1", 7, 2, 0).expect("begin");
        store.push_chunk("s1", "cp1", 0, b"ab").expect("chunk");

        let error = store.commit("s1", "cp1", 9).expect_err("offset drift");
        assert_eq!(error.code, KeeperErrorCode::CheckpointChunkMismatch);
        assert_eq!(store.pending_bytes(), 2);
    }

    #[test]
    fn an_unknown_session_or_upload_is_its_own_code() {
        let mut store = store();
        let unknown = [
            store.push_chunk("nope", "cp1", 0, b"x").map(|_| ()),
            store.push_chunk("s1", "nope", 0, b"x").map(|_| ()),
            store.commit("s1", "nope", 1).map(|_| ()),
        ];
        for result in unknown {
            assert_eq!(
                result.expect_err("unknown upload").code,
                KeeperErrorCode::UnknownCheckpointUpload
            );
        }
        assert!(
            !store.abort("s1", "nope", 0),
            "an abort of nothing reports false"
        );
        assert!(!store.abort("s1", "nope", 1), "a token cannot name an upload that never existed");
        assert!(store.stored("s1").is_empty(), "an unknown session stores nothing");
    }

    #[test]
    fn install_reports_a_declared_size_that_does_not_match_the_bytes() {
        let mut store = store();
        let error = store
            .install("s1", "cp1", 5, 8, b"abcd".to_vec())
            .expect_err("declared size must match");
        assert_eq!(error.code, KeeperErrorCode::CheckpointChunkMismatch);
        assert!(store.checkpoint("s1").is_none());
        assert_eq!(store.committed_bytes(), 0);
    }

    #[test]
    fn aborting_an_upload_returns_its_charge_and_leaves_the_commit_alone() {
        let mut store = store();
        install(&mut store, "cp1", 10, b"aaaa");
        let charged_after_commit = store.committed_bytes() + store.pending_bytes();

        store.begin("s1", "cp2", 20, 32, 0).expect("begin");
        assert!(store.pending_bytes() > 0);
        let token = store.upload("s1", "cp2").expect("upload").upload_token();
        assert!(store.abort("s1", "cp2", token), "the open upload aborts");
        assert!(
            !store.abort("s1", "cp2", token),
            "a second abort is a no-op"
        );
        assert_eq!(
            store.committed_bytes() + store.pending_bytes(),
            charged_after_commit
        );
        assert_eq!(store.checkpoint("s1").expect("kept").sequence_offset, 10);
    }

    #[test]
    fn a_commit_that_lost_the_race_releases_its_upload_instead_of_stranding_the_charge() {
        let mut store = store();
        store.begin("s1", "cp2", 200, 16, 0).expect("begin");
        store.push_chunk("s1", "cp2", 0, &[0u8; 16]).expect("a complete upload");
        let pending = store.upload("s1", "cp2").expect("upload").charged_bytes();
        assert_eq!(store.pending_bytes() as u64, pending);

        install(&mut store, "cp3", 300, b"cccc");
        let committed = store.committed_bytes() as u64;

        let error = store
            .commit("s1", "cp2", 200)
            .expect_err("offset 200 is behind the committed 300");
        assert_eq!(error.code, KeeperErrorCode::StaleCheckpoint);
        assert_eq!(error.offset, Some(300));
        assert_eq!(
            store.pending_bytes(),
            0,
            "a stale upload can never commit, so its charge must not be held until the session dies"
        );
        assert!(store.upload("s1", "cp2").is_none(), "the stale upload is dropped");
        assert_eq!(store.committed_bytes() as u64, committed, "the newer commit is untouched");
    }

    #[test]
    fn a_stale_frame_for_a_live_upload_id_releases_only_the_dead_upload() {
        let mut store = store();
        store.begin("s1", "cpA", 200, 8, 0).expect("upload A");
        store.begin("s1", "cpB", 400, 8, 0).expect("upload B");
        let live = store.upload("s1", "cpB").expect("upload").charged_bytes();

        store.begin("s1", "cpC", 300, 8, 0).expect("upload C");
        store.push_chunk("s1", "cpC", 0, &[0u8; 8]).expect("complete");
        store.commit("s1", "cpC", 300).expect("commit at 300");

        let error = store
            .begin("s1", "cpA", 150, 8, 0)
            .expect_err("offset 150 is behind the committed 300");
        assert_eq!(error.code, KeeperErrorCode::StaleCheckpoint);
        assert!(
            store.upload("s1", "cpA").is_none(),
            "an upload at or below the committed offset can never commit and is released"
        );
        let kept = store.upload("s1", "cpB").expect("a newer upload survives the stale frame");
        assert_eq!(kept.sequence_offset(), 400);
        assert_eq!(store.pending_bytes() as u64, live, "only what is still committable stays charged");
    }

    #[test]
    fn an_install_under_an_open_upload_id_moves_that_uploads_charge_to_committed() {
        let mut store = store();
        store.begin("s1", "cp1", 10, 8, 0).expect("begin");
        store.push_chunk("s1", "cp1", 0, b"abcd").expect("partial chunk");
        assert!(store.pending_bytes() > 0);

        let info = install(&mut store, "cp1", 20, b"abcdefgh");
        assert_eq!(info.stored_bytes, 8);
        assert_eq!(store.pending_bytes(), 0, "the superseded upload returns its charge");
        assert!(store.upload("s1", "cp1").is_none());
        assert_eq!(
            store.committed_bytes(),
            store.checkpoint("s1").expect("committed").allocated_bytes(),
            "one allocation, one bucket, no double charge"
        );
    }

    #[test]
    fn a_retried_begin_reclaims_the_first_attempt_before_charging_the_second() {
        // Size the cap to exactly one attempt's measured charge, so the retry fits only if the
        // first charge is returned before the second is taken. With headroom above that, a retry
        // that charged twice would still pass and prove nothing.
        let mut probe = store();
        probe.begin("s1", "cp1", 10, 32, 0).expect("measure the charge");
        let charge = probe.upload("s1", "cp1").expect("upload").charged_bytes();
        assert!(charge >= 32);

        let mut store = store_with_cap(charge);
        store.begin("s1", "cp1", 10, 32, 0).expect("first attempt");
        assert_eq!(store.pending_bytes() as u64, charge, "the whole cap is one attempt");

        let progress = store.begin("s1", "cp1", 10, 32, 0).expect("the retry reclaims the first charge");
        assert_eq!(progress.received_bytes, 0);
        let second = store.upload("s1", "cp1").expect("upload").charged_bytes();
        assert_eq!(store.pending_bytes() as u64, second, "one upload, one charge");
        assert_eq!(store.pending_bytes() as u64, charge, "and it still fits the cap exactly");
    }

    #[test]
    fn a_retried_begin_under_the_same_id_does_not_charge_twice() {
        let mut store = store();
        store.begin("s1", "cp1", 10, 32, 0).expect("begin");
        let first = store.upload("s1", "cp1").expect("upload").charged_bytes();
        assert_eq!(store.pending_bytes() as u64, first);

        let progress = store.begin("s1", "cp1", 10, 32, 0).expect("retried begin");
        assert_eq!(progress.received_bytes, 0);
        let again = store.upload("s1", "cp1").expect("upload").charged_bytes();
        assert_eq!(store.pending_bytes() as u64, again, "one charge for one upload");

        store.push_chunk("s1", "cp1", 0, &[0u8; 32]).expect("chunk");
        let committed = store.upload("s1", "cp1").expect("upload").charged_bytes();
        store.commit("s1", "cp1", 10).expect("commit");
        assert_eq!(store.committed_bytes() as u64, committed);
        assert_eq!(store.pending_bytes(), 0);
    }

    #[test]
    fn a_commit_replaces_the_previous_checkpoint_and_releases_its_bytes() {
        let mut store = store();
        install(&mut store, "cp1", 10, b"aaaaaaaaaa");
        let superseded = store.checkpoint("s1").expect("committed").allocated_bytes();
        assert_eq!(store.committed_bytes(), superseded);

        let info = begin_and_commit(&mut store, "cp2", 20, b"bbb");
        assert_eq!(info.stored_bytes, 3);
        let replaced = store.checkpoint("s1").expect("newest").allocated_bytes();
        assert_eq!(
            store.committed_bytes(),
            replaced,
            "the superseded checkpoint is released and the new one is charged in full"
        );
        assert_eq!(store.checkpoint("s1").expect("newest").checkpoint_id, "cp2");
    }

    #[test]
    fn forgetting_a_session_returns_every_charge_it_held() {
        let mut store = store();
        install(&mut store, "cp1", 10, b"aaaa");
        store.begin("s1", "cp2", 20, 8, 0).expect("begin");
        let committed = store.checkpoint("s1").expect("committed").allocated_bytes();
        let pending = store.upload("s1", "cp2").expect("upload").charged_bytes();
        assert_eq!(store.committed_bytes(), committed);
        assert_eq!(store.pending_bytes() as u64, pending);

        store.forget_session("s1");
        assert_eq!(store.committed_bytes(), 0);
        assert_eq!(store.pending_bytes(), 0);
        assert!(store.checkpoint("s1").is_none());
        assert_eq!(store.session_count(), 0);

        // Forgetting an unknown session is a no-op, not a panic.
        store.forget_session("nope");
    }

    #[test]
    fn sessions_hold_their_checkpoints_independently() {
        let mut store = store();
        store.install("s1", "cp1", 10, 4, b"aaaa".to_vec()).expect("s1");
        store.install("s2", "cp1", 5, 3, b"bbb".to_vec()).expect("s2");

        assert_eq!(store.stored("s1").sequence_offset, 10);
        assert_eq!(store.stored("s2").sequence_offset, 5);
        assert_eq!(store.committed_bytes(), 7);

        // A stale upload for s1 does not touch s2's recovery point.
        assert_eq!(
            store
                .install("s1", "cp2", 10, 1, b"x".to_vec())
                .expect_err("stale for s1")
                .code,
            KeeperErrorCode::StaleCheckpoint
        );
        assert_eq!(store.stored("s2").stored_bytes, 3);
        assert_eq!(store.session_count(), 2);
    }

    #[test]
    fn a_committed_checkpoint_is_charged_its_measured_capacity_not_its_length() {
        // A buffer with slack: the store keeps the allocation, so it must bill the allocation.
        let mut padded = Vec::with_capacity(256);
        padded.extend_from_slice(b"abcd");

        let mut store = store();
        store
            .install("s1", "cp1", 10, 4, padded)
            .expect("install keeps the buffer");
        let checkpoint = store.checkpoint("s1").expect("committed");

        assert_eq!(checkpoint.len(), 4);
        assert_eq!(checkpoint.allocated_bytes(), checkpoint.bytes.capacity());
        assert_eq!(store.committed_bytes(), checkpoint.allocated_bytes());
        assert!(
            store.committed_bytes() >= 4,
            "a slack-bearing buffer is billed for the slack it keeps"
        );
        assert_eq!(store.pending_bytes(), 0);
    }

    #[test]
    fn an_install_whose_allocation_cannot_be_billed_keeps_the_previous_checkpoint() {
        let mut store = store_with_cap(4);
        install(&mut store, "cp1", 10, b"aaaa");
        assert_eq!(store.committed_bytes(), 4);

        // The incoming buffer exists before the call, and the committed one is still live, so the
        // peak does not fit and nothing may be released to make room for it.
        let error = store
            .install("s1", "cp2", 20, 4, b"bbbb".to_vec())
            .expect_err("the peak allocation must be refused");
        assert_eq!(error.code, KeeperErrorCode::CheckpointBudgetExhausted);

        assert_eq!(store.checkpoint("s1").expect("preserved").bytes(), &b"aaaa"[..]);
        assert_eq!(store.checkpoint("s1").expect("preserved").sequence_offset, 10);
        assert_eq!(
            store.committed_bytes(),
            store.checkpoint("s1").expect("preserved").allocated_bytes(),
            "a refused install releases nothing and charges nothing"
        );
    }

    #[test]
    fn an_upload_is_allocated_and_charged_its_whole_declared_size_at_begin() {
        // The buffer is sized once, at `begin`, so filling it cannot reallocate: the peak the budget
        // bounds is this one allocation, and no chunk can push it past what was already charged.
        // Headroom above the declared size, so the allocator rounding a 64 byte reservation cannot
        // itself make `begin` fail and turn this into a test of the round-up path.
        let mut store = store_with_cap(128);
        store.begin("s1", "cp1", 10, 64, 0).expect("begin");
        let upload = store.upload("s1", "cp1").expect("upload");
        assert!(upload.allocated_bytes() >= 64, "the declared size is allocated up front");
        assert_eq!(upload.charged_bytes(), expected_charge(upload));
        assert_eq!(store.pending_bytes() as u64, upload.charged_bytes());
        assert!(store.pending_bytes() as u64 <= 128);

        let capacity_before = upload.allocated_bytes();
        store
            .push_chunk("s1", "cp1", 0, &[0u8; 64])
            .expect("the declared size fits");
        let upload = store.upload("s1", "cp1").expect("upload");
        assert_eq!(
            upload.allocated_bytes(),
            capacity_before,
            "a chunk inside the declared size never triggers a reallocation"
        );
        assert_eq!(
            store.pending_bytes() as u64,
            upload.charged_bytes(),
            "no capacity appeared that the budget had not already admitted"
        );
    }

    #[test]
    fn an_upload_the_budget_cannot_allocate_is_refused_and_stores_nothing() {
        let mut store = store_with_cap(32);
        let error = store
            .begin("s1", "cp1", 10, 64, 0)
            .expect_err("the declared size must be admissible before it is allocated");
        assert_eq!(error.code, KeeperErrorCode::CheckpointBudgetExhausted);

        assert!(store.upload("s1", "cp1").is_none());
        assert_eq!(store.pending_bytes(), 0, "a refused begin leaves no charge behind");
        assert!(store.checkpoint("s1").is_none());
        assert_eq!(store.session_count(), 0, "no session record is created for a refusal");
    }

    #[test]
    fn a_download_reads_bounded_chunks_of_one_pinned_checkpoint() {
        let bytes: Vec<u8> = (0..(MAX_CHECKPOINT_READ_CHUNK + 100)).map(|i| (i % 251) as u8).collect();
        let mut store = store();
        store
            .install("s1", "cp1", 10, bytes.len() as u64, bytes.clone())
            .expect("install");

        let pinned = store.pin("s1").expect("pinned");
        assert_eq!(pinned.checkpoint_id, "cp1");
        assert_eq!(pinned.sequence_offset, 10);
        assert_eq!(pinned.total_bytes, bytes.len() as u64);

        let mut buffer = vec![0u8; bytes.len() + MAX_CHECKPOINT_READ_CHUNK];
        let mut collected = Vec::new();
        let mut offset = 0;
        loop {
            let chunk = store.read_chunk(&pinned, offset, &mut buffer).expect("chunk");
            assert_eq!(chunk.checkpoint_id, "cp1");
            assert_eq!(chunk.sequence_offset, 10);
            assert_eq!(chunk.offset, offset);
            assert_eq!(chunk.total_bytes, bytes.len() as u64);
            assert!(chunk.filled <= MAX_CHECKPOINT_READ_CHUNK);
            collected.extend_from_slice(&buffer[..chunk.filled]);
            offset = chunk.next_offset();
            if chunk.complete {
                break;
            }
        }
        assert_eq!(collected, bytes);
        assert_eq!(offset, bytes.len() as u64);
    }

    #[test]
    fn a_download_fails_closed_when_the_checkpoint_is_replaced_underneath_it() {
        let mut store = store();
        store
            .install("s1", "cp1", 10, 8, b"aaaaaaaa".to_vec())
            .expect("install");
        let pinned = store.pin("s1").expect("pinned");

        let mut buffer = [0u8; 4];
        let first = store.read_chunk(&pinned, 0, &mut buffer).expect("first chunk");
        assert_eq!(&buffer[..first.filled], b"aaaa");
        assert!(!first.complete);

        // A newer checkpoint lands between two chunks of the download.
        store
            .install("s1", "cp2", 20, 8, b"bbbbbbbb".to_vec())
            .expect("replacement");

        let error = store
            .read_chunk(&pinned, first.next_offset(), &mut buffer)
            .expect_err("the stream must not splice two generations together");
        assert_eq!(error.code, KeeperErrorCode::StaleCheckpoint);
        assert_eq!(error.offset, Some(20), "the newest committed offset is reported");
    }

    #[test]
    fn a_download_of_a_dropped_checkpoint_fails_closed() {
        let mut store = store();
        store
            .install("s1", "cp1", 10, 8, b"aaaaaaaa".to_vec())
            .expect("install");
        let pinned = store.pin("s1").expect("pinned");
        store.forget_session("s1");

        let mut buffer = [0u8; 4];
        let error = store
            .read_chunk(&pinned, 0, &mut buffer)
            .expect_err("a forgotten session has nothing to serve");
        assert_eq!(error.code, KeeperErrorCode::StaleCheckpoint);
    }

    #[test]
    fn a_download_past_the_end_and_an_empty_store_are_typed_answers() {
        let mut store = store();
        assert!(store.pin("s1").is_none(), "nothing committed, nothing to pin");

        store
            .install("s1", "cp1", 10, 4, b"aaaa".to_vec())
            .expect("install");
        let pinned = store.pin("s1").expect("pinned");

        let mut buffer = [0u8; 8];
        let error = store
            .read_chunk(&pinned, 5, &mut buffer)
            .expect_err("offset past the end");
        assert_eq!(error.code, KeeperErrorCode::CheckpointChunkMismatch);
    }

    #[test]
    fn an_empty_checkpoint_id_is_refused_before_anything_is_charged_or_trimmed() {
        // The empty id is `CheckpointInfo::default`'s "nothing committed" sentinel. A checkpoint
        // stored under it is unaddressable - `stored()` reports none - while the replay prefix the
        // install trimmed is already gone. Both entry points refuse it before allocating.
        let mut store = store();
        install(&mut store, "cp1", 100, b"aaaa");
        let committed = store.committed_bytes();

        let error = store
            .install("s1", "", 200, 4, b"bbbb".to_vec())
            .expect_err("an empty id is not a checkpoint id");
        assert_eq!(error.code, KeeperErrorCode::BadFrame);
        assert_eq!(store.pending_bytes(), 0, "nothing was charged");
        assert_eq!(store.committed_bytes(), committed, "the commit is untouched");

        let error = store
            .begin("s1", "", 300, 64, 0)
            .expect_err("an empty id cannot open an upload");
        assert_eq!(error.code, KeeperErrorCode::BadFrame);
        assert_eq!(store.pending_bytes(), 0, "a refused begin charges nothing");
        assert!(store.upload("s1", "").is_none(), "no upload was recorded");
    }

    #[test]
    fn an_abort_only_releases_the_attempt_its_token_names() {
        // Two attempts on one `checkpoint_id`: the second supersedes the first, so the second's token
        // names the live buffer. An abort carrying the first token must not release it - that is the
        // wrong-victim failure the epoch alone could not prevent.
        let mut store = store();
        let first = store.begin("s1", "cp1", 10, 32, 1).expect("first attempt");
        let second = store.begin("s1", "cp1", 10, 32, 1).expect("retry supersedes it");
        assert_ne!(first.upload_token, second.upload_token);
        assert_eq!(store.upload("s1", "cp1").expect("upload").upload_token(), second.upload_token);

        let live = store.pending_bytes();
        assert!(live > 0);
        assert!(!store.abort("s1", "cp1", first.upload_token), "the superseded attempt is gone");
        assert_eq!(store.pending_bytes(), live, "the live attempt keeps its charge");
        assert!(store.abort("s1", "cp1", second.upload_token), "the live attempt aborts");
        assert_eq!(store.pending_bytes(), 0);
    }

    #[test]
    fn an_epoch_advance_retires_the_predecessors_uploads_and_spares_the_successors() {
        let mut store = store();
        store.begin("s1", "old", 10, 64, 1).expect("epoch 1 upload");
        let retired_charge = store.pending_bytes();
        assert!(retired_charge > 0);

        // The successor's own upload is opened at the new epoch and must survive.
        store.begin("s1", "new", 20, 64, 2).expect("epoch 2 upload");
        let both = store.pending_bytes();
        assert!(both > retired_charge);

        assert_eq!(store.retire_uploads_before_epoch("s1", 2), 1, "one attempt was retired");
        assert!(store.upload("s1", "old").is_none(), "the predecessor's upload is gone");
        assert!(store.upload("s1", "new").is_some(), "the successor's upload is untouched");
        assert_eq!(store.pending_bytes(), both - retired_charge, "exactly the retired charge returned");

        assert_eq!(store.retire_uploads_before_epoch("s1", 2), 0, "nothing left below epoch 2");
        assert_eq!(store.pending_bytes(), both - retired_charge, "a no-op retires nothing");
    }

    #[test]
    fn a_zero_byte_retry_supersedes_on_presence_not_on_charge() {
        // A zero-byte retry charges nothing, so a `charged_bytes > 0` gate left the first attempt in
        // place to shadow it: `push_chunk`, `commit` and `abort` all select the first match, so the
        // real upload could never take a chunk and the stale metadata accumulated without bound.
        let mut store = store();
        let first = store.begin("s1", "cp1", 10, 0, 0).expect("zero-byte attempt");
        let second = store.begin("s1", "cp1", 10, 0, 0).expect("zero-byte retry");

        assert_ne!(first.upload_token, second.upload_token);
        assert_eq!(
            store.upload("s1", "cp1").expect("upload").upload_token(),
            second.upload_token,
            "the retry is the upload the store keeps"
        );
        assert_eq!(store.pending_bytes(), 0, "a zero-byte attempt charges nothing either way");
        assert!(
            !store.abort("s1", "cp1", first.upload_token),
            "the superseded attempt is gone, not merely uncharged"
        );
        assert!(
            store.abort("s1", "cp1", second.upload_token),
            "the retry is addressable by its own token"
        );
        assert!(store.upload("s1", "cp1").is_none());
    }
}
