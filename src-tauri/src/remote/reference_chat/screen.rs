//! Read-only original-pane VT screen snapshots (task 6).
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918` (MIT,
//! `docs/chat/HERDR_LICENSE`). The reference reads the pane's own screen to decide what an
//! interactive prompt is asking and to validate an answer against the exact screen the user
//! saw; its screen read is the detector input in `server/prompt.ts`. Ferryx already owns the
//! VT-aware read path — [`crate::remote::mirror::RemoteTerminalMirror`] — so this lane is the
//! bounded adaptation of that mirror, not a second terminal implementation.
//!
//! Frozen contract: `docs/chat/herdr-port-contract.md` §5 (the screen shape) and §6 (the
//! [`snapshot_reference_screen`] entry point this module provides).
//!
//! ## What this module is, and is not
//!
//! * It **never writes** to the pane and **never resizes** it. It only feeds retained history
//!   bytes into the caller's mirror and asks for a full frame, so the pane's own geometry
//!   (`mirror.dimensions()`) is an input, never an output. A caller that needs a mirror at a
//!   different size creates it at the pane's size *before* calling here; this module has no
//!   resize path at all.
//! * It performs **no I/O and no authorization**. Resolving the session, checking the
//!   owner/epoch target binding and refusing a foreign target are the route layer's
//!   obligations (task 13). This module is a pure function of `(mirror, segments)`.
//! * It **never guesses**. History it cannot reconstruct is reported as `gap`, and output it
//!   had to cut is reported as `truncated`; both make
//!   [`ReferenceScreenSnapshot::is_answerable`] false, so an answer is never validated against
//!   a screen the reader could not fully reconstruct.
//!
//! ## The gap signal
//!
//! The frozen signature carries no separate gap argument, so the caller states it through the
//! history it supplies:
//!
//! * `Some(segments)` — the caller *could* reconstruct the segmented history; every payload is
//!   replayed in order and the snapshot is not a gap. `Some(&[])` is a legitimate, non-gapped
//!   read: the history is known and simply empty (a pane that has produced no output yet).
//! * `None` — the caller could *not* supply reconstructable history (a ring-buffer replay gap,
//!   or a session whose retained history was dropped). The snapshot is reported with
//!   `gap = true`, which is exactly the case the reference flags so the UI re-reads instead of
//!   acting on a screen it cannot trust.

use crate::remote::mirror::RemoteTerminalMirror;
use crate::remote::protocol::{RemoteGridFrame, RemoteGridLine};

use super::types::ReferenceScreenSnapshot;

/// Upper bound on the characters a single snapshot may carry.
///
/// The screen read is a prompt-detection input, not a transcript: a pane-sized frame is at
/// most `cols * rows` characters, and this bound keeps a caller that hands over an
/// unexpectedly tall mirror from turning one read into an unbounded response.
pub const REFERENCE_SCREEN_MAX_CHARS: usize = 16_384;

/// Upper bound on the lines a single snapshot may carry.
pub const REFERENCE_SCREEN_MAX_LINES: usize = 400;

/// FNV-1a 64-bit offset basis. The revision digest is hand-rolled so a screen revision stays
/// stable across processes, platforms and dependency upgrades; a randomised hasher would let
/// two identical screens disagree between two reads of the same pane.
const FNV1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64-bit prime.
const FNV1A_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Take a bounded, read-only snapshot of the original pane.
///
/// `segments` are the retained history payloads in stream order (the segmented history the
/// authenticated backend already holds for this target); they are replayed into `mirror`
/// before the frame is read. See the module docs for the `None`/`Some(&[])` gap semantics.
///
/// The returned snapshot never carries input and never changes the pane's geometry: the
/// mirror is used exactly as the caller sized it.
pub fn snapshot_reference_screen(
    mirror: &mut RemoteTerminalMirror,
    segments: Option<&[Vec<u8>]>,
) -> Result<ReferenceScreenSnapshot, String> {
    let gap = segments.is_none();
    if let Some(segments) = segments {
        for segment in segments {
            mirror.feed(segment).map_err(|err| err.to_string())?;
        }
    }

    let (cols, rows) = mirror.dimensions().map_err(|err| err.to_string())?;
    let frame = mirror.full_frame().map_err(|err| err.to_string())?;

    // The revision covers the complete rendering, before bounding: two screens that differ
    // only beyond the bound must still be distinguishable, or a stale card could be accepted.
    let text = render_reference_screen_text(&frame);
    let revision = reference_screen_revision(cols, rows, &text);
    let (text, truncated) = bound_reference_screen_text(&text);

    Ok(ReferenceScreenSnapshot {
        revision,
        text,
        truncated,
        gap,
        cols,
        rows,
    })
}

/// Render a full grid frame as screen text: one line per row, trailing padding removed.
///
/// Lines are placed by their own [`RemoteGridLine::index`], so a partial frame renders the rows
/// it names and leaves the rest blank rather than shifting content upward. Trailing blank rows
/// are dropped; interior blank lines are preserved, because a prompt's spacing is part of what
/// the detector reads.
pub fn render_reference_screen_text(frame: &RemoteGridFrame) -> String {
    let (rows, lines) = match frame {
        RemoteGridFrame::Grid { rows, lines, .. }
        | RemoteGridFrame::GridDiff { rows, lines, .. } => (*rows, lines),
    };

    let mut rendered = vec![String::new(); usize::from(rows)];
    for line in lines {
        let Some(slot) = rendered.get_mut(usize::from(line.index)) else {
            continue;
        };
        *slot = line_text(line);
    }

    while rendered.last().is_some_and(String::is_empty) {
        rendered.pop();
    }

    rendered.join("\n")
}

/// Bound a rendered screen to [`REFERENCE_SCREEN_MAX_LINES`] and
/// [`REFERENCE_SCREEN_MAX_CHARS`], keeping the tail.
///
/// The tail is the part that matters: a TUI's prompt, its status line and its menu all live at
/// the bottom of the screen, so the top is what a bound may drop. Cutting happens on character
/// boundaries, so a multi-byte line (CJK, Hangul, emoji) is never split into invalid UTF-8.
pub fn bound_reference_screen_text(text: &str) -> (String, bool) {
    let mut truncated = false;
    let mut tail = text;

    let mut line_starts = vec![0usize];
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            line_starts.push(index + 1);
        }
    }
    if line_starts.len() > REFERENCE_SCREEN_MAX_LINES {
        let keep_from = line_starts.len() - REFERENCE_SCREEN_MAX_LINES;
        tail = &tail[line_starts[keep_from]..];
        truncated = true;
    }

    let chars = tail.chars().count();
    if chars > REFERENCE_SCREEN_MAX_CHARS {
        let skip = chars - REFERENCE_SCREEN_MAX_CHARS;
        let offset = tail
            .char_indices()
            .nth(skip)
            .map(|(offset, _)| offset)
            .unwrap_or(tail.len());
        tail = &tail[offset..];
        truncated = true;
    }

    (tail.to_string(), truncated)
}

/// The revision an answer names, so a stale card can be refused.
///
/// It changes whenever the visible screen content changes. The cursor is deliberately
/// **excluded**: a blinking cursor changes pixels without changing what the pane is asking, and
/// folding it in would let a blink invalidate a card the user is still reading.
pub fn reference_screen_revision(cols: u16, rows: u16, text: &str) -> String {
    let mut hash = FNV1A_OFFSET_BASIS;
    absorb_fnv1a(&mut hash, &cols.to_le_bytes());
    absorb_fnv1a(&mut hash, &rows.to_le_bytes());
    absorb_fnv1a(&mut hash, &(text.len() as u64).to_le_bytes());
    absorb_fnv1a(&mut hash, text.as_bytes());
    format!("v1-{hash:016x}")
}

fn line_text(line: &RemoteGridLine) -> String {
    let mut text = String::new();
    for run in &line.runs {
        text.push_str(&run.text);
    }
    text.trim_end().to_string()
}

fn absorb_fnv1a(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(FNV1A_PRIME);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::protocol::{
        RemoteGridCursor, RemoteGridCursorVisualStyle, RemoteGridLine, RemoteGridRun,
    };

    fn cursor() -> RemoteGridCursor {
        RemoteGridCursor {
            x: 0,
            y: 0,
            visible: true,
            blinking: false,
            wide_tail: false,
            visual_style: RemoteGridCursorVisualStyle::Block,
        }
    }

    fn run(text: &str, cells: u16) -> RemoteGridRun {
        RemoteGridRun {
            text: text.to_string(),
            fg: None,
            bg: None,
            attrs: 0,
            cells,
        }
    }

    fn frame(rows: u16, lines: Vec<RemoteGridLine>) -> RemoteGridFrame {
        RemoteGridFrame::Grid {
            cols: 24,
            rows,
            cursor: cursor(),
            lines,
        }
    }

    #[test]
    fn segmented_history_replays_in_order_and_renders_the_screen() {
        let mut mirror = RemoteTerminalMirror::new(24, 4).expect("mirror");
        let segments: &[Vec<u8>] = &[b"first line\r\n".to_vec(), b"prompt> ".to_vec()];

        let snapshot =
            snapshot_reference_screen(&mut mirror, Some(segments)).expect("screen snapshot");

        assert_eq!(snapshot.text, "first line\nprompt>");
        assert_eq!((snapshot.cols, snapshot.rows), (24, 4));
        assert!(!snapshot.gap);
        assert!(!snapshot.truncated);
        assert!(!snapshot.revision.is_empty());
        assert!(snapshot.is_answerable());
    }

    #[test]
    fn absent_segmented_history_reports_a_replay_gap() {
        let mut mirror = RemoteTerminalMirror::new(24, 4).expect("mirror");

        let snapshot = snapshot_reference_screen(&mut mirror, None).expect("screen snapshot");

        assert!(snapshot.gap);
        assert_eq!(snapshot.text, "");
        assert!(!snapshot.is_answerable());
    }

    #[test]
    fn empty_segmented_history_is_not_a_gap() {
        let mut mirror = RemoteTerminalMirror::new(24, 4).expect("mirror");
        let segments: &[Vec<u8>] = &[];

        let snapshot =
            snapshot_reference_screen(&mut mirror, Some(segments)).expect("screen snapshot");

        assert!(!snapshot.gap);
        // Known-and-empty is a blank pane, not a lost one; only the empty text makes it
        // unanswerable, and it is unanswerable for that reason alone.
        assert!(snapshot.text.is_empty());
        assert!(!snapshot.is_answerable());
    }

    #[test]
    fn reading_the_same_pane_twice_is_stable_and_never_resizes() {
        let mut mirror = RemoteTerminalMirror::new(20, 4).expect("mirror");
        let segments: &[Vec<u8>] = &[b"prompt> ".to_vec()];

        let first =
            snapshot_reference_screen(&mut mirror, Some(segments)).expect("first screen snapshot");
        // A second read that supplies no new bytes must observe the identical screen: the
        // read path injected nothing into the pane.
        let second =
            snapshot_reference_screen(&mut mirror, Some(&[])).expect("second screen snapshot");

        assert_eq!(second.text, first.text);
        assert_eq!(second.revision, first.revision);
        assert_eq!((second.cols, second.rows), (20, 4));
        // Zero resizes: the pane still has the geometry its owner gave it.
        assert_eq!(mirror.dimensions().expect("dimensions"), (20, 4));
    }

    #[test]
    fn revision_tracks_the_visible_screen_and_ignores_the_cursor() {
        let mut mirror = RemoteTerminalMirror::new(20, 4).expect("mirror");
        let segments: &[Vec<u8>] = &[b"approve?".to_vec()];
        let original =
            snapshot_reference_screen(&mut mirror, Some(segments)).expect("screen snapshot");

        // Moving the cursor changes no content, so the revision a card was rendered from
        // still matches and a blink cannot invalidate it.
        mirror.feed(b"\x1b[1;1H").expect("cursor move");
        let moved = snapshot_reference_screen(&mut mirror, Some(&[])).expect("screen snapshot");
        assert_eq!(moved.text, original.text);
        assert_eq!(moved.revision, original.revision);

        // ...but a real content change must move it, or the equality above would be vacuous.
        mirror.feed(b" yes").expect("content change");
        let changed = snapshot_reference_screen(&mut mirror, Some(&[])).expect("screen snapshot");
        assert_ne!(changed.text, original.text);
        assert_ne!(changed.revision, original.revision);
    }

    #[test]
    fn rendered_text_places_lines_by_index_and_trims_padding() {
        let placed = frame(
            4,
            vec![RemoteGridLine {
                index: 1,
                runs: vec![run("second", 6), run("   ", 3)],
            }],
        );
        // Row 0 was not named, so it stays blank instead of shifting "second" upward.
        assert_eq!(render_reference_screen_text(&placed), "\nsecond");

        let trailing_blank_rows = frame(
            4,
            vec![RemoteGridLine {
                index: 0,
                runs: vec![run("only", 4)],
            }],
        );
        assert_eq!(render_reference_screen_text(&trailing_blank_rows), "only");
    }

    #[test]
    fn blank_screen_renders_no_text() {
        let blank = frame(3, Vec::new());
        assert_eq!(render_reference_screen_text(&blank), "");

        let mut mirror = RemoteTerminalMirror::new(8, 2).expect("mirror");
        let snapshot = snapshot_reference_screen(&mut mirror, Some(&[])).expect("screen snapshot");
        assert_eq!(snapshot.text, "");
        assert!(!snapshot.is_answerable());
    }

    #[test]
    fn bounded_output_keeps_the_tail_and_flags_truncation() {
        let long_line = "x".repeat(REFERENCE_SCREEN_MAX_CHARS + 32);
        let (bounded, truncated) = bound_reference_screen_text(&long_line);
        assert!(truncated);
        assert_eq!(bounded.chars().count(), REFERENCE_SCREEN_MAX_CHARS);
        assert!(long_line.ends_with(&bounded));

        let many_lines = (0..REFERENCE_SCREEN_MAX_LINES + 5)
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let (bounded_lines, truncated_lines) = bound_reference_screen_text(&many_lines);
        assert!(truncated_lines);
        assert_eq!(bounded_lines.lines().count(), REFERENCE_SCREEN_MAX_LINES);
        // The last five lines are the ones a bound may never drop.
        assert!(bounded_lines.starts_with("5\n"));
        assert!(bounded_lines.ends_with(&(REFERENCE_SCREEN_MAX_LINES + 4).to_string()));
    }

    #[test]
    fn bounding_never_splits_a_multibyte_character() {
        let hangul = "가".repeat(REFERENCE_SCREEN_MAX_CHARS + 10);

        let (bounded, truncated) = bound_reference_screen_text(&hangul);

        assert!(truncated);
        assert_eq!(bounded.chars().count(), REFERENCE_SCREEN_MAX_CHARS);
        assert!(bounded.chars().all(|ch| ch == '가'));
    }

    #[test]
    fn short_screen_is_returned_unbounded() {
        let (bounded, truncated) = bound_reference_screen_text("short screen");
        assert_eq!(bounded, "short screen");
        assert!(!truncated);

        // The bound is a maximum, not a target: exactly-at-the-bound is not truncation.
        let exact = "y".repeat(REFERENCE_SCREEN_MAX_CHARS);
        let (bounded, truncated) = bound_reference_screen_text(&exact);
        assert_eq!(bounded, exact);
        assert!(!truncated);
    }
}
