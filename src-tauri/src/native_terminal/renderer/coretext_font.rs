//! CoreText font resolution, variable font instantiation, and metrics derivation (macOS).

#[cfg(target_os = "macos")]
pub mod macos {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

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
        pub fn CTFontCopyFamilyName(font: &CTFont) -> CFRetained<CFString>;
        pub fn CTFontCopyFullName(font: &CTFont) -> CFRetained<CFString>;
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

    /// Fixed-pitch family that ships with every macOS install. Used whenever nothing the user
    /// configured is actually installed, so the grid is never measured from a proportional face.
    pub const PLATFORM_MONOSPACE_FAMILY: &str = "Menlo";

    static LOGGED_MISSING_FAMILIES: Mutex<Option<String>> = Mutex::new(None);

    /// `new()` re-runs on every font-size change, so only log a given set of missing families once.
    fn log_missing_families_once(missing: &[String]) {
        let joined = missing.join(", ");
        let mut last_logged = match LOGGED_MISSING_FAMILIES.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if last_logged.as_deref() != Some(joined.as_str()) {
            tracing::warn!(
                "CoreText: configured font families are not installed and were skipped: {joined}"
            );
            *last_logged = Some(joined);
        }
    }

    /// CSS-style generic keywords that reach us from the preference layer: the Rust default is
    /// `"monospace"` and the Ghostty importer appends it. CoreText knows nothing about these
    /// keywords (it hands back Helvetica for them), so they must map to a concrete family.
    fn is_generic_monospace_keyword(name: &str) -> bool {
        name.eq_ignore_ascii_case("monospace") || name.eq_ignore_ascii_case("ui-monospace")
    }

    /// `CTFontCreateWithName` never fails: for a name that is not installed CoreText silently
    /// substitutes the system default (Helvetica, proportional). Returns true only when the font
    /// really is the one asked for — by family, full or PostScript name, case-insensitively.
    pub fn font_matches_requested_name(font: &CTFont, requested: &str) -> bool {
        let requested = requested.trim();
        if requested.is_empty() {
            return false;
        }
        let names = unsafe {
            [
                CTFontCopyFamilyName(font).to_string(),
                CTFontCopyFullName(font).to_string(),
                CTFontCopyPostScriptName(font).to_string(),
            ]
        };
        names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(requested))
    }

    /// Keeps only the configured families CoreText can actually supply on this machine, mapping
    /// generic keywords to [`PLATFORM_MONOSPACE_FAMILY`] and dropping duplicates.
    ///
    /// Returns `(primary, remaining installed fallbacks)`. Missing families are excluded from the
    /// fallback list too: left in, they would resolve to Helvetica in `resolve_font_for_char`
    /// step 2 and hijack any glyph the primary face lacks.
    pub fn resolve_installed_families(
        family_names: &[String],
        font_size: f64,
    ) -> (String, Vec<String>) {
        let mut installed: Vec<String> = Vec::new();
        let mut missing: Vec<String> = Vec::new();
        for name in family_names {
            let candidate = if is_generic_monospace_keyword(name) {
                PLATFORM_MONOSPACE_FAMILY.to_string()
            } else {
                name.clone()
            };
            let cf_name = CFString::from_str(&candidate);
            let font = unsafe { CTFont::with_name(&cf_name, font_size, ptr::null()) };
            if font_matches_requested_name(&font, &candidate) {
                if !installed
                    .iter()
                    .any(|seen| seen.eq_ignore_ascii_case(&candidate))
                {
                    installed.push(candidate);
                }
            } else {
                missing.push(name.clone());
            }
        }
        if !missing.is_empty() {
            log_missing_families_once(&missing);
        }

        let mut installed = installed.into_iter();
        let primary = installed
            .next()
            .unwrap_or_else(|| PLATFORM_MONOSPACE_FAMILY.to_string());
        (primary, installed.collect())
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
        /// First configured family CoreText can actually supply here (else
        /// [`PLATFORM_MONOSPACE_FAMILY`]); the cell grid is measured from this face.
        pub resolved_primary_name: String,
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

            let fs = font_size as f64;
            let (primary_name, configured_fallbacks) =
                resolve_installed_families(&family_names, fs);
            let primary_regular = create_styled_font(&primary_name, fs, false, false);
            let primary_bold = create_styled_font(&primary_name, fs, true, false);
            let primary_italic = create_styled_font(&primary_name, fs, false, true);
            let primary_bold_italic = create_styled_font(&primary_name, fs, true, true);

            Self {
                font_family: font_family.to_string(),
                font_size,
                family_names,
                resolved_primary_name: primary_name,
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
                create_styled_font(&self.resolved_primary_name, effective_size, false, false)
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

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A family name no machine has installed, so CoreText substitutes Helvetica for it.
        const MISSING_FAMILY: &str = "Ferryx Definitely Not Installed 7f3a";

        fn font_named(name: &str) -> CFRetained<CTFont> {
            let cf_name = CFString::from_str(name);
            unsafe { CTFont::with_name(&cf_name, 13.0, ptr::null()) }
        }

        fn advance_of(font: &CTFont, ch: char) -> f64 {
            let unichar: UniChar = ch as u16;
            let mut glyph: CGGlyph = 0;
            assert!(
                unsafe { CTFontGetGlyphsForCharacters(font, &unichar, &mut glyph, 1) },
                "font has no glyph for {ch:?}"
            );
            let mut advance = CGSize {
                width: 0.0,
                height: 0.0,
            };
            unsafe { CTFontGetAdvancesForGlyphs(font, 0, &glyph, &mut advance, 1) };
            advance.width
        }

        #[test]
        fn detects_coretexts_silent_substitution_for_unknown_names() {
            assert!(!font_matches_requested_name(
                &font_named(MISSING_FAMILY),
                MISSING_FAMILY
            ));

            let menlo = font_named("Menlo");
            assert!(font_matches_requested_name(&menlo, "Menlo"));
            assert!(font_matches_requested_name(&menlo, "menlo"));
            assert!(font_matches_requested_name(&menlo, "Menlo-Regular"));
            assert!(!font_matches_requested_name(&menlo, "Monaco"));
            assert!(!font_matches_requested_name(&menlo, ""));
        }

        #[test]
        fn missing_primary_is_skipped_in_favour_of_the_first_installed_family() {
            let system = CoreTextFontSystem::new(&format!("{MISSING_FAMILY}, Menlo, Monaco"), 13.0);

            assert_eq!(system.resolved_primary_name, "Menlo");
            assert_eq!(system.configured_fallbacks, vec!["Monaco".to_string()]);
            assert!(font_matches_requested_name(
                &system.primary_regular,
                "Menlo"
            ));
        }

        #[test]
        fn generic_monospace_keyword_resolves_to_a_fixed_pitch_face() {
            // "monospace" is the preference-layer default and what the Ghostty importer appends.
            let system = CoreTextFontSystem::new("monospace", 13.0);
            assert_eq!(system.resolved_primary_name, PLATFORM_MONOSPACE_FAMILY);

            let font = &system.primary_regular;
            let (i, w, m) = (
                advance_of(font, 'i'),
                advance_of(font, 'W'),
                advance_of(font, 'M'),
            );
            assert!(
                (i - w).abs() < 1e-6 && (w - m).abs() < 1e-6,
                "expected fixed pitch, got i={i} W={w} M={m}"
            );
        }

        #[test]
        fn nothing_installed_falls_back_to_the_platform_monospace_family() {
            let system = CoreTextFontSystem::new(
                &format!("{MISSING_FAMILY}, {MISSING_FAMILY} Two, monospace"),
                13.0,
            );
            assert_eq!(system.resolved_primary_name, PLATFORM_MONOSPACE_FAMILY);
            assert!(system.configured_fallbacks.is_empty());

            let empty = CoreTextFontSystem::new("", 13.0);
            assert_eq!(empty.resolved_primary_name, PLATFORM_MONOSPACE_FAMILY);
        }

        #[test]
        fn missing_families_never_reach_the_configured_fallback_tier() {
            let system = CoreTextFontSystem::new(&format!("Menlo, {MISSING_FAMILY}, Monaco"), 13.0);
            assert_eq!(system.resolved_primary_name, "Menlo");
            assert_eq!(system.configured_fallbacks, vec!["Monaco".to_string()]);
        }

        #[test]
        fn cell_metrics_come_from_the_resolved_primary_at_every_scale() {
            let via_missing =
                CoreTextFontSystem::new(&format!("{MISSING_FAMILY}, monospace"), 13.0);
            let menlo = CoreTextFontSystem::new("Menlo", 13.0);
            for scale in [1.0_f32, 2.0] {
                assert_eq!(
                    via_missing.cell_metrics_for_scale(scale),
                    menlo.cell_metrics_for_scale(scale)
                );
            }
            // Helvetica's 'M' would give an 11px cell at 13pt; Menlo's gives 8px.
            assert_eq!(menlo.cell_metrics_for_scale(1.0).width_px, 8);
        }

        #[test]
        fn hangul_still_resolves_through_the_coretext_cascade() {
            let mut system = CoreTextFontSystem::new("monospace", 13.0);
            let font = system
                .resolve_font_for_char('한', false, false, 1.0)
                .expect("CoreText cascade must supply a Hangul face");
            assert!(font_has_glyph(&font, '한'));
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub mod macos {}
