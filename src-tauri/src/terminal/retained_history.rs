//! Retained reconstruction history: separate text and image budgets over whole VT units.
//!
//! This is deliberately *not* the recovery ring. The ring stays byte-oriented for
//! incremental (`after_sequence`) replay and machine transport accounting; this structure
//! keeps the longer, protocol-safe history a restarted GUI replays from scratch.

use std::collections::VecDeque;

use super::vt_state::TerminalStateRecorder;
use super::vt_stream::{Pending, Unit, UnitKind, VtUnitSplitter};

pub const DEFAULT_TEXT_RETENTION_BYTES: usize = 512 * 1024;
/// Images are charged separately so one screenshot cannot evict the text timeline.
pub const IMAGE_RETENTION_RATIO: usize = 16;
/// Ceiling for one retained transmission. A larger image is never partially retained:
/// the splitter discards the whole chain and resyncs at its terminator.
pub const MAX_RETAINED_GROUP_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionBudget {
    pub text_bytes: usize,
    pub image_bytes: usize,
}

impl RetentionBudget {
    pub fn from_text_capacity(text_bytes: usize) -> Self {
        Self {
            text_bytes,
            image_bytes: text_bytes.saturating_mul(IMAGE_RETENTION_RATIO),
        }
    }

    fn max_unit_bytes(&self) -> usize {
        self.image_bytes
            .max(self.text_bytes)
            .min(MAX_RETAINED_GROUP_BYTES)
            .max(64)
    }

    fn text_block_bytes(&self) -> usize {
        (self.text_bytes / 64).max(1)
    }
}

impl Default for RetentionBudget {
    fn default() -> Self {
        Self::from_text_capacity(DEFAULT_TEXT_RETENTION_BYTES)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    order: u64,
    sequence: u64,
    bytes: Vec<u8>,
    open: bool,
    image_group: Option<u64>,
}
pub struct RetainedHistory {
    budget: RetentionBudget,
    splitter: VtUnitSplitter,
    evicted_state: TerminalStateRecorder,
    text: VecDeque<Record>,
    images: VecDeque<Record>,
    text_bytes: usize,
    image_bytes: usize,
    units: Vec<Unit>,
}

impl RetainedHistory {
    pub fn new(budget: RetentionBudget) -> Self {
        Self {
            splitter: VtUnitSplitter::new(budget.max_unit_bytes(), budget.text_block_bytes()),
            budget,
            evicted_state: TerminalStateRecorder::new(),
            text: VecDeque::new(),
            images: VecDeque::new(),
            text_bytes: 0,
            image_bytes: 0,
            units: Vec::new(),
        }
    }

    pub fn budget(&self) -> RetentionBudget {
        self.budget
    }

    /// Trailing prefix (plus any parser resync bytes) of output whose terminator has not
    /// arrived, with the sequence it started at so resize segmentation can place it.
    pub fn pending(&self) -> Pending {
        self.splitter.pending()
    }

    pub fn state_prelude(&self) -> Vec<u8> {
        self.evicted_state.prelude().encode()
    }

    /// Full protocol-safe reconstruction: state prelude, retained units in stream order,
    /// then the in-flight prefix. Replaying this into a fresh parser reproduces the pane.
    pub fn replay(&self) -> Vec<u8> {
        let prelude = self.state_prelude();
        let pending = self.pending();
        let mut out = Vec::with_capacity(prelude.len() + self.len() + pending.bytes.len());
        out.extend_from_slice(&prelude);
        for (_, bytes) in self.records_with_pending(&pending) {
            out.extend_from_slice(bytes);
        }
        out
    }

    pub fn records_with_pending<'a>(&'a self, pending: &'a Pending) -> Vec<(u64, &'a [u8])> {
        let mut records: Vec<_> = self.text.iter().chain(self.images.iter())
            .map(|record| (record.order, record.sequence, record.bytes.as_slice()))
            .collect();
        for (index, &(start, order, sequence)) in pending.parts.iter().enumerate() {
            let end = pending.parts.get(index + 1).map_or(pending.bytes.len(), |part| part.0);
            if start < end {
                records.push((order, sequence, &pending.bytes[start..end]));
            }
        }
        records.sort_by_key(|record| record.0);
        records.into_iter().map(|(_, sequence, bytes)| (sequence, bytes)).collect()
    }

    pub fn push(&mut self, sequence: u64, bytes: &[u8]) {
        let mut units = std::mem::take(&mut self.units);
        units.clear();
        // Each unit carries the sequence of its own first byte, so a transmission that spans
        // several published chunks is attributed to the chunk that opened it.
        self.splitter.push(sequence, bytes, &mut units);
        for unit in units.drain(..) {
            self.retain_unit(unit);
        }
        self.units = units;
        self.evict();
    }

    /// Prevents text coalescing across a boundary whose sequence must stay addressable,
    /// which is what keeps resize segmentation exact after retention.
    pub fn seal(&mut self) {
        if let Some(record) = self.text.back_mut() {
            record.open = false;
        }
    }

    pub fn clear(&mut self) {
        self.splitter.reset();
        self.evicted_state.clear();
        self.text.clear();
        self.images.clear();
        self.text_bytes = 0;
        self.image_bytes = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.images.is_empty()
    }

    pub fn len(&self) -> usize {
        self.text_bytes + self.image_bytes
    }

    pub fn text_bytes(&self) -> usize {
        self.text_bytes
    }

    pub fn image_bytes(&self) -> usize {
        self.image_bytes
    }

    pub fn start_sequence(&self) -> Option<u64> {
        let records_start = match (self.text.front(), self.images.front()) {
            (Some(text), Some(image)) if image.order < text.order => Some(image.sequence),
            (Some(text), _) => Some(text.sequence),
            (None, Some(image)) => Some(image.sequence),
            (None, None) => None,
        };
        match (records_start, self.pending().sequence) {
            (Some(records), Some(pending)) => Some(records.min(pending)),
            (Some(records), None) => Some(records),
            (None, pending) => pending,
        }
    }

    /// Records in stream order, merged from the two budget-separated queues.
    pub fn records(&self) -> Vec<(u64, &[u8])> {
        let mut merged = Vec::with_capacity(self.text.len() + self.images.len());
        let mut text = self.text.iter().peekable();
        let mut images = self.images.iter().peekable();
        loop {
            let take_image = match (text.peek(), images.peek()) {
                (Some(t), Some(i)) => i.order < t.order,
                (None, Some(_)) => true,
                (Some(_), None) => false,
                (None, None) => break,
            };
            let record = if take_image {
                images.next()
            } else {
                text.next()
            };
            if let Some(record) = record {
                merged.push((record.sequence, record.bytes.as_slice()));
            }
        }
        merged
    }

    fn retain_unit(&mut self, unit: Unit) {
        match unit.kind {
            UnitKind::Image => {
                self.image_bytes += unit.bytes.len();
                let parts = if unit.parts.is_empty() {
                    vec![(0, unit.order, unit.sequence)]
                } else {
                    unit.parts
                };
                for (index, &(start, order, sequence)) in parts.iter().enumerate() {
                    let end = parts.get(index + 1).map_or(unit.bytes.len(), |part| part.0);
                    self.images.push_back(Record {
                        order,
                        sequence,
                        bytes: unit.bytes[start..end].to_vec(),
                        open: false,
                        image_group: Some(unit.order),
                    });
                }
                self.seal();
            }
            UnitKind::Text => {
                self.text_bytes += unit.bytes.len();
                let block_bytes = self.budget.text_block_bytes();
                let coalesce = self
                    .text
                    .back()
                    .is_some_and(|record| record.open && record.bytes.len() < block_bytes
                        && record.sequence == unit.sequence
                        && record.order + 1 == unit.order);
                if coalesce {
                    if let Some(record) = self.text.back_mut() {
                        record.bytes.extend_from_slice(&unit.bytes);
                        return;
                    }
                }
                self.text.push_back(Record {
                    order: unit.order,
                    sequence: unit.sequence,
                    bytes: unit.bytes,
                    open: true,
                    image_group: None,
                });
            }
        }
    }

    fn evict(&mut self) {
        while self.text_bytes > self.budget.text_bytes {
            match self.text.pop_front() {
                Some(record) => {
                    self.text_bytes -= record.bytes.len();
                    self.evicted_state.observe(&record.bytes);
                }
                None => break,
            }
        }
        while self.image_bytes > self.budget.image_bytes {
            match self.images.pop_front() {
                Some(record) => {
                    self.image_bytes -= record.bytes.len();
                    let group = record.image_group;
                    self.images.retain(|part| {
                        if part.image_group == group {
                            self.image_bytes -= part.bytes.len();
                            false
                        } else { true }
                    });
                }
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kitty(payload: usize) -> Vec<u8> {
        let mut out = b"\x1b_Ga=T,f=24;".to_vec();
        out.extend(std::iter::repeat_n(b'A', payload));
        out.extend_from_slice(b"\x1b\\");
        out
    }

    #[test]
    fn multipart_replay_preserves_interleaved_cursor_commands() {
        let mut retained = RetainedHistory::new(RetentionBudget {
            text_bytes: 4096,
            image_bytes: 4096,
        });
        let first = b"\x1b_Ga=T,f=24,s=2,v=1,m=1;/wAA\x1b\\";
        let middle = b"KEEP\r\n\x1b[3;4H";
        let last = b"\x1b_Gm=0;AP8A\x1b\\";
        retained.push(1, first);
        retained.push(2, middle);
        assert_eq!(retained.replay(), [first.as_slice(), middle.as_slice()].concat());
        retained.push(3, last);
        assert_eq!(retained.replay(), [first.as_slice(), middle.as_slice(), last.as_slice()].concat());
        retained.clear();
        let stream = [b"BEFORE".as_slice(), first.as_slice(), middle.as_slice(), last.as_slice()].concat();
        retained.push(1, &stream);
        assert_eq!(retained.replay(), stream);
    }

    fn kitty_multipart(parts: usize, payload: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for index in 0..parts {
            let more = u8::from(index + 1 < parts);
            out.extend_from_slice(format!("\x1b_Ga=T,f=24,m={more};").as_bytes());
            out.extend(std::iter::repeat_n(b'A', payload));
            out.extend_from_slice(b"\x1b\\");
        }
        out
    }

    fn history(retained: &RetainedHistory) -> Vec<u8> {
        retained
            .records()
            .into_iter()
            .flat_map(|(_, bytes)| bytes.to_vec())
            .collect()
    }

    #[test]
    fn retained_image_keeps_the_sequence_of_the_chunk_that_opened_it() {
        // Given: a transmission opened in one published chunk and closed in a later one,
        // with a resize boundary in between.
        let mut retained = RetainedHistory::new(RetentionBudget::default());
        retained.push(1, b"\x1b_Ga=T,f=24,m=1;AAA\x1b\\");
        retained.seal();
        retained.push(3, b"\x1b_Gm=0;BBB\x1b\\LABEL");

        // Then: the image is attributed to sequence 1 so resize segmentation wraps it with
        // the geometry it was drawn at, while the trailing text keeps its own sequence.
        let records = retained.records();
        let image = records
            .iter()
            .find(|(_, bytes)| bytes.starts_with(b"\x1b_G"))
            .expect("image record");
        assert_eq!(image.0, 1);
        let label = records
            .iter()
            .find(|(_, bytes)| bytes.ends_with(b"LABEL"))
            .expect("text record");
        assert_eq!(label.0, 3);
    }

    #[test]
    fn text_interleaved_in_a_transmission_survives_image_eviction() {
        // Given: an image budget that cannot hold the transmissions, with ordinary text
        // printed between their parts.
        let mut retained = RetainedHistory::new(RetentionBudget {
            text_bytes: 64 * 1024,
            image_bytes: 4096,
        });
        for index in 0..8u64 {
            let base = 1 + index * 3;
            retained.push(base, b"\x1b_Ga=T,f=24,m=1;AAAA\x1b\\");
            retained.push(base + 1, format!("INTERLEAVED-{index}\r\n").as_bytes());
            retained.push(base + 2, b"\x1b_Gm=0;BBBB\x1b\\");
        }

        // Then: image chains are evicted atomically but the interleaved text is accounted
        // separately and never leaves with them.
        let bytes = history(&retained);
        for index in 0..8u64 {
            let marker = format!("INTERLEAVED-{index}");
            assert!(
                bytes
                    .windows(marker.len())
                    .any(|w| w == marker.as_bytes()),
                "interleaved text {index} evicted with the image chain"
            );
        }
        assert!(retained.image_bytes() <= 4096);
    }

    #[test]
    fn oversized_transmission_is_never_partially_retained() {
        // Given: one transmission far beyond the per-group ceiling.
        let mut retained = RetainedHistory::new(RetentionBudget {
            text_bytes: 4096,
            image_bytes: 8192,
        });
        retained.push(1, b"BEFORE\r\n");
        retained.push(2, &kitty_multipart(64, 4096));
        retained.push(3, b"AFTER\r\n");

        // Then: no fragment of it is retained and the surrounding text is intact.
        let bytes = retained.replay();
        assert!(!bytes.windows(3).any(|w| w == b"\x1b_G"));
        assert!(bytes.windows(6).any(|w| w == b"BEFORE"));
        assert!(bytes.windows(5).any(|w| w == b"AFTER"));
    }

    #[test]
    fn multipart_image_eviction_never_leaves_orphan_continuation_commands() {
        // Given: several multi-part transmissions, more than the image budget holds.
        let mut retained = RetainedHistory::new(RetentionBudget {
            text_bytes: 4096,
            image_bytes: 16 * 1024,
        });
        retained.push(1, b"OLDEST\r\n");
        for index in 0..10u64 {
            retained.push(2 + index, &kitty_multipart(4, 2048));
        }
        retained.push(99, b"NEWEST\r\n");

        // Then: every retained transmission is complete - the count of opening commands
        // matches the count of m=0 terminators, so no continuation is replayed alone.
        let bytes = retained.replay();
        let opens = bytes.windows(5).filter(|w| *w == b"a=T,f").count();
        let closes = bytes.windows(4).filter(|w| *w == b"m=0;").count();
        assert_eq!(opens, closes * 4, "orphan continuation retained");
        assert!(retained.image_bytes() <= 16 * 1024);
        assert!(bytes.windows(6).any(|w| w == b"NEWEST"));
    }

    #[test]
    fn replay_restores_attributes_and_modes_lost_to_eviction() {
        // Given: colour and bracketed paste enabled in output that is later evicted.
        let mut retained = RetainedHistory::new(RetentionBudget {
            text_bytes: 512,
            image_bytes: 4096,
        });
        retained.push(1, b"\x1b[?2004h\x1b[38;5;196mstart\r\n");
        for sequence in 0..200u64 {
            retained.push(2 + sequence, format!("line-{sequence:04}\r\n").as_bytes());
        }

        // Then: replay re-establishes that state before the surviving tail.
        let bytes = retained.replay();
        assert!(bytes.starts_with(b"\x1b[?2004h"));
        assert!(bytes.windows(11).any(|w| w == b"\x1b[38;5;196m"));
    }

    #[test]
    fn replay_appends_the_in_flight_prefix_so_live_output_continues() {
        // Given: a transmission whose terminator has not arrived at attach time.
        let mut retained = RetainedHistory::new(RetentionBudget::default());
        retained.push(1, b"text");
        retained.push(2, b"\x1b_Ga=T,f=24,m=1;AAA\x1b\\\x1b_Gm=1;BB");

        // Then: replay carries the whole open chain so the next live chunk completes it
        // instead of leaking base64 as text.
        let bytes = retained.replay();
        assert!(bytes.starts_with(b"text"));
        assert_eq!(bytes.windows(3).filter(|w| *w == b"\x1b_G").count(), 2);
        assert!(bytes.ends_with(b"BB"));
    }

    #[test]
    fn image_flood_never_evicts_text_history() {
        // Given: budgets that make text tiny compared with the image traffic.
        let mut retained = RetainedHistory::new(RetentionBudget {
            text_bytes: 4096,
            image_bytes: 8192,
        });

        // When: one image far past its own budget is streamed between two text lines.
        retained.push(1, b"OLDEST\r\n");
        for sequence in 0..8u64 {
            retained.push(2 + sequence, &kitty(4096));
        }
        retained.push(20, b"NEWEST\r\n");

        // Then: images are bounded on their own budget and text survives untouched.
        assert!(retained.image_bytes() <= 8192);
        let bytes = history(&retained);
        assert!(bytes.windows(6).any(|w| w == b"OLDEST"));
        assert!(bytes.windows(6).any(|w| w == b"NEWEST"));
    }

    #[test]
    fn text_flood_never_evicts_retained_images() {
        // Given: an image retained before a long text flood.
        let mut retained = RetainedHistory::new(RetentionBudget {
            text_bytes: 1024,
            image_bytes: 64 * 1024,
        });
        retained.push(1, &kitty(2048));

        // When: text output far past the text budget arrives.
        for sequence in 0..400u64 {
            retained.push(2 + sequence, format!("line-{sequence:04}\r\n").as_bytes());
        }

        // Then: the image is still whole in the reconstruction.
        let bytes = history(&retained);
        assert_eq!(
            bytes.windows(3).filter(|w| *w == b"\x1b_G").count(),
            1,
            "image evicted by text flood"
        );
        assert!(retained.text_bytes() <= 1024);
    }

    #[test]
    fn retention_preserves_interleaved_stream_order() {
        let mut retained = RetainedHistory::new(RetentionBudget {
            text_bytes: 64 * 1024,
            image_bytes: 64 * 1024,
        });
        retained.push(1, b"A");
        retained.push(2, &kitty(16));
        retained.push(3, b"B");
        let bytes = history(&retained);
        let image_at = bytes.windows(3).position(|w| w == b"\x1b_G").unwrap();
        let a_at = bytes.iter().position(|b| *b == b'A').unwrap();
        let b_at = bytes.iter().position(|b| *b == b'B').unwrap();
        assert!(a_at < image_at && image_at < b_at);
    }

    #[test]
    fn sequence_split_across_pushes_is_retained_whole() {
        // Given: a kitty transmission delivered over three unaligned PTY reads.
        let mut retained = RetainedHistory::new(RetentionBudget::default());
        let image = kitty(4096);
        let third = image.len() / 3;
        retained.push(1, &image[..third]);
        retained.push(2, &image[third..third * 2]);
        retained.push(3, &image[third * 2..]);

        // Then: the reconstruction holds exactly one complete sequence.
        assert_eq!(history(&retained), image);
        assert_eq!(retained.start_sequence(), Some(1));
    }

    #[test]
    fn incomplete_trailing_sequence_is_not_retained_as_a_unit() {
        let mut retained = RetainedHistory::new(RetentionBudget::default());
        retained.push(1, b"text\x1b_Ga=T;AAA");
        assert_eq!(history(&retained), b"text");
        assert_eq!(retained.replay(), b"text\x1b_Ga=T;AAA");
    }

    #[test]
    fn seal_keeps_sequence_addressable_for_resize_segmentation() {
        let mut retained = RetainedHistory::new(RetentionBudget::default());
        retained.push(1, b"before");
        retained.seal();
        retained.push(3, b"after");
        let records = retained.records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].0, 1);
        assert_eq!(records[1].0, 3);
    }

    #[test]
    fn clear_drops_retained_state_and_parser_position() {
        let mut retained = RetainedHistory::new(RetentionBudget::default());
        retained.push(1, b"text\x1b_Ga=T;AAA");
        retained.clear();
        retained.push(2, b"fresh\x1b\\");
        assert!(retained.is_empty() || history(&retained).starts_with(b"fresh"));
        assert_eq!(retained.image_bytes(), 0);
    }
}
