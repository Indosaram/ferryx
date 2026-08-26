//! Glyph rasterizer producing alpha masks for ASCII, Unicode, CJK, and emoji using FontManager.

use super::font_manager::FontManager;

/// Rasterizes a text grapheme/string into an 8-bit alpha mask buffer (width x height) at 1.0x scale.
#[allow(dead_code)]
pub fn rasterize_glyph(text: &str, width: u32, height: u32, bold: bool, italic: bool) -> Vec<u8> {
    FontManager::global().rasterize_glyph(text, width, height, bold, italic)
}

/// Rasterizes a text grapheme/string into an 8-bit alpha mask buffer (width x height) with explicit scale factor.
pub fn rasterize_glyph_with_scale(
    text: &str,
    width: u32,
    height: u32,
    bold: bool,
    italic: bool,
    scale_factor: f32,
) -> Vec<u8> {
    FontManager::global().rasterize_glyph_for_scale(text, width, height, bold, italic, scale_factor)
}
