//! Byte-stream splitting into whole VT units so history retention never cuts a sequence.
//!
//! Three invariants make eviction protocol-safe:
//! 1. A unit is emitted only when its terminator has been seen.
//! 2. When a unit is too large to retain, the splitter does not "forget" that the parser is
//!    inside a string: it keeps a resync prefix (the introducer) so a replayed snapshot puts
//!    the receiving parser back into the same consume state, and it stays in a discard state
//!    until the real terminator arrives. Nothing is abandoned on a byte threshold.
//! 3. Every unit carries the stream order and the sequence of its FIRST byte, so retention
//!    and resize segmentation attribute it to where it started, not where it finished.

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const OSC_IMAGE_CLASSIFY_BYTES: usize = 19;
const DCS_CLASSIFY_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Text,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub kind: UnitKind,
    pub bytes: Vec<u8>,
    pub order: u64,
    pub sequence: u64,
    pub parts: Vec<(usize, u64, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub bytes: Vec<u8>,
    pub sequence: Option<u64>,
    pub parts: Vec<(usize, u64, u64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scan {
    Ground,
    Escape,
    Csi,
    CsiDiscard,
    String,
    StringEscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Continuation {
    Standalone,
    Opens,
    Continues,
    Closes,
}

/// Kitty graphics groups a transmission with `m=1` on every part except the last (`m=0`).
/// A part carrying no `m` key is a self-contained command.
fn kitty_continuation(payload: &[u8]) -> Continuation {
    if payload.first() != Some(&b'G') {
        return Continuation::Standalone;
    }
    let control = match payload.iter().position(|b| *b == b';') {
        Some(end) => &payload[1..end],
        None => &payload[1..],
    };
    let mut more: Option<bool> = None;
    for key_value in control.split(|b| *b == b',') {
        let mut parts = key_value.splitn(2, |b| *b == b'=');
        if parts.next() == Some(b"m") {
            more = Some(parts.next() == Some(b"1"));
        }
    }
    match more {
        Some(true) => Continuation::Opens,
        Some(false) => Continuation::Closes,
        None => Continuation::Standalone,
    }
}

fn iterm_continuation(payload: &[u8]) -> Continuation {
    if payload.starts_with(b"1337;MultipartFile=") {
        Continuation::Opens
    } else if payload.starts_with(b"1337;FilePart=") {
        Continuation::Continues
    } else if payload.starts_with(b"1337;FileEnd") {
        Continuation::Closes
    } else {
        Continuation::Standalone
    }
}

/// A multi-part image transmission is atomic for retention. `Discarding` is entered when the
/// chain cannot be retained; it persists until the chain's real terminator so the splitter
/// never re-opens a transmission in the middle of its tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transmission {
    Idle,
    Open,
    Discarding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mark {
    order: u64,
    sequence: u64,
}

pub struct VtUnitSplitter {
    scan: Scan,
    text: Vec<u8>,
    string: Vec<u8>,
    introducer: u8,
    kind: UnitKind,
    classified: bool,
    poisoned: bool,
    discarded_role: Option<Continuation>,
    max_unit_bytes: usize,
    text_flush_bytes: usize,
    transmission: Transmission,
    group: Vec<u8>,
    csi_start: usize,
    next_order: u64,
    sequence: u64,
    text_mark: Option<Mark>,
    string_mark: Option<Mark>,
    group_mark: Option<Mark>,
    group_parts: Vec<(usize, u64, u64)>,
}

impl VtUnitSplitter {
    pub fn new(max_unit_bytes: usize, text_flush_bytes: usize) -> Self {
        Self {
            scan: Scan::Ground,
            text: Vec::new(),
            string: Vec::new(),
            introducer: 0,
            kind: UnitKind::Text,
            classified: true,
            poisoned: false,
            discarded_role: None,
            max_unit_bytes: max_unit_bytes.max(64),
            text_flush_bytes,
            transmission: Transmission::Idle,
            group: Vec::new(),
            csi_start: 0,
            next_order: 0,
            sequence: 0,
            text_mark: None,
            string_mark: None,
            group_mark: None,
            group_parts: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        let preserved_order = self.next_order;
        *self = Self::new(self.max_unit_bytes, self.text_flush_bytes);
        self.next_order = preserved_order;
    }

    /// Bytes consumed but not yet emitted, plus any resync prefix required to keep the
    /// receiving parser in the same consume state. Replaying this after retained history
    /// guarantees the live continuation cannot leak payload bytes as visible text.
    pub fn pending(&self) -> Pending {
        let mut bytes = Vec::new();
        let mut sequence = None;
        let mut parts = Vec::new();
        if self.transmission == Transmission::Open && !self.group.is_empty() {
            bytes.extend_from_slice(&self.group);
            sequence = self.group_mark.map(|mark| mark.sequence);
            parts.extend_from_slice(&self.group_parts);
        }
        let mark = match self.scan {
            Scan::String | Scan::StringEscape => self.string_mark,
            _ => self.text_mark,
        };
        if let Some(mark) = mark {
            parts.push((bytes.len(), mark.order, mark.sequence));
        }
        match self.scan {
            Scan::String | Scan::StringEscape => {
                bytes.extend_from_slice(&self.string);
                if self.poisoned && self.scan == Scan::StringEscape {
                    bytes.push(ESC);
                }
                sequence = sequence.or(self.string_mark.map(|mark| mark.sequence));
            }
            Scan::CsiDiscard => {
                bytes.extend_from_slice(&self.text);
                // The discarded parameters are gone, but the introducer must be replayed so
                // the receiver consumes the live tail through the real final byte.
                bytes.extend_from_slice(b"\x1b[");
                sequence = sequence.or(self.text_mark.map(|mark| mark.sequence));
            }
            _ => {
                bytes.extend_from_slice(&self.text);
                sequence = sequence.or(self.text_mark.map(|mark| mark.sequence));
            }
        }
        Pending { bytes, sequence, parts }
    }

    pub fn has_pending(&self) -> bool {
        !self.group.is_empty()
            || !self.string.is_empty()
            || !self.text.is_empty()
            || self.scan == Scan::CsiDiscard
    }

    pub fn push(&mut self, sequence: u64, bytes: &[u8], out: &mut Vec<Unit>) {
        self.sequence = sequence;
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            match self.scan {
                Scan::Ground => {
                    self.push_text(byte);
                    if byte == ESC {
                        self.scan = Scan::Escape;
                    }
                    index += 1;
                }
                Scan::Escape => {
                    match byte {
                        ESC => self.push_text(ESC),
                        b'_' | b'P' | b']' | b'^' | b'X' => {
                            self.text.pop();
                            if self.text.is_empty() {
                                self.text_mark = None;
                            }
                            self.flush_text(out);
                            self.begin_string(byte);
                        }
                        _ => {
                            self.push_text(byte);
                            if byte == b'[' {
                                self.csi_start = self.text.len().saturating_sub(2);
                                self.scan = Scan::Csi;
                            } else {
                                self.scan = Scan::Ground;
                                self.flush_text_when_large(out);
                            }
                        }
                    }
                    index += 1;
                }
                Scan::Csi => {
                    if byte == ESC {
                        self.push_text(ESC);
                        self.scan = Scan::Escape;
                    } else {
                        self.push_text(byte);
                        if (0x40..=0x7e).contains(&byte) {
                            self.scan = Scan::Ground;
                            self.flush_text_when_large(out);
                        } else if self.text.len() - self.csi_start > self.max_unit_bytes {
                            // An unterminated control sequence must not grow without bound.
                            // Drop its bytes and resync: `pending` replays `\x1b[` so the
                            // receiver discards the live tail through the real final byte.
                            self.text.truncate(self.csi_start);
                            if self.text.is_empty() {
                                self.text_mark = None;
                            }
                            self.scan = Scan::CsiDiscard;
                        }
                    }
                    index += 1;
                }
                Scan::CsiDiscard => {
                    if byte == ESC {
                        self.push_text(ESC);
                        self.scan = Scan::Escape;
                    } else if (0x40..=0x7e).contains(&byte) {
                        self.scan = Scan::Ground;
                    }
                    index += 1;
                }
                Scan::String => {
                    if byte == ESC {
                        self.push_string(ESC);
                        self.scan = Scan::StringEscape;
                    } else if byte == BEL && self.introducer == b']' {
                        self.push_string(BEL);
                        self.finish_string(out);
                    } else {
                        self.push_string(byte);
                        self.classify();
                    }
                    index += 1;
                }
                Scan::StringEscape => {
                    if byte == b'\\' {
                        self.push_string(b'\\');
                        self.finish_string(out);
                        index += 1;
                    } else {
                        // ESC + non-ST cancels the string; re-dispatch the byte as an escape.
                        self.string.clear();
                        self.string_mark = None;
                        self.poisoned = false;
                        self.classified = true;
                        self.scan = Scan::Escape;
                        self.push_text(ESC);
                    }
                }
            }
        }
        if self.scan == Scan::Ground {
            self.flush_text(out);
        }
    }

    fn allocate(&mut self) -> Mark {
        let order = self.next_order;
        self.next_order += 1;
        Mark {
            order,
            sequence: self.sequence,
        }
    }

    fn push_text(&mut self, byte: u8) {
        if self.text.is_empty() {
            self.text_mark = Some(self.allocate());
        }
        self.text.push(byte);
    }

    fn begin_string(&mut self, introducer: u8) {
        self.introducer = introducer;
        self.kind = UnitKind::Text;
        self.classified = introducer == b'^' || introducer == b'X';
        self.poisoned = false;
        self.discarded_role = None;
        self.string = vec![ESC, introducer];
        self.string_mark = Some(self.allocate());
        self.scan = Scan::String;
    }

    fn push_string(&mut self, byte: u8) {
        if self.poisoned {
            return;
        }
        if self.string.len() >= self.max_unit_bytes {
            self.discarded_role = Some(self.adjusted_role(&self.string, false));
            self.poisoned = true;
            // Keep the introducer: it is the resync prefix that keeps a replayed snapshot
            // and the live stream in the same parser state. Dropping it here is what makes
            // raw payload bytes surface as text after an attach.
            self.string.truncate(2);
            return;
        }
        self.string.push(byte);
    }

    fn finish_string(&mut self, out: &mut Vec<Unit>) {
        let bytes = std::mem::take(&mut self.string);
        let mark = self.string_mark.take();
        let dropped = self.poisoned;
        self.poisoned = false;
        self.classified = true;
        self.scan = Scan::Ground;
        if bytes.is_empty() {
            return;
        }
        let role = self.discarded_role.take().unwrap_or_else(|| self.adjusted_role(&bytes, dropped));
        match self.transmission {
            Transmission::Idle => match role {
                Continuation::Opens => {
                    self.group_mark = mark;
                    self.group_parts.clear();
                    if let Some(mark) = mark {
                        self.group_parts.push((0, mark.order, mark.sequence));
                    }
                    if dropped {
                        self.transmission = Transmission::Discarding;
                        self.group = Vec::new();
                    } else {
                        self.transmission = Transmission::Open;
                        self.group = bytes;
                    }
                }
                Continuation::Continues | Continuation::Closes => {}
                Continuation::Standalone => self.emit(out, self.kind, bytes, mark, dropped),
            },
            Transmission::Open => match role {
                Continuation::Opens | Continuation::Continues => {
                    if let Some(mark) = mark {
                        self.group_parts.push((self.group.len(), mark.order, mark.sequence));
                    }
                    self.extend_group(&bytes, dropped);
                }
                Continuation::Closes => {
                    if let Some(mark) = mark {
                        self.group_parts.push((self.group.len(), mark.order, mark.sequence));
                    }
                    self.extend_group(&bytes, dropped);
                    self.close_group(out);
                }
                Continuation::Standalone => self.emit(out, self.kind, bytes, mark, dropped),
            },
            Transmission::Discarding => match role {
                Continuation::Closes => {
                    self.transmission = Transmission::Idle;
                    self.group_mark = None;
                }
                Continuation::Opens | Continuation::Continues => {}
                Continuation::Standalone => self.emit(out, self.kind, bytes, mark, dropped),
            },
        }
    }

    fn adjusted_role(&self, bytes: &[u8], dropped: bool) -> Continuation {
        let mut role = self.transmission_role(bytes);
        if self.kind != UnitKind::Image || !bytes.starts_with(b"\x1b_G") {
            return role;
        }
        match (self.transmission, role) {
            // A lone `m=0` carrying transmission metadata is a complete single-part image,
            // not the tail of a chain.
            (Transmission::Idle, Continuation::Closes) => {
                let control = bytes[3..]
                    .split(|byte| *byte == b';')
                    .next()
                    .unwrap_or_default();
                if control.split(|byte| *byte == b',').any(|field| {
                    field.starts_with(b"a=")
                        || field.starts_with(b"f=")
                        || field.starts_with(b"s=")
                        || field.starts_with(b"v=")
                }) {
                    role = Continuation::Standalone;
                }
            }
            // A chain whose sender omitted the final `m=0` marker still ends here.
            (Transmission::Open | Transmission::Discarding, Continuation::Standalone)
                if !dropped =>
            {
                role = Continuation::Closes;
            }
            _ => {}
        }
        role
    }

    fn emit(
        &mut self,
        out: &mut Vec<Unit>,
        kind: UnitKind,
        bytes: Vec<u8>,
        mark: Option<Mark>,
        dropped: bool,
    ) {
        if dropped || bytes.is_empty() {
            return;
        }
        let mark = mark.unwrap_or_else(|| self.allocate());
        out.push(Unit {
            kind,
            bytes,
            order: mark.order,
            sequence: mark.sequence,
            parts: Vec::new(),
        });
    }

    fn extend_group(&mut self, bytes: &[u8], dropped: bool) {
        if self.transmission != Transmission::Open {
            return;
        }
        if dropped || self.group.len() + bytes.len() > self.max_unit_bytes {
            self.transmission = Transmission::Discarding;
            self.group = Vec::new();
            self.group_parts.clear();
            return;
        }
        self.group.extend_from_slice(bytes);
    }

    fn close_group(&mut self, out: &mut Vec<Unit>) {
        let group = std::mem::take(&mut self.group);
        let parts = std::mem::take(&mut self.group_parts);
        let mark = self.group_mark.take();
        let discarded = self.transmission == Transmission::Discarding;
        self.transmission = Transmission::Idle;
        if discarded || group.is_empty() {
            return;
        }
        self.emit(out, UnitKind::Image, group, mark, false);
        if let Some(unit) = out.last_mut() {
            unit.parts = parts;
        }
    }

    fn transmission_role(&self, bytes: &[u8]) -> Continuation {
        if self.kind != UnitKind::Image && self.transmission == Transmission::Idle {
            return Continuation::Standalone;
        }
        if bytes.len() < 2 {
            return Continuation::Standalone;
        }
        match bytes[1] {
            b'_' => kitty_continuation(&bytes[2..]),
            b']' => iterm_continuation(&bytes[2..]),
            _ => Continuation::Standalone,
        }
    }

    fn flush_text(&mut self, out: &mut Vec<Unit>) {
        if self.text.is_empty() {
            return;
        }
        let bytes = std::mem::take(&mut self.text);
        let mark = self.text_mark.take();
        self.emit(out, UnitKind::Text, bytes, mark, false);
    }

    fn flush_text_when_large(&mut self, out: &mut Vec<Unit>) {
        if self.text.len() >= self.text_flush_bytes {
            self.flush_text(out);
        }
    }

    fn classify(&mut self) {
        if self.classified {
            return;
        }
        let payload = &self.string[2..];
        match self.introducer {
            b'_' => {
                self.kind = if payload.first() == Some(&b'G') {
                    UnitKind::Image
                } else {
                    UnitKind::Text
                };
                self.classified = true;
            }
            b'P' => {
                let final_byte = payload.last().copied().unwrap_or(0);
                if (0x40..=0x7e).contains(&final_byte) {
                    self.kind = if final_byte == b'q' {
                        UnitKind::Image
                    } else {
                        UnitKind::Text
                    };
                    self.classified = true;
                } else if payload.len() >= DCS_CLASSIFY_BYTES {
                    self.classified = true;
                }
            }
            b']' => {
                if payload.len() >= OSC_IMAGE_CLASSIFY_BYTES {
                    self.kind = if payload.starts_with(b"1337;File=")
                        || payload.starts_with(b"1337;MultipartFile=")
                    {
                        UnitKind::Image
                    } else {
                        UnitKind::Text
                    };
                    self.classified = true;
                }
            }
            _ => self.classified = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Harness {
        splitter: VtUnitSplitter,
        units: Vec<Unit>,
        sequence: u64,
    }

    impl Harness {
        fn new(max_unit: usize) -> Self {
            Self {
                splitter: VtUnitSplitter::new(max_unit, 64 * 1024),
                units: Vec::new(),
                sequence: 0,
            }
        }

        fn feed(&mut self, bytes: &[u8]) -> &mut Self {
            self.sequence += 1;
            let sequence = self.sequence;
            self.splitter.push(sequence, bytes, &mut self.units);
            self
        }

        fn pending(&self) -> Pending {
            self.splitter.pending()
        }

        fn retained(&self) -> Vec<u8> {
            self.units
                .iter()
                .flat_map(|unit| unit.bytes.clone())
                .collect()
        }
    }

    fn kitty_part(more: bool, payload: &[u8]) -> Vec<u8> {
        let header = if more { "a=T,f=24,m=1" } else { "m=0" };
        let mut out = b"\x1b_G".to_vec();
        out.extend_from_slice(header.as_bytes());
        out.push(b';');
        out.extend_from_slice(payload);
        out.extend_from_slice(b"\x1b\\");
        out
    }

    #[test]
    fn oversized_string_keeps_a_resync_prefix_so_live_payload_cannot_leak() {
        // Given: an OSC far larger than the retention ceiling, still unterminated.
        let mut harness = Harness::new(256);
        harness.feed(b"visible\x1b]1337;File=name=a:");
        harness.feed(&vec![b'Q'; 4096]);

        // Then: the pending prefix still puts a receiving parser inside the string, so the
        // live tail is consumed as payload instead of printed as text.
        let pending = harness.pending();
        assert_eq!(pending.bytes, b"\x1b]");
        assert_eq!(harness.retained(), b"visible");
    }

    #[test]
    fn discarded_string_resyncs_only_at_the_real_terminator() {
        // Given: an overflowing string whose terminator arrives much later.
        let mut harness = Harness::new(256);
        harness.feed(b"\x1b]1337;File=name=a:");
        harness.feed(&vec![b'Q'; 4096]);
        assert_eq!(harness.pending().bytes, b"\x1b]");

        // When: the real terminator finally arrives, followed by ordinary text.
        harness.feed(b"\x1b\\after");

        // Then: the splitter is back in ground state with no leaked payload.
        assert_eq!(harness.retained(), b"after");
        assert!(harness.pending().bytes.is_empty());
    }

    #[test]
    fn unterminated_control_sequence_is_bounded_and_resyncs() {
        // Given: a CSI whose parameters never terminate.
        let mut harness = Harness::new(128);
        harness.feed(b"text\x1b[");
        harness.feed(&vec![b'1'; 4096]);

        // Then: the partial CSI is bounded, and pending carries the visible text plus the
        // introducer so the live tail is consumed as parameters rather than printed.
        let pending = harness.pending();
        assert_eq!(pending.bytes, b"text\x1b[");
        assert!(pending.bytes.len() < 128, "unterminated CSI grew unbounded");

        // And: the real final byte resyncs the parser.
        harness.feed(b"mafter");
        assert!(harness.retained().ends_with(b"after"));
        assert!(harness.pending().bytes.is_empty());
    }

    #[test]
    fn uneven_multipart_overflow_discards_whole_chain_without_reopening_the_tail() {
        // Given: five 512-byte parts plus a final marker against a 1024-byte ceiling.
        let mut harness = Harness::new(1024);
        harness.feed(b"before\r\n");
        for _ in 0..5 {
            harness.feed(&kitty_part(true, &vec![b'A'; 512]));
        }
        harness.feed(&kitty_part(false, b"Z"));
        harness.feed(b"after\r\n");

        // Then: no fragment of the chain is retained, nothing re-opens mid-tail, and text
        // on both sides of the transmission survives.
        let retained = harness.retained();
        assert!(
            !retained.windows(3).any(|w| w == b"\x1b_G"),
            "partial transmission retained: {:?}",
            String::from_utf8_lossy(&retained)
        );
        assert!(retained.starts_with(b"before\r\n"));
        assert!(retained.ends_with(b"after\r\n"));
        assert!(harness.pending().bytes.is_empty());
    }

    #[test]
    fn oversized_final_part_closes_discard_and_preserves_next_image() {
        let mut harness = Harness::new(128);
        harness.feed(&kitty_part(true, b"AAAA"));
        harness.feed(&kitty_part(false, &[b'A'; 256]));
        let next = b"\x1b_Ga=T,f=24,s=1,v=1;/wAA\x1b\\";
        harness.feed(next);
        assert_eq!(harness.retained(), next);
        assert!(harness.pending().bytes.is_empty());
    }

    #[test]
    fn discarded_string_preserves_split_terminator_escape() {
        let mut harness = Harness::new(128);
        harness.feed(b"\x1b]1337;File=inline=1:");
        harness.feed(&[b'Q'; 256]);
        harness.feed(b"\x1b");
        assert_eq!(harness.pending().bytes, b"\x1b]\x1b");
        harness.feed(b"\\AFTER");
        assert_eq!(harness.retained(), b"AFTER");
    }

    #[test]
    fn unit_carries_the_sequence_of_its_first_byte() {
        // Given: an image whose open and close land in different published chunks.
        let mut harness = Harness::new(1 << 20);
        harness.feed(&kitty_part(true, b"AAA"));
        harness.feed(b"");
        harness.feed(&kitty_part(false, b"BBB"));

        // Then: the image is attributed to the chunk that opened it, not the one that
        // finished it, so resize segmentation wraps it with the geometry it was drawn at.
        let image = harness
            .units
            .iter()
            .find(|unit| unit.kind == UnitKind::Image)
            .expect("image unit");
        assert_eq!(image.sequence, 1);
    }

    #[test]
    fn pending_reports_the_sequence_where_it_started() {
        let mut harness = Harness::new(1 << 20);
        harness.feed(b"done\r\n");
        harness.feed(&kitty_part(true, b"AAA"));
        harness.feed(b"\x1b_Gm=1;BB");
        let pending = harness.pending();
        assert_eq!(pending.sequence, Some(2));
        assert_eq!(pending.bytes.windows(3).filter(|w| *w == b"\x1b_G").count(), 2);
    }

    #[test]
    fn text_interleaved_into_a_transmission_is_its_own_record() {
        // Given: ordinary text printed between two parts of one transmission.
        let mut harness = Harness::new(1 << 20);
        harness.feed(&kitty_part(true, b"AAA"));
        harness.feed(b"INTERLEAVED");
        harness.feed(&kitty_part(false, b"BBB"));

        // Then: the text is a separate text unit, so evicting the image chain atomically
        // cannot take the text with it.
        let text: Vec<&Unit> = harness
            .units
            .iter()
            .filter(|unit| unit.kind == UnitKind::Text)
            .collect();
        assert_eq!(text.len(), 1);
        assert_eq!(text[0].bytes, b"INTERLEAVED");
        let image = harness
            .units
            .iter()
            .find(|unit| unit.kind == UnitKind::Image)
            .expect("image unit");
        assert!(image.order < text[0].order, "stream order must be preserved");
        assert_eq!(image.bytes.windows(3).filter(|w| *w == b"\x1b_G").count(), 2);
    }

    #[test]
    fn kitty_multipart_transmission_is_one_atomic_unit() {
        let mut harness = Harness::new(1 << 20);
        let mut stream = b"pre".to_vec();
        stream.extend_from_slice(&kitty_part(true, b"AAA"));
        stream.extend_from_slice(&kitty_part(true, b"BBB"));
        stream.extend_from_slice(&kitty_part(false, b"CCC"));
        stream.extend_from_slice(b"post");
        harness.feed(&stream);

        assert_eq!(harness.units.len(), 3);
        assert_eq!(harness.units[0].bytes, b"pre");
        assert_eq!(harness.units[1].kind, UnitKind::Image);
        assert_eq!(
            harness.units[1].bytes.windows(3).filter(|w| *w == b"\x1b_G").count(),
            3
        );
        assert_eq!(harness.units[2].bytes, b"post");
    }

    #[test]
    fn single_part_image_with_m0_metadata_is_retained_standalone() {
        let mut harness = Harness::new(1 << 20);
        harness.feed(b"\x1b_Ga=T,f=24,s=2,v=2,m=0;AAAA\x1b\\x");
        let image = harness
            .units
            .iter()
            .find(|unit| unit.kind == UnitKind::Image)
            .expect("single-part image retained");
        assert!(image.bytes.ends_with(b"\x1b\\"));
    }

    #[test]
    fn chain_without_final_marker_still_closes() {
        let mut harness = Harness::new(1 << 20);
        harness.feed(&kitty_part(true, b"AAA"));
        harness.feed(b"\x1b_Ga=T,f=24;BBB\x1b\\tail");
        let image = harness
            .units
            .iter()
            .find(|unit| unit.kind == UnitKind::Image)
            .expect("image closed without m=0");
        assert_eq!(image.bytes.windows(3).filter(|w| *w == b"\x1b_G").count(), 2);
        assert!(harness.retained().ends_with(b"tail"));
    }

    #[test]
    fn iterm_multipart_file_groups_into_one_image_unit() {
        let mut harness = Harness::new(1 << 20);
        harness.feed(b"\x1b]1337;MultipartFile=name=a\x1b\\\x1b]1337;FilePart=AAAA\x1b\\\x1b]1337;FileEnd\x1b\\x");
        assert_eq!(harness.units[0].kind, UnitKind::Image);
        assert_eq!(
            harness.units[0].bytes.windows(5).filter(|w| *w == b"\x1b]133").count(),
            3
        );
        assert_eq!(harness.units[1].bytes, b"x");
    }

    #[test]
    fn orphan_continuation_without_an_opener_is_dropped() {
        let mut harness = Harness::new(1 << 20);
        harness.feed(b"\x1b_Gm=1;AAA\x1b\\visible");
        assert_eq!(harness.retained(), b"visible");
    }

    #[test]
    fn sixel_dcs_is_classified_as_image_and_osc_title_is_text() {
        let mut harness = Harness::new(1 << 20);
        harness.feed(b"\x1bPq#0;2;0;0;0#0~~@\x1b\\\x1b]0;title\x07");
        assert_eq!(harness.units[0].kind, UnitKind::Image);
        assert_eq!(harness.units[1].kind, UnitKind::Text);
    }

    #[test]
    fn escape_cancelling_a_string_does_not_lose_the_following_sequence() {
        let mut harness = Harness::new(1 << 20);
        harness.feed(b"\x1b_Gabc\x1b[31mafter");
        assert_eq!(harness.retained(), b"\x1b[31mafter");
        assert!(harness.pending().bytes.is_empty());
    }

    #[test]
    fn control_sequence_split_across_chunks_is_never_cut_into_two_units() {
        let mut harness = Harness::new(1 << 20);
        harness.feed(b"a\x1b[3");
        harness.feed(b"1mb");
        assert_eq!(harness.units.len(), 1);
        assert_eq!(harness.units[0].bytes, b"a\x1b[31mb");
    }
}
