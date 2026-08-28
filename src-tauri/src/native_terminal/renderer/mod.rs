//! Native Terminal GPU Renderer (Phase 2).

pub mod atlas;
pub mod color_glyph;
pub mod coretext_font;
pub mod coretext_raster;
#[cfg(target_os = "windows")]
pub mod directwrite_raster;
pub mod font_manager;
#[cfg(target_os = "linux")]
pub mod freetype_raster;
pub mod gpu_context;
pub mod instances;
pub mod pass;
pub mod pipeline;
pub mod rasterizer;
pub mod render_target;
pub mod renderer;
pub mod row_cache;
pub mod scenario;
pub mod shaders;
pub mod types;

pub use font_manager::FontManager;
pub use instances::build_row_instances;
pub use pipeline::{GlyphInstance, RectInstance};
pub use rasterizer::{rasterize_glyph, rasterize_glyph_with_scale, RasterizedGlyph};
pub use renderer::NativeTerminalRenderer;
pub use scenario::canonical_scenario;
pub use types::{
    parse_cursor_style, parse_hex_color, GlyphAtlasStats, OffscreenFrame, RendererConfig,
    RendererTheme, SelectionSnapshot, DEFAULT_RENDERER_BACKGROUND, DEFAULT_RENDERER_CURSOR,
    DEFAULT_RENDERER_CURSOR_ACCENT, DEFAULT_RENDERER_FOREGROUND, DEFAULT_RENDERER_SELECTION_BG,
    DEFAULT_RENDERER_SELECTION_FG,
};
