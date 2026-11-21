//! Per-session bounded replay log.
//!
//! The log is byte-oriented and opaque: the keeper never parses escape sequences or UTF-8, so it
//! never claims to resume at a terminal-language boundary. PTY reads land on arbitrary byte
//! boundaries, so an escape sequence or a multi-byte character may be split across two pushes; the
//! log keeps the bytes in order and makes no attempt to align them. [`REPLAY_SESSION_BYTES`] caps
//! what one session retains.
//!
//! Offsets are absolute, monotonically increasing byte positions in the session's output stream:
//! `start_offset` is the first byte still retained, [`ReplayLog::head`] is one past the newest byte
//! ever pushed. A consumer that asks for output before `start_offset` has lost it, and the answer
//! is a [`ReplayGap`] whose `resumed_at_offset` is the log's current head.
//!
//! ## Storage
//!
//! One log owns exactly one byte buffer of [`ReplayLog::capacity`] bytes, allocated once in
//! [`ReplayLog::new`] and never resized. Retention is a window over that buffer, so a push copies
//! its bytes in and a truncation only advances the window: neither allocates, and the byte storage
//! never exceeds `capacity`. That is why the keeper's per-session charge is an exact figure rather
//! than an estimate: it reserves [`REPLAY_SESSION_BYTES`], and that constant *is* this log's whole
//! allocation - `vec![0u8; capacity]` boxed, with nothing reserved for a growth step that cannot
//! happen. A doubling `Vec` would need headroom here, since it can hold nearly twice the bytes it
//! retains and a truncation that rebuilds the tail holds both copies while it does; this one does
//! not.
//!
//! Allocating once at construction means the reservation and the allocation are taken at the same
//! moment, on the spawn path, rather than at an unpredictable point later when a busy session first
//! crosses a growth step. The pages behind it are touched as output is written, so an idle session
//! costs address space, not resident memory.
//!
//! ## Overflow contract (lossy reset)
//!
//! When a push would exceed the cap, the retained suffix is discarded and the resume offset is
//! established at the current output head, in one step under the log's lock. A push either retains
//! its bytes in full or retains none of them: a partially retained suffix is never kept, so either
//! the bytes are all there for their offset range, or the range is gone and a gap is reported.
//!
//! A push never refuses and never blocks. The pump thread must keep draining the PTY whatever the
//! log can hold - stalling the read would back-pressure the child and, with a full PTY buffer,
//! wedge the very session the keeper exists to keep alive. So when the cap cannot take a run, the
//! log discards what it retained and publishes the gap in the same step; it never asks the producer
//! to wait, and it never drops bytes without telling a consumer that the range is gone.
//!
//! This is an explicitly lossy reset. It does not resume at a terminal-language boundary and does
//! not preserve the running program's display state. What it does guarantee about ordering: the
//! gap is published atomically with the reset, so a consumer that observes the new head can also
//! observe the gap, and the daemon must reset its parser before consuming any byte at
//! `resumed_at_offset`. The bytes retained after the reset begin at whatever byte the PTY read
//! returned at that moment; that boundary is arbitrary and may itself fall inside an escape
//! sequence or a multi-byte character. That is why the contract is "reset, do not interpret what
//! came before", never "resume at a clean boundary".
//!
//! ## Copies made when answering a consumer
//!
//! The log itself never allocates above its capacity. Answering a consumer, however, copies
//! retained bytes out, and that copy is not part of the per-session reservation:
//!
//! * [`ReplayLog::read_from`] and [`ReplayLog::snapshot`] return a fresh `Vec` of up to the whole
//!   retained suffix while the log still holds its own copy, so one live answer costs a second
//!   buffer. [`ReplayLog::read_into`] fills a caller-owned buffer instead, so a caller that keeps a
//!   reusable buffer pays nothing per answer.
//! * [`ReplayLog::covered_len`] reports the exact size of that buffer before it is allocated, which
//!   is the hook a caller uses to charge it against a budget first and release the charge once the
//!   response has been written out.
//!
//! Neither the charge nor the release can be enforced here: this module sees neither the keeper's
//! budget nor the connection that writes the reply. What it can state, and what holds for the whole
//! life of the log, is that the log's own footprint is exactly `capacity` and that a response copy
//! is chargeable before it exists. Reserving that size, keeping one response in flight per
//! connection, and holding the reservation until the socket write completes all belong to the
//! caller; a per-session cap bounds the log, never the sum of answers in flight.
//!
//! Both copy paths hold the log's lock for the duration of the copy, because the buffer they read
//! is the buffer a push writes. A reader answering a full 8 MiB suffix therefore keeps the pump
//! thread out of [`ReplayLog::push`] for one memcpy of that size. That is a latency note, not a
//! budget one: it does not grow the footprint, but it does mean a response copy is never free to
//! the producer, and a caller that must not delay the PTY drain should page from a smaller cap
//! rather than assume the read is instant.

use crate::error::{KeeperError, KeeperErrorCode, KeeperResult};

/// The per-session replay cap (8 MiB), re-exported so the keeper charges exactly what a log keeps.
pub use crate::REPLAY_SESSION_BYTES;

/// A discontinuity in the replay stream. The consumer must reset terminal state and resume at
/// `resumed_at_offset`; everything before it is lost and was never partially retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayGap {
    /// Absolute output offset the consumer must resume at: the head at the moment of the reset.
    pub resumed_at_offset: u64,
    /// Absolute offset the consumer asked for and did not get.
    ///
    /// A gap produced by [`ReplayLog::read_from`] carries the caller's own `after_offset`, which
    /// may itself be past the head when the caller asked for output that does not exist yet. A gap
    /// recorded by an overflowing [`ReplayLog::push`] carries the head as it stood before that
    /// push: the first offset of the run the log declined to retain, always below the resume
    /// offset.
    /// Either way the consumer resumes at `resumed_at_offset` and never at this value.
    pub requested_after_offset: u64,
    /// Retained lower bound when the gap was produced.
    pub retained_from_offset: u64,
}

impl ReplayGap {
    pub fn new(requested_after_offset: u64, resumed_at_offset: u64) -> Self {
        Self { resumed_at_offset, requested_after_offset, retained_from_offset: resumed_at_offset }
    }

    /// The typed `REPLAY_GAP` refusal, carrying the offset the daemon must continue at.
    pub fn as_error(&self, session_id: &str) -> KeeperError {
        KeeperError::at(
            KeeperErrorCode::ReplayGap,
            format!(
                "session {session_id}: replay gap, requested after offset {} but resumed at {}",
                self.requested_after_offset, self.resumed_at_offset
            ),
            self.resumed_at_offset,
        )
    }
}

/// Bounded per-session replay log. Bytes in, bytes out, no interpretation.
///
/// Deliberately not `Clone`: a clone would duplicate the whole fixed allocation, so a caller that
/// wants a second view shares the log (`Arc<Mutex<ReplayLog>>`, as the keeper does) instead of
/// copying its bytes and doubling its footprint.
pub struct ReplayLog {
    session_id: String,
    /// Exactly `capacity` bytes, allocated once. Retention is the window `start..start + len`,
    /// wrapping at the end of the buffer; the slice's length is the cap and never changes.
    storage: Box<[u8]>,
    /// Index of the oldest retained byte. Always `0` when `len == 0`.
    start: usize,
    /// Retained bytes; also the length of the window starting at `start`.
    len: usize,
    start_offset: u64,
    head_offset: u64,
    reset_count: u64,
    gap: Option<ReplayGap>,
}

impl std::fmt::Debug for ReplayLog {
    // Summarises rather than dumping the buffer: an 8 MiB log must not be rendered byte by byte.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplayLog")
            .field("session_id", &self.session_id)
            .field("capacity", &self.capacity())
            .field("retained", &self.len)
            .field("start_offset", &self.start_offset)
            .field("head_offset", &self.head_offset)
            .field("reset_count", &self.reset_count)
            .finish()
    }
}

impl ReplayLog {
    /// A log holding at most `capacity` bytes for `session_id`.
    ///
    /// The buffer is allocated here, at its final size, and boxed so the allocation is exactly the
    /// capacity: `vec![0u8; capacity]` is zeroed with `alloc_zeroed` and `into_boxed_slice` leaves
    /// no spare capacity behind for a later push to grow into. A large zeroed allocation usually
    /// arrives as untouched pages that fault in as they are written, so a session with little
    /// output does not pay for the whole reservation up front.
    pub fn new(session_id: impl Into<String>, capacity: usize) -> Self {
        Self {
            session_id: session_id.into(),
            storage: vec![0u8; capacity].into_boxed_slice(),
            start: 0,
            len: 0,
            start_offset: 0,
            head_offset: 0,
            reset_count: 0,
            gap: None,
        }
    }

    /// A log at the plan's per-session cap.
    pub fn with_default_capacity(session_id: impl Into<String>) -> Self {
        Self::new(session_id, REPLAY_SESSION_BYTES)
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn capacity(&self) -> usize {
        self.storage.len()
    }

    /// Bytes this log actually holds, allocation included - the number a budget must charge for it.
    /// Equal to [`ReplayLog::capacity`] for the whole life of the log.
    pub fn allocated_bytes(&self) -> usize {
        self.storage.len()
    }

    /// Absolute offset of the next byte to be pushed: the output head.
    pub fn head(&self) -> u64 {
        self.head_offset
    }

    /// Absolute offset of the next byte to be pushed. Same value as [`ReplayLog::head`], under the
    /// name the wire's `sequenceOffset` uses.
    pub fn head_offset(&self) -> u64 {
        self.head_offset
    }

    /// Absolute offset of the oldest retained byte; equals the head when nothing is retained.
    pub fn start_offset(&self) -> u64 {
        self.start_offset
    }

    /// Bytes currently retained, independent of the allocation that holds them.
    pub fn retained_bytes(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of lossy resets this log has performed. Telemetry for `OWNERSHIP_LIST`.
    pub fn reset_count(&self) -> u64 {
        self.reset_count
    }

    /// The most recent overflow gap, until a consumer takes it.
    pub fn pending_gap(&self) -> Option<ReplayGap> {
        self.gap
    }

    /// Takes the pending overflow gap, so a consumer sees each reset exactly once.
    pub fn take_gap(&mut self) -> Option<ReplayGap> {
        self.gap.take()
    }

    /// Appends output, returning the absolute offset the pushed run starts at.
    ///
    /// Never allocates: the bytes are copied into the fixed buffer, wrapping if the window crosses
    /// its end. Overflow discards the retained suffix entirely and anchors the resume offset at the
    /// new head, publishing a [`ReplayGap`] in the same step.
    ///
    /// The run that triggered the reset is not retained either. Its bytes are exactly
    /// `[requested_after_offset, resumed_at_offset)`, the half-open range the gap declares lost, so
    /// a consumer that resumes at the head never sees part of the run that caused the
    /// discontinuity; it sees only output pushed afterwards, starting at an arbitrary read
    /// boundary.
    ///
    /// Overflow never trims from the front to make room. A front trim would leave a suffix whose
    /// first byte sits at an offset no consumer was told about, which is a silent discontinuity;
    /// the only front trim this log performs is [`ReplayLog::truncate_before`], where a committed
    /// checkpoint covers exactly the bytes being dropped.
    pub fn push(&mut self, bytes: &[u8]) -> u64 {
        let start = self.head_offset;
        if bytes.is_empty() {
            return start;
        }
        self.head_offset = self.head_offset.saturating_add(bytes.len() as u64);
        let capacity = self.capacity();
        // Overflow-free: the second arm is evaluated only when the run itself fits, so `capacity
        // - bytes.len()` cannot underflow.
        let overflows = bytes.len() > capacity || self.len > capacity - bytes.len();
        if overflows {
            // The window empties and the resume offset moves in one update: a consumer either sees
            // the old suffix with the old head, or no suffix with the new head and the gap.
            self.start = 0;
            self.len = 0;
            self.start_offset = self.head_offset;
            self.reset_count = self.reset_count.saturating_add(1);
            // The resume point is the head, never the first discarded byte: nothing of the previous
            // suffix is retained, so the head is both the resume offset and the retained bound.
            self.gap = Some(self.gap_for(start));
            return start;
        }
        let write_at = self.write_index(self.len);
        let first = (capacity - write_at).min(bytes.len());
        self.storage[write_at..write_at + first].copy_from_slice(&bytes[..first]);
        if first < bytes.len() {
            self.storage[..bytes.len() - first].copy_from_slice(&bytes[first..]);
        }
        self.len += bytes.len();
        self.start_offset = self.head_offset.saturating_sub(self.len as u64);
        start
    }

    /// Drops everything retained before `offset`, keeping the tail so `offset..head` still replays.
    ///
    /// This is the checkpoint case: the committed checkpoint describes terminal state up to
    /// `offset`, and the bytes after it are what a reattaching daemon applies on top. It only moves
    /// the window: no byte is copied, so unlike a tail-rebuild the old and new suffix are never
    /// resident at the same time. Returns the new retained lower bound.
    ///
    /// The allocation is the same before and after, so a keeper that reserves the whole per-session
    /// cap up front (as the production keeper does) has nothing to release here: the call buys back
    /// no memory, only a shorter window and a truthful `start_offset`.
    pub fn truncate_before(&mut self, offset: u64) -> u64 {
        let offset = offset.min(self.head_offset);
        if offset <= self.start_offset {
            return self.start_offset;
        }
        // `offset <= head` and the window always ends at the head, so this never exceeds `len`.
        let dropped = ((offset - self.start_offset) as usize).min(self.len);
        if dropped == 0 {
            return self.start_offset;
        }
        self.start_offset += dropped as u64;
        self.len -= dropped;
        if self.len == 0 {
            self.start = 0;
            self.start_offset = self.head_offset;
        } else {
            self.start = self.write_index(dropped);
        }
        self.start_offset
    }

    /// Whether `after_offset` still lies in the retained window, so no byte from it to the head has
    /// been lost. `true` at the head itself: there is nothing new to serve, which is not a loss.
    pub fn covers(&self, after_offset: u64) -> bool {
        after_offset >= self.start_offset && after_offset <= self.head_offset
    }

    /// How many bytes [`ReplayLog::read_from`] would return for `after_offset`, or `None` when that
    /// range is a gap.
    ///
    /// This is the charge-before-allocate hook: a caller reserves exactly this many bytes of
    /// response buffer before the read, so a replay answer is accounted for like any other
    /// allocation instead of appearing after the fact.
    pub fn covered_len(&self, after_offset: u64) -> Option<usize> {
        if after_offset > self.head_offset || after_offset < self.start_offset {
            return None;
        }
        Some((self.head_offset - after_offset) as usize)
    }

    /// Bytes in `[after_offset, head)`, or a [`ReplayGap`] when the log no longer covers the range.
    ///
    /// An `after_offset` past the head is refused too: the caller is asking about output that does
    /// not exist yet, and inventing an empty replay for it would look like a clean history.
    pub fn read_from(&self, after_offset: u64) -> Result<Vec<u8>, ReplayGap> {
        let mut out = Vec::new();
        self.read_into(after_offset, &mut out)?;
        Ok(out)
    }

    /// [`ReplayLog::read_from`] into a caller-owned buffer, returning the byte count.
    ///
    /// `out` is cleared and then filled with the needed size reserved up front; on a gap it is left
    /// untouched and nothing is copied.
    pub fn read_into(&self, after_offset: u64, out: &mut Vec<u8>) -> Result<usize, ReplayGap> {
        let Some(len) = self.covered_len(after_offset) else {
            return Err(self.gap_for(after_offset));
        };
        out.clear();
        if len == 0 {
            return Ok(0);
        }
        let capacity = self.capacity();
        let begin = self.write_index((after_offset - self.start_offset) as usize);
        let first = (capacity - begin).min(len);
        out.reserve_exact(len);
        out.extend_from_slice(&self.storage[begin..begin + first]);
        if first < len {
            out.extend_from_slice(&self.storage[..len - first]);
        }
        Ok(len)
    }

    /// [`ReplayLog::read_into`] for at most `limit` bytes, so a reply never allocates the whole
    /// retained suffix.
    ///
    /// Returns the bytes copied and whether the log still holds more past them. `out` is reserved
    /// for the copied length alone, so the allocation this call makes is bounded by `limit` no
    /// matter how much the log retains.
    pub fn read_into_bounded(
        &self,
        after_offset: u64,
        limit: usize,
        out: &mut Vec<u8>,
    ) -> Result<(usize, bool), ReplayGap> {
        let Some(len) = self.covered_len(after_offset) else {
            return Err(self.gap_for(after_offset));
        };
        out.clear();
        let take = len.min(limit);
        if take == 0 {
            return Ok((0, false));
        }
        let capacity = self.capacity();
        let begin = self.write_index((after_offset - self.start_offset) as usize);
        let first = (capacity - begin).min(take);
        out.reserve_exact(take);
        out.extend_from_slice(&self.storage[begin..begin + first]);
        if first < take {
            out.extend_from_slice(&self.storage[..take - first]);
        }
        Ok((take, len > take))
    }

    /// [`ReplayLog::read_from`] with the typed refusal the server returns instead of a bare gap.
    pub fn try_read_from(&self, after_offset: u64) -> KeeperResult<Vec<u8>> {
        self.read_from(after_offset)
            .map_err(|gap| gap.as_error(&self.session_id))
    }

    /// Everything retained, with the offset its first byte sits at.
    pub fn snapshot(&self) -> (u64, Vec<u8>) {
        let mut out = Vec::with_capacity(self.len);
        if self.len > 0 {
            let capacity = self.capacity();
            let first = (capacity - self.start).min(self.len);
            out.extend_from_slice(&self.storage[self.start..self.start + first]);
            if first < self.len {
                out.extend_from_slice(&self.storage[..self.len - first]);
            }
        }
        (self.start_offset, out)
    }

    /// Buffer index of the byte `skip` positions into the window. Only meaningful for a non-empty
    /// window, which is the only case that has a capacity to take a remainder of.
    fn write_index(&self, skip: usize) -> usize {
        let capacity = self.capacity();
        debug_assert!(capacity > 0, "a window over a zero-capacity log has no index");
        (self.start + skip) % capacity
    }

    fn gap_for(&self, requested_after_offset: u64) -> ReplayGap {
        ReplayGap {
            resumed_at_offset: self.head_offset,
            requested_after_offset,
            retained_from_offset: self.start_offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_advances_head_and_replays_every_byte() {
        let mut log = ReplayLog::new("s1", 64);
        assert_eq!(log.push(b"hello"), 0);
        assert_eq!(log.push(b" world"), 5);
        assert_eq!(log.head(), 11);
        assert_eq!(log.start_offset(), 0);
        assert_eq!(log.read_from(0).expect("covered"), b"hello world");
        assert_eq!(log.read_from(6).expect("covered"), b"world");
        assert!(log.covers(11));
        assert_eq!(log.allocated_bytes(), 64);
    }

    #[test]
    fn reading_at_the_head_is_empty_rather_than_a_gap() {
        let mut log = ReplayLog::new("s1", 8);
        log.push(b"abc");
        assert_eq!(log.read_from(3).expect("nothing new"), Vec::<u8>::new());
        assert_eq!(log.covered_len(3), Some(0));
    }

    #[test]
    fn overflow_discards_the_retained_suffix_and_resumes_at_the_current_head() {
        let mut log = ReplayLog::new("s1", 8);
        log.push(b"abcdefgh");
        assert_eq!(log.retained_bytes(), 8);

        // This push overflows: the retained suffix goes away entirely.
        log.push(b"ij");
        assert_eq!(log.head(), 10);
        assert_eq!(log.retained_bytes(), 0, "the retained suffix is discarded entirely");
        assert_eq!(log.start_offset(), 10);
        assert!(log.is_empty());
        assert_eq!(log.reset_count(), 1);
        assert_eq!(log.allocated_bytes(), 8, "the reset frees the window, not the allocation");

        let gap = log.take_gap().expect("overflow must publish a gap");
        assert_eq!(gap.resumed_at_offset, 10);
        assert_eq!(gap.resumed_at_offset, log.head(), "the resume offset is the current head");
        assert_eq!(gap.requested_after_offset, 8);
        assert_eq!(gap.retained_from_offset, 10);
        assert!(log.take_gap().is_none(), "a gap is consumed exactly once");

        // The seek that produced the overflow is refused, naming the resume offset.
        let error = log.try_read_from(0).expect_err("lost range must not replay");
        assert_eq!(error.code, KeeperErrorCode::ReplayGap);
        assert_eq!(error.offset, Some(10));

        // Output after the reset accumulates from the head, and only that is replayable.
        log.push(b"klm");
        assert_eq!(log.read_from(10).expect("post-reset range"), b"klm");
        assert!(log.covers(10) && log.covers(13));
        assert!(!log.covers(9), "the byte before the resume offset is not covered");
    }

    #[test]
    fn a_run_larger_than_the_cap_is_lost_whole() {
        let mut log = ReplayLog::new("s1", 4);
        log.push(b"0123456789");
        assert_eq!(log.head(), 10);
        assert!(log.is_empty());
        assert_eq!(log.take_gap().expect("gap").resumed_at_offset, 10);
        assert_eq!(log.try_read_from(0).expect_err("lost").offset, Some(10));
    }

    #[test]
    fn a_split_escape_sequence_and_multibyte_character_are_never_partially_retained() {
        // A run that straddles the cap is dropped whole rather than cut: no parser is ever handed
        // the tail of a suffix whose head was thrown away. (The retained run that follows starts at
        // an arbitrary PTY read boundary - that is the lossy part of the contract, not a promise
        // the log makes about sequence alignment.)
        let mut log = ReplayLog::new("s1", 6);
        log.push(b"\x1b[3"); // 3 bytes: ESC, '[', '3'
        log.push(b"1m\xe4\xb8"); // 4 bytes: the CSI tail plus the first two bytes of a 3-byte char
        assert_eq!(log.head(), 7);
        assert!(log.is_empty(), "the overflowing run is not partially kept");
        assert_eq!(log.retained_bytes(), 0);
        assert_eq!(log.take_gap().expect("gap").resumed_at_offset, 7);

        // Everything after the reset is retained and replays from the head exactly.
        log.push(b"\xad\x1b[0m");
        assert_eq!(log.read_from(7).expect("covered"), b"\xad\x1b[0m");
    }

    #[test]
    fn a_sequence_split_across_the_reset_is_replayed_from_the_head_verbatim() {
        let mut log = ReplayLog::new("s1", 8);
        // A CSI sequence split by a PTY read boundary, then enough output to overflow the cap: the
        // 2 retained bytes plus this 8-byte run cannot both fit in 8.
        log.push(b"\x1b[");
        assert_eq!(log.retained_bytes(), 2);
        log.push(b"1;32mred");
        assert_eq!(log.head(), 10);
        assert!(log.is_empty(), "the overflow discards the retained suffix whole");
        assert_eq!(log.take_gap().expect("gap").resumed_at_offset, 10);

        // The next read begins in the middle of a sequence (the CSI tail). The log does not align
        // it and must not: it replays those bytes verbatim from the head, and the daemon resets its
        // parser before consuming them.
        log.push(b"mst");
        assert_eq!(log.read_from(10).expect("post-reset"), b"mst");
        assert!(log.read_from(9).is_err(), "no pre-head byte is served after a reset");
    }

    #[test]
    fn the_gap_is_published_with_the_same_head_that_new_bytes_are_readable_from() {
        let mut log = ReplayLog::new("s1", 4);
        log.push(b"abc");
        log.push(b"def"); // overflows at head 6
        let gap = log.pending_gap().expect("published at the reset, not on demand");
        assert_eq!(gap.resumed_at_offset, 6);
        assert_eq!(gap.requested_after_offset, 3, "the run whose push overflowed");
        assert!(log.read_from(3).is_err(), "the byte before the head is not served");

        log.push(b"gh");
        assert_eq!(log.pending_gap(), Some(gap), "still pending until the consumer takes it");
        assert_eq!(log.take_gap(), Some(gap));
        assert_eq!(log.read_from(6).expect("post-reset"), b"gh");
    }

    #[test]
    fn truncate_before_a_checkpoint_offset_keeps_the_tail_after_it() {
        let mut log = ReplayLog::new("s1", 64);
        log.push(b"0123456789");
        assert_eq!(log.truncate_before(4), 4);
        assert_eq!(log.start_offset(), 4);
        assert_eq!(log.retained_bytes(), 6);
        assert_eq!(log.allocated_bytes(), 64, "a truncation moves the window without reallocating");
        assert_eq!(log.read_from(4).expect("at the checkpoint offset"), b"456789");
        // Pre-checkpoint bytes are gone; asking for them is a gap, not a silent drop.
        assert!(log.read_from(0).is_err());
        log.push(b"ab");
        assert_eq!(log.read_from(4).expect("covered"), b"456789ab");
    }

    #[test]
    fn truncate_at_the_head_leaves_nothing_retained() {
        let mut log = ReplayLog::new("s1", 64);
        log.push(b"0123456789");
        assert_eq!(log.truncate_before(10), 10);
        assert!(log.is_empty());
        assert_eq!(log.start_offset(), log.head());
        assert_eq!(log.read_from(10).expect("resume point"), Vec::<u8>::new());
        // A truncation past the head is clamped, never a window running beyond the output.
        assert_eq!(log.truncate_before(99), 10);
    }

    #[test]
    fn a_wrapped_window_replays_in_stream_order() {
        let mut log = ReplayLog::new("s1", 8);
        log.push(b"abcd");
        assert_eq!(log.truncate_before(2), 2);
        assert_eq!(log.retained_bytes(), 2);

        // 6 bytes onto a 2-byte tail: the window now wraps the end of the buffer.
        log.push(b"efghij");
        assert_eq!(log.head(), 10);
        assert_eq!(log.retained_bytes(), 8);
        assert_eq!(log.start_offset(), 2);
        assert_eq!(log.read_from(2).expect("covered"), b"cdefghij");
        assert_eq!(log.read_from(6).expect("covered"), b"ghij");
        assert_eq!(log.snapshot(), (2, b"cdefghij".to_vec()));
        assert_eq!(log.allocated_bytes(), 8);

        // Truncating inside a wrapped window still keeps exactly the newest bytes.
        assert_eq!(log.truncate_before(5), 5);
        assert_eq!(log.retained_bytes(), 5);
        assert_eq!(log.read_from(5).expect("covered"), b"fghij");
        assert_eq!(log.allocated_bytes(), 8);
    }

    #[test]
    fn pushes_and_truncations_never_grow_the_allocation() {
        let mut log = ReplayLog::new("s1", 16);
        let allocated = log.allocated_bytes();
        assert_eq!(allocated, 16);
        assert_eq!(log.capacity(), 16);
        for byte in 0..64u8 {
            log.push(&[byte]);
            assert_eq!(log.allocated_bytes(), allocated, "no push may resize the buffer");
            assert!(log.retained_bytes() <= log.capacity());
        }
        let keep_from = log.head().saturating_sub(4);
        log.truncate_before(keep_from);
        assert_eq!(log.allocated_bytes(), allocated);
        assert_eq!(log.retained_bytes(), 4, "the four bytes from keep_from to the head");
        assert_eq!(log.read_from(keep_from).expect("covered"), vec![60, 61, 62, 63]);
    }

    #[test]
    fn covered_len_and_read_into_reuse_a_precharged_response_buffer() {
        let mut log = ReplayLog::new("s1", 8);
        log.push(b"abcdefgh");
        log.push(b"i"); // overflow: the suffix is discarded, the head moves to 9
        assert_eq!(log.covered_len(0), None);
        log.push(b"xyz");
        assert_eq!(log.covered_len(log.head()), Some(0));
        assert_eq!(log.covered_len(9), Some(3));

        // The charge-before-allocate path: a buffer reserved at exactly the reported size is filled
        // without allocating again, so what the caller charged is what the response actually costs.
        let mut out = Vec::with_capacity(3);
        let reserved = out.capacity();
        assert_eq!(log.read_into(9, &mut out).expect("covered"), 3);
        assert_eq!(out, b"xyz");
        assert_eq!(out.capacity(), reserved, "the pre-charged buffer was reused, not regrown");

        // A gap copies nothing, so the buffer keeps whatever it already held.
        let gap = log.read_into(0, &mut out).expect_err("a gap");
        assert_eq!(gap.requested_after_offset, 0);
        assert_eq!(gap.resumed_at_offset, 12);
        assert_eq!(out, b"xyz", "a refused read leaves the buffer untouched");

        // A second read clears and refills the same buffer.
        assert_eq!(log.read_into(12, &mut out).expect("head"), 0);
        assert!(out.is_empty());
    }

    #[test]
    fn a_zero_capacity_log_retains_nothing_and_reports_every_gap() {
        let mut log = ReplayLog::new("s1", 0);
        assert_eq!(log.capacity(), 0);
        assert_eq!(log.allocated_bytes(), 0);
        log.push(b"x");
        assert_eq!(log.head(), 1);
        assert!(log.is_empty());
        assert_eq!(log.take_gap().expect("gap").resumed_at_offset, 1);
        assert_eq!(log.snapshot(), (1u64, Vec::<u8>::new()));
        assert!(log.read_from(0).is_err());
        assert_eq!(log.read_from(1).expect("head"), Vec::<u8>::new());
    }

    #[test]
    fn a_fleet_of_logs_holds_exactly_the_sum_of_their_caps() {
        // The keeper charges one reservation per session, so the accounting is only honest if a
        // log's real footprint is its cap. 64 logs at 1 KiB must hold 64 KiB however they were
        // pushed, truncated or reset - never the 2x a doubling `Vec` can retain.
        let cap = 1024usize;
        let mut logs: Vec<ReplayLog> =
            (0..64).map(|n| ReplayLog::new(format!("s{n}"), cap)).collect();
        for log in &mut logs {
            for _ in 0..40 {
                log.push(&[b'x'; 97]);
            }
            let keep_from = log.head().saturating_sub(300);
            log.truncate_before(keep_from);
            log.push(&[b'y'; 200]);
        }
        let held: usize = logs.iter().map(ReplayLog::allocated_bytes).sum();
        assert_eq!(held, 64 * cap, "retained bytes may vary; the allocation may not");
        for log in &logs {
            assert!(log.retained_bytes() <= log.capacity());
            assert_eq!(log.allocated_bytes(), cap);
        }
    }

    #[test]
    fn a_run_that_exactly_fills_the_cap_is_retained() {
        // The overflow test is `>` and not `>=`: a push that lands exactly on the cap is kept, so
        // the retained suffix is the whole cap whenever the output happens to align with it.
        let mut log = ReplayLog::new("s1", 8);
        log.push(b"abcdefgh");
        assert_eq!(log.retained_bytes(), 8);
        assert_eq!(log.start_offset(), 0);
        assert!(log.pending_gap().is_none());
        assert_eq!(log.read_from(0).expect("covered"), b"abcdefgh");

        // Likewise a push that exactly fills the space left in the window.
        let mut log = ReplayLog::new("s1", 8);
        log.push(b"abc");
        log.push(b"defgh");
        assert_eq!(log.head(), 8);
        assert!(log.pending_gap().is_none(), "a full window is not an overflow");
        assert_eq!(log.read_from(0).expect("covered"), b"abcdefgh");

        // And the next byte is what overflows.
        log.push(b"i");
        assert_eq!(log.head(), 9);
        assert!(log.is_empty());
        assert_eq!(log.take_gap().expect("gap").resumed_at_offset, 9);
    }

    #[test]
    fn consecutive_overflows_coalesce_into_one_truthful_gap() {
        // Nothing forces a consumer to take the gap before the next overflow. The second reset must
        // still leave the log honest: the retained bound and the resume offset are the newest head,
        // and a read from anywhere older is refused with that same offset. The unconsumed first gap
        // is superseded, not kept as a stale alternative resume point.
        let mut log = ReplayLog::new("s1", 4);
        log.push(b"abcd");
        log.push(b"e"); // reset at head 5
        let first = log.pending_gap().expect("first gap");
        assert_eq!(first.resumed_at_offset, 5);

        log.push(b"fghij"); // 5 bytes into a 4-byte cap: a second reset at head 10
        let second = log.pending_gap().expect("second gap supersedes the first");
        assert_eq!(second.resumed_at_offset, 10);
        assert_eq!(second.resumed_at_offset, log.head());
        assert_eq!(second.requested_after_offset, 5, "the head before the overflowing push");
        assert_eq!(log.reset_count(), 2);
        assert!(log.is_empty());

        // Either gap's offset is at or below the head, so a reader that names one is told to resume
        // at the newest head rather than at a point whose bytes are no longer there.
        let stale = log.try_read_from(first.requested_after_offset).expect_err("lost");
        assert_eq!(stale.offset, Some(10));
        assert_eq!(log.try_read_from(0).expect_err("lost").offset, Some(10));

        log.push(b"kl");
        assert_eq!(log.read_from(10).expect("post-reset"), b"kl");
    }

    #[test]
    fn asking_past_the_head_is_a_gap_rather_than_a_clean_empty_history() {
        let mut log = ReplayLog::new("s1", 8);
        log.push(b"abc");
        assert_eq!(log.covered_len(4), None);
        let gap = log.read_from(4).expect_err("beyond the head");
        assert_eq!(gap.requested_after_offset, 4, "the caller's own offset, as asked");
        assert_eq!(gap.resumed_at_offset, 3, "the newest byte that exists");
        assert_eq!(gap.retained_from_offset, 0);
        assert!(log.covers(3), "the head itself is covered: nothing was lost");
        assert!(!log.covers(4));
    }

    #[test]
    fn a_gap_error_names_the_session_and_carries_the_resume_offset() {
        let error = ReplayGap::new(3, 40).as_error("s1");
        assert_eq!(error.code, KeeperErrorCode::ReplayGap);
        assert_eq!(error.offset, Some(40));
        assert!(error.message.contains("s1"));
    }
}
