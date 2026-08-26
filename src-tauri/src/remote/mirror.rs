use crate::native_terminal::{
    CellSnapshot, CellWide, ColorRgb, CursorSnapshot, CursorVisualStyle, NativeTerminal,
    NativeTerminalError, RenderSnapshot, TerminalEngine,
};
use crate::remote::protocol::{
    RemoteGridCursor, RemoteGridCursorVisualStyle, RemoteGridFrame, RemoteGridLine, RemoteGridRun,
};

const REMOTE_CELL_WIDTH_PX: u32 = 8;
const REMOTE_CELL_HEIGHT_PX: u32 = 16;

pub struct RemoteTerminalMirror {
    engine: NativeTerminal,
    last_cols: Option<u16>,
    last_rows: Option<u16>,
    last_lines: Option<Vec<Vec<RemoteGridRun>>>,
}

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

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<RemoteGridFrame, NativeTerminalError> {
        self.engine
            .resize(cols, rows, REMOTE_CELL_WIDTH_PX, REMOTE_CELL_HEIGHT_PX)?;
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

#[derive(Debug)]
struct RunFragment {
    text: String,
    fg: Option<[u8; 3]>,
    bg: Option<[u8; 3]>,
    attrs: u8,
}

impl RunFragment {
    fn is_default_space(&self) -> bool {
        self.fg.is_none()
            && self.bg.is_none()
            && self.attrs == 0
            && !self.text.is_empty()
            && self.text.chars().all(|ch| ch == ' ')
    }
}

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

fn cell_attrs(cell: &CellSnapshot) -> u8 {
    u8::from(cell.bold)
        | (u8::from(cell.italic) << 1)
        | (u8::from(cell.underline) << 2)
        | (u8::from(cell.inverse) << 3)
}

fn color_array(color: ColorRgb) -> [u8; 3] {
    [color.r, color.g, color.b]
}

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

#[cfg(test)]
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
}
