//! Kitty unicode placeholder (virtual placement) resolution.
//!
//! libghostty-vt stores a unicode-placeholder placement as virtual: the screen
//! position lives in the cells the client wrote, so the placement API reports it
//! invisible. Only a renderer scanning the grid can locate it, which is what this
//! module does, turning placeholder runs into placements the image renderer
//! already draws. The decode mirrors the vendored Ghostty implementation
//! (`src/terminal/kitty/graphics_unicode.zig`): the image id is the cell's raw
//! foreground color identifier, the placement id its underline color identifier,
//! the row/column are diacritics that may be inherited from the cell to the left,
//! and a run may only continue over compatible cells.

use super::images::{ImagePlacementSnapshot, VirtualPlacement};
use super::snapshot::CellSnapshot;
use std::sync::Arc;

const PLACEHOLDER: char = '\u{10EEEE}';

/// Row/column diacritics from the Kitty graphics protocol rowcolumn-diacritics
/// table. The index into the table is the encoded value.
#[rustfmt::skip]
const DIACRITICS: [u32; 297] = [
    0x0305, 0x030D, 0x030E, 0x0310, 0x0312, 0x033D,
    0x033E, 0x033F, 0x0346, 0x034A, 0x034B, 0x034C,
    0x0350, 0x0351, 0x0352, 0x0357, 0x035B, 0x0363,
    0x0364, 0x0365, 0x0366, 0x0367, 0x0368, 0x0369,
    0x036A, 0x036B, 0x036C, 0x036D, 0x036E, 0x036F,
    0x0483, 0x0484, 0x0485, 0x0486, 0x0487, 0x0592,
    0x0593, 0x0594, 0x0595, 0x0597, 0x0598, 0x0599,
    0x059C, 0x059D, 0x059E, 0x059F, 0x05A0, 0x05A1,
    0x05A8, 0x05A9, 0x05AB, 0x05AC, 0x05AF, 0x05C4,
    0x0610, 0x0611, 0x0612, 0x0613, 0x0614, 0x0615,
    0x0616, 0x0617, 0x0657, 0x0658, 0x0659, 0x065A,
    0x065B, 0x065D, 0x065E, 0x06D6, 0x06D7, 0x06D8,
    0x06D9, 0x06DA, 0x06DB, 0x06DC, 0x06DF, 0x06E0,
    0x06E1, 0x06E2, 0x06E4, 0x06E7, 0x06E8, 0x06EB,
    0x06EC, 0x0730, 0x0732, 0x0733, 0x0735, 0x0736,
    0x073A, 0x073D, 0x073F, 0x0740, 0x0741, 0x0743,
    0x0745, 0x0747, 0x0749, 0x074A, 0x07EB, 0x07EC,
    0x07ED, 0x07EE, 0x07EF, 0x07F0, 0x07F1, 0x07F3,
    0x0816, 0x0817, 0x0818, 0x0819, 0x081B, 0x081C,
    0x081D, 0x081E, 0x081F, 0x0820, 0x0821, 0x0822,
    0x0823, 0x0825, 0x0826, 0x0827, 0x0829, 0x082A,
    0x082B, 0x082C, 0x082D, 0x0951, 0x0953, 0x0954,
    0x0F82, 0x0F83, 0x0F86, 0x0F87, 0x135D, 0x135E,
    0x135F, 0x17DD, 0x193A, 0x1A17, 0x1A75, 0x1A76,
    0x1A77, 0x1A78, 0x1A79, 0x1A7A, 0x1A7B, 0x1A7C,
    0x1B6B, 0x1B6D, 0x1B6E, 0x1B6F, 0x1B70, 0x1B71,
    0x1B72, 0x1B73, 0x1CD0, 0x1CD1, 0x1CD2, 0x1CDA,
    0x1CDB, 0x1CE0, 0x1DC0, 0x1DC1, 0x1DC3, 0x1DC4,
    0x1DC5, 0x1DC6, 0x1DC7, 0x1DC8, 0x1DC9, 0x1DCB,
    0x1DCC, 0x1DD1, 0x1DD2, 0x1DD3, 0x1DD4, 0x1DD5,
    0x1DD6, 0x1DD7, 0x1DD8, 0x1DD9, 0x1DDA, 0x1DDB,
    0x1DDC, 0x1DDD, 0x1DDE, 0x1DDF, 0x1DE0, 0x1DE1,
    0x1DE2, 0x1DE3, 0x1DE4, 0x1DE5, 0x1DE6, 0x1DFE,
    0x20D0, 0x20D1, 0x20D4, 0x20D5, 0x20D6, 0x20D7,
    0x20DB, 0x20DC, 0x20E1, 0x20E7, 0x20E9, 0x20F0,
    0x2CEF, 0x2CF0, 0x2CF1, 0x2DE0, 0x2DE1, 0x2DE2,
    0x2DE3, 0x2DE4, 0x2DE5, 0x2DE6, 0x2DE7, 0x2DE8,
    0x2DE9, 0x2DEA, 0x2DEB, 0x2DEC, 0x2DED, 0x2DEE,
    0x2DEF, 0x2DF0, 0x2DF1, 0x2DF2, 0x2DF3, 0x2DF4,
    0x2DF5, 0x2DF6, 0x2DF7, 0x2DF8, 0x2DF9, 0x2DFA,
    0x2DFB, 0x2DFC, 0x2DFD, 0x2DFE, 0x2DFF, 0xA66F,
    0xA67C, 0xA67D, 0xA6F0, 0xA6F1, 0xA8E0, 0xA8E1,
    0xA8E2, 0xA8E3, 0xA8E4, 0xA8E5, 0xA8E6, 0xA8E7,
    0xA8E8, 0xA8E9, 0xA8EA, 0xA8EB, 0xA8EC, 0xA8ED,
    0xA8EE, 0xA8EF, 0xA8F0, 0xA8F1, 0xAAB0, 0xAAB2,
    0xAAB3, 0xAAB7, 0xAAB8, 0xAABE, 0xAABF, 0xAAC1,
    0xFE20, 0xFE21, 0xFE22, 0xFE23, 0xFE24, 0xFE25,
    0xFE26, 0x10A0F, 0x10A38, 0x1D185, 0x1D186, 0x1D187,
    0x1D188, 0x1D189, 0x1D1AA, 0x1D1AB, 0x1D1AC, 0x1D1AD,
    0x1D242, 0x1D243, 0x1D244,
];

fn diacritic_index(character: char) -> Option<u32> {
    DIACRITICS
        .iter()
        .position(|candidate| *candidate == character as u32)
        .map(|index| index as u32)
}

#[derive(Clone, Copy)]
struct DecodedCell {
    image_id_low: u32,
    image_id_high: Option<u8>,
    placement_id: Option<u32>,
    row: Option<u32>,
    column: Option<u32>,
}

fn decode_cell(cell: &CellSnapshot) -> Option<DecodedCell> {
    let mut characters = cell.text.chars();
    if characters.next()? != PLACEHOLDER {
        return None;
    }
    let image_id_low = cell.fg_ident?;
    let placement_id = cell.underline_ident.filter(|identifier| *identifier != 0);
    let diacritics: Vec<char> = characters.collect();
    // An invalid diacritic is treated as absent, like the reference decoder.
    let row = diacritics.first().and_then(|c| diacritic_index(*c));
    let column = diacritics.get(1).and_then(|c| diacritic_index(*c));
    let image_id_high = diacritics
        .get(2)
        .and_then(|c| diacritic_index(*c))
        .and_then(|value| u8::try_from(value).ok());
    Some(DecodedCell {
        image_id_low,
        image_id_high,
        placement_id,
        row,
        column,
    })
}

#[derive(Clone, Copy)]
struct Run {
    image_id_low: u32,
    image_id_high: Option<u8>,
    placement_id: Option<u32>,
    row: u32,
    source_start: u32,
    width: u32,
    cell_start: u32,
    cell_end: u32,
}

impl Run {
    fn start(cell: &DecodedCell, cell_column: u32) -> Self {
        Self {
            image_id_low: cell.image_id_low,
            image_id_high: cell.image_id_high,
            placement_id: cell.placement_id,
            row: cell.row.unwrap_or(0),
            source_start: cell.column.unwrap_or(0),
            width: 1,
            cell_start: cell_column,
            cell_end: cell_column,
        }
    }

    fn can_append(&self, cell: &DecodedCell, cell_column: u32) -> bool {
        cell_column == self.cell_end + 1
            && cell.image_id_low == self.image_id_low
            && cell.placement_id == self.placement_id
            && (cell.row.is_none() || cell.row == Some(self.row))
            && (cell.column.is_none() || cell.column == Some(self.source_start + self.width))
            && (cell.image_id_high.is_none() || cell.image_id_high == self.image_id_high)
    }

    fn append(&mut self, cell_column: u32) {
        self.width += 1;
        self.cell_end = cell_column;
    }

    fn image_id(&self) -> u32 {
        self.image_id_low | ((self.image_id_high.unwrap_or(0) as u32) << 24)
    }
}

/// Turn placeholder runs in the grid into drawable placements for the images
/// their foreground color and diacritics identify.
pub fn resolve_placements(
    grid: &[Vec<CellSnapshot>],
    virtuals: &[VirtualPlacement],
    cell_width_px: u32,
    cell_height_px: u32,
) -> Vec<ImagePlacementSnapshot> {
    if virtuals.is_empty() {
        return Vec::new();
    }
    let mut placements = Vec::new();
    for (row_index, cells) in grid.iter().enumerate() {
        let mut run: Option<Run> = None;
        for (column_index, cell) in cells.iter().enumerate() {
            let column_index = column_index as u32;
            let decoded = decode_cell(cell);
            let continues = match (decoded, run) {
                (Some(decoded), Some(run)) if run.can_append(&decoded, column_index) => true,
                _ => false,
            };
            match decoded {
                Some(decoded) => {
                    if continues {
                        if let Some(run) = run.as_mut() {
                            run.append(column_index);
                        }
                    } else {
                        push_run(
                            &mut placements,
                            run.take(),
                            row_index,
                            virtuals,
                            cell_width_px,
                            cell_height_px,
                        );
                        run = Some(Run::start(&decoded, column_index));
                    }
                }
                None => {
                    push_run(
                        &mut placements,
                        run.take(),
                        row_index,
                        virtuals,
                        cell_width_px,
                        cell_height_px,
                    );
                }
            }
        }
        push_run(
            &mut placements,
            run.take(),
            row_index,
            virtuals,
            cell_width_px,
            cell_height_px,
        );
    }
    placements
}

fn push_run(
    placements: &mut Vec<ImagePlacementSnapshot>,
    run: Option<Run>,
    viewport_row: usize,
    virtuals: &[VirtualPlacement],
    cell_width_px: u32,
    cell_height_px: u32,
) {
    let Some(run) = run else { return };
    let image_id = run.image_id();
    let Some(virtual_placement) = select_placement(virtuals, image_id, run.placement_id) else {
        return;
    };
    if run.source_start >= virtual_placement.grid_cols || run.row >= virtual_placement.grid_rows {
        return;
    }
    // Cells addressing beyond the placement grid are not part of the image; the
    // prefix that is inside stays drawable.
    let width = run
        .width
        .min(virtual_placement.grid_cols - run.source_start);
    let Some(tile) = tile_geometry(
        &run,
        width,
        viewport_row as u32,
        virtual_placement,
        cell_width_px.max(1),
        cell_height_px.max(1),
    ) else {
        return;
    };
    placements.push(ImagePlacementSnapshot {
        image: Arc::clone(&virtual_placement.image),
        placement_id: virtual_placement.placement_id,
        z: virtual_placement.z,
        viewport_col: tile.cell_column as i32,
        viewport_row: viewport_row as i32,
        offset_x: 0,
        offset_y: 0,
        pixel_width: tile.source[2],
        pixel_height: tile.source[3],
        source: tile.source,
        dest_px: Some(tile.dest),
    });
}

fn select_placement<'a>(
    virtuals: &'a [VirtualPlacement],
    image_id: u32,
    placement_id: Option<u32>,
) -> Option<&'a VirtualPlacement> {
    let matches_image = |candidate: &&VirtualPlacement| candidate.image.id == image_id;
    if let Some(placement_id) = placement_id {
        if let Some(explicit) = virtuals
            .iter()
            .find(|candidate| matches_image(&candidate) && candidate.placement_id == placement_id)
        {
            return Some(explicit);
        }
    }
    virtuals.iter().find(matches_image)
}

struct TileGeometry {
    cell_column: u32,
    source: [u32; 4],
    dest: [u32; 4],
}

fn tile_geometry(
    run: &Run,
    width: u32,
    viewport_row: u32,
    virtual_placement: &VirtualPlacement,
    cell_width_px: u32,
    cell_height_px: u32,
) -> Option<TileGeometry> {
    let image = &virtual_placement.image;
    let grid_width_px = virtual_placement.grid_cols as f64 * cell_width_px as f64;
    let grid_height_px = virtual_placement.grid_rows as f64 * cell_height_px as f64;
    let image_width = image.width.max(1) as f64;
    let image_height = image.height.max(1) as f64;
    // Fit the image into the placement grid preserving its aspect ratio and
    // centre the remainder, then place the tile inside that fitted rectangle.
    let scale = if image_width * grid_height_px > image_height * grid_width_px {
        grid_width_px / image_width
    } else {
        grid_height_px / image_height
    };
    let placements_width_px = image_width * scale;
    let placements_height_px = image_height * scale;
    let offset_x = (grid_width_px - placements_width_px) / 2.0;
    let offset_y = (grid_height_px - placements_height_px) / 2.0;

    // The placement's grid starts at the cell of its first source cell, which the
    // run identifies because source coordinates index the grid while screen
    // coordinates place it. The origin is negative whenever the block's top-left
    // is above or left of the viewport, which is exactly a block that has
    // partially scrolled off.
    let origin_x = (run.cell_start as f64 - run.source_start as f64) * cell_width_px as f64;
    let origin_y = (viewport_row as f64 - run.row as f64) * cell_height_px as f64;
    let grid_x = origin_x + offset_x;
    let grid_y = origin_y + offset_y;

    let tile_x = origin_x + run.source_start as f64 * cell_width_px as f64;
    let tile_y = origin_y + run.row as f64 * cell_height_px as f64;
    let tile_width = width as f64 * cell_width_px as f64;
    let tile_height = cell_height_px as f64;

    let left = tile_x.max(grid_x);
    let top = tile_y.max(grid_y);
    let right = (tile_x + tile_width).min(grid_x + placements_width_px);
    let bottom = (tile_y + tile_height).min(grid_y + placements_height_px);
    if right <= left || bottom <= top {
        return None;
    }

    let source_x = (left - grid_x) / scale;
    let source_y = (top - grid_y) / scale;
    let source_right = (right - grid_x) / scale;
    let source_bottom = (bottom - grid_y) / scale;

    // Integer texture pixels: the destination is derived from the rounded source
    // so the tile is never stretched, and a span shorter than one pixel keeps the
    // nearest pixel instead of disappearing (upscaled images).
    let mut source_left = source_x.floor() as i64;
    let mut source_top = source_y.floor() as i64;
    let mut source_right_px = source_right.ceil() as i64;
    let mut source_bottom_px = source_bottom.ceil() as i64;
    source_left = source_left.clamp(0, image.width.saturating_sub(1) as i64);
    source_top = source_top.clamp(0, image.height.saturating_sub(1) as i64);
    source_right_px = source_right_px.clamp(source_left + 1, image.width as i64);
    source_bottom_px = source_bottom_px.clamp(source_top + 1, image.height as i64);

    let dest_x = grid_x + source_left as f64 * scale;
    let dest_y = grid_y + source_top as f64 * scale;
    let dest_width = (source_right_px - source_left) as f64 * scale;
    let dest_height = (source_bottom_px - source_top) as f64 * scale;

    Some(TileGeometry {
        cell_column: run.cell_start,
        source: [
            source_left as u32,
            source_top as u32,
            (source_right_px - source_left) as u32,
            (source_bottom_px - source_top) as u32,
        ],
        dest: [
            dest_x.round().max(0.0) as u32,
            dest_y.round().max(0.0) as u32,
            dest_width.round().max(1.0) as u32,
            dest_height.round().max(1.0) as u32,
        ],
    })
}
