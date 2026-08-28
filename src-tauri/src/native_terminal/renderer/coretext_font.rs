//! CoreText font resolution, variable font instantiation, and metrics derivation (macOS).

#[cfg(target_os = "macos")]
pub mod macos {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};

    use objc2_core_foundation::{
        kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks, CFRange, CFRetained,
        CFString, CGFloat,
    };
    use objc2_core_text::{CTFont, CTFontSymbolicTraits};

    use crate::native_terminal::composition::CellMetrics;
    use crate::terminal::preferences::DEFAULT_TERMINAL_FONT_SIZE;

    #[repr(C)]
    pub struct CGSize {
        pub width: CGFloat,
        pub height: CGFloat,
    }

    pub type CGGlyph = u16;
    pub type UniChar = u16;

    extern "C" {
        pub fn CTFontGetAscent(font: &CTFont) -> CGFloat;
        pub fn CTFontGetDescent(font: &CTFont) -> CGFloat;
        pub fn CTFontGetLeading(font: &CTFont) -> CGFloat;
        pub fn CTFontGetUnitsPerEm(font: &CTFont) -> u32;
        pub fn CTFontGetGlyphsForCharacters(
            font: &CTFont,
            characters: *const UniChar,
            glyphs: *mut CGGlyph,
            count: isize,
        ) -> bool;
        pub fn CTFontGetAdvancesForGlyphs(
            font: &CTFont,
            orientation: u32,
            glyphs: *const CGGlyph,
            advances: *mut CGSize,
            count: isize,
        ) -> f64;
        pub fn CTFontCopyVariationAxes(font: &CTFont) -> *const c_void;
        pub fn CTFontCopyVariation(font: &CTFont) -> *const c_void;
        pub fn CTFontCopyPostScriptName(font: &CTFont) -> CFRetained<CFString>;
        pub fn CTFontCreateCopyWithAttributes(
            font: &CTFont,
            size: CGFloat,
            matrix: *const c_void,
            attributes: *const c_void,
        ) -> CFRetained<CTFont>;
        pub fn CTFontCreateCopyWithSymbolicTraits(
            font: &CTFont,
            size: CGFloat,
            matrix: *const c_void,
            symTraitValue: u32,
            symTraitMask: u32,
        ) -> *const CTFont;
        pub fn CTFontDescriptorCreateWithAttributes(attributes: *const c_void) -> *const c_void;

        pub fn CFRelease(cf: *const c_void);
        pub fn CFArrayGetCount(theArray: *const c_void) -> isize;
        pub fn CFArrayGetValueAtIndex(theArray: *const c_void, idx: isize) -> *const c_void;
        pub fn CFDictionaryGetValue(theDict: *const c_void, key: *const c_void) -> *const c_void;
        pub fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            numValues: isize,
            keyCallBacks: *const c_void,
            valueCallBacks: *const c_void,
        ) -> *const c_void;
        pub fn CFNumberCreate(
            allocator: *const c_void,
            theType: isize,
            valuePtr: *const c_void,
        ) -> *const c_void;
        pub fn CFNumberGetValue(
            number: *const c_void,
            theType: isize,
            valuePtr: *mut c_void,
        ) -> bool;

        pub static kCTFontVariationAxisIdentifierKey: *const c_void;
        pub static kCTFontVariationAxisMinimumValueKey: *const c_void;
        pub static kCTFontVariationAxisMaximumValueKey: *const c_void;
        pub static kCTFontVariationAttribute: *const c_void;
    }

    const K_CF_NUMBER_SINT64_TYPE: isize = 4;
    const K_CF_NUMBER_DOUBLE_TYPE: isize = 13;
    const WGHT_TAG: i64 = 0x77676874; // 'wght' = 2003265652

    static LOGGED_ERROR: AtomicBool = AtomicBool::new(false);

    fn log_error_once(msg: &str) {
        if !LOGGED_ERROR.swap(true, Ordering::Relaxed) {
            tracing::warn!("CoreText font system error: {msg}");
        }
    }

    /// Checks if the given font has a glyph mapping for character `ch`.
    pub fn font_has_glyph(font: &CTFont, ch: char) -> bool {
        let mut buf = [0u16; 2];
        let utf16 = ch.encode_utf16(&mut buf);
        let mut glyphs = [0u16; 2];
        let ok = unsafe {
            CTFontGetGlyphsForCharacters(
                font,
                utf16.as_ptr(),
                glyphs.as_mut_ptr(),
                utf16.len() as isize,
            )
        };
        ok && glyphs[0] != 0
    }

    /// Applies symbolic traits (bold, italic) to a CTFont.
    pub fn apply_traits(
        font: &CTFont,
        font_size: f64,
        bold: bool,
        italic: bool,
    ) -> CFRetained<CTFont> {
        let mut trait_value = 0u32;
        let mut trait_mask = 0u32;
        if bold {
            trait_value |= CTFontSymbolicTraits::TraitBold.0;
            trait_mask |= CTFontSymbolicTraits::TraitBold.0;
        }
        if italic {
            trait_value |= CTFontSymbolicTraits::TraitItalic.0;
            trait_mask |= CTFontSymbolicTraits::TraitItalic.0;
        }
        if trait_mask == 0 {
            return unsafe { CFRetained::retain(font.into()) };
        }

        let copy_ptr = unsafe {
            CTFontCreateCopyWithSymbolicTraits(
                font,
                font_size,
                ptr::null(),
                trait_value,
                trait_mask,
            )
        };
        if let Some(retained_ptr) = ptr::NonNull::new(copy_ptr as *mut CTFont) {
            unsafe { CFRetained::from_raw(retained_ptr) }
        } else {
            unsafe { CFRetained::retain(font.into()) }
        }
    }

    /// Inspects the variation axes of `font`, and if a `wght` axis is present,
    /// creates a font copy instantiated with `kCTFontVariationAttribute` = { "wght": target_weight }.
    pub fn apply_variable_weight(
        font: &CTFont,
        font_size: f64,
        target_weight: f64,
    ) -> CFRetained<CTFont> {
        let axes = unsafe { CTFontCopyVariationAxes(font) };
        if axes.is_null() {
            return unsafe { CFRetained::retain(font.into()) };
        }

        let mut wght_axis_id_val: Option<*const c_void> = None;
        let mut min_weight: f64 = 100.0;
        let mut max_weight: f64 = 900.0;

        let count = unsafe { CFArrayGetCount(axes) };
        for i in 0..count {
            let dict = unsafe { CFArrayGetValueAtIndex(axes, i) };
            if dict.is_null() {
                continue;
            }
            let id_val = unsafe { CFDictionaryGetValue(dict, kCTFontVariationAxisIdentifierKey) };
            if id_val.is_null() {
                continue;
            }
            let mut id_i64: i64 = 0;
            let ok = unsafe {
                CFNumberGetValue(
                    id_val,
                    K_CF_NUMBER_SINT64_TYPE,
                    &mut id_i64 as *mut _ as *mut c_void,
                )
            };
            if ok && id_i64 == WGHT_TAG {
                wght_axis_id_val = Some(id_val);
                let min_val =
                    unsafe { CFDictionaryGetValue(dict, kCTFontVariationAxisMinimumValueKey) };
                let max_val =
                    unsafe { CFDictionaryGetValue(dict, kCTFontVariationAxisMaximumValueKey) };
                if !min_val.is_null() {
                    let _ = unsafe {
                        CFNumberGetValue(
                            min_val,
                            K_CF_NUMBER_DOUBLE_TYPE,
                            &mut min_weight as *mut _ as *mut c_void,
                        )
                    };
                }
                if !max_val.is_null() {
                    let _ = unsafe {
                        CFNumberGetValue(
                            max_val,
                            K_CF_NUMBER_DOUBLE_TYPE,
                            &mut max_weight as *mut _ as *mut c_void,
                        )
                    };
                }
                break;
            }
        }

        let Some(id_val) = wght_axis_id_val else {
            unsafe { CFRelease(axes) };
            return unsafe { CFRetained::retain(font.into()) };
        };

        let clamped_weight = target_weight.clamp(min_weight, max_weight);
        let target_num = unsafe {
            CFNumberCreate(
                ptr::null(),
                K_CF_NUMBER_DOUBLE_TYPE,
                &clamped_weight as *const _ as *const c_void,
            )
        };

        let result = if !target_num.is_null() {
            let var_dict = unsafe {
                let keys = [id_val];
                let values = [target_num];
                CFDictionaryCreate(
                    ptr::null(),
                    keys.as_ptr(),
                    values.as_ptr(),
                    1,
                    &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
                    &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
                )
            };
            if !var_dict.is_null() {
                let attr_dict = unsafe {
                    let keys = [kCTFontVariationAttribute];
                    let values = [var_dict];
                    CFDictionaryCreate(
                        ptr::null(),
                        keys.as_ptr(),
                        values.as_ptr(),
                        1,
                        &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
                        &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
                    )
                };
                if !attr_dict.is_null() {
                    let desc = unsafe { CTFontDescriptorCreateWithAttributes(attr_dict) };
                    let copy = if !desc.is_null() {
                        let f = unsafe {
                            CTFontCreateCopyWithAttributes(font, font_size, ptr::null(), desc)
                        };
                        unsafe { CFRelease(desc) };
                        f
                    } else {
                        unsafe { CFRetained::retain(font.into()) }
                    };
                    unsafe {
                        CFRelease(attr_dict);
                        CFRelease(var_dict);
                        CFRelease(target_num);
                    }
                    copy
                } else {
                    unsafe {
                        CFRelease(var_dict);
                        CFRelease(target_num);
                        CFRetained::retain(font.into())
                    }
                }
            } else {
                unsafe {
                    CFRelease(target_num);
                    CFRetained::retain(font.into())
                }
            }
        } else {
            unsafe { CFRetained::retain(font.into()) }
        };

        unsafe { CFRelease(axes) };
        result
    }

    /// Creates a styled `CTFont` with symbolic traits and variable font weight applied.
    pub fn create_styled_font(
        family_name: &str,
        font_size: f64,
        bold: bool,
        italic: bool,
    ) -> CFRetained<CTFont> {
        let cf_name = CFString::from_str(family_name);
        let base = unsafe { CTFont::with_name(&cf_name, font_size, ptr::null()) };
        let with_traits = apply_traits(&base, font_size, bold, italic);
        let target_weight = if bold { 700.0 } else { 400.0 };
        apply_variable_weight(&with_traits, font_size, target_weight)
    }

    /// Full CoreText font subsystem holding primary styled fonts and a per-character fallback cache.
    pub struct CoreTextFontSystem {
        pub font_family: String,
        pub font_size: f32,
        pub family_names: Vec<String>,
        pub primary_regular: CFRetained<CTFont>,
        pub primary_bold: CFRetained<CTFont>,
        pub primary_italic: CFRetained<CTFont>,
        pub primary_bold_italic: CFRetained<CTFont>,
        pub configured_fallbacks: Vec<String>,
        pub glyph_font_cache: HashMap<(char, bool, bool), Option<CFRetained<CTFont>>>,
    }

    unsafe impl Send for CoreTextFontSystem {}
    unsafe impl Sync for CoreTextFontSystem {}

    impl CoreTextFontSystem {
        pub fn new(font_family: &str, font_size: f32) -> Self {
            let font_size = if font_size > 0.0 {
                font_size
            } else {
                DEFAULT_TERMINAL_FONT_SIZE
            };
            let family_names: Vec<String> = font_family
                .split(',')
                .map(|part| {
                    part.trim()
                        .trim_matches('\"')
                        .trim_matches('\'')
                        .trim()
                        .to_string()
                })
                .filter(|name| !name.is_empty())
                .collect();

            let primary_name = family_names
                .first()
                .cloned()
                .unwrap_or_else(|| "Menlo".to_string());
            let configured_fallbacks = family_names.iter().skip(1).cloned().collect();

            let fs = font_size as f64;
            let primary_regular = create_styled_font(&primary_name, fs, false, false);
            let primary_bold = create_styled_font(&primary_name, fs, true, false);
            let primary_italic = create_styled_font(&primary_name, fs, false, true);
            let primary_bold_italic = create_styled_font(&primary_name, fs, true, true);

            Self {
                font_family: font_family.to_string(),
                font_size,
                family_names,
                primary_regular,
                primary_bold,
                primary_italic,
                primary_bold_italic,
                configured_fallbacks,
                glyph_font_cache: HashMap::new(),
            }
        }

        pub fn set_font_size(&mut self, font_size: f32) {
            if !(font_size.is_finite() && font_size > 0.0)
                || (self.font_size - font_size).abs() < 1e-4
            {
                return;
            }
            *self = Self::new(&self.font_family, font_size);
        }

        pub fn cell_metrics_for_scale(&self, scale_factor: f32) -> CellMetrics {
            let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
                scale_factor
            } else {
                1.0
            };
            let effective_size = (self.font_size * scale) as f64;

            let font = if (scale - 1.0).abs() < 1e-4 {
                self.primary_regular.clone()
            } else {
                let primary_name = self
                    .family_names
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "Menlo".to_string());
                create_styled_font(&primary_name, effective_size, false, false)
            };

            let ascent = unsafe { CTFontGetAscent(&font) };
            let descent = unsafe { CTFontGetDescent(&font) };
            let leading = unsafe { CTFontGetLeading(&font) };

            let raw_height = ascent + descent + leading;
            let height_px = raw_height.round().max(1.0) as u32;

            let mut glyph: CGGlyph = 0;
            let unichar: UniChar = 'M' as u16;
            let has_glyph = unsafe { CTFontGetGlyphsForCharacters(&font, &unichar, &mut glyph, 1) };
            let width_px = if has_glyph && glyph != 0 {
                let mut advance = CGSize {
                    width: 0.0,
                    height: 0.0,
                };
                unsafe { CTFontGetAdvancesForGlyphs(&font, 0, &glyph, &mut advance, 1) };
                if advance.width > 0.0 {
                    advance.width.round().max(1.0) as u32
                } else {
                    (effective_size * 0.6).round().max(1.0) as u32
                }
            } else {
                (effective_size * 0.6).round().max(1.0) as u32
            };

            CellMetrics {
                width_px,
                height_px,
            }
        }

        pub fn resolve_font_for_char(
            &mut self,
            ch: char,
            bold: bool,
            italic: bool,
            scale_factor: f32,
        ) -> Option<CFRetained<CTFont>> {
            let key = (ch, bold, italic);
            let effective_size = ((self.font_size * scale_factor).max(1.0)) as f64;

            if (scale_factor - 1.0).abs() < 1e-4 {
                if let Some(cached) = self.glyph_font_cache.get(&key) {
                    return cached.clone();
                }
            }

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Step 1: Check primary faces based on requested style
                let primary_candidates: [&CFRetained<CTFont>; 4] = match (bold, italic) {
                    (true, true) => [
                        &self.primary_bold_italic,
                        &self.primary_bold,
                        &self.primary_italic,
                        &self.primary_regular,
                    ],
                    (true, false) => [
                        &self.primary_bold,
                        &self.primary_regular,
                        &self.primary_regular,
                        &self.primary_regular,
                    ],
                    (false, true) => [
                        &self.primary_italic,
                        &self.primary_regular,
                        &self.primary_regular,
                        &self.primary_regular,
                    ],
                    (false, false) => [
                        &self.primary_regular,
                        &self.primary_regular,
                        &self.primary_regular,
                        &self.primary_regular,
                    ],
                };

                for candidate in primary_candidates {
                    if font_has_glyph(candidate, ch) {
                        if (scale_factor - 1.0).abs() < 1e-4 {
                            return Some(candidate.clone());
                        } else {
                            let ps_name =
                                unsafe { CTFontCopyPostScriptName(candidate) }.to_string();
                            let target_weight = if bold { 700.0 } else { 400.0 };
                            let scaled_base = unsafe {
                                let cf_name = CFString::from_str(&ps_name);
                                CTFont::with_name(&cf_name, effective_size, ptr::null())
                            };
                            return Some(apply_variable_weight(
                                &scaled_base,
                                effective_size,
                                target_weight,
                            ));
                        }
                    }
                }

                // Step 2: User's remaining configured fallback families
                for family_name in &self.configured_fallbacks {
                    let candidate = create_styled_font(family_name, effective_size, bold, italic);
                    if font_has_glyph(&candidate, ch) {
                        return Some(candidate);
                    }
                }

                // Step 3: CoreText cascade-guided fallback
                let mut buf = [0u16; 2];
                let utf16 = ch.encode_utf16(&mut buf);
                let cf_str = CFString::from_str(&ch.to_string());
                let range = CFRange {
                    location: 0,
                    length: utf16.len() as isize,
                };
                let base_font = match (bold, italic) {
                    (true, true) => &self.primary_bold_italic,
                    (true, false) => &self.primary_bold,
                    (false, true) => &self.primary_italic,
                    (false, false) => &self.primary_regular,
                };
                let fallback = unsafe { base_font.for_string(&cf_str, range) };
                let ps_name = unsafe { CTFontCopyPostScriptName(&fallback) }.to_string();

                if !ps_name.is_empty() && ps_name != "LastResort" && font_has_glyph(&fallback, ch) {
                    let with_traits = apply_traits(&fallback, effective_size, bold, italic);
                    let target_weight = if bold { 700.0 } else { 400.0 };
                    let styled_fallback =
                        apply_variable_weight(&with_traits, effective_size, target_weight);
                    return Some(styled_fallback);
                }

                None
            }));

            match result {
                Ok(opt) => {
                    if (scale_factor - 1.0).abs() < 1e-4 {
                        self.glyph_font_cache.insert(key, opt.clone());
                    }
                    opt
                }
                Err(_) => {
                    log_error_once("panic during CoreText glyph font resolution");
                    None
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub mod macos {}
