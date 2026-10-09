use crate::wire::{record, u8_enum, Bytes, Codec, DResult, Digest, Reader, Reason};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Uuid(pub [u8; 16]);

u8_enum!(ClientKind { GuiWindow = 1, RemoteClient = 2, PolicyDaemon = 3 });
u8_enum!(Supervisor { Launchd = 1, Systemd = 2, TaskScheduler = 3 });
u8_enum!(Scope { Input = 1, Resize = 2 });
u8_enum!(Signal { Hangup = 1, Interrupt = 2, Terminate = 3, Kill = 4 });
u8_enum!(OperationKind { Spawn = 1, Kill = 2 });
u8_enum!(CursorStyle { Block = 0, Underline = 1, Bar = 2 });
u8_enum!(MouseMode { Off = 0, X10 = 1, Normal = 2, Button = 3, Any = 4 });
u8_enum!(MouseEncoding { Default = 0, Utf8 = 1, Sgr = 2, Urxvt = 3, SgrPixels = 4 });
u8_enum!(ClipboardTarget { Clipboard = 1, Primary = 2 });

record!(OperationId { host_instance_id: Uuid, op_seq: u64 });

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl Codec for Color {
    fn enc(&self, w: &mut Vec<u8>) {
        match *self {
            Color::Default => 0u8.enc(w),
            Color::Indexed(i) => {
                1u8.enc(w);
                i.enc(w);
            }
            Color::Rgb(r, g, b) => {
                2u8.enc(w);
                r.enc(w);
                g.enc(w);
                b.enc(w);
            }
        }
    }
    fn dec(r: &mut Reader<'_>) -> DResult<Self> {
        Ok(match r.u8()? {
            0 => Color::Default,
            1 => Color::Indexed(r.u8()?),
            2 => Color::Rgb(r.u8()?, r.u8()?, r.u8()?),
            _ => return Err(Reason::BadTag),
        })
    }
}

pub const ATTR_MASK: u16 = (1 << 11) - 1;

record!(Cell {
    codepoint: u32,
    grapheme_extra: Option<String>,
    width: u8,
    fg: Color,
    bg: Color,
    underline_color: Color,
    attrs: u16,
    hyperlink_id: u32,
} check |c| {
    if c.width > 2 { return Err(Reason::BadEnum); }
    if c.attrs & !ATTR_MASK != 0 { return Err(Reason::BadShape); }
    if matches!(&c.grapheme_extra, Some(s) if s.is_empty()) { return Err(Reason::OptCondition); }
    Ok(())
});

impl Cell {
    pub const BLANK: Cell = Cell {
        codepoint: 0,
        grapheme_extra: None,
        width: 1,
        fg: Color::Default,
        bg: Color::Default,
        underline_color: Color::Default,
        attrs: 0,
        hyperlink_id: 0,
    };
}

record!(RowData { wrapped: bool, cells: Vec<Cell> });

record!(Cursor { row: u16, col: u16, style: CursorStyle, blinking: bool, visible: bool });

record!(Palette {
    default_fg: Color,
    default_bg: Color,
    cursor_color: Option<Color>,
    overrides: Vec<(u8, (u8, (u8, u8)))>,
} check |p| crate::wire::strictly_ascending(&p.overrides, |o| o.0));

record!(Hyperlink { id: u32, uri: String } check |h| if h.id == 0 { Err(Reason::BadShape) } else { Ok(()) });

record!(ExitInfo { exit_code: Option<i32>, posix_signal: Option<u8> });

pub const MODE_FLAG_MASK: u32 = (1 << 11) - 1;

record!(Modes {
    flags: u32,
    mouse_mode: MouseMode,
    mouse_encoding: MouseEncoding,
} check |m| if m.flags & !MODE_FLAG_MASK != 0 { Err(Reason::BadShape) } else { Ok(()) });

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEventKind {
    ClipboardWrite { target: ClipboardTarget, text: String },
    Notification { title: String, body: String },
    Bell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiEvent {
    pub event_id: u64,
    pub kind: UiEventKind,
}

impl Codec for UiEvent {
    fn enc(&self, w: &mut Vec<u8>) {
        self.event_id.enc(w);
        match &self.kind {
            UiEventKind::ClipboardWrite { target, text } => {
                1u8.enc(w);
                target.enc(w);
                text.enc(w);
            }
            UiEventKind::Notification { title, body } => {
                2u8.enc(w);
                title.enc(w);
                body.enc(w);
            }
            UiEventKind::Bell => 3u8.enc(w),
        }
    }
    fn dec(r: &mut Reader<'_>) -> DResult<Self> {
        let event_id = r.u64()?;
        let kind = match r.u8()? {
            1 => UiEventKind::ClipboardWrite { target: Codec::dec(r)?, text: r.string()? },
            2 => {
                let title = r.string()?;
                let body = r.string()?;
                if title.len() > 256 || body.len() > 4096 {
                    return Err(Reason::BadShape);
                }
                UiEventKind::Notification { title, body }
            }
            3 => UiEventKind::Bell,
            _ => return Err(Reason::BadEnum),
        };
        Ok(UiEvent { event_id, kind })
    }
}

record!(Delta {
    subscription_id: u64,
    base_revision: u64,
    new_revision: u64,
    size: Option<(u16, u16)>,
    cursor: Option<Cursor>,
    modes: Option<Modes>,
    palette: Option<Palette>,
    title: Option<String>,
    dirty_rows: Vec<(u16, RowData)>,
    hyperlinks_added: Vec<Hyperlink>,
    scrollback_appended: Vec<(u64, RowData)>,
    scrollback_evicted_before: Option<u64>,
    exit_info: Option<ExitInfo>,
    ui_events: Vec<UiEvent>,
} check |d| {
    crate::wire::strictly_ascending(&d.dirty_rows, |r| r.0)?;
    crate::wire::strictly_ascending(&d.hyperlinks_added, |h| h.id)?;
    crate::wire::strictly_ascending(&d.scrollback_appended, |r| r.0)
});

record!(SBody {
    cols: u16,
    rows: u16,
    screen: Vec<RowData>,
    cursor: Cursor,
    modes: Modes,
    palette: Palette,
    title: String,
    hyperlinks: Vec<Hyperlink>,
    scrollback: Vec<(u64, RowData)>,
    exit_info: Option<ExitInfo>,
} check |s| {
    if s.screen.len() != s.rows as usize { return Err(Reason::BadShape); }
    let width_ok = |row: &RowData| row.cells.len() == s.cols as usize;
    if !s.screen.iter().all(width_ok) || !s.scrollback.iter().all(|(_, row)| width_ok(row)) {
        return Err(Reason::BadShape);
    }
    crate::wire::strictly_ascending(&s.hyperlinks, |h| h.id)?;
    crate::wire::strictly_ascending(&s.scrollback, |r| r.0)
});

record!(Body {
    revision: u64,
    state_digest: Digest,
    next_ui_event_id: u64,
    ui_event_gap: bool,
    state: SBody,
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotPayload {
    Full(Body),
    Chunk { index: u32, total: u32, total_len: u32, bytes: Bytes },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotFrame {
    pub subscription_id: u64,
    pub session_incarnation: Uuid,
    pub snapshot_id: u64,
    pub payload: SnapshotPayload,
}

impl Codec for SnapshotFrame {
    fn enc(&self, w: &mut Vec<u8>) {
        self.subscription_id.enc(w);
        self.session_incarnation.enc(w);
        self.snapshot_id.enc(w);
        match &self.payload {
            SnapshotPayload::Full(b) => {
                0u8.enc(w);
                b.enc(w);
            }
            SnapshotPayload::Chunk { index, total, total_len, bytes } => {
                1u8.enc(w);
                index.enc(w);
                total.enc(w);
                total_len.enc(w);
                bytes.enc(w);
            }
        }
    }
    fn dec(r: &mut Reader<'_>) -> DResult<Self> {
        let subscription_id = r.u64()?;
        let session_incarnation = r.uuid()?;
        let snapshot_id = r.u64()?;
        let payload = match r.u8()? {
            0 => SnapshotPayload::Full(Body::dec(r)?),
            1 => {
                let index = r.u32()?;
                let total = r.u32()?;
                let total_len = r.u32()?;
                let bytes = Bytes::dec(r)?;
                if total == 0 || index >= total || bytes.0.len() > total_len as usize {
                    return Err(Reason::ChunkInconsistent);
                }
                SnapshotPayload::Chunk { index, total, total_len, bytes }
            }
            _ => return Err(Reason::BadTag),
        };
        Ok(SnapshotFrame { subscription_id, session_incarnation, snapshot_id, payload })
    }
}

record!(SessionState {
    session_incarnation: Uuid,
    state_revision: u64,
    state_digest: Digest,
    input_epoch: u64,
    input_accepted: u64,
    input_committed: u64,
    resize_epoch: u64,
    resize_seq: u64,
    cols: u16,
    rows: u16,
    child_running: bool,
    exit_info: Option<ExitInfo>,
    pending_dropped_by_epoch: Vec<(u64, u64)>,
    vt_replies_dropped: u64,
    creation_operation_id: OperationId,
} check |s| {
    if s.child_running == s.exit_info.is_some() { return Err(Reason::OptCondition); }
    if s.pending_dropped_by_epoch.len() > 128 { return Err(Reason::BadShape); }
    Ok(())
});

record!(SessionEntry {
    session_id: Uuid,
    session_incarnation: Uuid,
    created_table_revision: u64,
    creation_operation_id: OperationId,
    child_running: bool,
    exit_info: Option<ExitInfo>,
} check |s| if s.child_running == s.exit_info.is_some() { Err(Reason::OptCondition) } else { Ok(()) });

record!(SessionList {
    request_token: (Uuid, u64),
    owner_instance_id: Uuid,
    table_revision: u64,
    complete: bool,
    sessions: Vec<SessionEntry>,
});

pub fn canonical_encode(s: &SBody) -> Vec<u8> {
    let mut w = Vec::new();
    s.enc(&mut w);
    w
}

pub fn state_digest(s: &SBody) -> Digest {
    Digest(xxhash_rust::xxh3::xxh3_128(&canonical_encode(s)).to_be_bytes())
}
