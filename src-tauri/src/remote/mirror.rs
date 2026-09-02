#[cfg(feature = "native-terminal")]
use crate::native_terminal::{
    CellSnapshot, CellWide, ColorRgb, CursorSnapshot, CursorVisualStyle, NativeTerminal,
    NativeTerminalError, RenderSnapshot, ScrollViewport, TerminalEngine,
};
#[cfg(feature = "native-terminal")]
use crate::remote::protocol::{
    RemoteGridCursor, RemoteGridCursorVisualStyle, RemoteGridFrame, RemoteGridLine, RemoteGridRun,
};
#[cfg(feature = "native-terminal")]
use crate::terminal::output_hub::HistorySegment;

#[cfg(feature = "native-terminal")]
const REMOTE_CELL_WIDTH_PX: u32 = 8;
#[cfg(feature = "native-terminal")]
const REMOTE_CELL_HEIGHT_PX: u32 = 16;

#[cfg(feature = "native-terminal")]
pub struct RemoteTerminalMirror {
    engine: NativeTerminal,
    last_cols: Option<u16>,
    last_rows: Option<u16>,
    last_lines: Option<Vec<Vec<RemoteGridRun>>>,
}

#[cfg(feature = "native-terminal")]
impl RemoteTerminalMirror {
    pub fn new(cols: u16, rows: u16) -> Result<Self, NativeTerminalError> {
        let mut engine = NativeTerminal::new(cols, rows)?;
        engine.resize(cols, rows, REMOTE_CELL_WIDTH_PX, REMOTE_CELL_HEIGHT_PX)?;
        Ok(Self {
            engine,
            last_cols: None,
            last_rows: None,
            last_lines: None,
        })
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<RemoteGridFrame, NativeTerminalError> {
        self.engine.feed(bytes)?;
        let snapshot = self.engine.render_snapshot()?;
        Ok(self.frame_from_snapshot(snapshot, false))
    }

    pub fn feed_segments(
        &mut self,
        segments: &[HistorySegment],
    ) -> Result<(), NativeTerminalError> {
        for segment in segments {
            if let (Some(cols), Some(rows)) = (segment.cols, segment.rows) {
                if self.engine.dimensions()? != (cols, rows) {
                    self.engine
                        .resize(cols, rows, REMOTE_CELL_WIDTH_PX, REMOTE_CELL_HEIGHT_PX)?;
                }
            }
            self.engine.feed(&segment.bytes)?;
        }
        Ok(())
    }

    pub fn dimensions(&self) -> Result<(u16, u16), NativeTerminalError> {
        self.engine.dimensions()
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<RemoteGridFrame, NativeTerminalError> {
        self.engine
            .resize(cols, rows, REMOTE_CELL_WIDTH_PX, REMOTE_CELL_HEIGHT_PX)?;
        self.clear_baseline();
        self.full_frame()
    }

    pub fn scroll(&mut self, rows: i16) -> Result<RemoteGridFrame, NativeTerminalError> {
        // ScrollViewport::Delta sign convention in ghostty-vt:
        // Negative delta moves viewport toward top (older scrollback content).
        // Positive delta moves viewport toward bottom (newer content).
        // Wire contract: positive rows = toward bottom (newer), negative rows = toward top (older).
        // Returning a full frame ensures the entire shifted viewport is rendered.
        self.engine
            .scroll_viewport(ScrollViewport::Delta(rows as isize))?;
        self.clear_baseline();
        self.full_frame()
    }

    pub fn full_frame(&mut self) -> Result<RemoteGridFrame, NativeTerminalError> {
        let snapshot = self.engine.render_snapshot()?;
        Ok(self.frame_from_snapshot(snapshot, true))
    }

    fn clear_baseline(&mut self) {
        self.last_cols = None;
        self.last_rows = None;
        self.last_lines = None;
    }

    fn frame_from_snapshot(
        &mut self,
        snapshot: RenderSnapshot,
        force_full: bool,
    ) -> RemoteGridFrame {
        let current_lines = snapshot
            .grid
            .iter()
            .map(|cells| build_runs(cells))
            .collect::<Vec<_>>();
        let cursor = map_cursor(snapshot.cursor);

        let dimensions_match =
            self.last_cols == Some(snapshot.cols) && self.last_rows == Some(snapshot.rows);
        let can_diff = !force_full
            && dimensions_match
            && self
                .last_lines
                .as_ref()
                .is_some_and(|lines| lines.len() == current_lines.len());

        let frame = if can_diff {
            let previous = self.last_lines.as_ref().expect("diff baseline exists");
            let lines = current_lines
                .iter()
                .enumerate()
                .filter_map(|(index, runs)| {
                    if previous[index] == *runs {
                        None
                    } else {
                        Some(RemoteGridLine {
                            index: index as u16,
                            runs: runs.clone(),
                        })
                    }
                })
                .collect();
            RemoteGridFrame::GridDiff {
                cols: snapshot.cols,
                rows: snapshot.rows,
                cursor,
                lines,
            }
        } else {
            let lines = current_lines
                .iter()
                .enumerate()
                .map(|(index, runs)| RemoteGridLine {
                    index: index as u16,
                    runs: runs.clone(),
                })
                .collect();
            RemoteGridFrame::Grid {
                cols: snapshot.cols,
                rows: snapshot.rows,
                cursor,
                lines,
            }
        };

        self.last_cols = Some(snapshot.cols);
        self.last_rows = Some(snapshot.rows);
        self.last_lines = Some(current_lines);
        frame
    }
}

#[cfg(feature = "native-terminal")]
#[derive(Debug)]
struct RunFragment {
    text: String,
    fg: Option<[u8; 3]>,
    bg: Option<[u8; 3]>,
    attrs: u8,
}

#[cfg(feature = "native-terminal")]
impl RunFragment {
    fn is_default_space(&self) -> bool {
        self.fg.is_none()
            && self.bg.is_none()
            && self.attrs == 0
            && !self.text.is_empty()
            && self.text.chars().all(|ch| ch == ' ')
    }
}

#[cfg(feature = "native-terminal")]
fn build_runs(cells: &[CellSnapshot]) -> Vec<RemoteGridRun> {
    let mut fragments = Vec::with_capacity(cells.len());
    for cell in cells {
        if cell.wide == CellWide::SpacerTail {
            continue;
        }
        fragments.push(RunFragment {
            text: if cell.text.is_empty() {
                " ".to_string()
            } else {
                cell.text.clone()
            },
            fg: cell.fg.map(color_array),
            bg: cell.bg.map(color_array),
            attrs: cell_attrs(cell),
        });
    }

    while fragments.last().is_some_and(RunFragment::is_default_space) {
        fragments.pop();
    }

    let mut runs: Vec<RemoteGridRun> = Vec::new();
    for fragment in fragments {
        if let Some(last) = runs.last_mut() {
            if last.fg == fragment.fg && last.bg == fragment.bg && last.attrs == fragment.attrs {
                last.text.push_str(&fragment.text);
                continue;
            }
        }
        runs.push(RemoteGridRun {
            text: fragment.text,
            fg: fragment.fg,
            bg: fragment.bg,
            attrs: fragment.attrs,
        });
    }
    runs
}

#[cfg(feature = "native-terminal")]
fn cell_attrs(cell: &CellSnapshot) -> u8 {
    u8::from(cell.bold)
        | (u8::from(cell.italic) << 1)
        | (u8::from(cell.underline) << 2)
        | (u8::from(cell.inverse) << 3)
}

#[cfg(feature = "native-terminal")]
fn color_array(color: ColorRgb) -> [u8; 3] {
    [color.r, color.g, color.b]
}

#[cfg(feature = "native-terminal")]
fn map_cursor(cursor: CursorSnapshot) -> RemoteGridCursor {
    RemoteGridCursor {
        x: cursor.x,
        y: cursor.y,
        visible: cursor.visible,
        blinking: cursor.blinking,
        wide_tail: cursor.wide_tail,
        visual_style: match cursor.visual_style {
            CursorVisualStyle::Bar => RemoteGridCursorVisualStyle::Bar,
            CursorVisualStyle::Block => RemoteGridCursorVisualStyle::Block,
            CursorVisualStyle::Underline => RemoteGridCursorVisualStyle::Underline,
            CursorVisualStyle::BlockHollow => RemoteGridCursorVisualStyle::BlockHollow,
        },
    }
}

#[cfg(all(test, feature = "native-terminal"))]
mod tests {
    use super::*;

    fn first_line(frame: &RemoteGridFrame) -> &RemoteGridLine {
        match frame {
            RemoteGridFrame::Grid { lines, .. } | RemoteGridFrame::GridDiff { lines, .. } => {
                &lines[0]
            }
        }
    }

    #[test]
    fn plain_ascii_line_produces_one_run() {
        let mut mirror = RemoteTerminalMirror::new(12, 3).expect("mirror");
        let frame = mirror.feed(b"hello").expect("feed");
        let line = first_line(&frame);
        assert_eq!(line.index, 0);
        assert_eq!(line.runs.len(), 1);
        assert_eq!(line.runs[0].text, "hello");
        assert_eq!(line.runs[0].attrs, 0);
        assert_eq!(line.runs[0].fg, None);
        assert_eq!(line.runs[0].bg, None);
    }

    #[test]
    fn sgr_bold_red_maps_attrs_and_foreground() {
        let mut mirror = RemoteTerminalMirror::new(12, 3).expect("mirror");
        let frame = mirror.feed(b"\x1b[1;31mX").expect("feed");
        let run = &first_line(&frame).runs[0];
        assert_eq!(run.text, "X");
        assert_eq!(run.attrs, 1);
        assert!(matches!(run.fg, Some([_, _, _])));
    }

    #[test]
    fn cjk_line_skips_wide_spacer_tails() {
        let mut mirror = RemoteTerminalMirror::new(12, 3).expect("mirror");
        let frame = mirror.feed("한글".as_bytes()).expect("feed");
        let line = first_line(&frame);
        assert_eq!(line.runs.len(), 1);
        assert_eq!(line.runs[0].text, "한글");
    }

    #[test]
    fn second_feed_only_emits_changed_line() {
        let mut mirror = RemoteTerminalMirror::new(12, 3).expect("mirror");
        assert!(matches!(
            mirror.feed(b"first").expect("first feed"),
            RemoteGridFrame::Grid { .. }
        ));

        let frame = mirror.feed(b"\x1b[2;1Hsecond").expect("second feed");
        match frame {
            RemoteGridFrame::GridDiff { lines, .. } => {
                assert_eq!(lines.len(), 1);
                assert_eq!(lines[0].index, 1);
                assert_eq!(lines[0].runs[0].text, "second");
            }
            other => panic!("expected gridDiff, got {other:?}"),
        }
    }

    #[test]
    fn resize_returns_full_frame_with_new_dimensions() {
        let mut mirror = RemoteTerminalMirror::new(12, 3).expect("mirror");
        let _ = mirror.feed(b"hello").expect("feed");
        let frame = mirror.resize(20, 5).expect("resize");
        match frame {
            RemoteGridFrame::Grid { cols, rows, .. } => {
                assert_eq!(cols, 20);
                assert_eq!(rows, 5);
            }
            other => panic!("expected full grid, got {other:?}"),
        }
    }

    #[test]
    fn clear_and_home_places_cursor_at_origin() {
        let mut mirror = RemoteTerminalMirror::new(12, 3).expect("mirror");
        let frame = mirror.feed(b"abc\x1b[2J\x1b[H").expect("clear and home");
        let cursor = match frame {
            RemoteGridFrame::Grid { cursor, .. } | RemoteGridFrame::GridDiff { cursor, .. } => {
                cursor
            }
        };
        assert_eq!((cursor.x, cursor.y), (0, 0));
    }

    #[test]
    fn feed_segments_replays_at_recorded_geometry_then_resizes() {
        let mut mirror = RemoteTerminalMirror::new(50, 6).expect("mirror");
        let seg1 = HistorySegment {
            cols: Some(120),
            rows: Some(6),
            bytes: b"\x1b[1;1H\xed\x95\x9c\xea\xb8\x80\x1b[110GX".to_vec(),
        };
        let seg2 = HistorySegment {
            cols: Some(50),
            rows: Some(6),
            bytes: b"\x1b[2;1Hnext".to_vec(),
        };
        mirror
            .feed_segments(&[seg1, seg2])
            .expect("feed_segments");
        let frame = mirror.full_frame().expect("full frame");
        let lines = match &frame {
            RemoteGridFrame::Grid { lines, cols, rows, .. } => {
                assert_eq!((*cols, *rows), (50, 6));
                lines
            }
            RemoteGridFrame::GridDiff { lines, cols, rows, .. } => {
                assert_eq!((*cols, *rows), (50, 6));
                lines
            }
        };
        let line2 = lines.iter().find(|l| l.index == 1).expect("line 2 exists");
        assert_eq!(line2.runs[0].text, "next");
    }

    #[test]
    fn flat_history_replay_corrupts_relative_to_segmented_replay() {
        let seg1_bytes = b"\x1b[1;1H\xed\x95\x9c\xea\xb8\x80\x1b[110GX";
        let seg2_bytes = b"\x1b[2;1Hnext";

        // Segmented replay
        let mut seg_mirror = RemoteTerminalMirror::new(50, 6).expect("seg mirror");
        seg_mirror
            .feed_segments(&[
                HistorySegment {
                    cols: Some(120),
                    rows: Some(6),
                    bytes: seg1_bytes.to_vec(),
                },
                HistorySegment {
                    cols: Some(50),
                    rows: Some(6),
                    bytes: seg2_bytes.to_vec(),
                },
            ])
            .expect("feed_segments");
        let seg_frame = seg_mirror.full_frame().expect("seg full frame");

        // Flat replay directly at 50 cols
        let mut flat_mirror = RemoteTerminalMirror::new(50, 6).expect("flat mirror");
        let mut flat_bytes = seg1_bytes.to_vec();
        flat_bytes.extend_from_slice(seg2_bytes);
        flat_mirror.feed(&flat_bytes).expect("flat feed");
        let flat_frame = flat_mirror.full_frame().expect("flat full frame");

        let seg_line0 = match &seg_frame {
            RemoteGridFrame::Grid { lines, .. } | RemoteGridFrame::GridDiff { lines, .. } => {
                lines.iter().find(|l| l.index == 0).expect("line 0")
            }
        };
        let flat_line0 = match &flat_frame {
            RemoteGridFrame::Grid { lines, .. } | RemoteGridFrame::GridDiff { lines, .. } => {
                lines.iter().find(|l| l.index == 0).expect("line 0")
            }
        };

        // In flat replay on a 50-col grid, ESC[110G clamps to col 50 (index 49) on line 0,
        // so flat_line0 contains "X" at col 50 (or has text with "X").
        // In segmented replay, 120-col line 0 had "X" at col 110 (index 109), which reflows
        // to a different position/line when resized down to 50 cols.
        assert_ne!(seg_line0, flat_line0);
    }

    #[test]
    fn scroll_negative_reveals_older_content_and_positive_returns_toward_newest() {
        let mut mirror = RemoteTerminalMirror::new(30, 10).expect("mirror");
        let lines = (0..200)
            .map(|index| format!("line-{index:03}"))
            .collect::<Vec<_>>()
            .join("\r\n");
        let initial_frame = mirror.feed(lines.as_bytes()).expect("feed lines");

        fn extract_line(frame: &RemoteGridFrame, idx: u16) -> String {
            let lines = match frame {
                RemoteGridFrame::Grid { lines, .. } | RemoteGridFrame::GridDiff { lines, .. } => {
                    lines
                }
            };
            lines
                .iter()
                .find(|l| l.index == idx)
                .map(|l| l.runs.iter().map(|r| r.text.as_str()).collect::<String>())
                .unwrap_or_default()
        }

        let bottom_row0 = extract_line(&initial_frame, 0);
        assert!(
            bottom_row0.starts_with("line-190"),
            "expected line-190 at row 0 at bottom, got: {bottom_row0}"
        );

        let scrolled_up = mirror.scroll(-5).expect("scroll up (-5)");
        assert!(matches!(scrolled_up, RemoteGridFrame::Grid { .. }));
        let up_row0 = extract_line(&scrolled_up, 0);
        assert!(
            up_row0.starts_with("line-185"),
            "expected line-185 at row 0 after scrolling up -5, got: {up_row0}"
        );

        let scrolled_down = mirror.scroll(5).expect("scroll back down (+5)");
        assert!(matches!(scrolled_down, RemoteGridFrame::Grid { .. }));
        let down_row0 = extract_line(&scrolled_down, 0);
        assert!(
            down_row0.starts_with("line-190"),
            "expected line-190 at row 0 after scrolling down +5, got: {down_row0}"
        );
    }
}
