//! Linux grapheme shaping through the same Pango/HarfBuzz/FreeType stack as GTK.
//!
//! Single-scalar non-emoji glyphs retain the existing FreeType fast path. This path is
//! called only on atlas misses for clusters and emoji, which require shaping and color
//! strike support rather than `FT_Load_Char` on the first scalar.

use std::ffi::{c_char, c_int, c_void, CString};
use std::ptr;

use super::rasterizer::RasterizedGlyph;

#[repr(C)]
#[derive(Default)]
struct Rectangle {
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
}

#[link(name = "pangocairo-1.0")]
unsafe extern "C" {
    fn pango_cairo_create_layout(cr: *mut c_void) -> *mut c_void;
    fn pango_cairo_show_layout(cr: *mut c_void, layout: *mut c_void);
}
#[link(name = "pango-1.0")]
unsafe extern "C" {
    fn pango_font_description_new() -> *mut c_void;
    fn pango_font_description_free(desc: *mut c_void);
    fn pango_font_description_set_family(desc: *mut c_void, family: *const c_char);
    fn pango_font_description_set_absolute_size(desc: *mut c_void, size: f64);
    fn pango_font_description_set_weight(desc: *mut c_void, weight: c_int);
    fn pango_font_description_set_style(desc: *mut c_void, style: c_int);
    fn pango_layout_set_font_description(layout: *mut c_void, desc: *const c_void);
    fn pango_layout_set_text(layout: *mut c_void, text: *const c_char, length: c_int);
    fn pango_layout_set_single_paragraph_mode(layout: *mut c_void, setting: c_int);
    fn pango_layout_get_pixel_extents(
        layout: *mut c_void,
        ink: *mut Rectangle,
        logical: *mut Rectangle,
    );
    fn pango_layout_get_baseline(layout: *mut c_void) -> c_int;
    fn pango_layout_get_unknown_glyphs_count(layout: *mut c_void) -> c_int;
}
#[link(name = "cairo")]
unsafe extern "C" {
    fn cairo_image_surface_create(format: c_int, width: c_int, height: c_int) -> *mut c_void;
    fn cairo_surface_status(surface: *mut c_void) -> c_int;
    fn cairo_surface_destroy(surface: *mut c_void);
    fn cairo_surface_flush(surface: *mut c_void);
    fn cairo_image_surface_get_data(surface: *mut c_void) -> *mut u8;
    fn cairo_image_surface_get_stride(surface: *mut c_void) -> c_int;
    fn cairo_create(surface: *mut c_void) -> *mut c_void;
    fn cairo_destroy(cr: *mut c_void);
    fn cairo_status(cr: *mut c_void) -> c_int;
    fn cairo_set_source_rgba(cr: *mut c_void, r: f64, g: f64, b: f64, a: f64);
    fn cairo_move_to(cr: *mut c_void, x: f64, y: f64);
    fn cairo_scale(cr: *mut c_void, sx: f64, sy: f64);
}
#[link(name = "gobject-2.0")]
unsafe extern "C" {
    fn g_object_unref(object: *mut c_void);
}

// All handles belong to this invocation, never cross threads, and are released on early return.
struct Drawing {
    surface: *mut c_void,
    cr: *mut c_void,
    layout: *mut c_void,
    desc: *mut c_void,
}
impl Drop for Drawing {
    fn drop(&mut self) {
        // SAFETY: non-null handles were created by their matching constructors below.
        unsafe {
            if !self.layout.is_null() {
                g_object_unref(self.layout);
            }
            if !self.desc.is_null() {
                pango_font_description_free(self.desc);
            }
            if !self.cr.is_null() {
                cairo_destroy(self.cr);
            }
            if !self.surface.is_null() {
                cairo_surface_destroy(self.surface);
            }
        }
    }
}

/// Returns premultiplied RGBA for real color pixels, alpha coverage otherwise.
/// No display connection is used: Cairo renders into an owned CPU image surface.
pub fn rasterize(
    family: &str,
    text: &str,
    width: u32,
    height: u32,
    font_size: f32,
    bold: bool,
    italic: bool,
) -> Option<RasterizedGlyph> {
    if width == 0
        || height == 0
        || width > i32::MAX as u32
        || height > i32::MAX as u32
        || !font_size.is_finite()
        || font_size <= 0.0
        || text.is_empty()
    {
        return None;
    }
    let pixels = (width as usize).checked_mul(height as usize)?;
    let bytes = pixels.checked_mul(4)?;
    let text = CString::new(text).ok()?;
    // Pango accepts comma-separated families, but not CSS family quotation marks.
    let family = CString::new(
        family
            .split(',')
            .map(|s| s.trim().trim_matches(['\'', '"']).trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(","),
    )
    .ok()?;
    let mut draw = Drawing {
        surface: ptr::null_mut(),
        cr: ptr::null_mut(),
        layout: ptr::null_mut(),
        desc: ptr::null_mut(),
    };
    // SAFETY: opaque objects stay live through Drawing; UTF-8 CStrings remain live for calls
    // which copy them. The image buffer is read only after flush and within reported stride.
    unsafe {
        // CAIRO_FORMAT_ARGB32 is native-endian, premultiplied, initially transparent.
        draw.surface = cairo_image_surface_create(0, width as c_int, height as c_int);
        if draw.surface.is_null() || cairo_surface_status(draw.surface) != 0 {
            return None;
        }
        draw.cr = cairo_create(draw.surface);
        if draw.cr.is_null() || cairo_status(draw.cr) != 0 {
            return None;
        }
        draw.layout = pango_cairo_create_layout(draw.cr);
        draw.desc = pango_font_description_new();
        if draw.layout.is_null() || draw.desc.is_null() {
            return None;
        }
        pango_font_description_set_family(draw.desc, family.as_ptr());
        pango_font_description_set_weight(draw.desc, if bold { 700 } else { 400 });
        pango_font_description_set_style(draw.desc, if italic { 2 } else { 0 });
        pango_font_description_set_absolute_size(draw.desc, f64::from(font_size) * 1024.0);
        pango_layout_set_font_description(draw.layout, draw.desc);
        pango_layout_set_single_paragraph_mode(draw.layout, 1);
        pango_layout_set_text(draw.layout, text.as_ptr(), -1);
        if pango_layout_get_unknown_glyphs_count(draw.layout) != 0 {
            return None;
        }
        let mut ink = Rectangle::default();
        pango_layout_get_pixel_extents(draw.layout, &mut ink, ptr::null_mut());
        if ink.width <= 0 || ink.height <= 0 {
            return None;
        }
        // Bitmap emoji can have a large natural strike. Reshape at the fitted size, rather
        // than treating its BGRA bitmap as grayscale or cropping it to one terminal cell.
        let fit = (f64::from(width) / f64::from(ink.width))
            .min(f64::from(height) / f64::from(ink.height))
            .min(1.0);
        if fit < 1.0 {
            pango_font_description_set_absolute_size(
                draw.desc,
                f64::from(font_size) * fit * 1024.0,
            );
            pango_layout_set_font_description(draw.layout, draw.desc);
            pango_layout_get_pixel_extents(draw.layout, &mut ink, ptr::null_mut());
        }
        if ink.width <= 0 || ink.height <= 0 {
            return None;
        }
        // Hinting/fixed bitmap strikes may round up after the size change. Preserve the
        // shaped layout and apply any remaining fit to its drawing, not another reshape.
        let draw_scale = (f64::from(width) / f64::from(ink.width))
            .min(f64::from(height) / f64::from(ink.height))
            .min(1.0);
        let available_width = f64::from(width) / draw_scale;
        let available_height = f64::from(height) / draw_scale;
        let baseline = f64::from(height - (height / 5).max(1)) / draw_scale;
        let x = (-f64::from(ink.x))
            .max(0.0)
            .min(available_width - f64::from(ink.x) - f64::from(ink.width));
        let y = (baseline - f64::from(pango_layout_get_baseline(draw.layout)) / 1024.0)
            .max(-f64::from(ink.y))
            .min(available_height - f64::from(ink.y) - f64::from(ink.height));
        cairo_set_source_rgba(draw.cr, 1.0, 1.0, 1.0, 1.0);
        cairo_scale(draw.cr, draw_scale, draw_scale);
        cairo_move_to(draw.cr, x, y);
        pango_cairo_show_layout(draw.cr, draw.layout);
        if cairo_status(draw.cr) != 0 {
            return None;
        }
        cairo_surface_flush(draw.surface);
        let data = cairo_image_surface_get_data(draw.surface);
        let stride = cairo_image_surface_get_stride(draw.surface);
        if data.is_null() || stride < 0 || (stride as usize) < width as usize * 4 {
            return None;
        }
        let mut rgba = vec![0; bytes];
        let mut colored = false;
        for row in 0..height as usize {
            for col in 0..width as usize {
                let src = data.add(row * stride as usize + col * 4);
                let argb = u32::from_ne_bytes([*src, *src.add(1), *src.add(2), *src.add(3)]);
                let [a, r, g, b] = argb.to_be_bytes();
                // The text source is white (RGB == alpha after premultiplication).
                // Black/white color-font artwork must also retain its intrinsic palette.
                colored |= a != 0 && (r != a || g != a || b != a);
                rgba[(row * width as usize + col) * 4..][..4].copy_from_slice(&[r, g, b, a]);
            }
        }
        if !rgba.chunks_exact(4).any(|p| p[3] != 0) {
            return None;
        }
        Some(if colored {
            RasterizedGlyph::Color(rgba)
        } else {
            RasterizedGlyph::Alpha(rgba.chunks_exact(4).map(|p| p[3]).collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(text: &str) -> RasterizedGlyph {
        rasterize("DejaVu Sans Mono", text, 64, 64, 40.0, false, false)
            .expect("Linux cluster QA requires DejaVu Sans Mono and Noto Color Emoji")
    }

    #[test]
    fn combining_mark_is_shaped_and_matches_precomposed_accent() {
        let plain = render("e");
        let decomposed = render("e\u{301}");
        assert_ne!(decomposed, plain, "the accent must not be discarded");
        assert_eq!(decomposed, render("é"));
    }

    #[test]
    fn emoji_color_strike_and_zwj_sequence_are_rasterized() {
        for text in ["🦀", "👩‍💻", "🇺🇸", "1\u{fe0f}\u{20e3}"] {
            let RasterizedGlyph::Color(pixels) = render(text) else {
                panic!("{text:?} must produce color emoji pixels");
            };
            assert!(pixels.chunks_exact(4).any(|p| p[3] > 0 && p[0] != p[1]));
            assert!(
                pixels
                    .chunks_exact(4)
                    .all(|p| p[..3].iter().all(|&c| c <= p[3])),
                "RGBA must remain premultiplied for the atlas shader"
            );
        }
        assert_ne!(
            render("👩‍💻"),
            render("👩"),
            "ZWJ suffix must not be discarded"
        );
    }

    #[test]
    fn grayscale_color_emoji_retains_black_and_white_artwork() {
        let RasterizedGlyph::Color(pixels) = render("🏁") else {
            panic!("black/white emoji must not become a foreground-tinted alpha mask");
        };
        assert!(pixels
            .chunks_exact(4)
            .any(|p| p[3] > 128 && p[0] < p[3] / 2));
        assert!(pixels
            .chunks_exact(4)
            .any(|p| p[3] > 128 && p[0] > (u16::from(p[3]) * 3 / 4) as u8));
    }

    #[test]
    fn variation_selectors_preserve_text_and_emoji_presentation() {
        assert!(matches!(render("❤\u{fe0e}"), RasterizedGlyph::Alpha(_)));
        assert!(matches!(render("❤\u{fe0f}"), RasterizedGlyph::Color(_)));
    }

    #[test]
    fn fitted_clusters_keep_cell_bounds_at_terminal_scales_and_styles() {
        for (width, height, size) in [(8, 16, 12.0), (16, 20, 16.0), (32, 40, 32.0)] {
            for (bold, italic) in [(false, false), (true, false), (false, true)] {
                for text in ["e\u{301}", "👩‍💻", "1\u{fe0f}\u{20e3}"] {
                    let glyph =
                        rasterize("DejaVu Sans Mono", text, width, height, size, bold, italic)
                            .expect("installed fixture fonts must rasterize even small cells");
                    match glyph {
                        RasterizedGlyph::Alpha(p) => {
                            assert_eq!(p.len(), (width * height) as usize);
                            assert!(p.iter().any(|&v| v != 0));
                        }
                        RasterizedGlyph::Color(p) => {
                            assert_eq!(p.len(), (width * height * 4) as usize);
                            assert!(p.chunks_exact(4).any(|v| v[3] != 0));
                        }
                        _ => panic!("Linux cluster path must not produce subpixel output"),
                    }
                }
            }
        }
    }

    #[test]
    fn narrow_positive_bearing_accent_matches_full_precomposed_glyph() {
        for width in [1, 2, 3, 8, 12] {
            for italic in [false, true] {
                let nfd = rasterize(
                    "DejaVu Sans Mono",
                    "e\u{301}",
                    width,
                    20,
                    24.0,
                    false,
                    italic,
                )
                .expect("narrow decomposed glyph");
                let nfc = rasterize("DejaVu Sans Mono", "é", width, 20, 24.0, false, italic)
                    .expect("narrow composed glyph");
                assert_eq!(nfd, nfc, "same shaped glyph must fit at width {width}");
            }
        }
    }

    #[test]
    fn invalid_sizes_and_empty_clusters_are_rejected() {
        for (text, width, height, size) in [
            ("", 16, 20, 16.0),
            ("x", 0, 20, 16.0),
            ("x", 16, 0, 16.0),
            ("x", 16, 20, f32::NAN),
            ("x", 16, 20, -1.0),
        ] {
            assert!(rasterize("monospace", text, width, height, size, false, false).is_none());
        }
    }
}
