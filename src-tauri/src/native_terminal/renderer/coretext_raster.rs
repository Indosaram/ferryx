//! CoreText / CoreGraphics alpha glyph rasterizer (macOS).

#[cfg(target_os = "macos")]
pub mod macos {
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};

    use objc2_core_foundation::{
        kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks, CFDictionary, CFRetained,
        CFString, CGFloat,
    };
    use objc2_core_graphics::CGColorSpace;
    use objc2_core_text::{kCTFontAttributeName, CTFont, CTLine};

    static LOGGED_ERROR: AtomicBool = AtomicBool::new(false);

    fn log_error_once(msg: &str) {
        if !LOGGED_ERROR.swap(true, Ordering::Relaxed) {
            tracing::warn!("CoreText alpha rasterization error: {msg}");
        }
    }

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
        fn CGContextSetAllowsFontSmoothing(context: *mut CGContext, flag: bool);
        fn CGContextSetShouldSmoothFonts(context: *mut CGContext, flag: bool);
        fn CGContextSetAllowsFontSubpixelPositioning(context: *mut CGContext, flag: bool);
        fn CGContextSetShouldSubpixelPositionFonts(context: *mut CGContext, flag: bool);
        fn CGContextSetAllowsFontSubpixelQuantization(context: *mut CGContext, flag: bool);
        fn CGContextSetShouldSubpixelQuantizeFonts(context: *mut CGContext, flag: bool);
        fn CGContextSetAllowsAntialiasing(context: *mut CGContext, flag: bool);
        fn CGContextSetShouldAntialias(context: *mut CGContext, flag: bool);
        fn CGContextRelease(context: *mut CGContext);
        fn CGContextSetTextPosition(context: *mut CGContext, x: CGFloat, y: CGFloat);
        fn CGContextSetRGBFillColor(
            context: *mut CGContext,
            r: CGFloat,
            g: CGFloat,
            b: CGFloat,
            a: CGFloat,
        );
        fn CTLineDraw(line: &CTLine, context: *mut CGContext);
        fn CFAttributedStringCreate(
            alloc: *const c_void,
            str: *const CFString,
            attributes: *const c_void,
        ) -> *const c_void;
        fn CTLineCreateWithAttributedString(attrStr: *const c_void) -> CFRetained<CTLine>;
        fn CTLineGetTypographicBounds(
            line: &CTLine,
            ascent: *mut CGFloat,
            descent: *mut CGFloat,
            leading: *mut CGFloat,
        ) -> f64;
        fn CFRelease(cf: *const c_void);
    }

    const K_CG_IMAGE_ALPHA_PREMULTIPLIED_LAST: u32 = 1;
    const K_CG_BITMAP_BYTE_ORDER_32_BIG: u32 = 4 << 12; // 0x4000
    const BITMAP_INFO_RGBA: u32 =
        K_CG_IMAGE_ALPHA_PREMULTIPLIED_LAST | K_CG_BITMAP_BYTE_ORDER_32_BIG;

    /// Rasterizes a text cluster/character using `font` into an 8-bit alpha mask buffer (`width * height`),
    /// top-left origin.
    #[allow(clippy::too_many_arguments)]
    pub fn rasterize_to_alpha_buffer(
        font: &CTFont,
        text: &str,
        buffer: &mut [u8],
        width: u32,
        height: u32,
        _font_size: f32,
        is_combining: bool,
        needs_synthetic_bold: bool,
        needs_synthetic_italic: bool,
    ) -> bool {
        if text.is_empty() || width == 0 || height == 0 || buffer.len() < (width * height) as usize
        {
            return false;
        }
        rasterize_impl(
            font,
            text,
            buffer,
            width,
            height,
            is_combining,
            needs_synthetic_bold,
            needs_synthetic_italic,
            false,
        )
    }

    /// Rasterizes a text cluster/character using `font` into an RGB subpixel coverage buffer
    /// (`width * height * 4`, RGBA layout with per-channel coverage in RGB and max coverage in A),
    /// top-left origin. Each of R, G, B holds the subpixel coverage for that channel, enabling
    /// LCD subpixel antialiasing when composited against the destination background.
    #[allow(clippy::too_many_arguments)]
    pub fn rasterize_to_subpixel_buffer(
        font: &CTFont,
        text: &str,
        buffer: &mut [u8],
        width: u32,
        height: u32,
        _font_size: f32,
        is_combining: bool,
        needs_synthetic_bold: bool,
        needs_synthetic_italic: bool,
    ) -> bool {
        if text.is_empty()
            || width == 0
            || height == 0
            || buffer.len() < (width * height * 4) as usize
        {
            return false;
        }
        rasterize_impl(
            font,
            text,
            buffer,
            width,
            height,
            is_combining,
            needs_synthetic_bold,
            needs_synthetic_italic,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn rasterize_impl(
        font: &CTFont,
        text: &str,
        buffer: &mut [u8],
        width: u32,
        height: u32,
        is_combining: bool,
        needs_synthetic_bold: bool,
        needs_synthetic_italic: bool,
        subpixel: bool,
    ) -> bool {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let cf_text = CFString::from_str(text);
            let font_attr_ptr = unsafe { kCTFontAttributeName as *const CFString as *const c_void };
            let font_ptr = font as *const CTFont as *const c_void;

            let mut keys = [font_attr_ptr];
            let mut values = [font_ptr];
            let attr_dict = unsafe {
                CFDictionary::new(
                    None,
                    keys.as_mut_ptr(),
                    values.as_mut_ptr(),
                    1,
                    &kCFTypeDictionaryKeyCallBacks,
                    &kCFTypeDictionaryValueCallBacks,
                )?
            };

            let cf_str_ptr = CFRetained::as_ptr(&cf_text).as_ptr() as *const CFString;
            let attr_dict_ptr = CFRetained::as_ptr(&attr_dict).as_ptr() as *const c_void;
            let attr_str =
                unsafe { CFAttributedStringCreate(ptr::null(), cf_str_ptr, attr_dict_ptr) };
            if attr_str.is_null() {
                return None;
            }

            let line = unsafe { CTLineCreateWithAttributedString(attr_str) };

            let mut ascent: CGFloat = 0.0;
            let mut descent: CGFloat = 0.0;
            let mut leading: CGFloat = 0.0;
            let advance = unsafe {
                CTLineGetTypographicBounds(&line, &mut ascent, &mut descent, &mut leading)
            };

            let font_height = (ascent + descent).max(1.0);
            let v_offset = (((height as f64) - font_height) / 2.0).floor();
            let baseline_y = (descent + v_offset.max(0.0)).round();

            let pos_x = if is_combining {
                0.0
            } else if advance > 0.0 && advance < (width as f64) {
                (((width as f64) - advance) / 2.0).floor()
            } else {
                0.0
            };

            let color_space = CGColorSpace::new_device_rgb()?;
            let bytes_per_row = (width as usize) * 4;
            let total_bytes = bytes_per_row * (height as usize);
            let mut rgba_buffer = vec![0u8; total_bytes];

            let context_raw = unsafe {
                CGBitmapContextCreate(
                    rgba_buffer.as_mut_ptr() as *mut c_void,
                    width as usize,
                    height as usize,
                    8,
                    bytes_per_row,
                    CFRetained::as_ptr(&color_space).as_ptr(),
                    BITMAP_INFO_RGBA,
                )
            };

            if context_raw.is_null() {
                unsafe { CFRelease(attr_str) };
                return None;
            }

            unsafe {
                CGContextSetAllowsFontSmoothing(context_raw, true);
                CGContextSetShouldSmoothFonts(context_raw, false);
                CGContextSetAllowsFontSubpixelPositioning(context_raw, true);
                CGContextSetShouldSubpixelPositionFonts(context_raw, true);
                CGContextSetAllowsFontSubpixelQuantization(context_raw, false);
                CGContextSetShouldSubpixelQuantizeFonts(context_raw, false);
                CGContextSetAllowsAntialiasing(context_raw, true);
                CGContextSetShouldAntialias(context_raw, true);
                CGContextSetRGBFillColor(context_raw, 1.0, 1.0, 1.0, 1.0);
                CGContextSetTextPosition(context_raw, pos_x, baseline_y);
                CTLineDraw(&line, context_raw);
                CGContextRelease(context_raw);
                CFRelease(attr_str);
            }

            let mut any_ink = false;
            let w = width as usize;
            let h = height as usize;

            for y in 0..h {
                let src_y = y;
                let italic_offset = if needs_synthetic_italic {
                    let shift = ((h.saturating_sub(1 + y)) * w) / ((3 * h).max(1));
                    shift.min(w / 3) as i32
                } else {
                    0
                };

                for x in 0..w {
                    let src_idx = (src_y * w + x) * 4;
                    let alpha = rgba_buffer[src_idx + 3].max(rgba_buffer[src_idx]);
                    if alpha == 0 {
                        continue;
                    }
                    any_ink = true;

                    let px = (x as i32) + italic_offset;
                    if px >= 0 && (px as usize) < w {
                        if subpixel {
                            // RGBA subpixel coverage: R, G, B each hold per-channel coverage, A holds max coverage.
                            let dst_idx = (y * w + (px as usize)) * 4;
                            let r = rgba_buffer[src_idx];
                            let g = rgba_buffer[src_idx + 1];
                            let b = rgba_buffer[src_idx + 2];
                            let a = rgba_buffer[src_idx + 3];
                            if is_combining {
                                buffer[dst_idx] = buffer[dst_idx].saturating_add(r);
                                buffer[dst_idx + 1] = buffer[dst_idx + 1].saturating_add(g);
                                buffer[dst_idx + 2] = buffer[dst_idx + 2].saturating_add(b);
                                buffer[dst_idx + 3] = buffer[dst_idx + 3].saturating_add(a);
                            } else {
                                buffer[dst_idx] = buffer[dst_idx].max(r);
                                buffer[dst_idx + 1] = buffer[dst_idx + 1].max(g);
                                buffer[dst_idx + 2] = buffer[dst_idx + 2].max(b);
                                buffer[dst_idx + 3] = buffer[dst_idx + 3].max(a);
                            }

                            if needs_synthetic_bold && (px + 1) < (w as i32) {
                                let bold_idx = (y * w + ((px + 1) as usize)) * 4;
                                buffer[bold_idx] = buffer[bold_idx].saturating_add(r);
                                buffer[bold_idx + 1] = buffer[bold_idx + 1].saturating_add(g);
                                buffer[bold_idx + 2] = buffer[bold_idx + 2].saturating_add(b);
                                buffer[bold_idx + 3] = buffer[bold_idx + 3].saturating_add(a);
                            }
                        } else {
                            // 8-bit alpha mask (grayscale).
                            let dst_idx = y * w + (px as usize);
                            if is_combining {
                                buffer[dst_idx] = buffer[dst_idx].saturating_add(alpha);
                            } else {
                                buffer[dst_idx] = buffer[dst_idx].max(alpha);
                            }

                            if needs_synthetic_bold && (px + 1) < (w as i32) {
                                let bold_idx = y * w + ((px + 1) as usize);
                                buffer[bold_idx] = buffer[bold_idx].saturating_add(alpha);
                            }
                        }
                    }
                }
            }

            Some(any_ink)
        }));

        match result {
            Ok(Some(ink)) => ink,
            Ok(None) => false,
            Err(_) => {
                log_error_once("panic caught during CoreText alpha rasterization");
                false
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub mod macos {}
