use std::collections::{BTreeMap, BTreeSet, VecDeque};

use fxsh::types::{
    Cell, Color, Cursor, CursorStyle, Delta, ExitInfo, Hyperlink, Modes, MouseEncoding, MouseMode, Palette, RowData,
    SBody, UiEvent, UiEventKind,
};
use fxsh::wire::Digest;

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
        // Matches `SBody::enc` byte-for-byte without cloning or allocating.
        let screen: usize = self.screen.iter().map(row_encoded_len).sum();
        let links: usize = self.hyperlinks.values().map(|uri| 8 + uri.len()).sum();
        let scrollback: usize = self.scrollback.iter().map(|(_, r)| 8 + row_encoded_len(r)).sum();
        let exit = match &self.exit_info {
            None => 1,
            Some(e) => 1 + exit_info_encoded_len(e),
        };
        2 // cols
            + 2 // rows
            + 4 + screen
            + CURSOR_ENCODED_LEN
            + MODES_ENCODED_LEN
            + palette_encoded_len(&self.palette)
            + 4 + self.title.len()
            + 4 + links
            + 4 + scrollback
            + exit
    }

    /// Computes `fxsh::state_digest(&self.to_body())` by streaming the canonical
    /// `SBody` wire representation directly into XXH3-128 without cloning the
    /// replica or allocating an intermediate buffer.
    pub fn state_digest(&self) -> Digest {
        let mut h = DigestWriter::new();
        h.put_u16(self.cols);
        h.put_u16(self.rows);
        h.put_len(self.screen.len());
        for row in &self.screen {
            h.put_row(row);
        }
        h.put_cursor(&self.cursor);
        h.put_modes(&self.modes);
        h.put_palette(&self.palette);
        h.put_str(&self.title);
        h.put_len(self.hyperlinks.len());
        for (id, uri) in &self.hyperlinks {
            h.put_u32(*id);
            h.put_str(uri);
        }
        h.put_len(self.scrollback.len());
        for (id, row) in &self.scrollback {
            h.put_u64(*id);
            h.put_row(row);
        }
        match &self.exit_info {
            None => h.put_u8(0),
            Some(e) => {
                h.put_u8(1);
                h.put_exit_info(e);
            }
        }
        h.finish()
    }

    pub fn evict_to_limit(&mut self, limit: usize) -> Option<u64> {
        let mut total = self.encoded_len();
        let mut evicted = None;
        while total > limit {
            let Some((id, row)) = self.scrollback.pop_front() else { break };
            total -= 8 + row_encoded_len(&row);
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

/// Owned counterpart to [`merge`] that moves `RowData`, hyperlink URIs, and
/// `UiEvent`s out of the input deltas instead of deep-cloning them.
pub fn merge_owned<I: IntoIterator<Item = Delta>>(deltas: I) -> Option<Delta> {
    let mut iter = deltas.into_iter();
    let first = iter.next()?;
    let Some(second) = iter.next() else {
        let rows_sorted = first.dirty_rows.windows(2).all(|w| w[0].0 < w[1].0);
        let links_sorted = first.hyperlinks_added.windows(2).all(|w| w[0].id < w[1].id);
        if rows_sorted && links_sorted {
            let mut out = first;
            if let Some((_, final_rows)) = out.size {
                out.dirty_rows.retain(|(r, _)| *r < final_rows);
            }
            if let Some(before) = out.scrollback_evicted_before {
                out.scrollback_appended.retain(|(id, _)| *id >= before);
            }
            return Some(out);
        }
        let mut rows: BTreeMap<u16, RowData> = first.dirty_rows.into_iter().collect();
        let links: BTreeMap<u32, String> = first.hyperlinks_added.into_iter().map(|h| (h.id, h.uri)).collect();
        if let Some((_, final_rows)) = first.size {
            rows.retain(|r, _| *r < final_rows);
        }
        let mut scrollback_appended = first.scrollback_appended;
        if let Some(before) = first.scrollback_evicted_before {
            scrollback_appended.retain(|(id, _)| *id >= before);
        }
        return Some(Delta {
            dirty_rows: rows.into_iter().collect(),
            hyperlinks_added: links.into_iter().map(|(id, uri)| Hyperlink { id, uri }).collect(),
            scrollback_appended,
            ..first
        });
    };

    let mut out = Delta {
        subscription_id: first.subscription_id,
        base_revision: first.base_revision,
        new_revision: first.new_revision,
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
    for d in [first, second].into_iter().chain(iter) {
        out.new_revision = d.new_revision;
        if d.size.is_some() {
            out.size = d.size;
        }
        if d.cursor.is_some() {
            out.cursor = d.cursor;
        }
        if d.modes.is_some() {
            out.modes = d.modes;
        }
        if d.palette.is_some() {
            out.palette = d.palette;
        }
        if d.title.is_some() {
            out.title = d.title;
        }
        if d.exit_info.is_some() {
            out.exit_info = d.exit_info;
        }
        for (r, data) in d.dirty_rows {
            rows.insert(r, data);
        }
        for h in d.hyperlinks_added {
            links.insert(h.id, h.uri);
        }
        out.scrollback_appended.extend(d.scrollback_appended);
        out.scrollback_evicted_before = match (out.scrollback_evicted_before, d.scrollback_evicted_before) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        out.ui_events.extend(d.ui_events);
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

pub const CURSOR_ENCODED_LEN: usize = 7;
pub const MODES_ENCODED_LEN: usize = 6;

#[inline]
pub fn color_encoded_len(c: Color) -> usize {
    match c {
        Color::Default => 1,
        Color::Indexed(_) => 2,
        Color::Rgb(_, _, _) => 4,
    }
}

#[inline]
pub fn cell_encoded_len(c: &Cell) -> usize {
    // codepoint(4) + width(1) + attrs(2) + hyperlink_id(4) = 11
    11 + match &c.grapheme_extra {
        None => 1,
        Some(s) => 5 + s.len(),
    } + color_encoded_len(c.fg)
        + color_encoded_len(c.bg)
        + color_encoded_len(c.underline_color)
}

#[inline]
pub fn row_encoded_len(r: &RowData) -> usize {
    // wrapped(1) + cells len(4) + cells
    5 + r.cells.iter().map(cell_encoded_len).sum::<usize>()
}

#[inline]
pub fn palette_encoded_len(p: &Palette) -> usize {
    color_encoded_len(p.default_fg)
        + color_encoded_len(p.default_bg)
        + match p.cursor_color {
            None => 1,
            Some(c) => 1 + color_encoded_len(c),
        }
        + 4
        + p.overrides.len() * 4
}

#[inline]
pub fn exit_info_encoded_len(e: &ExitInfo) -> usize {
    (if e.exit_code.is_some() { 5 } else { 1 }) + (if e.posix_signal.is_some() { 2 } else { 1 })
}

#[inline]
pub fn ui_event_encoded_len(e: &UiEvent) -> usize {
    8 + match &e.kind {
        UiEventKind::ClipboardWrite { text, .. } => 6 + text.len(),
        UiEventKind::Notification { title, body } => 9 + title.len() + body.len(),
        UiEventKind::Bell => 1,
    }
}

pub fn delta_encoded_len(d: &Delta) -> usize {
    let mut len = 24; // subscription_id(8) + base_revision(8) + new_revision(8)
    len += if d.size.is_some() { 5 } else { 1 };
    len += if d.cursor.is_some() { 1 + CURSOR_ENCODED_LEN } else { 1 };
    len += if d.modes.is_some() { 1 + MODES_ENCODED_LEN } else { 1 };
    len += match &d.palette {
        None => 1,
        Some(p) => 1 + palette_encoded_len(p),
    };
    len += match &d.title {
        None => 1,
        Some(t) => 5 + t.len(),
    };
    len += 4 + d.dirty_rows.iter().map(|(_, r)| 2 + row_encoded_len(r)).sum::<usize>();
    len += 4 + d.hyperlinks_added.iter().map(|h| 8 + h.uri.len()).sum::<usize>();
    len += 4 + d.scrollback_appended.iter().map(|(_, r)| 8 + row_encoded_len(r)).sum::<usize>();
    len += if d.scrollback_evicted_before.is_some() { 9 } else { 1 };
    len += match &d.exit_info {
        None => 1,
        Some(e) => 1 + exit_info_encoded_len(e),
    };
    len += 4 + d.ui_events.iter().map(ui_event_encoded_len).sum::<usize>();
    len
}

struct DigestWriter(xxhash_rust::xxh3::Xxh3Default);

impl DigestWriter {
    fn new() -> Self {
        Self(xxhash_rust::xxh3::Xxh3Default::new())
    }

    #[inline]
    fn put_u8(&mut self, v: u8) {
        self.0.update(&[v]);
    }

    #[inline]
    fn put_u16(&mut self, v: u16) {
        self.0.update(&v.to_be_bytes());
    }

    #[inline]
    fn put_u32(&mut self, v: u32) {
        self.0.update(&v.to_be_bytes());
    }

    #[inline]
    fn put_u64(&mut self, v: u64) {
        self.0.update(&v.to_be_bytes());
    }

    #[inline]
    fn put_i32(&mut self, v: i32) {
        self.0.update(&v.to_be_bytes());
    }

    #[inline]
    fn put_bool(&mut self, v: bool) {
        self.put_u8(v as u8);
    }

    #[inline]
    fn put_len(&mut self, len: usize) {
        self.put_u32(u32::try_from(len).expect("FXSH length field exceeds u32"));
    }

    #[inline]
    fn put_str(&mut self, s: &str) {
        self.put_len(s.len());
        self.0.update(s.as_bytes());
    }

    #[inline]
    fn put_color(&mut self, c: Color) {
        match c {
            Color::Default => self.put_u8(0),
            Color::Indexed(i) => self.0.update(&[1, i]),
            Color::Rgb(r, g, b) => self.0.update(&[2, r, g, b]),
        }
    }

    #[inline]
    fn put_cell(&mut self, c: &Cell) {
        self.put_u32(c.codepoint);
        match &c.grapheme_extra {
            None => self.put_u8(0),
            Some(s) => {
                self.put_u8(1);
                self.put_str(s);
            }
        }
        self.put_u8(c.width);
        self.put_color(c.fg);
        self.put_color(c.bg);
        self.put_color(c.underline_color);
        self.put_u16(c.attrs);
        self.put_u32(c.hyperlink_id);
    }

    fn put_row(&mut self, r: &RowData) {
        self.put_bool(r.wrapped);
        self.put_len(r.cells.len());
        for cell in &r.cells {
            self.put_cell(cell);
        }
    }

    fn put_cursor(&mut self, c: &Cursor) {
        self.put_u16(c.row);
        self.put_u16(c.col);
        self.put_u8(c.style as u8);
        self.put_bool(c.blinking);
        self.put_bool(c.visible);
    }

    fn put_modes(&mut self, m: &Modes) {
        self.put_u32(m.flags);
        self.put_u8(m.mouse_mode as u8);
        self.put_u8(m.mouse_encoding as u8);
    }

    fn put_palette(&mut self, p: &Palette) {
        self.put_color(p.default_fg);
        self.put_color(p.default_bg);
        match p.cursor_color {
            None => self.put_u8(0),
            Some(c) => {
                self.put_u8(1);
                self.put_color(c);
            }
        }
        self.put_len(p.overrides.len());
        for &(idx, (r, (g, b))) in &p.overrides {
            self.0.update(&[idx, r, g, b]);
        }
    }

    fn put_exit_info(&mut self, e: &ExitInfo) {
        match e.exit_code {
            None => self.put_u8(0),
            Some(code) => {
                self.put_u8(1);
                self.put_i32(code);
            }
        }
        match e.posix_signal {
            None => self.put_u8(0),
            Some(sig) => {
                self.put_u8(1);
                self.put_u8(sig);
            }
        }
    }

    fn finish(self) -> Digest {
        Digest(self.0.digest128().to_be_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fxsh::types::ClipboardTarget;
    use fxsh::Codec;

    fn sample_row(wrapped: bool) -> RowData {
        RowData {
            wrapped,
            cells: vec![
                Cell::BLANK,
                Cell {
                    codepoint: 'A' as u32,
                    grapheme_extra: Some("\u{0301}".to_owned()),
                    width: 1,
                    fg: Color::Indexed(12),
                    bg: Color::Rgb(1, 2, 3),
                    underline_color: Color::Rgb(4, 5, 6),
                    attrs: 0x15,
                    hyperlink_id: 7,
                },
                Cell {
                    codepoint: 0x1F600,
                    grapheme_extra: None,
                    width: 2,
                    fg: Color::Default,
                    bg: Color::Indexed(200),
                    underline_color: Color::Default,
                    attrs: 0,
                    hyperlink_id: 0,
                },
            ],
        }
    }

    #[test]
    fn delta_encoded_len_matches_wire_codec_across_populated_deltas() {
        let empty = Delta {
            subscription_id: 1,
            base_revision: 0,
            new_revision: 1,
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
        let populated = Delta {
            subscription_id: 42,
            base_revision: 10,
            new_revision: 15,
            size: Some((80, 24)),
            cursor: Some(Cursor {
                row: 3,
                col: 12,
                style: CursorStyle::Bar,
                blinking: true,
                visible: false,
            }),
            modes: Some(Modes {
                flags: 0x055,
                mouse_mode: MouseMode::Any,
                mouse_encoding: MouseEncoding::SgrPixels,
            }),
            palette: Some(Palette {
                default_fg: Color::Rgb(240, 240, 240),
                default_bg: Color::Indexed(0),
                cursor_color: Some(Color::Rgb(255, 128, 0)),
                overrides: vec![(1, (10, (20, 30))), (15, (250, (251, 252)))],
            }),
            title: Some("ferryx-session-\u{2693}".to_owned()),
            dirty_rows: vec![(0, sample_row(false)), (5, sample_row(true))],
            hyperlinks_added: vec![
                Hyperlink { id: 7, uri: "https://example.com/a".to_owned() },
                Hyperlink { id: 9, uri: "https://example.com/b?x=1".to_owned() },
            ],
            scrollback_appended: vec![(100, sample_row(true)), (101, sample_row(false))],
            scrollback_evicted_before: Some(50),
            exit_info: Some(ExitInfo { exit_code: Some(130), posix_signal: Some(2) }),
            ui_events: vec![
                UiEvent { event_id: 1, kind: UiEventKind::Bell },
                UiEvent {
                    event_id: 2,
                    kind: UiEventKind::ClipboardWrite {
                        target: ClipboardTarget::Clipboard,
                        text: "hello clipboard".to_owned(),
                    },
                },
                UiEvent {
                    event_id: 3,
                    kind: UiEventKind::Notification {
                        title: "Done".to_owned(),
                        body: "Build finished".to_owned(),
                    },
                },
            ],
        };
        let partial_exit = Delta {
            exit_info: Some(ExitInfo { exit_code: None, posix_signal: Some(9) }),
            palette: Some(default_palette()),
            ..empty.clone()
        };

        for d in [&empty, &populated, &partial_exit] {
            let mut v = Vec::new();
            d.enc(&mut v);
            assert_eq!(delta_encoded_len(d), v.len());
        }
    }

    #[test]
    fn replica_encoded_len_and_state_digest_match_to_body() {
        let mut r = Replica::new(3, 2);
        r.screen[0] = sample_row(false);
        r.screen[1] = sample_row(true);
        r.cursor = Cursor { row: 1, col: 2, style: CursorStyle::Underline, blinking: true, visible: true };
        r.modes = Modes { flags: 7, mouse_mode: MouseMode::Normal, mouse_encoding: MouseEncoding::Sgr };
        r.palette = Palette {
            default_fg: Color::Rgb(1, 2, 3),
            default_bg: Color::Default,
            cursor_color: Some(Color::Indexed(5)),
            overrides: vec![(2, (7, (8, 9)))],
        };
        r.title = "demo".to_owned();
        r.hyperlinks.insert(7, "https://example.com/a".to_owned());
        r.scrollback.push_back((1, sample_row(true)));
        r.scrollback.push_back((2, sample_row(false)));
        r.exit_info = Some(ExitInfo { exit_code: Some(0), posix_signal: None });

        let body = r.to_body();
        let mut encoded = Vec::new();
        body.enc(&mut encoded);
        assert_eq!(r.encoded_len(), encoded.len());
        assert_eq!(r.state_digest(), fxsh::state_digest(&body));

        r.exit_info = None;
        r.hyperlinks.clear();
        r.scrollback.clear();
        let body2 = r.to_body();
        let mut encoded2 = Vec::new();
        body2.enc(&mut encoded2);
        assert_eq!(r.encoded_len(), encoded2.len());
        assert_eq!(r.state_digest(), fxsh::state_digest(&body2));
    }

    #[test]
    fn merge_owned_matches_borrowed_merge() {
        let d1 = Delta {
            subscription_id: 3,
            base_revision: 1,
            new_revision: 2,
            size: Some((3, 2)),
            cursor: Some(default_cursor()),
            modes: None,
            palette: None,
            title: Some("first".to_owned()),
            dirty_rows: vec![(0, sample_row(false)), (1, sample_row(true))],
            hyperlinks_added: vec![Hyperlink { id: 1, uri: "https://a".to_owned() }],
            scrollback_appended: vec![(1, sample_row(false)), (2, sample_row(true))],
            scrollback_evicted_before: None,
            exit_info: None,
            ui_events: vec![UiEvent { event_id: 1, kind: UiEventKind::Bell }],
        };
        let d2 = Delta {
            subscription_id: 3,
            base_revision: 2,
            new_revision: 3,
            size: Some((3, 1)),
            cursor: None,
            modes: Some(default_modes()),
            palette: None,
            title: Some("second".to_owned()),
            dirty_rows: vec![(0, sample_row(true))],
            hyperlinks_added: vec![Hyperlink { id: 2, uri: "https://b".to_owned() }],
            scrollback_appended: vec![(3, sample_row(false))],
            scrollback_evicted_before: Some(2),
            exit_info: Some(ExitInfo { exit_code: Some(0), posix_signal: None }),
            ui_events: vec![UiEvent { event_id: 2, kind: UiEventKind::Bell }],
        };

        assert_eq!(merge_owned(std::iter::empty()), None);
        assert_eq!(merge_owned(vec![d1.clone()]), merge(&[d1.clone()]));
        assert_eq!(merge_owned(vec![d1.clone(), d2.clone()]), merge(&[d1, d2]));
    }
}
