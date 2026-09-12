use crate::remote::protocol::{
    RemoteGridCursor, RemoteGridCursorVisualStyle, RemoteGridFrame, RemoteGridLine, RemoteGridRun,
};
use crate::terminal::output_hub::HistorySegment;

#[cfg(feature = "native-terminal")]
use crate::native_terminal::{
    CellSnapshot, CellWide, ColorRgb, CursorSnapshot, CursorVisualStyle, NativeTerminal,
    NativeTerminalError, RenderSnapshot, ScrollViewport, TerminalEngine,
};

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

#[derive(Debug)]
struct RunFragment {
    text: String,
    fg: Option<[u8; 3]>,
    bg: Option<[u8; 3]>,
    attrs: u8,
    cells: u16,
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

fn fragments_to_runs(mut fragments: Vec<RunFragment>) -> Vec<RemoteGridRun> {
    while fragments.last().is_some_and(RunFragment::is_default_space) {
        fragments.pop();
    }

    let mut runs: Vec<RemoteGridRun> = Vec::new();
    for fragment in fragments {
        if let Some(last) = runs.last_mut() {
            if last.fg == fragment.fg && last.bg == fragment.bg && last.attrs == fragment.attrs {
                last.text.push_str(&fragment.text);
                last.cells += fragment.cells;
                continue;
            }
        }
        runs.push(RemoteGridRun {
            text: fragment.text,
            fg: fragment.fg,
            bg: fragment.bg,
            attrs: fragment.attrs,
            cells: fragment.cells,
        });
    }
    runs
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
            cells: if cell.wide == CellWide::Wide { 2 } else { 1 },
        });
    }
    fragments_to_runs(fragments)
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

pub mod headless {
    use super::*;

    #[derive(Clone, PartialEq, Eq, Debug)]
    pub struct HeadlessCell {
        pub ch: char,
        pub combining: Vec<char>,
        pub fg: Option<[u8; 3]>,
        pub bg: Option<[u8; 3]>,
        pub attrs: u8,
        pub is_spacer_tail: bool,
        pub width: u16,
    }

    impl Default for HeadlessCell {
        fn default() -> Self {
            Self {
                ch: ' ',
                combining: Vec::new(),
                fg: None,
                bg: None,
                attrs: 0,
                is_spacer_tail: false,
                width: 1,
            }
        }
    }

    const ANSI_COLORS: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 0, 0],
        [0, 205, 0],
        [205, 205, 0],
        [0, 0, 238],
        [205, 0, 205],
        [0, 205, 205],
        [229, 229, 229],
        [127, 127, 127],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [92, 92, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];

    fn parse_256_color(n: u8) -> [u8; 3] {
        if (n as usize) < 16 {
            ANSI_COLORS[n as usize]
        } else if n < 232 {
            let n = n - 16;
            let r = ((n / 36) % 6) * 51;
            let g = ((n / 6) % 6) * 51;
            let b = (n % 6) * 51;
            [r, g, b]
        } else {
            let gray = (n - 232) * 10 + 8;
            [gray, gray, gray]
        }
    }

    fn char_width(ch: char) -> u16 {
        let u = ch as u32;
        if ch.is_control() {
            return 0;
        }
        // Combining diacritical marks and zero-width characters
        if (0x0300..=0x036F).contains(&u)
            || (0x1AB0..=0x1AFF).contains(&u)
            || (0x1DC0..=0x1DFF).contains(&u)
            || (0x20D0..=0x20FF).contains(&u)
            || (0xFE20..=0xFE2F).contains(&u)
            || u == 0x200B
            || u == 0xFEFF
        {
            return 0;
        }
        if (0x1100..=0x115F).contains(&u)
            || (0x2E80..=0xA4CF).contains(&u)
            || (0xAC00..=0xD7A3).contains(&u)
            || (0xF900..=0xFAFF).contains(&u)
            || (0xFE10..=0xFE19).contains(&u)
            || (0xFE30..=0xFE6F).contains(&u)
            || (0xFF00..=0xFF60).contains(&u)
            || (0xFFE0..=0xFFE6).contains(&u)
            || (0x20000..=0x2FFFD).contains(&u)
            || (0x30000..=0x3FFFD).contains(&u)
        {
            2
        } else {
            1
        }
    }

    fn build_runs(cells: &[HeadlessCell]) -> Vec<RemoteGridRun> {
        let mut fragments = Vec::with_capacity(cells.len());
        for cell in cells {
            if cell.is_spacer_tail {
                continue;
            }
            let mut text = cell.ch.to_string();
            for &c in &cell.combining {
                text.push(c);
            }
            fragments.push(RunFragment {
                text,
                fg: cell.fg,
                bg: cell.bg,
                attrs: cell.attrs,
                cells: cell.width,
            });
        }
        fragments_to_runs(fragments)
    }

    pub struct RemoteTerminalMirror {
        pub cols: u16,
        pub rows: u16,
        pub scroll_top: u16,
        pub scroll_bottom: u16,
        pub alt_screen_active: bool,
        pub primary_grid: Vec<Vec<HeadlessCell>>,
        pub primary_cursor: (u16, u16),
        pub alt_grid: Vec<Vec<HeadlessCell>>,
        pub alt_cursor: (u16, u16),
        pub scrollback: Vec<Vec<HeadlessCell>>,
        viewport_offset: usize,
        pub cursor_x: u16,
        pub cursor_y: u16,
        saved_cursor: (u16, u16),
        cursor_visible: bool,
        current_fg: Option<[u8; 3]>,
        current_bg: Option<[u8; 3]>,
        current_attrs: u8,
        pending: Vec<u8>,
    }

    impl RemoteTerminalMirror {
        pub fn new(cols: u16, rows: u16) -> Result<Self, String> {
            if cols == 0 || rows == 0 {
                return Err(format!("invalid dimensions: {cols}x{rows}"));
            }
            let primary_grid = vec![vec![HeadlessCell::default(); cols as usize]; rows as usize];
            let alt_grid = vec![vec![HeadlessCell::default(); cols as usize]; rows as usize];
            let scroll_bottom = rows.saturating_sub(1);
            Ok(Self {
                cols,
                rows,
                scroll_top: 0,
                scroll_bottom,
                alt_screen_active: false,
                primary_grid,
                primary_cursor: (0, 0),
                alt_grid,
                alt_cursor: (0, 0),
                scrollback: Vec::new(),
                viewport_offset: 0,
                cursor_x: 0,
                cursor_y: 0,
                saved_cursor: (0, 0),
                cursor_visible: true,
                current_fg: None,
                current_bg: None,
                current_attrs: 0,
                pending: Vec::new(),
            })
        }

        pub fn active_grid(&self) -> &Vec<Vec<HeadlessCell>> {
            if self.alt_screen_active {
                &self.alt_grid
            } else {
                &self.primary_grid
            }
        }

        pub fn active_grid_mut(&mut self) -> &mut Vec<Vec<HeadlessCell>> {
            if self.alt_screen_active {
                &mut self.alt_grid
            } else {
                &mut self.primary_grid
            }
        }

        fn sync_active_cursor(&mut self) {
            if self.alt_screen_active {
                self.alt_cursor = (self.cursor_x, self.cursor_y);
            } else {
                self.primary_cursor = (self.cursor_x, self.cursor_y);
            }
        }

        pub fn dimensions(&self) -> Result<(u16, u16), String> {
            Ok((self.cols, self.rows))
        }

        pub fn resize(&mut self, cols: u16, rows: u16) -> Result<RemoteGridFrame, String> {
            if cols == 0 || rows == 0 {
                return Err(format!("invalid dimensions: {cols}x{rows}"));
            }
            self.cols = cols;
            self.rows = rows;
            self.scroll_top = 0;
            self.scroll_bottom = rows.saturating_sub(1);

            for line in &mut self.primary_grid {
                line.resize(cols as usize, HeadlessCell::default());
            }
            for line in &mut self.scrollback {
                line.resize(cols as usize, HeadlessCell::default());
            }
            if self.primary_grid.len() < rows as usize {
                let diff = rows as usize - self.primary_grid.len();
                for _ in 0..diff {
                    self.primary_grid.push(vec![HeadlessCell::default(); cols as usize]);
                }
            } else if self.primary_grid.len() > rows as usize {
                let diff = self.primary_grid.len() - rows as usize;
                for _ in 0..diff {
                    let old_line = self.primary_grid.remove(0);
                    self.scrollback.push(old_line);
                }
            }

            for line in &mut self.alt_grid {
                line.resize(cols as usize, HeadlessCell::default());
            }
            if self.alt_grid.len() < rows as usize {
                let diff = rows as usize - self.alt_grid.len();
                for _ in 0..diff {
                    self.alt_grid.push(vec![HeadlessCell::default(); cols as usize]);
                }
            } else if self.alt_grid.len() > rows as usize {
                let diff = self.alt_grid.len() - rows as usize;
                for _ in 0..diff {
                    self.alt_grid.remove(0);
                }
            }

            self.primary_cursor.0 = self.primary_cursor.0.min(cols.saturating_sub(1));
            self.primary_cursor.1 = self.primary_cursor.1.min(rows.saturating_sub(1));
            self.alt_cursor.0 = self.alt_cursor.0.min(cols.saturating_sub(1));
            self.alt_cursor.1 = self.alt_cursor.1.min(rows.saturating_sub(1));

            self.cursor_x = self.cursor_x.min(cols.saturating_sub(1));
            self.cursor_y = self.cursor_y.min(rows.saturating_sub(1));
            self.sync_active_cursor();
            self.full_frame()
        }

        pub fn scroll(&mut self, rows: i16) -> Result<RemoteGridFrame, String> {
            if self.alt_screen_active {
                return self.full_frame();
            }
            let max_offset = self.scrollback.len();
            if rows < 0 {
                let count = (-rows) as usize;
                self.viewport_offset = (self.viewport_offset + count).min(max_offset);
            } else {
                let count = rows as usize;
                self.viewport_offset = self.viewport_offset.saturating_sub(count);
            }
            self.full_frame()
        }

        pub fn full_frame(&self) -> Result<RemoteGridFrame, String> {
            let rows_usize = self.rows as usize;

            let lines: Vec<RemoteGridLine> = if self.alt_screen_active {
                (0..self.rows)
                    .map(|row_idx| {
                        let runs = if let Some(line) = self.alt_grid.get(row_idx as usize) {
                            build_runs(line)
                        } else {
                            Vec::new()
                        };
                        RemoteGridLine {
                            index: row_idx,
                            runs,
                        }
                    })
                    .collect()
            } else {
                let total_lines = self.scrollback.len() + self.primary_grid.len();
                (0..self.rows)
                    .map(|row_idx| {
                        let runs = if self.viewport_offset == 0 {
                            if let Some(line) = self.primary_grid.get(row_idx as usize) {
                                build_runs(line)
                            } else {
                                Vec::new()
                            }
                        } else {
                            let end_idx = total_lines.saturating_sub(self.viewport_offset);
                            let start_idx = end_idx.saturating_sub(rows_usize);
                            let target_idx = start_idx + (row_idx as usize);
                            if target_idx < self.scrollback.len() {
                                build_runs(&self.scrollback[target_idx])
                            } else {
                                let grid_idx = target_idx - self.scrollback.len();
                                if let Some(line) = self.primary_grid.get(grid_idx) {
                                    build_runs(line)
                                } else {
                                    Vec::new()
                                }
                            }
                        };
                        RemoteGridLine {
                            index: row_idx,
                            runs,
                        }
                    })
                    .collect()
            };

            let cursor = RemoteGridCursor {
                x: self.cursor_x.min(self.cols.saturating_sub(1)),
                y: self.cursor_y.min(self.rows.saturating_sub(1)),
                visible: self.cursor_visible,
                blinking: false,
                wide_tail: false,
                visual_style: RemoteGridCursorVisualStyle::Block,
            };

            Ok(RemoteGridFrame::Grid {
                cols: self.cols,
                rows: self.rows,
                cursor,
                lines,
            })
        }

        pub fn feed(&mut self, bytes: &[u8]) -> Result<RemoteGridFrame, String> {
            let mut input = std::mem::take(&mut self.pending);
            input.extend_from_slice(bytes);
            self.viewport_offset = 0;

            let mut i = 0;
            let len = input.len();

            while i < len {
                let b = input[i];
                if b == 0x1B {
                    if i + 1 >= len {
                        self.pending = input[i..].to_vec();
                        break;
                    }
                    let next = input[i + 1];
                    if next == b'[' {
                        let start = i + 2;
                        let mut found = false;
                        let mut scan = start;
                        while scan < len {
                            let cb = input[scan];
                            if (0x40..=0x7E).contains(&cb) {
                                found = true;
                                let seq = &input[start..scan];
                                self.handle_csi(seq, cb);
                                i = scan + 1;
                                break;
                            } else if !(cb.is_ascii_digit()
                                || cb == b';'
                                || cb == b'?'
                                || (0x20..=0x2F).contains(&cb))
                            {
                                i += 2;
                                found = true;
                                break;
                            }
                            scan += 1;
                        }
                        if !found {
                            self.pending = input[i..].to_vec();
                            break;
                        }
                    } else if next == b']' {
                        let mut scan = i + 2;
                        let mut found = false;
                        while scan < len {
                            if input[scan] == 0x07 {
                                i = scan + 1;
                                found = true;
                                break;
                            }
                            if input[scan] == 0x1B && scan + 1 < len && input[scan + 1] == b'\\' {
                                i = scan + 2;
                                found = true;
                                break;
                            }
                            scan += 1;
                        }
                        if !found {
                            self.pending = input[i..].to_vec();
                            break;
                        }
                    } else if next == b'(' || next == b')' || next == b'*' || next == b'+' {
                        if i + 2 < len {
                            i += 3;
                        } else {
                            self.pending = input[i..].to_vec();
                            break;
                        }
                    } else if next == b'7' {
                        self.saved_cursor = (self.cursor_x, self.cursor_y);
                        i += 2;
                    } else if next == b'8' {
                        self.cursor_x = self.saved_cursor.0.min(self.cols.saturating_sub(1));
                        self.cursor_y = self.saved_cursor.1.min(self.rows.saturating_sub(1));
                        self.sync_active_cursor();
                        i += 2;
                    } else if next == b'M' {
                        self.reverse_index();
                        i += 2;
                    } else {
                        i += 2;
                    }
                } else if b == b'\r' {
                    self.cursor_x = 0;
                    self.sync_active_cursor();
                    i += 1;
                } else if b == b'\n' {
                    if self.cursor_y == self.scroll_bottom {
                        self.scroll_up();
                    } else if self.cursor_y < self.rows.saturating_sub(1) {
                        self.cursor_y += 1;
                    }
                    self.sync_active_cursor();
                    i += 1;
                } else if b == 0x08 {
                    self.cursor_x = self.cursor_x.saturating_sub(1);
                    self.sync_active_cursor();
                    i += 1;
                } else if b == b'\t' {
                    self.cursor_x = ((self.cursor_x / 8) + 1) * 8;
                    if self.cursor_x >= self.cols {
                        self.cursor_x = self.cols.saturating_sub(1);
                    }
                    self.sync_active_cursor();
                    i += 1;
                } else if b < 0x20 || b == 0x7F {
                    i += 1;
                } else {
                    match std::str::from_utf8(&input[i..]) {
                        Ok(s) => {
                            let ch = s.chars().next().expect("non-empty string");
                            i += ch.len_utf8();
                            self.put_char(ch);
                        }
                        Err(e) => {
                            if e.valid_up_to() > 0 {
                                let s = std::str::from_utf8(&input[i..i + e.valid_up_to()])
                                    .expect("valid utf8");
                                let ch = s.chars().next().expect("non-empty string");
                                i += ch.len_utf8();
                                self.put_char(ch);
                            } else if e.error_len().is_none() {
                                self.pending = input[i..].to_vec();
                                break;
                            } else {
                                i += 1;
                            }
                        }
                    }
                }
            }

            if self.pending.len() > 256 {
                self.pending.clear();
            }

            self.full_frame()
        }

        pub fn feed_segments(&mut self, segments: &[HistorySegment]) -> Result<(), String> {
            for segment in segments {
                if let (Some(cols), Some(rows)) = (segment.cols, segment.rows) {
                    if (self.cols, self.rows) != (cols, rows) {
                        let _ = self.resize(cols, rows)?;
                    }
                }
                let _ = self.feed(&segment.bytes)?;
            }
            Ok(())
        }

        fn is_scrolling_region_active(&self) -> bool {
            self.scroll_top > 0 || self.scroll_bottom < self.rows.saturating_sub(1)
        }

        fn scroll_up(&mut self) {
            let cols = self.cols as usize;
            if self.is_scrolling_region_active() {
                let top = self.scroll_top as usize;
                let bottom = self.scroll_bottom as usize;
                let grid = self.active_grid_mut();
                if top <= bottom && bottom < grid.len() {
                    grid.remove(top);
                    grid.insert(bottom, vec![HeadlessCell::default(); cols]);
                }
            } else if self.alt_screen_active {
                if !self.alt_grid.is_empty() {
                    self.alt_grid.remove(0);
                    self.alt_grid.push(vec![HeadlessCell::default(); cols]);
                }
            } else {
                if !self.primary_grid.is_empty() {
                    let old_line = self.primary_grid.remove(0);
                    self.scrollback.push(old_line);
                    if self.scrollback.len() > 10_000 {
                        self.scrollback.remove(0);
                    }
                    self.primary_grid.push(vec![HeadlessCell::default(); cols]);
                }
            }
        }

        fn reverse_index(&mut self) {
            if self.cursor_y == self.scroll_top {
                let top = self.scroll_top as usize;
                let bottom = self.scroll_bottom as usize;
                let cols = self.cols as usize;
                let grid = self.active_grid_mut();
                if top <= bottom && bottom < grid.len() {
                    grid.remove(bottom);
                    grid.insert(top, vec![HeadlessCell::default(); cols]);
                }
            } else if self.cursor_y > self.scroll_top {
                self.cursor_y = self.cursor_y.saturating_sub(1);
            } else {
                self.cursor_y = self.cursor_y.saturating_sub(1);
            }
            self.sync_active_cursor();
        }

        fn put_char(&mut self, ch: char) {
            let width = char_width(ch);
            if width == 0 {
                let cy = self.cursor_y as usize;
                let cx = self.cursor_x as usize;
                let grid = self.active_grid_mut();
                if cy < grid.len() {
                    let target_x = if cx > 0 { cx - 1 } else { 0 };
                    if target_x < grid[cy].len() {
                        let actual_x = if grid[cy][target_x].is_spacer_tail && target_x > 0 {
                            target_x - 1
                        } else {
                            target_x
                        };
                        grid[cy][actual_x].combining.push(ch);
                    }
                }
                return;
            }

            if self.cursor_x >= self.cols || (width == 2 && self.cursor_x + 1 >= self.cols) {
                self.cursor_x = 0;
                if self.cursor_y == self.scroll_bottom {
                    self.scroll_up();
                } else if self.cursor_y < self.rows.saturating_sub(1) {
                    self.cursor_y += 1;
                }
            }

            let fg = self.current_fg;
            let bg = self.current_bg;
            let attrs = self.current_attrs;
            let cy = self.cursor_y as usize;
            let cx = self.cursor_x as usize;
            let grid = self.active_grid_mut();
            if cy < grid.len() && cx < grid[cy].len() {
                // If this cell was a spacer tail of an earlier wide character, clear its head
                if grid[cy][cx].is_spacer_tail && cx > 0 {
                    grid[cy][cx - 1] = HeadlessCell::default();
                }
                // If this cell was itself a wide character, clear its trailing spacer
                if grid[cy][cx].width == 2 && cx + 1 < grid[cy].len() {
                    grid[cy][cx + 1] = HeadlessCell::default();
                }
                // If writing a wide character and next cell was a wide character head, clear its tail
                if width == 2 && cx + 1 < grid[cy].len() {
                    if grid[cy][cx + 1].width == 2 && cx + 2 < grid[cy].len() {
                        grid[cy][cx + 2] = HeadlessCell::default();
                    }
                }

                grid[cy][cx] = HeadlessCell {
                    ch,
                    combining: Vec::new(),
                    fg,
                    bg,
                    attrs,
                    is_spacer_tail: false,
                    width,
                };
                if width == 2 && cx + 1 < grid[cy].len() {
                    grid[cy][cx + 1] = HeadlessCell {
                        ch: ' ',
                        combining: Vec::new(),
                        fg,
                        bg,
                        attrs,
                        is_spacer_tail: true,
                        width: 0,
                    };
                }
            }

            self.cursor_x += width;
            self.sync_active_cursor();
        }

        fn handle_csi(&mut self, seq: &[u8], final_byte: u8) {
            let seq_str = std::str::from_utf8(seq).unwrap_or("");
            let is_private = seq_str.starts_with('?');
            let params_str = if is_private { &seq_str[1..] } else { seq_str };
            let params: Vec<u32> = if params_str.is_empty() {
                Vec::new()
            } else {
                params_str
                    .split(';')
                    .map(|p| p.parse::<u32>().unwrap_or(0))
                    .collect()
            };

            match final_byte {
                b'm' => {
                    let sgr_params = if params.is_empty() { vec![0] } else { params };
                    let mut iter = sgr_params.into_iter();
                    while let Some(code) = iter.next() {
                        match code {
                            0 => {
                                self.current_fg = None;
                                self.current_bg = None;
                                self.current_attrs = 0;
                            }
                            1 => self.current_attrs |= 1,
                            2 => {}
                            3 => self.current_attrs |= 2,
                            4 => self.current_attrs |= 4,
                            7 => self.current_attrs |= 8,
                            22 => self.current_attrs &= !1,
                            23 => self.current_attrs &= !2,
                            24 => self.current_attrs &= !4,
                            27 => self.current_attrs &= !8,
                            30..=37 => self.current_fg = Some(ANSI_COLORS[(code - 30) as usize]),
                            38 => match iter.next() {
                                Some(5) => {
                                    if let Some(n) = iter.next() {
                                        self.current_fg = Some(parse_256_color(n as u8));
                                    }
                                }
                                Some(2) => {
                                    let r = iter.next().unwrap_or(0) as u8;
                                    let g = iter.next().unwrap_or(0) as u8;
                                    let b = iter.next().unwrap_or(0) as u8;
                                    self.current_fg = Some([r, g, b]);
                                }
                                _ => {}
                            },
                            39 => self.current_fg = None,
                            40..=47 => self.current_bg = Some(ANSI_COLORS[(code - 40) as usize]),
                            48 => match iter.next() {
                                Some(5) => {
                                    if let Some(n) = iter.next() {
                                        self.current_bg = Some(parse_256_color(n as u8));
                                    }
                                }
                                Some(2) => {
                                    let r = iter.next().unwrap_or(0) as u8;
                                    let g = iter.next().unwrap_or(0) as u8;
                                    let b = iter.next().unwrap_or(0) as u8;
                                    self.current_bg = Some([r, g, b]);
                                }
                                _ => {}
                            },
                            49 => self.current_bg = None,
                            90..=97 => {
                                self.current_fg = Some(ANSI_COLORS[(code - 90 + 8) as usize])
                            }
                            100..=107 => {
                                self.current_bg = Some(ANSI_COLORS[(code - 100 + 8) as usize])
                            }
                            _ => {}
                        }
                    }
                }
                b'H' | b'f' => {
                    let row = params.first().copied().unwrap_or(1).max(1);
                    let col = params.get(1).copied().unwrap_or(1).max(1);
                    self.cursor_y = (row - 1).min(self.rows.saturating_sub(1) as u32) as u16;
                    self.cursor_x = (col - 1).min(self.cols.saturating_sub(1) as u32) as u16;
                    self.sync_active_cursor();
                }
                b'J' => {
                    let mode = params.first().copied().unwrap_or(0);
                    let cy = self.cursor_y as usize;
                    let cx = self.cursor_x as usize;
                    let cols = self.cols as usize;
                    let grid = self.active_grid_mut();
                    match mode {
                        0 => {
                            if cy < grid.len() {
                                for x in cx..cols {
                                    if x < grid[cy].len() {
                                        grid[cy][x] = HeadlessCell::default();
                                    }
                                }
                                for y in (cy + 1)..grid.len() {
                                    grid[y].fill(HeadlessCell::default());
                                }
                            }
                        }
                        1 => {
                            for y in 0..cy.min(grid.len()) {
                                grid[y].fill(HeadlessCell::default());
                            }
                            if cy < grid.len() {
                                for x in 0..=cx.min(cols.saturating_sub(1)) {
                                    if x < grid[cy].len() {
                                        grid[cy][x] = HeadlessCell::default();
                                    }
                                }
                            }
                        }
                        2 | 3 => {
                            for line in grid {
                                line.fill(HeadlessCell::default());
                            }
                        }
                        _ => {}
                    }
                }
                b'K' => {
                    let mode = params.first().copied().unwrap_or(0);
                    let cy = self.cursor_y as usize;
                    let cx = self.cursor_x as usize;
                    let cols = self.cols as usize;
                    let grid = self.active_grid_mut();
                    if cy < grid.len() {
                        match mode {
                            0 => {
                                for x in cx..cols {
                                    if x < grid[cy].len() {
                                        grid[cy][x] = HeadlessCell::default();
                                    }
                                }
                            }
                            1 => {
                                for x in 0..=cx.min(cols.saturating_sub(1)) {
                                    if x < grid[cy].len() {
                                        grid[cy][x] = HeadlessCell::default();
                                    }
                                }
                            }
                            2 => {
                                grid[cy].fill(HeadlessCell::default());
                            }
                            _ => {}
                        }
                    }
                }
                b'A' => {
                    let count = params.first().copied().unwrap_or(1).max(1) as u16;
                    self.cursor_y = self.cursor_y.saturating_sub(count);
                    self.sync_active_cursor();
                }
                b'B' => {
                    let count = params.first().copied().unwrap_or(1).max(1) as u16;
                    self.cursor_y = (self.cursor_y + count).min(self.rows.saturating_sub(1));
                    self.sync_active_cursor();
                }
                b'C' => {
                    let count = params.first().copied().unwrap_or(1).max(1) as u16;
                    self.cursor_x = (self.cursor_x + count).min(self.cols.saturating_sub(1));
                    self.sync_active_cursor();
                }
                b'D' => {
                    let count = params.first().copied().unwrap_or(1).max(1) as u16;
                    self.cursor_x = self.cursor_x.saturating_sub(count);
                    self.sync_active_cursor();
                }
                b'E' => {
                    let count = params.first().copied().unwrap_or(1).max(1) as u16;
                    self.cursor_x = 0;
                    self.cursor_y = (self.cursor_y + count).min(self.rows.saturating_sub(1));
                    self.sync_active_cursor();
                }
                b'F' => {
                    let count = params.first().copied().unwrap_or(1).max(1) as u16;
                    self.cursor_x = 0;
                    self.cursor_y = self.cursor_y.saturating_sub(count);
                    self.sync_active_cursor();
                }
                b'G' => {
                    let col = params.first().copied().unwrap_or(1).max(1) as u16;
                    self.cursor_x = (col - 1).min(self.cols.saturating_sub(1));
                    self.sync_active_cursor();
                }
                b'd' => {
                    let row = params.first().copied().unwrap_or(1).max(1) as u16;
                    self.cursor_y = (row - 1).min(self.rows.saturating_sub(1));
                    self.sync_active_cursor();
                }
                b'@' => {
                    let count = params.first().copied().unwrap_or(1).max(1) as usize;
                    let cy = self.cursor_y as usize;
                    let cx = self.cursor_x as usize;
                    let cols = self.cols as usize;
                    let grid = self.active_grid_mut();
                    if cy < grid.len() && cx < cols {
                        let insert_count = count.min(cols - cx);
                        for _ in 0..insert_count {
                            grid[cy].insert(cx, HeadlessCell::default());
                            grid[cy].pop();
                        }
                    }
                }
                b'P' => {
                    let count = params.first().copied().unwrap_or(1).max(1) as usize;
                    let cy = self.cursor_y as usize;
                    let cx = self.cursor_x as usize;
                    let cols = self.cols as usize;
                    let grid = self.active_grid_mut();
                    if cy < grid.len() && cx < cols {
                        let delete_count = count.min(cols - cx);
                        for _ in 0..delete_count {
                            grid[cy].remove(cx);
                            grid[cy].push(HeadlessCell::default());
                        }
                    }
                }
                b'L' => {
                    let count = params.first().copied().unwrap_or(1).max(1) as usize;
                    let cy = self.cursor_y as usize;
                    let top = self.scroll_top as usize;
                    let bottom = self.scroll_bottom as usize;
                    let cols = self.cols as usize;
                    let grid = self.active_grid_mut();
                    if cy >= top && cy <= bottom && bottom < grid.len() {
                        let insert_count = count.min(bottom - cy + 1);
                        for _ in 0..insert_count {
                            grid.insert(cy, vec![HeadlessCell::default(); cols]);
                            grid.remove(bottom + 1);
                        }
                    }
                }
                b'M' => {
                    let count = params.first().copied().unwrap_or(1).max(1) as usize;
                    let cy = self.cursor_y as usize;
                    let top = self.scroll_top as usize;
                    let bottom = self.scroll_bottom as usize;
                    let cols = self.cols as usize;
                    let grid = self.active_grid_mut();
                    if cy >= top && cy <= bottom && bottom < grid.len() {
                        let delete_count = count.min(bottom - cy + 1);
                        for _ in 0..delete_count {
                            grid.remove(cy);
                            grid.insert(bottom, vec![HeadlessCell::default(); cols]);
                        }
                    }
                }
                b'r' => {
                    if !is_private {
                        let top = match params.first().copied() {
                            Some(0) | None => 1,
                            Some(v) => v,
                        };
                        let bottom = match params.get(1).copied() {
                            Some(0) | None => self.rows as u32,
                            Some(v) => v,
                        };
                        if top <= bottom && bottom <= self.rows as u32 && top >= 1 {
                            self.scroll_top = (top - 1) as u16;
                            self.scroll_bottom = (bottom - 1) as u16;
                            self.cursor_x = 0;
                            self.cursor_y = 0;
                            self.sync_active_cursor();
                        }
                    }
                }
                b'h' => {
                    if is_private {
                        for param in &params {
                            match *param {
                                25 => self.cursor_visible = true,
                                47 | 1049 => {
                                    if !self.alt_screen_active {
                                        self.primary_cursor = (self.cursor_x, self.cursor_y);
                                        self.alt_grid = vec![
                                            vec![HeadlessCell::default(); self.cols as usize];
                                            self.rows as usize
                                        ];
                                        self.alt_cursor = (0, 0);
                                        self.cursor_x = 0;
                                        self.cursor_y = 0;
                                        self.scroll_top = 0;
                                        self.scroll_bottom = self.rows.saturating_sub(1);
                                        self.alt_screen_active = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                b'l' => {
                    if is_private {
                        for param in &params {
                            match *param {
                                25 => self.cursor_visible = false,
                                47 | 1049 => {
                                    if self.alt_screen_active {
                                        self.alt_cursor = (self.cursor_x, self.cursor_y);
                                        self.cursor_x = self.primary_cursor.0;
                                        self.cursor_y = self.primary_cursor.1;
                                        self.scroll_top = 0;
                                        self.scroll_bottom = self.rows.saturating_sub(1);
                                        self.alt_screen_active = false;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(not(feature = "native-terminal"))]
pub use headless::RemoteTerminalMirror;

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
        assert_eq!(line.runs[0].cells, 5);
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
        // Wide cells count 2 each so the DOM renderer can snap run boundaries
        // exactly onto the terminal grid and keep the cursor overlay aligned.
        assert_eq!(line.runs[0].cells, 4);
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
        mirror.feed_segments(&[seg1, seg2]).expect("feed_segments");
        let frame = mirror.full_frame().expect("full frame");
        let lines = match &frame {
            RemoteGridFrame::Grid {
                lines, cols, rows, ..
            } => {
                assert_eq!((*cols, *rows), (50, 6));
                lines
            }
            RemoteGridFrame::GridDiff {
                lines, cols, rows, ..
            } => {
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

#[cfg(test)]
mod headless_tests {
    use super::headless::RemoteTerminalMirror;
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
        assert_eq!(line.runs[0].cells, 5);
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
        assert_eq!(line.runs[0].cells, 4);
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
        mirror.feed_segments(&[seg1, seg2]).expect("feed_segments");
        let frame = mirror.full_frame().expect("full frame");
        let lines = match &frame {
            RemoteGridFrame::Grid {
                lines, cols, rows, ..
            } => {
                assert_eq!((*cols, *rows), (50, 6));
                lines
            }
            RemoteGridFrame::GridDiff {
                lines, cols, rows, ..
            } => {
                assert_eq!((*cols, *rows), (50, 6));
                lines
            }
        };
        let line2 = lines.iter().find(|l| l.index == 1).expect("line 2 exists");
        assert_eq!(line2.runs[0].text, "next");
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

    #[test]
    fn backspace_and_carriage_return() {
        let mut mirror = RemoteTerminalMirror::new(12, 3).expect("mirror");
        let frame = mirror.feed(b"abc\x08d\ref").expect("feed");
        let line = first_line(&frame);
        assert_eq!(line.runs[0].text, "efd");
    }

    #[test]
    fn wrapping_advances_to_next_line() {
        let mut mirror = RemoteTerminalMirror::new(5, 3).expect("mirror");
        let frame = mirror.feed(b"123456").expect("feed");
        let lines = match &frame {
            RemoteGridFrame::Grid { lines, .. } => lines,
            RemoteGridFrame::GridDiff { lines, .. } => lines,
        };
        assert_eq!(lines[0].runs[0].text, "12345");
        assert_eq!(lines[1].runs[0].text, "6");
    }

    #[test]
    fn test_headless_alternate_screen_buffer() {
        let mut mirror = RemoteTerminalMirror::new(20, 5).expect("new mirror");

        // 1. Write to primary screen
        mirror.feed(b"primary content").expect("feed primary");
        assert!(!mirror.alt_screen_active);

        let frame1 = mirror.full_frame().expect("full frame");
        let line1 = match &frame1 {
            RemoteGridFrame::Grid { lines, .. } => lines[0].runs[0].text.clone(),
            _ => panic!("expected Grid"),
        };
        assert_eq!(line1, "primary content");
        assert_eq!(mirror.primary_cursor, (15, 0));

        // 2. Switch to alternate screen buffer with CSI ?1049h
        mirror.feed(b"\x1b[?1049h").expect("switch to alt");
        assert!(mirror.alt_screen_active);
        assert_eq!(mirror.alt_cursor, (0, 0));

        // Alt screen should be empty/cleared
        let frame_alt_empty = mirror.full_frame().expect("full frame alt empty");
        let alt_empty_runs = match &frame_alt_empty {
            RemoteGridFrame::Grid { lines, .. } => lines[0].runs.clone(),
            _ => panic!("expected Grid"),
        };
        assert!(alt_empty_runs.is_empty());

        // 3. Write in alternate buffer
        mirror.feed(b"alt buffer text").expect("feed alt");
        assert_eq!(mirror.alt_cursor, (15, 0));

        let frame_alt = mirror.full_frame().expect("full frame alt");
        let alt_line = match &frame_alt {
            RemoteGridFrame::Grid { lines, .. } => lines[0].runs[0].text.clone(),
            _ => panic!("expected Grid"),
        };
        assert_eq!(alt_line, "alt buffer text");

        // Primary buffer must NOT be polluted
        let primary_text = mirror.primary_grid[0]
            .iter()
            .take(15)
            .map(|c| c.ch)
            .collect::<String>();
        assert_eq!(primary_text, "primary content");
        assert_eq!(mirror.primary_cursor, (15, 0));

        // 4. Switch back to primary screen buffer with CSI ?1049l
        mirror.feed(b"\x1b[?1049l").expect("switch to primary");
        assert!(!mirror.alt_screen_active);
        assert_eq!(mirror.cursor_x, 15);
        assert_eq!(mirror.cursor_y, 0);

        // Primary screen content is intact
        let frame_restored = mirror.full_frame().expect("full frame restored");
        let restored_line = match &frame_restored {
            RemoteGridFrame::Grid { lines, .. } => lines[0].runs[0].text.clone(),
            _ => panic!("expected Grid"),
        };
        assert_eq!(restored_line, "primary content");

        // 5. Verify CSI ?47h and ?47l work identically
        mirror.feed(b"\x1b[?47h").expect("switch to alt 47");
        assert!(mirror.alt_screen_active);
        mirror.feed(b"alt 47 write").expect("feed alt 47");
        mirror.feed(b"\x1b[?47l").expect("switch back 47");
        assert!(!mirror.alt_screen_active);

        let frame_restored2 = mirror.full_frame().expect("full frame restored 2");
        let restored_line2 = match &frame_restored2 {
            RemoteGridFrame::Grid { lines, .. } => lines[0].runs[0].text.clone(),
            _ => panic!("expected Grid"),
        };
        assert_eq!(restored_line2, "primary content");
    }

    fn row_text(frame: &RemoteGridFrame, idx: u16) -> String {
        match frame {
            RemoteGridFrame::Grid { lines, .. } | RemoteGridFrame::GridDiff { lines, .. } => lines
                .iter()
                .find(|l| l.index == idx)
                .map(|l| l.runs.iter().map(|r| r.text.as_str()).collect::<String>())
                .unwrap_or_default(),
        }
    }

    #[test]
    fn test_decstbm_insert_line_preserves_footer() {
        let mut mirror = RemoteTerminalMirror::new(10, 3).expect("new mirror");
        mirror.feed(b"line1\r\nline2\r\nfooter").expect("feed initial");

        let frame = mirror.full_frame().expect("full frame");
        assert_eq!(row_text(&frame, 0), "line1");
        assert_eq!(row_text(&frame, 1), "line2");
        assert_eq!(row_text(&frame, 2), "footer");

        // CSI 1;2r sets scrolling region to rows 1..2 (0..=1), footer is row 3 (idx 2).
        // homes cursor to (0, 0).
        mirror.feed(b"\x1b[1;2r").expect("feed decstbm");
        assert_eq!(mirror.scroll_top, 0);
        assert_eq!(mirror.scroll_bottom, 1);
        assert_eq!((mirror.cursor_x, mirror.cursor_y), (0, 0));

        // CSI L (Insert Line):
        // Within scrolling region 0..=1, row 0 becomes blank, line1 shifts to row 1, line2 is discarded.
        // Row 3 (footer at idx 2) MUST be preserved!
        mirror.feed(b"\x1b[L").expect("feed insert line");

        let frame_after = mirror.full_frame().expect("full frame after insert");
        assert_eq!(row_text(&frame_after, 0), "");
        assert_eq!(row_text(&frame_after, 1), "line1");
        assert_eq!(row_text(&frame_after, 2), "footer");

        // Also test CSI M (Delete Line) within scrolling region preserves footer on row 3
        mirror.feed(b"\x1b[M").expect("feed delete line");
        let frame_after_del = mirror.full_frame().expect("full frame after delete");
        assert_eq!(row_text(&frame_after_del, 0), "line1");
        assert_eq!(row_text(&frame_after_del, 1), "");
        assert_eq!(row_text(&frame_after_del, 2), "footer");
    }

    #[test]
    fn test_reverse_index_scroll_down_at_scroll_top() {
        // Case 1: Full screen (scroll_top = 0, scroll_bottom = 2)
        let mut mirror = RemoteTerminalMirror::new(10, 3).expect("new mirror");
        mirror.feed(b"row0\r\nrow1\r\nrow2").expect("feed initial");
        mirror.feed(b"\x1b[H").expect("home");
        assert_eq!((mirror.cursor_x, mirror.cursor_y), (0, 0));

        // ESC M at cursor_y == scroll_top (0 == 0):
        // Shifts rows down, discards line at scroll_bottom (row2), inserts blank line at scroll_top.
        mirror.feed(b"\x1bM").expect("feed ESC M");
        let frame = mirror.full_frame().expect("full frame");
        assert_eq!(row_text(&frame, 0), "");
        assert_eq!(row_text(&frame, 1), "row0");
        assert_eq!(row_text(&frame, 2), "row1");
        assert_eq!((mirror.cursor_x, mirror.cursor_y), (0, 0));

        // Case 2: Custom scrolling region CSI 2;3r (scroll_top = 1, scroll_bottom = 2)
        let mut mirror2 = RemoteTerminalMirror::new(10, 3).expect("mirror2");
        mirror2.feed(b"header\r\nrow1\r\nrow2").expect("feed");
        mirror2.feed(b"\x1b[2;3r").expect("feed decstbm");
        assert_eq!(mirror2.scroll_top, 1);
        assert_eq!(mirror2.scroll_bottom, 2);

        // Position cursor at scroll_top: row 2 (cursor_y = 1)
        mirror2.feed(b"\x1b[2;1H").expect("move to row 2");
        assert_eq!(mirror2.cursor_y, 1);

        // ESC M at cursor_y == scroll_top:
        // Rows 1..=2 scroll down; row 0 ("header") is unaffected!
        mirror2.feed(b"\x1bM").expect("feed ESC M");
        let frame2 = mirror2.full_frame().expect("full frame 2");
        assert_eq!(row_text(&frame2, 0), "header");
        assert_eq!(row_text(&frame2, 1), "");
        assert_eq!(row_text(&frame2, 2), "row1");
        assert_eq!(mirror2.cursor_y, 1);

        // When cursor_y > scroll_top, ESC M decrements cursor_y without scrolling
        mirror2.feed(b"\x1b[3;1H").expect("move to row 3 (y=2)");
        assert_eq!(mirror2.cursor_y, 2);
        mirror2.feed(b"\x1bM").expect("feed ESC M");
        assert_eq!(mirror2.cursor_y, 1);
        let frame3 = mirror2.full_frame().expect("full frame 3");
        assert_eq!(row_text(&frame3, 0), "header");
        assert_eq!(row_text(&frame3, 1), "");
        assert_eq!(row_text(&frame3, 2), "row1");
    }

    #[test]
    fn test_combining_characters_and_wide_glyph_overwrite() {
        // 1. Wide character overwrite: overwriting '가' (2 cells) with 'A' (1 cell)
        // must clear the spacer tail so subsequent text is not shifted.
        let mut mirror = RemoteTerminalMirror::new(10, 2).expect("mirror");
        mirror.feed("가나".as_bytes()).expect("feed wide chars");
        assert_eq!(mirror.cursor_x, 4);

        // Move cursor back to col 0 with \r
        mirror.feed(b"\rA").expect("overwrite with A");
        assert_eq!(mirror.cursor_x, 1);

        let frame = mirror.full_frame().expect("full frame");
        let line0 = match &frame {
            RemoteGridFrame::Grid { lines, .. } => &lines[0],
            _ => panic!("expected Grid"),
        };
        // The text runs should contain 'A', ' ', and '나'
        let full_text: String = line0.runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(full_text, "A 나");

        // 2. Combining diacritical mark (U+0301) must be retained with width 0 and not consume an extra cell
        let mut mirror2 = RemoteTerminalMirror::new(10, 2).expect("mirror2");
        // 'e' + U+0301 (combining acute) + 'b'
        mirror2.feed("e\u{0301}b".as_bytes()).expect("feed combining");
        assert_eq!(mirror2.cursor_x, 2); // 'e\u{0301}' (cell 0, width 1) + 'b' (cell 1, width 1) = cursor at 2

        let frame2 = mirror2.full_frame().expect("full frame 2");
        let line0_2 = match &frame2 {
            RemoteGridFrame::Grid { lines, .. } => &lines[0],
            _ => panic!("expected Grid"),
        };
        let text2: String = line0_2.runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text2, "e\u{0301}b");
        let total_cells: u16 = line0_2.runs.iter().map(|r| r.cells).sum();
        assert_eq!(total_cells, 2);
    }
}
