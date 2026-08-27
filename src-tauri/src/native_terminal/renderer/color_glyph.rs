//! CoreText and CoreGraphics based color emoji rasterizer (macOS only).

#[cfg(target_os = "macos")]
pub mod macos {
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};

    use objc2_core_foundation::{
        kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks, CFAttributedString,
        CFDictionary, CFRetained, CFString, CGFloat,
    };
    use objc2_core_graphics::CGColorSpace;
    use objc2_core_text::{kCTFontAttributeName, CTFont, CTLine};

    #[repr(C)]
    pub struct CGContext {
        _private: [u8; 0],
    }

    extern "C" {
        fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits_per_component: usize,
            bytes_per_row: usize,
            space: *const CGColorSpace,
            bitmap_info: u32,
        ) -> *mut CGContext;

        fn CGContextRelease(context: *mut CGContext);
        fn CGContextSetTextPosition(context: *mut CGContext, x: CGFloat, y: CGFloat);
        fn CTLineDraw(line: &CTLine, context: *mut CGContext);
    }

    const K_CG_IMAGE_ALPHA_PREMULTIPLIED_LAST: u32 = 1;
    const K_CG_BITMAP_BYTE_ORDER_32_BIG: u32 = 4 << 12; // 0x4000
    const BITMAP_INFO_RGBA: u32 = K_CG_IMAGE_ALPHA_PREMULTIPLIED_LAST | K_CG_BITMAP_BYTE_ORDER_32_BIG;

    static LOGGED_ERROR: AtomicBool = AtomicBool::new(false);

    fn log_error_once(msg: &str) {
        if !LOGGED_ERROR.swap(true, Ordering::Relaxed) {
            tracing::warn!("Color glyph rasterization error: {msg}");
        }
    }

    /// Rasterizes a text cluster into an RGBA8 premultiplied pixel buffer (width * height * 4),
    /// row-major, top-left origin.
    pub fn rasterize_color_glyph(
        text: &str,
        width: u32,
        height: u32,
        font_size: f32,
    ) -> Option<Vec<u8>> {
        if text.is_empty() || width == 0 || height == 0 || font_size <= 0.0 {
            return None;
        }

        let result = std::panic::catch_unwind(|| {
            let cf_str = CFString::from_str(text);
            let font_name = CFString::from_str("Apple Color Emoji");
            let mut font = unsafe { CTFont::with_name(&font_name, font_size as f64, ptr::null()) };

            let create_line = |font_ref: &CFRetained<CTFont>| -> Option<(CFRetained<CTLine>, CGFloat, CGFloat, CGFloat)> {
                let font_attr_ptr = unsafe { kCTFontAttributeName as *const CFString as *const c_void };
                let mut keys = [font_attr_ptr];
                let mut values = [CFRetained::as_ptr(font_ref).as_ptr() as *const c_void];
                let attributes = unsafe {
                    CFDictionary::new(
                        None,
                        keys.as_mut_ptr(),
                        values.as_mut_ptr(),
                        1,
                        &kCFTypeDictionaryKeyCallBacks,
                        &kCFTypeDictionaryValueCallBacks,
                    )?
                };

                let attr_str = unsafe {
                    CFAttributedString::new(
                        None,
                        Some(&cf_str),
                        Some(&attributes),
                    )?
                };

                let line = unsafe { CTLine::with_attributed_string(&attr_str) };
                let mut ascent: CGFloat = 0.0;
                let mut descent: CGFloat = 0.0;
                let mut leading: CGFloat = 0.0;
                let typographic_width = unsafe {
                    line.typographic_bounds(&mut ascent, &mut descent, &mut leading)
                };

                Some((line, typographic_width, ascent, descent))
            };

            let (mut line, mut typographic_width, mut ascent, mut descent) =
                create_line(&font)?;

            if typographic_width <= 0.0 && ascent <= 0.0 {
                return None;
            }

            let init_font_height = ascent + descent;
            let scale_ratio_y = if init_font_height > (height as f64 + 1.0) && height > 0 {
                (height as f64) / init_font_height
            } else {
                1.0
            };
            let scale_ratio_x = if typographic_width > (width as f64 + 1.0) && width > 0 {
                (width as f64) / typographic_width
            } else {
                1.0
            };
            let scale_ratio = scale_ratio_x.min(scale_ratio_y);

            if scale_ratio < 0.99 {
                let scaled_size = (font_size as f64) * scale_ratio;
                font = unsafe { CTFont::with_name(&font_name, scaled_size, ptr::null()) };
                if let Some((l, tw, a, d)) = create_line(&font) {
                    line = l;
                    typographic_width = tw;
                    ascent = a;
                    descent = d;
                }
            }

            let color_space = CGColorSpace::new_device_rgb()?;
            let bytes_per_row = (width as usize) * 4;
            let total_bytes = bytes_per_row * (height as usize);
            let mut raw_buffer = vec![0u8; total_bytes];

            let context_raw = unsafe {
                CGBitmapContextCreate(
                    raw_buffer.as_mut_ptr() as *mut c_void,
                    width as usize,
                    height as usize,
                    8,
                    bytes_per_row,
                    CFRetained::as_ptr(&color_space).as_ptr(),
                    BITMAP_INFO_RGBA,
                )
            };

            if context_raw.is_null() {
                return None;
            }

            let font_height = ascent + descent;
            let v_offset = (((height as f64) - font_height) / 2.0).floor();
            // CoreGraphics coordinates: (0,0) is bottom-left. Baseline Y is distance from bottom.
            let baseline_cg_y = (descent + v_offset.max(0.0)).round();

            // Horizontal alignment: center glyph horizontally within target width if smaller
            let pos_x = if typographic_width > 0.0 && typographic_width < (width as f64) {
                (((width as f64) - typographic_width) / 2.0).floor()
            } else {
                0.0
            };

            unsafe {
                CGContextSetTextPosition(context_raw, pos_x, baseline_cg_y);
                CTLineDraw(&line, context_raw);
                CGContextRelease(context_raw);
            }

            // Check if any pixels were actually drawn
            let has_non_zero_pixel = raw_buffer.iter().any(|&b| b != 0);
            if !has_non_zero_pixel {
                return None;
            }

            // Flip rows vertically from bottom-left origin (CoreGraphics) to top-left origin
            let mut flipped = vec![0u8; total_bytes];
            let row_bytes = (width as usize) * 4;
            for y in 0..(height as usize) {
                let src_y = (height as usize) - 1 - y;
                let src_offset = src_y * row_bytes;
                let dst_offset = y * row_bytes;
                flipped[dst_offset..dst_offset + row_bytes]
                    .copy_from_slice(&raw_buffer[src_offset..src_offset + row_bytes]);
            }

            Some(flipped)
        });

        match result {
            Ok(opt) => opt,
            Err(_) => {
                log_error_once("panic during color glyph rasterization");
                None
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub mod non_macos {
    pub fn rasterize_color_glyph(
        _text: &str,
        _width: u32,
        _height: u32,
        _font_size: f32,
    ) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(target_os = "macos")]
pub use macos::rasterize_color_glyph;

#[cfg(not(target_os = "macos"))]
pub use non_macos::rasterize_color_glyph;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn test_rasterize_color_glyph_cat_emoji() {
        let width = 32;
        let height = 32;
        let font_size = 16.0;
        let result = rasterize_color_glyph("😺", width, height, font_size);
        assert!(result.is_some(), "rasterize_color_glyph(\"😺\") must return Some");

        let buffer = result.unwrap();
        assert_eq!(buffer.len(), (width * height * 4) as usize);

        // Find at least one pixel where alpha > 200 and NOT all channels equal (proves color, not silhouette)
        let mut found_color_pixel = false;
        for chunk in buffer.chunks_exact(4) {
            let r = chunk[0];
            let g = chunk[1];
            let b = chunk[2];
            let a = chunk[3];
            if a > 200 && !(r == g && g == b) {
                found_color_pixel = true;
                break;
            }
        }

        assert!(
            found_color_pixel,
            "must have at least one pixel with alpha > 200 and distinct RGB channels (color emoji)"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_rasterize_color_glyph_complex_sequences() {
        for emoji in ["👨‍👩‍👧‍👦", "🇺🇸", "⚡\u{fe0f}", "❤️"] {
            let result = rasterize_color_glyph(emoji, 32, 32, 16.0);
            assert!(result.is_some(), "emoji {emoji:?} must rasterize");
            let buf = result.unwrap();
            assert!(buf.iter().any(|&b| b > 0), "emoji {emoji:?} must have non-zero pixels");
        }
    }
}
