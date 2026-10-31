//! Daemon-side screen mirror: a headless VT fed with every published chunk.
//!
//! Full replay used to hand a fresh parser the raw ring, which starts at an arbitrary byte. TUIs
//! that redraw in place (cursor-up then rewrite) were replayed from the wrong origin: their frame
//! landed at the top of an empty grid with no scrollback above it. The mirror holds the screen the
//! ring produced, so a full attach can ship that screen instead of the bytes that built it.
//!
//! The mirror's scrollback is compressed once the session goes quiet, through the same
//! [`CompressionScheduler`] the desktop pump and the remote grid mirror use. The owner runs each
//! step while it holds the mirror's mutex, so steps are serialized with feeds and seeds.

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tokio::sync::Notify;

use crate::native_terminal::{
    CompressionScheduler, CompressionStep, NativeTerminal, NativeTerminalError, TerminalEngine,
};
use crate::terminal::program_status::{ProgramStatusRecords, ProgramStatusSummary};
use crate::terminal::program_status_ingest::{apply_event, ProgramStatusIngest};

/// Rows of history the mirror keeps above the screen. Bounds both memory and the size of the
/// reconstruction that a full attach and a handover carry.
const MIRROR_SCROLLBACK_LINES: usize = 2_000;
/// The mirror never reports pixels; any non-zero cell size satisfies the parser.
const CELL_WIDTH_PX: u32 = 8;
const CELL_HEIGHT_PX: u32 = 16;
/// Largest seed a handover carries. Beyond this the successor rebuilds from retained bytes, so the
/// frame stays inside the handover payload limit next to the ring itself.
pub const MAX_HANDOVER_SEED_BYTES: usize = 1024 * 1024;

/// Visible text of the seam line `TerminalOutputHub::publish_gap` feeds into the mirror. Readers
/// that classify the screen drop it: it is Ferryx's marker, not program output.
pub const GAP_SEAM_TEXT: &str = "[Ferryx: output stream gap - some terminal output was missed]";

pub struct ScreenMirror {
    terminal: NativeTerminal,
    /// Idle scrollback compression; output and resizes restart its delay.
    compression: CompressionScheduler,
    /// Signalled on activity and on drop, so a compression driver can wait without a timer
    /// until there is something to compress, and notice when the mirror is gone.
    activity: Arc<Notify>,
    /// OSC 7501 records, present only once [`Self::enable_program_status`] opted this mirror in.
    /// The parser's sink fills it during `feed`, under the mirror's own mutex.
    program_status: Option<Arc<parking_lot::Mutex<ProgramStatusRecords>>>,
    /// The summary last handed out by [`Self::take_program_status`], to report only changes.
    published_summary: Option<ProgramStatusSummary>,
}

/// A mirror's screen in transferable form: the size it was taken at and the VT bytes that
/// rebuild it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenSeed {
    pub cols: u16,
    pub rows: u16,
    #[serde(serialize_with = "encode_bytes", deserialize_with = "decode_bytes")]
    pub bytes: Vec<u8>,
}

fn encode_bytes<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&STANDARD.encode(bytes))
}

fn decode_bytes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    let text = String::deserialize(deserializer)?;
    STANDARD.decode(text).map_err(serde::de::Error::custom)
}

/// The rows of a mirror's visible screen, copied out under its lock and not yet processed.
pub struct VisibleRows {
    cols: u16,
    rows: u16,
    alt_screen: bool,
    /// Up to [`seam_slack`] scrollback rows above the visible screen, then the visible rows.
    lines: Vec<String>,
}

/// Plain text of a mirror's visible screen: rows trimmed at the end, trailing blank rows and the
/// gap seam dropped, joined with `\n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleScreen {
    pub text: String,
    pub cols: u16,
    pub rows: u16,
    pub alt_screen: bool,
}

impl VisibleRows {
    /// Turns the copied rows into text. Runs outside the mirror's lock.
    ///
    /// The text is the screen the program's own output produced: gap seams are removed and the
    /// last `rows` remaining rows kept, so the rows a seam scrolled off the top come back from
    /// the slack above. A seam starts with CRLF, so when the cursor was already at column 0 it
    /// leaves a blank row above the seam that the program never wrote; that row is dropped with
    /// the seam.
    pub fn into_screen(self) -> VisibleScreen {
        let seam: String = GAP_SEAM_TEXT.split_whitespace().collect();
        let mut lines: Vec<String> = Vec::with_capacity(self.lines.len());
        let mut index = 0;
        while index < self.lines.len() {
            if let Some(span) = gap_seam_rows(&self.lines[index..], &seam) {
                if lines.last().is_some_and(|line| line.is_empty()) {
                    lines.pop();
                }
                index += span;
                continue;
            }
            lines.push(self.lines[index].trim_end().to_string());
            index += 1;
        }
        let excess = lines.len().saturating_sub(usize::from(self.rows));
        lines.drain(..excess);
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        VisibleScreen {
            text: lines.join("\n"),
            cols: self.cols,
            rows: self.rows,
            alt_screen: self.alt_screen,
        }
    }
}

/// Scrollback rows copied above the visible screen: enough to restore what two gap seams, each
/// wrapped at `cols` and preceded by a blank row, scrolled off the top.
fn seam_slack(cols: u16) -> usize {
    2 * (GAP_SEAM_TEXT.len().div_ceil(usize::from(cols.max(1))) + 1)
}

/// How many rows at the head of `rows` hold one gap seam, which wraps on narrow screens.
/// Whitespace is ignored so a wrap that falls on a space still matches.
fn gap_seam_rows(rows: &[String], seam: &str) -> Option<usize> {
    let mut joined = String::new();
    for (index, row) in rows.iter().enumerate() {
        joined.extend(row.split_whitespace());
        if joined.is_empty() || !seam.starts_with(joined.as_str()) {
            return None;
        }
        if joined.len() == seam.len() {
            return Some(index + 1);
        }
    }
    None
}

impl ScreenMirror {
    pub fn new(cols: u16, rows: u16) -> Option<Self> {
        let mut terminal = match NativeTerminal::new(cols, rows) {
            Ok(terminal) => terminal,
            Err(error) => {
                tracing::warn!(%error, cols, rows, "screen mirror unavailable");
                return None;
            }
        };
        // Query answers belong to the real client; the mirror must never generate any.
        terminal.set_pty_writes_suppressed(true);
        if let Err(error) = terminal.set_scrollback_limit_lines(Some(MIRROR_SCROLLBACK_LINES)) {
            tracing::warn!(%error, "screen mirror scrollback limit rejected");
            return None;
        }
        Some(Self {
            terminal,
            compression: CompressionScheduler::default(),
            activity: Arc::new(Notify::new()),
            program_status: None,
            published_summary: None,
        })
    }

    /// Built from a seed. The seed is replayed before any opt-in, so it never produces program
    /// status reports or query replies.
    pub fn from_seed(seed: &ScreenSeed) -> Option<Self> {
        let mut mirror = Self::new(seed.cols, seed.rows)?;
        mirror.feed_replay(&seed.bytes);
        Some(mirror)
    }

    /// Makes this mirror the session's OSC 7501 consumer and support query replier. Only for
    /// sessions this daemon answers for. Every other PTY write stays suppressed. Returns false
    /// when the parser refused the callbacks.
    pub fn enable_program_status(&mut self) -> bool {
        if self.program_status.is_some() {
            return true;
        }
        let records = Arc::new(parking_lot::Mutex::new(ProgramStatusRecords::new()));
        let sink_records = Arc::clone(&records);
        if let Err(error) = self
            .terminal
            .enable_program_status(move |event| apply_event(&mut sink_records.lock(), event))
        {
            tracing::warn!(%error, "screen mirror program status unavailable");
            return false;
        }
        self.terminal.set_program_status_reply_capture(true);
        self.program_status = Some(records);
        true
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        if let Err(error) = self.terminal.feed(bytes) {
            tracing::warn!(%error, "screen mirror feed failed");
        }
        self.mark_activity();
    }

    /// Feeds bytes that already reached the program before (seed, retained history, a gap seam):
    /// no program status report is ingested and no query reply is kept.
    pub fn feed_replay(&mut self, bytes: &[u8]) {
        self.terminal.set_program_status_muted(true);
        self.terminal.set_program_status_reply_capture(false);
        self.feed(bytes);
        self.terminal.set_program_status_muted(false);
        self.terminal
            .set_program_status_reply_capture(self.program_status.is_some());
    }

    /// Drains the query replies kept since the last call and reports the summary when it changed.
    pub fn take_program_status(&mut self) -> ProgramStatusIngest {
        let replies = self.terminal.take_program_status_replies();
        let summary = self.program_status_summary_change(|_| {});
        ProgramStatusIngest { replies, summary }
    }

    /// The user saw the session: drops `done`/`error` records. `None` when not opted in, else
    /// the summary afterwards.
    pub fn acknowledge_program_status(&mut self) -> Option<Option<ProgramStatusSummary>> {
        self.program_status.as_ref()?;
        self.program_status_summary_change(ProgramStatusRecords::acknowledge);
        Some(self.published_summary.clone())
    }

    /// The foreground program exited: drops `working`/`blocked`/`idle` records. `None` when not
    /// opted in, else the summary afterwards.
    pub fn program_status_exited(&mut self) -> Option<Option<ProgramStatusSummary>> {
        self.program_status.as_ref()?;
        self.program_status_summary_change(ProgramStatusRecords::program_exited);
        Some(self.published_summary.clone())
    }

    fn program_status_summary_change(
        &mut self,
        update: impl FnOnce(&mut ProgramStatusRecords),
    ) -> Option<Option<ProgramStatusSummary>> {
        let summary = {
            let mut records = self.program_status.as_ref()?.lock();
            update(&mut records);
            records.summary()
        };
        if summary == self.published_summary {
            return None;
        }
        self.published_summary = summary.clone();
        Some(summary)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if let Err(error) = self.terminal.resize(cols, rows, CELL_WIDTH_PX, CELL_HEIGHT_PX) {
            tracing::warn!(%error, cols, rows, "screen mirror resize failed");
        }
        self.mark_activity();
    }

    fn mark_activity(&mut self) {
        self.compression.mark_activity(Instant::now());
        self.activity.notify_one();
    }

    /// Signalled whenever the mirror changes and when it is dropped.
    pub fn activity_signal(&self) -> Arc<Notify> {
        Arc::clone(&self.activity)
    }

    /// Time until the next idle compression step is due, or `None` when none is scheduled.
    pub fn compression_wakeup(&self, now: Instant) -> Option<Duration> {
        self.compression.next_wakeup(now)
    }

    /// Whether idle compression can still run; false for good once the platform reported it
    /// unsupported, so a driver can stop.
    pub fn compression_supported(&self) -> bool {
        self.compression.is_supported()
    }

    /// Runs one due incremental compression step; `enabled` is the `scrollback-compression`
    /// preference. Compression changes no cell, so [`Self::seed`] is unaffected.
    pub fn run_due_compression(
        &mut self,
        now: Instant,
        enabled: bool,
    ) -> Result<Option<CompressionStep>, NativeTerminalError> {
        self.compression.run_due_step(now, enabled, &mut self.terminal)
    }

    pub fn is_alternate_screen(&self) -> bool {
        self.terminal.is_alternate_screen().unwrap_or(true)
    }

    /// Copies the visible screen's rows plus a few scrollback rows above them (see
    /// [`VisibleRows::into_screen`]). Screen coordinates count the active screen's scrollback
    /// first, so the visible rows start after it. Reads only; idle compression is not re-armed.
    pub fn visible_rows(&self) -> Option<VisibleRows> {
        let (cols, rows) = self.terminal.dimensions().ok()?;
        let top = self.terminal.scrollback_rows().ok()?;
        let alt_screen = self.terminal.is_alternate_screen().ok()?;
        let first = top.saturating_sub(seam_slack(cols));
        let mut lines = Vec::with_capacity(top - first + usize::from(rows));
        for row in first..top + usize::from(rows) {
            match self.terminal.screen_row_text(row) {
                Ok(text) => lines.push(text),
                Err(_) => break,
            }
        }
        Some(VisibleRows { cols, rows, alt_screen, lines })
    }

    /// The mirror's screen as a seed, or `None` when the parser cannot serialize it.
    ///
    /// The formatter reads every scrollback page, which restores compressed ones, so taking a
    /// seed counts as activity: idle compression is re-armed and its driver woken.
    pub fn seed(&mut self) -> Option<ScreenSeed> {
        let seed = self.serialize();
        self.mark_activity();
        seed
    }

    fn serialize(&self) -> Option<ScreenSeed> {
        let (cols, rows) = self.terminal.dimensions().ok()?;
        match self.terminal.reconstruction_bytes() {
            Ok(bytes) => Some(ScreenSeed { cols, rows, bytes }),
            Err(error) => {
                tracing::warn!(%error, "screen mirror serialization failed");
                None
            }
        }
    }
}

impl Drop for ScreenMirror {
    fn drop(&mut self) {
        // Wakes a driver waiting for activity so it sees the mirror is gone and exits.
        self.activity.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_terminal::compression::COMPRESSION_IDLE_DELAY;

    /// Visible text of each grid row (scrollback first) after replaying a seed into a fresh parser.
    fn replayed_rows(seed: &ScreenSeed) -> Vec<String> {
        let mirror = ScreenMirror::from_seed(seed).unwrap();
        mirror_rows(&mirror)
    }

    fn mirror_rows(mirror: &ScreenMirror) -> Vec<String> {
        let total = mirror.terminal.scrollback_rows().unwrap() + usize::from(mirror.terminal.rows().unwrap());
        (0..total).map(|row| mirror.terminal.screen_row_text(row).unwrap()).collect()
    }

    /// A TUI that redraws its frame in place: each update moves the cursor up over the previous
    /// frame and rewrites it. Replaying only the tail of such a stream into an empty grid clamps
    /// the cursor-up at row 0 and pins the frame to the top.
    fn in_place_frames(frames: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for line in 0..60 {
            out.extend_from_slice(format!("history {line}\r\n").as_bytes());
        }
        for frame in 0..frames {
            if frame > 0 {
                out.extend_from_slice(b"\x1b[3A\r");
            }
            for row in 0..3 {
                out.extend_from_slice(format!("\x1b[2Kframe {frame} row {row}\r\n").as_bytes());
            }
        }
        out
    }

    #[test]
    fn seed_rebuilds_screen_and_scrollback_row_for_row() {
        let mut mirror = ScreenMirror::new(40, 10).unwrap();
        mirror.feed(&in_place_frames(5));
        let seed = mirror.seed().unwrap();
        assert_eq!(replayed_rows(&seed), mirror_rows(&mirror));

        // Live output after the seed lands exactly where it lands on the original screen.
        let mut rebuilt = ScreenMirror::from_seed(&seed).unwrap();
        let next = b"\x1b[3A\r\x1b[2Kframe 5 row 0\r\n\x1b[2Kframe 5 row 1\r\n\x1b[2Kframe 5 row 2\r\n";
        mirror.feed(next);
        rebuilt.feed(next);
        let rows = mirror_rows(&mirror);
        assert_eq!(mirror_rows(&rebuilt), rows);
        assert!(rows.iter().any(|row| row == "history 0"), "scrollback is part of the seed");
        assert!(rows.iter().any(|row| row == "frame 5 row 2"));
        assert!(!rows.iter().any(|row| row.starts_with("frame 4")));
    }

    #[test]
    fn seed_keeps_the_cursor_row_when_the_screen_bottom_is_blank() {
        let mut mirror = ScreenMirror::new(40, 10).unwrap();
        mirror.feed(b"one\r\ntwo\r\n");
        let mut rebuilt = ScreenMirror::from_seed(&mirror.seed().unwrap()).unwrap();
        mirror.feed(b"\x1b[1Arewritten");
        rebuilt.feed(b"\x1b[1Arewritten");
        let rows = mirror_rows(&mirror);
        assert_eq!(mirror_rows(&rebuilt), rows);
        assert_eq!(&rows[..2], ["one", "rewritten"]);
    }

    #[test]
    fn seed_round_trips_through_serde() {
        let mut mirror = ScreenMirror::new(20, 5).unwrap();
        mirror.feed(b"\x1b[31mred\x1b[0m\r\n");
        let seed = mirror.seed().unwrap();
        let json = serde_json::to_string(&seed).unwrap();
        assert_eq!(serde_json::from_str::<ScreenSeed>(&json).unwrap(), seed);
    }

    /// A seed taken by the previous libghostty-vt pin (6a508fd) crosses a rolling handover into
    /// this build's mirror. The fixture holds the rows, cursor and screen the old build saw; this
    /// parser must rebuild exactly that screen from the old seed bytes.
    #[test]
    fn seed_from_fixture_replays_row_for_row() {
        #[derive(Deserialize)]
        struct Fixture {
            seed: ScreenSeed,
            rows: Vec<String>,
            cursor: (u16, u16),
            alternate_screen: bool,
        }
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../fixtures/terminal_protocol/screen_seed_6a508fd.json"
        ))
        .unwrap();
        assert!(fixture.rows.len() > 200, "fixture carries the 200-line scrollback");
        assert_eq!(fixture.cursor.1, 10, "fixture cursor sits on row 10");

        let mirror = ScreenMirror::from_seed(&fixture.seed).unwrap();
        assert_eq!(mirror_rows(&mirror), fixture.rows);
        assert_eq!(mirror.terminal.cursor_position().unwrap(), fixture.cursor);
        assert_eq!(mirror.is_alternate_screen(), fixture.alternate_screen);
    }

    /// Todo 9 decision input: compress the daemon mirror only if a full mirror holds more than
    /// 1 MiB resident. Prints the measured figure (`--nocapture`) for the evidence log.
    #[test]
    fn full_mirror_resident_memory_is_measured() {
        let mut mirror = ScreenMirror::new(200, 50).unwrap();
        let mut input = Vec::new();
        for line in 0..(MIRROR_SCROLLBACK_LINES * 2) {
            input.extend_from_slice(format!("\x1b[3{}mmirror line {line:06} {}\x1b[0m\r\n", line % 8, "x".repeat(150)).as_bytes());
        }
        mirror.feed(&input);
        let usage = mirror.terminal.memory_usage().unwrap();
        let resident = usage.primary_resident_bytes + usage.alternate_resident_bytes;
        eprintln!(
            "screen mirror 200x50 + {MIRROR_SCROLLBACK_LINES} lines: resident_bytes={resident} pages={} over_1mib={}",
            usage.primary_pages,
            resident > 1024 * 1024
        );
        assert!(resident > 0);
        assert!(mirror.terminal.scrollback_rows().unwrap() <= MIRROR_SCROLLBACK_LINES * 2);
    }

    fn full_mirror() -> ScreenMirror {
        let mut mirror = ScreenMirror::new(200, 50).unwrap();
        let mut input = Vec::new();
        for line in 0..(MIRROR_SCROLLBACK_LINES * 2) {
            input.extend_from_slice(format!("\x1b[3{}mmirror line {line:06} {}\x1b[0m\r\n", line % 8, "x".repeat(150)).as_bytes());
        }
        mirror.feed(&input);
        mirror
    }

    #[test]
    fn screen_mirror_compresses_after_idle_and_keeps_its_seed() {
        let mut mirror = full_mirror();
        let before = mirror.seed().unwrap();
        let usage_before = mirror.terminal.memory_usage().unwrap();
        // The feed armed the scheduler at the real clock; nothing is due before the idle delay.
        assert_eq!(mirror.run_due_compression(Instant::now(), true).unwrap(), None);

        let mut now = Instant::now() + COMPRESSION_IDLE_DELAY;
        let mut last = None;
        for _ in 0..100_000 {
            let Some(wait) = mirror.compression_wakeup(now) else { break };
            now += wait;
            last = mirror.run_due_compression(now, true).unwrap();
        }
        assert_eq!(mirror.compression_wakeup(now), None, "the pass must end");

        // Measure before taking the seed: the formatter reads every scrollback page, and reading a
        // compressed page restores it, so a seed taken first would undo the pass being measured.
        let usage_after = mirror.terminal.memory_usage().unwrap();
        assert_eq!(mirror.seed().unwrap(), before, "compression must not change the seed");
        if usage_after.compression_supported {
            assert_eq!(last, Some(CompressionStep::Complete));
            assert!(usage_after.primary_compressed_pages > 0, "{usage_after:?}");
            assert!(
                usage_after.primary_resident_bytes < usage_before.primary_resident_bytes,
                "before {usage_before:?} after {usage_after:?}"
            );
        } else {
            assert_eq!(last, Some(CompressionStep::Unsupported));
            // Unsupported stops for good: new output schedules nothing.
            mirror.feed(b"more\r\n");
            assert_eq!(mirror.compression_wakeup(Instant::now()), None);
        }
    }

    #[test]
    fn screen_mirror_compression_honours_the_preference_off() {
        let mut mirror = full_mirror();
        let due = Instant::now() + COMPRESSION_IDLE_DELAY;
        assert_eq!(mirror.run_due_compression(due, false).unwrap(), None);
        assert_eq!(mirror.compression_wakeup(due), None, "the preference off disarms the pass");
        assert_eq!(mirror.terminal.memory_usage().unwrap().primary_compressed_pages, 0);
    }

    /// Runs every due compression step from one idle delay after now until the pass ends.
    fn drain_compression(mirror: &mut ScreenMirror) -> Option<CompressionStep> {
        let mut now = Instant::now() + COMPRESSION_IDLE_DELAY;
        let mut last = None;
        for _ in 0..100_000 {
            let Some(wait) = mirror.compression_wakeup(now) else { break };
            now += wait;
            last = mirror.run_due_compression(now, true).unwrap();
        }
        assert_eq!(mirror.compression_wakeup(now), None, "the pass must end");
        last
    }

    #[test]
    fn screen_mirror_recompresses_after_a_seed_reads_its_history() {
        let mut mirror = full_mirror();
        let first = drain_compression(&mut mirror);
        let compressed = mirror.terminal.memory_usage().unwrap();
        // A full-replay attach takes a seed of an idle mirror; the formatter restores every
        // compressed page it reads.
        assert!(!mirror.seed().unwrap().bytes.is_empty());
        if compressed.compression_supported {
            assert_eq!(first, Some(CompressionStep::Complete));
            assert!(compressed.primary_compressed_pages > 0, "{compressed:?}");
            assert!(
                mirror.compression_wakeup(Instant::now()).is_some(),
                "taking a seed must re-arm idle compression"
            );
            assert_eq!(drain_compression(&mut mirror), Some(CompressionStep::Complete));
            let after = mirror.terminal.memory_usage().unwrap();
            assert!(after.primary_compressed_pages > 0, "{after:?}");
        } else {
            assert_eq!(first, Some(CompressionStep::Unsupported));
            assert_eq!(mirror.compression_wakeup(Instant::now()), None);
        }
    }
}
