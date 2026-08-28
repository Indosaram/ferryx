//! Every desktop platform must rasterize real glyph coverage; an all-zero alpha mask
//! renders an invisible terminal even when the surface presents successfully.

#![cfg(feature = "native-terminal")]

use ferryx_lib::native_terminal::renderer::font_manager::FontManager;

fn coverage(text: &str) -> (usize, usize) {
    let manager = FontManager::new_with_family_and_size("Menlo", 14.0);
    let metrics = manager.cell_metrics();
    let glyph = manager.rasterize_glyph_for_scale(
        text,
        metrics.width_px,
        metrics.height_px,
        false,
        false,
        1.0,
    );
    let buffer = glyph.buffer();
    (buffer.iter().filter(|&&v| v > 0).count(), buffer.len())
}

#[test]
fn ascii_glyphs_rasterize_visible_coverage() {
    for text in ["F", "E", "R", "Y", "X", "0", "_"] {
        let (nonzero, len) = coverage(text);
        assert!(len > 0, "glyph buffer for {text:?} must not be empty");
        assert!(
            nonzero > 0,
            "glyph {text:?} rasterized to an all-zero mask ({len} bytes); the terminal would render blank"
        );
    }
}

#[test]
fn whitespace_rasterizes_without_coverage() {
    let (nonzero, len) = coverage(" ");
    assert!(len > 0, "space must still allocate a cell-sized buffer");
    assert_eq!(nonzero, 0, "space must not paint any pixels");
}

#[test]
fn distinct_characters_produce_distinct_masks() {
    let manager = FontManager::new_with_family_and_size("Menlo", 14.0);
    let metrics = manager.cell_metrics();
    let raster = |text: &str| {
        manager
            .rasterize_glyph_for_scale(text, metrics.width_px, metrics.height_px, false, false, 1.0)
            .buffer()
            .to_vec()
    };

    assert_ne!(
        raster("W"),
        raster("."),
        "different characters must produce different coverage masks"
    );
}
