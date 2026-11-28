//! Terminal grid instance batch generator for background and glyph passes.

use std::hash::{Hash, Hasher};

use super::atlas::{GlyphAtlas, GlyphKey, PreparedGlyphs};
use super::pipeline::{GlyphInstance, RectInstance};
use super::types::{RendererConfig, SelectionSnapshot};
use crate::native_terminal::cursor::CursorVisualStyle;
use crate::native_terminal::error::NativeTerminalError;
use crate::native_terminal::snapshot::{CellSnapshot, CellWide, RenderSnapshot};

const FNV1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_PRIME: u64 = 0x0100_0000_01b3;

struct Fnv1aHasher(u64);

impl Fnv1aHasher {
    #[inline]
    fn new() -> Self {
        Self(FNV1A_OFFSET_BASIS)
    }
}

impl Hasher for Fnv1aHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut hash = self.0;
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV1A_PRIME);
        }
        self.0 = hash;
    }
}

#[inline]
fn has_under_background_image(snapshot: &RenderSnapshot) -> bool {
    snapshot.images.iter().any(|p| p.z < i32::MIN / 2)
}

#[inline]
fn push_bg_rect(
    bg_instances: &mut Vec<RectInstance>,
    rect: [f32; 4],
    color: [f32; 4],
    merge: bool,
) {
    if merge {
        if let Some(last) = bg_instances.last_mut() {
            if last.color == color && last.rect[1] == rect[1] && last.rect[3] == rect[3] {
                last.rect[2] += rect[2];
                return;
            }
        }
    }
    bg_instances.push(RectInstance { rect, color });
}

fn visible_glyph(cell: &CellSnapshot) -> bool {
    cell.wide != CellWide::SpacerTail
        && cell.wide != CellWide::SpacerHead
        && !cell.text.is_empty()
        && !cell.invisible
}

pub fn prepare_visible_glyphs(
    snapshot: &RenderSnapshot,
    config: &RendererConfig,
) -> PreparedGlyphs {
    let mut prepared = PreparedGlyphs::default();
    for row in snapshot.grid.iter().take(snapshot.rows as usize) {
        for cell in row
            .iter()
            .take(snapshot.cols as usize)
            .filter(|c| visible_glyph(c))
        {
            prepared.add(
                GlyphKey {
                    text: cell.text.clone(),
                    bold: cell.bold,
                    italic: cell.italic,
                },
                cell.wide == CellWide::Wide,
                config,
            );
        }
    }
    prepared
}

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
    let mut hasher = Fnv1aHasher::new();
    row.hash(&mut hasher);
    config.cell_width_px.hash(&mut hasher);
    config.cell_height_px.hash(&mut hasher);
    config.theme.hash(&mut hasher);
    snapshot.cols.hash(&mut hasher);
    has_under_background_image(snapshot).hash(&mut hasher);

    if let Some(cells) = snapshot.grid.get(row as usize) {
        for cell in cells {
            cell.text.hash(&mut hasher);
            (cell.wide as u8).hash(&mut hasher);
            cell.fg.map(|c| (c.r, c.g, c.b)).hash(&mut hasher);
            cell.bg.map(|c| (c.r, c.g, c.b)).hash(&mut hasher);
            let attrs: u16 = u16::from(cell.bold)
                | (u16::from(cell.italic) << 1)
                | (u16::from(cell.underline) << 2)
                | (u16::from(cell.inverse) << 3)
                | (u16::from(cell.faint) << 4)
                | (u16::from(cell.blink) << 5)
                | (u16::from(cell.invisible) << 6)
                | (u16::from(cell.strikethrough) << 7)
                | (u16::from(cell.overline) << 8);
            attrs.hash(&mut hasher);
        }
    }

    let is_cursor_row = snapshot.cursor.visible && snapshot.cursor.y == row;
    is_cursor_row.hash(&mut hasher);
    if is_cursor_row {
        snapshot.cursor.x.hash(&mut hasher);
        (snapshot.cursor.visual_style as u8).hash(&mut hasher);
    }

    if let Some(sel) = selection {
        true.hash(&mut hasher);
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
    prepared: Option<&PreparedGlyphs>,
) -> Result<(Vec<RectInstance>, Vec<GlyphInstance>), NativeTerminalError> {
    // Kitty images with z < i32::MIN / 2 rely on per-cell bg rects in
    // `image_background_occluders`; merge contiguous runs on all other frames.
    let merge_bg = !has_under_background_image(snapshot);
    let mut bg_instances = Vec::with_capacity(if merge_bg {
        8.min(snapshot.cols as usize)
    } else {
        snapshot.cols as usize
    });
    let mut decorations = Vec::new();
    let mut glyph_instances = Vec::new();
    let (cell_w, cell_h) = (config.cell_width_px as f32, config.cell_height_px as f32);
    let row_cells = snapshot.grid.get(row as usize);

    for col in 0..snapshot.cols {
        let cell = row_cells.and_then(|r| r.get(col as usize));
        let (px_x, px_y) = (col as f32 * cell_w, row as f32 * cell_h);

        let mut bg_color = cell
            .and_then(|c| c.bg)
            .map_or(config.theme.background, |c| {
                [
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    1.0,
                ]
            });
        let mut fg_color = cell
            .and_then(|c| c.fg)
            .map_or(config.theme.foreground, |c| {
                [
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    1.0,
                ]
            });

        if cell.is_some_and(|c| c.inverse) {
            std::mem::swap(&mut bg_color, &mut fg_color);
        }
        if cell.is_some_and(|c| c.faint) {
            fg_color = [
                (fg_color[0] + bg_color[0]) * 0.5,
                (fg_color[1] + bg_color[1]) * 0.5,
                (fg_color[2] + bg_color[2]) * 0.5,
                fg_color[3],
            ];
        }
        if selection.is_some_and(|s| s.contains_cell(col, row)) {
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

        push_bg_rect(&mut bg_instances, [px_x, px_y, cell_w, cell_h], bg_color, merge_bg);

        if is_cursor {
            append_cursor_decorations(
                &mut decorations,
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
                decorations.push(RectInstance {
                    rect: [px_x, px_y + cell_h - line_h - 1.0, cell_w, line_h],
                    color: fg_color,
                });
            }
            if c.strikethrough {
                decorations.push(RectInstance {
                    rect: [
                        px_x,
                        px_y + (cell_h * 0.5 - line_h * 0.5).round(),
                        cell_w,
                        line_h,
                    ],
                    color: fg_color,
                });
            }
            if c.overline {
                decorations.push(RectInstance {
                    rect: [px_x, px_y, cell_w, line_h],
                    color: fg_color,
                });
            }

            if visible_glyph(c) {
                let is_wide = c.wide == CellWide::Wide;
                let entry = match prepared {
                    Some(prepared) => atlas.insert_prepared(
                        prepared.get(&GlyphKey {
                            text: c.text.clone(),
                            bold: c.bold,
                            italic: c.italic,
                        }),
                        queue,
                    ),
                    None => atlas.get_or_insert(&c.text, c.bold, c.italic, is_wide, config, queue),
                }?;
                if let Some(entry) = entry {
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

    bg_instances.extend(decorations);

    Ok((bg_instances, glyph_instances))
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
                bg_instances.push(RectInstance {
                    rect,
                    color: cursor_color,
                });
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
            Err(error) => {
                eprintln!("FERRYX_GPU_TEST_UNAVAILABLE: {error}");
                return;
            }
        };
        eprintln!("FERRYX_GPU_TEST_READY");
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = test_config();
        let base = CellSnapshot {
            text: "A".into(),
            wide: CellWide::Narrow,
            ..Default::default()
        };
        let (bg_plain, _) = build_row_instances(
            0,
            &single_cell_scenario(base.clone()),
            None,
            &config,
            &mut atlas,
            &gpu.queue,
            None,
        )
        .unwrap();
        let mut under = base;
        under.underline = true;
        let (bg_under, _) = build_row_instances(
            0,
            &single_cell_scenario(under),
            None,
            &config,
            &mut atlas,
            &gpu.queue,
            None,
        )
        .unwrap();
        assert_eq!(
            bg_under.len(),
            bg_plain.len() + 1,
            "underlined cell produces one more RectInstance"
        );
        let deco_y = bg_under.last().unwrap().rect[1];
        let cell_h = config.cell_height_px as f32;
        assert!(
            deco_y >= cell_h * 0.7 && deco_y < cell_h,
            "underline sits in lower part of cell: {deco_y}"
        );
    }

    #[test]
    fn test_cell_strikethrough_produces_rect_in_vertical_middle() {
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(error) => {
                eprintln!("FERRYX_GPU_TEST_UNAVAILABLE: {error}");
                return;
            }
        };
        eprintln!("FERRYX_GPU_TEST_READY");
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = test_config();
        let mut strike = CellSnapshot {
            text: "A".into(),
            wide: CellWide::Narrow,
            ..Default::default()
        };
        strike.strikethrough = true;
        let (bg_strike, _) = build_row_instances(
            0,
            &single_cell_scenario(strike),
            None,
            &config,
            &mut atlas,
            &gpu.queue,
            None,
        )
        .unwrap();
        assert!(bg_strike.len() >= 2, "strikethrough produces rect");
        let deco_y = bg_strike.last().unwrap().rect[1];
        let cell_h = config.cell_height_px as f32;
        let mid_y = cell_h * 0.5;
        assert!(
            (deco_y - mid_y).abs() <= cell_h * 0.2,
            "strikethrough sits near vertical middle: {deco_y}"
        );
    }

    #[test]
    fn test_cell_overline_produces_rect_at_cell_top() {
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(error) => {
                eprintln!("FERRYX_GPU_TEST_UNAVAILABLE: {error}");
                return;
            }
        };
        eprintln!("FERRYX_GPU_TEST_READY");
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = test_config();
        let mut over = CellSnapshot {
            text: "A".into(),
            wide: CellWide::Narrow,
            ..Default::default()
        };
        over.overline = true;
        let (bg_over, _) = build_row_instances(
            0,
            &single_cell_scenario(over),
            None,
            &config,
            &mut atlas,
            &gpu.queue,
            None,
        )
        .unwrap();
        assert!(bg_over.len() >= 2, "overline produces rect");
        let deco_y = bg_over.last().unwrap().rect[1];
        let cell_h = config.cell_height_px as f32;
        assert!(
            deco_y >= 0.0 && deco_y <= cell_h * 0.15,
            "overline sits at cell top: {deco_y}"
        );
    }

    #[test]
    fn test_cell_invisible_produces_zero_glyph_instances() {
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(error) => {
                eprintln!("FERRYX_GPU_TEST_UNAVAILABLE: {error}");
                return;
            }
        };
        eprintln!("FERRYX_GPU_TEST_READY");
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = test_config();
        let mut invis = CellSnapshot {
            text: "A".into(),
            wide: CellWide::Narrow,
            ..Default::default()
        };
        invis.invisible = true;
        let (bg_invis, glyph_invis) = build_row_instances(
            0,
            &single_cell_scenario(invis),
            None,
            &config,
            &mut atlas,
            &gpu.queue,
            None,
        )
        .unwrap();
        assert_eq!(bg_invis.len(), 1, "invisible cell keeps background rect");
        assert_eq!(
            glyph_invis.len(),
            0,
            "invisible cell produces zero GlyphInstances"
        );
    }

    #[test]
    fn test_cell_faint_glyph_color_differs_from_non_faint() {
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(error) => {
                eprintln!("FERRYX_GPU_TEST_UNAVAILABLE: {error}");
                return;
            }
        };
        eprintln!("FERRYX_GPU_TEST_READY");
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = test_config();
        let normal = CellSnapshot {
            text: "A".into(),
            wide: CellWide::Narrow,
            fg: Some(ColorRgb {
                r: 200,
                g: 200,
                b: 200,
            }),
            bg: Some(ColorRgb { r: 0, g: 0, b: 0 }),
            ..Default::default()
        };
        let (_, glyph_normal) = build_row_instances(
            0,
            &single_cell_scenario(normal.clone()),
            None,
            &config,
            &mut atlas,
            &gpu.queue,
            None,
        )
        .unwrap();
        let mut faint = normal;
        faint.faint = true;
        let (_, glyph_faint) = build_row_instances(
            0,
            &single_cell_scenario(faint),
            None,
            &config,
            &mut atlas,
            &gpu.queue,
            None,
        )
        .unwrap();
        assert_eq!(glyph_normal.len(), 1);
        assert_eq!(glyph_faint.len(), 1);
        assert_ne!(
            glyph_faint[0].color, glyph_normal[0].color,
            "faint glyph color differs"
        );
    }

    #[test]
    fn test_row_hash_invalidates_on_attribute_changes() {
        let config = RendererConfig::default();
        let base = CellSnapshot {
            text: "A".into(),
            wide: CellWide::Narrow,
            ..Default::default()
        };
        let h_base = compute_row_hash(0, &single_cell_scenario(base.clone()), None, &config);

        let mut bold = base.clone();
        bold.bold = true;
        assert_ne!(
            h_base,
            compute_row_hash(0, &single_cell_scenario(bold), None, &config)
        );

        let mut italic = base.clone();
        italic.italic = true;
        assert_ne!(
            h_base,
            compute_row_hash(0, &single_cell_scenario(italic), None, &config)
        );

        let mut underline = base.clone();
        underline.underline = true;
        assert_ne!(
            h_base,
            compute_row_hash(0, &single_cell_scenario(underline), None, &config)
        );

        let mut inverse = base.clone();
        inverse.inverse = true;
        assert_ne!(
            h_base,
            compute_row_hash(0, &single_cell_scenario(inverse), None, &config)
        );

        let mut faint = base.clone();
        faint.faint = true;
        assert_ne!(
            h_base,
            compute_row_hash(0, &single_cell_scenario(faint), None, &config)
        );

        let mut blink = base.clone();
        blink.blink = true;
        assert_ne!(
            h_base,
            compute_row_hash(0, &single_cell_scenario(blink), None, &config)
        );

        let mut invis = base.clone();
        invis.invisible = true;
        assert_ne!(
            h_base,
            compute_row_hash(0, &single_cell_scenario(invis), None, &config)
        );

        let mut strike = base.clone();
        strike.strikethrough = true;
        assert_ne!(
            h_base,
            compute_row_hash(0, &single_cell_scenario(strike), None, &config)
        );

        let mut over = base;
        over.overline = true;
        assert_ne!(
            h_base,
            compute_row_hash(0, &single_cell_scenario(over), None, &config)
        );
    }

    #[test]
    fn test_fnv1a_hasher_matches_reference_vectors() {
        let mut h_empty = Fnv1aHasher::new();
        h_empty.write(b"");
        assert_eq!(h_empty.finish(), 0xcbf2_9ce4_8422_2325);

        let mut h_a = Fnv1aHasher::new();
        h_a.write(b"a");
        assert_eq!(h_a.finish(), 0xaf63_dc4c_8601_ec8c);

        let mut h_foobar = Fnv1aHasher::new();
        h_foobar.write(b"foobar");
        assert_eq!(h_foobar.finish(), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn test_bg_run_length_merging_produces_pixel_identical_geometry() {
        let c_default = [0.1, 0.1, 0.1, 1.0];
        let c_red = [1.0, 0.0, 0.0, 1.0];
        let c_blue = [0.0, 0.0, 1.0, 1.0];
        let colors = [
            c_default, c_default, c_default, c_red, c_red, c_blue, c_default, c_default,
            c_default, c_default,
        ];
        let (cell_w, cell_h, row_y) = (10.0f32, 20.0f32, 40.0f32);

        let mut merged = Vec::new();
        let mut unmerged = Vec::new();
        for (col, &color) in colors.iter().enumerate() {
            let rect = [col as f32 * cell_w, row_y, cell_w, cell_h];
            push_bg_rect(&mut merged, rect, color, true);
            push_bg_rect(&mut unmerged, rect, color, false);
        }

        assert_eq!(unmerged.len(), 10);
        assert_eq!(merged.len(), 4);
        assert_eq!(merged[0].rect, [0.0, 40.0, 30.0, 20.0]);
        assert_eq!(merged[0].color, c_default);
        assert_eq!(merged[1].rect, [30.0, 40.0, 20.0, 20.0]);
        assert_eq!(merged[1].color, c_red);
        assert_eq!(merged[2].rect, [50.0, 40.0, 10.0, 20.0]);
        assert_eq!(merged[2].color, c_blue);
        assert_eq!(merged[3].rect, [60.0, 40.0, 40.0, 20.0]);
        assert_eq!(merged[3].color, c_default);

        // Verify every pixel coordinate in the 100x20 row resolves identically.
        let sample_color = |rects: &[RectInstance], x: f32, y: f32| -> Option<[f32; 4]> {
            rects.iter().rev().find_map(|r| {
                let in_x = x >= r.rect[0] && x < r.rect[0] + r.rect[2];
                let in_y = y >= r.rect[1] && y < r.rect[1] + r.rect[3];
                (in_x && in_y).then_some(r.color)
            })
        };
        for px in 0..100 {
            let x = px as f32 + 0.5;
            for py in [row_y + 0.5, row_y + 10.0, row_y + 19.5] {
                assert_eq!(
                    sample_color(&merged, x, py),
                    sample_color(&unmerged, x, py),
                    "mismatch at ({x}, {py})"
                );
            }
        }
    }

    #[test]
    fn test_build_row_instances_merges_contiguous_bg_and_preserves_decorations() {
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(error) => {
                eprintln!("FERRYX_GPU_TEST_UNAVAILABLE: {error}");
                return;
            }
        };
        eprintln!("FERRYX_GPU_TEST_READY");
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = test_config();

        let red = ColorRgb { r: 255, g: 0, b: 0 };
        let mut row = vec![CellSnapshot::default(); 8];
        row[2].bg = Some(red);
        row[3].bg = Some(red);
        row[3].underline = true;
        row[4].text = "A".into();
        let snapshot = RenderSnapshot {
            cols: 8,
            rows: 1,
            cursor: crate::native_terminal::cursor::CursorSnapshot {
                x: 0,
                y: 0,
                visible: true,
                blinking: false,
                wide_tail: false,
                visual_style: CursorVisualStyle::Underline,
            },
            grid: vec![row],
            images: Vec::new(),
        };

        let (bg, glyphs) =
            build_row_instances(0, &snapshot, None, &config, &mut atlas, &gpu.queue, None).unwrap();
        // 3 merged bg spans: [0..2) default, [2..4) red, [4..8) default, followed by
        // 2 trailing decorations: cursor underline at col 0 + cell underline at col 3.
        assert_eq!(bg.len(), 3 + 2);
        assert_eq!(bg[0].rect, [0.0, 0.0, 20.0, 20.0]);
        assert_eq!(bg[1].rect, [20.0, 0.0, 20.0, 20.0]);
        assert_eq!(bg[1].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(bg[2].rect, [40.0, 0.0, 40.0, 20.0]);
        assert_eq!(bg[3].rect, [0.0, 18.0, 10.0, 2.0]);
        assert_eq!(bg[4].rect[0], 30.0);
        assert_eq!(bg[4].rect[2], 10.0);
        assert_eq!(glyphs.len(), 1);
    }
}
