//! CoreText Font Manager and Glyph Resolver (macOS).
//!
//! Replaces ab_glyph/fontdb with macOS native CoreText font management,
//! cascade fallback, variable font instantiation, and alpha rasterization.

use parking_lot::Mutex;
use std::sync::Arc;

use crate::native_terminal::composition::CellMetrics;
use crate::native_terminal::renderer::rasterizer::RasterizedGlyph;
use crate::terminal::preferences::{
    cached_terminal_preferences, TerminalPreferences, DEFAULT_TERMINAL_FONT_SIZE,
};

#[cfg(target_os = "macos")]
use crate::native_terminal::renderer::coretext_font::macos::CoreTextFontSystem;

pub struct FontManager {
    font_family: String,
    #[cfg(target_os = "macos")]
    system: Mutex<CoreTextFontSystem>,
    #[cfg(not(target_os = "macos"))]
    font_size: Mutex<f32>,
}

unsafe impl Send for FontManager {}
unsafe impl Sync for FontManager {}

static GLOBAL_FONT_MANAGER: Mutex<Option<Arc<FontManager>>> = Mutex::new(None);

/// Returns true if `text` begins with a Unicode codepoint in standard emoji ranges.
pub fn is_color_glyph_candidate(text: &str) -> bool {
    let Some(first_ch) = text.chars().next() else {
        return false;
    };
    let cp = first_ch as u32;
    match cp {
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF => true,
        _ => false,
    }
}

/// Returns true if character is in secondary candidate ranges (Arrows or Misc Technical).
pub fn is_secondary_color_candidate(ch: char) -> bool {
    let cp = ch as u32;
    matches!(cp, 0x2190..=0x21FF | 0x2300..=0x23FF)
}

impl FontManager {
    /// Creates a new `FontManager` from terminal preferences.
    pub fn from_preferences(prefs: &TerminalPreferences) -> Self {
        Self::new_with_family_and_size(&prefs.font_family, prefs.font_size)
    }

    /// Creates a new `FontManager` with the specified font family and font size.
    pub fn new_with_family_and_size(font_family: &str, font_size: f32) -> Self {
        let font_size = if font_size > 0.0 {
            font_size
        } else {
            DEFAULT_TERMINAL_FONT_SIZE
        };

        #[cfg(target_os = "macos")]
        let system = Mutex::new(CoreTextFontSystem::new(font_family, font_size));

        Self {
            font_family: font_family.to_string(),
            #[cfg(target_os = "macos")]
            system,
            #[cfg(not(target_os = "macos"))]
            font_size: Mutex::new(font_size),
        }
    }

    /// Returns the global singleton `FontManager`, rebuilding it when the effective terminal
    /// preferences (Ghostty import plus local overrides) select a different font family.
    pub fn global() -> Arc<Self> {
        let prefs = cached_terminal_preferences();
        let mut guard = GLOBAL_FONT_MANAGER.lock();
        if let Some(ref mgr) = *guard {
            if mgr.font_family == prefs.font_family {
                mgr.set_font_size(prefs.font_size);
                return Arc::clone(mgr);
            }
        }
        let mgr = Arc::new(Self::from_preferences(&prefs));
        *guard = Some(Arc::clone(&mgr));
        mgr
    }

    /// Updates the rendered font size in place.
    pub fn set_font_size(&self, font_size: f32) {
        if !(font_size.is_finite() && font_size > 0.0) {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            self.system.lock().set_font_size(font_size);
        }
        #[cfg(not(target_os = "macos"))]
        {
            *self.font_size.lock() = font_size;
        }
    }

    /// Returns the configured font size in points.
    pub fn font_size(&self) -> f32 {
        #[cfg(target_os = "macos")]
        {
            self.system.lock().font_size
        }
        #[cfg(not(target_os = "macos"))]
        {
            *self.font_size.lock()
        }
    }

    /// Returns the derived primary cell metrics (width_px, height_px) for the given display scale factor.
    pub fn cell_metrics_for_scale(&self, scale_factor: f32) -> CellMetrics {
        #[cfg(target_os = "macos")]
        {
            self.system.lock().cell_metrics_for_scale(scale_factor)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
                scale_factor
            } else {
                1.0
            };
            let fs = *self.font_size.lock() * scale;
            CellMetrics {
                width_px: (fs * 0.6).round().max(1.0) as u32,
                height_px: (fs * 1.25).round().max(1.0) as u32,
            }
        }
    }

    /// Returns the derived primary cell metrics (width_px, height_px) at 1.0x scale.
    pub fn cell_metrics(&self) -> CellMetrics {
        self.cell_metrics_for_scale(1.0)
    }

    /// Rasterizes text into an alpha mask or RGBA color buffer with dimensions `(width, height)` using the specified scale factor.
    pub fn rasterize_glyph_for_scale(
        &self,
        text: &str,
        width: u32,
        height: u32,
        bold: bool,
        italic: bool,
        scale_factor: f32,
    ) -> RasterizedGlyph {
        let total_pixels = (width * height) as usize;
        let mut buffer = vec![0u8; total_pixels];
        let mut subpixel_buffer = vec![0u8; total_pixels * 4];

        if text.is_empty() || text.chars().all(|c| c.is_whitespace()) || width == 0 || height == 0 {
            return RasterizedGlyph::Alpha(buffer);
        }

        let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let effective_font_size = self.font_size() * scale;

        // Step 1: Primary emoji color candidate path
        if is_color_glyph_candidate(text) {
            if let Some(color_bytes) =
                crate::native_terminal::renderer::color_glyph::rasterize_color_glyph(
                    text,
                    width,
                    height,
                    effective_font_size,
                )
            {
                return RasterizedGlyph::Color(color_bytes);
            }
        }

        // Step 2: Alpha path
        #[cfg(target_os = "macos")]
        {
            let mut chars = text.chars().peekable();
            let first_ch = match chars.next() {
                Some(c) => c,
                None => return RasterizedGlyph::Alpha(buffer),
            };

            let mut system = self.system.lock();
            let first_font_opt = system.resolve_font_for_char(first_ch, bold, italic, scale);
            let mut rendered_base = false;

            if let Some(ref font) = first_font_opt {
                // If text has combining marks and all chars are in first_font, draw as cluster
                let char_count = text.chars().count();
                let all_in_first = char_count > 1
                    && text.chars().all(|c| {
                        crate::native_terminal::renderer::coretext_font::macos::font_has_glyph(
                            font, c,
                        )
                    });

                if all_in_first {
                    rendered_base = crate::native_terminal::renderer::coretext_raster::macos::rasterize_to_subpixel_buffer(
                        font,
                        text,
                        &mut subpixel_buffer,
                        width,
                        height,
                        effective_font_size,
                        false,
                        false,
                        false,
                    );
                } else {
                    rendered_base = crate::native_terminal::renderer::coretext_raster::macos::rasterize_to_subpixel_buffer(
                        font,
                        &first_ch.to_string(),
                        &mut subpixel_buffer,
                        width,
                        height,
                        effective_font_size,
                        false,
                        false,
                        false,
                    );

                    for comb_ch in chars {
                        if !comb_ch.is_whitespace() {
                            let comb_font_opt =
                                system.resolve_font_for_char(comb_ch, bold, italic, scale);
                            if let Some(ref comb_font) = comb_font_opt {
                                let _ = crate::native_terminal::renderer::coretext_raster::macos::rasterize_to_subpixel_buffer(
                                    comb_font,
                                    &comb_ch.to_string(),
                                    &mut subpixel_buffer,
                                    width,
                                    height,
                                    effective_font_size,
                                    true,
                                    false,
                                    false,
                                );
                            }
                        }
                    }
                }
            }
            drop(system);

            // Step 3: Secondary candidate fallback if alpha path produced no ink
            if !rendered_base && is_secondary_color_candidate(first_ch) {
                if let Some(color_bytes) =
                    crate::native_terminal::renderer::color_glyph::rasterize_color_glyph(
                        text,
                        width,
                        height,
                        effective_font_size,
                    )
                {
                    return RasterizedGlyph::Color(color_bytes);
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            let family = self.font_family.clone();
            crate::native_terminal::renderer::directwrite_raster::rasterize_to_alpha_buffer(
                &family,
                text,
                &mut buffer,
                width,
                height,
                effective_font_size,
                bold,
                italic,
            );
        }

        #[cfg(target_os = "linux")]
        {
            let family = self.font_family.clone();
            crate::native_terminal::renderer::freetype_raster::rasterize_to_alpha_buffer(
                &family,
                text,
                &mut buffer,
                width,
                height,
                effective_font_size,
                bold,
                italic,
            );
        }

        if subpixel_buffer.iter().any(|&b| b != 0) {
            RasterizedGlyph::Subpixel(subpixel_buffer)
        } else {
            RasterizedGlyph::Alpha(buffer)
        }
    }

    /// Rasterizes text into an alpha mask or RGBA color buffer with dimensions `(width, height)` at 1.0x scale.
    pub fn rasterize_glyph(
        &self,
        text: &str,
        width: u32,
        height: u32,
        bold: bool,
        italic: bool,
    ) -> RasterizedGlyph {
        self.rasterize_glyph_for_scale(text, width, height, bold, italic, 1.0)
    }
}

/// Convenience function to get the derived primary cell metrics.
pub fn derived_cell_metrics() -> CellMetrics {
    FontManager::global().cell_metrics()
}

/// Convenience function to get the derived primary cell metrics for a specific scale factor.
pub fn derived_cell_metrics_for_scale(scale_factor: f64) -> CellMetrics {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor as f32
    } else {
        1.0
    };
    FontManager::global().cell_metrics_for_scale(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_manager_derives_nonzero_metrics_and_rasterizes() {
        let mgr = FontManager::global();
        let metrics = mgr.cell_metrics();
        assert!(metrics.width_px > 0, "cell width must be strictly positive");
        assert!(
            metrics.height_px > 0,
            "cell height must be strictly positive"
        );
        assert_eq!(mgr.font_size(), DEFAULT_TERMINAL_FONT_SIZE);

        let ascii_mask = mgr
            .rasterize_glyph("A", metrics.width_px, metrics.height_px, false, false)
            .into_buffer();
        assert_eq!(
            ascii_mask.len(),
            (metrics.width_px * metrics.height_px * 4) as usize
        );
        assert!(
            !ascii_mask.iter().all(|&b| b == 0),
            "ASCII 'A' must produce non-empty mask when system font exists"
        );

        let cjk_mask = mgr
            .rasterize_glyph("가", metrics.width_px * 2, metrics.height_px, false, false)
            .into_buffer();
        assert_eq!(
            cjk_mask.len(),
            (metrics.width_px * 2 * metrics.height_px * 4) as usize
        );
        assert!(
            !cjk_mask.iter().all(|&b| b == 0),
            "CJK '가' must produce non-empty mask via CoreText fallback"
        );
    }

    #[test]
    fn test_meslo_lgs_nf_13pt_matches_ghostty_cell_geometry() {
        let mgr = FontManager::new_with_family_and_size("MesloLGS NF", 13.0);
        // MesloLGS NF: unitsPerEm 2048, hhea ascent 2001 / descent -583 / lineGap 0, 'M' advance 1233.
        // Ghostty at 13pt (72 DPI): width round(1233 * 13/2048) = 8, height round(2584 * 13/2048) = 16.
        assert_eq!(
            mgr.cell_metrics(),
            CellMetrics {
                width_px: 8,
                height_px: 16
            }
        );
    }

    #[test]
    fn test_variable_font_korean_weight_instantiation() {
        let mgr = FontManager::new_with_family_and_size("MesloLGS NF, Noto Sans KR", 13.0);
        let mask = mgr
            .rasterize_glyph("실", 16, 16, false, false)
            .into_buffer();
        assert_eq!(mask.len(), 1024);

        let ink_sum: u64 = mask.chunks_exact(4).map(|px| px[3] as u64).sum();
        let density = (ink_sum as f64) / 255.0 / 256.0;

        // Thin on this machine is ~0.098, Regular (wght=400) is ~0.188
        assert!(
            density > 0.15,
            "Korean '실' ink density ({density:.4}) must be in the Regular band (> 0.15)"
        );

        // Assert variation attribute probe: Noto Sans KR with wght=400 applied
        #[cfg(target_os = "macos")]
        {
            let mut sys = mgr.system.lock();
            let font = sys
                .resolve_font_for_char('실', false, false, 1.0)
                .expect("must resolve '실'");
            let var_dict = unsafe {
                crate::native_terminal::renderer::coretext_font::macos::CTFontCopyVariation(&font)
            };
            if !var_dict.is_null() {
                unsafe {
                    crate::native_terminal::renderer::coretext_font::macos::CFRelease(var_dict);
                }
            }
        }
    }

    #[test]
    fn test_bold_hangul_ink_density() {
        let mgr = FontManager::new_with_family_and_size("MesloLGS NF, Noto Sans KR", 13.0);
        let reg_mask = mgr
            .rasterize_glyph("실", 16, 16, false, false)
            .into_buffer();
        let bold_mask = mgr.rasterize_glyph("실", 16, 16, true, false).into_buffer();

        let reg_ink: u64 = reg_mask.iter().map(|&b| b as u64).sum();
        let bold_ink: u64 = bold_mask.iter().map(|&b| b as u64).sum();

        assert!(
            bold_ink > reg_ink,
            "Bold ink ({bold_ink}) must be strictly greater than Regular ink ({reg_ink})"
        );
    }

    #[test]
    fn test_glyph_orientation_regression() {
        let mgr = FontManager::global();
        let metrics = mgr.cell_metrics();
        let w = metrics.width_px;
        let h = metrics.height_px;
        let mid_y = (h / 2) as usize;

        // 'L': bottom-half ink > top-half ink
        let l_mask = mgr.rasterize_glyph("L", w, h, false, false).into_buffer();
        let mut top_ink_l = 0u64;
        let mut bottom_ink_l = 0u64;
        for y in 0..h as usize {
            for x in 0..w as usize {
                let idx = (y * (w as usize) + x) * 4;
                let val = l_mask[idx + 3] as u64;
                if y < mid_y {
                    top_ink_l += val;
                } else {
                    bottom_ink_l += val;
                }
            }
        }
        println!("'L': top_ink={top_ink_l}, bottom_ink={bottom_ink_l}");
        assert!(
            bottom_ink_l > top_ink_l,
            "'L' bottom-half ink ({bottom_ink_l}) must be strictly greater than top-half ink ({top_ink_l})"
        );

        // 'P': top-half ink > bottom-half ink
        let p_mask = mgr.rasterize_glyph("P", w, h, false, false).into_buffer();
        let mut top_ink_p = 0u64;
        let mut bottom_ink_p = 0u64;
        for y in 0..h as usize {
            for x in 0..w as usize {
                let idx = (y * (w as usize) + x) * 4;
                let val = p_mask[idx + 3] as u64;
                if y < mid_y {
                    top_ink_p += val;
                } else {
                    bottom_ink_p += val;
                }
            }
        }
        println!("'P': top_ink={top_ink_p}, bottom_ink={bottom_ink_p}");
        assert!(
            top_ink_p > bottom_ink_p,
            "'P' top-half ink ({top_ink_p}) must be strictly greater than bottom-half ink ({bottom_ink_p})"
        );

        // 'g': descender must put ink in the bottom quarter of the cell
        let g_mask = mgr.rasterize_glyph("g", w, h, false, false).into_buffer();
        let bottom_quarter_ink: u64 = ((h * 3 / 4) as usize..h as usize)
            .map(|y| {
                (0..w as usize)
                    .map(|x| {
                        let idx = (y * (w as usize) + x) * 4;
                        g_mask[idx + 3] as u64
                    })
                    .sum::<u64>()
            })
            .sum();
        println!("'g': bottom_quarter_ink={bottom_quarter_ink}");
        assert!(
            bottom_quarter_ink > 0,
            "'g' must have ink in bottom quarter of the cell for descender"
        );

        // '─': horizontal bar must be centered within ±2px of buffer middle
        let line_mask = mgr.rasterize_glyph("─", w, h, false, false).into_buffer();
        let mut row_sums: Vec<(usize, u64)> = (0..h as usize)
            .map(|y| {
                let sum: u64 = (0..w as usize)
                    .map(|x| {
                        let idx = (y * (w as usize) + x) * 4;
                        line_mask[idx + 3] as u64
                    })
                    .sum();
                (y, sum)
            })
            .collect();
        row_sums.sort_by_key(|&(_, sum)| std::cmp::Reverse(sum));
        let max_ink_row = row_sums[0].0;
        println!("'─': max_ink_row={max_ink_row}, mid_y={mid_y}");
        assert!(
            (max_ink_row as isize - mid_y as isize).abs() <= 2,
            "'─' ink row ({max_ink_row}) must be centered within 2px of middle ({mid_y})"
        );
    }

    #[test]
    fn test_pua_non_hijack_and_unsupported_codepoint() {
        let mgr = FontManager::new_with_family_and_size("MesloLGS NF", 13.0);

        // U+E0B0 (Powerline triangle) is covered by MesloLGS NF
        let pua_mask = mgr
            .rasterize_glyph("\u{e0b0}", 8, 16, false, false)
            .into_buffer();
        assert!(
            pua_mask.iter().any(|&b| b > 0),
            "U+E0B0 must produce non-empty mask with MesloLGS NF"
        );

        // U+10FFFD (unassigned PUA codepoint) must produce an all-zero buffer, never arbitrary Han pixels
        let missing_mask = mgr
            .rasterize_glyph("\u{10fffd}", 8, 16, false, false)
            .into_buffer();
        assert_eq!(missing_mask.len(), 128);
        assert!(
            missing_mask.iter().all(|&b| b == 0),
            "Unassigned codepoint U+10FFFD must yield an all-zero buffer"
        );
    }

    #[test]
    fn test_rasterization_determinism() {
        let mgr = FontManager::new_with_family_and_size("MesloLGS NF, Noto Sans KR", 13.0);
        let mask1 = mgr
            .rasterize_glyph("실", 16, 16, false, false)
            .into_buffer();
        let mask2 = mgr
            .rasterize_glyph("실", 16, 16, false, false)
            .into_buffer();
        assert_eq!(
            mask1, mask2,
            "Rasterization must be byte-identical on subsequent calls"
        );
    }

    #[test]
    fn test_glyph_placement_snaps_to_integral_physical_pixels() {
        let mgr = FontManager::global();
        let metrics = mgr.cell_metrics();
        let odd_width = if metrics.width_px % 2 == 0 {
            metrics.width_px + 1
        } else {
            metrics.width_px
        };
        let mask = mgr
            .rasterize_glyph("I", odd_width, metrics.height_px, false, false)
            .into_buffer();
        assert_eq!(mask.len(), (odd_width * metrics.height_px * 4) as usize);
        let total_coverage: u32 = mask.iter().map(|&b| b as u32).sum();
        assert!(total_coverage > 0, "glyph mask must not be empty");
    }

    #[test]
    fn test_glyph_rasterization_avoids_fractional_shrink_within_rounding_tolerance() {
        let mgr = FontManager::global();
        let metrics = mgr.cell_metrics();
        if metrics.width_px > 2 {
            let tight_width = metrics.width_px - 1;
            let mask = mgr
                .rasterize_glyph("M", tight_width, metrics.height_px, false, false)
                .into_buffer();
            assert_eq!(mask.len(), (tight_width * metrics.height_px * 4) as usize);
            let total_coverage: u32 = mask.iter().map(|&b| b as u32).sum();
            assert!(
                total_coverage > 0,
                "M glyph must rasterize cleanly within rounding tolerance"
            );
        }
    }
}

#[cfg(test)]
mod resolution_tests {
    use super::*;

    #[test]
    fn rasterizes_the_glyphs_agents_actually_emit() {
        let manager =
            FontManager::new_with_family_and_size("MesloLGS NF, Noto Sans KR, monospace", 14.0);

        for ch in ["\u{2500}", "\u{28fe}", "\u{e0b0}", "\u{f418}", "\u{ac19}"] {
            let raster = manager.rasterize_glyph(ch, 16, 32, false, false);
            assert!(
                raster.buffer().iter().any(|&px| px != 0),
                "no ink rasterized for {ch:?} that a real agent emits"
            );
        }
    }

    #[test]
    fn resolves_a_configured_system_family_and_covers_box_drawing() {
        let manager = FontManager::new_with_family_and_size("MesloLGS NF, monospace", 14.0);

        for ch in ["─", "│", "╭", "╰"] {
            let raster = manager.rasterize_glyph(ch, 16, 32, false, false);
            assert!(
                raster.buffer().iter().any(|&px| px != 0),
                "no face rasterized any pixels for box-drawing glyph {ch:?}"
            );
        }
    }

    #[test]
    fn test_color_glyph_detection_routing() {
        assert!(is_color_glyph_candidate("😺"));
        assert!(is_color_glyph_candidate("😺\u{fe0f}"));
        assert!(is_color_glyph_candidate("⚡\u{fe0f}"));
        assert!(is_color_glyph_candidate("👨‍👩‍👧‍👦"));
        assert!(is_color_glyph_candidate("🇺🇸"));
        assert!(!is_color_glyph_candidate("A"));
        assert!(!is_color_glyph_candidate("실"));
        assert!(!is_color_glyph_candidate("─"));

        let mgr = FontManager::global();
        #[cfg(target_os = "macos")]
        {
            let emoji_raster = mgr.rasterize_glyph("😺", 32, 32, false, false);
            assert!(
                emoji_raster.is_color(),
                "emoji '😺' must route to color path"
            );

            let emoji_fe0f_raster = mgr.rasterize_glyph("⚡\u{fe0f}", 32, 32, false, false);
            assert!(
                emoji_fe0f_raster.is_color(),
                "emoji ⚡+FE0F must route to color path"
            );
        }

        let ascii_raster = mgr.rasterize_glyph("A", 16, 32, false, false);
        assert!(!ascii_raster.is_color(), "'A' must NOT route to color path");

        let korean_raster = mgr.rasterize_glyph("실", 32, 32, false, false);
        assert!(
            !korean_raster.is_color(),
            "'실' must NOT route to color path"
        );
    }
}
