//! Clipboard image extraction for panes whose shell runs on another machine.
//!
//! A local pane never needs this: Ferryx forwards the raw `0x16` paste chord and the agent
//! reads this machine's clipboard itself. An SSH pane's agent lives on the far side of the
//! connection, where that clipboard does not exist, so the image bytes have to travel with
//! the paste and land in a file the remote agent can open.
//!
//! Every platform reads its own clipboard behind [`read_clipboard_image`]. Linux shells out to the
//! session's own helper (`wl-paste` under Wayland, `xclip` under X11), so a host with neither
//! installed reports an empty clipboard rather than a broken paste.

use crate::ipc::IpcError;

/// Upper bound on what one paste may push across the connection. Enforced by the caller so an
/// oversized clipboard reports why it was refused instead of looking like an empty clipboard.
pub const MAX_CLIPBOARD_IMAGE_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_REMOTE_DROP_FILE_BYTES: usize = 30 * 1024 * 1024;

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
    if let Some(tiff) = data_for("public.tiff") {
        if let Some(rep) = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(&tiff)) {
            // SAFETY: UB category: FFI boundary UB.
            // Runtime invariant: `rep` is a live NSBitmapImageRep decoded from pasteboard TIFF and the
            // property dictionary is a valid empty NSDictionary, which is what PNG encoding accepts.
            let png = unsafe {
                rep.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            };
            if let Some(png) = png {
                if let Some(image) = ClipboardImage::new(png.to_vec(), "png") {
                    return Some(image);
                }
            }
        }
    }

    // Check if a file URL pointing to an image was copied (e.g. from Finder).
    let file_url_type = NSString::from_str("public.file-url");
    if let Some(text) = pasteboard.stringForType(&file_url_type) {
        if let Some(url) = objc2_foundation::NSURL::URLWithString(&text) {
            if let Some(path_str) = url.path() {
                let path_buf = std::path::PathBuf::from(path_str.to_string());
                if let Some(ext) = path_buf.extension().and_then(|e| e.to_str()) {
                    let ext_lower = ext.to_ascii_lowercase();
                    let extension = match ext_lower.as_str() {
                        "png" => Some("png"),
                        "jpg" | "jpeg" => Some("jpg"),
                        "gif" => Some("gif"),
                        "webp" => Some("webp"),
                        _ => None,
                    };
                    if let Some(ext_static) = extension {
                        if let Ok(bytes) = std::fs::read(&path_buf) {
                            if let Some(image) = ClipboardImage::new(bytes, ext_static) {
                                return Some(image);
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
pub fn read_clipboard_image() -> Option<ClipboardImage> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW,
        OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    // Standard Win32 clipboard formats, also needed without the native renderer.
    const CF_DIB_ID: u32 = 8;
    const CF_DIBV5_ID: u32 = 17;

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
        if format == CF_DIB_ID || format == CF_DIBV5_ID {
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

/// Longest a clipboard helper may run before it is killed, so a wedged tool cannot hold a paste.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const CLIPBOARD_TOOL_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);

/// Clipboard helpers to try, in order. Wayland comes first: `wl-paste` reads the compositor's own
/// clipboard, which is the only one that exists under a Wayland session even when XWayland runs.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const CLIPBOARD_TOOLS: [(&str, &[&str]); 2] = [
    ("wl-paste", &["--type", "image/png"]),
    ("xclip", &["-selection", "clipboard", "-t", "image/png", "-o"]),
];

/// Runs one clipboard helper and hands back its stdout bytes.
///
/// A tool that is not installed, exits non-zero, prints nothing, or wedges are all ordinary
/// misses, so the caller moves on to the next helper instead of reporting a clipboard error.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn run_clipboard_tool(program: &str, args: &[&str]) -> Option<Vec<u8>> {
    use std::io::Read;

    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    // The payload is drained on its own thread: a helper that writes more than a pipe buffer holds
    // would otherwise block on write until the timeout killed it and the image would be lost.
    let stdout = child.stdout.take()?;
    let drain = std::thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });

    let deadline = std::time::Instant::now() + CLIPBOARD_TOOL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let bytes = drain.join().unwrap_or_default();
                return status.success().then_some(bytes);
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // Wedged or unreapable: kill it so the paste fails fast instead of hanging.
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = drain.join();
                return None;
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn read_clipboard_image() -> Option<ClipboardImage> {
    read_clipboard_image_from(&CLIPBOARD_TOOLS)
}

/// Reads the first helper in `tools` that hands back real PNG bytes.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn read_clipboard_image_from(tools: &[(&str, &[&str])]) -> Option<ClipboardImage> {
    const PNG_MAGIC: [u8; 4] = [0x89, 0x50, 0x4E, 0x47];

    for &(program, args) in tools {
        let Some(bytes) = run_clipboard_tool(program, args) else {
            continue;
        };
        // A helper can exit successfully with a payload in a format this build cannot decode;
        // that is a miss, not an image, so only real PNG bytes travel on.
        if bytes.starts_with(&PNG_MAGIC) {
            if let Some(image) = ClipboardImage::new(bytes, "png") {
                return Some(image);
            }
        }
    }

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

const MAX_PASTE_FILE_NAME_LEN: usize = 128;

fn invalid_arg(msg: impl Into<String>) -> IpcError {
    IpcError::new(crate::ipc::error::IpcErrorCode::InvalidArgument, msg)
}

pub fn validate_paste_file_name(file_name: &str) -> Result<(), IpcError> {
    if file_name.is_empty() || file_name.len() > MAX_PASTE_FILE_NAME_LEN {
        return Err(invalid_arg("Invalid paste file name length"));
    }
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err(invalid_arg("Path traversal is forbidden"));
    }
    if !file_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(invalid_arg("Invalid characters in paste file name"));
    }
    if file_name.starts_with('.') || !file_name.contains('.') {
        return Err(invalid_arg("File name must contain an extension"));
    }
    Ok(())
}

pub fn validate_upload_id(upload_id: &str) -> Result<(), IpcError> {
    if upload_id.is_empty() || upload_id.len() > 64 {
        return Err(invalid_arg("Invalid upload ID length"));
    }
    if !upload_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(invalid_arg("Invalid characters in upload ID"));
    }
    Ok(())
}

pub fn prune_old_paste_files(dir: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        let now = std::time::SystemTime::now();
        let one_day = std::time::Duration::from_secs(24 * 60 * 60);
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                let is_stale = meta
                    .modified()
                    .ok()
                    .and_then(|m| now.duration_since(m).ok())
                    .is_some_and(|age| age > one_day);
                if is_stale {
                    if meta.is_dir() {
                        let _ = std::fs::remove_dir_all(entry.path());
                    } else if meta.is_file() {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}

pub fn save_paste_chunk_in_dir(
    dir: &std::path::Path,
    upload_id: &str,
    file_name: &str,
    chunk_index: u32,
    total_chunks: u32,
    data: &[u8],
) -> Result<Option<std::path::PathBuf>, IpcError> {
    validate_paste_file_name(file_name)?;
    validate_upload_id(upload_id)?;
    if total_chunks == 0 || chunk_index >= total_chunks {
        return Err(invalid_arg("Invalid chunk index or total chunks"));
    }

    std::fs::create_dir_all(dir)
        .map_err(|e| IpcError::internal(format!("Failed to create paste directory: {e}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }

    if chunk_index == 0 {
        prune_old_paste_files(dir);
    }

    let staging_name = format!("staging-{upload_id}.part");
    let staging_path = dir.join(staging_name);

    {
        let mut options = std::fs::OpenOptions::new();
        options.create(true).write(true);
        if chunk_index == 0 {
            options.truncate(true);
        } else {
            options.append(true);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        use std::io::Write;
        let mut file = options
            .open(&staging_path)
            .map_err(|e| IpcError::internal(format!("Failed to open staging file: {e}")))?;
        file.write_all(data)
            .map_err(|e| IpcError::internal(format!("Failed to write paste chunk: {e}")))?;
        file.flush()
            .map_err(|e| IpcError::internal(format!("Failed to flush paste chunk: {e}")))?;
    }

    if chunk_index + 1 == total_chunks {
        let final_path = dir.join(file_name);
        std::fs::rename(&staging_path, &final_path)
            .map_err(|e| IpcError::internal(format!("Failed to finalize paste file: {e}")))?;
        Ok(Some(final_path))
    } else {
        Ok(None)
    }
}

pub fn save_paste_file_in_dir(
    dir: &std::path::Path,
    file_name: &str,
    data: &[u8],
) -> Result<std::path::PathBuf, IpcError> {
    let upload_id = uuid::Uuid::new_v4().to_string();
    let res = save_paste_chunk_in_dir(dir, &upload_id, file_name, 0, 1, data)?;
    res.ok_or_else(|| IpcError::internal("Failed to finalize single paste file"))
}

pub fn default_paste_dir() -> std::path::PathBuf {
    let tmp = std::env::var_os("TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    tmp.join("ferryx-paste")
}

pub fn save_paste_file(file_name: &str, data: &[u8]) -> Result<std::path::PathBuf, IpcError> {
    let dir = default_paste_dir();
    save_paste_file_in_dir(&dir, file_name, data)
}

pub fn save_paste_chunk(
    upload_id: &str,
    file_name: &str,
    chunk_index: u32,
    total_chunks: u32,
    data: &[u8],
) -> Result<Option<std::path::PathBuf>, IpcError> {
    let dir = default_paste_dir();
    save_paste_chunk_in_dir(&dir, upload_id, file_name, chunk_index, total_chunks, data)
}

pub fn validate_drop_file_name(file_name: &str) -> Result<(), IpcError> {
    if file_name.is_empty() || file_name.len() > 255 {
        return Err(invalid_arg("Invalid drop file name length"));
    }
    if file_name == "." || file_name == ".." {
        return Err(invalid_arg("Drop file name cannot be a relative directory reference"));
    }
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err(invalid_arg("Path traversal is forbidden"));
    }
    if file_name.chars().any(|c| c.is_control()) {
        return Err(invalid_arg("Control characters forbidden in drop file name"));
    }
    if file_name.chars().any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')) {
        return Err(invalid_arg("Invalid characters in drop file name"));
    }
    if file_name.ends_with(' ') || file_name.ends_with('.') {
        return Err(invalid_arg("Drop file name cannot end with space or dot"));
    }
    Ok(())
}

/// Sanitizes an arbitrary local file name so it is guaranteed to pass [`validate_drop_file_name`].
pub fn sanitize_drop_file_name(raw: &str) -> String {
    let base = std::path::Path::new(raw)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(raw);
    let mut sanitized = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_control() || matches!(ch, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') {
            sanitized.push('_');
        } else {
            sanitized.push(ch);
        }
    }
    let trimmed = sanitized.trim_matches([' ', '.'].as_slice());
    let mut candidate = if trimmed.is_empty() {
        "file".to_string()
    } else {
        trimmed.to_string()
    };

    let stem = candidate.split('.').next().unwrap_or(&candidate);
    let is_reserved = matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6"
            | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6"
            | "LPT7" | "LPT8" | "LPT9"
    );
    if is_reserved {
        candidate = format!("_{candidate}");
    }

    if candidate.len() > 200 {
        let extension = std::path::Path::new(&candidate)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        let stem_budget = 200usize.saturating_sub(extension.len());
        let mut truncated = String::new();
        for ch in candidate.chars() {
            if truncated.len() + ch.len_utf8() > stem_budget {
                break;
            }
            truncated.push(ch);
        }
        let clean_stem = truncated.trim_end_matches([' ', '.'].as_slice());
        candidate = format!("{clean_stem}{extension}");
        if candidate.is_empty() {
            candidate = "file".to_string();
        }
    }
    candidate
}

/// Fallback for legacy hosts whose `validate_paste_file_name` requires ASCII `[a-zA-Z0-9._-]`
/// and a dot with non-empty extension.
pub fn legacy_safe_drop_file_name(upload_id: &str, file_name: &str) -> String {
    let sanitized = sanitize_drop_file_name(file_name);
    let mut ascii = String::with_capacity(sanitized.len());
    for ch in sanitized.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
            ascii.push(ch);
        } else {
            ascii.push('_');
        }
    }
    let prefix = if upload_id.len() >= 8 {
        &upload_id[..8]
    } else {
        "paste"
    };
    let mut parts: Vec<&str> = ascii.split('.').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return format!("{prefix}.dat");
    }
    let ext = if parts.len() > 1 {
        parts.pop().unwrap()
    } else {
        "dat"
    };
    let stem = parts.join("_");
    let stem = stem.trim_matches(['_', '-'].as_slice());
    let stem = if stem.is_empty() { "file" } else { stem };
    let max_stem_len = 120usize.saturating_sub(prefix.len() + 1 + 1 + ext.len());
    let truncated_stem = if stem.len() > max_stem_len {
        &stem[..max_stem_len]
    } else {
        stem
    };
    format!("{prefix}_{truncated_stem}.{ext}")
}

#[derive(Debug)]
struct DropUploadPending {
    created_at: std::time::Instant,
    file_name: String,
    total_chunks: u32,
    total_bytes: u64,
    received_chunks: std::collections::HashSet<u32>,
    received_bytes: u64,
}

static DROP_UPLOAD_STORE: parking_lot::Mutex<
    Option<std::collections::HashMap<(std::path::PathBuf, String), DropUploadPending>>,
> = parking_lot::Mutex::new(None);

pub fn save_paste_chunk_v2_in_dir(
    dir: &std::path::Path,
    upload_id: &str,
    file_name: &str,
    chunk_index: u32,
    total_chunks: u32,
    offset: u64,
    total_bytes: u64,
    data: &[u8],
) -> Result<Option<std::path::PathBuf>, IpcError> {
    validate_drop_file_name(file_name)?;
    validate_upload_id(upload_id)?;

    if total_bytes == 0 || total_bytes > MAX_REMOTE_DROP_FILE_BYTES as u64 {
        return Err(IpcError::new(
            crate::ipc::error::IpcErrorCode::PayloadTooLarge,
            format!(
                "Drop upload totalBytes {} exceeds limit of {} bytes",
                total_bytes, MAX_REMOTE_DROP_FILE_BYTES
            ),
        ));
    }
    if total_chunks == 0 || chunk_index >= total_chunks {
        return Err(invalid_arg("Invalid chunk index or total chunks"));
    }
    if offset.saturating_add(data.len() as u64) > total_bytes {
        return Err(invalid_arg(
            "Chunk offset and length exceed declared total bytes",
        ));
    }

    std::fs::create_dir_all(dir)
        .map_err(|e| IpcError::internal(format!("Failed to create paste directory: {e}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }

    let staging_dir = dir.join(upload_id);
    std::fs::create_dir_all(&staging_dir)
        .map_err(|e| IpcError::internal(format!("Failed to create staging directory: {e}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staging_dir, std::fs::Permissions::from_mode(0o700));
    }

    let staging_path = staging_dir.join(".upload.staging");
    let final_path = staging_dir.join(file_name);

    if final_path.is_file() {
        if let Ok(meta) = final_path.metadata() {
            if meta.len() == total_bytes {
                return Ok(Some(final_path));
            }
        }
    }

    // Write chunk at exact offset
    {
        use std::io::Seek;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&staging_path)
            .map_err(|e| IpcError::internal(format!("Failed to open staging file: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }

        file.seek(std::io::SeekFrom::Start(offset))
            .map_err(|e| IpcError::internal(format!("Failed to seek staging file: {e}")))?;
        file.write_all(data)
            .map_err(|e| IpcError::internal(format!("Failed to write chunk: {e}")))?;
        file.flush()
            .map_err(|e| IpcError::internal(format!("Failed to flush chunk: {e}")))?;
    }

    // Register receipt in assembly store
    let key = (dir.to_path_buf(), upload_id.to_string());
    let mut guard = DROP_UPLOAD_STORE.lock();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);

    // Evict entries older than 30 minutes
    let now = std::time::Instant::now();
    let thirty_mins = std::time::Duration::from_secs(30 * 60);
    map.retain(|_, v| now.duration_since(v.created_at) < thirty_mins);

    let entry = map.entry(key.clone()).or_insert_with(|| {
        prune_old_paste_files(dir);
        DropUploadPending {
            created_at: now,
            file_name: file_name.to_string(),
            total_chunks,
            total_bytes,
            received_chunks: std::collections::HashSet::new(),
            received_bytes: 0,
        }
    });

    if entry.file_name != file_name
        || entry.total_chunks != total_chunks
        || entry.total_bytes != total_bytes
    {
        map.remove(&key);
        let _ = std::fs::remove_file(&staging_path);
        return Err(invalid_arg(
            "Inconsistent metadata across chunks for upload ID",
        ));
    }

    if entry.received_chunks.insert(chunk_index) {
        entry.received_bytes = entry.received_bytes.saturating_add(data.len() as u64);
    }

    let is_complete = entry.received_chunks.len() == entry.total_chunks as usize
        && entry.received_bytes == entry.total_bytes;

    if is_complete {
        map.remove(&key);
        drop(guard);

        let actual_len = std::fs::metadata(&staging_path)
            .map_err(|e| IpcError::internal(format!("Failed to read staging metadata: {e}")))?
            .len();
        if actual_len != total_bytes {
            let _ = std::fs::remove_file(&staging_path);
            return Err(IpcError::internal(format!(
                "Staged file size mismatch: actual {actual_len} != expected {total_bytes}"
            )));
        }

        std::fs::rename(&staging_path, &final_path)
            .map_err(|e| IpcError::internal(format!("Failed to finalize drop file: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&final_path, std::fs::Permissions::from_mode(0o600));
        }

        Ok(Some(final_path))
    } else {
        Ok(None)
    }
}

pub fn save_paste_chunk_v2(
    upload_id: &str,
    file_name: &str,
    chunk_index: u32,
    total_chunks: u32,
    offset: u64,
    total_bytes: u64,
    data: &[u8],
) -> Result<Option<std::path::PathBuf>, IpcError> {
    let dir = default_paste_dir();
    save_paste_chunk_v2_in_dir(
        &dir,
        upload_id,
        file_name,
        chunk_index,
        total_chunks,
        offset,
        total_bytes,
        data,
    )
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalClipboardImagePaste {
    pub local_path: String,
    pub byte_length: usize,
}

#[tauri::command]
pub async fn cmd_local_paste_clipboard_image<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    session_id: Option<String>,
) -> Result<Option<LocalClipboardImagePaste>, IpcError> {
    use tauri::Manager;
    let daemon = session_id
        .as_ref()
        .map(|_| app.state::<std::sync::Arc<crate::daemon::client::DaemonClient>>().inner().clone());
    let Some(image) = read_clipboard_image_for_app(&app).await? else {
        return Ok(None);
    };
    let detect = |id: String| {
        let daemon = daemon.clone();
        async move {
            match daemon {
                Some(daemon) => daemon.detect_manual_ssh(&id).await,
                None => Ok(None),
            }
        }
    };
    paste_image_for_session(image, session_id, detect, |ssh, name, bytes| async move {
        let target = crate::ssh::manual::target_for(&ssh)?;
        crate::ssh::manual::upload(&target, &name, bytes).await
    })
    .await
    .map(Some)
}

/// Routes a paste to the destination that can read it. A detected manual SSH must be the same
/// process before and after the transfer, so an ssh that exits or is replaced mid-upload never
/// inserts a path from the wrong host. Detection failures never degrade to a local save.
async fn paste_image_for_session<D, DF, U, UF>(
    image: ClipboardImage,
    session_id: Option<String>,
    detect: D,
    upload: U,
) -> Result<LocalClipboardImagePaste, IpcError>
where
    D: Fn(String) -> DF,
    DF: std::future::Future<Output = Result<Option<crate::terminal::manual_ssh::ManualSshProcess>, IpcError>>,
    U: FnOnce(crate::terminal::manual_ssh::ManualSshProcess, String, Vec<u8>) -> UF,
    UF: std::future::Future<Output = Result<String, IpcError>>,
{
    let byte_length = image.bytes.len();
    if byte_length > MAX_CLIPBOARD_IMAGE_BYTES {
        return Err(IpcError::new(
            crate::ipc::error::IpcErrorCode::PayloadTooLarge,
            format!(
                "Clipboard image exceeds maximum size of {} bytes",
                MAX_CLIPBOARD_IMAGE_BYTES
            ),
        ));
    }

    let file_name = format!("{}.{}", uuid::Uuid::new_v4(), image.extension);
    let before = match session_id.clone() {
        Some(id) => detect(id).await?,
        None => None,
    };
    let Some(before) = before else {
        let path = crate::ipc::run_blocking(move || save_paste_file(&file_name, &image.bytes)).await?;
        // An ssh started while the file was written would receive a path its host cannot read.
        if let Some(id) = session_id {
            if detect(id).await?.is_some() {
                return Err(changed_during_paste());
            }
        }
        return Ok(LocalClipboardImagePaste {
            local_path: path.to_string_lossy().to_string(),
            byte_length,
        });
    };
    let remote_path = upload(before.clone(), file_name, image.bytes).await?;
    let after = detect(session_id.unwrap_or_default()).await?;
    if after.as_ref() != Some(&before) {
        return Err(changed_during_paste());
    }
    Ok(LocalClipboardImagePaste {
        local_path: remote_path,
        byte_length,
    })
}

fn changed_during_paste() -> IpcError {
    IpcError::new(
        crate::ipc::error::IpcErrorCode::Unsupported,
        "The SSH session changed during the image paste",
    )
    .with_details(serde_json::json!({ "code": crate::terminal::manual_ssh::CODE_CHANGED }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::manual_ssh::ManualSshProcess;
    use std::sync::Mutex as StdMutex;

    fn ssh(pid: u32) -> ManualSshProcess {
        ManualSshProcess { pid, foreground_group: pid, argv: vec!["ssh".into(), "box".into()], executable: None, cwd: None }
    }

    fn png() -> ClipboardImage {
        ClipboardImage { bytes: vec![1, 2, 3], extension: "png" }
    }

    async fn route(answers: Vec<Result<Option<ManualSshProcess>, IpcError>>) -> Result<LocalClipboardImagePaste, IpcError> {
        let answers = StdMutex::new(answers.into_iter());
        paste_image_for_session(
            png(),
            Some("s1".into()),
            |_| {
                let next = answers.lock().unwrap().next().expect("unexpected detect call");
                async move { next }
            },
            |_, name, _| async move { Ok(format!("/tmp/ferryx-paste/{name}")) },
        )
        .await
    }

    #[tokio::test]
    async fn manual_ssh_returns_remote_path_when_owner_is_unchanged() {
        let paste = route(vec![Ok(Some(ssh(7))), Ok(Some(ssh(7)))]).await.unwrap();
        assert!(paste.local_path.starts_with("/tmp/ferryx-paste/"));
        assert_eq!(paste.byte_length, 3);
    }

    #[tokio::test]
    async fn manual_ssh_replaced_during_upload_fails() {
        for after in [None, Some(ssh(8))] {
            let err = route(vec![Ok(Some(ssh(7))), Ok(after)]).await.unwrap_err();
            assert_eq!(err.details.unwrap()["code"], crate::terminal::manual_ssh::CODE_CHANGED);
        }
    }

    #[tokio::test]
    async fn ssh_started_during_local_save_fails() {
        let err = route(vec![Ok(None), Ok(Some(ssh(9)))]).await.unwrap_err();
        assert_eq!(err.details.unwrap()["code"], crate::terminal::manual_ssh::CODE_CHANGED);
    }

    #[tokio::test]
    async fn detection_errors_never_fall_back_to_local_save() {
        let old_daemon = crate::daemon::client::manual_ssh_response(crate::daemon::protocol::DaemonResponse::Error {
            message: "Malformed request: unknown variant".into(),
            code: None,
            details: None,
        })
        .unwrap_err();
        assert!(matches!(old_daemon.code, crate::ipc::error::IpcErrorCode::DaemonProtocolMismatch));
        let err = route(vec![Err(old_daemon)]).await.unwrap_err();
        assert!(matches!(err.code, crate::ipc::error::IpcErrorCode::DaemonProtocolMismatch));
    }

    #[test]
    fn structured_detection_codes_map_to_unsupported() {
        let err = crate::daemon::client::manual_ssh_response(crate::daemon::protocol::DaemonResponse::Error {
            message: "Multiple SSH clients own this terminal".into(),
            code: Some(crate::terminal::manual_ssh::CODE_AMBIGUOUS.into()),
            details: None,
        })
        .unwrap_err();
        assert!(matches!(err.code, crate::ipc::error::IpcErrorCode::Unsupported));
        let io = crate::daemon::client::manual_ssh_response(crate::daemon::protocol::DaemonResponse::Error {
            message: "proc_listpgrppids failed".into(),
            code: Some(crate::terminal::manual_ssh::CODE_IO.into()),
            details: None,
        })
        .unwrap_err();
        assert!(matches!(io.code, crate::ipc::error::IpcErrorCode::IoError));
    }

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

    #[test]
    fn save_paste_file_in_dir_writes_file_and_prunes_stale_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("ferryx-paste");

        // Pre-populate an old file (older than 24h)
        std::fs::create_dir_all(&dir).expect("create dir");
        let old_file = dir.join("old.png");
        std::fs::write(&old_file, b"old-content").expect("write old file");
        let two_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(48 * 3600);
        let f = std::fs::File::open(&old_file).expect("open old file");
        let _ = f.set_modified(two_days_ago);

        // Pre-populate a fresh file
        let fresh_file = dir.join("fresh.png");
        std::fs::write(&fresh_file, b"fresh-content").expect("write fresh file");

        // Save new file
        let saved = save_paste_file_in_dir(&dir, "paste-1.png", b"new-bytes")
            .expect("save_paste_file_in_dir succeeds");

        assert_eq!(saved, dir.join("paste-1.png"));
        assert_eq!(std::fs::read(&saved).expect("read saved"), b"new-bytes");

        // Stale file must be pruned, fresh file must remain
        assert!(!old_file.exists(), "stale file should have been pruned");
        assert!(fresh_file.exists(), "fresh file should remain");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir_meta = std::fs::metadata(&dir).expect("dir meta");
            assert_eq!(dir_meta.permissions().mode() & 0o777, 0o700);
            let file_meta = std::fs::metadata(&saved).expect("file meta");
            assert_eq!(file_meta.permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn save_paste_chunk_in_dir_assembles_multi_part_upload() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("ferryx-paste");
        let upload_id = "test-upload-123";
        let file_name = "image.png";

        let chunk1 = b"Hello, ";
        let chunk2 = b"world!";

        let res1 = save_paste_chunk_in_dir(&dir, upload_id, file_name, 0, 2, chunk1)
            .expect("chunk 0 succeeds");
        assert_eq!(res1, None);

        let res2 = save_paste_chunk_in_dir(&dir, upload_id, file_name, 1, 2, chunk2)
            .expect("chunk 1 succeeds");
        let final_path = res2.expect("chunk 1 finalizes");
        assert_eq!(final_path, dir.join(file_name));
        assert_eq!(
            std::fs::read(&final_path).expect("read final"),
            b"Hello, world!"
        );
    }

    #[test]
    fn save_paste_file_rejects_path_traversal_and_invalid_names() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("ferryx-paste");

        assert!(save_paste_file_in_dir(&dir, "../escape.png", b"data").is_err());
        assert!(save_paste_file_in_dir(&dir, "sub/dir.png", b"data").is_err());
        assert!(save_paste_file_in_dir(&dir, "noextension", b"data").is_err());
        assert!(save_paste_file_in_dir(&dir, ".hidden", b"data").is_err());
        assert!(save_paste_file_in_dir(&dir, "", b"data").is_err());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn a_missing_clipboard_tool_is_a_miss_rather_than_a_panic() {
        // No helper is installed under this name, which is what a host with neither `wl-paste`
        // nor `xclip` looks like: the read comes back empty instead of failing the paste.
        let absent: [(&str, &[&str]); 1] = [("ferryx-no-such-clipboard-tool", &["--type"])];

        assert_eq!(read_clipboard_image_from(&absent), None);
    }

    #[test]
    fn save_paste_chunk_v2_assembles_out_of_order_and_is_idempotent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("ferryx-paste");
        let upload_id = "v2-assembly-12345678";
        let file_name = "test_document.txt";
        let part1 = b"Part One; ";
        let part2 = b"Part Two; ";
        let part3 = b"Part Three.";
        let total_bytes = (part1.len() + part2.len() + part3.len()) as u64;

        // Deliver part 2 first
        let res = save_paste_chunk_v2_in_dir(
            &dir,
            upload_id,
            file_name,
            1,
            3,
            part1.len() as u64,
            total_bytes,
            part2,
        )
        .expect("part 2 ok");
        assert_eq!(res, None);

        // Deliver part 3 next
        let res = save_paste_chunk_v2_in_dir(
            &dir,
            upload_id,
            file_name,
            2,
            3,
            (part1.len() + part2.len()) as u64,
            total_bytes,
            part3,
        )
        .expect("part 3 ok");
        assert_eq!(res, None);

        // Deliver duplicate part 2 — idempotent write
        let res = save_paste_chunk_v2_in_dir(
            &dir,
            upload_id,
            file_name,
            1,
            3,
            part1.len() as u64,
            total_bytes,
            part2,
        )
        .expect("duplicate part 2 ok");
        assert_eq!(res, None);

        // Deliver part 1 — completes
        let res = save_paste_chunk_v2_in_dir(
            &dir,
            upload_id,
            file_name,
            0,
            3,
            0,
            total_bytes,
            part1,
        )
        .expect("part 1 completes");
        let final_path = res.expect("finalized");
        assert_eq!(final_path, dir.join(upload_id).join(file_name));
        let assembled = std::fs::read(&final_path).expect("read assembled");
        assert_eq!(assembled, b"Part One; Part Two; Part Three.");

        // Re-requesting after finalization returns the existing path idempotently
        let res_after = save_paste_chunk_v2_in_dir(
            &dir,
            upload_id,
            file_name,
            0,
            3,
            0,
            total_bytes,
            part1,
        )
        .expect("retry after finalize ok");
        assert_eq!(res_after, Some(final_path));
    }

    #[test]
    fn save_paste_chunk_v2_rejects_payload_above_limit_and_inconsistent_meta() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("ferryx-paste");
        let upload_id = "v2-limit-12345678";

        // Exceeds 30MB
        let too_large = (MAX_REMOTE_DROP_FILE_BYTES + 1) as u64;
        let err = save_paste_chunk_v2_in_dir(&dir, upload_id, "large.bin", 0, 1, 0, too_large, b"x");
        assert!(err.is_err());

        // Inconsistent metadata across chunks for same upload_id
        let res1 = save_paste_chunk_v2_in_dir(&dir, upload_id, "name1.txt", 0, 2, 0, 10, b"hello");
        assert!(res1.is_ok());
        let res2 = save_paste_chunk_v2_in_dir(&dir, upload_id, "name2.txt", 1, 2, 5, 10, b"world");
        assert!(res2.is_err(), "mismatched file_name must be rejected");
    }

    #[test]
    fn sanitize_and_validate_drop_file_name_handles_unicode_and_reserved_names() {
        assert!(validate_drop_file_name("보고서.pdf").is_ok());
        assert!(validate_drop_file_name("my report 2026.docx").is_ok());
        assert!(validate_drop_file_name(".hidden").is_ok());
        assert!(validate_drop_file_name("").is_err());
        assert!(validate_drop_file_name("..").is_err());
        assert!(validate_drop_file_name("dir/file.txt").is_err());
        assert!(validate_drop_file_name("dir\\file.txt").is_err());
        assert!(validate_drop_file_name("trailing. ").is_err());

        assert_eq!(sanitize_drop_file_name("con.txt"), "_con.txt");
        assert_eq!(sanitize_drop_file_name("my<bad>:name.txt"), "my_bad__name.txt");
        assert_eq!(sanitize_drop_file_name("보고서 2026.pdf"), "보고서 2026.pdf");
        assert_eq!(sanitize_drop_file_name("   "), "file");

        let legacy = legacy_safe_drop_file_name("u12345678-abcd", "보고서 2026.pdf");
        assert!(legacy.starts_with("u1234567_"));
        assert!(legacy.ends_with(".pdf"));
        assert!(validate_paste_file_name(&legacy).is_ok());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn a_wedged_clipboard_tool_is_killed_instead_of_holding_the_paste() {
        let wedged: [(&str, &[&str]); 1] = [("sleep", &["30"])];

        let started = std::time::Instant::now();
        assert_eq!(read_clipboard_image_from(&wedged), None);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "a wedged helper must be killed at the timeout, not waited out"
        );
    }
}
