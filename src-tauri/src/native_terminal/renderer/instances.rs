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
            cell.fg.map(|c| (c.r, c.g, c.b)).hash(&mut hasher);
            cell.bg.map(|c| (c.r, c.g, c.b)).hash(&mut hasher);
            (
                cell.bold,
                cell.italic,
                cell.underline,
                cell.inverse,
                cell.faint,
                cell.blink,
                cell.invisible,
                cell.strikethrough,
                cell.overline,
            )
                .hash(&mut hasher);
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
    let (cell_w, cell_h) = (config.cell_width_px as f32, config.cell_height_px as f32);
    let row_cells = snapshot.grid.get(row as usize);

    for col in 0..snapshot.cols {
        let cell = row_cells.and_then(|r| r.get(col as usize));
        let (px_x, px_y) = (col as f32 * cell_w, row as f32 * cell_h);

        let mut bg_color = cell.and_then(|c| c.bg).map_or(config.theme.background, |c| {
            [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, 1.0]
        });
        let mut fg_color = cell.and_then(|c| c.fg).map_or(config.theme.foreground, |c| {
            [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, 1.0]
        });

        if cell.map_or(false, |c| c.inverse) {
            std::mem::swap(&mut bg_color, &mut fg_color);
        }
        if cell.map_or(false, |c| c.faint) {
            fg_color = [
                (fg_color[0] + bg_color[0]) * 0.5,
                (fg_color[1] + bg_color[1]) * 0.5,
                (fg_color[2] + bg_color[2]) * 0.5,
                fg_color[3],
            ];
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
            let line_h = 1.0f32.max((cell_h * 0.08).round());
            if c.underline {
                bg_instances.push(RectInstance {
                    rect: [px_x, px_y + cell_h - line_h - 1.0, cell_w, line_h],
                    color: fg_color,
                });
            }
            if c.strikethrough {
                bg_instances.push(RectInstance {
                    rect: [px_x, px_y + (cell_h * 0.5 - line_h * 0.5).round(), cell_w, line_h],
                    color: fg_color,
                });
            }
            if c.overline {
                bg_instances.push(RectInstance {
                    rect: [px_x, px_y, cell_w, line_h],
                    color: fg_color,
                });
            }

            if c.wide != CellWide::SpacerTail
                && c.wide != CellWide::SpacerHead
                && !c.text.is_empty()
                && !c.invisible
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
            for rect in [
                [px_x, px_y, cell_w, line_t],
                [px_x, px_y + cell_h - line_t, cell_w, line_t],
                [px_x, px_y, line_t, cell_h],
                [px_x + cell_w - line_t, px_y, line_t, cell_h],
            ] {
                bg_instances.push(RectInstance { rect, color: cursor_color });
            }
        }
        CursorVisualStyle::Block => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_terminal::color::ColorRgb;
    use crate::native_terminal::renderer::gpu_context::GpuContext;
    use crate::native_terminal::renderer::scenario::single_cell_scenario;
    use crate::native_terminal::renderer::types::RendererTheme;
    use crate::native_terminal::snapshot::CellSnapshot;

    fn test_config() -> RendererConfig {
        RendererConfig {
            cell_width_px: 10,
            cell_height_px: 20,
            device_scale_factor: 1.0,
            theme: RendererTheme::default(),
        }
    }

    #[test]
    fn test_cell_underline_produces_rect_in_lower_cell() {
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(_) => return,
        };
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = test_config();
        let base = CellSnapshot { text: "A".into(), wide: CellWide::Narrow, ..Default::default() };
        let (bg_plain, _) = build_row_instances(0, &single_cell_scenario(base.clone()), None, &config, &mut atlas, &gpu.queue);
        let mut under = base;
        under.underline = true;
        let (bg_under, _) = build_row_instances(0, &single_cell_scenario(under), None, &config, &mut atlas, &gpu.queue);
        assert_eq!(bg_under.len(), bg_plain.len() + 1, "underlined cell produces one more RectInstance");
        let deco_y = bg_under.last().unwrap().rect[1];
        let cell_h = config.cell_height_px as f32;
        assert!(deco_y >= cell_h * 0.7 && deco_y < cell_h, "underline sits in lower part of cell: {deco_y}");
    }

    #[test]
    fn test_cell_strikethrough_produces_rect_in_vertical_middle() {
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(_) => return,
        };
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = test_config();
        let mut strike = CellSnapshot { text: "A".into(), wide: CellWide::Narrow, ..Default::default() };
        strike.strikethrough = true;
        let (bg_strike, _) = build_row_instances(0, &single_cell_scenario(strike), None, &config, &mut atlas, &gpu.queue);
        assert!(bg_strike.len() >= 2, "strikethrough produces rect");
        let deco_y = bg_strike.last().unwrap().rect[1];
        let cell_h = config.cell_height_px as f32;
        let mid_y = cell_h * 0.5;
        assert!((deco_y - mid_y).abs() <= cell_h * 0.2, "strikethrough sits near vertical middle: {deco_y}");
    }

    #[test]
    fn test_cell_overline_produces_rect_at_cell_top() {
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(_) => return,
        };
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = test_config();
        let mut over = CellSnapshot { text: "A".into(), wide: CellWide::Narrow, ..Default::default() };
        over.overline = true;
        let (bg_over, _) = build_row_instances(0, &single_cell_scenario(over), None, &config, &mut atlas, &gpu.queue);
        assert!(bg_over.len() >= 2, "overline produces rect");
        let deco_y = bg_over.last().unwrap().rect[1];
        let cell_h = config.cell_height_px as f32;
        assert!(deco_y >= 0.0 && deco_y <= cell_h * 0.15, "overline sits at cell top: {deco_y}");
    }

    #[test]
    fn test_cell_invisible_produces_zero_glyph_instances() {
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(_) => return,
        };
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = test_config();
        let mut invis = CellSnapshot { text: "A".into(), wide: CellWide::Narrow, ..Default::default() };
        invis.invisible = true;
        let (bg_invis, glyph_invis) = build_row_instances(0, &single_cell_scenario(invis), None, &config, &mut atlas, &gpu.queue);
        assert_eq!(bg_invis.len(), 1, "invisible cell keeps background rect");
        assert_eq!(glyph_invis.len(), 0, "invisible cell produces zero GlyphInstances");
    }

    #[test]
    fn test_cell_faint_glyph_color_differs_from_non_faint() {
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(_) => return,
        };
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = test_config();
        let normal = CellSnapshot {
            text: "A".into(),
            wide: CellWide::Narrow,
            fg: Some(ColorRgb { r: 200, g: 200, b: 200 }),
            bg: Some(ColorRgb { r: 0, g: 0, b: 0 }),
            ..Default::default()
        };
        let (_, glyph_normal) = build_row_instances(0, &single_cell_scenario(normal.clone()), None, &config, &mut atlas, &gpu.queue);
        let mut faint = normal;
        faint.faint = true;
        let (_, glyph_faint) = build_row_instances(0, &single_cell_scenario(faint), None, &config, &mut atlas, &gpu.queue);
        assert_eq!(glyph_normal.len(), 1);
        assert_eq!(glyph_faint.len(), 1);
        assert_ne!(glyph_faint[0].color, glyph_normal[0].color, "faint glyph color differs");
    }

    #[test]
    fn test_row_hash_invalidates_on_attribute_changes() {
        let config = RendererConfig::default();
        let base = CellSnapshot { text: "A".into(), wide: CellWide::Narrow, ..Default::default() };
        let h_base = compute_row_hash(0, &single_cell_scenario(base.clone()), None, &config);

        let mut faint = base.clone();
        faint.faint = true;
        assert_ne!(h_base, compute_row_hash(0, &single_cell_scenario(faint), None, &config));

        let mut invis = base.clone();
        invis.invisible = true;
        assert_ne!(h_base, compute_row_hash(0, &single_cell_scenario(invis), None, &config));

        let mut strike = base.clone();
        strike.strikethrough = true;
        assert_ne!(h_base, compute_row_hash(0, &single_cell_scenario(strike), None, &config));

        let mut over = base;
        over.overline = true;
        assert_ne!(h_base, compute_row_hash(0, &single_cell_scenario(over), None, &config));
    }
}
