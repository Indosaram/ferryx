//! Retained reconstruction history: separate text and image budgets over whole VT units.
//!
//! This is deliberately *not* the recovery ring. The ring stays byte-oriented for
//! incremental (`after_sequence`) replay and machine transport accounting; this structure
//! keeps the longer, protocol-safe history a restarted GUI replays from scratch.

use std::collections::VecDeque;

use super::vt_state::TerminalStateRecorder;
use super::vt_stream::{Pending, Unit, UnitKind, VtUnitSplitter};

pub const DEFAULT_TEXT_RETENTION_BYTES: usize = 512 * 1024;
/// Line cap beside the byte caps: 512 KiB of one-character lines is hundreds of thousands of
/// lines, and a reconstruction no one can read is not worth the memory. Lines are counted as
/// newlines in retained text, so a pathological session is bounded by both budgets.
pub const DEFAULT_TEXT_RETENTION_LINES: usize = 5_000;
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

    /// Splits one total ceiling between text and images in the same ratio as
    /// [`Self::from_text_capacity`], so the session's retained bytes (`text + images`) stay
    /// within `total_bytes`. Used where the ceiling is a share of the daemon-wide budget rather
    /// than a text capacity.
    pub fn from_total(total_bytes: usize) -> Self {
        let divisor = IMAGE_RETENTION_RATIO + 1;
        let image_bytes = total_bytes / divisor;
        Self {
            text_bytes: total_bytes - image_bytes,
            image_bytes,
        }
    }

    fn max_unit_bytes(&self) -> usize {
        self.image_bytes
            .max(self.text_bytes)
            .clamp(64, MAX_RETAINED_GROUP_BYTES)
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
    /// Newlines in `bytes`, so the line budget is evicted without rescanning. Images never
    /// carry lines: their own budget bounds them.
    lines: usize,
    open: bool,
    image_group: Option<u64>,
}
pub struct RetainedHistory {
    budget: RetentionBudget,
    max_text_lines: usize,
    splitter: VtUnitSplitter,
    evicted_state: TerminalStateRecorder,
    text: VecDeque<Record>,
    images: VecDeque<Record>,
    text_bytes: usize,
    text_lines: usize,
    head_evicted: bool,
    image_bytes: usize,
    units: Vec<Unit>,
}

impl RetainedHistory {
    pub fn new(budget: RetentionBudget) -> Self {
        Self::with_text_line_cap(budget, DEFAULT_TEXT_RETENTION_LINES)
    }

    /// [`Self::new`] with the line cap chosen by the caller, so retention is bounded by the
    /// newest `text_lines` lines as well as by the byte budgets.
    pub fn with_text_line_cap(budget: RetentionBudget, text_lines: usize) -> Self {
        Self {
            splitter: VtUnitSplitter::new(budget.max_unit_bytes(), budget.text_block_bytes()),
            budget,
            max_text_lines: text_lines,
            evicted_state: TerminalStateRecorder::new(),
            text: VecDeque::new(),
            images: VecDeque::new(),
            text_bytes: 0,
            text_lines: 0,
            head_evicted: false,
            image_bytes: 0,
            units: Vec::new(),
        }
    }

    pub fn budget(&self) -> RetentionBudget {
        self.budget
    }

    /// Replaces the budget and evicts immediately to fit it, preserving the text/image ratio.
    /// The splitter keeps the unit sizes it was built with: its pending bytes must not be
    /// reclassified mid-unit on a budget change, and shrinking only lowers the coalescing
    /// granularity of records retained afterwards.
    pub fn set_budget(&mut self, budget: RetentionBudget) {
        if budget == self.budget {
            return;
        }
        self.budget = budget;
        self.evict();
    }

    /// Re-bounds the text budget, keeping [`IMAGE_RETENTION_RATIO`] for images, and evicts to fit.
    pub fn set_text_budget(&mut self, text_bytes: usize) {
        self.set_budget(RetentionBudget::from_text_capacity(text_bytes));
    }

    /// Trailing prefix (plus any parser resync bytes) of output whose terminator has not
    /// arrived, with the sequence it started at so resize segmentation can place it.
    pub fn pending(&self) -> Pending {
        self.splitter.pending()
    }

    pub fn state_prelude(&self) -> Vec<u8> {
        self.evicted_state.prelude().encode()
    }

    /// Structured form of the accumulated eviction state, carried across export/import so a
    /// restored session reconstructs the same prelude its predecessor would have produced.
    pub fn state(&self) -> super::vt_state::TerminalStatePrelude {
        self.evicted_state.prelude()
    }

    /// Layer an exported prelude back in as the base eviction state after re-seeding records.
    pub fn restore_state(&mut self, prelude: super::vt_state::TerminalStatePrelude) {
        self.evicted_state.restore(prelude);
    }

    /// Full protocol-safe reconstruction: state prelude, retained units in stream order,
    /// then the in-flight prefix. Replaying this into a fresh parser reproduces the pane.
    ///
    /// Two defects of a reconstructed head are repaired here, on the replay boundary:
    ///
    /// * an incomplete UTF-8 prefix left by an eviction cut at a non-character boundary is
    ///   dropped rather than emitted as a replacement-character head, and
    /// * device queries are stripped, because a parser that consumes this replay would answer
    ///   them into the session's live input ([`strip_replay_queries`]).
    ///
    /// Only [`Self::replay`] cleans; the stored records stay byte-exact for callers that
    /// address them by sequence.
    pub fn replay(&self) -> Vec<u8> {
        let prelude = self.state_prelude();
        let pending = self.pending();
        let mut body = Vec::with_capacity(self.len() + pending.bytes.len());
        for (_, bytes) in self.records_with_pending(&pending) {
            body.extend_from_slice(bytes);
        }
        // The prelude re-establishes attributes and is never a query, so cleaning runs on the
        // body alone and cannot disturb it. Cleaning here - at the replay boundary - leaves the
        // stored records byte-exact for callers that address them by sequence.
        let skip = if self.head_evicted { incomplete_utf8_head(&body) } else { 0 };
        if skip > 0 {
            body.drain(..skip);
        }
        strip_replay_queries(&mut body);
        let mut out = Vec::with_capacity(prelude.len() + body.len());
        out.extend_from_slice(&prelude);
        out.extend_from_slice(&body);
        out
    }

    pub fn records_with_pending<'a>(&'a self, pending: &'a Pending) -> Vec<(u64, &'a [u8])> {
        let mut records: Vec<_> = self.text.iter().chain(self.images.iter())
            .map(|record| (record.order, record.sequence, record.bytes.as_slice()))
            .collect();
        if pending.parts.is_empty() && !pending.bytes.is_empty() {
            let next_order = records.iter().map(|r| r.0).max().map_or(0, |m| m + 1);
            records.push((next_order, pending.sequence.unwrap_or(0), &pending.bytes[..]));
        } else {
            for (index, &(start, order, sequence)) in pending.parts.iter().enumerate() {
                let end = pending.parts.get(index + 1).map_or(pending.bytes.len(), |part| part.0);
                if start < end {
                    records.push((order, sequence, &pending.bytes[start..end]));
                }
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
        self.text_lines = 0;
        self.head_evicted = false;
        self.image_bytes = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.images.is_empty()
    }

    pub fn len(&self) -> usize {
        self.text_bytes + self.image_bytes
    }

    /// Retained bytes plus the unit still being assembled. The in-flight prefix is real memory the
    /// session holds, so the budget charges it: otherwise a stream without terminators could sit
    /// just under any ceiling with an uncharged group behind it.
    ///
    /// At rest this equals [`Self::len`], because the splitter flushes its text buffer whenever a
    /// push ends in the ground state - which is every push a well-behaved program produces after a
    /// completed sequence. A push that ends mid-control-sequence or mid-string leaves that prefix
    /// in flight, and it cannot be evicted: it is the unit being assembled, not a retained record.
    /// The prefix is bounded by the splitter's `max_unit_bytes`, which is
    /// `max(text, image).clamp(64, MAX_RETAINED_GROUP_BYTES)` - at most `16/17` of a session
    /// ceiling derived by [`RetentionBudget::from_total`]. So a session's retained bytes stay at
    /// or below its ceiling at rest, and stay below twice its ceiling in the worst case where one
    /// unterminated unit is outstanding. The aggregate is therefore inside the daemon budget in
    /// the state every published-then-idle session is in, which is the state the footprint is
    /// measured in; the outstanding unit is transient and bounded per session.
    pub fn retained_bytes(&self) -> usize {
        self.len() + self.splitter.pending_len()
    }

    pub fn text_bytes(&self) -> usize {
        self.text_bytes
    }

    /// Newlines currently retained in the text timeline.
    pub fn text_lines(&self) -> usize {
        self.text_lines
    }

    /// The line cap this history evicts against.
    pub fn max_text_lines(&self) -> usize {
        self.max_text_lines
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
                        lines: 0,
                        open: false,
                        image_group: Some(unit.order),
                    });
                }
                self.seal();
            }
            UnitKind::Text => {
                let lines = unit.bytes.iter().filter(|byte| **byte == b'\n').count();
                self.text_bytes += unit.bytes.len();
                self.text_lines += lines;
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
                        record.lines += lines;
                        return;
                    }
                }
                self.text.push_back(Record {
                    order: unit.order,
                    sequence: unit.sequence,
                    bytes: unit.bytes,
                    lines,
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
                    self.text_lines -= record.lines;
                    self.evicted_state.observe(&record.bytes);
                    self.head_evicted = true;
                }
                None => break,
            }
        }
        // Line budget beside the byte budgets: whole front records leave, the same shape the
        // byte loop uses, so a short-line flood cannot pin an unbounded number of lines.
        while self.text_lines > self.max_text_lines {
            match self.text.pop_front() {
                Some(record) => {
                    self.text_bytes -= record.bytes.len();
                    self.text_lines -= record.lines;
                    self.evicted_state.observe(&record.bytes);
                    self.head_evicted = true;
                }
                None => break,
            }
        }
        while self.image_bytes > self.budget.image_bytes {
            match self.images.pop_front() {
                Some(record) => {
                    self.image_bytes -= record.bytes.len();
                    let group = record.image_group;
                    // retain_unit appends all parts of a completed image group together.
                    // Only its consecutive front records can share this group: scanning
                    // every surviving image here makes steady-state eviction quadratic.
                    while self.images.front().is_some_and(|part| part.image_group == group) {
                        let part = self.images.pop_front().expect("front image part exists");
                        self.image_bytes -= part.bytes.len();
                    }
                }
                None => break,
            }
        }
    }
}

/// Removes device queries from replayed bytes, in place.
pub fn strip_replay_queries(bytes: &mut Vec<u8>) {
    let spans = query_spans(bytes);
    if spans.is_empty() {
        return;
    }
    let removed: usize = spans.iter().map(|(start, end)| end - start).sum();
    let mut out = Vec::with_capacity(bytes.len() - removed);
    let mut cursor = 0;
    for (start, end) in spans {
        if start > cursor {
            out.extend_from_slice(&bytes[cursor..start]);
        }
        cursor = end;
    }
    if cursor < bytes.len() {
        out.extend_from_slice(&bytes[cursor..]);
    }
    *bytes = out;
}

/// Half-open `(start, end)` byte spans of the device queries in `bytes`, ascending.
///
/// Replay-only interrogation is suppressed, but cursor-position requests (`CSI 6 n` and
/// `CSI ? 6 n`) must reach the terminal emulator: ConPTY can be waiting for that answer before
/// the shell starts. Printed responses are program output and are never matched.
///
/// One unit at a time is enough because the splitter never splits a complete sequence across
/// two text units: it flushes pending text when a string introducer begins, and emits a string
/// only once its terminator has arrived. An unterminated sequence at the end is the in-flight
/// prefix, which the live continuation completes, so it is left alone here.
pub fn query_spans(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0x1b {
            index += 1;
            continue;
        }
        match bytes.get(index + 1) {
            Some(b'[') => {
                let start = index + 2;
                let mut cursor = start;
                while cursor < bytes.len() && !(0x40..=0x7e).contains(&bytes[cursor]) {
                    cursor += 1;
                }
                let Some(&final_byte) = bytes.get(cursor) else { break };
                let region = &bytes[start..cursor];
                // Each arm matches the query but not the reply, because replies are a
                // program's own output and stripping them would corrupt the picture: a DSR
                // reply ends in `R`, a DA reply carries parameters before its `c`, a kitty
                // flags reply carries the flags, a DECRQM reply ends in `y`.
                let query = match final_byte {
                    // Cursor-position requests must reach the terminal emulator: ConPTY
                    // waits for its answer before starting the shell. Other DSR queries
                    // are replay-only device interrogation and must not be re-echoed.
                    b'n' => !matches!(region, b"6" | b"?6"),
                    // DA: `ESC [ c`, `ESC [ > c`, `ESC [ = c` - a reply has parameters.
                    b'c' => !region.iter().any(|byte| byte.is_ascii_digit()),
                    // Kitty keyboard flags query is exactly `ESC [ ? u`.
                    b'u' => region == b"?".as_slice(),
                    // DECRQM: `$` only appears in the query, never in the `$y` reply.
                    b'p' => region.contains(&b'$'),
                    // XTVERSION: `ESC [ > 0 q`; DECLL `ESC [ 0 q` is not a query.
                    b'q' => region.first() == Some(&b'>'),
                    _ => false,
                };
                if query {
                    spans.push((index, cursor + 1));
                }
                index = cursor + 1;
            }
            Some(b']' | b'P' | b'_' | b'^' | b'X') => {
                let introducer = bytes[index + 1];
                let start = index + 2;
                let mut cursor = start;
                let mut end = None;
                while cursor < bytes.len() {
                    // BEL terminates an OSC only; in DCS, APC, PM and SOS it is payload.
                    if bytes[cursor] == 0x07 && introducer == b']' {
                        end = Some(cursor + 1);
                        break;
                    }
                    if bytes[cursor] == 0x1b && bytes.get(cursor + 1) == Some(&b'\\') {
                        end = Some(cursor + 2);
                        break;
                    }
                    cursor += 1;
                }
                let Some(end) = end else { break };
                let payload = &bytes[start..cursor];
                let query = match introducer {
                    b']' => is_osc_query(payload),
                    // DECRQSS and XTGETTCAP are queries; every other DCS passes through.
                    b'P' => payload.starts_with(b"$q") || payload.starts_with(b"+q"),
                    // APC, PM and SOS carry image payloads, never a query.
                    _ => false,
                };
                if query {
                    spans.push((index, end));
                }
                index = end;
            }
            Some(_) => index += 2,
            None => break,
        }
    }
    spans
}

/// Sequence resumes belong to an existing parser, even when the snapshot reports a gap.
/// Only an explicitly full reconstruction can discard an evicted UTF-8 head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplayCleanup {
    Full { head_evicted: bool },
    Delta,
}

impl ReplayCleanup {
    pub(crate) fn for_request(after_sequence: Option<u64>, start_sequence: Option<u64>) -> Self {
        match after_sequence {
            Some(_) => Self::Delta,
            None => Self::Full {
                head_evicted: start_sequence.is_some_and(|sequence| sequence > 1),
            },
        }
    }
}

/// Returns cleaned bytes and `(original_start, removed_len)` spans for offset remapping.
pub(crate) fn clean_replay_bytes(
    bytes: &[u8],
    cleanup: ReplayCleanup,
) -> (Vec<u8>, Vec<(usize, usize)>) {
    // A resumed parser owns the preceding bytes, including partial UTF-8 and VT sequences.
    if cleanup == ReplayCleanup::Delta {
        return (bytes.to_vec(), Vec::new());
    }
    let head = if cleanup == (ReplayCleanup::Full { head_evicted: true }) {
        incomplete_utf8_head(bytes)
    } else {
        0
    };
    let mut removed: Vec<(usize, usize)> = Vec::new();
    if head > 0 {
        removed.push((0, head));
    }
    let mut cleaned = bytes[head..].to_vec();
    for (start, end) in query_spans(&cleaned) {
        removed.push((head + start, end - start));
    }
    strip_replay_queries(&mut cleaned);
    (cleaned, removed)
}

/// OSC payloads that ask the terminal a question and expect it to write an answer back.
fn is_osc_query(payload: &[u8]) -> bool {
    // Dynamic colours (10/11/12), palette (4), clipboard (52) and the OSC 7501 support query
    // all put `?` in their parameters.
    const PREFIXES: [&[u8]; 5] = [b"10;?", b"11;?", b"12;?", b"52;?", b"7501;?"];
    PREFIXES.iter().any(|prefix| payload.starts_with(prefix))
        || (payload.starts_with(b"4;") && payload.ends_with(b";?"))
}

/// Bytes to skip so a reconstruction does not start inside a multi-byte code point.
///
/// Eviction cuts the head at a record boundary, and a unit boundary is a flush or escape
/// boundary rather than a character boundary, so the surviving head can open with the tail of a
/// character whose lead byte was evicted. Two shapes are possible, and a replay must drop both:
///
/// * leading continuation bytes (`0b10xxxxxx`) with no lead byte, and
/// * a lead byte whose continuations were cut off, which is a truncated sequence when the next
///   byte is not a continuation or the buffer ends mid-sequence.
///
/// Returns 0 when the head starts on a character boundary, which is the usual case.
pub fn incomplete_utf8_head(bytes: &[u8]) -> usize {
    let Some(&first) = bytes.first() else { return 0 };
    if first & 0xc0 == 0x80 {
        // Orphaned continuation bytes: at most three, since a character is at most four bytes.
        return bytes
            .iter()
            .take(3)
            .take_while(|byte| **byte & 0xc0 == 0x80)
            .count();
    }
    let expected = match first {
        0x00..=0x7f => return 0,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        // A continuation-only or invalid lead byte: drop it, it cannot start a character.
        _ => return 1,
    };
    let continuations = bytes
        .iter()
        .skip(1)
        .take(expected - 1)
        .take_while(|byte| **byte & 0xc0 == 0x80)
        .count();
    if continuations == expected - 1 {
        // The character is complete; the head is on a boundary.
        0
    } else {
        // Truncated sequence: the lead byte and whatever continuations survived are unusable.
        1 + continuations
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
    fn eviction_removes_whole_front_groups_and_preserves_later_images_and_text() {
        let first = b"\x1b_Ga=T,m=1;AAAA\x1b\\";
        let last = b"\x1b_Gm=0;BBBB\x1b\\";
        let second = b"\x1b_Ga=p,i=2;\x1b\\";
        let third = b"\x1b_Ga=p,i=3;\x1b\\";
        let mut retained = RetainedHistory::new(RetentionBudget {
            text_bytes: 4096,
            image_bytes: first.len() + last.len() + second.len(),
        });
        retained.push(1, first);
        retained.push(2, b"INTERLEAVED\r\n");
        retained.push(3, last);
        retained.push(4, second);
        assert_eq!(retained.image_bytes(), first.len() + last.len() + second.len());

        // Exceeding the budget by one small command removes the entire first multipart
        // group, not merely its first part, and leaves both later groups in stream order.
        retained.push(5, third);
        assert_eq!(retained.image_bytes(), second.len() + third.len());
        assert_eq!(retained.replay(), [b"INTERLEAVED\r\n".as_slice(), second, third].concat());

        // One push may evict several groups. The replacement image fits the budget alone.
        let replacement = kitty(retained.budget().image_bytes - kitty(0).len());
        retained.push(6, &replacement);
        assert_eq!(retained.image_bytes(), replacement.len());
        assert_eq!(retained.replay(), [b"INTERLEAVED\r\n".as_slice(), &replacement].concat());
    }

    #[test]
    fn reseeded_history_keeps_image_groups_atomic_during_eviction() {
        let first = b"\x1b_Ga=T,m=1;AAAA\x1b\\";
        let last = b"\x1b_Gm=0;BBBB\x1b\\";
        let later = b"\x1b_Ga=p,i=2;\x1b\\";
        let budget = RetentionBudget {
            text_bytes: 4096,
            image_bytes: first.len() + last.len() + later.len(),
        };
        let mut original = RetainedHistory::new(budget);
        original.push(1, first);
        original.push(2, b"BETWEEN");
        original.push(3, last);
        original.push(4, later);
        // Snapshot restoration also reconstructs retention through push, never by
        // inserting individual records into its private image queue.
        let mut restored = RetainedHistory::new(budget);
        for (sequence, bytes) in original.records() {
            restored.push(sequence, bytes);
        }
        restored.restore_state(original.state());
        assert_eq!(restored.replay(), original.replay());
        restored.push(5, later);
        original.push(5, later);
        assert_eq!(restored.image_bytes(), 2 * later.len());
        assert_eq!(restored.replay(), original.replay());
        assert_eq!(restored.replay(), [b"BETWEEN".as_slice(), later, later].concat());
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
        retained.push(1, b"after");
        let records = retained.records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].0, 1);
        assert_eq!(records[0].1, b"before");
        assert_eq!(records[1].0, 1);
        assert_eq!(records[1].1, b"after");
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

    #[test]
    fn csi_discard_with_empty_text_retains_introducer_in_replay() {
        let budget = RetentionBudget {
            text_bytes: 64,
            image_bytes: 64,
        };
        let max_unit = budget.max_unit_bytes();
        let mut retained = RetainedHistory::new(budget);
        let mut oversized_csi = b"\x1b[".to_vec();
        oversized_csi.extend(std::iter::repeat_n(b'1', max_unit + 1));
        retained.push(1, &oversized_csi);

        let replayed = retained.replay();
        assert!(
            replayed.ends_with(b"\x1b[") || replayed.windows(2).any(|w| w == b"\x1b["),
            "csi discard introducer must be preserved in replay, got: {:?}",
            replayed
        );
    }

    #[test]
    fn line_cap_evicts_oldest_lines_while_the_byte_budget_is_unreached() {
        // Given: a byte budget large enough for far more than the line cap, and one-character
        // lines, which is exactly the shape a byte-only cap cannot bound.
        let mut retained = RetainedHistory::with_text_line_cap(
            RetentionBudget {
                text_bytes: 1024 * 1024,
                image_bytes: 4096,
            },
            10,
        );

        // When: 100 one-character lines arrive, nowhere near the byte budget.
        for line in 0..100u32 {
            retained.push(line as u64 + 1, format!("{}\r\n", (b'a' + (line % 26) as u8) as char).as_bytes());
        }

        // Then: retention holds the newest lines only, and the byte budget never bound it.
        assert_eq!(retained.text_lines(), 10);
        assert!(retained.text_bytes() < 100, "byte budget was never the binding constraint");
        let replayed = retained.replay();
        assert!(replayed.ends_with(b"v\r\n"), "newest line must survive: {:?}", String::from_utf8_lossy(&replayed));
        assert!(!replayed.starts_with(b"a\r\n"), "oldest line must be evicted");
    }

    #[test]
    fn line_cap_defaults_to_five_thousand_lines() {
        assert_eq!(DEFAULT_TEXT_RETENTION_LINES, 5_000);
        let retained = RetainedHistory::new(RetentionBudget::default());
        assert_eq!(retained.max_text_lines(), DEFAULT_TEXT_RETENTION_LINES);
    }

    #[test]
    fn byte_cap_still_evicts_when_the_line_cap_is_not_reached() {
        // A byte cap must keep working: the line cap is an addition, not a replacement.
        let mut retained = RetainedHistory::with_text_line_cap(
            RetentionBudget {
                text_bytes: 64,
                image_bytes: 4096,
            },
            10_000,
        );
        for sequence in 0..20u64 {
            retained.push(sequence + 1, b"0123456789\r\n");
        }
        assert!(retained.text_lines() < 20, "byte cap stopped evicting");
        assert!(retained.text_bytes() <= 64);
    }

    #[test]
    fn replay_strips_device_queries_but_keeps_printed_responses() {
        // Given: history holding a cursor query, a device-attributes query and a colour query,
        // plus a response a program printed itself.
        let mut retained = RetainedHistory::new(RetentionBudget::default());
        retained.push(1, b"prompt$ \x1b[6n\x1b[c\x1b]11;?\x07");
        retained.push(2, b"\x1b[1;1Rvisible\r\n");

        // Then: every query is gone, and the printed response stays.
        let replayed = retained.replay();
        assert!(replayed.windows(4).any(|w| w == b"\x1b[6n"), "emulator cursor query stripped");
        assert!(!replayed.windows(3).any(|w| w == b"\x1b[c"), "device attributes query replayed");
        assert!(!replayed.windows(6).any(|w| w == b"\x1b]11;?"), "colour query replayed");
        assert!(replayed.windows(6).any(|w| w == b"\x1b[1;1R"), "printed response was stripped");
        assert!(replayed.windows(7).any(|w| w == b"visible"));
    }

    #[test]
    fn replay_cleanup_distinguishes_full_evicted_heads_from_sequence_resumes() {
        let bytes = b"\x82\xac\x1b[5n\x1b[6n\x1b[?6n";
        let delta = ReplayCleanup::for_request(Some(1), Some(2));
        assert_eq!(delta, ReplayCleanup::Delta);
        assert_eq!(clean_replay_bytes(bytes, delta), (bytes.to_vec(), Vec::new()));
        let full = ReplayCleanup::for_request(None, Some(2));
        assert_eq!(full, ReplayCleanup::Full { head_evicted: true });
        assert_eq!(clean_replay_bytes(bytes, full).0, b"\x1b[6n\x1b[?6n");
        assert_eq!(clean_replay_bytes(b"\xe2", ReplayCleanup::for_request(None, Some(1))).0, b"\xe2");
    }

    #[test]
    fn replay_keeps_an_unterminated_in_flight_prefix_untouched() {
        // A query cut off mid-sequence is the live continuation, not history: stripping it would
        // make the next live chunk complete the wrong thing.
        let mut retained = RetainedHistory::new(RetentionBudget::default());
        retained.push(1, b"text\x1b[6");
        assert_eq!(retained.replay(), b"text\x1b[6");
    }

    #[test]
    fn replay_drops_an_incomplete_utf8_head_instead_of_mangling_it() {
        // Given: the lead byte of a two-byte character sits in the oldest record and that record
        // is evicted, so the surviving head opens on its orphaned continuation byte.
        let mut retained = RetainedHistory::new(RetentionBudget {
            text_bytes: 8,
            image_bytes: 4096,
        });
        retained.push(1, b"\xc3\xa9");
        retained.push(2, b"\xa9 tail\r\n");
        assert!(retained.text_bytes() <= 8, "the eviction this test relies on did not happen");

        // Then: the replay opens on the surviving character, not on a replacement glyph.
        let replayed = retained.replay();
        assert!(
            !replayed.first().is_some_and(|byte| byte & 0xc0 == 0x80),
            "replay opens on an orphaned continuation byte: {:?}",
            replayed
        );
        assert!(replayed.windows(6).any(|w| w == b" tail\r"), "body damaged: {:?}", replayed);
    }

    #[test]
    fn incomplete_utf8_head_only_skips_a_partial_character() {
        assert_eq!(incomplete_utf8_head(b"plain ascii"), 0);
        assert_eq!(incomplete_utf8_head("é".as_bytes()), 0, "complete 2-byte char");
        assert_eq!(incomplete_utf8_head("€".as_bytes()), 0, "complete 3-byte char");
        assert_eq!(incomplete_utf8_head("😀".as_bytes()), 0, "complete 4-byte char");
        // Orphaned continuations, as left by a cut after the lead byte.
        assert_eq!(incomplete_utf8_head(&[0xa9, b'o', b'k']), 1);
        assert_eq!(incomplete_utf8_head(&[0x82, 0xac, b'o']), 2);
        // A lead byte whose continuations were cut off.
        assert_eq!(incomplete_utf8_head(&[0xe2, 0x82]), 2, "truncated 3-byte sequence");
        assert_eq!(incomplete_utf8_head(&[0xf0]), 1, "lone 4-byte lead");
    }

    #[test]
    fn replay_strips_a_query_at_the_very_start_of_history() {
        // The strip must not depend on the query sitting mid-buffer.
        let mut retained = RetainedHistory::with_text_line_cap(
            RetentionBudget {
                text_bytes: 4096,
                image_bytes: 4096,
            },
            4,
        );
        retained.push(1, b"\x1b[6nold\r\n");
        retained.push(2, b"kept\r\n");
        retained.push(3, b"newest\r\n");
        let replayed = retained.replay();
        assert!(replayed.windows(4).any(|w| w == b"\x1b[6n"));
        assert!(replayed.windows(7).any(|w| w == b"newest\r"));
    }
}
