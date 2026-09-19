use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::broadcast;

const DEFAULT_BUFFER_CAPACITY: usize = 512 * 1024; // 512 KiB
const BROADCAST_CAPACITY: usize = 1024;
const RESIZE_LEDGER_CAPACITY: usize = 4096;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputChunk {
    pub sequence: u64,
    /// Shared, immutable chunk payload. Storing the bytes behind an `Arc` lets the buffer,
    /// the sequence broadcast, and the raw broadcast share a single allocation instead of
    /// deep-copying a 64 KiB `Vec<u8>` for every subscriber on each publish.
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
    pub replay_gap: Option<ReplayGap>,
    #[serde(default)]
    pub transport_owner: bool,
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
}

pub fn scan_dec_mode_2004(bytes: &[u8]) -> Option<bool> {
    if !bytes.windows(4).any(|w| w == b"2004") {
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
        Self {
            capacity,
            chunks: VecDeque::new(),
            current_size: 0,
            next_sequence: 1,
            bracketed_paste_enabled: false,
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
    ) -> Self {
        let current_size = chunks.iter().map(|c| c.bytes.len()).sum();
        Self {
            capacity,
            chunks: VecDeque::from(chunks),
            current_size,
            next_sequence,
            bracketed_paste_enabled,
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
        let (history, _, start, end, gap) = self.snapshot_after_segmented(after_sequence, &[]);
        (history, start, end, gap)
    }

    pub fn snapshot_after_segmented(
        &self,
        after_sequence: Option<u64>,
        ledger: &[ResizePoint],
    ) -> (
        Vec<u8>,
        Vec<HistorySegment>,
        Option<u64>,
        Option<u64>,
        Option<ReplayGap>,
    ) {
        if self.chunks.is_empty() {
            let segments = segment_history(&[], ledger, after_sequence);
            return (Vec::new(), segments, None, None, None);
        }

        let first_seq = self.chunks.front().unwrap().sequence;
        let last_seq = self.chunks.back().unwrap().sequence;

        match after_sequence {
            None => {
                let chunk_refs: Vec<&OutputChunk> = self.chunks.iter().collect();
                let mut segments = segment_history(&chunk_refs, ledger, after_sequence);
                let history = self.snapshot();
                if self.bracketed_paste_enabled && !self.buffer_contains_active_bracketed_paste() {
                    if let Some(first_seg) = segments.first_mut() {
                        first_seg.bytes.splice(0..0, b"\x1b[?2004h".iter().copied());
                    } else {
                        segments.push(HistorySegment {
                            cols: None,
                            rows: None,
                            bytes: b"\x1b[?2004h".to_vec(),
                        });
                    }
                }
                (history, segments, Some(first_seq), Some(last_seq), None)
            }
            Some(req_seq) => {
                if req_seq >= last_seq {
                    let segments = segment_history(&[], ledger, Some(req_seq));
                    (Vec::new(), segments, None, Some(last_seq), None)
                } else if req_seq + 1 < first_seq {
                    // Eviction gap: requested sequence has been evicted
                    let gap = Some(ReplayGap {
                        requested_after_sequence: req_seq,
                        available_from_sequence: first_seq,
                    });
                    let chunk_refs: Vec<&OutputChunk> = self.chunks.iter().collect();
                    let mut segments = segment_history(&chunk_refs, ledger, after_sequence);
                    let history = self.snapshot();
                    if self.bracketed_paste_enabled
                        && !self.buffer_contains_active_bracketed_paste()
                    {
                        if let Some(first_seg) = segments.first_mut() {
                            first_seg.bytes.splice(0..0, b"\x1b[?2004h".iter().copied());
                        } else {
                            segments.push(HistorySegment {
                                cols: None,
                                rows: None,
                                bytes: b"\x1b[?2004h".to_vec(),
                            });
                        }
                    }
                    (history, segments, Some(first_seq), Some(last_seq), gap)
                } else {
                    let mut history = Vec::new();
                    let mut start_seq = None;
                    let mut included_chunks = Vec::new();
                    for chunk in &self.chunks {
                        if chunk.sequence > req_seq {
                            if start_seq.is_none() {
                                start_seq = Some(chunk.sequence);
                            }
                            history.extend_from_slice(&chunk.bytes);
                            included_chunks.push(chunk);
                        }
                    }
                    let segments = segment_history(&included_chunks, ledger, after_sequence);
                    (history, segments, start_seq, Some(last_seq), None)
                }
            }
        }
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

pub struct SessionHub {
    pub(crate) machine_senders: Vec<machine_output::MachineSender>,
    pub(crate) buffer: BoundedBuffer,
    pub(crate) sender: broadcast::Sender<OutputChunk>,
    pub(crate) raw_sender: broadcast::Sender<Vec<u8>>,
    pub(crate) resize_ledger: Vec<ResizePoint>,
    pub(crate) replay_gap: Option<ReplayGap>,
}

impl SessionHub {
    pub fn export_state(&self) -> SessionHubSnapshot {
        SessionHubSnapshot {
            capacity: self.buffer.capacity,
            chunks: self.buffer.chunks.iter().cloned().collect(),
            next_sequence: self.buffer.next_sequence,
            bracketed_paste_enabled: self.buffer.bracketed_paste_enabled,
            resize_ledger: self.resize_ledger.clone(),
            replay_gap: self.replay_gap.clone(),
            transport_owner: false,
        }
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

#[derive(Clone)]
pub struct TerminalOutputHub {
    sessions: Arc<RwLock<HashMap<String, Arc<RwLock<SessionHub>>>>>,
    transport_owners: Arc<RwLock<std::collections::HashSet<String>>>,
    capacity: usize,
}

impl Default for TerminalOutputHub {
    fn default() -> Self {
        Self::new(DEFAULT_BUFFER_CAPACITY)
    }
}

impl TerminalOutputHub {
    pub fn new(capacity: usize) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            transport_owners: Arc::new(RwLock::new(std::collections::HashSet::new())),
            capacity,
        }
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
        let (raw_tx, raw_rx) = broadcast::channel(BROADCAST_CAPACITY);
        let hub = SessionHub {
            machine_senders: Vec::new(),
            buffer: BoundedBuffer::new(self.capacity),
            sender: tx,
            raw_sender: raw_tx,
            resize_ledger: Vec::new(),
            replay_gap: None,
        };
        self.sessions
            .write()
            .insert(session_id.to_string(), Arc::new(RwLock::new(hub)));
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
        hub.buffer.chunks.clear();
        hub.buffer.current_size = 0;
        let size = hub.resize_ledger.last().cloned();
        hub.resize_ledger.clear();
        if let Some(mut size) = size {
            size.sequence = sequence;
            hub.resize_ledger.push(size);
        }
        hub.replay_gap = Some(gap.clone());
        let boundary = OutputChunk {
            sequence,
            bytes: Arc::from([]),
            metrics_read_unix_micros: None,
            replay_gap: Some(gap),
        };
        hub.machine_senders.retain(|sender| sender.publish(&boundary));
        let _ = hub.sender.send(boundary.clone());
        Some(boundary)
    }

    pub fn publish_with_read_timestamp(
        &self,
        session_id: &str,
        chunk_bytes: Vec<u8>,
        metrics_read_unix_micros: Option<u64>,
    ) -> Option<OutputChunk> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;

        let mut hub = session_hub.write();
        let chunk = hub
            .buffer
            .push_with_read_timestamp(chunk_bytes, metrics_read_unix_micros)?;

        hub.machine_senders.retain(|sender| sender.publish(&chunk));
        // Broadcast to sequence subscribers (cheap Arc refcount bump on the payload).
        let _ = hub.sender.send(chunk.clone());
        // Broadcast to legacy raw receivers
        let _ = hub.raw_sender.send(chunk.bytes.to_vec());

        Some(chunk)
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
        if hub.resize_ledger.is_empty() {
            hub.resize_ledger.push(ResizePoint {
                sequence: 0,
                cols,
                rows,
            });
        }
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
        if hub.resize_ledger.len() >= RESIZE_LEDGER_CAPACITY {
            hub.resize_ledger.remove(0);
        }
        hub.resize_ledger.push(ResizePoint {
            sequence,
            cols,
            rows,
        });
        Some(sequence)
    }

    pub fn subscribe(&self, session_id: &str) -> Option<(Vec<u8>, broadcast::Receiver<Vec<u8>>)> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;

        let hub = session_hub.read();
        // Subscribe first to preserve subscriber-first snapshot invariant
        let rx = hub.raw_sender.subscribe();
        let history = hub.buffer.snapshot();
        Some((history, rx))
    }

    pub fn subscribe_with_sequence(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Option<SessionAttachment> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;

        let hub = session_hub.read();
        // 1. Subscribe FIRST in the critical section
        let rx = hub.sender.subscribe();
        // 2. Snapshot within the same critical section
        let (history, history_segments, history_start_sequence, history_end_sequence, gap) = hub
            .buffer
            .snapshot_after_segmented(after_sequence, &hub.resize_ledger);
        let gap = gap.or_else(|| {
            hub.replay_gap.clone().filter(|gap| {
                after_sequence.is_none_or(|after| after < gap.available_from_sequence - 1)
            })
        });

        let snapshot = AttachmentSnapshot {
            session_id: session_id.to_string(),
            history_start_sequence,
            history_end_sequence,
            history,
            history_segments,
            gap,
        };

        Some(SessionAttachment {
            snapshot,
            receiver: rx,
        })
    }

    pub fn remove_session(&self, session_id: &str) {
        self.sessions.write().remove(session_id);
        self.transport_owners.write().remove(session_id);
    }

    pub fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().contains_key(session_id)
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
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;
        let hub = session_hub.read();
        let mut snapshot = hub.export_state();
        snapshot.transport_owner = self.transport_owner(session_id);
        Some(snapshot)
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

        let buffer = BoundedBuffer::from_snapshot(
            capacity,
            snapshot.chunks,
            snapshot.next_sequence,
            snapshot.bracketed_paste_enabled,
        );

        let is_transport_owner = snapshot.transport_owner;
        let hub = SessionHub {
            machine_senders: Vec::new(),
            buffer,
            sender: tx,
            raw_sender: raw_tx,
            resize_ledger: snapshot.resize_ledger,
            replay_gap: snapshot.replay_gap,
        };

        self.sessions
            .write()
            .insert(session_id.to_string(), Arc::new(RwLock::new(hub)));

        if is_transport_owner {
            self.transport_owners.write().insert(session_id.to_string());
        } else {
            self.transport_owners.write().remove(session_id);
        }

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
    async fn ssh_process_survival_gap_precedes_recovered_bytes_and_invalidates_history() {
        let hub = TerminalOutputHub::new(1024);
        hub.register_session("remote");
        hub.publish("remote", b"stale".to_vec());
        let mut live = hub.subscribe_with_sequence("remote", Some(1)).unwrap();
        hub.publish_gap("remote").unwrap();
        let between = hub.subscribe_with_sequence("remote", Some(1)).unwrap();
        assert!(between.snapshot.history.is_empty());
        assert!(between.snapshot.gap.is_some());
        hub.publish("remote", b"recovered".to_vec());
        let boundary = live.receiver.try_recv().unwrap();
        assert!(boundary.bytes.is_empty());
        let gap = boundary.replay_gap.unwrap();
        assert_eq!(gap.requested_after_sequence, 1);
        let output = live.receiver.try_recv().unwrap();
        assert_eq!(&*output.bytes, b"recovered");
        assert_eq!(gap.available_from_sequence, output.sequence);
        let late = hub.subscribe_with_sequence("remote", Some(1)).unwrap();
        assert_eq!(late.snapshot.history, b"recovered");
        assert_eq!(late.snapshot.gap, Some(gap));
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
        assert!(pred_attachment.snapshot.gap.is_some());
        assert_eq!(pred_attachment.snapshot.history_start_sequence, Some(2));
        assert_eq!(pred_attachment.snapshot.history_end_sequence, Some(3));
        assert_eq!(
            pred_attachment.snapshot.history,
            b"\x1b[?2004hfirst chunk second chunk\n"
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
        assert!(snapshot.replay_gap.is_some());
        assert!(snapshot.transport_owner);
        assert_eq!(snapshot.chunks.len(), 2);
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
            replay_gap: None,
            transport_owner: false,
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
            replay_gap: None,
            transport_owner: false,
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
            replay_gap: None,
            transport_owner: false,
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
            replay_gap: None,
            transport_owner: false,
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
            replay_gap: None,
            transport_owner: false,
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
            replay_gap: None,
            transport_owner: false,
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
            replay_gap: None,
            transport_owner: false,
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
            replay_gap: Some(ReplayGap {
                requested_after_sequence: 5,
                available_from_sequence: 5,
            }),
            transport_owner: false,
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
            replay_gap: Some(ReplayGap {
                requested_after_sequence: 8,
                available_from_sequence: 11,
            }),
            transport_owner: false,
        };
        assert_eq!(
            snap_gap_future.validate(),
            Err(OutputHubSnapshotError::ReplayGapExceedsNextSequence(11))
        );
    }
}
