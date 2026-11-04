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
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

use crate::native_terminal::{
    CellSnapshot, CellWide, ColorRgb, CompressionScheduler, CompressionStep, NativeTerminal,
    NativeTerminalError, ScrollViewport, TerminalEngine,
};
use crate::terminal::program_status::{ProgramStatusRecords, ProgramStatusSummary};
use crate::terminal::program_status_ingest::{
    apply_event, next_revision, ProgramStatusChange, ProgramStatusIngest,
};

/// Rows of history the mirror keeps above the screen unless the session opted into a larger
/// budget ([`ScreenMirror::set_scrollback_lines`]). Bounds both memory and the size of the
/// reconstruction that a full attach and a handover carry.
pub(crate) const MIRROR_SCROLLBACK_LINES: usize = 2_000;
/// Largest per-session scrollback a `SetScrollbackBudget` request may ask for.
pub const MAX_MIRROR_SCROLLBACK_LINES: usize = 10_000;
/// Estimated bytes one scrollback cell costs; the budget estimate is `lines * cols * 16`.
const SCROLLBACK_BYTES_PER_CELL: usize = 16;
/// Largest estimated scrollback memory one session may hold.
pub const MAX_SCROLLBACK_BUDGET_BYTES: usize = 64 * 1024 * 1024;
/// Error code of a refused `SetScrollbackBudget` request.
pub const SCROLLBACK_BUDGET_EXCEEDED: &str = "SCROLLBACK_BUDGET_EXCEEDED";
/// The mirror never reports pixels; any non-zero cell size satisfies the parser.
const CELL_WIDTH_PX: u32 = 8;
const CELL_HEIGHT_PX: u32 = 16;
/// Largest seed a handover carries to a successor that did not ask for continuation frames.
/// Beyond this the successor rebuilds from retained bytes, so the frame stays inside the handover
/// payload limit next to the ring itself.
pub const MAX_HANDOVER_SEED_BYTES: usize = 1024 * 1024;
/// Largest seed a handover carries to a successor that asked for continuation frames
/// (`TransferSessions.session_cont`): the session frame is split into `KIND_SESSION_CONT` parts,
/// so no single frame grows past the handover payload limit.
pub const MAX_CONT_HANDOVER_SEED_BYTES: usize = 16 * 1024 * 1024;
/// The libghostty-vt revision this build's mirror parses with. Grid hashes taken by builds with a
/// different revision may legitimately differ; compare their text-only hashes instead.
pub const VT_VERSION: &str = env!("FERRYX_GHOSTTY_VT_SHA");

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
    /// The revision of the newest program status update, 0 before any. Every record applied
    /// so far carries this revision or an older one.
    program_status_revision: u64,
    /// Scrollback rows this session asked to keep; [`MIRROR_SCROLLBACK_LINES`] by default.
    scrollback_lines: usize,
}

/// Why a scrollback budget was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScrollbackBudgetError {
    /// More than [`MAX_MIRROR_SCROLLBACK_LINES`] lines, or `lines * cols * 16` bytes above
    /// [`MAX_SCROLLBACK_BUDGET_BYTES`].
    Exceeded { lines: usize, cols: u16 },
    /// The parser refused the limit.
    Rejected(String),
}

impl std::fmt::Display for ScrollbackBudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exceeded { lines, cols } => write!(
                f,
                "scrollback of {lines} lines at {cols} columns exceeds the per-session budget \
                 ({MAX_MIRROR_SCROLLBACK_LINES} lines, {MAX_SCROLLBACK_BUDGET_BYTES} bytes)"
            ),
            Self::Rejected(reason) => write!(f, "scrollback limit rejected: {reason}"),
        }
    }
}

/// Estimated scrollback memory of `lines` rows at `cols` columns.
pub fn scrollback_budget_bytes(lines: usize, cols: u16) -> usize {
    lines
        .saturating_mul(usize::from(cols.max(1)))
        .saturating_mul(SCROLLBACK_BYTES_PER_CELL)
}

/// Checks a requested budget against the per-session limits.
pub fn check_scrollback_budget(lines: usize, cols: u16) -> Result<(), ScrollbackBudgetError> {
    if lines > MAX_MIRROR_SCROLLBACK_LINES
        || scrollback_budget_bytes(lines, cols) > MAX_SCROLLBACK_BUDGET_BYTES
    {
        return Err(ScrollbackBudgetError::Exceeded { lines, cols });
    }
    Ok(())
}

/// SHA-256 digests of a mirror's grid, hex encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenHash {
    /// Size, every retained scrollback and screen cell (text, width, colours, attributes) and
    /// the cursor.
    pub hash: String,
    /// Only the text of every retained row; stable across parser revisions.
    pub text_hash: String,
    /// Scrollback rows covered by the hashes: the newest `min(scrollback, budget)` rows.
    pub retained_lines: usize,
    pub scrollback_lines: usize,
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hash_color(hasher: &mut Sha256, color: Option<ColorRgb>) {
    match color {
        Some(color) => hasher.update([1, color.r, color.g, color.b]),
        None => hasher.update([0]),
    }
}

/// Attribute bits of one cell, the fields `remote::mirror` reads plus the rest of the SGR set.
fn cell_attr_bits(cell: &CellSnapshot) -> u16 {
    u16::from(cell.bold)
        | (u16::from(cell.italic) << 1)
        | (u16::from(cell.underline) << 2)
        | (u16::from(cell.inverse) << 3)
        | (u16::from(cell.faint) << 4)
        | (u16::from(cell.blink) << 5)
        | (u16::from(cell.invisible) << 6)
        | (u16::from(cell.strikethrough) << 7)
        | (u16::from(cell.overline) << 8)
}

fn hash_cell(hasher: &mut Sha256, cell: &CellSnapshot) {
    hasher.update((cell.text.len() as u64).to_le_bytes());
    hasher.update(cell.text.as_bytes());
    hasher.update([match cell.wide {
        CellWide::Narrow => 0u8,
        CellWide::Wide => 1,
        CellWide::SpacerTail => 2,
        CellWide::SpacerHead => 3,
    }]);
    hash_color(hasher, cell.fg);
    hash_color(hasher, cell.bg);
    hasher.update(cell_attr_bits(cell).to_le_bytes());
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
            program_status_revision: 0,
            scrollback_lines: MIRROR_SCROLLBACK_LINES,
        })
    }

    /// Built from a seed. The seed is replayed before any opt-in, so it never produces program
    /// status reports or query replies.
    pub fn from_seed(seed: &ScreenSeed) -> Option<Self> {
        Self::from_seed_with_scrollback(seed, MIRROR_SCROLLBACK_LINES)
    }

    /// [`Self::from_seed`] with the session's scrollback budget applied before the replay, so a
    /// seed carrying more than the default history keeps all of it.
    pub fn from_seed_with_scrollback(seed: &ScreenSeed, scrollback_lines: usize) -> Option<Self> {
        let mut mirror = Self::new(seed.cols, seed.rows)?;
        if scrollback_lines != MIRROR_SCROLLBACK_LINES {
            if let Err(error) = mirror.set_scrollback_lines(scrollback_lines) {
                tracing::warn!(%error, "seed scrollback budget refused; keeping the default");
            }
        }
        mirror.feed_replay(&seed.bytes);
        Some(mirror)
    }

    /// Scrollback rows this mirror keeps.
    pub fn scrollback_lines(&self) -> usize {
        self.scrollback_lines
    }

    /// Sets the scrollback rows this mirror keeps, refusing budgets above
    /// [`MAX_MIRROR_SCROLLBACK_LINES`] or above [`MAX_SCROLLBACK_BUDGET_BYTES`] at the current
    /// width. Lowering the budget prunes history; raising it keeps future output only.
    pub fn set_scrollback_lines(&mut self, lines: usize) -> Result<(), ScrollbackBudgetError> {
        let cols = self
            .terminal
            .cols()
            .map_err(|error| ScrollbackBudgetError::Rejected(error.to_string()))?;
        check_scrollback_budget(lines, cols)?;
        self.terminal
            .set_scrollback_limit_lines(Some(lines))
            .map_err(|error| ScrollbackBudgetError::Rejected(error.to_string()))?;
        self.scrollback_lines = lines;
        Ok(())
    }

    /// The parser limit for the current width: the session's budget, lowered when a resize made
    /// `lines * cols * 16` exceed [`MAX_SCROLLBACK_BUDGET_BYTES`].
    fn effective_scrollback_lines(&self, cols: u16) -> usize {
        let per_line = scrollback_budget_bytes(1, cols);
        self.scrollback_lines.min(MAX_SCROLLBACK_BUDGET_BYTES / per_line)
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
        match self.update_program_status_stamped(|_| {}) {
            Some((changed, transient, change)) => ProgramStatusIngest {
                replies,
                transient_applied: transient.then_some(change.revision),
                summary: changed.then_some(change),
            },
            None => ProgramStatusIngest {
                replies,
                ..ProgramStatusIngest::default()
            },
        }
    }

    /// The user saw the session: drops `done`/`error` records. `None` when not opted in, else
    /// whether the summary changed and the summary afterwards.
    pub fn acknowledge_program_status(&mut self) -> Option<(bool, ProgramStatusChange)> {
        self.update_program_status(ProgramStatusRecords::acknowledge)
    }

    /// The foreground program exited, as proven by evidence gathered while `through` was the
    /// session's newest published revision: drops the `working`/`blocked`/`idle` records applied
    /// by then, under this mirror's lock, and keeps any applied later. `None` when not opted in,
    /// else whether the summary changed and the summary afterwards.
    pub fn program_status_exited(&mut self, through: u64) -> Option<(bool, ProgramStatusChange)> {
        self.update_program_status(|records| records.program_exited(through))
    }

    /// The revision of the newest program status update, 0 before any: evidence gathered after
    /// reading it covers every record applied by then.
    pub fn program_status_revision(&self) -> u64 {
        self.program_status_revision
    }

    /// Applies `update` and returns whether the summary changed, with the summary afterwards
    /// under a revision taken here, while the caller holds this mirror's lock. A publisher
    /// drops a change only when it already applied a newer one, so unchanged summaries need no
    /// re-publication. Records applied since the last update are stamped with that revision.
    fn update_program_status(
        &mut self,
        update: impl FnOnce(&mut ProgramStatusRecords),
    ) -> Option<(bool, ProgramStatusChange)> {
        self.update_program_status_stamped(update)
            .map(|(changed, _, change)| (changed, change))
    }

    /// [`Self::update_program_status`], also returning whether a transient record was stamped.
    fn update_program_status_stamped(
        &mut self,
        update: impl FnOnce(&mut ProgramStatusRecords),
    ) -> Option<(bool, bool, ProgramStatusChange)> {
        let revision = next_revision();
        let (transient, summary) = {
            let mut records = self.program_status.as_ref()?.lock();
            update(&mut records);
            let transient = records.stamp(revision);
            (transient, records.summary())
        };
        self.program_status_revision = revision;
        let changed = summary != self.published_summary;
        self.published_summary = summary.clone();
        Some((changed, transient, ProgramStatusChange { revision, summary }))
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if let Err(error) = self.terminal.resize(cols, rows, CELL_WIDTH_PX, CELL_HEIGHT_PX) {
            tracing::warn!(%error, cols, rows, "screen mirror resize failed");
        }
        if self.scrollback_lines != MIRROR_SCROLLBACK_LINES {
            let limit = self.effective_scrollback_lines(cols);
            if let Err(error) = self.terminal.set_scrollback_limit_lines(Some(limit)) {
                tracing::warn!(%error, limit, "screen mirror scrollback limit rejected after resize");
            }
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

    /// Hashes the grid: SHA-256 over cols, rows, every cell of the newest
    /// `min(scrollback, budget)` scrollback rows and of the screen (text, width, fg, bg,
    /// attributes), and the cursor x, y and visibility. A second, text-only digest covers the row
    /// text alone, for comparing grids across parser revisions.
    ///
    /// Walks the history by moving the viewport page by page and returns it to the bottom
    /// afterwards; no cell changes, so idle compression is not re-armed.
    pub fn grid_hash(&mut self) -> Option<ScreenHash> {
        let result = self.hash_pages();
        self.terminal.scroll_viewport(ScrollViewport::Bottom).ok()?;
        result
    }

    fn hash_pages(&mut self) -> Option<ScreenHash> {
        let (cols, rows) = self.terminal.dimensions().ok()?;
        let scrollback = self.terminal.scrollback_rows().ok()?;
        let cursor = self.terminal.cursor_state().ok()?;
        let retained = scrollback.min(self.scrollback_lines);
        let first = scrollback - retained;
        let end = scrollback + usize::from(rows);

        let mut hasher = Sha256::new();
        hasher.update(cols.to_le_bytes());
        hasher.update(rows.to_le_bytes());
        let mut next = first;
        while next < end {
            self.terminal.scroll_viewport(ScrollViewport::Row(next)).ok()?;
            let top = usize::try_from(self.terminal.scrollbar().ok()?.offset).ok()?;
            if top > next || top + usize::from(rows) <= next {
                tracing::warn!(next, top, "screen mirror viewport did not reach the hashed row");
                return None;
            }
            let snapshot = self.terminal.render_snapshot().ok()?;
            let page_end = end.min(top + usize::from(rows));
            for row in next..page_end {
                let cells = snapshot.grid.get(row - top)?;
                hasher.update((cells.len() as u64).to_le_bytes());
                for cell in cells {
                    hash_cell(&mut hasher, cell);
                }
            }
            next = page_end;
        }
        hasher.update(cursor.x.to_le_bytes());
        hasher.update(cursor.y.to_le_bytes());
        hasher.update([u8::from(cursor.visible)]);

        let mut text_hasher = Sha256::new();
        for row in first..end {
            let text = self.terminal.screen_row_text(row).ok()?;
            text_hasher.update((text.len() as u64).to_le_bytes());
            text_hasher.update(text.as_bytes());
        }
        Some(ScreenHash {
            hash: hex(&hasher.finalize()),
            text_hash: hex(&text_hasher.finalize()),
            retained_lines: retained,
            scrollback_lines: self.scrollback_lines,
        })
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

/// Read-only grid access for the property tests in `terminal::fuzz_tests`.
#[cfg(test)]
impl ScreenMirror {
    pub(crate) fn scrollback_rows_for_test(&self) -> usize {
        self.terminal.scrollback_rows().unwrap()
    }

    /// Visible text of every grid row, scrollback first.
    pub(crate) fn grid_text_for_test(&self) -> Vec<String> {
        let total = self.scrollback_rows_for_test() + usize::from(self.terminal.rows().unwrap());
        (0..total).map(|row| self.terminal.screen_row_text(row).unwrap()).collect()
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
