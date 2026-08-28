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
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn rasterize_to_alpha_buffer(
    family: &str,
    text: &str,
    buffer: &mut [u8],
    width: u32,
    height: u32,
    font_size: f32,
    bold: bool,
    italic: bool,
) -> bool {
    if width == 0 || height == 0 || buffer.len() < (width * height) as usize {
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

    let face = wide(family);
    let glyphs = wide(text);
    let glyph_len = glyphs.len().saturating_sub(1) as i32;
    if glyph_len == 0 {
        return false;
    }

    // SAFETY: every GDI object created below is selected out and deleted before returning,
    // and the DIB pixel pointer is only read for `width * height` 32-bit pixels.
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
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
            return false;
        }
        let old_bitmap = SelectObject(dc, bitmap);

        let font = CreateFontW(
            -(font_size.round() as i32),
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
        let old_font = if font.is_null() {
            std::ptr::null_mut()
        } else {
            SelectObject(dc, font)
        };

        SetBkMode(dc, TRANSPARENT_MODE);
        SetTextColor(dc, 0x00FF_FFFF);
        TextOutW(dc, 0, 0, glyphs.as_ptr(), glyph_len);

        let pixels = std::slice::from_raw_parts(bits as *const u32, (width * height) as usize);
        let mut inked = false;
        for (dst, px) in buffer.iter_mut().zip(pixels.iter()) {
            let coverage = ((px >> 8) & 0xFF) as u8;
            if coverage > 0 {
                inked = true;
            }
            *dst = (*dst).max(coverage);
        }

        if !old_font.is_null() {
            SelectObject(dc, old_font);
        }
        if !font.is_null() {
            DeleteObject(font);
        }
        SelectObject(dc, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(dc);
        inked
    }
}
