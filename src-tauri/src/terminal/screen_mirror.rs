//! Daemon-side screen mirror: a headless VT fed with every published chunk.
//!
//! Full replay used to hand a fresh parser the raw ring, which starts at an arbitrary byte. TUIs
//! that redraw in place (cursor-up then rewrite) were replayed from the wrong origin: their frame
//! landed at the top of an empty grid with no scrollback above it. The mirror holds the screen the
//! ring produced, so a full attach can ship that screen instead of the bytes that built it.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::native_terminal::{NativeTerminal, TerminalEngine};

/// Rows of history the mirror keeps above the screen. Bounds both memory and the size of the
/// reconstruction that a full attach and a handover carry.
const MIRROR_SCROLLBACK_LINES: usize = 2_000;
/// The mirror never reports pixels; any non-zero cell size satisfies the parser.
const CELL_WIDTH_PX: u32 = 8;
const CELL_HEIGHT_PX: u32 = 16;
/// Largest seed a handover carries. Beyond this the successor rebuilds from retained bytes, so the
/// frame stays inside the handover payload limit next to the ring itself.
pub const MAX_HANDOVER_SEED_BYTES: usize = 1024 * 1024;

pub struct ScreenMirror {
    terminal: NativeTerminal,
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
        Some(Self { terminal })
    }

    pub fn from_seed(seed: &ScreenSeed) -> Option<Self> {
        let mut mirror = Self::new(seed.cols, seed.rows)?;
        mirror.feed(&seed.bytes);
        Some(mirror)
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        if let Err(error) = self.terminal.feed(bytes) {
            tracing::warn!(%error, "screen mirror feed failed");
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if let Err(error) = self.terminal.resize(cols, rows, CELL_WIDTH_PX, CELL_HEIGHT_PX) {
            tracing::warn!(%error, cols, rows, "screen mirror resize failed");
        }
    }

    pub fn is_alternate_screen(&self) -> bool {
        self.terminal.is_alternate_screen().unwrap_or(true)
    }

    /// The mirror's screen as a seed, or `None` when the parser cannot serialize it.
    pub fn seed(&self) -> Option<ScreenSeed> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
