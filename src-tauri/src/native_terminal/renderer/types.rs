//! Core types and statistics for native terminal GPU rendering.

use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::native_terminal::cursor::CursorVisualStyle;
use crate::native_terminal::error::NativeTerminalError;
use crate::terminal::preferences::{TerminalPreferences, TerminalThemeColors};

pub const DEFAULT_RENDERER_BACKGROUND: [f32; 4] = [0.07, 0.07, 0.09, 1.0];
pub const DEFAULT_RENDERER_FOREGROUND: [f32; 4] = [0.90, 0.90, 0.90, 1.0];
pub const DEFAULT_RENDERER_CURSOR: [f32; 4] = [0.85, 0.85, 0.85, 1.0];
pub const DEFAULT_RENDERER_CURSOR_ACCENT: [f32; 4] = [0.05, 0.05, 0.05, 1.0];
pub const DEFAULT_RENDERER_SELECTION_BG: [f32; 4] = [0.22, 0.44, 0.77, 1.0];
pub const DEFAULT_RENDERER_SELECTION_FG: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// Parses a 3-, 4-, 6-, or 8-digit hexadecimal color string into normalized RGBA `[r, g, b, a]`.
/// Falls back to `default_color` if unparsable.
pub fn parse_hex_color(s: &str, default_color: [f32; 4]) -> [f32; 4] {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16);
        let g = u8::from_str_radix(&hex[2..4], 16);
        let b = u8::from_str_radix(&hex[4..6], 16);
        match (r, g, b) {
            (Ok(r), Ok(g), Ok(b)) => [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0],
            _ => default_color,
        }
    } else if hex.len() == 8 {
        let r = u8::from_str_radix(&hex[0..2], 16);
        let g = u8::from_str_radix(&hex[2..4], 16);
        let b = u8::from_str_radix(&hex[4..6], 16);
        let a = u8::from_str_radix(&hex[6..8], 16);
        match (r, g, b, a) {
            (Ok(r), Ok(g), Ok(b), Ok(a)) => [
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
                a as f32 / 255.0,
            ],
            _ => default_color,
        }
    } else if hex.len() == 3 {
        let r = u8::from_str_radix(&hex[0..1], 16);
        let g = u8::from_str_radix(&hex[1..2], 16);
        let b = u8::from_str_radix(&hex[2..3], 16);
        match (r, g, b) {
            (Ok(r), Ok(g), Ok(b)) => [
                (r * 17) as f32 / 255.0,
                (g * 17) as f32 / 255.0,
                (b * 17) as f32 / 255.0,
                1.0,
            ],
            _ => default_color,
        }
    } else if hex.len() == 4 {
        let r = u8::from_str_radix(&hex[0..1], 16);
        let g = u8::from_str_radix(&hex[1..2], 16);
        let b = u8::from_str_radix(&hex[2..3], 16);
        let a = u8::from_str_radix(&hex[3..4], 16);
        match (r, g, b, a) {
            (Ok(r), Ok(g), Ok(b), Ok(a)) => [
                (r * 17) as f32 / 255.0,
                (g * 17) as f32 / 255.0,
                (b * 17) as f32 / 255.0,
                (a * 17) as f32 / 255.0,
            ],
            _ => default_color,
        }
    } else {
        default_color
    }
}

pub fn parse_cursor_style(style: &str) -> CursorVisualStyle {
    match style.trim().to_lowercase().as_str() {
        "bar" => CursorVisualStyle::Bar,
        "underline" => CursorVisualStyle::Underline,
        "block_hollow" | "blockhollow" | "hollow" => CursorVisualStyle::BlockHollow,
        _ => CursorVisualStyle::Block,
    }
}

/// Color and visual styling configuration for terminal background, text, cursor, and selection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RendererTheme {
    /// Default background color as normalized RGBA [0.0..1.0].
    pub background: [f32; 4],
    /// Default foreground (text) color as normalized RGBA [0.0..1.0].
    pub foreground: [f32; 4],
    /// Cursor color as normalized RGBA [0.0..1.0].
    pub cursor: [f32; 4],
    /// Cursor accent/text color for block cursor as normalized RGBA [0.0..1.0].
    pub cursor_accent: [f32; 4],
    /// Selection background color as normalized RGBA [0.0..1.0].
    pub selection_background: [f32; 4],
    /// Selection text color as normalized RGBA [0.0..1.0].
    pub selection_foreground: [f32; 4],
    /// Configured cursor visual style (block, bar, underline, hollow).
    pub cursor_style: CursorVisualStyle,
}

impl RendererTheme {
    pub fn from_theme_colors(colors: &TerminalThemeColors) -> Self {
        Self::from(colors)
    }
}

impl Default for RendererTheme {
    fn default() -> Self {
        Self {
            background: DEFAULT_RENDERER_BACKGROUND,
            foreground: DEFAULT_RENDERER_FOREGROUND,
            cursor: DEFAULT_RENDERER_CURSOR,
            cursor_accent: DEFAULT_RENDERER_CURSOR_ACCENT,
            selection_background: DEFAULT_RENDERER_SELECTION_BG,
            selection_foreground: DEFAULT_RENDERER_SELECTION_FG,
            cursor_style: CursorVisualStyle::Block,
        }
    }
}

impl Hash for RendererTheme {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for v in self.background {
            v.to_bits().hash(state);
        }
        for v in self.foreground {
            v.to_bits().hash(state);
        }
        for v in self.cursor {
            v.to_bits().hash(state);
        }
        for v in self.cursor_accent {
            v.to_bits().hash(state);
        }
        for v in self.selection_background {
            v.to_bits().hash(state);
        }
        for v in self.selection_foreground {
            v.to_bits().hash(state);
        }
        (self.cursor_style as u8).hash(state);
    }
}

impl From<&TerminalThemeColors> for RendererTheme {
    fn from(colors: &TerminalThemeColors) -> Self {
        let background = parse_hex_color(&colors.background, DEFAULT_RENDERER_BACKGROUND);
        let foreground = parse_hex_color(&colors.foreground, DEFAULT_RENDERER_FOREGROUND);
        let cursor = parse_hex_color(&colors.cursor, DEFAULT_RENDERER_CURSOR);
        let cursor_accent = parse_hex_color(&colors.cursor_accent, DEFAULT_RENDERER_CURSOR_ACCENT);
        let selection_background =
            parse_hex_color(&colors.selection_background, DEFAULT_RENDERER_SELECTION_BG);
        let selection_foreground = match &colors.selection_foreground {
            Some(s) => parse_hex_color(s, DEFAULT_RENDERER_SELECTION_FG),
            None => DEFAULT_RENDERER_SELECTION_FG,
        };

        Self {
            background,
            foreground,
            cursor,
            cursor_accent,
            selection_background,
            selection_foreground,
            cursor_style: CursorVisualStyle::Block,
        }
    }
}

impl From<TerminalThemeColors> for RendererTheme {
    fn from(colors: TerminalThemeColors) -> Self {
        RendererTheme::from(&colors)
    }
}

impl From<&TerminalPreferences> for RendererTheme {
    fn from(prefs: &TerminalPreferences) -> Self {
        let mut theme = RendererTheme::from(&prefs.theme);
        theme.cursor_style = parse_cursor_style(&prefs.cursor_style);
        theme
    }
}

impl From<TerminalPreferences> for RendererTheme {
    fn from(prefs: TerminalPreferences) -> Self {
        RendererTheme::from(&prefs)
    }
}

/// Configuration parameters for native terminal renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RendererConfig {
    /// Width of a single character cell in physical pixels.
    pub cell_width_px: u32,
    /// Height of a single character cell in physical pixels.
    pub cell_height_px: u32,
    /// HiDPI / Retina scale factor.
    pub device_scale_factor: f32,
    /// Color theme and cursor visual styling.
    pub theme: RendererTheme,
}

impl Hash for RendererConfig {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.cell_width_px.hash(state);
        self.cell_height_px.hash(state);
        self.device_scale_factor.to_bits().hash(state);
        self.theme.hash(state);
    }
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            cell_width_px: 10,
            cell_height_px: 20,
            device_scale_factor: 1.0,
            theme: RendererTheme::default(),
        }
    }
}

impl RendererConfig {
    /// Validates renderer configuration rejecting zero or overflow dimensions.
    pub fn validate(&self) -> Result<(), NativeTerminalError> {
        if self.cell_width_px == 0 || self.cell_height_px == 0 {
            return Err(NativeTerminalError::InvalidDimensions(
                self.cell_width_px as u16,
                self.cell_height_px as u16,
            ));
        }
        if self.cell_width_px > 4096 || self.cell_height_px > 4096 {
            return Err(NativeTerminalError::InvalidDimensions(
                self.cell_width_px as u16,
                self.cell_height_px as u16,
            ));
        }
        Ok(())
    }
}

/// Selection boundary snapshot for terminal grid highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionSnapshot {
    pub start_col: u16,
    pub start_row: u16,
    pub end_col: u16,
    pub end_row: u16,
}

impl SelectionSnapshot {
    /// Returns true if the given cell (col, row) is within the selection range.
    pub fn contains_cell(&self, col: u16, row: u16) -> bool {
        let (min_r, max_r, min_c, max_c) = if self.start_row < self.end_row
            || (self.start_row == self.end_row && self.start_col <= self.end_col)
        {
            (self.start_row, self.end_row, self.start_col, self.end_col)
        } else {
            (self.end_row, self.start_row, self.end_col, self.start_col)
        };

        if row < min_r || row > max_r {
            return false;
        }
        if min_r == max_r {
            col >= min_c && col <= max_c
        } else if row == min_r {
            col >= min_c
        } else if row == max_r {
            col <= max_c
        } else {
            true
        }
    }
}

/// Bounded glyph atlas memory and entry cache statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GlyphAtlasStats {
    /// Number of distinct glyph entries currently cached in the atlas.
    pub entry_count: usize,
    /// Allocated byte size of the glyph atlas texture and metadata.
    pub allocated_bytes: usize,
    /// Configured maximum capacity in bytes before bounded eviction occurs.
    pub max_capacity_bytes: usize,
}

/// Resulting pixel buffer of an offscreen terminal render pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffscreenFrame {
    /// Width of rendered frame in pixels.
    pub width_px: u32,
    /// Height of rendered frame in pixels.
    pub height_px: u32,
    /// Packed unpadded RGBA8 pixel bytes.
    pub pixels: Vec<u8>,
    /// Number of terminal grid rows rendered.
    pub rendered_row_count: u16,
    /// Number of dirty rows rebuilt during this frame.
    pub rebuilt_row_count: u16,
    /// Number of cached rows reused during this frame.
    pub reused_row_count: u16,
}

impl OffscreenFrame {
    /// Saves the RGBA offscreen pixel buffer as a standard PNG image file.
    pub fn save_png<P: AsRef<Path>>(&self, path: P) -> Result<(), NativeTerminalError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| NativeTerminalError::IoError(e.to_string()))?;
            }
        }
        let file =
            std::fs::File::create(path).map_err(|e| NativeTerminalError::IoError(e.to_string()))?;
        let w = std::io::BufWriter::new(file);

        let mut encoder = png::Encoder::new(w, self.width_px, self.height_px);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        let mut writer = encoder
            .write_header()
            .map_err(|e| NativeTerminalError::IoError(e.to_string()))?;
        writer
            .write_image_data(&self.pixels)
            .map_err(|e| NativeTerminalError::IoError(e.to_string()))?;
        writer
            .finish()
            .map_err(|e| NativeTerminalError::IoError(e.to_string()))?;

        Ok(())
    }
}
