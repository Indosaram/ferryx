use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Weak};
use thiserror::Error;
use tokio::sync::broadcast;

use super::scrollback_budget::{Eviction, ScrollbackBudget};
use super::PtyError;

const DEFAULT_BUFFER_CAPACITY: usize = 512 * 1024; // 512 KiB
// Sequence-channel lag/backlog bound: 64 chunks x 64 KiB = ~4 MiB worst-case backlog vs
// the 512 KiB ring = 8 chunks, still 8x headroom over ring coverage. A smaller bound only
// increases Lagged frequency, which ReplayGap recovery already handles by resyncing via
// snapshot_after.
const BROADCAST_CAPACITY: usize = 64;
// Legacy raw-byte channel keeps the pre-reduction capacity: its consumers have no sequence
// metadata and no ReplayGap resynchronization, so lagging would silently drop output.
const RAW_BROADCAST_CAPACITY: usize = 1024;
const RESIZE_LEDGER_CAPACITY: usize = 4096;
pub(crate) type ExitStatusSlot = Arc<RwLock<Option<i32>>>;

/// Output after which the screen holds nothing drawn earlier: a full reset (RIS), an erase of
/// the whole display (ED 2), or entering the alternate screen, which starts blank.
fn resets_whole_screen(bytes: &[u8]) -> bool {
    const RESETS: [&[u8]; 3] = [b"\x1bc", b"\x1b[2J", b"\x1b[?1049h"];
    RESETS
        .iter()
        .any(|reset| bytes.windows(reset.len()).any(|window| window == *reset))
}

/// Compact wire form for the opaque byte payloads a checkpoint carries.
///
/// A checkpoint blob is `serde_json` of [`SessionHubSnapshot`], which holds the retained ring:
/// 512 KiB of terminal output per session. Serde's default `Vec<u8>`/`Arc<[u8]>` representation is
/// a JSON **number array** (`[27,91,51,...]`), which writes every byte as one to three decimal
/// digits plus a comma -- around 3.6x the bytes on this repo's data, built one `serde_json` number
/// at a time. The keeper never parses a checkpoint (it stores the blob and hands it back
/// verbatim), so both ends of that cost are the daemon's: the producer serializes the whole ring
/// every checkpoint, and a recovery deserializes it. Every such transient is large and short
/// lived, which is the shape that makes a process's footprint oscillate while its steady-state
/// size is flat.
///
/// The wire form is therefore base64 text, matching every other opaque field on these wires
/// (`daemon::protocol::base64_serde`, `screen_mirror::ScreenSeed`). Measured on a 512 KiB ring:
/// 699 KB encoded (1.334x) against 1.87 MB for the number array (3.56x).
/// [`base64::display::Base64Display`] streams the encoding straight into the serializer, so a
/// checkpoint's cost is the output buffer itself, with no per-chunk copy on the way.
///
/// Decoding accepts **both** forms. The keeper holds a checkpoint across a daemon restart and
/// returns it verbatim, so a successor daemon of this generation can be handed a blob a
/// predecessor of the previous generation produced; refusing the number array would strand that
/// recovery point. The array arm is read-only legacy and nothing writes it any more.
pub(crate) mod compact_bytes {
    use base64::display::Base64Display;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    use serde::de::{self, SeqAccess, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;

    /// Streams the base64 text into the serializer: no intermediate `String` per field.
    fn write<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&Base64Display::new(bytes, &STANDARD))
    }

    /// base64 text, or the legacy JSON number array.
    fn read<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        struct BytesVisitor;

        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("base64 text or a JSON array of byte values")
            }

            /// Ahead of the array arm and reached from both borrowed and owned input, since serde's
            /// default `visit_borrowed_str` forwards here.
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Vec<u8>, E> {
                STANDARD.decode(value.as_bytes()).map_err(E::custom)
            }

            /// The pre-base64 wire form, still produced by a keeper holding an older generation's
            /// checkpoint. The size hint is capped so a malformed frame cannot make this reserve an
            /// arbitrary amount up front; a genuinely longer array still decodes, it just grows.
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
                let hint = seq.size_hint().unwrap_or(0).min(MAX_BYTES_HINT);
                let mut bytes = Vec::with_capacity(hint);
                while let Some(byte) = seq.next_element::<u8>()? {
                    bytes.push(byte);
                }
                Ok(bytes)
            }
        }

        deserializer.deserialize_any(BytesVisitor)
    }

    /// Only a pre-allocation hint; a larger array still decodes, it just grows on the way.
    const MAX_BYTES_HINT: usize = 1024 * 1024;

    /// For [`super::OutputChunk::bytes`], which is shared so the buffer, the sequence broadcast and
    /// the raw broadcast all point at one allocation.
    pub(crate) mod as_arc {
        use super::{read, write};
        use serde::{Deserializer, Serializer};
        use std::sync::Arc;

        pub(crate) fn serialize<S: Serializer>(
            bytes: &Arc<[u8]>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            write(bytes, serializer)
        }

        pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Arc<[u8]>, D::Error> {
            read(deserializer).map(Arc::from)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResizePoint {
    pub sequence: u64,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HistorySegment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cols: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u16>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRange {
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    pub start: usize,
    pub end: usize,
}

pub type SegmentedHistory = (
    Vec<u8>,
    Vec<HistorySegment>,
    Option<u64>,
    Option<u64>,
    Option<ReplayGap>,
);
pub(crate) type RangedHistory = (
    Vec<u8>,
    Vec<HistoryRange>,
    Option<u64>,
    Option<u64>,
    Option<ReplayGap>,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputChunk {
    pub sequence: u64,
    /// Shared, immutable chunk payload. Storing the bytes behind an `Arc` lets the buffer,
    /// the sequence broadcast, and the raw broadcast share a single allocation instead of
    /// deep-copying a 64 KiB `Vec<u8>` for every subscriber on each publish.
    ///
    /// Serialized as base64 text, not a JSON number array: this is the field that made a 512 KiB
    /// checkpoint ring encode to several times its size every minute (see `compact_bytes`).
    #[serde(with = "compact_bytes::as_arc")]
    pub bytes: Arc<[u8]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_read_unix_micros: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_gap: Option<ReplayGap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayGap {
    pub requested_after_sequence: u64,
    pub available_from_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentSnapshot {
    pub session_id: String,
    pub history_start_sequence: Option<u64>,
    pub history_end_sequence: Option<u64>,
    pub history: Vec<u8>,
    #[serde(default)]
    pub history_segments: Vec<HistorySegment>,
    pub gap: Option<ReplayGap>,
    /// The hub lost output to a replay gap and has seen no full-screen reset since, so even a
    /// full replay (`after_sequence = None`) carries only part of what the pane shows.
    #[serde(default)]
    pub history_incomplete: bool,
}

#[derive(Debug)]
pub struct SessionAttachment {
    pub snapshot: AttachmentSnapshot,
    pub receiver: broadcast::Receiver<OutputChunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OutputHubSnapshotError {
    #[error("next_sequence must be greater than 0, got {0}")]
    InvalidNextSequence(u64),

    #[error("chunk sequence must be greater than 0, got {0}")]
    InvalidChunkSequence(u64),

    #[error("chunk sequence {0} exceeds or equals next_sequence {1}")]
    ChunkSequenceExceedsNextSequence(u64, u64),

    #[error("chunk sequences must be strictly increasing: {0} <= {1}")]
    ChunkSequenceNotMonotonic(u64, u64),

    #[error("resize point sequence {0} exceeds or equals next_sequence {1}")]
    ResizeSequenceExceedsNextSequence(u64, u64),

    #[error("resize ledger must be ordered: sequence {0} < previous sequence {1}")]
    ResizeLedgerNotOrdered(u64, u64),

    #[error("retained chunks byte size ({actual} bytes) exceeds capacity ({capacity} bytes)")]
    RetainedBytesExceedCapacity { actual: usize, capacity: usize },

    #[error("invalid replay gap range: requested_after_sequence {requested} >= available_from_sequence {available}")]
    InvalidReplayGapRange { requested: u64, available: u64 },

    #[error("replay gap available_from_sequence {0} exceeds next_sequence")]
    ReplayGapExceedsNextSequence(u64),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionHubSnapshot {
    #[serde(default)]
    pub capacity: usize,
    pub chunks: Vec<OutputChunk>,
    pub next_sequence: u64,
    pub bracketed_paste_enabled: bool,
    pub resize_ledger: Vec<ResizePoint>,
    /// Carried across handover so a successor keeps naming the geometry the session was
    /// created with. `resize_ledger` cannot stand in: it evicts its oldest point at capacity,
    /// after which its first entry is an ordinary phone resize.
    #[serde(default)]
    pub initial_size: Option<(u16, u16)>,
    pub replay_gap: Option<ReplayGap>,
    #[serde(default)]
    pub gap_floor: u64,

    #[serde(default)]
    pub transport_owner: bool,
    /// Eviction-accumulated VT state. The full-snapshot reconstruction replays `retained`,
    /// which derives its prelude from bytes that may already be gone from `chunks`; without
    /// carrying it here an import loses that state (e.g. a bracketed-paste mode set before
    /// its chunk was evicted from the ring).
    #[serde(default)]
    pub retained_state: Option<vt_state::TerminalStatePrelude>,
    /// The predecessor's mirrored screen, so a successor answers full attaches with the same
    /// screen instead of re-deriving it from a ring that starts mid-stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_seed: Option<super::screen_mirror::ScreenSeed>,
    /// [SessionHub::history_incomplete], carried so a successor does not mistake post-gap
    /// history for the whole screen.
    #[serde(default)]
    pub history_incomplete: bool,
}

impl SessionHubSnapshot {
    pub fn validate(&self) -> Result<(), OutputHubSnapshotError> {
        self.validate_with_capacity(0)
    }

    pub fn validate_with_capacity(
        &self,
        fallback_capacity: usize,
    ) -> Result<(), OutputHubSnapshotError> {
        if self.next_sequence == 0 {
            return Err(OutputHubSnapshotError::InvalidNextSequence(0));
        }
        if self.gap_floor > self.next_sequence {
            return Err(OutputHubSnapshotError::ReplayGapExceedsNextSequence(self.gap_floor));
        }

        let mut prev_chunk_seq = 0u64;
        let mut total_bytes = 0usize;
        for chunk in &self.chunks {
            if chunk.sequence == 0 {
                return Err(OutputHubSnapshotError::InvalidChunkSequence(0));
            }
            if chunk.sequence >= self.next_sequence {
                return Err(OutputHubSnapshotError::ChunkSequenceExceedsNextSequence(
                    chunk.sequence,
                    self.next_sequence,
                ));
            }
            if chunk.sequence <= prev_chunk_seq {
                return Err(OutputHubSnapshotError::ChunkSequenceNotMonotonic(
                    chunk.sequence,
                    prev_chunk_seq,
                ));
            }
            prev_chunk_seq = chunk.sequence;
            total_bytes = total_bytes.saturating_add(chunk.bytes.len());

            if let Some(gap) = &chunk.replay_gap {
                if gap.requested_after_sequence >= gap.available_from_sequence {
                    return Err(OutputHubSnapshotError::InvalidReplayGapRange {
                        requested: gap.requested_after_sequence,
                        available: gap.available_from_sequence,
                    });
                }
                if gap.available_from_sequence > self.next_sequence {
                    return Err(OutputHubSnapshotError::ReplayGapExceedsNextSequence(
                        gap.available_from_sequence,
                    ));
                }
            }
        }

        let mut prev_resize_seq = 0u64;
        for (i, point) in self.resize_ledger.iter().enumerate() {
            if point.sequence >= self.next_sequence {
                return Err(OutputHubSnapshotError::ResizeSequenceExceedsNextSequence(
                    point.sequence,
                    self.next_sequence,
                ));
            }
            if i > 0 && point.sequence < prev_resize_seq {
                return Err(OutputHubSnapshotError::ResizeLedgerNotOrdered(
                    point.sequence,
                    prev_resize_seq,
                ));
            }
            prev_resize_seq = point.sequence;
        }

        let max_capacity = if self.capacity > 0 {
            self.capacity
        } else {
            fallback_capacity
        };
        if max_capacity > 0 && total_bytes > max_capacity {
            return Err(OutputHubSnapshotError::RetainedBytesExceedCapacity {
                actual: total_bytes,
                capacity: max_capacity,
            });
        }

        if let Some(gap) = &self.replay_gap {
            if gap.requested_after_sequence >= gap.available_from_sequence {
                return Err(OutputHubSnapshotError::InvalidReplayGapRange {
                    requested: gap.requested_after_sequence,
                    available: gap.available_from_sequence,
                });
            }
            if gap.available_from_sequence > self.next_sequence {
                return Err(OutputHubSnapshotError::ReplayGapExceedsNextSequence(
                    gap.available_from_sequence,
                ));
            }
        }

        Ok(())
    }
}

pub struct BoundedBuffer {
    capacity: usize,
    chunks: VecDeque<OutputChunk>,
    current_size: usize,
    next_sequence: u64,
    bracketed_paste_enabled: bool,
    retained: retained_history::RetainedHistory,
    gap_floor: u64,
}

pub fn scan_dec_mode_2004(bytes: &[u8]) -> Option<bool> {
    if !bytes.contains(&b'2') {
        return None;
    }
    let mut last_state = None;
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i] == 0x1b && bytes[i + 1] == b'[' && bytes[i + 2] == b'?' {
            let mut j = i + 3;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b'h' || bytes[j] == b'l') {
                let params = &bytes[i + 3..j];
                let is_set = bytes[j] == b'h';
                for part in params.split(|&b| b == b';') {
                    if part == b"2004" {
                        last_state = Some(is_set);
                    }
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    last_state
}

impl BoundedBuffer {
    pub fn new(capacity: usize) -> Self {
        Self::with_retention(
            capacity,
            retained_history::RetentionBudget::from_text_capacity(capacity),
        )
    }

    pub fn with_retention(
        capacity: usize,
        budget: retained_history::RetentionBudget,
    ) -> Self {
        Self {
            capacity,
            chunks: VecDeque::new(),
            current_size: 0,
            next_sequence: 1,
            bracketed_paste_enabled: false,
            retained: retained_history::RetainedHistory::new(budget),
            gap_floor: 0,
        }
    }

    pub fn allocate_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        sequence
    }

    pub fn push(&mut self, chunk_bytes: Vec<u8>) -> Option<OutputChunk> {
        self.push_with_read_timestamp(chunk_bytes, None)
    }

    pub fn push_with_read_timestamp(
        &mut self,
        chunk_bytes: Vec<u8>,
        metrics_read_unix_micros: Option<u64>,
    ) -> Option<OutputChunk> {
        if chunk_bytes.is_empty() {
            return None;
        }

        let sequence = self.allocate_sequence();

        // Convert the owned payload into a shared allocation exactly once. Every later clone
        // (buffer retention, sequence broadcast, raw broadcast) is a cheap refcount bump.
        let chunk = OutputChunk {
            sequence,
            bytes: Arc::from(chunk_bytes),
            metrics_read_unix_micros,
            replay_gap: None,
        };

        self.current_size += chunk.bytes.len();
        if let Some(enabled) = scan_dec_mode_2004(&chunk.bytes) {
            self.bracketed_paste_enabled = enabled;
        }
        self.retained.push(sequence, &chunk.bytes);
        self.chunks.push_back(chunk.clone());

        while self.current_size > self.capacity && !self.chunks.is_empty() {
            if let Some(front) = self.chunks.pop_front() {
                self.current_size -= front.bytes.len();
            }
        }

        Some(chunk)
    }

    pub fn start_sequence(&self) -> Option<u64> {
        self.chunks.front().map(|c| c.sequence)
    }

    /// Oldest sequence a *full* replay can reconstruct. Retention outlives the recovery ring,
    /// so this is at or before [`Self::start_sequence`].
    pub fn retained_start_sequence(&self) -> Option<u64> {
        self.retained.start_sequence().or_else(|| self.start_sequence())
    }

    pub fn retained_text_bytes(&self) -> usize {
        self.retained.text_bytes()
    }

    pub fn retained_image_bytes(&self) -> usize {
        self.retained.image_bytes()
    }

    pub fn end_sequence(&self) -> Option<u64> {
        self.chunks.back().map(|c| c.sequence)
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn current_size(&self) -> usize {
        self.current_size
    }

    pub fn bracketed_paste_enabled(&self) -> bool {
        self.bracketed_paste_enabled
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn chunks(&self) -> &VecDeque<OutputChunk> {
        &self.chunks
    }

    pub fn from_snapshot(
        capacity: usize,
        chunks: Vec<OutputChunk>,
        next_sequence: u64,
        bracketed_paste_enabled: bool,
        retained_state: Option<vt_state::TerminalStatePrelude>,
    ) -> Self {
        let current_size = chunks.iter().map(|c| c.bytes.len()).sum();
        let chunks = VecDeque::from(chunks);
        // Rebuild retained VT units from the restored ring exactly as the live push path does,
        // so a full-snapshot reconstruction (which replays retained, not the ring) sees history.
        let mut retained = retained_history::RetainedHistory::new(
            retained_history::RetentionBudget::from_text_capacity(capacity),
        );
        for chunk in &chunks {
            retained.push(chunk.sequence, &chunk.bytes);
        }
        // Restore last: the exported prelude is authoritative for the predecessor's evicted
        // state, which the surviving ring alone cannot reproduce.
        if let Some(state) = retained_state {
            retained.restore_state(state);
        }
        Self {
            capacity,
            chunks,
            current_size,
            next_sequence,
            bracketed_paste_enabled,
            retained,
            gap_floor: 0,
        }
    }

    fn buffer_contains_active_bracketed_paste(&self) -> bool {
        let mut in_buffer = false;
        for chunk in &self.chunks {
            if let Some(enabled) = scan_dec_mode_2004(&chunk.bytes) {
                in_buffer = enabled;
            }
        }
        in_buffer
    }

    pub fn snapshot(&self) -> Vec<u8> {
        let needs_prefix =
            self.bracketed_paste_enabled && !self.buffer_contains_active_bracketed_paste();
        let mut out = Vec::with_capacity(self.current_size + if needs_prefix { 8 } else { 0 });
        if needs_prefix {
            out.extend_from_slice(b"\x1b[?2004h");
        }
        for chunk in &self.chunks {
            out.extend_from_slice(&chunk.bytes);
        }
        out
    }

    pub fn snapshot_after(
        &self,
        after_sequence: Option<u64>,
    ) -> (Vec<u8>, Option<u64>, Option<u64>, Option<ReplayGap>) {
        let (history, _, start, end, gap) = self.snapshot_after_ranges_impl(after_sequence, &[], false);
        (history, start, end, gap)
    }

    pub fn snapshot_after_segmented(
        &self,
        after_sequence: Option<u64>,
        ledger: &[ResizePoint],
    ) -> SegmentedHistory {
        let (history, ranges, start, end, gap) = if after_sequence.is_none() {
            self.snapshot_after_ranges_retained(after_sequence, ledger)
        } else {
            self.snapshot_after_ranges(after_sequence, ledger)
        };
        let segments = ranges.into_iter().map(|r| HistorySegment {
            cols: r.cols,
            rows: r.rows,
            bytes: history[r.start..r.end].to_vec(),
        }).collect();
        (history, segments, start, end, gap)
    }

    pub(crate) fn snapshot_after_ranges(
        &self,
        after_sequence: Option<u64>,
        ledger: &[ResizePoint],
    ) -> RangedHistory {
        self.snapshot_after_ranges_impl(after_sequence, ledger, true)
    }

    pub(crate) fn snapshot_after_ranges_retained(
        &self,
        after_sequence: Option<u64>,
        ledger: &[ResizePoint],
    ) -> RangedHistory {
        self.snapshot_after_ranges_impl(after_sequence, ledger, true)
    }

    fn snapshot_after_ranges_impl(
        &self,
        after_sequence: Option<u64>,
        ledger: &[ResizePoint],
        from_retained: bool,
    ) -> RangedHistory {
        let first = if from_retained {
            self.retained_start_sequence().map(|sequence| sequence.max(self.gap_floor))
        } else {
            self.start_sequence()
        };
        let last = self.end_sequence();
        let gap = after_sequence.and_then(|requested| {
            if self.gap_floor > 0 && requested < self.gap_floor {
                return Some(ReplayGap {
                    requested_after_sequence: requested,
                    available_from_sequence: self.gap_floor,
                });
            }
            if let Some(first) = first {
                if requested < first.saturating_sub(1) || requested > last.unwrap_or(0) {
                    Some(ReplayGap {
                        requested_after_sequence: requested,
                        available_from_sequence: first,
                    })
                } else {
                    None
                }
            } else if self.next_sequence > 1 || requested > 0 {
                Some(ReplayGap {
                    requested_after_sequence: requested,
                    available_from_sequence: self.next_sequence,
                })
            } else {
                None
            }
        });
        let full = after_sequence.is_none() || gap.is_some();
        // Full reconstruction uses retained VT units; contiguous deltas and the
        // separately budgeted machine transport continue to use exact ring bytes.
        let retained_pending;
        let (items, prefix): (Vec<(u64, &[u8])>, Vec<u8>) = if from_retained && full {
            retained_pending = self.retained.pending();
            let items = self.retained.records_with_pending(&retained_pending);
            (items, self.retained.state_prelude())
        } else {
            (
                self.chunks
                    .iter()
                    .filter(|c| full || Some(c.sequence) > after_sequence)
                    .map(|c| (c.sequence, &*c.bytes))
                    .collect(),
                if full
                    && self.bracketed_paste_enabled
                    && !self.buffer_contains_active_bracketed_paste()
                {
                    b"\x1b[?2004h".to_vec()
                } else {
                    Vec::new()
                },
            )
        };

        if items.is_empty() && prefix.is_empty() {
            let ranges = ledger
                .last()
                .filter(|p| after_sequence.is_none_or(|s| p.sequence > s))
                .map(|p| {
                    vec![HistoryRange {
                        cols: Some(p.cols),
                        rows: Some(p.rows),
                        start: 0,
                        end: 0,
                    }]
                })
                .unwrap_or_default();
            return (Vec::new(), ranges, None, last, gap);
        }

        let first_sequence = items.first().map(|(sequence, _)| *sequence).or(last);
        let total: usize = items.iter().map(|(_, bytes)| bytes.len()).sum();
        let mut history = Vec::with_capacity(prefix.len() + total);
        history.extend_from_slice(&prefix);

        let mut li = 0;
        let mut size = (None, None);
        if let Some(start) = first_sequence {
            while li < ledger.len() && ledger[li].sequence <= start {
                size = (Some(ledger[li].cols), Some(ledger[li].rows));
                li += 1;
            }
        }
        let mut ranges = Vec::new();
        let mut start = 0;
        for (index, (sequence, bytes)) in items.iter().enumerate() {
            if index > 0 {
                let mut next_size = size;
                while li < ledger.len() && ledger[li].sequence <= *sequence {
                    next_size = (Some(ledger[li].cols), Some(ledger[li].rows));
                    li += 1;
                }
                if next_size != size {
                    ranges.push(HistoryRange {
                        cols: size.0,
                        rows: size.1,
                        start,
                        end: history.len(),
                    });
                    start = history.len();
                    size = next_size;
                }
            }
            history.extend_from_slice(bytes);
        }
        ranges.push(HistoryRange {
            cols: size.0,
            rows: size.1,
            start,
            end: history.len(),
        });
        if let Some(point) = ledger.get(li..).and_then(|suffix| suffix.last()) {
            if (Some(point.cols), Some(point.rows)) != size {
                ranges.push(HistoryRange {
                    cols: Some(point.cols),
                    rows: Some(point.rows),
                    start: history.len(),
                    end: history.len(),
                });
            }
        }
        (history, ranges, first_sequence, last, gap)
    }
}

pub fn segment_history(
    chunks: &[&OutputChunk],
    ledger: &[ResizePoint],
    after_sequence: Option<u64>,
) -> Vec<HistorySegment> {
    // Both `chunks` and `ledger` are sequence-ordered, so a single advancing cursor over the
    // ledger yields the segmentation in O(chunks + ledger) with no per-chunk re-scan.
    if chunks.is_empty() {
        // The trailing size is just the last ledger point, provided it sits past the cursor
        // (the ledger suffix with `sequence > req` is the only part that matters).
        let last_point = match after_sequence {
            Some(req_seq) => ledger.last().filter(|p| p.sequence > req_seq),
            None => ledger.last(),
        };
        if let Some(last_point) = last_point {
            return vec![HistorySegment {
                cols: Some(last_point.cols),
                rows: Some(last_point.rows),
                bytes: Vec::new(),
            }];
        }
        return Vec::new();
    }

    let total_bytes: usize = chunks.iter().map(|c| c.bytes.len()).sum();
    let mut remaining = total_bytes;

    let first_chunk = chunks[0];

    // Advance to the last ledger point at or before the first chunk to seed the current size.
    let mut li = 0usize;
    let mut current_size: (Option<u16>, Option<u16>) = (None, None);
    while li < ledger.len() && ledger[li].sequence <= first_chunk.sequence {
        current_size = (Some(ledger[li].cols), Some(ledger[li].rows));
        li += 1;
    }

    let mut segments: Vec<HistorySegment> = Vec::with_capacity(ledger.len().saturating_sub(li) + 1);
    let mut current_bytes: Vec<u8> = Vec::with_capacity(total_bytes);
    current_bytes.extend_from_slice(&first_chunk.bytes);
    remaining -= first_chunk.bytes.len();

    for chunk in &chunks[1..] {
        // Consume every ledger point in (prev_chunk_seq, chunk.sequence]; only the newest one
        // decides this chunk's size. `li` never rewinds, keeping the walk linear.
        let mut newest_size: Option<(Option<u16>, Option<u16>)> = None;
        while li < ledger.len() && ledger[li].sequence <= chunk.sequence {
            newest_size = Some((Some(ledger[li].cols), Some(ledger[li].rows)));
            li += 1;
        }

        if let Some(new_size) = newest_size {
            if new_size != current_size {
                segments.push(HistorySegment {
                    cols: current_size.0,
                    rows: current_size.1,
                    bytes: current_bytes,
                });
                current_bytes = Vec::with_capacity(remaining);
                current_size = new_size;
            }
        }
        current_bytes.extend_from_slice(&chunk.bytes);
        remaining -= chunk.bytes.len();
    }

    segments.push(HistorySegment {
        cols: current_size.0,
        rows: current_size.1,
        bytes: current_bytes,
    });

    // Ledger points remaining past the final chunk form a trailing (empty) segment when they
    // change the size. `li` already sits at the first such point.
    let mut trailing_size: Option<(Option<u16>, Option<u16>)> = None;
    while li < ledger.len() {
        trailing_size = Some((Some(ledger[li].cols), Some(ledger[li].rows)));
        li += 1;
    }
    if let Some(trailing_size) = trailing_size {
        if trailing_size != current_size {
            segments.push(HistorySegment {
                cols: trailing_size.0,
                rows: trailing_size.1,
                bytes: Vec::new(),
            });
        }
    }

    segments
}

#[path = "machine_output.rs"]
pub mod machine_output;

#[path = "retained_history.rs"]
pub mod retained_history;

#[path = "vt_state.rs"]
pub mod vt_state;

#[path = "vt_stream.rs"]
pub mod vt_stream;

pub struct SessionHub {
    exit_status: ExitStatusSlot,
    machine_senders: Vec<machine_output::MachineSender>,
    pub(crate) buffer: BoundedBuffer,
    pub(crate) sender: broadcast::Sender<OutputChunk>,
    pub(crate) raw_sender: broadcast::Sender<Vec<u8>>,
    pub(crate) resize_ledger: Vec<ResizePoint>,
    /// The first size this session was ever seen at. Unlike `resize_ledger`, which evicts its
    /// oldest boundary at capacity, this survives for the session's lifetime, so desktop
    /// restoration still has a baseline after thousands of phone resizes.
    pub(crate) initial_size: Option<(u16, u16)>,
    pub(crate) replay_gap: Option<ReplayGap>,
    /// Set by `publish_gap`; cleared only by output that redraws the whole screen. Unlike
    /// `replay_gap`, ordinary output after the gap does not clear it.
    pub(crate) history_incomplete: bool,
    /// Someone subscribed to this session at least once.
    pub(crate) subscribed: bool,
    /// The PTY output ended before the first subscription: the session is kept only so that
    /// late first attach still sees the output (see `end_session`).
    pub(crate) ended: bool,
    pub(crate) published_bytes: u64,
    /// Headless parser fed with every published chunk. `None` only when the parser could not be
    /// created, in which case full attaches fall back to retained replay.
    pub(crate) screen: Option<parking_lot::Mutex<super::screen_mirror::ScreenMirror>>,
}

impl SessionHub {
    pub fn export_state(&self) -> SessionHubSnapshot {
        SessionHubSnapshot {
            capacity: self.buffer.capacity,
            chunks: self.buffer.chunks.iter().cloned().collect(),
            next_sequence: self.buffer.next_sequence,
            bracketed_paste_enabled: self.buffer.bracketed_paste_enabled,
            resize_ledger: self.resize_ledger.clone(),
            initial_size: self.initial_size,
            replay_gap: self.replay_gap.clone(),
            gap_floor: self.buffer.gap_floor,
            transport_owner: false,
            retained_state: Some(self.buffer.retained.state()),
            screen_seed: self
                .screen_seed()
                .filter(|seed| seed.bytes.len() <= super::screen_mirror::MAX_HANDOVER_SEED_BYTES),
            history_incomplete: self.history_incomplete(),
        }
    }

    /// Whether a full replay still carries output lost to a gap. It stops doing so once the
    /// screen was redrawn, or once retention evicted the gap seam: the lost output then lies
    /// before the replay's start like any other evicted output. Without the second case a pane
    /// that never clears the whole screen (an inline TUI such as an agent's) stayed unreadable
    /// for good after one reconnect gap.
    pub(crate) fn history_incomplete(&self) -> bool {
        self.history_incomplete
            && self
                .buffer
                .retained_start_sequence()
                .is_none_or(|start| start < self.buffer.gap_floor)
    }

    /// The mirrored screen, when it can stand in for retained replay.
    ///
    /// Two cases keep retained replay: an alternate screen (the formatter serializes only the
    /// active screen, so the primary screen's history would be lost) and retained inline images
    /// (the formatter carries no image data).
    ///
    /// Taking the seed re-arms the mirror's idle compression and wakes its driver.
    fn screen_seed(&self) -> Option<super::screen_mirror::ScreenSeed> {
        if self.buffer.retained_image_bytes() > 0 {
            return None;
        }
        let mut screen = self.screen.as_ref()?.lock();
        if screen.is_alternate_screen() {
            return None;
        }
        screen.seed()
    }

    pub fn buffer(&self) -> &BoundedBuffer {
        &self.buffer
    }

    pub fn resize_ledger(&self) -> &[ResizePoint] {
        &self.resize_ledger
    }

    pub fn replay_gap(&self) -> Option<&ReplayGap> {
        self.replay_gap.as_ref()
    }
}

/// Starts idle scrollback compression for `hub`'s screen mirror on the current tokio runtime.
///
/// One task per hub, holding only a weak reference: it sleeps until the mirror's scheduler is
/// due, or waits for the mirror's activity signal when nothing is scheduled, and exits once the
/// hub is gone or compression is unsupported here. Without a runtime (synchronous callers) the
/// mirror is simply not compressed.
///
/// The current runtime must have its time driver enabled: tokio offers no non-panicking probe
/// for it, and the driver's sleep would panic in the spawned task on an IO-only runtime. Every
/// caller registers sessions from the daemon's `enable_all` runtime or the Tauri runtime.
fn spawn_screen_compression(hub: &Arc<RwLock<SessionHub>>) -> Option<tokio::task::JoinHandle<()>> {
    let runtime = tokio::runtime::Handle::try_current().ok()?;
    let activity = hub.read().screen.as_ref()?.lock().activity_signal();
    Some(runtime.spawn(drive_screen_compression(Arc::downgrade(hub), activity)))
}

async fn drive_screen_compression(hub: Weak<RwLock<SessionHub>>, activity: Arc<tokio::sync::Notify>) {
    loop {
        let enabled = super::cached_terminal_preferences().scrollback_compression;
        // The wall clock the mirror's activity marks also use. A sleep may end slightly before
        // the step is due; the scheduler then reports the remainder. Every guard is released
        // before the next await.
        let now = std::time::Instant::now();
        let wakeup = {
            let Some(hub) = hub.upgrade() else { return };
            let hub = hub.read();
            let Some(screen) = hub.screen.as_ref() else { return };
            let mut screen = screen.lock();
            if let Err(error) = screen.run_due_compression(now, enabled) {
                tracing::warn!(%error, "screen mirror scrollback compression step failed");
            }
            if !screen.compression_supported() {
                // Nothing will ever be scheduled again; waking on every feed would only
                // contend with publishers for the hub and screen locks.
                return;
            }
            screen.compression_wakeup(now)
        };
        match wakeup {
            Some(wait) => tokio::time::sleep(wait).await,
            // Output, a resize, or the mirror's drop wakes this.
            None => activity.notified().await,
        }
    }
}

#[derive(Clone)]
pub struct TerminalOutputHub {
    sessions: Arc<RwLock<HashMap<String, Arc<RwLock<SessionHub>>>>>,
    transport_owners: Arc<RwLock<std::collections::HashSet<String>>>,
    exits: Arc<parking_lot::Mutex<HashMap<String, ExitStatusSlot>>>,
    capacity: usize,
    /// The retention this hub configured for one session. Every session keeps this exact split
    /// unless the daemon-wide budget below is tighter, so a hub without a budget behaves
    /// byte-for-byte as it did before Phase 1.
    retention: retained_history::RetentionBudget,
    /// The daemon-wide retained-scrollback budget, when this hub enforces one.
    ///
    /// `None` is not an unbounded path: such a hub still bounds every session at `retention`, it
    /// simply does not divide one aggregate budget across sessions. Only the daemon opts in
    /// ([`Self::for_daemon`]); tests, replay-only hubs and paired proxies keep their configured
    /// retention so introducing the budget cannot change their footprint.
    budget: Arc<parking_lot::Mutex<Option<ScrollbackBudget>>>,
    /// Receives `scrollback_budget_exhausted` events; nothing is emitted while no one listens.
    budget_events: broadcast::Sender<Eviction>,
    /// Receives program status summary changes; installed by the daemon at startup.
    program_status_publisher:
        Arc<RwLock<Option<Arc<dyn super::program_status_ingest::ProgramStatusPublisher>>>>,
    /// The durable admission charge. Absent leaves the ceiling as the only bound, as before.
    admission: Arc<parking_lot::Mutex<Option<Arc<super::admission_store::AdmissionLedger>>>>,
    /// Why admission is refused outright, when it is. Set when the durable log could not be opened
    /// or reconciled: the true reservation count is then unknown, so admitting would mean guessing
    /// it, and a guess is what the hard limit exists to prevent.
    admission_refusal: Arc<parking_lot::Mutex<Option<super::admission_store::StoreError>>>,
}

impl Default for TerminalOutputHub {
    fn default() -> Self {
        Self::new(DEFAULT_BUFFER_CAPACITY)
    }
}

impl TerminalOutputHub {
    pub fn new(capacity: usize) -> Self {
        Self::new_with_retention(
            capacity,
            retained_history::RetentionBudget::from_text_capacity(capacity),
        )
    }

    /// Recovery ring capacity and reconstruction retention are independent: the ring bounds
    /// incremental replay and transport accounting, retention bounds full replay.
    pub fn new_with_retention(
        capacity: usize,
        retention: retained_history::RetentionBudget,
    ) -> Self {
        Self::with_budget(capacity, retention, None)
    }

    /// The daemon's production hub: every session shares one configured `SCROLLBACK_BUDGET_BYTES`
    /// of retained scrollback, so the per-session ceiling is `SCROLLBACK_BUDGET_BYTES / live
    /// sessions` and the session beyond `budget / MIN_SESSION_HISTORY_BYTES` is refused with a
    /// typed `SESSION_CAPACITY_EXHAUSTED`. This is the one hub the daemon builds for all its PTYs.
    ///
    /// The pair is read through [`super::preferences::configured_scrollback_budget`], so a
    /// reduced-budget deployment gets a proportionally lower admission limit instead of the
    /// compiled-in constant. An unusable pair falls back to the defaults there, so a typo cannot
    /// produce an undefined state.
    pub fn for_daemon() -> Self {
        let (budget_bytes, min_session_bytes) = super::preferences::configured_scrollback_budget();
        Self::for_daemon_with_budget(budget_bytes, min_session_bytes)
    }

    /// [`Self::for_daemon`] with the pair supplied, for the tests and for a caller that already
    /// validated the configuration. The pair is validated again here: a hub built with a budget at
    /// or below libghostty's default would silently restore the pruning `lifecycle.rs` removed.
    pub fn for_daemon_with_budget(budget_bytes: usize, min_session_bytes: usize) -> Self {
        let (budget_bytes, min_session_bytes) =
            super::preferences::validated_scrollback_budget(budget_bytes, min_session_bytes);
        Self::with_budget(
            DEFAULT_BUFFER_CAPACITY,
            retained_history::RetentionBudget::from_total(budget_bytes),
            Some(ScrollbackBudget::new_with_floor(budget_bytes, min_session_bytes)),
        )
    }

    /// A hub that enforces `budget` across its sessions, with `retention` as the per-session split
    /// and the prior bound. Used by the budget-pressure tests and by any fork that configures its
    /// own budget.
    pub fn new_with_budget(
        capacity: usize,
        retention: retained_history::RetentionBudget,
        budget: ScrollbackBudget,
    ) -> Self {
        Self::with_budget(capacity, retention, Some(budget))
    }

    fn with_budget(
        capacity: usize,
        retention: retained_history::RetentionBudget,
        budget: Option<ScrollbackBudget>,
    ) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            transport_owners: Arc::new(RwLock::new(std::collections::HashSet::new())),
            exits: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            capacity,
            retention,
            budget: Arc::new(parking_lot::Mutex::new(budget)),
            budget_events: broadcast::channel(64).0,
            program_status_publisher: Arc::new(RwLock::new(None)),
            admission: Arc::new(parking_lot::Mutex::new(None)),
            admission_refusal: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    /// The retention budget that gives a session `ceiling` bytes of retained history.
    ///
    /// When the ceiling covers what this hub already configured, the configured split is kept
    /// verbatim: the budget is a bound, not a re-shaping, so an existing hub's retention (and the
    /// tests that pin it) is untouched. Only a tighter ceiling re-splits text and images.
    fn retention_for(&self, ceiling: usize) -> retained_history::RetentionBudget {
        if ceiling >= self.configured_retention_bytes() {
            self.retention
        } else {
            retained_history::RetentionBudget::from_total(ceiling)
        }
    }

    fn configured_retention_bytes(&self) -> usize {
        self.retention
            .text_bytes
            .saturating_add(self.retention.image_bytes)
    }

    /// True when the budget, not the pre-existing configuration, is the binding constraint. Used
    /// to leave the screen mirror on its line cap whenever the budget does not bind.
    fn budget_binds(&self, ceiling: usize) -> bool {
        ceiling < self.configured_retention_bytes()
    }

    /// Runs `f` against the ledger if this hub enforces one.
    fn with_ledger<R>(&self, f: impl FnOnce(&mut ScrollbackBudget) -> R) -> Option<R> {
        self.budget.lock().as_mut().map(f)
    }

    /// Admits `session_id` to the daemon-wide scrollback budget or refuses it with the typed
    /// `SESSION_CAPACITY_EXHAUSTED` error naming the limit. Idempotent for a live session.
    /// Admission shares the budget with one more session, so the trims that follow are applied
    /// and announced here rather than on the next write.
    pub fn admit_session(&self, session_id: &str) -> Result<(), PtyError> {
        match self.with_ledger(|budget| budget.admit(session_id)) {
            Some(result) => result?,
            None => return Ok(()),
        }
        self.reapply_ceilings();
        self.enforce_budget();
        Ok(())
    }

    /// Installs the durable charge this hub consults before admitting a new session.
    ///
    /// Installing a usable ledger also clears the refusal: the startup pass that retries after a
    /// repaired log must not keep refusing creates because of the earlier failure.
    pub fn install_admission(&self, ledger: Arc<super::admission_store::AdmissionLedger>) {
        *self.admission.lock() = Some(ledger);
        *self.admission_refusal.lock() = None;
    }

    /// Refuses every further admission because the durable log cannot be trusted.
    ///
    /// This is the fail-closed arm of startup: a corrupt or unwritable log hides an unknown number
    /// of reservations, so no create is admitted at all. Running sessions keep being served; only
    /// new ones are refused, and they are refused with the typed store error rather than a
    /// transient-capacity one, because retrying will not help.
    pub fn refuse_admission(&self, error: super::admission_store::StoreError) {
        tracing::error!(%error, "admission is refused: the durable reservation log is unusable");
        *self.admission.lock() = None;
        *self.admission_refusal.lock() = Some(error);
    }

    /// Why admission is currently refused, if it is.
    pub fn admission_refusal(&self) -> Option<super::admission_store::StoreError> {
        self.admission_refusal.lock().clone()
    }

    /// Opens the durable reservation log at `path` and installs it, or refuses every further
    /// admission when it cannot be read.
    ///
    /// This is the startup gate: it runs before any spawn can be accepted, so a reservation left
    /// by the daemon that died stays charged. A corrupt log returns the typed store error and
    /// leaves the hub refusing creates; it is never replaced by an empty ledger, because an empty
    /// ledger would claim the count is zero.
    ///
    /// Blocking: performs synchronous disk I/O. Callers on an async runtime must run it through
    /// `run_blocking` / `spawn_blocking` or a dedicated thread.
    pub fn open_durable_admission(
        &self,
        path: &std::path::Path,
    ) -> Result<Arc<super::admission_store::AdmissionLedger>, super::admission_store::StoreError>
    {
        match super::admission_store::open_admission(path) {
            Ok(ledger) => {
                self.install_admission(Arc::clone(&ledger));
                Ok(ledger)
            }
            Err(error) => {
                self.refuse_admission(error.clone());
                Err(error)
            }
        }
    }

    pub fn admission(&self) -> Option<Arc<super::admission_store::AdmissionLedger>> {
        self.admission.lock().clone()
    }

    /// The ceiling a durable reservation is charged against: exactly the budget this hub admits
    /// against, so the disk record and the in-memory bound can never name different limits.
    pub fn admission_ceiling(&self) -> Option<super::admission_store::AdmissionCeiling> {
        self.with_ledger(|budget| super::admission_store::AdmissionCeiling::from_budget(budget))
    }

    /// The configured budget bytes, for the startup log. `None` on a hub that enforces no budget.
    pub fn budget_bytes(&self) -> Option<usize> {
        self.with_ledger(|budget| budget.budget_bytes())
    }

    /// The configured per-session floor, for the startup log. `None` on a hub with no budget.
    pub fn min_session_bytes(&self) -> Option<usize> {
        self.with_ledger(|budget| budget.min_session_bytes())
    }

    /// Charges a durable slot and then the in-memory budget, in that order: the disk write is what
    /// survives a crash, so it happens before anything else can observe the session.
    ///
    /// A store that cannot be written refuses admission outright. Continuing would mean admitting
    /// against an unknown reservation count, and a crash there could leave a child this daemon has
    /// no record of.
    pub fn admit_durably(
        &self,
        session_id: &str,
    ) -> Result<super::admission_store::ReservationOutcome, super::admission_store::AdmissionRefusal>
    {
        self.admit_durably_with_request(session_id, None)
    }

    /// [`Self::admit_durably`] recording `request_id` inside the reservation, so the keeper-client
    /// retry path can present the same logical spawn id after a crash instead of minting a new one.
    /// The id is durable before this returns `Ok`, i.e. before any `SPAWN` may leave the process.
    pub fn admit_durably_with_request(
        &self,
        session_id: &str,
        request_id: Option<&str>,
    ) -> Result<super::admission_store::ReservationOutcome, super::admission_store::AdmissionRefusal>
    {
        if let Some(refusal) = self.admission_refusal() {
            return Err(super::admission_store::AdmissionRefusal::StoreUnusable(refusal));
        }
        let Some(ledger) = self.admission() else {
            self.admit_session(session_id)
                .map_err(super::admission_store::AdmissionRefusal::Capacity)?;
            return Ok(super::admission_store::ReservationOutcome::Reserved);
        };
        let Some(ceiling) = self.admission_ceiling() else {
            self.admit_session(session_id)
                .map_err(super::admission_store::AdmissionRefusal::Capacity)?;
            return Ok(super::admission_store::ReservationOutcome::Reserved);
        };
        let outcome = ledger.reserve_with_request(session_id, request_id, ceiling)?;
        if let Err(error) = self.admit_session(session_id) {
            // The in-memory ceiling is the tighter one, and nothing has been dispatched, so the
            // durable charge is provably dead and must not be left behind.
            let _ = ledger.abandon_before_dispatch(session_id, "refused by the in-memory ceiling");
            return Err(super::admission_store::AdmissionRefusal::Capacity(error));
        }
        Ok(outcome)
    }

    /// The logical spawn request id durably recorded for `session_id`, for a keeper retry.
    ///
    /// A retry must present this exact id: the keeper retires a duplicate `SPAWN` by request id,
    /// so a fresh id turns "retry of spawn A" into "new spawn A" and can fork a second child once
    /// the first session is gone. `None` means no durable id exists (a direct PTY, or a hub with
    /// no ledger), and the caller then mints one for a new logical spawn.
    pub fn admission_request_id(&self, session_id: &str) -> Option<String> {
        self.admission()?.request_id_for(session_id)
    }

    /// Counts the sessions startup recovered from the keeper. Never refused, never counted twice.
    pub fn note_recovered_sessions(
        &self,
        session_ids: &[String],
    ) -> Result<super::admission_store::RecoveredAccounting, super::admission_store::StoreError> {
        let Some(ledger) = self.admission() else {
            let mut counted = 0;
            for session_id in session_ids {
                self.track_budget_session(session_id);
                counted += 1;
            }
            return Ok(super::admission_store::RecoveredAccounting {
                counted,
                bound_reservations: 0,
            });
        };
        let accounting = ledger.note_recovered_sessions(session_ids)?;
        for session_id in session_ids {
            self.track_budget_session(session_id);
        }
        Ok(accounting)
    }

    /// Adds a session this hub did not admit through [`Self::admit_session`] (an adopted
    /// handover session, or one registered before its spawn gate ran). Accounting must cover it;
    /// the same trims follow because the ceiling changed.
    pub fn track_budget_session(&self, session_id: &str) {
        if self.with_ledger(|budget| budget.track(session_id)).is_none() {
            return;
        }
        self.reapply_ceilings();
        self.enforce_budget();
    }

    /// Drops a session from the budget, which raises every survivor's ceiling again.
    pub fn release_budget_session(&self, session_id: &str) {
        if self.with_ledger(|budget| budget.release(session_id)).is_none() {
            return;
        }
        self.reapply_ceilings();
        if let Some(ledger) = self.admission() {
            // The ledger releases only a bound slot: an unacknowledged reservation may still fork.
            if let Err(error) = ledger.end_session(session_id) {
                tracing::warn!(%session_id, %error, "admission slot could not be released");
            }
        }
    }

    /// The user looked at this session: it is marked active, so only an older active session is
    /// evicted before it, and it is the last candidate of all.
    pub fn note_session_focus(&self, session_id: &str) {
        let _ = self.with_ledger(|budget| budget.note_focus(session_id));
    }

    pub fn note_session_blur(&self, session_id: &str) {
        let _ = self.with_ledger(|budget| budget.note_blur(session_id));
    }

    pub fn subscribe_budget_events(&self) -> broadcast::Receiver<Eviction> {
        self.budget_events.subscribe()
    }

    /// Trims forced so far, the deterministic signal the budget-pressure tests assert on.
    pub fn eviction_count(&self) -> u64 {
        self.with_ledger(|budget| budget.eviction_count()).unwrap_or(0)
    }

    /// Retained bytes this session holds right now, text plus images plus the unit still being
    /// assembled.
    pub fn retained_bytes(&self, session_id: &str) -> usize {
        let Some(session_hub) = self.sessions.read().get(session_id).cloned() else {
            return 0;
        };
        let hub = session_hub.read();
        hub.buffer.retained.retained_bytes()
    }

    /// Retained bytes this session is allowed to hold right now: its share of the budget, or the
    /// hub's configured retention when this hub does not enforce one.
    pub fn session_ceiling_bytes(&self) -> usize {
        self.with_ledger(|budget| budget.ceiling_bytes())
            .unwrap_or_else(|| self.configured_retention_bytes())
    }

    /// Gives every session the budget its current ceiling allows, and never lowers one:
    /// lowering is [`Self::enforce_budget`]'s job, so exactly one path evicts and exactly one path
    /// counts and announces the trim. A raise cannot evict, so this is safe to run on the
    /// add/remove path where the shared ceiling moves.
    fn reapply_ceilings(&self) {
        let Some(ceiling) = self.with_ledger(|budget| budget.ceiling_bytes()) else {
            return;
        };
        let (retention, mirror_limit) = self.retention_and_mirror_limit(ceiling);
        let sessions: Vec<Arc<RwLock<SessionHub>>> =
            self.sessions.read().values().cloned().collect();
        for session_hub in sessions {
            let mut hub = session_hub.write();
            let current = hub.buffer.retained.budget();
            let current_total = current.text_bytes.saturating_add(current.image_bytes);
            if current_total < retention.text_bytes.saturating_add(retention.image_bytes) {
                hub.buffer.retained.set_budget(retention);
            }
            // The parser's ceiling follows the share in both directions. A shrinking share has to
            // reach the screen here even when no retained-history trim is needed: the mirror holds
            // lines outside that ledger, so an older, larger limit would leave the session's real
            // memory above its share with no trim to show for it.
            if let Some(screen) = hub.screen.as_ref() {
                let mut screen = screen.lock();
                if screen.scrollback_limit_bytes() != Some(mirror_limit) {
                    screen.set_scrollback_limit_bytes(mirror_limit);
                }
            }
        }
    }

    /// The byte ceiling the session's screen mirror enforces for `ceiling`. When the budget does not
    /// bind, the mirror keeps the creation-time ceiling rather than an unset (unlimited) limit, so a
    /// hub without a binding budget behaves exactly as it did before the budget existed.
    fn mirror_limit_bytes(&self, ceiling: usize) -> usize {
        if self.budget_binds(ceiling) {
            ceiling
        } else {
            super::NATIVE_SCROLLBACK_CEILING_BYTES
        }
    }

    /// Both halves of a session's share for one ceiling: the retention budget its ring gets and the
    /// byte limit its screen mirror gets. They are derived together because every caller that
    /// resizes a session's share must move both; a share applied to only one of them leaves the
    /// session's real memory above its ceiling with no trim to show for it.
    fn retention_and_mirror_limit(
        &self,
        ceiling: usize,
    ) -> (retained_history::RetentionBudget, usize) {
        (self.retention_for(ceiling), self.mirror_limit_bytes(ceiling))
    }

    /// Records the session's retained history and applies whatever trims the budget requires.
    /// Called after every published chunk, with no hub lock held.
    fn account_retained_history(&self, session_id: &str) {
        let retained = self.retained_bytes(session_id);
        if self
            .with_ledger(|budget| budget.note_retained(session_id, retained))
            .is_none()
        {
            return;
        }
        self.enforce_budget();
    }

    /// Drives the budget's trim plan to completion, applying each trim to the session's retained
    /// history and announcing it as `scrollback_budget_exhausted`. Never holds the budget lock
    /// across a session lock: the plan is taken, then applied.
    fn enforce_budget(&self) {
        let mut stuck = std::collections::HashSet::new();
        loop {
            let Some(trim) = self
                .with_ledger(|budget| budget.next_trim_excluding(&stuck))
                .flatten()
            else {
                return;
            };
            let Some(session_hub) = self.sessions.read().get(&trim.session_id).cloned() else {
                // Not registered in this hub yet: nothing to trim, and the ceiling it will be
                // built with is already recorded. Retiring it keeps the loop from spinning.
                stuck.insert(trim.session_id);
                continue;
            };
            let retained_after = {
                let mut hub = session_hub.write();
                let (retention, mirror_limit) =
                    self.retention_and_mirror_limit(trim.target_bytes);
                hub.buffer.retained.set_budget(retention);
                if let Some(screen) = hub.screen.as_ref() {
                    screen
                        .lock()
                        .set_scrollback_limit_bytes(mirror_limit);
                }
                hub.buffer.retained.retained_bytes()
            };
            let eviction = match self.with_ledger(|budget| {
                budget.record_trim(&trim.session_id, trim.target_bytes, retained_after)
            }) {
                Some(eviction) => eviction,
                None => return,
            };
            if eviction.released_bytes == 0 {
                // This session's whole retained history is the unit it is assembling right now:
                // nothing more can go. Its ceiling stays in force for the next write, and the
                // remaining sessions are still brought down.
                stuck.insert(trim.session_id);
                continue;
            }
            tracing::warn!(
                session_id = %eviction.session_id,
                ceiling_bytes = eviction.ceiling_bytes,
                released_bytes = eviction.released_bytes,
                eviction_count = eviction.eviction_count,
                "scrollback_budget_exhausted"
            );
            let _ = self.budget_events.send(eviction);
        }
    }

    /// Installs where program status summary changes of opted-in sessions are published.
    pub(crate) fn set_program_status_publisher(
        &self,
        publisher: Arc<dyn super::program_status_ingest::ProgramStatusPublisher>,
    ) {
        *self.program_status_publisher.write() = Some(publisher);
    }

    /// Makes the session's screen mirror its OSC 7501 consumer and support query replier. Only
    /// for sessions this daemon answers for: local PTYs and SSH bridge sessions, never a paired
    /// proxy. Returns false when the session or its mirror is missing or the parser refused.
    pub fn enable_program_status(&self, session_id: &str) -> bool {
        let Some(session_hub) = self.sessions.read().get(session_id).cloned() else {
            return false;
        };
        let hub = session_hub.read();
        let Some(screen) = hub.screen.as_ref() else {
            return false;
        };
        let enabled = screen.lock().enable_program_status();
        enabled
    }

    /// The session was seen: drops its `done`/`error` program status and publishes the summary
    /// afterwards. `None` when the session did not opt in, else that summary.
    pub fn acknowledge_program_outcome(
        &self,
        session_id: &str,
    ) -> Option<Option<super::program_status::ProgramStatusSummary>> {
        self.update_program_status(session_id, |screen| screen.acknowledge_program_status())
    }

    /// The session's foreground program exited, as proven by evidence gathered while `through`
    /// was its newest published revision: drops the transient program status applied by then
    /// and publishes the summary afterwards. `None` when the session did not opt in, else that
    /// summary.
    pub fn program_exited(
        &self,
        session_id: &str,
        through: u64,
    ) -> Option<Option<super::program_status::ProgramStatusSummary>> {
        self.update_program_status(session_id, |screen| screen.program_status_exited(through))
    }

    /// The revision of the session's newest program status update, 0 before any or when the
    /// session has no mirror. Read before a process listing is requested, it bounds the records
    /// that listing's evidence covers.
    pub fn program_status_revision(&self, session_id: &str) -> u64 {
        let Some(session_hub) = self.sessions.read().get(session_id).cloned() else {
            return 0;
        };
        let hub = session_hub.read();
        hub.screen
            .as_ref()
            .map_or(0, |screen| screen.lock().program_status_revision())
    }

    /// Runs a record update under the screen lock and, when the summary changed, publishes the
    /// revisioned result after every lock is released, through the same path as live feeds.
    fn update_program_status(
        &self,
        session_id: &str,
        update: impl FnOnce(
            &mut super::screen_mirror::ScreenMirror,
        ) -> Option<(bool, super::program_status_ingest::ProgramStatusChange)>,
    ) -> Option<Option<super::program_status::ProgramStatusSummary>> {
        let session_hub = self.sessions.read().get(session_id).cloned()?;
        let (changed, change) = {
            let hub = session_hub.read();
            let mut screen = hub.screen.as_ref()?.lock();
            update(&mut screen)
        }?;
        if changed {
            self.publish_program_status(session_id, &change);
        }
        Some(change.summary)
    }

    fn publish_program_status(
        &self,
        session_id: &str,
        change: &super::program_status_ingest::ProgramStatusChange,
    ) {
        let publisher = self.program_status_publisher.read().clone();
        if let Some(publisher) = publisher {
            publisher.publish(session_id, change);
        }
    }

    /// Records how a session's process ended. `None` is an unknown status, not a success.
    pub fn record_exit(&self, session_id: &str, code: Option<i32>) {
        if let Some(status) = self.exit_status(session_id) {
            *status.write() = code;
        }
    }

    /// The recorded exit code of the session's process, if it ended with a known one.
    pub fn exit_code(&self, session_id: &str) -> Option<i32> {
        self.exit_status(session_id).and_then(|status| *status.read())
    }

    pub(crate) fn exit_status(&self, session_id: &str) -> Option<ExitStatusSlot> {
        if let Some(status) = self.exits.lock().get(session_id).cloned() {
            return Some(status);
        }
        if let Some(hub) = self.sessions.read().get(session_id) {
            return Some(Arc::clone(&hub.read().exit_status));
        }
        None
    }

    pub(crate) fn begin_exit_status(&self, session_id: &str) -> ExitStatusSlot {
        let status = Arc::new(RwLock::new(None));
        let mut exits = self.exits.lock();
        // Keep the status alive between PTY creation and output registration even
        // when a fast child exits and leaves the PTY registry during that interval.
        exits.insert(session_id.to_string(), Arc::clone(&status));
        drop(exits);
        if let Some(hub) = self.sessions.read().get(session_id) {
            hub.write().exit_status = Arc::clone(&status);
        }
        status
    }

    pub fn register_session(&self, session_id: &str) -> broadcast::Receiver<Vec<u8>> {
        let (raw_rx, _seq_rx) = self.register_session_channels(session_id);
        raw_rx
    }

    pub fn register_session_with_sequence(
        &self,
        session_id: &str,
    ) -> broadcast::Receiver<OutputChunk> {
        let (_raw_rx, seq_rx) = self.register_session_channels(session_id);
        seq_rx
    }

    pub fn register_session_channels(
        &self,
        session_id: &str,
    ) -> (
        broadcast::Receiver<Vec<u8>>,
        broadcast::Receiver<OutputChunk>,
    ) {
        let (tx, rx) = broadcast::channel(BROADCAST_CAPACITY);
        let (raw_tx, raw_rx) = broadcast::channel(RAW_BROADCAST_CAPACITY);
        // Tracked before the ceiling is read so the new session's own share already accounts for
        // it; a session registered without passing the spawn gate (an adopted handover session,
        // a paired proxy) is still covered, or its history would be unbounded.
        let _ = self.with_ledger(|budget| budget.track(session_id));
        let ceiling = self.session_ceiling_bytes();
        let (retention, mirror_limit) = self.retention_and_mirror_limit(ceiling);
        let hub = SessionHub {
            exit_status: self.exit_status(session_id).unwrap_or_else(|| self.begin_exit_status(session_id)),
            machine_senders: Vec::new(),
            // The session's share of the daemon budget, not the hub-wide ring capacity: the
            // ceiling is recomputed on every session add/remove.
            buffer: BoundedBuffer::with_retention(self.capacity, retention),
            sender: tx,
            raw_sender: raw_tx,
            resize_ledger: Vec::new(),
            initial_size: None,
            replay_gap: None,
            history_incomplete: false,
            subscribed: false,
            ended: false,
            published_bytes: 0,
            screen: super::screen_mirror::ScreenMirror::new(80, 24).map(parking_lot::Mutex::new),
        };
        let hub = Arc::new(RwLock::new(hub));
        {
            let registered = hub.read();
            if let Some(screen) = registered.screen.as_ref() {
                screen.lock().set_scrollback_limit_bytes(mirror_limit);
            }
        }
        spawn_screen_compression(&hub);
        self.sessions.write().insert(session_id.to_string(), hub);
        // Survivors whose ceiling the new session just lowered are brought down now.
        self.enforce_budget();
        (raw_rx, rx)
    }

    pub fn publish(&self, session_id: &str, chunk_bytes: Vec<u8>) -> Option<OutputChunk> {
        self.publish_with_read_timestamp(session_id, chunk_bytes, None)
    }

    pub fn publish_gap(&self, session_id: &str) -> Option<OutputChunk> {
        let session_hub = self.sessions.read().get(session_id).cloned()?;
        let mut hub = session_hub.write();
        let sequence = hub.buffer.allocate_sequence();
        let gap = ReplayGap {
            requested_after_sequence: sequence.saturating_sub(1),
            available_from_sequence: sequence + 1,
        };
        hub.buffer.gap_floor = sequence + 1;
        const GAP_SEAM: &[u8] = b"\r\n\x1b[0m\x1b[33m[Ferryx: output stream gap - some terminal output was missed]\x1b[0m\r\n";
        hub.buffer.retained.push(sequence, GAP_SEAM);
        let seam = OutputChunk {
            sequence,
            bytes: Arc::from(GAP_SEAM),
            metrics_read_unix_micros: None,
            replay_gap: None,
        };
        hub.buffer.current_size += seam.bytes.len();
        hub.buffer.chunks.push_back(seam);
        while hub.buffer.current_size > hub.buffer.capacity && !hub.buffer.chunks.is_empty() {
            if let Some(front) = hub.buffer.chunks.pop_front() {
                hub.buffer.current_size -= front.bytes.len();
            }
        }
        if let Some(screen) = hub.screen.as_ref() {
            screen.lock().feed_replay(GAP_SEAM);
        }
        hub.replay_gap = Some(gap.clone());
        hub.history_incomplete = true;
        let boundary = OutputChunk {
            sequence,
            bytes: Arc::from([]),
            metrics_read_unix_micros: None,
            replay_gap: Some(gap),
        };
        hub.machine_senders
            .retain(|sender| sender.publish(&boundary));
        let _ = hub.sender.send(boundary.clone());
        Some(boundary)
    }

    /// Publishes a chunk for a caller that does not write the session's input. Any OSC 7501 query
    /// reply the chunk produced is dropped; output pumps of sessions that opted into program
    /// status use [`Self::publish_with_replies`].
    pub fn publish_with_read_timestamp(
        &self,
        session_id: &str,
        chunk_bytes: Vec<u8>,
        metrics_read_unix_micros: Option<u64>,
    ) -> Option<OutputChunk> {
        self.publish_with_replies(session_id, chunk_bytes, metrics_read_unix_micros)
            .map(|(chunk, _replies)| chunk)
    }

    /// Publishes a chunk and returns the OSC 7501 support query replies it produced, for the
    /// caller to write to the session's input. Returned after every hub and screen lock is
    /// released; a program status summary change is published the same way.
    pub fn publish_with_replies(
        &self,
        session_id: &str,
        chunk_bytes: Vec<u8>,
        metrics_read_unix_micros: Option<u64>,
    ) -> Option<(OutputChunk, Vec<Vec<u8>>)> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;

        let mut hub = session_hub.write();
        let chunk = hub
            .buffer
            .push_with_read_timestamp(chunk_bytes, metrics_read_unix_micros)?;
        let ingest = hub.screen.as_ref().map(|screen| {
            let mut screen = screen.lock();
            screen.feed(&chunk.bytes);
            screen.take_program_status()
        });
        hub.replay_gap = None;
        if hub.history_incomplete && resets_whole_screen(&chunk.bytes) {
            hub.history_incomplete = false;
        }
        hub.published_bytes = hub
            .published_bytes
            .checked_add(chunk.bytes.len() as u64)
            .expect("published byte counter overflow");

        hub.machine_senders.retain(|sender| sender.publish(&chunk));
        // Broadcast to sequence subscribers (cheap Arc refcount bump on the payload).
        let _ = hub.sender.send(chunk.clone());
        // Broadcast to legacy raw receivers. Skip the deep copy when nobody is listening:
        // a subscriber that attaches later gets its history from the buffer snapshot, and
        // one that falls more than BROADCAST_CAPACITY chunks behind resyncs via Lagged +
        // ReplayGap recovery, so skipping can only drop copies, never correctness.
        if hub.raw_sender.receiver_count() > 0 {
            let _ = hub.raw_sender.send(chunk.bytes.to_vec());
        }
        drop(hub);
        let ingest = ingest.unwrap_or_default();
        if let Some(revision) = ingest.transient_applied {
            let publisher = self.program_status_publisher.read().clone();
            if let Some(publisher) = publisher {
                publisher.transient_applied(session_id, revision);
            }
        }
        if let Some(change) = ingest.summary.as_ref() {
            self.publish_program_status(session_id, change);
        }
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        crate::terminal::qa_liveness::observe_published_output(
            session_id,
            chunk.sequence,
            Arc::clone(&chunk.bytes),
        );
        // Charged after every lock is released: the budget may trim this or another session.
        self.account_retained_history(session_id);

        Some((chunk, ingest.replies))
    }

    pub fn record_initial_size(&self, session_id: &str, cols: u16, rows: u16) {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        };
        let Some(session_hub) = session_hub else {
            return;
        };

        let mut hub = session_hub.write();
        // Set once: a reconnect or phone resize reports a later size, and the baseline must
        // keep naming the geometry the host session was created with.
        if hub.initial_size.is_none() && cols > 0 && rows > 0 {
            hub.initial_size = Some((cols, rows));
        }
        if hub.resize_ledger.is_empty() {
            hub.resize_ledger.push(ResizePoint {
                sequence: 0,
                cols,
                rows,
            });
            if let Some(screen) = hub.screen.as_ref() {
                screen.lock().resize(cols, rows);
            }
        }
    }

    /// The size this session was first seen at, surviving resize-ledger eviction.
    pub fn initial_size(&self, session_id: &str) -> Option<(u16, u16)> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;
        let initial_size = session_hub.read().initial_size;
        initial_size
    }

    pub fn record_resize(&self, session_id: &str, cols: u16, rows: u16) -> Option<u64> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;

        let mut hub = session_hub.write();
        // Accepted boundary fuzz: a few in-flight bytes produced just before SIGWINCH may carry
        // sequences greater than the marker, identical to what the live pane experienced.
        let sequence = hub.buffer.allocate_sequence();
        hub.buffer.retained.seal();
        if hub.resize_ledger.len() >= RESIZE_LEDGER_CAPACITY {
            hub.resize_ledger.remove(0);
        }
        hub.resize_ledger.push(ResizePoint {
            sequence,
            cols,
            rows,
        });
        if let Some(screen) = hub.screen.as_ref() {
            screen.lock().resize(cols, rows);
        }
        Some(sequence)
    }

    /// Text of the session's mirrored visible screen. The hub and mirror locks are held only
    /// while the rows are copied out; trimming and gap-seam removal run after they are released.
    pub fn visible_screen_text(
        &self,
        session_id: &str,
    ) -> Option<super::screen_mirror::VisibleScreen> {
        let session_hub = self.sessions.read().get(session_id).cloned()?;
        let rows = {
            let hub = session_hub.read();
            let screen = hub.screen.as_ref()?.lock();
            screen.visible_rows()
        }?;
        Some(rows.into_screen())
    }

    pub fn subscribe(&self, session_id: &str) -> Option<(Vec<u8>, broadcast::Receiver<Vec<u8>>)> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;

        let mut hub = session_hub.write();
        // Subscribe first to preserve subscriber-first snapshot invariant
        let rx = hub.raw_sender.subscribe();
        let history = hub.buffer.snapshot();
        hub.subscribed = true;
        let ended = hub.ended;
        drop(hub);
        // The pane in view is the last eviction candidate, so subscribing is a focus event.
        self.note_session_focus(session_id);
        if ended {
            self.remove_ended_session(session_id);
        }
        Some((history, rx))
    }

    #[cfg(test)]
    pub(crate) fn observe_for_test(
        &self,
        session_id: &str,
    ) -> Option<(Vec<u8>, broadcast::Receiver<Vec<u8>>)> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;
        let hub = session_hub.read();
        let rx = hub.raw_sender.subscribe();
        let history = hub.buffer.snapshot();
        Some((history, rx))
    }

    /// Full attaches strip replay-only device interrogation, preserving cursor-position
    /// requests for the terminal emulator. Only an evicted full head is UTF-8-trimmed;
    /// sequence resumes remain byte-exact ([`super::remote::clean_replay`]).
    ///
    /// This is the replay the remote gateway's live terminal socket and its VT-mirror readers are
    /// served, so it is the same boundary as [`Self::subscribe_with_screen_mirror`] and uses the
    /// same helper. Cleaning happens on the bytes handed out, not in the buffer: retention keeps
    /// the byte-exact record that callers addressing history by sequence account for.
    pub fn subscribe_with_sequence(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Option<SessionAttachment> {
        let (mut attachment, ranges) = self.subscribe_with_sequence_ranges(session_id, after_sequence)?;
        let ranges = super::remote::clean_replay(&mut attachment, ranges, after_sequence);
        attachment.snapshot.history_segments = ranges.into_iter().map(|r| HistorySegment {
            cols: r.cols,
            rows: r.rows,
            bytes: attachment.snapshot.history[r.start..r.end].to_vec(),
        }).collect();
        Some(attachment)
    }

    // Subscribe and capture offsets under one lock, without duplicating snapshot bytes.
    pub(crate) fn subscribe_with_sequence_ranges(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Option<(SessionAttachment, Vec<HistoryRange>)> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;

        let mut hub = session_hub.write();
        // 1. Subscribe FIRST in the critical section
        let rx = hub.sender.subscribe();
        hub.subscribed = true;
        // 2. Snapshot within the same critical section
        let (history, history_segments, history_start_sequence, history_end_sequence, gap) = hub
            .buffer
            .snapshot_after_ranges(after_sequence, &hub.resize_ledger);
        let gap = gap.or_else(|| {
            hub.replay_gap.clone().filter(|gap| {
                after_sequence.is_some_and(|after| after < gap.available_from_sequence - 1)
            })
        });
        let snapshot = AttachmentSnapshot {
            session_id: session_id.to_string(),
            history_start_sequence,
            history_end_sequence,
            history,
            history_segments: Vec::new(),
            gap,
            history_incomplete: hub.history_incomplete(),
        };
        let ended = hub.ended;
        drop(hub);
        // The pane in view is the last eviction candidate, so subscribing is a focus event.
        self.note_session_focus(session_id);
        if ended {
            self.remove_ended_session(session_id);
        }

        Some((SessionAttachment {
            snapshot,
            receiver: rx,
        }, history_segments))
    }

    /// Like [`Self::subscribe_with_sequence_ranges`] but substitutes the mirrored screen for
    /// raw bytes when the retained replay starts mid-stream (eviction happened).
    ///
    /// This is the replay a local session's attach delivers, so it is cleaned the same way a
    /// remote replay is: full snapshots strip replay-only interrogation but retain emulator
    /// cursor queries; sequence resumes remain byte-exact ([`super::remote::clean_replay`]).
    /// Cleaning happens here, on the bytes handed out, and
    /// not in the buffer: retention keeps the byte-exact record that callers addressing
    /// history by sequence account for, and eviction is untouched.
    pub(crate) fn subscribe_with_screen_mirror(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Option<(SessionAttachment, Vec<HistoryRange>)> {
        let session_hub = self.sessions.read().get(session_id).cloned()?;
        let (mut attachment, mut ranges) =
            self.subscribe_with_sequence_ranges(session_id, after_sequence)?;
        let hub = session_hub.read();
        let retained_lost_bytes = hub.buffer.retained_start_sequence().is_some_and(|seq| seq > 1);
        let is_full_replay = after_sequence.is_none() || attachment.snapshot.gap.is_some();
        if is_full_replay && retained_lost_bytes {
            if let Some(seed) = hub.screen_seed() {
                ranges = vec![HistoryRange {
                    cols: Some(seed.cols),
                    rows: Some(seed.rows),
                    start: 0,
                    end: seed.bytes.len(),
                }];
                attachment.snapshot.history = seed.bytes;
            }
        }
        drop(hub);
        let ranges = super::remote::clean_replay(&mut attachment, ranges, after_sequence);
        Some((attachment, ranges))
    }

    /// The per-session recovery ring's byte capacity.
    ///
    /// This is the most history one attach can be served as a single chunk: [`BoundedBuffer`]
    /// evicts whatever pushes the total past it. A recovery that has to rebuild a session's history
    /// from the keeper reads this many trailing bytes and no more, so the first chunk it publishes
    /// is already the whole tail a reader needs instead of a prefix that only becomes the history
    /// once the last page lands.
    pub fn session_ring_capacity(&self) -> usize {
        self.capacity
    }

    /// Records that the history this hub holds begins **mid-stream**, without publishing a seam.
    ///
    /// [`Self::publish_gap`] marks a hole that opens inside a live stream: it pushes seam text so a
    /// reader sees where output was lost. A recovery hole is the other shape -- the bytes before
    /// the recovered tail were never in this hub at all -- so there is no splice to announce, and
    /// pushing seam bytes here would shift every later hub byte off the keeper's offset axis by the
    /// seam's length, which is exactly the axis a checkpoint offset is derived from. The floor and
    /// the incomplete flag carry the same information without moving it.
    pub fn mark_history_gap(&self, session_id: &str) -> bool {
        let Some(session_hub) = self.sessions.read().get(session_id).cloned() else {
            return false;
        };
        let mut hub = session_hub.write();
        let floor = hub.buffer.next_sequence().max(1);
        hub.buffer.gap_floor = floor;
        hub.replay_gap = Some(ReplayGap {
            requested_after_sequence: floor.saturating_sub(1),
            available_from_sequence: floor,
        });
        hub.history_incomplete = true;
        true
    }

    pub fn remove_session(&self, session_id: &str) {
        self.sessions.write().remove(session_id);
        self.exits.lock().remove(session_id);
        self.transport_owners.write().remove(session_id);
        self.release_budget_session(session_id);
    }

    pub fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().contains_key(session_id)
    }

    /// The session's PTY output ended. A session that was subscribed to is removed, which ends
    /// every subscriber's stream. One nobody has subscribed to yet (a command that exited before
    /// its first attach) keeps its output for that first attach, which then removes it; returns
    /// `false` in that case so the caller can bound the wait with `remove_ended_session`.
    pub fn end_session(&self, session_id: &str) -> bool {
        let Some(session_hub) = self.sessions.read().get(session_id).cloned() else {
            return true;
        };
        {
            let mut hub = session_hub.write();
            if !hub.subscribed {
                hub.ended = true;
                return false;
            }
        }
        self.remove_session(session_id);
        true
    }

    /// Removes a session that `end_session` kept for a first attach, if it is still waiting.
    pub fn remove_ended_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write();
        if sessions.get(session_id).is_some_and(|hub| hub.read().ended) {
            sessions.remove(session_id);
            drop(sessions);
            self.exits.lock().remove(session_id);
            self.transport_owners.write().remove(session_id);
            // The session is gone, so its share of the budget goes back to the survivors.
            self.release_budget_session(session_id);
        }
    }

    pub fn claim_transport(&self, session_id: &str) -> bool {
        self.transport_owners.write().insert(session_id.to_string())
    }

    pub fn release_transport(&self, session_id: &str) {
        self.transport_owners.write().remove(session_id);
    }

    pub fn transport_owner(&self, session_id: &str) -> bool {
        self.transport_owners.read().contains(session_id)
    }

    pub fn session_sequence_range(&self, session_id: &str) -> Option<(Option<u64>, Option<u64>)> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;
        let hub = session_hub.read();
        Some((hub.buffer.start_sequence(), hub.buffer.end_sequence()))
    }

    pub fn is_bracketed_paste_enabled(&self, session_id: &str) -> bool {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        };
        let Some(session_hub) = session_hub else {
            return false;
        };
        let enabled = session_hub.read().buffer.bracketed_paste_enabled();
        enabled
    }

    pub fn export_session_state(&self, session_id: &str) -> Option<SessionHubSnapshot> {
        let (snapshot, _published_bytes) = self.export_session_state_with_offset(session_id)?;
        Some(snapshot)
    }

    /// [`Self::export_session_state`] plus the cumulative published-byte counter, both read under
    /// the same `SessionHub` lock so the state and the offset are one atomic observation.
    ///
    /// The counter is the number of output bytes this hub has published for the session, not a
    /// chunk or sequence count. It is never larger than what the snapshot contains: a gap seam adds
    /// text to the state without moving the counter ([`Self::publish_gap`]), so the state covers at
    /// least `[0, published_bytes)`. A caller that commits a checkpoint against a larger number
    /// truncates replay past bytes the state never applied; reading the counter in a second call has
    /// the same hole, because the hub applies chunks on another thread.
    pub fn export_session_state_with_offset(
        &self,
        session_id: &str,
    ) -> Option<(SessionHubSnapshot, u64)> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;
        let hub = session_hub.read();
        let mut snapshot = hub.export_state();
        snapshot.transport_owner = self.transport_owner(session_id);
        Some((snapshot, hub.published_bytes))
    }

    pub fn import_session_state(
        &self,
        session_id: &str,
        snapshot: SessionHubSnapshot,
    ) -> Result<(), OutputHubSnapshotError> {
        snapshot.validate_with_capacity(self.capacity)?;

        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        let (raw_tx, _raw_rx) = broadcast::channel(BROADCAST_CAPACITY);

        let capacity = if snapshot.capacity > 0 {
            snapshot.capacity
        } else {
            self.capacity
        };

        let replay_gap = snapshot.replay_gap.clone();
        let mut buffer = BoundedBuffer::from_snapshot(
            capacity,
            snapshot.chunks,
            snapshot.next_sequence,
            snapshot.bracketed_paste_enabled,
            snapshot.retained_state,
        );
        buffer.gap_floor = snapshot.gap_floor;

        let is_transport_owner = snapshot.transport_owner;
        // A predecessor that predates `initial_size` sends no value; its oldest ledger point is
        // the best remaining evidence. Newer snapshots carry the real baseline and always win.
        let initial_size = snapshot
            .initial_size
            .or_else(|| snapshot.resize_ledger.first().map(|point| (point.cols, point.rows)));
        // The predecessor's mirrored screen is the authoritative starting point; without one the
        // mirror is rebuilt from the retained bytes, as good as a raw replay and no worse.
        let screen = match snapshot.screen_seed.as_ref() {
            Some(seed) => super::screen_mirror::ScreenMirror::from_seed(seed),
            None => {
                let (cols, rows) = snapshot
                    .resize_ledger
                    .last()
                    .map(|point| (point.cols, point.rows))
                    .unwrap_or((80, 24));
                super::screen_mirror::ScreenMirror::new(cols, rows).map(|mut mirror| {
                    mirror.feed_replay(&buffer.retained.replay());
                    mirror
                })
            }
        };
        let hub = SessionHub {
            exit_status: self.exit_status(session_id).unwrap_or_else(|| self.begin_exit_status(session_id)),
            machine_senders: Vec::new(),
            buffer,
            sender: tx,
            raw_sender: raw_tx,
            resize_ledger: snapshot.resize_ledger,
            initial_size,
            history_incomplete: snapshot.history_incomplete,
            replay_gap,
            subscribed: false,
            ended: false,
            published_bytes: 0,
            screen: screen.map(parking_lot::Mutex::new),
        };

        let hub = Arc::new(RwLock::new(hub));
        spawn_screen_compression(&hub);
        self.sessions.write().insert(session_id.to_string(), hub);

        if is_transport_owner {
            self.transport_owners.write().insert(session_id.to_string());
        } else {
            self.transport_owners.write().remove(session_id);
        }

        // The imported history is the predecessor's and had no ceiling here yet: the session is
        // live in this daemon now, so it takes a share of the budget like any other. Without this
        // a restored session's retained bytes would sit outside every bound.
        self.track_budget_session(session_id);

        Ok(())
    }

    pub fn session_next_sequence(&self, session_id: &str) -> Option<u64> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;
        let hub = session_hub.read();
        Some(hub.buffer.next_sequence())
    }

    /// Cumulative bytes published to this session's hub across its local in-process lifetime.
    ///
    /// This counter measures actual committed output chunks produced into the local hub.
    /// It is intentionally scoped to the local session hub lifetime and resets on state import.
    pub fn session_published_bytes(&self, session_id: &str) -> Option<u64> {
        let session_hub = self.sessions.read().get(session_id).cloned()?;
        let hub = session_hub.read();
        Some(hub.published_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    #[test]
    fn output_registration_preserves_a_fast_child_exit() {
        let hub = TerminalOutputHub::default();
        let status = hub.begin_exit_status("fast-child");
        hub.record_exit("fast-child", Some(37));
        drop(status);
        let _output = hub.register_session("fast-child");
        assert_eq!(hub.exit_code("fast-child"), Some(37));
    }

    #[test]
    fn new_pty_lifetime_forgets_the_previous_exit() {
        let hub = TerminalOutputHub::default();
        let previous = hub.begin_exit_status("reused-id");
        hub.record_exit("reused-id", Some(37));
        let _current = hub.begin_exit_status("reused-id");
        let _output = hub.register_session("reused-id");
        assert_eq!(hub.exit_code("reused-id"), None);
        hub.record_exit("reused-id", Some(9));
        assert_eq!(hub.exit_code("reused-id"), Some(9));
        assert_eq!(*previous.read(), Some(37));
    }

    #[test]
    fn late_attach_retains_exit_across_many_other_exits() {
        let hub = TerminalOutputHub::default();
        let output = hub.register_session("late");
        hub.record_exit("late", Some(37));
        assert!(!hub.end_session("late"));
        drop(output);
        for index in 0..300 {
            let id = format!("other-{index}");
            let output = hub.register_session(&id);
            hub.record_exit(&id, Some(1));
            hub.remove_session(&id);
            drop(output);
        }
        let status = hub.exit_status("late").expect("retained status");
        let _attachment = hub.subscribe_with_screen_mirror("late", None).expect("late attach");
        assert!(!hub.has_session("late"));
        assert_eq!(*status.read(), Some(37));
        drop(status);
        let _next = hub.begin_exit_status("next");
        assert!(!hub.exits.lock().contains_key("late"));
    }

    #[test]
    fn local_attach_strips_device_queries_from_replayed_history() {
        // Given: a local session whose retained text holds device queries plus a response a
        // program printed itself.
        let hub = TerminalOutputHub::new(64 * 1024);
        let _rx = hub.register_session("local-query");
        hub.publish("local-query", b"prompt$ \x1b[6n\x1b]11;?\x07".to_vec())
            .expect("publish the queries");
        hub.publish("local-query", b"\x1b[1;1Rvisible\r\n".to_vec())
            .expect("publish the printed response");

        // When: the GUI attaches. For a local session this is the replay the daemon delivers.
        let (attachment, ranges) = hub
            .subscribe_with_screen_mirror("local-query", None)
            .expect("local attach");

        // Cursor queries must reach the emulator; replay-only colour interrogation must not.
        // Printed responses remain real output.
        let history = &attachment.snapshot.history;
        assert!(
            history.windows(4).any(|w| w == b"\x1b[6n"),
            "emulator cursor query was stripped: {:?}",
            String::from_utf8_lossy(history)
        );
        assert!(
            !history.windows(6).any(|w| w == b"\x1b]11;?"),
            "colour query reached the client: {:?}",
            String::from_utf8_lossy(history)
        );
        assert!(
            history.windows(6).any(|w| w == b"\x1b[1;1R"),
            "printed response was stripped: {:?}",
            String::from_utf8_lossy(history)
        );
        assert!(history.windows(7).any(|w| w == b"visible"));

        // And the returned offsets still address the cleaned bytes, so the wire segments the
        // daemon slices from them cover the replay exactly.
        let joined: Vec<u8> = ranges
            .iter()
            .flat_map(|range| history[range.start..range.end].to_vec())
            .collect();
        assert_eq!(joined.as_slice(), history.as_slice(), "segment offsets do not match the cleaned history");
    }

    #[test]
    fn sequence_attach_strips_device_queries_from_replayed_history() {
        // Given: a session whose retained text holds device queries plus a response a program
        // printed itself.
        let hub = TerminalOutputHub::new(64 * 1024);
        let _rx = hub.register_session("sequence-query");
        hub.publish("sequence-query", b"prompt$ \x1b[6n\x1b]11;?\x07".to_vec())
            .expect("publish the queries");
        hub.publish("sequence-query", b"\x1b[1;1Rvisible\r\n".to_vec())
            .expect("publish the printed response");

        // When: a client attaches through the sequence path - the replay the remote gateway's
        // live terminal socket and its VT-mirror readers are served.
        let attachment = hub
            .subscribe_with_sequence("sequence-query", None)
            .expect("sequence attach");

        // Keep the emulator's startup handshake while stripping replay-only interrogation.
        let history = &attachment.snapshot.history;
        assert!(
            history.windows(4).any(|w| w == b"\x1b[6n"),
            "emulator cursor query was stripped: {:?}",
            String::from_utf8_lossy(history)
        );
        assert!(
            !history.windows(6).any(|w| w == b"\x1b]11;?"),
            "colour query reached the client: {:?}",
            String::from_utf8_lossy(history)
        );
        assert!(
            history.windows(6).any(|w| w == b"\x1b[1;1R"),
            "printed response was stripped: {:?}",
            String::from_utf8_lossy(history)
        );
        assert!(history.windows(7).any(|w| w == b"visible"));

        // And the segments were cut from the cleaned bytes with the remapped offsets, so they
        // still join back into the history the client is sent.
        let joined: Vec<u8> = attachment
            .snapshot
            .history_segments
            .iter()
            .flat_map(|segment| segment.bytes.clone())
            .collect();
        assert_eq!(
            joined.as_slice(),
            history.as_slice(),
            "segment offsets do not match the cleaned history"
        );
    }

    #[test]
    fn resumed_replay_preserves_utf8_split_across_sequence_boundary() {
        let hub = TerminalOutputHub::new(64 * 1024);
        let _rx = hub.register_session("split-utf8");
        let first: u64 = hub.publish("split-utf8", vec![0xe2])
            .expect("publish lead").sequence;
        hub.publish("split-utf8", vec![0x82, 0xac]).expect("publish tail");

        let attachment = hub.subscribe_with_sequence("split-utf8", Some(first))
            .expect("resume sequence replay");
        assert!(attachment.snapshot.gap.is_none());
        assert_eq!(attachment.snapshot.history, vec![0x82, 0xac]);
        let mut client = vec![0xe2];
        client.extend_from_slice(&attachment.snapshot.history);
        assert_eq!(std::str::from_utf8(&client).expect("complete character"), "\u{20ac}");
        let segments: Vec<u8> = attachment.snapshot.history_segments.iter()
            .flat_map(|segment| segment.bytes.iter().copied()).collect();
        assert_eq!(segments, vec![0x82, 0xac]);

        let (attachment, ranges) = hub.subscribe_with_screen_mirror("split-utf8", Some(first))
            .expect("resume screen replay");
        assert_eq!(attachment.snapshot.history, vec![0x82, 0xac]);
        let joined: Vec<u8> = ranges.iter().flat_map(|range|
            attachment.snapshot.history[range.start..range.end].iter().copied()).collect();
        assert_eq!(joined, vec![0x82, 0xac]);
    }

    #[test]
    fn full_replay_preserves_a_pending_utf8_lead_without_eviction() {
        let hub = TerminalOutputHub::new(64 * 1024);
        let _rx = hub.register_session("pending-utf8");
        hub.publish("pending-utf8", vec![0xe2]).expect("publish lead");
        let attachment = hub.subscribe_with_sequence("pending-utf8", None).expect("full replay");
        assert_eq!(attachment.snapshot.history, vec![0xe2]);
    }

    #[test]
    fn imported_output_retains_the_adopted_child_status() {
        let hub = TerminalOutputHub::default();
        let _output = hub.register_session("adopted");
        let status = hub.begin_exit_status("adopted");
        *status.write() = Some(7);
        drop(status);
        assert_eq!(hub.exit_code("adopted"), Some(7));
    }

    struct CountingAlloc;
    thread_local! {
        static ALLOC_COUNTS: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
    }
    impl CountingAlloc {
        fn adjust(old: usize, new: usize) {
            let _ = ALLOC_COUNTS.try_with(|counts| {
                if let Some((current, peak)) = counts.get() {
                    let current = current.saturating_sub(old) + new;
                    counts.set(Some((current, peak.max(current))));
                }
            });
        }
        fn enable() { ALLOC_COUNTS.with(|c| c.set(Some((0, 0)))); }
        fn disable() -> usize {
            ALLOC_COUNTS.with(|c| c.replace(None).unwrap().1)
        }
    }
    unsafe impl GlobalAlloc for CountingAlloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = System.alloc(layout);
            if !ptr.is_null() { Self::adjust(0, layout.size()); }
            ptr
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let ptr = System.alloc_zeroed(layout);
            if !ptr.is_null() { Self::adjust(0, layout.size()); }
            ptr
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            System.dealloc(ptr, layout);
            Self::adjust(layout.size(), 0);
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let new_ptr = System.realloc(ptr, layout, size);
            if !new_ptr.is_null() { Self::adjust(layout.size(), size); }
            new_ptr
        }
    }
    #[global_allocator]
    static COUNTING_ALLOC: CountingAlloc = CountingAlloc;

    #[test]
    fn alloc_peak_single_materialization() {
        let retained = 192 * 1024;
        let mut buffer = BoundedBuffer::new(retained);
        for _ in 0..48 { buffer.push(vec![b'x'; retained / 48]); }
        let ledger = vec![ResizePoint { sequence: 1, cols: 80, rows: 24 }];
        CountingAlloc::enable();
        let snapshot = buffer.snapshot_after_ranges_retained(None, &ledger);
        let peak = CountingAlloc::disable();
        assert_eq!(snapshot.0.len(), retained);
        assert!(!snapshot.1.is_empty());
        println!("alloc peak: {peak}/{retained} = {:.6}x", peak as f64 / retained as f64);
        assert!(peak * 100 <= retained * 135, "peak_bytes={peak}, retained={retained}, ratio={:.6}", peak as f64 / retained as f64);
    }

    #[test]
    fn attach_mid_transmission_then_live_continuation_reconstructs_without_leaking_payload() {
        // Given: an image transmission still in flight when a restarted GUI attaches, whose
        // payload already overflowed the retention ceiling.
        let hub = TerminalOutputHub::new_with_retention(
            8 * 1024,
            retained_history::RetentionBudget {
                text_bytes: 8 * 1024,
                image_bytes: 8 * 1024,
            },
        );
        hub.register_session("inflight");
        hub.publish("inflight", b"VISIBLE-TEXT\r\n".to_vec());
        hub.publish("inflight", b"\x1b_Ga=T,f=24,s=8,v=8,m=1;".to_vec());
        hub.publish("inflight", vec![b'Q'; 64 * 1024]);

        // When: the GUI attaches with no cursor and then receives the live continuation.
        let attachment = hub
            .subscribe_with_sequence("inflight", None)
            .expect("attachment");
        let history = attachment.snapshot.history;

        // Then: the snapshot keeps the parser inside the string (the introducer survives the
        // overflow), so the live tail is consumed as payload instead of printed as text.
        assert!(history.windows(12).any(|w| w == b"VISIBLE-TEXT"));
        let introducers = history.windows(2).filter(|w| *w == b"\x1b_").count();
        assert_eq!(introducers, 1, "resync introducer missing from snapshot");
        let terminators = history.windows(2).filter(|w| *w == b"\x1b\\").count();
        assert_eq!(terminators, 0, "snapshot must not fabricate a terminator");
        assert!(
            history.iter().filter(|byte| **byte == b'Q').count() < 64 * 1024,
            "overflowing payload must not be retained verbatim"
        );

        // And: once the real terminator arrives, retention resyncs and keeps new text.
        hub.publish("inflight", b"\x1b\\AFTER-IMAGE\r\n".to_vec());
        let resumed = hub
            .subscribe_with_sequence("inflight", None)
            .expect("attachment")
            .snapshot
            .history;
        assert!(resumed.windows(11).any(|w| w == b"AFTER-IMAGE"));
        assert_eq!(resumed.windows(2).filter(|w| *w == b"\x1b_").count(), 0);
    }

    #[test]
    fn transport_claim_fences_live_owner_and_adopts_retained_history() {
        let hub = TerminalOutputHub::new(1024);
        hub.register_session("s");
        hub.publish("s", b"history".to_vec());
        assert!(hub.claim_transport("s"));
        assert!(!hub.claim_transport("s"));
        assert!(hub.transport_owner("s"));
        hub.release_transport("s");
        assert!(!hub.transport_owner("s"));
        assert!(hub.has_session("s"));
        let attachment = hub.subscribe_with_sequence("s", None).unwrap();
        assert!(!attachment.snapshot.history.is_empty());
        assert!(hub.claim_transport("s"));
    }

    #[tokio::test]
    async fn ssh_process_survival_gap_precedes_recovered_bytes_and_keeps_history_behind_seam() {
        let hub = TerminalOutputHub::new(1024);
        hub.register_session("remote");
        hub.publish("remote", b"stale".to_vec());
        let mut live = hub.subscribe_with_sequence("remote", Some(1)).unwrap();
        hub.publish_gap("remote").unwrap();
        let between = hub.subscribe_with_sequence("remote", Some(1)).unwrap();
        assert!(between.snapshot.history.starts_with(b"stale"));
        assert!(between.snapshot.history.windows(9).any(|w| w == b"[Ferryx: "));
        assert!(between.snapshot.history_incomplete);
        assert!(between.snapshot.gap.is_some());
        hub.publish("remote", b"recovered".to_vec());
        let boundary = live.receiver.try_recv().unwrap();
        assert!(boundary.bytes.is_empty());
        let gap = boundary.replay_gap.unwrap();
        assert_eq!(gap.requested_after_sequence, 1);
        let output = live.receiver.try_recv().unwrap();
        assert_eq!(&*output.bytes, b"recovered");
        assert_eq!(gap.available_from_sequence, output.sequence);
        let recovered_state = hub.export_session_state("remote").expect("export recovered gap state");
        assert_eq!(recovered_state.replay_gap, None);
        assert_eq!(recovered_state.gap_floor, gap.available_from_sequence);
        let successor = TerminalOutputHub::new(1024);
        successor.import_session_state("remote", recovered_state).expect("import recovered gap state");
        let after_handover = successor.subscribe_with_sequence("remote", Some(1)).unwrap();
        assert_eq!(after_handover.snapshot.gap.as_ref().map(|g| g.available_from_sequence), Some(gap.available_from_sequence));
        assert!(after_handover.snapshot.history.windows(9).any(|w| w == b"[Ferryx: "));
        let late = hub.subscribe_with_sequence("remote", Some(1)).unwrap();
        assert!(late.snapshot.history.starts_with(b"stale"));
        assert!(late.snapshot.history.ends_with(b"recovered"));
        assert_eq!(late.snapshot.gap, Some(gap));
    }

    #[tokio::test]
    async fn remote_gap_keeps_pre_gap_lines_and_replays_them_before_the_seam() {
        let hub = TerminalOutputHub::new(1024);
        hub.register_session("gap-history");
        for line in 0..5 {
            hub.publish("gap-history", format!("line-{line}\r\n").into_bytes());
        }
        let boundary = hub.publish_gap("gap-history").expect("publish gap");
        let full = hub.subscribe_with_sequence("gap-history", None).expect("full attach");
        assert!(full.snapshot.history.starts_with(b"line-0\r\nline-1\r\nline-2\r\nline-3\r\nline-4\r\n"));
        assert!(full.snapshot.history.windows(9).any(|w| w == b"[Ferryx: "));
        assert!(full.snapshot.history_incomplete);
        assert!(full.snapshot.gap.is_none());

        hub.publish("gap-history", b"after-gap\r\n".to_vec());
        let stale = hub.subscribe_with_sequence("gap-history", Some(4)).expect("stale attach");
        assert!(stale.snapshot.history.starts_with(b"line-0\r\nline-1\r\nline-2\r\nline-3\r\nline-4\r\n"));
        assert!(stale.snapshot.history.windows(9).any(|w| w == b"[Ferryx: "));
        assert!(stale.snapshot.history.ends_with(b"after-gap\r\n"));
        assert_eq!(stale.snapshot.gap.as_ref().map(|gap| gap.available_from_sequence), boundary.replay_gap.as_ref().map(|gap| gap.available_from_sequence));

        let exported = hub.export_session_state("gap-history").expect("export gap state");
        assert_eq!(exported.gap_floor, boundary.sequence + 1);
        let successor = TerminalOutputHub::new(1024);
        successor.import_session_state("gap-history", exported).expect("import gap state");
        let after_handover = successor.subscribe_with_sequence("gap-history", Some(4)).expect("reattach after handover");
        assert!(after_handover.snapshot.history.starts_with(b"line-0\r\nline-1\r\nline-2\r\nline-3\r\nline-4\r\n"));
        assert!(after_handover.snapshot.history.windows(9).any(|w| w == b"[Ferryx: "));
        assert!(after_handover.snapshot.history.ends_with(b"after-gap\r\n"));
        assert_eq!(after_handover.snapshot.gap.as_ref().map(|gap| gap.available_from_sequence), boundary.replay_gap.as_ref().map(|gap| gap.available_from_sequence));

        let recovered = hub.subscribe_with_sequence("gap-history", None).expect("recovered attach");
        assert!(recovered.snapshot.history.ends_with(b"after-gap\r\n"));
        assert!(recovered.snapshot.history_incomplete);
    }

    #[tokio::test]
    async fn test_output_hub_replay_and_broadcast() {
        let hub = TerminalOutputHub::new(1024);
        let session_id = "test-session";
        let _initial_rx = hub.register_session(session_id);

        hub.publish(session_id, b"hello ".to_vec());
        hub.publish(session_id, b"world\n".to_vec());

        let (history, mut rx) = hub.subscribe(session_id).expect("session exists");
        assert_eq!(history, b"hello world\n");

        hub.publish(session_id, b"live chunk".to_vec());
        let received = rx.recv().await.expect("received live message");
        assert_eq!(&received[..], b"live chunk");
    }

    #[tokio::test]
    async fn test_bounded_buffer_overflow() {
        let mut buffer = BoundedBuffer::new(10);
        buffer.push(b"12345".to_vec());
        buffer.push(b"67890".to_vec());
        buffer.push(b"abcdef".to_vec());

        let snap = buffer.snapshot();
        assert!(snap.len() <= 16); // pops chunks until under capacity
        assert!(snap.ends_with(b"abcdef"));
    }

    #[tokio::test]
    async fn test_monotonic_u64_sequences_per_session() {
        let hub = TerminalOutputHub::new(1024);
        let session1 = "session-1";
        let session2 = "session-2";
        let _rx1 = hub.register_session(session1);
        let _rx2 = hub.register_session(session2);

        let c1_1 = hub
            .publish(session1, b"s1_first".to_vec())
            .expect("chunk published");
        let c1_2 = hub
            .publish(session1, b"s1_second".to_vec())
            .expect("chunk published");
        let c2_1 = hub
            .publish(session2, b"s2_first".to_vec())
            .expect("chunk published");
        let c1_3 = hub
            .publish(session1, b"s1_third".to_vec())
            .expect("chunk published");

        assert_eq!(c1_1.sequence, 1);
        assert_eq!(c1_2.sequence, 2);
        assert_eq!(c1_3.sequence, 3);
        assert_eq!(
            c2_1.sequence, 1,
            "session 2 sequence must start at 1 independently"
        );
    }

    #[tokio::test]
    async fn test_bounded_history_sequence_range_and_eviction() {
        let hub = TerminalOutputHub::new(24);
        let session_id = "bounded-session";
        let _rx = hub.register_session(session_id);

        hub.publish(session_id, b"1234567890".to_vec()); // seq 1, len 10
        hub.publish(session_id, b"abcdefghij".to_vec()); // seq 2, len 10 (total 20 <= 24)
        hub.publish(session_id, b"klmnopqrst".to_vec()); // seq 3, len 10 (total 30 > 24 -> evicts seq 1)

        let attachment = hub
            .subscribe_with_sequence(session_id, None)
            .expect("session attachment");

        assert_eq!(attachment.snapshot.history_start_sequence, Some(2));
        assert_eq!(attachment.snapshot.history_end_sequence, Some(3));
        assert_eq!(attachment.snapshot.history, b"abcdefghijklmnopqrst");
        assert_eq!(attachment.snapshot.gap, None);
    }

    #[tokio::test]
    async fn test_replay_after_sequence_partial_and_up_to_date() {
        let hub = TerminalOutputHub::new(1024);
        let session_id = "replay-session";
        let _rx = hub.register_session(session_id);

        hub.publish(session_id, b"one;".to_vec()); // seq 1
        hub.publish(session_id, b"two;".to_vec()); // seq 2
        hub.publish(session_id, b"three;".to_vec()); // seq 3
        hub.publish(session_id, b"four;".to_vec()); // seq 4

        // Replay after seq 2
        let attach_mid = hub
            .subscribe_with_sequence(session_id, Some(2))
            .expect("attach mid");
        assert_eq!(attach_mid.snapshot.history_start_sequence, Some(3));
        assert_eq!(attach_mid.snapshot.history_end_sequence, Some(4));
        assert_eq!(attach_mid.snapshot.history, b"three;four;");
        assert_eq!(attach_mid.snapshot.gap, None);

        // Replay after seq 4 (already up to date)
        let attach_latest = hub
            .subscribe_with_sequence(session_id, Some(4))
            .expect("attach latest");
        assert_eq!(attach_latest.snapshot.history_start_sequence, None);
        assert_eq!(attach_latest.snapshot.history_end_sequence, Some(4));
        assert_eq!(attach_latest.snapshot.history, b"");
        assert_eq!(attach_latest.snapshot.gap, None);
    }

    #[tokio::test]
    async fn test_replay_gap_detection_on_eviction() {
        let hub = TerminalOutputHub::new(20);
        let session_id = "gap-session";
        let _rx = hub.register_session(session_id);

        hub.publish(session_id, b"1234567890".to_vec()); // seq 1
        hub.publish(session_id, b"abcdefghij".to_vec()); // seq 2
        hub.publish(session_id, b"klmnopqrst".to_vec()); // seq 3 (seq 1 evicted)
        hub.publish(session_id, b"uvwxyz1234".to_vec()); // seq 4 (seq 2 evicted)

        // Request after seq 1 (which was evicted)
        let attach_gapped = hub
            .subscribe_with_sequence(session_id, Some(1))
            .expect("attach gapped");

        assert_eq!(
            attach_gapped.snapshot.gap,
            Some(ReplayGap {
                requested_after_sequence: 1,
                available_from_sequence: 3,
            })
        );
        assert_eq!(attach_gapped.snapshot.history_start_sequence, Some(3));
        assert_eq!(attach_gapped.snapshot.history_end_sequence, Some(4));
        assert_eq!(attach_gapped.snapshot.history, b"klmnopqrstuvwxyz1234");

        // Request after seq 2 (next requested is 3, which is available_from_sequence) -> NO GAP
        let attach_boundary = hub
            .subscribe_with_sequence(session_id, Some(2))
            .expect("attach boundary");
        assert_eq!(attach_boundary.snapshot.gap, None);
        assert_eq!(attach_boundary.snapshot.history_start_sequence, Some(3));
        assert_eq!(attach_boundary.snapshot.history_end_sequence, Some(4));
    }

    #[test]
    fn test_replay_after_last_emitted_sequence_is_bounded_unless_evicted() {
        let mut buffer = BoundedBuffer::new(10);
        buffer.push(b"11111".to_vec()); // 1
        buffer.push(b"22222".to_vec()); // 2
        buffer.push(b"33333".to_vec()); // 3, evicts 1

        let (bounded, start, end, gap) = buffer.snapshot_after(Some(2));
        assert_eq!(bounded, b"33333");
        assert_eq!(start, Some(3));
        assert_eq!(end, Some(3));
        assert_eq!(gap, None);

        let (required_full, start, end, gap) = buffer.snapshot_after(Some(0));
        assert_eq!(required_full, b"2222233333");
        assert_eq!(start, Some(2));
        assert_eq!(end, Some(3));
        assert_eq!(
            gap,
            Some(ReplayGap {
                requested_after_sequence: 0,
                available_from_sequence: 2,
            })
        );
    }

    #[tokio::test]
    async fn test_subscriber_first_attachment_ordering_prevents_lost_chunks() {
        let hub = TerminalOutputHub::new(1024);
        let session_id = "ordering-session";
        let _rx = hub.register_session(session_id);

        hub.publish(session_id, b"init-1;".to_vec()); // seq 1
        hub.publish(session_id, b"init-2;".to_vec()); // seq 2

        let mut attachment = hub
            .subscribe_with_sequence(session_id, None)
            .expect("attachment");

        assert_eq!(attachment.snapshot.history_end_sequence, Some(2));
        assert_eq!(attachment.snapshot.history, b"init-1;init-2;");

        // Live publish occurs after attachment
        let live_chunk = hub
            .publish(session_id, b"live-3;".to_vec())
            .expect("published");
        assert_eq!(live_chunk.sequence, 3);

        let received = attachment
            .receiver
            .recv()
            .await
            .expect("subscriber receives live chunk");
        assert_eq!(received.sequence, 3);
        assert_eq!(&received.bytes[..], b"live-3;");
    }

    #[tokio::test]
    async fn test_output_hub_resize_ledger_and_segmented_snapshot() {
        let hub = TerminalOutputHub::new(1024);
        let session_id = "resize-seq-session";
        let _rx = hub.register_session(session_id);

        let c1 = hub.publish(session_id, b"A".to_vec()).expect("publish A");
        assert_eq!(c1.sequence, 1);

        let resize_seq = hub
            .record_resize(session_id, 120, 30)
            .expect("record resize");
        assert_eq!(resize_seq, 2);

        let c2 = hub.publish(session_id, b"B".to_vec()).expect("publish B");
        assert_eq!(c2.sequence, 3);
        assert_eq!(c2.sequence, resize_seq + 1);

        let attachment = hub
            .subscribe_with_sequence(session_id, None)
            .expect("attachment");

        assert_eq!(attachment.snapshot.history, b"AB");
        assert_eq!(
            attachment.snapshot.history_segments,
            vec![
                HistorySegment {
                    cols: None,
                    rows: None,
                    bytes: b"A".to_vec(),
                },
                HistorySegment {
                    cols: Some(120),
                    rows: Some(30),
                    bytes: b"B".to_vec(),
                },
            ]
        );

        let concatenated: Vec<u8> = attachment
            .snapshot
            .history_segments
            .iter()
            .flat_map(|s| s.bytes.clone())
            .collect();
        assert_eq!(concatenated, attachment.snapshot.history);
    }

    #[tokio::test]
    async fn initial_size_survives_resize_ledger_eviction_and_handover() {
        let source = TerminalOutputHub::new(24);
        let session_id = "initial-size-survives-eviction";
        let _rx = source.register_session(session_id);
        source.record_initial_size(session_id, 132, 43);

        for _ in 0..=RESIZE_LEDGER_CAPACITY {
            source.record_resize(session_id, 81, 25);
        }
        assert_eq!(
            source.initial_size(session_id),
            Some((132, 43)),
            "the baseline must not be evicted with the ledger"
        );
        assert_ne!(
            source
                .export_session_state(session_id)
                .and_then(|snapshot| snapshot.resize_ledger.first().map(|point| (point.cols, point.rows))),
            Some((132, 43)),
            "precondition: the ledger head has already moved off the baseline"
        );

        let snapshot = source.export_session_state(session_id).expect("snapshot");
        assert_eq!(snapshot.initial_size, Some((132, 43)));
        let successor = TerminalOutputHub::new(24);
        successor.import_session_state(session_id, snapshot).unwrap();
        assert_eq!(
            successor.initial_size(session_id),
            Some((132, 43)),
            "a successor must inherit the baseline, not the evicted ledger head"
        );
    }

    #[tokio::test]
    async fn a_snapshot_without_initial_size_still_imports() {
        let hub = TerminalOutputHub::new(24);
        let session_id = "legacy-snapshot-without-baseline";
        let _rx = hub.register_session(session_id);
        hub.record_initial_size(session_id, 90, 30);
        let mut snapshot = hub.export_session_state(session_id).expect("snapshot");
        snapshot.initial_size = None;

        let successor = TerminalOutputHub::new(24);
        successor.import_session_state(session_id, snapshot).unwrap();
        assert_eq!(
            successor.initial_size(session_id),
            Some((90, 30)),
            "an older snapshot falls back to its oldest ledger point"
        );
    }

    #[tokio::test]
    async fn test_output_hub_initial_size_and_eviction_segmented_snapshot() {
        let hub = TerminalOutputHub::new(24);
        let session_id = "initial-and-eviction-session";
        let _rx = hub.register_session(session_id);

        hub.record_initial_size(session_id, 80, 24);
        // Repeated initial size call is no-op
        hub.record_initial_size(session_id, 999, 999);

        let _c1 = hub
            .publish(session_id, b"1234567890".to_vec())
            .expect("seq 1");
        let _res1 = hub.record_resize(session_id, 100, 30).expect("seq 2");
        let _c2 = hub
            .publish(session_id, b"abcdefghij".to_vec())
            .expect("seq 3");
        let _res2 = hub.record_resize(session_id, 120, 40).expect("seq 4");
        let _c3 = hub
            .publish(session_id, b"klmnopqrst".to_vec())
            .expect("seq 5");

        // Bounded capacity 24 means seq 1 (10 bytes) is evicted when total exceeds 24 (10 + 10 + 10 = 30 > 24).
        // Surviving chunks are seq 3 ("abcdefghij") and seq 5 ("klmnopqrst").
        let attachment = hub
            .subscribe_with_sequence(session_id, None)
            .expect("attachment");

        assert_eq!(attachment.snapshot.history, b"abcdefghijklmnopqrst");
        assert_eq!(
            attachment.snapshot.history_segments,
            vec![
                HistorySegment {
                    cols: Some(100),
                    rows: Some(30),
                    bytes: b"abcdefghij".to_vec(),
                },
                HistorySegment {
                    cols: Some(120),
                    rows: Some(40),
                    bytes: b"klmnopqrst".to_vec(),
                },
            ]
        );

        let concatenated: Vec<u8> = attachment
            .snapshot
            .history_segments
            .iter()
            .flat_map(|s| s.bytes.clone())
            .collect();
        assert_eq!(concatenated, attachment.snapshot.history);
    }

    #[tokio::test]
    async fn test_output_hub_resize_with_no_output_after_produces_trailing_segment() {
        let hub = TerminalOutputHub::new(1024);
        let session_id = "trailing-resize-session";
        let _rx = hub.register_session(session_id);

        hub.record_initial_size(session_id, 80, 24);
        let _c1 = hub
            .publish(session_id, b"hello".to_vec())
            .expect("publish hello");
        let _res1 = hub.record_resize(session_id, 140, 50).expect("resize");

        let attachment = hub
            .subscribe_with_sequence(session_id, None)
            .expect("attachment");

        assert_eq!(attachment.snapshot.history, b"hello");
        assert_eq!(
            attachment.snapshot.history_segments,
            vec![
                HistorySegment {
                    cols: Some(80),
                    rows: Some(24),
                    bytes: b"hello".to_vec(),
                },
                HistorySegment {
                    cols: Some(140),
                    rows: Some(50),
                    bytes: Vec::new(),
                },
            ]
        );

        let concatenated: Vec<u8> = attachment
            .snapshot
            .history_segments
            .iter()
            .flat_map(|s| s.bytes.clone())
            .collect();
        assert_eq!(concatenated, attachment.snapshot.history);

        // Also test pure resize with zero output chunks
        let session_empty = "empty-resize-session";
        let _rx_empty = hub.register_session(session_empty);
        hub.record_resize(session_empty, 120, 30);
        let empty_attachment = hub
            .subscribe_with_sequence(session_empty, None)
            .expect("empty attachment");
        assert_eq!(empty_attachment.snapshot.history, b"");
        assert_eq!(
            empty_attachment.snapshot.history_segments,
            vec![HistorySegment {
                cols: Some(120),
                rows: Some(30),
                bytes: Vec::new(),
            }]
        );
    }

    #[tokio::test]
    async fn test_output_hub_bracketed_paste_retention_on_eviction() {
        // Create hub with small capacity (64 bytes)
        let hub = TerminalOutputHub::new(64);
        let session_id = "bracketed-eviction-session";
        let _rx = hub.register_session(session_id);

        // 1. Initial output enables bracketed paste
        hub.publish(session_id, b"\x1b[?2004hprompt> ".to_vec())
            .expect("enable 2004h");
        assert!(hub.is_bracketed_paste_enabled(session_id));

        // 2. Flood with output to evict the initial chunk with \x1b[?2004h
        for i in 0..10 {
            hub.publish(session_id, format!("flood line {}\r\n", i).into_bytes())
                .expect("publish flood");
        }

        // Bracketed paste must still be tracked as enabled
        assert!(hub.is_bracketed_paste_enabled(session_id));

        // 3. Snapshot must retain \x1b[?2004h at the beginning even though the chunk was evicted
        let attachment = hub
            .subscribe_with_sequence(session_id, None)
            .expect("attachment");
        assert!(
            attachment.snapshot.history.starts_with(b"\x1b[?2004h"),
            "snapshot history must start with \\x1b[?2004h prefix"
        );
        assert!(
            attachment.snapshot.history_segments[0]
                .bytes
                .starts_with(b"\x1b[?2004h"),
            "first history segment must start with \\x1b[?2004h prefix"
        );

        // 4. Disabling bracketed paste with \x1b[?2004l must clear it
        hub.publish(session_id, b"\x1b[?2004lexiting\r\n".to_vec())
            .expect("disable 2004l");
        assert!(!hub.is_bracketed_paste_enabled(session_id));
    }

    #[test]
    fn seq_broadcast_backlog_is_bounded() {
        // Given: a registered session whose sequence receiver never drains.
        let hub = TerminalOutputHub::new(1024);
        let (_raw_rx, mut seq_rx) = hub.register_session_channels("t-lag");

        // When: 70 chunks are published against the 64-slot sequence ring.
        for _ in 0..70 {
            hub.publish("t-lag", vec![0u8; 64]).expect("chunk published");
        }

        // Then: the receiver lags by exactly 70 - 64 = 6 skipped chunks instead of
        // retaining the whole unbounded backlog. ReplayGap recovery resyncs lagged
        // sequence subscribers from the ring snapshot.
        assert!(matches!(
            seq_rx.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(6))
        ));
    }

    #[test]
    fn raw_channel_keeps_legacy_capacity() {
        // Given: the legacy raw channel has no sequence metadata and no ReplayGap
        // resync, so it must retain the pre-reduction capacity instead of lagging.
        let hub = TerminalOutputHub::new(1024);
        let (mut raw_rx, _seq_rx) = hub.register_session_channels("t-raw");

        // When: 70 chunks are published (below the 1024-slot raw ring).
        for _ in 0..70 {
            hub.publish("t-raw", vec![0u8; 64]).expect("chunk published");
        }

        // Then: the raw receiver reads every chunk with no loss and no lag error.
        for _ in 0..70 {
            let chunk = raw_rx
                .try_recv()
                .expect("raw consumer never lags within legacy capacity");
            assert_eq!(chunk.len(), 64);
        }
    }

    #[test]
    fn publish_without_raw_receivers_keeps_new_subscriber_correct() {
        // Given: a session registered with no raw receiver attached at all.
        let hub = TerminalOutputHub::new(1024);
        let (raw_rx, _seq_rx) = hub.register_session_channels("t-skip");
        drop(raw_rx);

        // When: 2000 chunks are published with zero raw receivers...
        for _ in 0..2000 {
            hub.publish("t-skip", vec![0u8; 64]).expect("chunk published");
        }

        // ...then a raw subscriber attaches and one more chunk is published.
        let (_history, mut late_rx) = hub.subscribe("t-skip").expect("session exists");
        hub.publish("t-skip", b"after-subscribe".to_vec())
            .expect("chunk published");

        // Then: the late subscriber receives the newest chunk - skipping the copy
        // while nobody listened never costs correctness for subscribed receivers.
        let received = late_rx
            .try_recv()
            .expect("new subscriber receives the newest chunk");
        assert_eq!(&received[..], b"after-subscribe");
    }

    #[tokio::test]
    async fn test_output_hub_state_export_import_roundtrip() {
        let hub1 = TerminalOutputHub::new(1024);
        let session_id = "test-session-handover";
        let _rx = hub1.register_session(session_id);

        // 1. Initial size ledger entry at sequence 0
        hub1.record_initial_size(session_id, 80, 24);

        // 2. Publish gap so SessionHub replay_gap is populated
        let gap_chunk = hub1.publish_gap(session_id).expect("publish gap");
        assert_eq!(gap_chunk.sequence, 1);

        // 3. Bracketed paste mode set + chunks
        let c1 = hub1
            .publish(session_id, b"\x1b[?2004hfirst chunk ".to_vec())
            .expect("publish c1");
        assert_eq!(c1.sequence, 2);

        let c2 = hub1
            .publish(session_id, b"second chunk\n".to_vec())
            .expect("publish c2");
        assert_eq!(c2.sequence, 3);
        assert!(hub1.is_bracketed_paste_enabled(session_id));

        // 4. ResizePoint AFTER the last chunk
        let resize_seq = hub1
            .record_resize(session_id, 120, 40)
            .expect("record resize");
        assert_eq!(resize_seq, 4);
        assert!(resize_seq > c2.sequence);

        // Claim transport
        assert!(hub1.claim_transport(session_id));
        assert!(hub1.transport_owner(session_id));

        // Predecessor next_sequence at freeze
        let pred_next_seq = hub1
            .session_next_sequence(session_id)
            .expect("pred next seq");
        assert_eq!(pred_next_seq, 5);

        // Subscribe on predecessor before export to capture baseline attachment
        let pred_attachment = hub1
            .subscribe_with_sequence(session_id, None)
            .expect("pred attachment");
        assert_eq!(pred_attachment.snapshot.gap, None);
        assert_eq!(pred_attachment.snapshot.history_start_sequence, Some(1));
        assert_eq!(pred_attachment.snapshot.history_end_sequence, Some(3));
        assert_eq!(
            pred_attachment.snapshot.history,
            b"\r\n\x1b[0m\x1b[33m[Ferryx: output stream gap - some terminal output was missed]\x1b[0m\r\n\x1b[?2004hfirst chunk second chunk\n"
        );
        assert_eq!(pred_attachment.snapshot.history_segments.len(), 2);
        // Last segment is trailing empty segment from resize at seq 4
        assert_eq!(
            pred_attachment.snapshot.history_segments.last().unwrap().cols,
            Some(120)
        );
        assert_eq!(
            pred_attachment.snapshot.history_segments.last().unwrap().rows,
            Some(40)
        );
        assert!(pred_attachment
            .snapshot
            .history_segments
            .last()
            .unwrap()
            .bytes
            .is_empty());

        // Export state from hub1
        let snapshot = hub1
            .export_session_state(session_id)
            .expect("export session state");
        assert_eq!(snapshot.next_sequence, 5);
        assert!(snapshot.bracketed_paste_enabled);
        assert_eq!(snapshot.replay_gap, None);
        assert_eq!(snapshot.gap_floor, 2);
        assert!(snapshot.transport_owner);
        assert_eq!(snapshot.chunks.len(), 3);
        assert_eq!(snapshot.resize_ledger.len(), 2);

        // Serde roundtrip
        let serialized = serde_json::to_string(&snapshot).expect("serialize snapshot");
        let deserialized: SessionHubSnapshot =
            serde_json::from_str(&serialized).expect("deserialize snapshot");
        assert_eq!(snapshot, deserialized);

        // Import into fresh successor hub
        let hub2 = TerminalOutputHub::new(1024);
        hub2.import_session_state(session_id, deserialized)
            .expect("import session state");

        // Successor next_sequence equals predecessor next_sequence at freeze
        let succ_next_seq = hub2
            .session_next_sequence(session_id)
            .expect("succ next seq");
        assert_eq!(succ_next_seq, pred_next_seq);
        assert!(hub2.is_bracketed_paste_enabled(session_id));
        assert!(hub2.transport_owner(session_id));

        // subscribe_with_sequence(None) after import returns IDENTICAL history and snapshot
        let mut succ_attachment = hub2
            .subscribe_with_sequence(session_id, None)
            .expect("succ attachment");
        assert_eq!(
            succ_attachment.snapshot.history,
            pred_attachment.snapshot.history
        );
        assert_eq!(
            succ_attachment.snapshot.history_segments,
            pred_attachment.snapshot.history_segments
        );
        assert_eq!(
            succ_attachment.snapshot.history_start_sequence,
            pred_attachment.snapshot.history_start_sequence
        );
        assert_eq!(
            succ_attachment.snapshot.history_end_sequence,
            pred_attachment.snapshot.history_end_sequence
        );
        assert_eq!(succ_attachment.snapshot.gap, pred_attachment.snapshot.gap);
        assert_eq!(succ_attachment.snapshot, pred_attachment.snapshot);

        // First new publish on successor allocates EXACTLY next_sequence
        let new_chunk = hub2
            .publish(session_id, b"successor first publish\n".to_vec())
            .expect("publish on successor");
        assert_eq!(new_chunk.sequence, pred_next_seq);

        // Fresh process-local broadcast senders deliver to the successor subscriber
        let received = succ_attachment
            .receiver
            .recv()
            .await
            .expect("live chunk received on successor receiver");
        assert_eq!(received.sequence, pred_next_seq);
        assert_eq!(&*received.bytes, b"successor first publish\n");

        // Next sequence has advanced to pred_next_seq + 1
        assert_eq!(
            hub2.session_next_sequence(session_id),
            Some(pred_next_seq + 1)
        );
    }

    #[test]
    fn test_session_hub_direct_export_state() {
        let hub = TerminalOutputHub::new(512);
        hub.register_session("s_direct");
        hub.publish("s_direct", b"abc".to_vec());
        hub.record_resize("s_direct", 80, 25);

        let session_hub = {
            let sessions = hub.sessions.read();
            sessions.get("s_direct").cloned().unwrap()
        };
        let hub_guard = session_hub.read();
        let snapshot = hub_guard.export_state();
        assert_eq!(snapshot.chunks.len(), 1);
        assert_eq!(snapshot.chunks[0].sequence, 1);
        assert_eq!(snapshot.resize_ledger.len(), 1);
        assert_eq!(snapshot.resize_ledger[0].sequence, 2);
        assert_eq!(snapshot.next_sequence, 3);
        assert!(!snapshot.transport_owner);
        assert_eq!(hub_guard.buffer().next_sequence(), 3);
        assert_eq!(hub_guard.resize_ledger().len(), 1);
        assert!(hub_guard.replay_gap().is_none());
    }

    #[tokio::test]
    async fn test_output_hub_evicted_bracketed_paste_export_import_roundtrip() {
        let hub1 = TerminalOutputHub::new(64);
        let session_id = "bracketed-evict-handover";
        let _rx = hub1.register_session(session_id);

        hub1.publish(session_id, b"\x1b[?2004hprefix> ".to_vec())
            .expect("enable 2004h");
        assert!(hub1.is_bracketed_paste_enabled(session_id));

        // Flood to evict the initial chunk
        for i in 0..10 {
            hub1.publish(session_id, format!("flood {}\r\n", i).into_bytes())
                .expect("publish flood");
        }
        assert!(hub1.is_bracketed_paste_enabled(session_id));

        let pred_attach = hub1
            .subscribe_with_sequence(session_id, None)
            .expect("pred attach");
        assert!(pred_attach.snapshot.history.starts_with(b"\x1b[?2004h"));

        let snapshot = hub1
            .export_session_state(session_id)
            .expect("export state");
        assert!(snapshot.bracketed_paste_enabled);

        let serialized = serde_json::to_string(&snapshot).expect("serialize");
        let deserialized: SessionHubSnapshot =
            serde_json::from_str(&serialized).expect("deserialize");

        let hub2 = TerminalOutputHub::new(64);
        hub2.import_session_state(session_id, deserialized)
            .expect("import state");

        assert!(hub2.is_bracketed_paste_enabled(session_id));
        let succ_attach = hub2
            .subscribe_with_sequence(session_id, None)
            .expect("succ attach");
        assert_eq!(succ_attach.snapshot.history, pred_attach.snapshot.history);
        assert_eq!(
            succ_attach.snapshot.history_segments,
            pred_attach.snapshot.history_segments
        );
    }

    #[test]
    fn test_output_hub_import_validation_failures() {
        let valid_chunk = OutputChunk {
            sequence: 1,
            bytes: Arc::from(b"data".as_slice()),
            metrics_read_unix_micros: None,
            replay_gap: None,
        };

        // 1. Invalid next_sequence == 0
        let snap_zero_next = SessionHubSnapshot {
          capacity: 512,
          chunks: Vec::new(),
          next_sequence: 0,
          bracketed_paste_enabled: false,
          resize_ledger: Vec::new(),
          initial_size: None,
          replay_gap: None,
          gap_floor: 0,
          transport_owner: false,
          retained_state: None,
          screen_seed: None,
          history_incomplete: false,
        };
        assert_eq!(
            snap_zero_next.validate(),
            Err(OutputHubSnapshotError::InvalidNextSequence(0))
        );

        // 2. Chunk sequence == 0
        let snap_zero_chunk_seq = SessionHubSnapshot {
          capacity: 512,
          chunks: vec![OutputChunk {
              sequence: 0,
              bytes: Arc::from(b"".as_slice()),
              metrics_read_unix_micros: None,
              replay_gap: None,
          }],
          next_sequence: 5,
          bracketed_paste_enabled: false,
          resize_ledger: Vec::new(),
          initial_size: None,
          replay_gap: None,
          gap_floor: 0,
          transport_owner: false,
          retained_state: None,
          screen_seed: None,
          history_incomplete: false,
        };
        assert_eq!(
            snap_zero_chunk_seq.validate(),
            Err(OutputHubSnapshotError::InvalidChunkSequence(0))
        );

        // 3. Chunk sequence >= next_sequence
        let snap_chunk_seq_exceeds = SessionHubSnapshot {
          capacity: 512,
          chunks: vec![valid_chunk.clone(), OutputChunk {
              sequence: 5,
              bytes: Arc::from(b"exceed".as_slice()),
              metrics_read_unix_micros: None,
              replay_gap: None,
          }],
          next_sequence: 5,
          bracketed_paste_enabled: false,
          resize_ledger: Vec::new(),
          initial_size: None,
          replay_gap: None,
          gap_floor: 0,
          transport_owner: false,
          retained_state: None,
          screen_seed: None,
          history_incomplete: false,
        };
        assert_eq!(
            snap_chunk_seq_exceeds.validate(),
            Err(OutputHubSnapshotError::ChunkSequenceExceedsNextSequence(5, 5))
        );

        // 4. Non-monotonic chunk sequences
        let snap_non_monotonic = SessionHubSnapshot {
          capacity: 512,
          chunks: vec![
              OutputChunk {
                  sequence: 3,
                  bytes: Arc::from(b"a".as_slice()),
                  metrics_read_unix_micros: None,
                  replay_gap: None,
              },
              OutputChunk {
                  sequence: 2,
                  bytes: Arc::from(b"b".as_slice()),
                  metrics_read_unix_micros: None,
                  replay_gap: None,
              },
          ],
          next_sequence: 10,
          bracketed_paste_enabled: false,
          resize_ledger: Vec::new(),
          initial_size: None,
          replay_gap: None,
          gap_floor: 0,
          transport_owner: false,
          retained_state: None,
          screen_seed: None,
          history_incomplete: false,
        };
        assert_eq!(
            snap_non_monotonic.validate(),
            Err(OutputHubSnapshotError::ChunkSequenceNotMonotonic(2, 3))
        );

        // 5. ResizePoint sequence >= next_sequence
        let snap_resize_exceeds = SessionHubSnapshot {
          capacity: 512,
          chunks: vec![valid_chunk.clone()],
          next_sequence: 5,
          bracketed_paste_enabled: false,
          resize_ledger: vec![ResizePoint {
              sequence: 5,
              cols: 80,
              rows: 24,
          }],
          initial_size: None,
          replay_gap: None,
          gap_floor: 0,
          transport_owner: false,
          retained_state: None,
          screen_seed: None,
          history_incomplete: false,
        };
        assert_eq!(
            snap_resize_exceeds.validate(),
            Err(OutputHubSnapshotError::ResizeSequenceExceedsNextSequence(5, 5))
        );

        // 6. Resize ledger not ordered
        let snap_resize_unordered = SessionHubSnapshot {
          capacity: 512,
          chunks: vec![valid_chunk.clone()],
          next_sequence: 10,
          bracketed_paste_enabled: false,
          resize_ledger: vec![
              ResizePoint {
                  sequence: 4,
                  cols: 80,
                  rows: 24,
              },
              ResizePoint {
                  sequence: 2,
                  cols: 100,
                  rows: 30,
              },
          ],
          initial_size: None,
          replay_gap: None,
          gap_floor: 0,
          transport_owner: false,
          retained_state: None,
          screen_seed: None,
          history_incomplete: false,
        };
        assert_eq!(
            snap_resize_unordered.validate(),
            Err(OutputHubSnapshotError::ResizeLedgerNotOrdered(2, 4))
        );

        // 7. Retained bytes exceed capacity
        let snap_bytes_exceed = SessionHubSnapshot {
          capacity: 5,
          chunks: vec![OutputChunk {
              sequence: 1,
              bytes: Arc::from(b"123456".as_slice()),
              metrics_read_unix_micros: None,
              replay_gap: None,
          }],
          next_sequence: 2,
          bracketed_paste_enabled: false,
          resize_ledger: Vec::new(),
          initial_size: None,
          replay_gap: None,
          gap_floor: 0,
          transport_owner: false,
          retained_state: None,
          screen_seed: None,
          history_incomplete: false,
        };
        assert_eq!(
            snap_bytes_exceed.validate(),
            Err(OutputHubSnapshotError::RetainedBytesExceedCapacity {
                actual: 6,
                capacity: 5,
            })
        );

        // 8. Invalid replay gap range (requested >= available)
        let snap_invalid_gap = SessionHubSnapshot {
          capacity: 512,
          chunks: Vec::new(),
          next_sequence: 10,
          bracketed_paste_enabled: false,
          resize_ledger: Vec::new(),
          initial_size: None,
          replay_gap: Some(ReplayGap {
              requested_after_sequence: 5,
              available_from_sequence: 5,
          }),
          gap_floor: 0,
          transport_owner: false,
          retained_state: None,
          screen_seed: None,
          history_incomplete: false,
        };
        assert_eq!(
            snap_invalid_gap.validate(),
            Err(OutputHubSnapshotError::InvalidReplayGapRange {
                requested: 5,
                available: 5,
            })
        );

        // 9. Replay gap available > next_sequence
        let snap_gap_future = SessionHubSnapshot {
          capacity: 512,
          chunks: Vec::new(),
          next_sequence: 10,
          bracketed_paste_enabled: false,
          resize_ledger: Vec::new(),
          initial_size: None,
          replay_gap: Some(ReplayGap {
              requested_after_sequence: 8,
              available_from_sequence: 11,
          }),
          gap_floor: 0,
          transport_owner: false,
          retained_state: None,
          screen_seed: None,
          history_incomplete: false,
        };
        assert_eq!(
            snap_gap_future.validate(),
            Err(OutputHubSnapshotError::ReplayGapExceedsNextSequence(11))
        );

        let snap_gap_floor_future = SessionHubSnapshot {
            capacity: 512,
            chunks: Vec::new(),
            next_sequence: 10,
            bracketed_paste_enabled: false,
            resize_ledger: Vec::new(),
            initial_size: None,
            replay_gap: None,
            gap_floor: 11,
            transport_owner: false,
            retained_state: None,
            screen_seed: None,
            history_incomplete: false,
        };
        assert_eq!(
            snap_gap_floor_future.validate(),
            Err(OutputHubSnapshotError::ReplayGapExceedsNextSequence(11))
        );
    }

    #[test]
    fn ended_machine_attachment_keeps_snapshot_and_releases_hub() {
        let hub = TerminalOutputHub::new(1024);
        hub.register_session("ended-machine");
        hub.publish("ended-machine", b"final output".to_vec());
        assert!(!hub.end_session("ended-machine"));
        let attachment = hub.subscribe_machine("ended-machine", None).unwrap().unwrap();
        assert_eq!(attachment.snapshot.value.history, b"final output");
        assert!(hub.session_sequence_range("ended-machine").is_none());
    }

    #[test]
    fn test_snapshot_after_ranges_emits_gap_when_requested_exceeds_last() {
        let mut buffer = BoundedBuffer::new(1024);
        buffer.push_with_read_timestamp(b"hello".to_vec(), None);
        buffer.push_with_read_timestamp(b"world".to_vec(), None);
        assert_eq!(buffer.end_sequence(), Some(2));

        let (history, _ranges, _first, _last, gap) = buffer.snapshot_after_ranges(Some(500), &[]);
        assert!(gap.is_some(), "expected ReplayGap when requested sequence exceeds end sequence");
        let gap = gap.unwrap();
        assert_eq!(gap.requested_after_sequence, 500);
        assert_eq!(gap.available_from_sequence, 1);
        assert_eq!(history, b"helloworld");
    }

    #[test]
    fn test_snapshot_after_ranges_emits_gap_when_all_chunks_evicted() {
        let mut buffer = BoundedBuffer::new(10);
        buffer.push_with_read_timestamp(vec![b'x'; 20], None);
        assert!(buffer.chunks().is_empty());
        assert!(buffer.next_sequence() > 1);

        let (_history, _ranges, _first, _last, gap) = buffer.snapshot_after_ranges(Some(0), &[]);
        assert!(gap.is_some(), "expected ReplayGap when all chunks have been evicted");
        let gap = gap.unwrap();
        assert_eq!(gap.requested_after_sequence, 0);
        assert_eq!(gap.available_from_sequence, buffer.next_sequence());
    }

    #[test]
    fn test_snapshot_after_ranges_retained_avoids_spurious_gap_when_in_retained_range() {
        let mut buffer = BoundedBuffer::with_retention(
            10,
            retained_history::RetentionBudget {
                text_bytes: 1024,
                image_bytes: 1024,
            },
        );
        for seq in 1..=10u64 {
            buffer.push_with_read_timestamp(format!("d{seq:02}").into_bytes(), None);
        }
        let ring_start = buffer.start_sequence().expect("ring start");
        let retained_start = buffer.retained_start_sequence().expect("retained start");
        assert!(retained_start < ring_start, "retained start ({retained_start}) should be older than ring start ({ring_start})");

        let (history, _ranges, _first, _last, gap) = buffer.snapshot_after_ranges(Some(2), &[]);
        assert_eq!(gap, None, "spurious ReplayGap emitted even though retained history covers the requested range");
        assert!(!history.is_empty(), "history should not be empty");
    }

    #[test]
    fn a_recovered_midstream_history_is_flagged_without_moving_the_sequence_axis() {
        // A recovery that read only the tail of a longer log begins mid-stream. An attach must be
        // able to say so, but the hub's own sequence numbers are the axis a checkpoint offset is
        // derived from, so the flag is carried on the floor and the incomplete bit -- never as
        // seam bytes, which would shift every later hub byte off the keeper's offsets.
        let hub = TerminalOutputHub::default();
        hub.register_session("recovered");
        assert!(hub.mark_history_gap("recovered"));

        let attachment = hub
            .subscribe_with_sequence("recovered", Some(0))
            .expect("attachment");
        assert!(
            attachment.snapshot.gap.is_some(),
            "an attach below the recovered floor is told the history begins mid-stream"
        );
        assert!(
            !attachment.snapshot.history_incomplete || attachment.snapshot.gap.is_some(),
            "a reader is never told the history is whole while a prefix is missing"
        );

        // The axis is untouched: the first real byte is still sequence 1, which is what a keeper
        // offset of `head - tail` maps onto. Seam bytes would have consumed a sequence number.
        let chunk = hub.publish("recovered", b"tail".to_vec()).expect("published");
        assert_eq!(chunk.sequence, 1, "marking the gap must not consume a sequence number");

        // A session the hub never saw cannot be marked: there is no floor to set.
        assert!(!hub.mark_history_gap("never-seen"));
    }

    #[test]
    fn test_publish_clears_replay_gap_and_does_not_pollute_none_subscribers() {
        let hub = TerminalOutputHub::new(1024);
        hub.register_session("gap-clear-test");

        hub.publish_gap("gap-clear-test");
        {
            let sessions = hub.sessions.read();
            let session_hub = sessions.get("gap-clear-test").unwrap().read();
            assert!(session_hub.replay_gap.is_some(), "replay_gap should be set after publish_gap");
        }

        for i in 0..5 {
            hub.publish("gap-clear-test", format!("chunk-{i}\n").into_bytes());
        }

        {
            let sessions = hub.sessions.read();
            let session_hub = sessions.get("gap-clear-test").unwrap().read();
            assert_eq!(session_hub.replay_gap, None, "hub.replay_gap must be cleared after publishing new output");
        }

        let (attachment, _ranges) = hub
            .subscribe_with_sequence_ranges("gap-clear-test", None)
            .expect("attachment");
        assert_eq!(
            attachment.snapshot.gap, None,
            "fresh subscriber with after_sequence: None must not receive stale replay gap"
        );
    }

    #[test]
    fn test_output_hub_cumulative_published_bytes_and_eviction_gap() {
        let hub = TerminalOutputHub::new(64);
        let session_id = "test_cumul_session";
        let _rx = hub.register_session(session_id);

        assert_eq!(hub.session_published_bytes(session_id), Some(0));

        // Publish 40 bytes (fits in 64-byte ring)
        let c1 = hub.publish(session_id, vec![b'a'; 40]).expect("chunk 1");
        assert_eq!(c1.bytes.len(), 40);
        assert_eq!(hub.session_published_bytes(session_id), Some(40));
        assert_eq!(hub.session_sequence_range(session_id), Some((Some(1), Some(1))));

        // Publish another 40 bytes (total 80 bytes -> causes eviction of c1 from 64-byte ring)
        let c2 = hub.publish(session_id, vec![b'b'; 40]).expect("chunk 2");
        assert_eq!(c2.bytes.len(), 40);
        assert_eq!(hub.session_published_bytes(session_id), Some(80));
        // Sequence 1 was evicted from the ring
        assert_eq!(hub.session_sequence_range(session_id), Some((Some(2), Some(2))));

        // Publish gap: gap chunk has 0 bytes, sequence advances, published_bytes unchanged
        let gap = hub.publish_gap(session_id).expect("gap chunk");
        assert_eq!(gap.bytes.len(), 0);
        assert!(gap.replay_gap.is_some());
        assert_eq!(hub.session_published_bytes(session_id), Some(80));

        // Import does not carry published_bytes across handover (document not handover lifetime)
        let snap = hub.export_session_state(session_id).expect("snapshot");
        let hub2 = TerminalOutputHub::new(64);
        hub2.import_session_state("imported_session", snap).expect("import");
        assert_eq!(hub2.session_published_bytes("imported_session"), Some(0));
        hub2.publish("imported_session", vec![b'c'; 15]).expect("publish after import");
        assert_eq!(hub2.session_published_bytes("imported_session"), Some(15));
    }

    #[tokio::test]
    async fn screen_compression_driver_stops_when_the_hub_is_dropped() {
        let hub = TerminalOutputHub::new(4096);
        // Registered off the runtime, so registration spawns no driver: the one below is the
        // only one.
        let registering = hub.clone();
        let _output = std::thread::spawn(move || registering.register_session("compressed"))
            .join()
            .expect("register off the runtime");
        let session = hub.sessions.read().get("compressed").cloned().expect("session hub");
        // No output yet, so nothing is scheduled and the driver waits on the activity signal,
        // which only the mirror's drop can wake.
        let scheduled = session
            .read()
            .screen
            .as_ref()
            .expect("screen mirror")
            .lock()
            .compression_wakeup(std::time::Instant::now());
        assert_eq!(scheduled, None);
        let driver = spawn_screen_compression(&session).expect("driver on a tokio runtime");
        tokio::task::yield_now().await;
        assert!(!driver.is_finished(), "the driver lives while the hub does");

        hub.remove_session("compressed");
        drop(session);
        tokio::time::timeout(std::time::Duration::from_secs(10), driver)
            .await
            .expect("the driver must exit once the hub is gone")
            .expect("driver task");
    }

    // ---- Scrollback budget: deterministic pressure, no clock involved ----

    /// A hub whose budget is small enough to be exhausted by a few writes, so the tests assert
    /// on the eviction counter instead of waiting for anything.
    fn budgeted_hub(budget_bytes: usize, min_session_bytes: usize) -> TerminalOutputHub {
        TerminalOutputHub::new_with_budget(
            64 * 1024,
            retained_history::RetentionBudget::from_total(budget_bytes),
            ScrollbackBudget::new_with_floor(budget_bytes, min_session_bytes),
        )
    }

    /// Publishes 64 KiB chunks until the session's retained history reaches its ceiling.
    fn fill_to_ceiling(hub: &TerminalOutputHub, session_id: &str) {
        let ceiling = hub.session_ceiling_bytes();
        let payload = vec![b'h'; 64 * 1024];
        let mut written = 0;
        while written + payload.len() <= ceiling {
            hub.publish(session_id, payload.clone());
            written += payload.len();
        }
    }

    #[test]
    fn a_session_beyond_the_admission_limit_is_refused_with_the_typed_error() {
        // 1 MiB budget / 256 KiB floor = 4 sessions, and the fifth is refused by name.
        let hub = budgeted_hub(1024 * 1024, 256 * 1024);
        for index in 0..4 {
            hub.admit_session(&format!("s{index}")).expect("admit within limit");
        }
        let error = hub.admit_session("s4").expect_err("session 5 is refused");
        assert!(
            matches!(error, crate::terminal::PtyError::SessionCapacityExhausted { limit: 4, .. }),
            "expected a typed capacity refusal, got: {error:?}"
        );
        // The refusal names the limit in its message, so a log reader never has to guess.
        assert!(error.to_string().contains("SESSION_CAPACITY_EXHAUSTED"));
        assert!(error.to_string().contains("4 concurrent sessions"));
        // Nothing was admitted, and nothing was silently given zero history.
        assert_eq!(hub.session_ceiling_bytes(), 256 * 1024);
    }

    #[test]
    fn admission_pressure_emits_scrollback_budget_exhausted_and_trims_inactive_first() {
        // Two sessions share 4 MiB (2 MiB each); a third lowers both to 1.33 MiB.
        let hub = budgeted_hub(4 * 1024 * 1024, 512 * 1024);
        hub.admit_session("inactive").expect("admit");
        hub.register_session("inactive");
        hub.admit_session("active").expect("admit");
        hub.register_session("active");
        assert_eq!(hub.session_ceiling_bytes(), 2 * 1024 * 1024);

        hub.note_session_focus("inactive");
        hub.note_session_focus("active");
        hub.note_session_blur("inactive");

        fill_to_ceiling(&hub, "inactive");
        fill_to_ceiling(&hub, "active");
        assert_eq!(hub.eviction_count(), 0, "nothing is over budget yet");

        let mut events = hub.subscribe_budget_events();
        let before = hub.eviction_count();
        hub.admit_session("focused").expect("admit");
        hub.register_session("focused");
        hub.note_session_focus("focused");

        assert!(hub.eviction_count() > before, "the lowered ceiling must force trims");
        // The inactive pane is the first candidate; the focused pane is never first.
        let first = events.try_recv().expect("scrollback_budget_exhausted event");
        assert_eq!(first.session_id, "inactive");
        assert_eq!(first.ceiling_bytes, 4 * 1024 * 1024 / 3);
        assert!(first.released_bytes > 0);
        assert!(first.eviction_count >= 1);
        assert_ne!(first.session_id, "focused", "the focused pane is evicted last");
    }

    #[test]
    fn a_sole_focused_session_is_trimmed_to_its_own_ceiling_not_left_unbounded() {
        // One session alone owns the whole budget; its retained history still stops there.
        let hub = budgeted_hub(1024 * 1024, 256 * 1024);
        hub.admit_session("only").expect("admit");
        hub.register_session("only");
        hub.note_session_focus("only");
        assert_eq!(hub.session_ceiling_bytes(), 1024 * 1024);

        // The focused pane writes far past every bound it could hope for. Its own ceiling is what
        // stops it, and the recording is what proves it: no unbounded path is left behind.
        for _ in 0..40 {
            hub.publish("only", vec![b'o'; 64 * 1024]);
        }
        assert!(
            hub.retained_bytes("only") <= 1024 * 1024,
            "retained {} exceeds the focused pane's own ceiling",
            hub.retained_bytes("only")
        );
        assert_eq!(hub.session_ceiling_bytes(), 1024 * 1024);
    }

    #[test]
    fn floor_exhaustion_keeps_the_next_write_at_the_focused_panes_own_ceiling() {
        // 3 sessions x 1 MiB share = the whole budget, so every pane's ceiling is the floor.
        let hub = budgeted_hub(3 * 1024 * 1024, 1024 * 1024);
        for session in ["a", "b", "c"] {
            hub.admit_session(session).expect("admit");
            hub.register_session(session);
        }
        let ceiling = hub.session_ceiling_bytes();
        assert_eq!(ceiling, 1024 * 1024);
        hub.note_session_focus("c");

        for session in ["a", "b"] {
            fill_to_ceiling(&hub, session);
        }
        assert_eq!(hub.eviction_count(), 0, "at the floor nothing needs to go yet");

        // The focused pane writes past every bound it could hope for and is still held to its own
        // ceiling: the floor is what stops it, for the focused pane exactly as for the others.
        for _ in 0..40 {
            hub.publish("c", vec![b'c'; 64 * 1024]);
        }
        assert!(hub.retained_bytes("c") <= ceiling);
        assert!(hub.retained_bytes("a") <= ceiling);
        assert!(hub.retained_bytes("b") <= ceiling);
    }

    #[test]
    fn lowering_a_session_to_the_floor_evicts_and_is_counted() {
        // Three panes, each filled to the 2 MiB share the ceiling gives them. A fourth lowers
        // every share to 1.5 MiB, and the trims that forces are what the counter reports.
        let hub = budgeted_hub(6 * 1024 * 1024, 1024 * 1024);
        for session in ["a", "b", "c"] {
            hub.admit_session(session).expect("admit");
            hub.register_session(session);
        }
        assert_eq!(hub.session_ceiling_bytes(), 6 * 1024 * 1024 / 3);
        for session in ["a", "b", "c"] {
            fill_to_ceiling(&hub, session);
        }
        assert_eq!(hub.eviction_count(), 0, "nothing is over its share yet");

        let mut events = hub.subscribe_budget_events();
        hub.admit_session("d").expect("admit");
        hub.register_session("d");

        assert_eq!(hub.session_ceiling_bytes(), 6 * 1024 * 1024 / 4);
        assert_eq!(hub.eviction_count(), 3, "every survivor above the new share is trimmed");
        let first = events.try_recv().expect("scrollback_budget_exhausted event");
        assert_eq!(first.session_id, "a", "the least recently focused survivor gives first");
        assert!(first.released_bytes > 0);
        for session in ["a", "b", "c"] {
            assert!(hub.retained_bytes(session) <= hub.session_ceiling_bytes());
        }
    }

    #[test]
    fn removing_a_session_raises_every_survivors_ceiling_again() {
        let hub = budgeted_hub(2 * 1024 * 1024, 512 * 1024);
        hub.admit_session("a").expect("admit");
        hub.admit_session("b").expect("admit");
        assert_eq!(hub.session_ceiling_bytes(), 1024 * 1024);
        hub.release_budget_session("b");
        assert_eq!(hub.session_ceiling_bytes(), 2 * 1024 * 1024);
    }

    // ---- Production budget and durable admission ----

    #[test]
    fn the_daemon_budget_pair_sets_the_ceiling_and_the_admission_limit() {
        // 1 MiB across sessions with a 256 KiB floor: four sessions, the fifth is refused.
        let hub = TerminalOutputHub::for_daemon_with_budget(1024 * 1024, 256 * 1024);
        assert_eq!(hub.budget_bytes(), Some(1024 * 1024));
        assert_eq!(hub.min_session_bytes(), Some(256 * 1024));
        assert_eq!(
            hub.admission_ceiling(),
            Some(crate::terminal::admission_store::AdmissionCeiling {
                limit: 4,
                budget_bytes: 1024 * 1024,
                min_session_bytes: 256 * 1024,
            })
        );

        for index in 0..4 {
            hub.admit_session(&format!("s{index}")).expect("admit within limit");
        }
        assert_eq!(hub.session_ceiling_bytes(), 256 * 1024);
        let error = hub.admit_session("s4").expect_err("the fifth is refused");
        assert!(
            matches!(
                error,
                crate::terminal::PtyError::SessionCapacityExhausted {
                    limit: 4,
                    // A pattern takes the literal pair, not the `1024 * 1024` expression.
                    budget_bytes: 1_048_576,
                    min_session_bytes: 262_144,
                }
            ),
            "the refusal carries the configured pair, not the compiled-in one: {error:?}"
        );
    }

    #[test]
    fn an_unusable_budget_pair_falls_back_to_the_compiled_in_default() {
        // A zero budget, a floor at libghostty's own default, and a floor above the budget are
        // all unusable; none may produce a hub that prunes or refuses everything.
        let defaults = (
            crate::terminal::preferences::DEFAULT_SCROLLBACK_BUDGET_BYTES,
            crate::terminal::preferences::MIN_SESSION_HISTORY_BYTES,
        );
        for (budget, min) in [
            (0usize, 256 * 1024usize),
            (1024 * 1024, 0),
            (1024 * 1024, crate::terminal::preferences::LIBGHOSTTY_DEFAULT_SCROLLBACK_BYTES),
            (1024 * 1024, 1024 * 1024 + 1),
        ] {
            let hub = TerminalOutputHub::for_daemon_with_budget(budget, min);
            assert_eq!(
                (hub.budget_bytes(), hub.min_session_bytes()),
                (Some(defaults.0), Some(defaults.1)),
                "({budget}, {min}) must fall back to the defaults"
            );
        }
    }

    #[test]
    fn a_budget_pair_at_its_own_floor_is_usable_and_admits_one_session() {
        // `min == budget` is not a typo the fallback covers: it is a deployment that wants exactly
        // one full-budget pane. Only a floor *above* the budget is unusable.
        let hub = TerminalOutputHub::for_daemon_with_budget(1024 * 1024, 1024 * 1024);
        assert_eq!(hub.budget_bytes(), Some(1024 * 1024));
        assert_eq!(hub.min_session_bytes(), Some(1024 * 1024));
        assert_eq!(hub.admission_ceiling().map(|ceiling| ceiling.limit), Some(1));
    }

    #[test]
    fn the_daemon_hub_agrees_with_the_configured_pair() {
        // No environment mutation: this asserts the two agree for whatever is configured here.
        let (budget, min) = crate::terminal::preferences::configured_scrollback_budget();
        let hub = TerminalOutputHub::for_daemon();
        assert_eq!(hub.budget_bytes(), Some(budget));
        assert_eq!(hub.min_session_bytes(), Some(min));
        assert_eq!(
            hub.admission_ceiling().map(|ceiling| ceiling.limit),
            Some((budget / min.max(1)).max(1)),
            "the admission limit must follow the configured budget"
        );
    }

    #[test]
    fn a_hub_without_a_budget_reports_no_ceiling_and_admits_everything() {
        let hub = TerminalOutputHub::new(1024);
        assert_eq!(hub.budget_bytes(), None);
        assert_eq!(hub.min_session_bytes(), None);
        assert_eq!(hub.admission_ceiling(), None);
        for index in 0..64 {
            hub.admit_session(&format!("s{index}")).expect("no budget, no limit");
        }
    }

    #[test]
    fn a_corrupt_log_leaves_the_hub_refusing_creates() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = crate::terminal::admission_store::admission_path_for(dir.path());
        std::fs::write(&path, b"not a reservation log at all").expect("damage");

        let hub = TerminalOutputHub::for_daemon_with_budget(4 * 1024 * 1024, 1024 * 1024);
        let error = hub.open_durable_admission(&path).expect_err("must fail closed");
        assert!(matches!(
            error,
            crate::terminal::admission_store::StoreError::BadMagic
        ));
        assert_eq!(hub.admission_refusal(), Some(error));
        assert!(hub.admission().is_none(), "no empty ledger may stand in for it");

        let refusal = hub
            .admit_durably("pane-a")
            .expect_err("creates are refused while the log is unusable");
        assert!(
            matches!(refusal, crate::terminal::admission_store::AdmissionRefusal::StoreUnusable(_)),
            "the refusal names the store, not a transient capacity limit: {refusal:?}"
        );
        assert!(matches!(
            crate::terminal::PtyError::from_admission_refusal(refusal),
            crate::terminal::PtyError::AdmissionStoreUnusable(_)
        ));
    }

    #[test]
    fn an_answered_reservation_survives_a_restart_and_an_unanswered_one_stays_charged() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = crate::terminal::admission_store::admission_path_for(dir.path());

        let first = TerminalOutputHub::for_daemon_with_budget(4 * 1024 * 1024, 1024 * 1024);
        first.open_durable_admission(&path).expect("open");
        first
            .admit_durably_with_request("pane-acked", Some("spawn-acked"))
            .expect("reserve");
        first
            .admit_durably_with_request("pane-lost", Some("spawn-lost"))
            .expect("reserve");
        // The acknowledged spawn is bound; the other keeps no acknowledgement, as a crash mid-spawn
        // would leave it.
        first.admission().expect("ledger").bind("pane-acked").expect("bind");
        drop(first);

        let restarted = TerminalOutputHub::for_daemon_with_budget(4 * 1024 * 1024, 1024 * 1024);
        let ledger = restarted.open_durable_admission(&path).expect("reopen");
        assert_eq!(ledger.charged(), 2, "both records occupy a slot");
        assert_eq!(ledger.pending(), 1, "only the unanswered one may still fork");
        assert_eq!(
            restarted.admission_request_id("pane-lost").as_deref(),
            Some("spawn-lost"),
            "the retry after the restart presents the id the first attempt recorded"
        );
        assert_eq!(
            restarted.admission_request_id("pane-acked"),
            None,
            "an acknowledged spawn has no retry to protect"
        );
    }

    #[test]
    fn recovered_sessions_are_charged_once_and_never_against_the_ceiling() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = crate::terminal::admission_store::admission_path_for(dir.path());
        // Two sessions' worth of admission limit, so recovering three proves the ceiling is not
        // consulted; a restart must not deny the panes that are already running.
        let hub = TerminalOutputHub::for_daemon_with_budget(2 * 1024 * 1024, 1024 * 1024);
        hub.open_durable_admission(&path).expect("open");
        assert_eq!(hub.admission_ceiling().map(|ceiling| ceiling.limit), Some(2));
        let recovered = vec![
            "pane-a".to_string(),
            "pane-b".to_string(),
            "pane-c".to_string(),
        ];

        let accounting = hub.note_recovered_sessions(&recovered).expect("recover");
        assert_eq!(accounting.counted, 3, "recovery is not an admission");
        assert_eq!(hub.admission().expect("ledger").charged(), 3);
        // Recovered panes join the budget, so each share is now below the floor and the floor is
        // what bounds the retained history: the ceiling never falls under it.
        assert_eq!(hub.session_ceiling_bytes(), 1024 * 1024);

        // A new create is refused: the recovered panes fill the configured limit.
        let refusal = hub.admit_durably("pane-d").expect_err("the limit is full");
        assert!(matches!(
            refusal,
            crate::terminal::admission_store::AdmissionRefusal::Capacity(_)
        ));

        // A second pass over the same list (a retried startup) charges nothing new.
        let again = hub.note_recovered_sessions(&recovered).expect("recover again");
        assert_eq!(again.counted, 0);
        assert_eq!(hub.admission().expect("ledger").charged(), 3);
    }

    #[test]
    fn a_create_refused_by_the_in_memory_ceiling_abandons_its_durable_record() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = crate::terminal::admission_store::admission_path_for(dir.path());
        let hub = TerminalOutputHub::for_daemon_with_budget(2 * 1024 * 1024, 1024 * 1024);
        let ledger = hub.open_durable_admission(&path).expect("open");
        // Fill the budget without a durable charge, so the durable limit still has room and the
        // in-memory one is what refuses: that is the only shape that reaches the abandon arm.
        hub.track_budget_session("budget-only-0");
        hub.track_budget_session("budget-only-1");
        assert_eq!(ledger.charged(), 0);

        let refusal = hub.admit_durably("pane-a").expect_err("the budget is full");
        assert!(matches!(
            refusal,
            crate::terminal::admission_store::AdmissionRefusal::Capacity(_)
        ));
        assert!(
            !ledger.is_charged("pane-a"),
            "nothing was dispatched, so the record it wrote is provably dead"
        );
        assert_eq!(
            ledger.store().state_of("pane-a"),
            Some(crate::terminal::admission_store::ReservationState::Abandoned {
                detail: "refused by the in-memory ceiling".to_string()
            })
        );
    }

    #[test]
    fn a_create_refused_by_the_durable_ceiling_writes_no_record() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = crate::terminal::admission_store::admission_path_for(dir.path());
        let hub = TerminalOutputHub::for_daemon_with_budget(2 * 1024 * 1024, 1024 * 1024);
        let ledger = hub.open_durable_admission(&path).expect("open");

        // Two sessions fill the budget; the third is refused, and nothing about it is recorded.
        hub.admit_durably("pane-a").expect("first");
        hub.admit_durably("pane-b").expect("second");
        let refusal = hub.admit_durably("pane-c").expect_err("third is refused");
        assert!(matches!(
            refusal,
            crate::terminal::admission_store::AdmissionRefusal::Capacity(_)
        ));
        assert!(!ledger.is_charged("pane-c"), "a refused create is charged nothing");
        assert_eq!(ledger.charged(), 2);
        assert_eq!(ledger.store().state_of("pane-c"), None);
    }

    #[test]
    fn a_retried_spawn_reuses_its_durable_request_id() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = crate::terminal::admission_store::admission_path_for(dir.path());
        let hub = TerminalOutputHub::for_daemon_with_budget(4 * 1024 * 1024, 1024 * 1024);
        hub.open_durable_admission(&path).expect("open");
        hub.admit_durably_with_request("pane-a", Some("spawn-1"))
            .expect("reserve");
        assert_eq!(hub.admission_request_id("pane-a").as_deref(), Some("spawn-1"));

        // The retry reads the id back rather than minting a new one.
        let retry_id = hub
            .admission_request_id("pane-a")
            .expect("the retry must reuse this id");
        hub.admit_durably_with_request("pane-a", Some(&retry_id))
            .expect("retry");
        assert_eq!(hub.admission_request_id("pane-a").as_deref(), Some("spawn-1"));
        assert_eq!(hub.admission().expect("ledger").charged(), 1);
    }

    #[test]
    fn a_hub_without_a_ledger_records_no_request_id_and_still_admits() {
        let hub = TerminalOutputHub::new(1024);
        hub.admit_durably_with_request("pane-a", Some("spawn-1"))
            .expect("a replay-only hub has nothing to persist");
        assert_eq!(hub.admission_request_id("pane-a"), None);
    }

    /// Every byte value, so a codec that mangles NULs, quotes, backslashes or high bytes fails.
    fn opaque_payload(len: usize) -> Vec<u8> {
        (0..len).map(|index| (index % 251) as u8).collect()
    }

    fn snapshot_with_payload(payload: Vec<u8>) -> SessionHubSnapshot {
        SessionHubSnapshot {
            capacity: 512 * 1024,
            chunks: vec![
                OutputChunk {
                    sequence: 1,
                    bytes: Arc::from(opaque_payload(payload.len() / 2)),
                    metrics_read_unix_micros: None,
                    replay_gap: None,
                },
                OutputChunk {
                    sequence: 2,
                    bytes: Arc::from(payload),
                    metrics_read_unix_micros: Some(7),
                    replay_gap: Some(ReplayGap {
                        requested_after_sequence: 3,
                        available_from_sequence: 5,
                    }),
                },
            ],
            next_sequence: 3,
            bracketed_paste_enabled: true,
            resize_ledger: vec![ResizePoint { sequence: 2, cols: 120, rows: 40 }],
            initial_size: None,
            replay_gap: None,
            gap_floor: 0,
            transport_owner: true,
            retained_state: None,
            screen_seed: Some(super::super::screen_mirror::ScreenSeed {
                cols: 120,
                rows: 40,
                bytes: opaque_payload(4096),
            }),
            history_incomplete: false,
        }
    }

    /// A checkpoint's byte fields must survive a round trip unchanged. This is the whole contract
    /// the keeper relies on: it stores the blob and hands it back verbatim.
    #[test]
    fn checkpoint_snapshot_round_trips_every_byte_field() {
        let snapshot = snapshot_with_payload(opaque_payload(64 * 1024));
        let encoded = serde_json::to_vec(&snapshot).expect("checkpoint serializes");
        let decoded: SessionHubSnapshot =
            serde_json::from_slice(&encoded).expect("checkpoint deserializes");
        assert_eq!(decoded, snapshot);
    }

    /// A predecessor daemon of the previous generation wrote `Vec<u8>` as a JSON number array, and
    /// the keeper can hand that blob to this generation's successor. It must still restore.
    ///
    /// `screenSeed` is base64 in the fixture on purpose: that field was already base64 before this
    /// change, so the ring chunks are the fields that had a number-array form to still accept.
    #[test]
    fn checkpoint_snapshot_still_decodes_the_legacy_number_array_form() {
        let legacy = serde_json::json!({
            "capacity": 1024,
            "chunks": [
                { "sequence": 1, "bytes": [104, 105, 0, 255] },
            ],
            "next_sequence": 2,
            "bracketed_paste_enabled": false,
            "resize_ledger": [],
            "replay_gap": null,
            "gap_floor": 0,
            "transport_owner": false,
            "retained_state": null,
            "screen_seed": { "cols": 80, "rows": 24, "bytes": "G1tI" },
            "history_incomplete": false,
        });
        let decoded: SessionHubSnapshot =
            serde_json::from_value(legacy).expect("a legacy checkpoint still decodes");
        assert_eq!(&decoded.chunks[0].bytes[..], &b"hi\x00\xff"[..]);
        assert_eq!(decoded.screen_seed.expect("seed").bytes, b"\x1b[H".to_vec());
    }

    /// The size bound: a 512 KiB ring -- the retained ceiling -- must encode to well under 1.4x.
    /// The number-array form was ~4x, and that transient is what this encoding exists to remove.
    #[test]
    fn a_512_kib_checkpoint_ring_encodes_to_under_1_4x() {
        let ring = 512 * 1024;
        let snapshot = SessionHubSnapshot {
            capacity: ring,
            chunks: vec![OutputChunk {
                sequence: 1,
                bytes: Arc::from(opaque_payload(ring)),
                metrics_read_unix_micros: None,
                replay_gap: None,
            }],
            next_sequence: 2,
            bracketed_paste_enabled: false,
            resize_ledger: Vec::new(),
            initial_size: None,
            replay_gap: None,
            gap_floor: 0,
            transport_owner: false,
            retained_state: None,
            screen_seed: None,
            history_incomplete: false,
        };

        let encoded = serde_json::to_vec(&snapshot).expect("checkpoint serializes");
        let boundary = ring * 14 / 10;
        assert!(
            encoded.len() < boundary,
            "a {ring}-byte ring encoded to {} bytes, at or past the {boundary}-byte boundary: \
             the byte fields are back to a JSON number array",
            encoded.len()
        );

        // And the legacy form really is far larger, so this bound is not vacuous.
        let legacy = serde_json::to_vec(&opaque_payload(ring)).expect("the legacy form serializes");
        assert!(
            legacy.len() > 3 * ring,
            "the number-array form this replaces was {} bytes for {ring} bytes of output, which \
             is the ~4x blowup the compact encoding removes",
            legacy.len()
        );
    }
}