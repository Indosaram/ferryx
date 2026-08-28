//! FreeType renders each char to an 8-bit bitmap; fontconfig resolves the family to a file.

use std::ffi::{c_char, c_int, c_long, c_uint, c_void, CStr, CString};
use std::ptr;
use std::sync::{Mutex, OnceLock};

const FT_LOAD_RENDER: i32 = 1 << 2;
const FC_MATCH_PATTERN: c_int = 0;

#[repr(C)]
struct FtBitmap {
    rows: c_uint,
    width: c_uint,
    pitch: c_int,
    buffer: *const u8,
    num_grays: u16,
    pixel_mode: c_char,
    palette_mode: c_char,
    palette: *mut c_void,
}

#[repr(C)]
struct FtGlyphSlot {
    library: *mut c_void,
    face: *mut c_void,
    next: *mut c_void,
    glyph_index: c_uint,
    generic_data: *mut c_void,
    generic_finalizer: *mut c_void,
    metrics: [c_long; 8],
    linear_hori_advance: c_long,
    linear_vert_advance: c_long,
    advance_x: c_long,
    advance_y: c_long,
    format: c_uint,
    bitmap: FtBitmap,
    bitmap_left: c_int,
    bitmap_top: c_int,
}

#[repr(C)]
struct FtFaceRec {
    num_faces: c_long,
    face_index: c_long,
    face_flags: c_long,
    style_flags: c_long,
    num_glyphs: c_long,
    family_name: *const c_char,
    style_name: *const c_char,
    num_fixed_sizes: c_int,
    available_sizes: *mut c_void,
    num_charmaps: c_int,
    charmaps: *mut c_void,
    generic_data: *mut c_void,
    generic_finalizer: *mut c_void,
    bbox: [c_long; 4],
    units_per_em: u16,
    ascender: i16,
    descender: i16,
    height: i16,
    max_advance_width: i16,
    max_advance_height: i16,
    underline_position: i16,
    underline_thickness: i16,
    glyph: *mut FtGlyphSlot,
    size: *mut c_void,
    charmap: *mut c_void,
}

#[link(name = "freetype")]
unsafe extern "C" {
    fn FT_Init_FreeType(library: *mut *mut c_void) -> c_int;
    fn FT_New_Face(
        library: *mut c_void,
        path: *const c_char,
        index: c_long,
        face: *mut *mut FtFaceRec,
    ) -> c_int;
    fn FT_Done_Face(face: *mut FtFaceRec) -> c_int;
    fn FT_Set_Pixel_Sizes(face: *mut FtFaceRec, width: c_uint, height: c_uint) -> c_int;
    fn FT_Load_Char(face: *mut FtFaceRec, code: c_ulong_t, flags: i32) -> c_int;
}

#[allow(non_camel_case_types)]
type c_ulong_t = std::ffi::c_ulong;

#[link(name = "fontconfig")]
unsafe extern "C" {
    fn FcInitLoadConfigAndFonts() -> *mut c_void;
    fn FcNameParse(name: *const u8) -> *mut c_void;
    fn FcConfigSubstitute(config: *mut c_void, pattern: *mut c_void, kind: c_int) -> c_int;
    fn FcDefaultSubstitute(pattern: *mut c_void);
    fn FcFontMatch(config: *mut c_void, pattern: *mut c_void, result: *mut c_int) -> *mut c_void;
    fn FcPatternGetString(
        pattern: *mut c_void,
        object: *const c_char,
        index: c_int,
        value: *mut *mut u8,
    ) -> c_int;
    fn FcPatternDestroy(pattern: *mut c_void);
}

struct Library(*mut c_void);

// SAFETY: the FreeType library handle is only used behind the mutex below, which serializes
// every call into the non-reentrant FreeType API.
unsafe impl Send for Library {}

fn library() -> Option<&'static Mutex<Library>> {
    static LIBRARY: OnceLock<Option<Mutex<Library>>> = OnceLock::new();
    LIBRARY
        .get_or_init(|| {
            let mut handle = ptr::null_mut();
            // SAFETY: FT_Init_FreeType writes a library handle into the out-pointer.
            let status = unsafe { FT_Init_FreeType(&mut handle) };
            (status == 0 && !handle.is_null()).then(|| Mutex::new(Library(handle)))
        })
        .as_ref()
}

fn resolve_font_path(family: &str) -> Option<String> {
    let name = CString::new(family).ok()?;
    let file_key = CString::new("file").ok()?;
    // SAFETY: every fontconfig pattern created here is destroyed before returning, and the
    // borrowed file string is copied out while its owning pattern is still alive.
    unsafe {
        let config = FcInitLoadConfigAndFonts();
        let pattern = FcNameParse(name.as_ptr() as *const u8);
        if pattern.is_null() {
            return None;
        }
        FcConfigSubstitute(config, pattern, FC_MATCH_PATTERN);
        FcDefaultSubstitute(pattern);
        let mut result: c_int = 0;
        let matched = FcFontMatch(config, pattern, &mut result);
        FcPatternDestroy(pattern);
        if matched.is_null() {
            return None;
        }
        let mut file: *mut u8 = ptr::null_mut();
        let path = if FcPatternGetString(matched, file_key.as_ptr(), 0, &mut file) == 0
            && !file.is_null()
        {
            CStr::from_ptr(file as *const c_char)
                .to_str()
                .ok()
                .map(str::to_owned)
        } else {
            None
        };
        FcPatternDestroy(matched);
        path
    }
}

pub fn rasterize_to_alpha_buffer(
    family: &str,
    text: &str,
    buffer: &mut [u8],
    width: u32,
    height: u32,
    font_size: f32,
    _bold: bool,
    _italic: bool,
) -> bool {
    if width == 0 || height == 0 || buffer.len() < (width * height) as usize {
        return false;
    }
    let Some(ch) = text.chars().next().filter(|c| !c.is_whitespace()) else {
        return false;
    };
    let Some(library) = library() else {
        return false;
    };
    let Some(path) = resolve_font_path(family).or_else(|| resolve_font_path("monospace")) else {
        return false;
    };
    let Ok(path) = CString::new(path) else {
        return false;
    };
    let Ok(guard) = library.lock() else {
        return false;
    };

    let pixel_size = font_size.round().max(1.0) as c_uint;
    // SAFETY: the face is created and destroyed within this lock, and the glyph bitmap is
    // only read for the rows/width FreeType reports.
    unsafe {
        let mut face: *mut FtFaceRec = ptr::null_mut();
        if FT_New_Face(guard.0, path.as_ptr(), 0, &mut face) != 0 || face.is_null() {
            return false;
        }
        FT_Set_Pixel_Sizes(face, 0, pixel_size);
        if FT_Load_Char(face, ch as c_ulong_t, FT_LOAD_RENDER) != 0 {
            FT_Done_Face(face);
            return false;
        }
        let slot = (*face).glyph;
        if slot.is_null() {
            FT_Done_Face(face);
            return false;
        }
        let bitmap = &(*slot).bitmap;
        if bitmap.buffer.is_null() || bitmap.pitch <= 0 {
            FT_Done_Face(face);
            return false;
        }

        let baseline = (pixel_size as c_int * 4) / 5;
        let left = (*slot).bitmap_left.max(0);
        let top = (baseline - (*slot).bitmap_top).max(0);
        let mut inked = false;
        for row in 0..bitmap.rows as c_int {
            let dst_y = top + row;
            if dst_y < 0 || dst_y >= height as c_int {
                continue;
            }
            for col in 0..bitmap.width as c_int {
                let dst_x = left + col;
                if dst_x < 0 || dst_x >= width as c_int {
                    continue;
                }
                let src = *bitmap.buffer.add((row * bitmap.pitch + col) as usize);
                if src > 0 {
                    inked = true;
                    let idx = (dst_y as u32 * width + dst_x as u32) as usize;
                    buffer[idx] = buffer[idx].max(src);
                }
            }
        }
        FT_Done_Face(face);
        inked
    }
}
