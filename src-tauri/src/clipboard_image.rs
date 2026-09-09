//! Clipboard image extraction for panes whose shell runs on another machine.
//!
//! A local pane never needs this: Ferryx forwards the raw `0x16` paste chord and the agent
//! reads this machine's clipboard itself. An SSH pane's agent lives on the far side of the
//! connection, where that clipboard does not exist, so the image bytes have to travel with
//! the paste and land in a file the remote agent can open.
//!
//! Every platform reads its own clipboard behind [`read_clipboard_image`]; targets without an
//! implementation return `None`, which leaves the caller on the existing local paste chord.

/// Upper bound on what one paste may push across the connection. Enforced by the caller so an
/// oversized clipboard reports why it was refused instead of looking like an empty clipboard.
pub const MAX_CLIPBOARD_IMAGE_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardImage {
    pub bytes: Vec<u8>,
    /// File extension without the dot. Agents route on it, so it must match the bytes.
    pub extension: &'static str,
}

impl ClipboardImage {
    pub fn new(bytes: Vec<u8>, extension: &'static str) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }
        Some(Self { bytes, extension })
    }
}

#[cfg(target_os = "macos")]
pub fn read_clipboard_image() -> Option<ClipboardImage> {
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSPasteboard};
    use objc2_foundation::{NSData, NSDictionary, NSString};

    let pasteboard = NSPasteboard::generalPasteboard();
    let data_for = |uti: &str| -> Option<Vec<u8>> {
        let ty = NSString::from_str(uti);
        pasteboard.dataForType(&ty).map(|data| data.to_vec())
    };

    // Screenshots and browser copies already carry a format agents accept; pass those through
    // untouched rather than round-tripping them through a decoder.
    for (uti, extension) in [
        ("public.png", "png"),
        ("public.jpeg", "jpg"),
        ("com.compuserve.gif", "gif"),
        ("org.webmproject.webp", "webp"),
    ] {
        if let Some(image) = data_for(uti).and_then(|bytes| ClipboardImage::new(bytes, extension)) {
            return Some(image);
        }
    }

    // AppKit apps (Preview, Keynote) leave only TIFF behind, which agents reject.
    let tiff = data_for("public.tiff")?;
    let rep = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(&tiff))?;
    // SAFETY: UB category: FFI boundary UB.
    // Runtime invariant: `rep` is a live NSBitmapImageRep decoded from pasteboard TIFF and the
    // property dictionary is a valid empty NSDictionary, which is what PNG encoding accepts.
    let png = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    ClipboardImage::new(png.to_vec(), "png")
}

#[cfg(target_os = "windows")]
pub fn read_clipboard_image() -> Option<ClipboardImage> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW,
        OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            // SAFETY: UB category: FFI boundary UB.
            // Runtime invariant: the guard exists only after a successful OpenClipboard, so this
            // closes the clipboard this thread owns exactly once.
            unsafe { CloseClipboard() };
        }
    }

    // SAFETY: UB category: FFI boundary UB.
    // Runtime invariant: a null HWND associates the clipboard with the current task, which is
    // the documented way to read it without owning a window.
    if unsafe { OpenClipboard(std::ptr::null_mut::<std::ffi::c_void>() as HWND) } == 0 {
        return None;
    }
    let _guard = ClipboardGuard;

    let read_format = |format: u32| -> Option<Vec<u8>> {
        // SAFETY: UB category: FFI boundary UB.
        // Runtime invariant: called only while this thread holds the clipboard open; the handle
        // stays owned by the clipboard and is only locked for the length GlobalSize reports.
        unsafe {
            let handle = GetClipboardData(format);
            if handle.is_null() {
                return None;
            }
            let size = GlobalSize(handle);
            if size == 0 {
                return None;
            }
            let pointer = GlobalLock(handle);
            if pointer.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(pointer as *const u8, size).to_vec();
            GlobalUnlock(handle);
            Some(bytes)
        }
    };

    let mut png_format: Option<u32> = None;
    let mut dib_format: Option<u32> = None;
    let mut format = 0u32;
    loop {
        // SAFETY: UB category: FFI boundary UB.
        // Runtime invariant: enumeration runs only while the clipboard is open by this thread.
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        if format == crate::ipc::native_terminal::CF_DIB_ID
            || format == crate::ipc::native_terminal::CF_DIBV5_ID
        {
            dib_format.get_or_insert(format);
            continue;
        }
        let mut name = [0u16; 64];
        // SAFETY: UB category: FFI boundary UB.
        // Runtime invariant: `name` is a live buffer of the length passed to the call.
        let length =
            unsafe { GetClipboardFormatNameW(format, name.as_mut_ptr(), name.len() as i32) };
        if length > 0 {
            let label = String::from_utf16_lossy(&name[..length as usize]);
            if label.eq_ignore_ascii_case("PNG") {
                png_format.get_or_insert(format);
            }
        }
    }

    if let Some(image) = png_format
        .and_then(read_format)
        .and_then(|bytes| ClipboardImage::new(bytes, "png"))
    {
        return Some(image);
    }
    // Paint, Excel and older apps publish only a device-independent bitmap.
    dib_format
        .and_then(read_format)
        .and_then(|dib| dib_to_png(&dib))
        .and_then(|bytes| ClipboardImage::new(bytes, "png"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn read_clipboard_image() -> Option<ClipboardImage> {
    None
}

/// Reads the clipboard on the thread the platform requires and hands back the bytes.
///
/// macOS pasteboard access follows the same main-thread dispatch the native terminal paste path
/// already uses; other platforms read on a blocking worker.
pub async fn read_clipboard_image_for_app<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<ClipboardImage>, crate::ipc::IpcError> {
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        let window = app
            .get_window("main")
            .ok_or_else(|| crate::ipc::IpcError::internal("Main Ferryx window is unavailable"))?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        window
            .run_on_main_thread(move || {
                let _ = sender.send(read_clipboard_image());
            })
            .map_err(|error| {
                crate::ipc::IpcError::internal(format!(
                    "Could not dispatch native clipboard read: {error}"
                ))
            })?;
        receiver.await.map_err(|_| {
            crate::ipc::IpcError::internal(
                "Main thread stopped before native clipboard read completed",
            )
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        tokio::task::spawn_blocking(read_clipboard_image)
            .await
            .map_err(|error| {
                crate::ipc::IpcError::internal(format!("blocking clipboard read failed: {error}"))
            })
    }
}

const BITMAP_INFO_HEADER_BYTES: usize = 40;
const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    read_u32(bytes, offset).map(|value| value as i32)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

/// Encodes a packed Windows DIB (`CF_DIB`/`CF_DIBV5` payload) as PNG.
///
/// Only the uncompressed 24/32-bit forms the clipboard actually carries are supported; anything
/// else returns `None` so the caller can fall back instead of shipping a corrupt file.
pub fn dib_to_png(dib: &[u8]) -> Option<Vec<u8>> {
    let header_bytes = read_u32(dib, 0)? as usize;
    if header_bytes < BITMAP_INFO_HEADER_BYTES || header_bytes > dib.len() {
        return None;
    }
    let width = read_i32(dib, 4)?;
    let raw_height = read_i32(dib, 8)?;
    let bit_count = read_u16(dib, 14)?;
    let compression = read_u32(dib, 16)?;
    let colors_used = read_u32(dib, 32)? as usize;

    if width <= 0 || raw_height == 0 || !matches!(bit_count, 24 | 32) {
        return None;
    }
    // BI_BITFIELDS is accepted only in its default channel order, which is the only layout the
    // clipboard produces for 32-bit bitmaps.
    if compression != BI_RGB && !(compression == BI_BITFIELDS && bit_count == 32) {
        return None;
    }
    // A bottom-up bitmap stores its first row last; a negative height means top-down.
    let top_down = raw_height < 0;
    let height = raw_height.unsigned_abs() as usize;
    let width = width as usize;

    let mut pixel_offset = header_bytes;
    if compression == BI_BITFIELDS && header_bytes == BITMAP_INFO_HEADER_BYTES {
        // A v3 header keeps its channel masks between the header and the pixels.
        pixel_offset += 12;
    }
    pixel_offset += colors_used * 4;

    let channels = bit_count as usize / 8;
    let stride = width
        .checked_mul(channels)?
        .checked_add(3)?
        .checked_div(4)?
        .checked_mul(4)?;
    let pixels = dib.get(pixel_offset..)?;
    if pixels.len() < stride.checked_mul(height)? {
        return None;
    }

    let mut rgba = Vec::with_capacity(width.checked_mul(height)?.checked_mul(4)?);
    let mut any_alpha = false;
    for row in 0..height {
        let source_row = if top_down { row } else { height - 1 - row };
        let start = source_row * stride;
        for column in 0..width {
            let pixel = &pixels[start + column * channels..start + (column + 1) * channels];
            let alpha = if channels == 4 { pixel[3] } else { 255 };
            any_alpha |= alpha != 0;
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], alpha]);
        }
    }
    // 32-bit BI_RGB carries an undefined fourth byte that older producers leave zeroed; treating
    // it as alpha would encode a fully transparent image.
    if channels == 4 && !any_alpha {
        for pixel in rgba.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
    }

    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(&rgba).ok()?;
    writer.finish().ok()?;
    Some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap_info_header(width: i32, height: i32, bit_count: u16, compression: u32) -> Vec<u8> {
        let mut header = Vec::new();
        header.extend_from_slice(&(BITMAP_INFO_HEADER_BYTES as u32).to_le_bytes());
        header.extend_from_slice(&width.to_le_bytes());
        header.extend_from_slice(&height.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&bit_count.to_le_bytes());
        header.extend_from_slice(&compression.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        header.extend_from_slice(&0i32.to_le_bytes());
        header.extend_from_slice(&0i32.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        header
    }

    fn decode(png_bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
        let mut reader = decoder.read_info().expect("read png info");
        let mut buffer = vec![0; reader.output_buffer_size().expect("output size")];
        let info = reader.next_frame(&mut buffer).expect("decode frame");
        buffer.truncate(info.buffer_size());
        (info.width, info.height, buffer)
    }

    #[test]
    fn bottom_up_24bpp_dib_is_flipped_and_converted_to_rgba() {
        // One pixel per row, BGR plus one padding byte, bottom row stored first.
        let mut dib = bitmap_info_header(1, 2, 24, BI_RGB);
        dib.extend_from_slice(&[255, 0, 0, 0]);
        dib.extend_from_slice(&[0, 0, 255, 0]);

        let (width, height, pixels) = decode(&dib_to_png(&dib).expect("convert"));
        assert_eq!((width, height), (1, 2));
        assert_eq!(pixels, vec![255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn top_down_dib_keeps_row_order() {
        let mut dib = bitmap_info_header(1, -2, 24, BI_RGB);
        dib.extend_from_slice(&[255, 0, 0, 0]);
        dib.extend_from_slice(&[0, 0, 255, 0]);

        let (_, _, pixels) = decode(&dib_to_png(&dib).expect("convert"));
        assert_eq!(pixels, vec![0, 0, 255, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn zeroed_alpha_channel_is_treated_as_opaque() {
        let mut dib = bitmap_info_header(2, 1, 32, BI_RGB);
        dib.extend_from_slice(&[10, 20, 30, 0, 40, 50, 60, 0]);

        let (_, _, pixels) = decode(&dib_to_png(&dib).expect("convert"));
        assert_eq!(pixels, vec![30, 20, 10, 255, 60, 50, 40, 255]);
    }

    #[test]
    fn real_alpha_channel_survives() {
        let mut dib = bitmap_info_header(2, 1, 32, BI_BITFIELDS);
        dib.extend_from_slice(&[0u8; 12]);
        dib.extend_from_slice(&[10, 20, 30, 128, 40, 50, 60, 0]);

        let (_, _, pixels) = decode(&dib_to_png(&dib).expect("convert"));
        assert_eq!(pixels, vec![30, 20, 10, 128, 60, 50, 40, 0]);
    }

    #[test]
    fn unsupported_and_truncated_bitmaps_are_rejected() {
        assert_eq!(dib_to_png(&[]), None);
        assert_eq!(dib_to_png(&bitmap_info_header(1, 1, 24, BI_RGB)), None);
        let mut palette_indexed = bitmap_info_header(1, 1, 8, BI_RGB);
        palette_indexed.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(dib_to_png(&palette_indexed), None);
        let mut compressed = bitmap_info_header(1, 1, 24, 1);
        compressed.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(dib_to_png(&compressed), None);
    }

    #[test]
    fn an_empty_pasteboard_payload_never_becomes_a_clipboard_image() {
        assert_eq!(ClipboardImage::new(Vec::new(), "png"), None);
        assert!(ClipboardImage::new(vec![1, 2, 3], "png").is_some());
    }
}
