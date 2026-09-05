use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
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

pub struct BoundedBuffer {
    capacity: usize,
    chunks: VecDeque<OutputChunk>,
    current_size: usize,
    next_sequence: u64,
}

impl BoundedBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            chunks: VecDeque::new(),
            current_size: 0,
            next_sequence: 1,
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
        };

        self.current_size += chunk.bytes.len();
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

    pub fn snapshot(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.current_size);
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
                let segments = segment_history(&chunk_refs, ledger, after_sequence);
                (
                    self.snapshot(),
                    segments,
                    Some(first_seq),
                    Some(last_seq),
                    None,
                )
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
                    let segments = segment_history(&chunk_refs, ledger, after_sequence);
                    (
                        self.snapshot(),
                        segments,
                        Some(first_seq),
                        Some(last_seq),
                        gap,
                    )
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

    let mut segments: Vec<HistorySegment> =
        Vec::with_capacity(ledger.len().saturating_sub(li) + 1);
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

struct SessionHub {
    buffer: BoundedBuffer,
    sender: broadcast::Sender<OutputChunk>,
    raw_sender: broadcast::Sender<Vec<u8>>,
    resize_ledger: Vec<ResizePoint>,
}

#[derive(Clone)]
pub struct TerminalOutputHub {
    sessions: Arc<RwLock<HashMap<String, Arc<RwLock<SessionHub>>>>>,
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
            buffer: BoundedBuffer::new(self.capacity),
            sender: tx,
            raw_sender: raw_tx,
            resize_ledger: Vec::new(),
        };
        self.sessions
            .write()
            .insert(session_id.to_string(), Arc::new(RwLock::new(hub)));
        (raw_rx, rx)
    }

    pub fn publish(&self, session_id: &str, chunk_bytes: Vec<u8>) -> Option<OutputChunk> {
        self.publish_with_read_timestamp(session_id, chunk_bytes, None)
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

    pub fn subscribe(
        &self,
        session_id: &str,
    ) -> Option<(Vec<u8>, broadcast::Receiver<Vec<u8>>)> {
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
    }

    pub fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().contains_key(session_id)
    }

    pub fn session_sequence_range(&self, session_id: &str) -> Option<(Option<u64>, Option<u64>)> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;
        let hub = session_hub.read();
        Some((hub.buffer.start_sequence(), hub.buffer.end_sequence()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
