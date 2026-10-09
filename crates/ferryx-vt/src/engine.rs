use std::collections::{BTreeMap, VecDeque};
use std::ffi::{c_int, c_void};
use std::ptr;

use fxsh::types::{
    Cell, ClipboardTarget, Color, Cursor, CursorStyle, Delta, Hyperlink, Modes, MouseEncoding, MouseMode, Palette, RowData, UiEvent, UiEventKind,
};
use session_core::replica::{normalize_size, Replica};

use crate::sys::{self, *};

pub const GRAPHEME_EXTRA_MAX: usize = 32;
pub const TITLE_MAX: usize = 4096;
pub const URI_MAX: usize = 2048;
pub const LINKS_MAX: usize = 1024;
pub const DEFAULT_SCROLLBACK_LINES: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    VtReply(Vec<u8>),
    Ui(UiEventKind),
}

#[derive(Default)]
struct Sink {
    effects: Vec<Effect>,
    cols: u16,
    rows: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The scrollback rows retained before the step are unchanged except for
    /// eviction from the front; `appended` new rows were pushed at the back.
    Appended { evicted: usize, appended: usize },
    /// Scrollback rows were rewritten (§2.3); line_ids must be reassigned.
    Rewritten,
}

struct Anchor {
    tracked: Tracked,
    hash: u64,
    y: u32,
}

pub struct VtEngine {
    term: Terminal,
    render: RenderState,
    sink: Box<Sink>,
    sb_rows: usize,
    cols: u16,
    rows: u16,
    screen: c_int,
    anchor: Option<Anchor>,
}

unsafe impl Send for VtEngine {}

fn check(r: GhosttyResult, what: &str) {
    assert!(r == SUCCESS, "libghostty-vt {what} failed: {r}");
}

unsafe extern "C" fn on_write_pty(_t: Terminal, ud: *mut c_void, data: *const u8, len: usize) {
    if ud.is_null() || data.is_null() || len == 0 {
        return;
    }
    let sink = unsafe { &mut *(ud as *mut Sink) };
    sink.effects.push(Effect::VtReply(unsafe { std::slice::from_raw_parts(data, len) }.to_vec()));
}

unsafe extern "C" fn on_bell(_t: Terminal, ud: *mut c_void) {
    if !ud.is_null() {
        unsafe { &mut *(ud as *mut Sink) }.effects.push(Effect::Ui(UiEventKind::Bell));
    }
}

unsafe extern "C" fn on_size(_t: Terminal, ud: *mut c_void, out: *mut SizeReport) -> bool {
    if ud.is_null() || out.is_null() {
        return false;
    }
    let sink = unsafe { &*(ud as *const Sink) };
    unsafe { *out = SizeReport { rows: sink.rows, columns: sink.cols, cell_width: 8, cell_height: 16 } };
    true
}

unsafe extern "C" fn on_device_attributes(_t: Terminal, _ud: *mut c_void, out: *mut DeviceAttributes) -> bool {
    if out.is_null() {
        return false;
    }
    let da = unsafe { &mut *out };
    da.conformance_level = 62;
    da.features = [0; 64];
    da.features[0] = 22;
    da.features[1] = 52;
    da.num_features = 2;
    da.device_type = 1;
    da.firmware_version = 10;
    da.rom_cartridge = 0;
    da.unit_id = 0;
    true
}

unsafe extern "C" fn on_clipboard_write(_t: Terminal, ud: *mut c_void, w: *const ClipboardWrite) -> c_int {
    if ud.is_null() || w.is_null() {
        return CLIPBOARD_WRITE_SUCCESS;
    }
    let w = unsafe { &*w };
    let target = match w.location {
        CLIPBOARD_SELECTION | CLIPBOARD_PRIMARY => ClipboardTarget::Primary,
        _ => ClipboardTarget::Clipboard,
    };
    let contents = if w.contents.is_null() { &[][..] } else { unsafe { std::slice::from_raw_parts(w.contents, w.contents_len) } };
    let text = contents
        .iter()
        .find(|c| c.mime.bytes().starts_with(b"text/plain"))
        .or(contents.first())
        .and_then(|c| std::str::from_utf8(c.data.bytes()).ok());
    if let Some(text) = text {
        unsafe { &mut *(ud as *mut Sink) }.effects.push(Effect::Ui(UiEventKind::ClipboardWrite { target, text: text.to_owned() }));
    }
    CLIPBOARD_WRITE_SUCCESS
}

unsafe extern "C" fn on_notification(_t: Terminal, ud: *mut c_void, n: *const DesktopNotification) {
    if ud.is_null() || n.is_null() {
        return;
    }
    let n = unsafe { &*n };
    let cut = |b: &[u8], max: usize| String::from_utf8_lossy(&b[..b.len().min(max)]).into_owned();
    let mut title = cut(n.title.bytes(), 256);
    while title.len() > 256 {
        title.pop();
    }
    let mut body = cut(n.body.bytes(), 4096);
    while body.len() > 4096 {
        body.pop();
    }
    unsafe { &mut *(ud as *mut Sink) }.effects.push(Effect::Ui(UiEventKind::Notification { title, body }));
}

impl VtEngine {
    pub fn new(cols: u16, rows: u16, scrollback_lines: usize) -> Self {
        let (cols, rows) = normalize_size(cols, rows);
        let mut term: Terminal = ptr::null_mut();
        let mut render: RenderState = ptr::null_mut();
        unsafe {
            check(ghostty_terminal_new(ptr::null(), &mut term, cols, rows), "terminal_new");
            check(ghostty_render_state_new(ptr::null(), &mut render), "render_state_new");
        }
        let mut sink = Box::new(Sink { effects: Vec::new(), cols, rows });
        let ud = &mut *sink as *mut Sink as *const c_void;
        unsafe {
            check(ghostty_terminal_set(term, OPT_USERDATA, ud), "set userdata");
            check(ghostty_terminal_set(term, OPT_WRITE_PTY, on_write_pty as *const c_void), "set write_pty");
            check(ghostty_terminal_set(term, OPT_BELL, on_bell as *const c_void), "set bell");
            check(ghostty_terminal_set(term, OPT_SIZE, on_size as *const c_void), "set size");
            check(ghostty_terminal_set(term, OPT_DEVICE_ATTRIBUTES, on_device_attributes as *const c_void), "set da");
            check(ghostty_terminal_set(term, OPT_CLIPBOARD_WRITE, on_clipboard_write as *const c_void), "set clipboard");
            check(ghostty_terminal_set(term, OPT_DESKTOP_NOTIFICATION, on_notification as *const c_void), "set notification");
            check(ghostty_terminal_set(term, OPT_SCROLLBACK_MAX_BYTES, ptr::null()), "unset scrollback bytes");
            let lines = scrollback_lines;
            check(ghostty_terminal_set(term, OPT_SCROLLBACK_MAX_LINES, &lines as *const usize as *const c_void), "set scrollback lines");
        }
        let mut e = VtEngine { term, render, sink, sb_rows: 0, cols, rows, screen: 0, anchor: None };
        e.arm();
        e
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.sink.effects)
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Step {
        unsafe { ghostty_terminal_vt_write(self.term, bytes.as_ptr(), bytes.len()) };
        self.observe()
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Step {
        let (cols, rows) = normalize_size(cols, rows);
        self.sink.cols = cols;
        self.sink.rows = rows;
        unsafe { check(ghostty_terminal_resize(self.term, cols, rows, 8, 16), "resize") };
        self.observe()
    }

    fn get<T>(&self, key: c_int, out: &mut T) -> GhosttyResult {
        unsafe { ghostty_terminal_get(self.term, key, out as *mut T as *mut c_void) }
    }

    fn get_ok<T: Default>(&self, key: c_int) -> T {
        let mut v = T::default();
        check(self.get(key, &mut v), "terminal_get");
        v
    }

    fn row_hash(&self, tag: c_int, y: u32, cols: u16) -> u64 {
        let mut links = LinkTable::default();
        let row = self.read_row(tag, y, cols, &mut links);
        let mut w = Vec::new();
        fxsh::Codec::enc(&row, &mut w);
        for (_, uri) in &links.uris {
            w.extend_from_slice(uri.as_bytes());
        }
        xxhash_rust::xxh3::xxh3_64(&w)
    }

    fn arm(&mut self) {
        if let Some(a) = self.anchor.take() {
            unsafe { ghostty_tracked_grid_ref_free(a.tracked) };
        }
        self.sb_rows = self.get_ok(DATA_SCROLLBACK_ROWS);
        self.cols = self.get_ok(DATA_COLS);
        self.rows = self.get_ok(DATA_ROWS);
        self.screen = self.get_ok(DATA_ACTIVE_SCREEN);
        if self.sb_rows == 0 {
            return;
        }
        let y = (self.sb_rows - 1) as u32;
        let mut tracked: Tracked = ptr::null_mut();
        unsafe { check(ghostty_terminal_grid_ref_track(self.term, Point::new(POINT_HISTORY, 0, y), &mut tracked), "grid_ref_track") };
        let hash = self.row_hash(POINT_HISTORY, y, self.cols);
        self.anchor = Some(Anchor { tracked, hash, y });
    }

    /// The P0-3 detector: compare size, active screen, and the position and
    /// content of a tracked reference on the newest scrollback row. It never
    /// misses a rewrite; it may report one that did not happen.
    fn observe(&mut self) -> Step {
        let before = self.sb_rows;
        let (cols, rows, screen): (u16, u16, c_int) = (self.get_ok(DATA_COLS), self.get_ok(DATA_ROWS), self.get_ok(DATA_ACTIVE_SCREEN));
        let sb: usize = self.get_ok(DATA_SCROLLBACK_ROWS);
        let step = if cols != self.cols || rows != self.rows || screen != self.screen {
            Step::Rewritten
        } else {
            match &self.anchor {
                None if before == 0 => Step::Appended { evicted: 0, appended: sb },
                None => Step::Rewritten,
                Some(a) => {
                    let mut c = PointCoordinate::default();
                    let live = unsafe { ghostty_tracked_grid_ref_has_value(a.tracked) }
                        && unsafe { ghostty_tracked_grid_ref_point(a.tracked, POINT_HISTORY, &mut c) } == SUCCESS;
                    if !live || c.y > a.y || c.y as usize >= sb {
                        Step::Rewritten
                    } else {
                        let evicted = (a.y - c.y) as usize;
                        if sb + evicted < before || self.row_hash(POINT_HISTORY, c.y, cols) != a.hash {
                            Step::Rewritten
                        } else {
                            Step::Appended { evicted, appended: sb + evicted - before }
                        }
                    }
                }
            }
        };
        self.arm();
        step
    }

    fn read_row(&self, tag: c_int, y: u32, cols: u16, links: &mut LinkTable) -> RowData {
        let mut gref = GridRef::default();
        unsafe { check(ghostty_terminal_grid_ref(self.term, Point::new(tag, 0, y), &mut gref), "grid_ref") };
        let mut row: Row = 0;
        unsafe { check(ghostty_grid_ref_row(&gref, &mut row), "grid_ref_row") };
        let mut wrapped = false;
        unsafe { check(ghostty_row_get(row, ROW_WRAP, &mut wrapped as *mut bool as *mut c_void), "row_get wrap") };
        let mut cells = Vec::with_capacity(cols as usize);
        for x in 0..cols {
            unsafe { check(ghostty_terminal_grid_ref(self.term, Point::new(tag, x, y), &mut gref), "grid_ref") };
            cells.push(self.read_cell(&gref, links));
        }
        RowData { wrapped, cells }
    }

    fn read_cell(&self, gref: &GridRef, links: &mut LinkTable) -> Cell {
        let mut raw: sys::Cell = 0;
        unsafe { check(ghostty_grid_ref_cell(gref, &mut raw), "grid_ref_cell") };
        let mut wide: u32 = 0;
        unsafe { check(ghostty_cell_get(raw, CELL_WIDE, &mut wide as *mut u32 as *mut c_void), "cell wide") };
        let mut cps = [0u32; 64];
        let mut n = 0usize;
        let r = unsafe { ghostty_grid_ref_graphemes(gref, cps.as_mut_ptr(), cps.len(), &mut n) };
        if r == OUT_OF_SPACE {
            n = cps.len();
        } else {
            check(r, "graphemes");
        }
        let codepoint = if n > 0 { cps[0] } else { 0 };
        let grapheme_extra = (n > 1).then(|| {
            let mut s = String::new();
            for cp in &cps[1..n] {
                if let Some(ch) = char::from_u32(*cp) {
                    if s.len() + ch.len_utf8() > GRAPHEME_EXTRA_MAX {
                        break;
                    }
                    s.push(ch);
                }
            }
            s
        });
        let grapheme_extra = grapheme_extra.filter(|s| !s.is_empty());
        let width = match wide {
            WIDE_WIDE => 2,
            WIDE_SPACER_TAIL => 0,
            _ => 1,
        };
        let mut st = Style::default();
        unsafe { check(ghostty_grid_ref_style(gref, &mut st), "grid_ref_style") };
        let mut attrs: u16 = 0;
        let set = |b: bool, bit: u16, a: &mut u16| {
            if b {
                *a |= 1 << bit
            }
        };
        set(st.bold, 0, &mut attrs);
        set(st.faint, 1, &mut attrs);
        set(st.italic, 2, &mut attrs);
        match st.underline {
            0 => {}
            2 => attrs |= 1 << 4,
            3 => attrs |= 1 << 5,
            _ => attrs |= 1 << 3,
        }
        set(st.blink, 6, &mut attrs);
        set(st.inverse, 7, &mut attrs);
        set(st.invisible, 8, &mut attrs);
        set(st.strikethrough, 9, &mut attrs);
        set(st.overline, 10, &mut attrs);
        let mut ubuf = [0u8; URI_MAX];
        let mut ulen = 0usize;
        let hr = unsafe { ghostty_grid_ref_hyperlink_uri(gref, ubuf.as_mut_ptr(), ubuf.len(), &mut ulen) };
        let hyperlink_id = if hr == SUCCESS && ulen > 0 {
            std::str::from_utf8(&ubuf[..ulen]).ok().map_or(0, |u| links.intern(u))
        } else {
            0
        };
        Cell {
            codepoint,
            grapheme_extra,
            width,
            fg: color(st.fg),
            bg: color(st.bg),
            underline_color: color(st.underline_color),
            attrs,
            hyperlink_id,
        }
    }

    fn mode(&self, m: u16) -> bool {
        let mut mc = ModeConfig { mode: m, value: false };
        self.get(DATA_MODE, &mut mc) == SUCCESS && mc.value
    }

    fn modes(&self) -> Modes {
        let bits: [(u16, u32); 11] = [
            (dec_mode(1049), 0),
            (dec_mode(1), 1),
            (dec_mode(66), 2),
            (dec_mode(2004), 3),
            (dec_mode(1004), 4),
            (dec_mode(6), 5),
            (dec_mode(7), 6),
            (dec_mode(5), 7),
            (ansi_mode(4), 8),
            (dec_mode(25), 9),
            (dec_mode(2026), 10),
        ];
        let mut flags = 0u32;
        for (m, bit) in bits {
            if self.mode(m) {
                flags |= 1 << bit;
            }
        }
        if self.get_ok::<c_int>(DATA_ACTIVE_SCREEN) == 1 {
            flags |= 1;
        }
        let mouse_mode = if self.mode(dec_mode(1003)) {
            MouseMode::Any
        } else if self.mode(dec_mode(1002)) {
            MouseMode::Button
        } else if self.mode(dec_mode(1000)) {
            MouseMode::Normal
        } else if self.mode(dec_mode(9)) {
            MouseMode::X10
        } else {
            MouseMode::Off
        };
        let mouse_encoding = if self.mode(dec_mode(1016)) {
            MouseEncoding::SgrPixels
        } else if self.mode(dec_mode(1006)) {
            MouseEncoding::Sgr
        } else if self.mode(dec_mode(1015)) {
            MouseEncoding::Urxvt
        } else if self.mode(dec_mode(1005)) {
            MouseEncoding::Utf8
        } else {
            MouseEncoding::Default
        };
        Modes { flags, mouse_mode, mouse_encoding }
    }

    fn rgb(&self, key: c_int, default_key: c_int) -> Color {
        let (mut cur, mut def) = (Rgb::default(), Rgb::default());
        let has = self.get(key, &mut cur) == SUCCESS;
        let has_def = self.get(default_key, &mut def) == SUCCESS;
        match (has, has_def) {
            (true, true) if cur == def => Color::Default,
            (true, _) => Color::Rgb(cur.r, cur.g, cur.b),
            _ => Color::Default,
        }
    }

    fn palette(&self) -> Palette {
        let mut cur = [Rgb::default(); 256];
        let mut def = [Rgb::default(); 256];
        check(self.get(DATA_COLOR_PALETTE, &mut cur), "palette");
        check(self.get(DATA_COLOR_PALETTE_DEFAULT, &mut def), "palette default");
        let overrides = (0..256usize).filter(|i| cur[*i] != def[*i]).map(|i| (i as u8, (cur[i].r, (cur[i].g, cur[i].b)))).collect();
        let cursor_color = match self.rgb(DATA_COLOR_CURSOR, DATA_COLOR_CURSOR_DEFAULT) {
            Color::Default => None,
            c => Some(c),
        };
        Palette {
            default_fg: self.rgb(DATA_COLOR_FOREGROUND, DATA_COLOR_FOREGROUND_DEFAULT),
            default_bg: self.rgb(DATA_COLOR_BACKGROUND, DATA_COLOR_BACKGROUND_DEFAULT),
            cursor_color,
            overrides,
        }
    }

    fn cursor(&self) -> Cursor {
        unsafe { check(ghostty_render_state_update(self.render, self.term), "render_state_update") };
        let (mut style, mut visible, mut blinking): (c_int, bool, bool) = (1, true, false);
        unsafe {
            ghostty_render_state_get(self.render, RS_CURSOR_VISUAL_STYLE, &mut style as *mut c_int as *mut c_void);
            ghostty_render_state_get(self.render, RS_CURSOR_VISIBLE, &mut visible as *mut bool as *mut c_void);
            ghostty_render_state_get(self.render, RS_CURSOR_BLINKING, &mut blinking as *mut bool as *mut c_void);
        }
        let style = match style {
            0 => CursorStyle::Bar,
            2 => CursorStyle::Underline,
            _ => CursorStyle::Block,
        };
        let (x, y): (u16, u16) = (self.get_ok(DATA_CURSOR_X), self.get_ok(DATA_CURSOR_Y));
        Cursor { row: y.min(self.rows.saturating_sub(1)), col: x.min(self.cols.saturating_sub(1)), style, blinking, visible }
    }

    fn title(&self) -> String {
        let mut s = GString::default();
        if self.get(DATA_TITLE, &mut s) != SUCCESS {
            return String::new();
        }
        let b = s.bytes();
        let mut t = String::from_utf8_lossy(&b[..b.len().min(TITLE_MAX)]).into_owned();
        while t.len() > TITLE_MAX {
            t.pop();
        }
        t
    }

    pub fn read_screen(&self, links: &mut LinkTable) -> ScreenState {
        let screen = (0..self.rows as u32).map(|y| self.read_row(POINT_ACTIVE, y, self.cols, links)).collect();
        ScreenState { cols: self.cols, rows: self.rows, screen, cursor: self.cursor(), modes: self.modes(), palette: self.palette(), title: self.title() }
    }

    pub fn scrollback_rows(&self) -> usize {
        self.sb_rows
    }

    pub fn read_scrollback(&self, from: usize, links: &mut LinkTable) -> Vec<RowData> {
        (from..self.sb_rows).map(|y| self.read_row(POINT_HISTORY, y as u32, self.cols, links)).collect()
    }
}

impl Drop for VtEngine {
    fn drop(&mut self) {
        if let Some(a) = self.anchor.take() {
            unsafe { ghostty_tracked_grid_ref_free(a.tracked) };
        }
        unsafe {
            ghostty_render_state_free(self.render);
            ghostty_terminal_free(self.term);
        }
    }
}

fn color(c: StyleColor) -> Color {
    match c.tag {
        STYLE_COLOR_PALETTE => Color::Indexed(unsafe { c.value.palette }),
        STYLE_COLOR_RGB => {
            let v = unsafe { c.value.rgb };
            Color::Rgb(v.r, v.g, v.b)
        }
        _ => Color::Default,
    }
}

pub struct ScreenState {
    pub cols: u16,
    pub rows: u16,
    pub screen: Vec<RowData>,
    pub cursor: Cursor,
    pub modes: Modes,
    pub palette: Palette,
    pub title: String,
}

/// Hyperlink ids for one session incarnation: an id is never reused (§A.1),
/// and at most `LINKS_MAX` URIs are assigned (§2.2); later links are dropped.
#[derive(Default)]
pub struct LinkTable {
    by_uri: BTreeMap<String, u32>,
    pub uris: BTreeMap<u32, String>,
    next: u32,
}

impl LinkTable {
    pub fn intern(&mut self, uri: &str) -> u32 {
        if let Some(id) = self.by_uri.get(uri) {
            return *id;
        }
        if self.uris.len() >= LINKS_MAX {
            return 0;
        }
        self.next += 1;
        self.by_uri.insert(uri.to_owned(), self.next);
        self.uris.insert(self.next, uri.to_owned());
        self.next
    }

    pub fn retain_referenced(&mut self, referenced: &std::collections::BTreeSet<u32>) {
        self.uris.retain(|id, _| referenced.contains(id));
        self.by_uri.retain(|_, id| referenced.contains(id));
    }
}

pub struct HostTerminal {
    pub engine: VtEngine,
    pub replica: Replica,
    pub links: LinkTable,
    next_line_id: u64,
    next_event_id: u64,
    row_bytes: VecDeque<usize>,
    scrollback_bytes: usize,
    trimmed: usize,
}

fn encoded_len<T: fxsh::Codec>(v: &T) -> usize {
    let mut w = Vec::new();
    v.enc(&mut w);
    w.len()
}

pub struct Advance {
    pub rewritten: bool,
    pub changed: bool,
    pub delta: Option<Delta>,
    pub vt_replies: Vec<Vec<u8>>,
    pub ui_events: Vec<UiEvent>,
}

struct Light {
    screen: Vec<RowData>,
    cursor: Cursor,
    modes: Modes,
    palette: Palette,
    title: String,
    links: BTreeMap<u32, String>,
    had_scrollback: bool,
}

impl HostTerminal {
    pub fn new(cols: u16, rows: u16, scrollback_lines: usize) -> Self {
        let engine = VtEngine::new(cols, rows, scrollback_lines);
        let mut t = HostTerminal {
            replica: Replica::new(engine.cols(), engine.rows()),
            engine,
            links: LinkTable::default(),
            next_line_id: 1,
            next_event_id: 1,
            row_bytes: VecDeque::new(),
            scrollback_bytes: 0,
            trimmed: 0,
        };
        t.rebuild(true);
        t
    }

    pub fn next_ui_event_id(&self) -> u64 {
        self.next_event_id
    }

    fn light(&self) -> Light {
        let r = &self.replica;
        Light {
            screen: r.screen.clone(),
            cursor: r.cursor.clone(),
            modes: r.modes.clone(),
            palette: r.palette.clone(),
            title: r.title.clone(),
            links: r.hyperlinks.clone(),
            had_scrollback: !r.scrollback.is_empty(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Advance {
        let light = self.light();
        let step = self.engine.feed(bytes);
        self.advance(step, light)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Advance {
        let light = self.light();
        let step = self.engine.resize(cols, rows);
        self.advance(step, light)
    }

    fn advance(&mut self, step: Step, light: Light) -> Advance {
        let mut appended_rows: Vec<(u64, RowData)> = Vec::new();
        let mut evicted_any = false;
        let rewritten = match step {
            Step::Rewritten => {
                self.rebuild(true);
                true
            }
            Step::Appended { evicted, appended } => {
                let from_trimmed = evicted.min(self.trimmed);
                self.trimmed -= from_trimmed;
                for _ in 0..(evicted - from_trimmed).min(self.replica.scrollback.len()) {
                    self.pop_front_row();
                    evicted_any = true;
                }
                let sb = self.engine.scrollback_rows();
                let from = sb.saturating_sub(appended);
                let rows = self.engine.read_scrollback(from, &mut self.links);
                for row in rows {
                    appended_rows.push((self.next_line_id, row.clone()));
                    self.push_back_row(row);
                }
                if self.replica.scrollback.len() + self.trimmed != sb {
                    self.rebuild(true);
                    true
                } else {
                    self.rebuild(false);
                    false
                }
            }
        };
        let mut vt_replies = Vec::new();
        let mut ui_events = Vec::new();
        for e in self.engine.take_effects() {
            match e {
                Effect::VtReply(b) => vt_replies.push(b),
                Effect::Ui(kind) => {
                    ui_events.push(UiEvent { event_id: self.next_event_id, kind });
                    self.next_event_id += 1;
                }
            }
        }
        evicted_any |= self.enforce_limit(&mut appended_rows);
        let delta = (!rewritten).then(|| self.build_delta(&light, appended_rows, evicted_any, &ui_events));
        let changed = rewritten || delta.as_ref().is_some_and(state_changed);
        Advance { rewritten, changed, delta, vt_replies, ui_events }
    }

    fn push_back_row(&mut self, row: RowData) {
        let n = encoded_len(&row) + 8;
        self.row_bytes.push_back(n);
        self.scrollback_bytes += n;
        self.replica.scrollback.push_back((self.next_line_id, row));
        self.next_line_id += 1;
    }

    fn pop_front_row(&mut self) {
        self.replica.scrollback.pop_front();
        if let Some(n) = self.row_bytes.pop_front() {
            self.scrollback_bytes -= n;
        }
    }

    fn non_scrollback_len(&self) -> usize {
        let r = &self.replica;
        let screen: usize = r.screen.iter().map(encoded_len).sum();
        let links: usize = r.hyperlinks.iter().map(|(_, u)| 8 + u.len()).sum();
        256 + screen + encoded_len(&r.cursor) + encoded_len(&r.modes) + encoded_len(&r.palette) + 4 + r.title.len() + links
    }

    /// Keeps canonical_encode(S) at or below S_MAX (§2.2) by evicting the
    /// oldest scrollback rows; returns whether any row was evicted.
    fn enforce_limit(&mut self, appended: &mut Vec<(u64, RowData)>) -> bool {
        let limit = session_core::replica::S_MAX;
        let fixed = self.non_scrollback_len();
        let mut evicted = false;
        while fixed + self.scrollback_bytes > limit && !self.replica.scrollback.is_empty() {
            self.pop_front_row();
            self.trimmed += 1;
            evicted = true;
        }
        if evicted {
            let first = self.replica.first_line_id().unwrap_or(self.next_line_id);
            appended.retain(|(id, _)| *id >= first);
        }
        evicted
    }

    fn build_delta(&self, light: &Light, appended: Vec<(u64, RowData)>, evicted: bool, ui_events: &[UiEvent]) -> Delta {
        let r = &self.replica;
        let evicted_before = (evicted && light.had_scrollback).then(|| r.first_line_id().unwrap_or(self.next_line_id));
        Delta {
            subscription_id: 0,
            base_revision: 0,
            new_revision: 0,
            size: None,
            cursor: (light.cursor != r.cursor).then(|| r.cursor.clone()),
            modes: (light.modes != r.modes).then(|| r.modes.clone()),
            palette: (light.palette != r.palette).then(|| r.palette.clone()),
            title: (light.title != r.title).then(|| r.title.clone()),
            dirty_rows: r.screen.iter().enumerate().filter(|(i, row)| light.screen.get(*i) != Some(row)).map(|(i, row)| (i as u16, row.clone())).collect(),
            hyperlinks_added: r.hyperlinks.iter().filter(|(id, uri)| light.links.get(id) != Some(uri)).map(|(id, uri)| Hyperlink { id: *id, uri: uri.clone() }).collect(),
            scrollback_appended: appended,
            scrollback_evicted_before: evicted_before,
            exit_info: None,
            ui_events: ui_events.to_vec(),
        }
    }

    fn rebuild(&mut self, scrollback: bool) {
        let s = self.engine.read_screen(&mut self.links);
        if scrollback {
            let rows = self.engine.read_scrollback(0, &mut self.links);
            self.replica.scrollback = VecDeque::new();
            self.row_bytes.clear();
            self.scrollback_bytes = 0;
            self.trimmed = 0;
            for row in rows {
                self.push_back_row(row);
            }
        }
        let r = &mut self.replica;
        r.cols = s.cols;
        r.rows = s.rows;
        r.screen = s.screen;
        r.cursor = s.cursor;
        r.modes = s.modes;
        r.palette = s.palette;
        r.title = s.title;
        let referenced = r.referenced_links();
        self.links.retain_referenced(&referenced);
        r.hyperlinks = self.links.uris.clone();
    }

    pub fn set_exit(&mut self, exit: fxsh::types::ExitInfo) {
        self.replica.exit_info = Some(exit);
    }
}

pub fn state_changed(d: &Delta) -> bool {
    d.size.is_some()
        || d.cursor.is_some()
        || d.modes.is_some()
        || d.palette.is_some()
        || d.title.is_some()
        || !d.dirty_rows.is_empty()
        || !d.hyperlinks_added.is_empty()
        || !d.scrollback_appended.is_empty()
        || d.scrollback_evicted_before.is_some()
        || d.exit_info.is_some()
}
