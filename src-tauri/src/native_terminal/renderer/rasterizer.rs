//! Glyph rasterizer producing alpha masks or RGBA color buffers for ASCII, Unicode, CJK, and emoji using FontManager.

use super::font_manager::FontManager;

/// Result of rasterizing a glyph cluster: either an 8-bit alpha mask or a 32-bit RGBA color buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RasterizedGlyph {
    Alpha(Vec<u8>),
    Color(Vec<u8>),
}

impl RasterizedGlyph {
    pub fn is_color(&self) -> bool {
        matches!(self, Self::Color(_))
    }

    pub fn buffer(&self) -> &[u8] {
        match self {
            Self::Alpha(b) | Self::Color(b) => b,
        }
    }

    pub fn into_buffer(self) -> Vec<u8> {
        match self {
            Self::Alpha(b) | Self::Color(b) => b,
        }
    }
}

/// Rasterizes a text grapheme/string into an 8-bit alpha mask or RGBA color buffer (width x height) at 1.0x scale.
#[allow(dead_code)]
pub fn rasterize_glyph(
    text: &str,
    width: u32,
    height: u32,
    bold: bool,
    italic: bool,
) -> RasterizedGlyph {
    FontManager::global().rasterize_glyph(text, width, height, bold, italic)
}

/// Rasterizes a text grapheme/string into an 8-bit alpha mask or RGBA color buffer (width x height) with explicit scale factor.
pub fn rasterize_glyph_with_scale(
    text: &str,
    width: u32,
    height: u32,
    bold: bool,
    italic: bool,
    scale_factor: f32,
) -> RasterizedGlyph {
    FontManager::global().rasterize_glyph_for_scale(text, width, height, bold, italic, scale_factor)
}
