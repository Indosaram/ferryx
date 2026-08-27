//! CoreText cascade-guided font fallback resolver for macOS.
//!
//! Resolves missing glyphs using macOS system CoreText fallback cascade list
//! to select standard, high-quality system fallbacks matching requested family and traits.

#[cfg(target_os = "macos")]
pub mod macos {
    use std::path::{Path, PathBuf};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};

    use fontdb::{Database, ID};
    use objc2_core_foundation::{CFRange, CFRetained, CFString, CFURL};
    use objc2_core_text::{
        kCTFontURLAttribute, CTFont, CTFontSymbolicTraits,
    };

    static LOGGED_ERROR: AtomicBool = AtomicBool::new(false);

    fn log_error_once(msg: &str) {
        if !LOGGED_ERROR.swap(true, Ordering::Relaxed) {
            tracing::warn!("CoreText font fallback resolution error: {msg}");
        }
    }

    /// Resolves the best system fallback face ID for character `ch` using CoreText cascade.
    pub fn resolve_coretext_fallback(
        db: &mut Database,
        family_names: &[String],
        ch: char,
        bold: bool,
        italic: bool,
    ) -> Option<ID> {
        if db.faces().next().is_none() {
            return None;
        }

        let (ps_name, file_url) = query_coretext(family_names, ch, bold, italic)?;

        // 1. Try to find matching face in db by PostScript name
        if let Some(id) = find_face_in_db(db, &ps_name, ch) {
            return Some(id);
        }

        // 2. If not found in db but URL exists on disk, load font into db and check again
        if let Some(path) = file_url {
            if path.exists() && db.load_font_file(&path).is_ok() {
                if let Some(id) = find_face_in_db(db, &ps_name, ch) {
                    return Some(id);
                }
            }
        }

        None
    }

    fn query_coretext(
        family_names: &[String],
        ch: char,
        bold: bool,
        italic: bool,
    ) -> Option<(String, Option<PathBuf>)> {
        let result = std::panic::catch_unwind(|| {
            let mut char_buf = [0u16; 2];
            let utf16_slice = ch.encode_utf16(&mut char_buf);
            let utf16_len = utf16_slice.len();

            let cf_str = CFString::from_str(&ch.to_string());
            let range = CFRange {
                location: 0,
                length: utf16_len as isize,
            };

            let base_font = create_base_font(family_names, bold, italic);
            let fallback_font = unsafe { base_font.for_string(&cf_str, range) };
            let ps_name_cf = unsafe { fallback_font.post_script_name() };
            let ps_name = ps_name_cf.to_string();

            if ps_name.is_empty() {
                return None;
            }

            let file_url = extract_font_path(&fallback_font);
            Some((ps_name, file_url))
        });

        match result {
            Ok(opt) => opt,
            Err(_) => {
                log_error_once("panic during CoreText fallback query");
                None
            }
        }
    }

    fn create_base_font(family_names: &[String], bold: bool, italic: bool) -> CFRetained<CTFont> {
        let font_size = 12.0;

        if let Some(name) = family_names.first() {
            let cf_name = CFString::from_str(name);
            let font = unsafe { CTFont::with_name(&cf_name, font_size, ptr::null()) };
            apply_style(&font, font_size, bold, italic)
        } else {
            let cf_default = CFString::from_str("Menlo");
            let font = unsafe { CTFont::with_name(&cf_default, font_size, ptr::null()) };
            apply_style(&font, font_size, bold, italic)
        }
    }

    fn apply_style(font: &CTFont, font_size: f64, bold: bool, italic: bool) -> CFRetained<CTFont> {
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

        if trait_mask != 0 {
            if let Some(styled) = unsafe {
                font.copy_with_symbolic_traits(
                    font_size,
                    ptr::null(),
                    CTFontSymbolicTraits(trait_value),
                    CTFontSymbolicTraits(trait_mask),
                )
            } {
                return styled;
            }
        }

        unsafe { CTFont::with_name(&font.post_script_name(), font_size, ptr::null()) }
    }

    fn extract_font_path(font: &CTFont) -> Option<PathBuf> {
        unsafe {
            let desc = font.font_descriptor();
            let attr = desc.attribute(kCTFontURLAttribute)?;
            let raw_ptr = CFRetained::as_ptr(&attr).as_ptr() as *const CFURL;
            let cf_url = &*raw_ptr;
            let path_cf = cf_url.path()?;
            let path_str = path_cf.to_string();
            if path_str.is_empty() {
                None
            } else {
                Some(Path::new(&path_str).to_path_buf())
            }
        }
    }

    fn find_face_in_db(db: &Database, ps_name: &str, _ch: char) -> Option<ID> {
        let ps_lower = ps_name.to_lowercase();

        // 1. Exact match (case-insensitive)
        for face in db.faces() {
            if face.post_script_name.eq_ignore_ascii_case(ps_name) {
                return Some(face.id);
            }
        }

        // 2. Prefix match (e.g. family or variant prefix)
        for face in db.faces() {
            let face_ps_lower = face.post_script_name.to_lowercase();
            if face_ps_lower.starts_with(&ps_lower) || ps_lower.starts_with(&face_ps_lower) {
                return Some(face.id);
            }
        }

        None
    }
}

#[cfg(not(target_os = "macos"))]
pub mod non_macos {
    use fontdb::{Database, ID};

    pub fn resolve_coretext_fallback(
        _db: &mut Database,
        _family_names: &[String],
        _ch: char,
        _bold: bool,
        _italic: bool,
    ) -> Option<ID> {
        None
    }
}

#[cfg(target_os = "macos")]
pub use macos::resolve_coretext_fallback;

#[cfg(not(target_os = "macos"))]
pub use non_macos::resolve_coretext_fallback;

#[cfg(test)]
mod tests {
    use super::*;
    use fontdb::Database;

    #[test]
    #[cfg(target_os = "macos")]
    fn test_coretext_fallback_korean() {
        let mut db = Database::new();
        db.load_system_fonts();

        let families = vec!["MesloLGS NF".to_string()];
        let resolved_id = resolve_coretext_fallback(&mut db, &families, '한', false, false);
        assert!(
            resolved_id.is_some(),
            "CoreText fallback must resolve Korean char '한'"
        );

        let face_id = resolved_id.unwrap();
        let face = db.face(face_id).expect("resolved face info in db");
        let ps_name = &face.post_script_name;

        assert!(
            ps_name.contains("AppleSDGothicNeo"),
            "resolved PostScript name '{ps_name}' must contain AppleSDGothicNeo"
        );
        assert!(
            !ps_name.ends_with("-Light") && !ps_name.contains("-Light"),
            "resolved PostScript name '{ps_name}' must NEVER be -Light variant"
        );
        assert!(
            ps_name.contains("Regular") || ps_name.contains("Medium") || ps_name.contains("Bold"),
            "resolved PostScript name '{ps_name}' must be Regular/Medium/Bold variant"
        );
    }
}

