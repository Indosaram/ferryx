//! Terminal grid instance batch generator for background and glyph passes.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::atlas::GlyphAtlas;
use super::pipeline::{GlyphInstance, RectInstance};
use super::types::{RendererConfig, SelectionSnapshot};
use crate::native_terminal::cursor::CursorVisualStyle;
use crate::native_terminal::snapshot::{CellWide, RenderSnapshot};

#[derive(Clone, Debug, Default)]
pub struct RowCacheEntry {
    pub hash: u64,
    pub bg_instances: Vec<RectInstance>,
    pub glyph_instances: Vec<GlyphInstance>,
}

pub fn compute_row_hash(
    row: u16,
    snapshot: &RenderSnapshot,
    selection: Option<&SelectionSnapshot>,
    config: &RendererConfig,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    row.hash(&mut hasher);
    config.cell_width_px.hash(&mut hasher);
    config.cell_height_px.hash(&mut hasher);
    config.theme.hash(&mut hasher);
    snapshot.cols.hash(&mut hasher);

    if let Some(cells) = snapshot.grid.get(row as usize) {
        for cell in cells {
            cell.text.hash(&mut hasher);
            (cell.wide as u8).hash(&mut hasher);
            if let Some(fg) = &cell.fg {
                1u8.hash(&mut hasher);
                fg.r.hash(&mut hasher);
                fg.g.hash(&mut hasher);
                fg.b.hash(&mut hasher);
            } else {
                0u8.hash(&mut hasher);
            }
            if let Some(bg) = &cell.bg {
                1u8.hash(&mut hasher);
                bg.r.hash(&mut hasher);
                bg.g.hash(&mut hasher);
                bg.b.hash(&mut hasher);
            } else {
                0u8.hash(&mut hasher);
            }
            cell.bold.hash(&mut hasher);
            cell.italic.hash(&mut hasher);
            cell.underline.hash(&mut hasher);
            cell.inverse.hash(&mut hasher);
        }
    }

    let is_cursor_row = snapshot.cursor.visible && snapshot.cursor.y == row;
    is_cursor_row.hash(&mut hasher);
    if is_cursor_row {
        snapshot.cursor.x.hash(&mut hasher);
        (snapshot.cursor.visual_style as u8).hash(&mut hasher);
    }

    if let Some(sel) = selection {
        for col in 0..snapshot.cols {
            sel.contains_cell(col, row).hash(&mut hasher);
        }
    } else {
        false.hash(&mut hasher);
    }

    hasher.finish()
}

pub fn build_row_instances(
    row: u16,
    snapshot: &RenderSnapshot,
    selection: Option<&SelectionSnapshot>,
    config: &RendererConfig,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
) -> (Vec<RectInstance>, Vec<GlyphInstance>) {
    let mut bg_instances = Vec::with_capacity(snapshot.cols as usize);
    let mut glyph_instances = Vec::new();

    let cell_w = config.cell_width_px as f32;
    let cell_h = config.cell_height_px as f32;
    let row_cells = snapshot.grid.get(row as usize);

    for col in 0..snapshot.cols {
        let cell = row_cells.and_then(|r| r.get(col as usize));
        let px_x = col as f32 * cell_w;
        let px_y = row as f32 * cell_h;

        let default_bg = config.theme.background;
        let default_fg = config.theme.foreground;

        let mut bg_color = cell.and_then(|c| c.bg).map_or(default_bg, |c| {
            [
                c.r as f32 / 255.0,
                c.g as f32 / 255.0,
                c.b as f32 / 255.0,
                1.0,
            ]
        });

        let mut fg_color = cell.and_then(|c| c.fg).map_or(default_fg, |c| {
            [
                c.r as f32 / 255.0,
                c.g as f32 / 255.0,
                c.b as f32 / 255.0,
                1.0,
            ]
        });

        if cell.map_or(false, |c| c.inverse) {
            std::mem::swap(&mut bg_color, &mut fg_color);
        }

        if selection.map_or(false, |s| s.contains_cell(col, row)) {
            bg_color = config.theme.selection_background;
            fg_color = config.theme.selection_foreground;
        }

        let is_cursor =
            snapshot.cursor.visible && snapshot.cursor.x == col && snapshot.cursor.y == row;

        let effective_cursor_style = if snapshot.cursor.visual_style == CursorVisualStyle::Block {
            config.theme.cursor_style
        } else {
            snapshot.cursor.visual_style
        };

        if is_cursor && effective_cursor_style == CursorVisualStyle::Block {
            bg_color = config.theme.cursor;
            fg_color = config.theme.cursor_accent;
        }

        bg_instances.push(RectInstance {
            rect: [px_x, px_y, cell_w, cell_h],
            color: bg_color,
        });

        if is_cursor {
            append_cursor_decorations(
                &mut bg_instances,
                effective_cursor_style,
                px_x,
                px_y,
                cell_w,
                cell_h,
                config.theme.cursor,
            );
        }

        if let Some(c) = cell {
            if c.wide != CellWide::SpacerTail
                && c.wide != CellWide::SpacerHead
                && !c.text.is_empty()
            {
                let is_wide = c.wide == CellWide::Wide;
                if let Some(entry) =
                    atlas.get_or_insert(&c.text, c.bold, c.italic, is_wide, config, queue)
                {
                    glyph_instances.push(GlyphInstance {
                        rect: [px_x, px_y, entry.width as f32, entry.height as f32],
                        uv: [
                            entry.uv_min[0],
                            entry.uv_min[1],
                            entry.uv_max[0],
                            entry.uv_max[1],
                        ],
                        color: fg_color,
                        is_color: if entry.is_color { 1.0 } else { 0.0 },
                        _pad: [0.0; 3],
                    });
                }
            }
        }
    }

    (bg_instances, glyph_instances)
}

fn append_cursor_decorations(
    bg_instances: &mut Vec<RectInstance>,
    style: CursorVisualStyle,
    px_x: f32,
    px_y: f32,
    cell_w: f32,
    cell_h: f32,
    cursor_color: [f32; 4],
) {
    match style {
        CursorVisualStyle::Bar => {
            bg_instances.push(RectInstance {
                rect: [px_x, px_y, 2.0f32.min(cell_w), cell_h],
                color: cursor_color,
            });
        }
        CursorVisualStyle::Underline => {
            bg_instances.push(RectInstance {
                rect: [px_x, px_y + cell_h - 2.0, cell_w, 2.0],
                color: cursor_color,
            });
        }
        CursorVisualStyle::BlockHollow => {
            let line_t = 1.5;
            bg_instances.push(RectInstance {
                rect: [px_x, px_y, cell_w, line_t],
                color: cursor_color,
            });
            bg_instances.push(RectInstance {
                rect: [px_x, px_y + cell_h - line_t, cell_w, line_t],
                color: cursor_color,
            });
            bg_instances.push(RectInstance {
                rect: [px_x, px_y, line_t, cell_h],
                color: cursor_color,
            });
            bg_instances.push(RectInstance {
                rect: [px_x + cell_w - line_t, px_y, line_t, cell_h],
                color: cursor_color,
            });
        }
        CursorVisualStyle::Block => {}
    }
}
