use std::collections::{BTreeMap, BTreeSet, VecDeque};

use fxsh::types::{
    Cell, Color, Cursor, CursorStyle, Delta, ExitInfo, Hyperlink, Modes, MouseEncoding, MouseMode, Palette, RowData,
    SBody, UiEvent,
};
use fxsh::Codec;

pub const S_MAX: usize = 64 * 1024 * 1024;
pub const MAX_COLS: u16 = 1024;
pub const MAX_ROWS: u16 = 512;

pub fn normalize_size(cols: u16, rows: u16) -> (u16, u16) {
    (cols.clamp(1, MAX_COLS), rows.clamp(1, MAX_ROWS))
}

pub fn blank_row(cols: u16) -> RowData {
    RowData { wrapped: false, cells: vec![Cell::BLANK; cols as usize] }
}

pub fn default_cursor() -> Cursor {
    Cursor { row: 0, col: 0, style: CursorStyle::Block, blinking: false, visible: true }
}

pub fn default_modes() -> Modes {
    Modes { flags: (1 << 6) | (1 << 9), mouse_mode: MouseMode::Off, mouse_encoding: MouseEncoding::Default }
}

pub fn default_palette() -> Palette {
    Palette { default_fg: Color::Default, default_bg: Color::Default, cursor_color: None, overrides: Vec::new() }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replica {
    pub cols: u16,
    pub rows: u16,
    pub screen: Vec<RowData>,
    pub cursor: Cursor,
    pub modes: Modes,
    pub palette: Palette,
    pub title: String,
    pub hyperlinks: BTreeMap<u32, String>,
    pub scrollback: VecDeque<(u64, RowData)>,
    pub exit_info: Option<ExitInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffError {
    ScrollbackRewritten,
}

impl Replica {
    pub fn new(cols: u16, rows: u16) -> Self {
        let (cols, rows) = normalize_size(cols, rows);
        Replica {
            cols,
            rows,
            screen: (0..rows).map(|_| blank_row(cols)).collect(),
            cursor: default_cursor(),
            modes: default_modes(),
            palette: default_palette(),
            title: String::new(),
            hyperlinks: BTreeMap::new(),
            scrollback: VecDeque::new(),
            exit_info: None,
        }
    }

    pub fn from_body(b: &SBody) -> Self {
        Replica {
            cols: b.cols,
            rows: b.rows,
            screen: b.screen.clone(),
            cursor: b.cursor.clone(),
            modes: b.modes.clone(),
            palette: b.palette.clone(),
            title: b.title.clone(),
            hyperlinks: b.hyperlinks.iter().map(|h| (h.id, h.uri.clone())).collect(),
            scrollback: b.scrollback.iter().cloned().collect(),
            exit_info: b.exit_info.clone(),
        }
    }

    pub fn to_body(&self) -> SBody {
        SBody {
            cols: self.cols,
            rows: self.rows,
            screen: self.screen.clone(),
            cursor: self.cursor.clone(),
            modes: self.modes.clone(),
            palette: self.palette.clone(),
            title: self.title.clone(),
            hyperlinks: self.hyperlinks.iter().map(|(id, uri)| Hyperlink { id: *id, uri: uri.clone() }).collect(),
            scrollback: self.scrollback.iter().cloned().collect(),
            exit_info: self.exit_info.clone(),
        }
    }

    pub fn referenced_links(&self) -> BTreeSet<u32> {
        let rows = self.screen.iter().chain(self.scrollback.iter().map(|(_, r)| r));
        rows.flat_map(|r| r.cells.iter().map(|c| c.hyperlink_id)).filter(|id| *id != 0).collect()
    }

    pub fn prune_hyperlinks(&mut self) {
        let keep = self.referenced_links();
        self.hyperlinks.retain(|id, _| keep.contains(id));
    }

    pub fn last_line_id(&self) -> Option<u64> {
        self.scrollback.back().map(|(id, _)| *id)
    }

    pub fn first_line_id(&self) -> Option<u64> {
        self.scrollback.front().map(|(id, _)| *id)
    }

    pub fn apply_delta(&mut self, d: &Delta) -> Vec<UiEvent> {
        if let Some((cols, rows)) = d.size {
            self.cols = cols;
            self.rows = rows;
            self.screen.resize_with(rows as usize, || blank_row(cols));
            for row in &mut self.screen {
                row.cells.resize(cols as usize, Cell::BLANK);
            }
        }
        if let Some(c) = &d.cursor {
            self.cursor = c.clone();
        }
        if let Some(m) = &d.modes {
            self.modes = m.clone();
        }
        if let Some(p) = &d.palette {
            self.palette = p.clone();
        }
        if let Some(t) = &d.title {
            self.title = t.clone();
        }
        for (row, data) in &d.dirty_rows {
            if let Some(slot) = self.screen.get_mut(*row as usize) {
                *slot = data.clone();
            }
        }
        for h in &d.hyperlinks_added {
            self.hyperlinks.insert(h.id, h.uri.clone());
        }
        if let Some(before) = d.scrollback_evicted_before {
            while self.first_line_id().is_some_and(|id| id < before) {
                self.scrollback.pop_front();
            }
        }
        for (id, data) in &d.scrollback_appended {
            if self.last_line_id().map_or(true, |last| *id > last) {
                self.scrollback.push_back((*id, data.clone()));
            }
        }
        if let Some(e) = &d.exit_info {
            self.exit_info = Some(e.clone());
        }
        self.prune_hyperlinks();
        d.ui_events.clone()
    }

    pub fn encoded_len(&self) -> usize {
        let mut w = Vec::new();
        self.to_body().enc(&mut w);
        w.len()
    }

    pub fn evict_to_limit(&mut self, limit: usize) -> Option<u64> {
        let mut total = self.encoded_len();
        let mut evicted = None;
        while total > limit {
            let Some((id, row)) = self.scrollback.pop_front() else { break };
            let mut w = Vec::new();
            (id, row).enc(&mut w);
            total -= w.len();
            evicted = Some(id + 1);
        }
        if evicted.is_some() {
            self.prune_hyperlinks();
        }
        evicted
    }
}

pub fn diff(prev: &Replica, next: &Replica, subscription_id: u64, base: u64, new: u64) -> Result<Delta, DiffError> {
    let scroll_common = prev.scrollback.iter().filter(|(id, _)| next.first_line_id().is_some_and(|f| *id >= f));
    let next_by_id: BTreeMap<u64, &RowData> = next.scrollback.iter().map(|(id, r)| (*id, r)).collect();
    for (id, row) in scroll_common {
        match next_by_id.get(id) {
            Some(r) if *r == row => {}
            _ => return Err(DiffError::ScrollbackRewritten),
        }
    }
    let prev_last = prev.last_line_id();
    let appended: Vec<(u64, RowData)> =
        next.scrollback.iter().filter(|(id, _)| prev_last.map_or(true, |l| *id > l)).cloned().collect();
    if let (Some(pl), Some(nf)) = (prev_last, next.first_line_id()) {
        if nf <= pl && !prev.scrollback.iter().any(|(id, _)| *id == nf) {
            return Err(DiffError::ScrollbackRewritten);
        }
    }
    let evicted_before = match (prev.first_line_id(), next.first_line_id()) {
        (Some(pf), Some(nf)) if nf > pf => Some(nf),
        (Some(_), None) => Some(prev_last.map_or(0, |l| l + 1).max(next.last_line_id().map_or(0, |l| l + 1))),
        _ => None,
    };
    let size_changed = prev.cols != next.cols || prev.rows != next.rows;
    let dirty_rows: Vec<(u16, RowData)> = next
        .screen
        .iter()
        .enumerate()
        .filter(|(i, row)| size_changed || prev.screen.get(*i) != Some(row))
        .map(|(i, row)| (i as u16, row.clone()))
        .collect();
    let hyperlinks_added = next
        .hyperlinks
        .iter()
        .filter(|(id, uri)| prev.hyperlinks.get(id) != Some(uri))
        .map(|(id, uri)| Hyperlink { id: *id, uri: uri.clone() })
        .collect();
    Ok(Delta {
        subscription_id,
        base_revision: base,
        new_revision: new,
        size: size_changed.then_some((next.cols, next.rows)),
        cursor: (prev.cursor != next.cursor).then(|| next.cursor.clone()),
        modes: (prev.modes != next.modes).then(|| next.modes.clone()),
        palette: (prev.palette != next.palette).then(|| next.palette.clone()),
        title: (prev.title != next.title).then(|| next.title.clone()),
        dirty_rows,
        hyperlinks_added,
        scrollback_appended: appended,
        scrollback_evicted_before: evicted_before,
        exit_info: (prev.exit_info != next.exit_info).then(|| next.exit_info.clone()).flatten(),
        ui_events: Vec::new(),
    })
}

pub fn merge(deltas: &[Delta]) -> Option<Delta> {
    let first = deltas.first()?;
    let last = deltas.last()?;
    let mut out = Delta {
        subscription_id: first.subscription_id,
        base_revision: first.base_revision,
        new_revision: last.new_revision,
        size: None,
        cursor: None,
        modes: None,
        palette: None,
        title: None,
        dirty_rows: Vec::new(),
        hyperlinks_added: Vec::new(),
        scrollback_appended: Vec::new(),
        scrollback_evicted_before: None,
        exit_info: None,
        ui_events: Vec::new(),
    };
    let mut rows: BTreeMap<u16, RowData> = BTreeMap::new();
    let mut links: BTreeMap<u32, String> = BTreeMap::new();
    for d in deltas {
        if d.size.is_some() {
            out.size = d.size;
        }
        if d.cursor.is_some() {
            out.cursor = d.cursor.clone();
        }
        if d.modes.is_some() {
            out.modes = d.modes.clone();
        }
        if d.palette.is_some() {
            out.palette = d.palette.clone();
        }
        if d.title.is_some() {
            out.title = d.title.clone();
        }
        if d.exit_info.is_some() {
            out.exit_info = d.exit_info.clone();
        }
        for (r, data) in &d.dirty_rows {
            rows.insert(*r, data.clone());
        }
        for h in &d.hyperlinks_added {
            links.insert(h.id, h.uri.clone());
        }
        out.scrollback_appended.extend(d.scrollback_appended.iter().cloned());
        out.scrollback_evicted_before = match (out.scrollback_evicted_before, d.scrollback_evicted_before) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        out.ui_events.extend(d.ui_events.iter().cloned());
    }
    if let Some((_, final_rows)) = out.size {
        rows.retain(|r, _| *r < final_rows);
    }
    if let Some(before) = out.scrollback_evicted_before {
        out.scrollback_appended.retain(|(id, _)| *id >= before);
    }
    out.dirty_rows = rows.into_iter().collect();
    out.hyperlinks_added = links.into_iter().map(|(id, uri)| Hyperlink { id, uri }).collect();
    Some(out)
}

pub fn delta_encoded_len(d: &Delta) -> usize {
    let mut w = Vec::new();
    d.enc(&mut w);
    w.len()
}
