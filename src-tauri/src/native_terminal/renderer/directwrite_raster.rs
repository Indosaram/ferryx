//! GDI draws white text into a zeroed 32-bit DIB; each pixel's green channel is the
//! 8-bit coverage value wgpu needs.

use std::ffi::c_void;

type Hdc = *mut c_void;
type Hbitmap = *mut c_void;
type Hfont = *mut c_void;
type Hgdiobj = *mut c_void;

const TRANSPARENT_MODE: i32 = 1;
const DIB_RGB_COLORS: u32 = 0;
const BI_RGB: u32 = 0;
const FW_NORMAL: i32 = 400;
const FW_BOLD: i32 = 700;
const DEFAULT_CHARSET: u32 = 1;
const OUT_TT_PRECIS: u32 = 4;
const CLIP_DEFAULT_PRECIS: u32 = 0;
const ANTIALIASED_QUALITY: u32 = 4;
const FIXED_PITCH: u32 = 1;
const FF_MODERN: u32 = 48;
const GGI_MARK_NONEXISTING_GLYPHS: u32 = 0x0001;
const GDI_ERROR: u32 = 0xFFFF_FFFF;
/// OpenType table tag `cmap` as the little-endian DWORD `GetFontData` expects.
const CMAP_TAG: u32 = u32::from_le_bytes(*b"cmap");

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_pels_per_meter: i32,
    y_pels_per_meter: i32,
    clr_used: u32,
    clr_important: u32,
}

#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [u32; 3],
}

#[repr(C)]
#[allow(non_snake_case, dead_code)]
struct LogFontW {
    lfHeight: i32,
    lfWidth: i32,
    lfEscapement: i32,
    lfOrientation: i32,
    lfWeight: i32,
    lfItalic: u8,
    lfUnderline: u8,
    lfStrikeOut: u8,
    lfCharSet: u8,
    lfOutPrecision: u8,
    lfClipPrecision: u8,
    lfQuality: u8,
    lfPitchAndFamily: u8,
    lfFaceName: [u16; 32],
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(hdc: Hdc) -> Hdc;
    fn DeleteDC(hdc: Hdc) -> i32;
    fn CreateDIBSection(
        hdc: Hdc,
        info: *const BitmapInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: *mut c_void,
        offset: u32,
    ) -> Hbitmap;
    fn DeleteObject(obj: Hgdiobj) -> i32;
    fn SelectObject(hdc: Hdc, obj: Hgdiobj) -> Hgdiobj;
    fn SetBkMode(hdc: Hdc, mode: i32) -> i32;
    fn SetTextColor(hdc: Hdc, color: u32) -> u32;
    fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strikeout: u32,
        charset: u32,
        out_precision: u32,
        clip_precision: u32,
        quality: u32,
        pitch_and_family: u32,
        face: *const u16,
    ) -> Hfont;
    fn TextOutW(hdc: Hdc, x: i32, y: i32, text: *const u16, len: i32) -> i32;
    fn GdiFlush() -> i32;
    fn GetGlyphIndicesW(hdc: Hdc, lpstr: *const u16, c: i32, pgi: *mut u16, fl: u32) -> u32;
    fn GetFontData(hdc: Hdc, table: u32, offset: u32, buffer: *mut c_void, size: u32) -> u32;
    fn EnumFontFamiliesExW(
        hdc: Hdc,
        lpLogfont: *const LogFontW,
        lpProc: unsafe extern "system" fn(*const LogFontW, *const c_void, u32, isize) -> i32,
        lParam: isize,
        dwFlags: u32,
    ) -> i32;
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn clean_family_name(raw: &str) -> &str {
    raw.trim().trim_matches(|c| c == '\'' || c == '"').trim()
}

const SYSTEM_FALLBACK_FAMILIES: &[&str] = &[
    "Malgun Gothic",
    "Microsoft YaHei",
    "Yu Gothic",
    "MS Gothic",
    "Segoe UI Symbol",
    "Segoe UI Emoji",
    "Cascadia Mono",
    "Consolas",
];

fn candidate_families<'a>(family_stack: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    let user_families = family_stack.split(',').filter_map(|candidate| {
        let cleaned = clean_family_name(candidate);
        if cleaned.is_empty() {
            None
        } else if cleaned.eq_ignore_ascii_case("monospace") {
            Some("Consolas")
        } else {
            Some(cleaned)
        }
    });

    let mut seen: Vec<&'a str> = Vec::new();
    user_families
        .chain(SYSTEM_FALLBACK_FAMILIES.iter().copied())
        .filter(move |&family| {
            if seen.iter().any(|s| s.eq_ignore_ascii_case(family)) {
                false
            } else {
                seen.push(family);
                true
            }
        })
}

/// Callback for `EnumFontFamiliesExW` existence check: sets the boolean flag and stops enumeration.
unsafe extern "system" fn enum_font_fam_ex_proc(
    _lpelfe: *const LogFontW,
    _lpntme: *const c_void,
    _font_type: u32,
    l_param: isize,
) -> i32 {
    let found = l_param as *mut bool;
    if !found.is_null() {
        *found = true;
    }
    0
}

/// Returns true when a font family named `face` (NUL-terminated UTF-16, at most 32 units)
/// is installed. `EnumFontFamiliesExW` matches both English and localized family names.
///
/// SAFETY: `dc` must be a valid device context handle.
unsafe fn family_exists(dc: Hdc, face: &[u16]) -> bool {
    if face.len() > 32 {
        return false;
    }
    let mut logfont = LogFontW {
        lfHeight: 0,
        lfWidth: 0,
        lfEscapement: 0,
        lfOrientation: 0,
        lfWeight: 0,
        lfItalic: 0,
        lfUnderline: 0,
        lfStrikeOut: 0,
        lfCharSet: DEFAULT_CHARSET as u8,
        lfOutPrecision: 0,
        lfClipPrecision: 0,
        lfQuality: 0,
        lfPitchAndFamily: 0,
        lfFaceName: [0u16; 32],
    };
    logfont.lfFaceName[..face.len()].copy_from_slice(face);

    let mut found = false;
    EnumFontFamiliesExW(
        dc,
        &logfont,
        enum_font_fam_ex_proc,
        (&mut found as *mut bool) as isize,
        0,
    );
    found
}

fn be_u16(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn be_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Returns true when a full-Unicode (format 12) subtable of the `cmap` table maps
/// `codepoint` to a real glyph. `GetGlyphIndicesW` works on UTF-16 code units and reports
/// every surrogate half as missing, so supplementary-plane coverage must read the cmap.
fn cmap_format12_covers(cmap: &[u8], codepoint: u32) -> bool {
    let Some(num_tables) = be_u16(cmap, 2) else {
        return false;
    };
    for index in 0..num_tables as usize {
        let record = 4 + index * 8;
        let (Some(platform), Some(encoding), Some(offset)) = (
            be_u16(cmap, record),
            be_u16(cmap, record + 2),
            be_u32(cmap, record + 4),
        ) else {
            return false;
        };
        let full_unicode =
            (platform == 3 && encoding == 10) || (platform == 0 && (encoding == 4 || encoding == 6));
        let subtable = offset as usize;
        if !full_unicode || be_u16(cmap, subtable) != Some(12) {
            continue;
        }
        let Some(num_groups) = be_u32(cmap, subtable + 12) else {
            continue;
        };
        for group in 0..num_groups as usize {
            let base = subtable + 16 + group * 12;
            let (Some(start), Some(end), Some(start_glyph)) =
                (be_u32(cmap, base), be_u32(cmap, base + 4), be_u32(cmap, base + 8))
            else {
                break;
            };
            if (start..=end).contains(&codepoint) {
                return start_glyph.wrapping_add(codepoint - start) != 0;
            }
        }
    }
    false
}

/// Codepoint-aware coverage check for the font currently selected into `dc`.
///
/// SAFETY: `dc` must be a valid device context with a font selected.
unsafe fn selected_font_covers_codepoints(dc: Hdc, text: &[u16]) -> bool {
    let size = GetFontData(dc, CMAP_TAG, 0, std::ptr::null_mut(), 0);
    if size == GDI_ERROR || size == 0 {
        return false;
    }
    let mut cmap = vec![0u8; size as usize];
    if GetFontData(dc, CMAP_TAG, 0, cmap.as_mut_ptr().cast(), size) != size {
        return false;
    }
    char::decode_utf16(text.iter().copied())
        .all(|decoded| matches!(decoded, Ok(ch) if cmap_format12_covers(&cmap, ch as u32)))
}

/// Finds the first family in `family_stack` (or Windows system fallbacks) that actually exists
/// and has glyphs for every character of text in `glyphs`, leaving it selected into `dc`
/// and returning `(font, old_font)`.
///
/// SAFETY: `dc` must be a valid device context handle. If `Some((font, old_font))` is returned,
/// the caller must restore `old_font` via `SelectObject` and delete `font` with `DeleteObject`.
/// Any rejected candidate fonts created during enumeration are deselected and deleted before proceeding.
unsafe fn select_covering_font(
    dc: Hdc,
    family_stack: &str,
    glyphs: &[u16],
    glyph_len: i32,
    font_size: f32,
    bold: bool,
    italic: bool,
) -> Option<(Hfont, Hgdiobj)> {
    let effective_size = if font_size.is_finite() && font_size > 0.0 {
        font_size
    } else {
        14.0
    };

    for resolved_family in candidate_families(family_stack) {
        let face = wide(resolved_family);
        if !family_exists(dc, &face) {
            continue;
        }

        let font = CreateFontW(
            -(effective_size.round() as i32),
            0,
            0,
            0,
            if bold { FW_BOLD } else { FW_NORMAL },
            u32::from(italic),
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            FIXED_PITCH | FF_MODERN,
            face.as_ptr(),
        );
        if font.is_null() {
            continue;
        }

        let old_font = SelectObject(dc, font);

        let text_units = &glyphs[..glyph_len as usize];
        let all_covered = if text_units.iter().any(|unit| (0xD800..=0xDFFF).contains(unit)) {
            // Supplementary-plane text: GetGlyphIndicesW cannot see surrogate pairs.
            selected_font_covers_codepoints(dc, text_units)
        } else {
            // GetGlyphIndicesW with GGI_MARK_NONEXISTING_GLYPHS returns 0xFFFF for any
            // unsupported glyph in the string.
            let mut glyph_indices = vec![0u16; glyph_len as usize];
            let ret = GetGlyphIndicesW(
                dc,
                glyphs.as_ptr(),
                glyph_len,
                glyph_indices.as_mut_ptr(),
                GGI_MARK_NONEXISTING_GLYPHS,
            );
            ret != GDI_ERROR && !glyph_indices.contains(&0xFFFF)
        };

        if !all_covered {
            SelectObject(dc, old_font);
            DeleteObject(font);
            continue;
        }

        return Some((font, old_font));
    }

    None
}

#[cfg(all(test, target_os = "windows"))]
pub(crate) fn any_family_covers(stack: &str, text: &str) -> bool {
    let glyphs = wide(text);
    let glyph_len = glyphs.len().saturating_sub(1) as i32;
    if glyph_len == 0 {
        return false;
    }

    // SAFETY: DC and any created fonts are selected out and deleted before returning.
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return false;
        }
        let covering = select_covering_font(dc, stack, &glyphs, glyph_len, 14.0, false, false);
        if let Some((font, old_font)) = covering {
            SelectObject(dc, old_font);
            DeleteObject(font);
            DeleteDC(dc);
            true
        } else {
            DeleteDC(dc);
            false
        }
    }
}

/// Test-only probe: is `family` installed? Independent of the coverage decision made by
/// `select_covering_font`, so tests can gate on the environment without mirroring the code.
#[cfg(all(test, target_os = "windows"))]
pub(crate) fn family_installed(family: &str) -> bool {
    // SAFETY: the DC is created and deleted here; no GDI objects are selected into it.
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return false;
        }
        let exists = family_exists(dc, &wide(family));
        DeleteDC(dc);
        exists
    }
}

/// The font and cell a glyph is rasterized for by [`rasterize_to_alpha_buffer`].
pub struct AlphaGlyphRequest<'a> {
    pub family: &'a str,
    pub width: u32,
    pub height: u32,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
}

pub fn rasterize_to_alpha_buffer(
    request: &AlphaGlyphRequest<'_>,
    text: &str,
    buffer: &mut [u8],
) -> bool {
    let &AlphaGlyphRequest {
        family,
        width,
        height,
        font_size,
        bold,
        italic,
    } = request;
    if width == 0 || height == 0 || buffer.len() < (width * height) as usize {
        return false;
    }

    let glyphs = wide(text);
    let glyph_len = glyphs.len().saturating_sub(1) as i32;
    if glyph_len == 0 {
        buffer.fill(0);
        return false;
    }

    let info = BitmapInfo {
        header: BitmapInfoHeader {
            size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            width: width as i32,
            // Negative height selects a top-down DIB so row 0 is the top scanline.
            height: -(height as i32),
            planes: 1,
            bit_count: 32,
            compression: BI_RGB,
            size_image: 0,
            x_pels_per_meter: 0,
            y_pels_per_meter: 0,
            clr_used: 0,
            clr_important: 0,
        },
        colors: [0; 3],
    };

    // SAFETY: every GDI object created below is selected out and deleted before returning,
    // and the DIB pixel pointer is only read for `width * height` 32-bit pixels.
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            buffer.fill(0);
            return false;
        }
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            dc,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(dc);
            buffer.fill(0);
            return false;
        }
        let old_bitmap = SelectObject(dc, bitmap);

        let covering = select_covering_font(
            dc,
            family,
            &glyphs,
            glyph_len,
            font_size,
            bold,
            italic,
        );

        let Some((font, old_font)) = covering else {
            SelectObject(dc, old_bitmap);
            DeleteObject(bitmap);
            DeleteDC(dc);
            buffer.fill(0);
            return false;
        };

        SetBkMode(dc, TRANSPARENT_MODE);
        SetTextColor(dc, 0x00FF_FFFF);
        TextOutW(dc, 0, 0, glyphs.as_ptr(), glyph_len);
        GdiFlush();

        let pixels = std::slice::from_raw_parts(bits as *const u32, (width * height) as usize);
        let mut inked = false;
        for (dst, px) in buffer.iter_mut().zip(pixels.iter()) {
            let coverage = ((px >> 8) & 0xFF) as u8;
            if coverage > 0 {
                inked = true;
            }
            *dst = (*dst).max(coverage);
        }

        SelectObject(dc, old_font);
        DeleteObject(font);
        SelectObject(dc, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(dc);
        inked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candidate_families_fallback_order_and_deduplication() {
        let candidates: Vec<&str> =
            candidate_families("MesloLGS NF, 'Noto Sans KR', monospace").collect();
        assert_eq!(&candidates[..3], ["MesloLGS NF", "Noto Sans KR", "Consolas"]);
        let fallback_tail: Vec<&str> = SYSTEM_FALLBACK_FAMILIES
            .iter()
            .copied()
            .filter(|family| *family != "Consolas")
            .collect();
        assert_eq!(&candidates[3..], fallback_tail.as_slice());

        let deduped: Vec<&str> =
            candidate_families("malgun gothic, CONSOLAS, Malgun Gothic").collect();
        assert_eq!(&deduped[..2], ["malgun gothic", "CONSOLAS"]);
        assert!(!deduped[2..].iter().any(|family| {
            family.eq_ignore_ascii_case("malgun gothic") || family.eq_ignore_ascii_case("consolas")
        }));
        assert_eq!(deduped.len(), SYSTEM_FALLBACK_FAMILIES.len());
    }

    #[test]
    fn test_system_fallback_covers_korean_on_windows() {
        assert!(
            family_installed("Malgun Gothic"),
            "the Windows test baseline requires the Malgun Gothic font"
        );
        assert!(any_family_covers("monospace", "가"));
        assert!(!any_family_covers("monospace", "\u{10fffd}"));
    }

    #[test]
    fn test_supplementary_plane_glyph_is_covered_and_inked() {
        assert!(
            family_installed("Segoe UI Emoji"),
            "the Windows test baseline requires the Segoe UI Emoji font"
        );
        assert!(any_family_covers("monospace", "\u{1F600}"));

        let mut buf = vec![0u8; 32 * 32];
        let inked =
            rasterize_to_alpha_buffer(
                &AlphaGlyphRequest {
                    family: "monospace",
                    width: 32,
                    height: 32,
                    font_size: 20.0,
                    bold: false,
                    italic: false,
                },
                "\u{1F600}",
                &mut buf,
            );
        assert!(inked, "U+1F600 must rasterize ink through the GDI path");
        assert!(buf.iter().any(|&b| b > 0));
    }

    #[test]
    fn test_cmap_format12_lookup() {
        // cmap header (version 0, 1 table) + record (3, 10, offset 12) + format 12 subtable
        // with one group mapping U+1F600..=U+1F601 to glyphs 0 and 1.
        let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12];
        cmap.extend_from_slice(&[0, 12, 0, 0]);
        cmap.extend_from_slice(&28u32.to_be_bytes());
        cmap.extend_from_slice(&0u32.to_be_bytes());
        cmap.extend_from_slice(&1u32.to_be_bytes());
        cmap.extend_from_slice(&0x1F600u32.to_be_bytes());
        cmap.extend_from_slice(&0x1F601u32.to_be_bytes());
        cmap.extend_from_slice(&0u32.to_be_bytes());
        assert!(!cmap_format12_covers(&cmap, 0x1F600), "glyph 0 is .notdef, not coverage");
        assert!(cmap_format12_covers(&cmap, 0x1F601));
        assert!(!cmap_format12_covers(&cmap, 0x1F602));
        assert!(!cmap_format12_covers(&cmap[..10], 0x1F601), "truncated tables must not panic");
    }

    #[test]
    fn test_rasterize_korean_with_fallback() {
        let mut buf = vec![0u8; 16 * 16];
        let inked = rasterize_to_alpha_buffer(
            &AlphaGlyphRequest {
                family: "monospace",
                width: 16,
                height: 16,
                font_size: 14.0,
                bold: false,
                italic: false,
            },
            "가",
            &mut buf,
        );
        assert!(inked);
        assert!(buf.iter().any(|&b| b > 0));

        let mut blank_buf = vec![0u8; 16 * 16];
        let blank_inked = rasterize_to_alpha_buffer(
            &AlphaGlyphRequest {
                family: "monospace",
                width: 16,
                height: 16,
                font_size: 14.0,
                bold: false,
                italic: false,
            },
            "\u{10fffd}",
            &mut blank_buf,
        );
        assert!(!blank_inked);
        assert!(blank_buf.iter().all(|&b| b == 0));
    }
}
