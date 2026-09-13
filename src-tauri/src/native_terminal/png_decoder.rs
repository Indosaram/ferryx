//! Process-wide PNG decoder. Output ownership transfers using Ghostty's allocator,
//! never Rust's allocator, exactly as required by GhosttySysDecodePngFn.
use super::{
    error::NativeTerminalError,
    sys::{kitty, types::GhosttyAllocator},
};
use std::{ffi::c_void, io::Cursor, sync::OnceLock};

pub const IMAGE_LIMIT: usize = 64 * 1024 * 1024;

pub fn install() -> Result<(), NativeTerminalError> {
    static RESULT: OnceLock<i32> = OnceLock::new();
    let result = *RESULT.get_or_init(|| {
        // SAFETY: Static callback, installed once before any terminal is created.
        unsafe { kitty::ghostty_sys_set(kitty::DECODE_PNG, decode_png as *const c_void) }
    });
    NativeTerminalError::from_c_result(result, "ghostty_sys_set(DecodePng)")
}

fn decode(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_limits(png::Limits { bytes: IMAGE_LIMIT });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    let len = (info.width as usize)
        .checked_mul(info.height as usize)?
        .checked_mul(4)?;
    if len == 0 || len > IMAGE_LIMIT {
        return None;
    }
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut pixels).ok()?;
    let source = &pixels[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => source.to_vec(),
        png::ColorType::Rgb => source
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => source
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => source.iter().flat_map(|&p| [p, p, p, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((info.width, info.height, rgba))
}

unsafe extern "C" fn decode_png(
    _userdata: *mut c_void,
    allocator: *const GhosttyAllocator,
    data: *const u8,
    len: usize,
    out: *mut kitty::SysImage,
) -> bool {
    if data.is_null() || out.is_null() || len == 0 || len > IMAGE_LIMIT {
        return false;
    }
    // No Rust unwind may cross the C boundary. Malformed PNG is a protocol error.
    let decoded = std::panic::catch_unwind(|| {
        // SAFETY: Ghostty borrows a live input buffer for the callback duration.
        decode(unsafe { std::slice::from_raw_parts(data, len) })
    });
    let Ok(Some((width, height, rgba))) = decoded else {
        return false;
    };
    // SAFETY: The supplied allocator is valid during the callback. Ghostty takes
    // ownership on success and frees through this same allocator.
    let buffer = unsafe { kitty::ghostty_alloc(allocator, rgba.len()) }.cast::<u8>();
    if buffer.is_null() {
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(rgba.as_ptr(), buffer, rgba.len());
        out.write(kitty::SysImage {
            width,
            height,
            data: buffer,
            data_len: rgba.len(),
        });
    }
    true
}
