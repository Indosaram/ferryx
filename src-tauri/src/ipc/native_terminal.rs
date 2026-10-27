use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::{mpsc, oneshot};

use crate::daemon::DaemonClient;
use crate::ipc::{IpcError, IpcErrorCode};

pub const NATIVE_TERMINAL_RECEIPT_TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(250);

/// Upper bound on how long a bounds request waits for its frame to reach the screen.
///
/// Geometry (layout, PTY size, child view frame) is applied synchronously inside the render call,
/// so only the presentation acknowledgement is waited on. A frame can be dropped without any
/// signal (an open synchronized-output transaction, a busy frame loan), and with no further
/// output the wait would never end; the UI queues every newer measurement behind the in-flight
/// request, so a split pane would keep its old width. Past this bound the deferred receipt is
/// returned and the next queued geometry can go out.
pub const NATIVE_TERMINAL_PRESENTATION_WAIT: std::time::Duration =
    std::time::Duration::from_millis(500);

/// Event carrying an input receipt that is no longer awaited inline.
///
/// The receipt only positions the IME candidate window, but waiting for it used to hold the UI's
/// per-session input queue open for the whole round trip, so the next keystroke could not start
/// until the previous one's receipt arrived. Emitting it lets the command return as soon as the
/// PTY write lands, while the anchor still updates a moment later.
pub const NATIVE_TERMINAL_INPUT_RECEIPT_EVENT: &str = "native_terminal_input_receipt";
pub const NATIVE_TERMINAL_PRESENTATION_RECEIPT_EVENT: &str = "pane_presentation_receipt";
use crate::native_terminal::composition::{CellMetrics, LogicalBounds, SurfaceCompositionLayout};
use crate::native_terminal::snapshot_slot::{PresentedFrame, SnapshotSlot};
use crate::native_terminal::surface_host::{
    NativeTerminalBoundsRequest, NativeTerminalSurfaceHostState, NativeTerminalSurfaceReceipt,
};
use crate::native_terminal::{
    KeyModifiers, MouseAction, MouseEvent, MousePosition, MouseRendererSize, NativeTerminalError,
    NativeTerminalInput, ScrollViewport, TerminalEngine, TerminalWheelOutcome,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalLogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalBoundsReceipt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
    pub presented: bool,
    #[serde(default)]
    pub render_deferred: bool,
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
    pub rebuilt_rows: u16,
    pub reused_rows: u16,
    pub cursor_col: u16,
    pub cursor_row: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_scale_factor: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalAttachReceipt {
    pub session_id: String,
    pub attach_tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
    pub presented: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalScrollbarReceipt {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum NativeTerminalScrollBehavior {
    Top,
    Bottom,
    Delta { rows: isize },
    Row { offset: usize },
}

impl NativeTerminalScrollBehavior {
    pub fn to_scroll_viewport(&self) -> ScrollViewport {
        match self {
            Self::Top => ScrollViewport::Top,
            Self::Bottom => ScrollViewport::Bottom,
            Self::Delta { rows } => ScrollViewport::Delta(*rows),
            Self::Row { offset } => ScrollViewport::Row(*offset),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum NativeTerminalSelectMode {
    All,
    Word { col: u16, row: u16 },
    Line { row: u16 },
    /// Inclusive cell range in screen (scrollback-inclusive) rows, for keyboard copy mode.
    #[serde(rename_all = "camelCase")]
    Range {
        start_col: u16,
        start_row: u32,
        end_col: u16,
        end_row: u32,
        rectangle: bool,
    },
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalSelectionRange {
    pub start_col: u16,
    pub start_row: u16,
    pub end_col: u16,
    pub end_row: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalSelectionReceipt {
    pub text: Option<String>,
    pub range: Option<NativeTerminalSelectionRange>,
}

/// PTY cursor and viewport placement used to seed keyboard copy mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalCursorReceipt {
    /// Cursor column in the active area.
    pub col: u16,
    /// Cursor row in the active area (0 = top of the live screen).
    pub row: u16,
    /// Screen (scrollback-inclusive) row of the cursor.
    pub screen_row: u64,
    /// Screen row shown at the top of the viewport.
    pub viewport_top_row: u64,
    /// Monotonic count of rows pruned from the top of the screen; stored screen rows
    /// shift up by the difference between two readings.
    pub scrollback_pruned_rows: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalMouseReceipt {
    pub mouse_tracking_enabled: bool,
    pub receipt: Option<NativeTerminalBoundsReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalSearchMatch {
    pub row: u16,
    pub start_col: u16,
    pub end_col: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalSearchResult {
    pub matches: Vec<NativeTerminalSearchMatch>,
    pub total_matches: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalLineReceipt {
    pub text: String,
    pub col: u16,
    pub row: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NativeTerminalClipboardContent {
    Text { text: String },
    Image,
    Empty,
}

pub const CF_TEXT_ID: u32 = 1;
pub const CF_BITMAP_ID: u32 = 2;
pub const CF_DIB_ID: u32 = 8;
pub const CF_UNICODETEXT_ID: u32 = 13;
pub const CF_HDROP_ID: u32 = 15;
pub const CF_DIBV5_ID: u32 = 17;

pub fn encode_windows_clipboard_utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn decode_windows_clipboard_utf16(slice: &[u16]) -> Option<String> {
    if slice.is_empty() {
        return None;
    }
    let len = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    if len == 0 {
        return None;
    }
    let s = String::from_utf16_lossy(&slice[..len]);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub fn is_windows_image_or_file_format(format_id: u32, format_name: Option<&str>) -> bool {
    match format_id {
        CF_BITMAP_ID | CF_DIB_ID | CF_DIBV5_ID | CF_HDROP_ID => true,
        _ => {
            if let Some(name) = format_name {
                let name_lower = name.to_ascii_lowercase();
                name_lower.starts_with("png")
                    || name_lower.starts_with("image/")
                    || name_lower.contains("bitmap")
                    || name_lower.contains("dib")
                    || name_lower == "jfif"
                    || name_lower == "gif"
                    || name_lower == "tiff"
                    || name_lower == "heic"
                    || name_lower == "heif"
                    || name_lower == "webp"
                    || name_lower.starts_with("filename")
                    || name_lower.starts_with("filegroupdescriptor")
                    || name_lower.starts_with("filecontents")
            } else {
                false
            }
        }
    }
}

pub fn classify_windows_clipboard(
    text: Option<String>,
    format_ids: &[u32],
    format_names: &[String],
) -> NativeTerminalClipboardContent {
    if let Some(text) = text {
        if !text.is_empty() {
            return NativeTerminalClipboardContent::Text { text };
        }
    }
    let has_image_id = format_ids
        .iter()
        .any(|&id| is_windows_image_or_file_format(id, None));
    if has_image_id {
        return NativeTerminalClipboardContent::Image;
    }
    let has_image_name = format_names
        .iter()
        .any(|name| is_windows_image_or_file_format(0, Some(name.as_str())));
    if has_image_name {
        return NativeTerminalClipboardContent::Image;
    }
    if !format_ids.is_empty() || !format_names.is_empty() {
        // Parity with macOS DOM paste fallback: non-text clipboard items
        // (e.g. custom dropped files / shell objects) route to the image/attachment paste chord.
        return NativeTerminalClipboardContent::Image;
    }
    NativeTerminalClipboardContent::Empty
}

fn is_image_pasteboard_type(t: &str) -> bool {
    t.starts_with("public.png")
        || t.starts_with("public.tiff")
        || t.starts_with("public.jpeg")
        || t.starts_with("public.heic")
        || t.starts_with("public.heif")
        || t.starts_with("public.webp")
        || t.starts_with("com.compuserve.gif")
        || t.starts_with("com.microsoft.bmp")
        || t.starts_with("public.image")
}

pub fn classify_clipboard_content(
    text: Option<String>,
    types: &[String],
) -> NativeTerminalClipboardContent {
    if let Some(text) = text {
        if !text.is_empty() {
            return NativeTerminalClipboardContent::Text { text };
        }
    }
    let has_image = types.iter().any(|t| is_image_pasteboard_type(t));
    if has_image {
        return NativeTerminalClipboardContent::Image;
    }
    if !types.is_empty() {
        // Parity with the previous DOM paste path: ANY non-text payload
        // (e.g. Finder file URLs) routes the agent image-paste chord.
        return NativeTerminalClipboardContent::Image;
    }
    NativeTerminalClipboardContent::Empty
}

pub fn decode_linux_clipboard_text(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes).into_owned();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

pub fn decode_linux_clipboard_descriptors(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn is_linux_image_clipboard_type(clipboard_type: &str) -> bool {
    clipboard_type
        .trim()
        .to_ascii_lowercase()
        .starts_with("image/")
}

/// The Linux clipboard helper family that answered a probe.
///
/// Wayland (`wl-paste`) and X11 (`xclip`) keep separate clipboards, so a probe has to pick one
/// family and stay on it: reading the types through one and the bytes through the other would
/// validate one clipboard while decoding another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxClipboardBackend {
    Wayland,
    X11,
}

impl LinuxClipboardBackend {
    /// Probe order: Wayland first, then the X11 fallback.
    pub const PROBE_ORDER: [LinuxClipboardBackend; 2] =
        [LinuxClipboardBackend::Wayland, LinuxClipboardBackend::X11];

    /// Helper binary that speaks for this backend.
    pub fn program(self) -> &'static str {
        match self {
            LinuxClipboardBackend::Wayland => "wl-paste",
            LinuxClipboardBackend::X11 => "xclip",
        }
    }

    /// Arguments that list the types this clipboard advertises.
    pub fn type_list_args(self) -> &'static [&'static str] {
        match self {
            LinuxClipboardBackend::Wayland => &["--list-types"],
            LinuxClipboardBackend::X11 => &["-selection", "clipboard", "-t", "TARGETS", "-o"],
        }
    }

    /// Arguments that read this clipboard's text.
    pub fn text_args(self) -> &'static [&'static str] {
        match self {
            LinuxClipboardBackend::Wayland => &["--no-newline"],
            LinuxClipboardBackend::X11 => &["-selection", "clipboard", "-o"],
        }
    }
}

/// Picks the first Linux clipboard backend whose type-list probe answers.
///
/// An empty list still counts as an answer (that is what a working helper reports for an empty
/// clipboard), so exactly one backend is chosen and every later read must go through it. The probe
/// is injected so the selection rule is testable without spawning clipboard helpers.
pub fn select_linux_clipboard_backend<F>(mut probe: F) -> Option<(LinuxClipboardBackend, Vec<String>)>
where
    F: FnMut(LinuxClipboardBackend) -> Option<Vec<u8>>,
{
    LinuxClipboardBackend::PROBE_ORDER
        .into_iter()
        .find_map(|backend| {
            probe(backend).map(|bytes| (backend, decode_linux_clipboard_descriptors(&bytes)))
        })
}

/// Reports whether the advertised clipboard types include a text flavour.
///
/// The Wayland/X11 text read is untyped, so it must only run when text is actually on offer;
/// alongside `text/*` the X11 atoms `TEXT`, `UTF8_STRING` and `STRING` also denote text.
pub fn linux_clipboard_advertises_text(types: &[String]) -> bool {
    types.iter().any(|t| {
        let t = t.trim().to_ascii_lowercase();
        t.starts_with("text/") || matches!(t.as_str(), "text" | "utf8_string" | "string")
    })
}

pub fn classify_linux_clipboard(
    text: Option<String>,
    types: &[String],
) -> NativeTerminalClipboardContent {
    let has_image = types.iter().any(|t| is_linux_image_clipboard_type(t));
    // An image-only clipboard can still hand raw image bytes to the untyped text read, and the
    // lossy decode turns those into non-empty garbage. With no text flavour advertised, the
    // image content is the only trustworthy reading.
    if has_image && !linux_clipboard_advertises_text(types) {
        return NativeTerminalClipboardContent::Image;
    }
    if let Some(text) = text {
        if !text.is_empty() {
            return NativeTerminalClipboardContent::Text { text };
        }
    }
    if has_image {
        return NativeTerminalClipboardContent::Image;
    }
    // Wayland/X11 clipboards also advertise `text/*` and private atoms; unlike macOS there is no
    // DOM paste fallback for those, so only real image content routes the image paste chord.
    NativeTerminalClipboardContent::Empty
}

#[cfg(target_os = "macos")]
fn read_native_pasteboard() -> (NativeTerminalClipboardContent, Vec<String>) {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    let pasteboard = NSPasteboard::generalPasteboard();
    let types: Vec<String> = pasteboard
        .types()
        .map(|arr| arr.iter().map(|t| t.to_string()).collect())
        .unwrap_or_default();
    let text = unsafe {
        pasteboard
            .stringForType(NSPasteboardTypeString)
            .map(|s| s.to_string())
    };
    let content = classify_clipboard_content(text, &types);
    (content, types)
}

#[cfg(target_os = "macos")]
fn write_to_pasteboard(pasteboard: &objc2_app_kit::NSPasteboard, text: &str) -> bool {
    use objc2::runtime::ProtocolObject;
    use objc2_foundation::{NSArray, NSString};

    if text.is_empty() {
        return false;
    }

    let ns_text = NSString::from_str(text);
    let obj = ProtocolObject::from_ref(&*ns_text);
    let objects = NSArray::from_slice(&[obj]);
    let _ = pasteboard.clearContents();
    pasteboard.writeObjects(&objects)
}

#[cfg(target_os = "windows")]
fn read_native_pasteboard() -> (NativeTerminalClipboardContent, Vec<String>) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW,
        IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            // SAFETY: UB category: FFI boundary UB.
            // Runtime invariant: CloseClipboard is called exactly once when ClipboardGuard is dropped
            // to balance a preceding successful OpenClipboard call for the current thread/task.
            unsafe {
                CloseClipboard();
            }
        }
    }

    let mut opened = false;
    for _ in 0..5 {
        // SAFETY: UB category: FFI boundary UB.
        // Runtime invariant: Calling OpenClipboard with null HWND (0) is explicitly valid per Win32
        // specification to associate the clipboard with the current process task. Return value is checked
        // before proceeding.
        if unsafe { OpenClipboard(std::ptr::null_mut() as HWND) } != 0 {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    if !opened {
        return (NativeTerminalClipboardContent::Empty, Vec::new());
    }

    let _guard = ClipboardGuard;

    let mut format_ids = Vec::new();
    let mut format_names = Vec::new();
    let mut descriptor_types = Vec::new();

    let mut format = 0u32;
    loop {
        // SAFETY: UB category: FFI boundary UB.
        // Runtime invariant: EnumClipboardFormats is invoked only while the clipboard is opened by the current task.
        // It starts with 0 and passes the previously returned format ID until 0 is returned.
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        format_ids.push(format);

        if format >= 0xC000 {
            let mut name_buf = [0u16; 256];
            // SAFETY: UB category: Out-of-bounds/invalid pointer and FFI boundary UB.
            // Runtime invariant: name_buf is a valid stack-allocated slice of 256 u16 elements. The length
            // passed (256) matches the buffer capacity, preventing out-of-bounds writes. format is a registered ID (>= 0xC000).
            let len = unsafe {
                GetClipboardFormatNameW(format, name_buf.as_mut_ptr(), name_buf.len() as i32)
            };
            if len > 0 {
                let name = String::from_utf16_lossy(&name_buf[..len as usize]);
                descriptor_types.push(name.clone());
                format_names.push(name);
            } else {
                descriptor_types.push(format!("CF_CUSTOM_{format}"));
            }
        } else {
            let name = match format {
                CF_UNICODETEXT_ID => "CF_UNICODETEXT",
                CF_TEXT_ID => "CF_TEXT",
                CF_BITMAP_ID => "CF_BITMAP",
                CF_DIB_ID => "CF_DIB",
                CF_DIBV5_ID => "CF_DIBV5",
                CF_HDROP_ID => "CF_HDROP",
                other => {
                    descriptor_types.push(format!("CF_STANDARD_{other}"));
                    ""
                }
            };
            if !name.is_empty() {
                descriptor_types.push(name.to_string());
            }
        }
    }

    let mut text: Option<String> = None;
    // SAFETY: UB category: FFI boundary UB.
    // Runtime invariant: CF_UNICODETEXT_ID (13) is a standard Win32 format constant, and query occurs
    // while the clipboard is opened.
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT_ID) } != 0 {
        // SAFETY: UB category: FFI boundary UB.
        // Runtime invariant: GetClipboardData is called on the currently open clipboard. The returned
        // handle is checked for null before being dereferenced or passed to GlobalLock.
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT_ID) };
        if !handle.is_null() {
            // SAFETY: UB category: Invalid pointer and FFI boundary UB.
            // Runtime invariant: handle is non-null and obtained from GetClipboardData. The returned
            // pointer is checked for null before dereferencing.
            let ptr = unsafe { GlobalLock(handle) } as *const u16;
            if !ptr.is_null() {
                // SAFETY: UB category: FFI boundary UB.
                // Runtime invariant: handle is a valid, non-null global memory handle returned by GetClipboardData.
                let byte_size = unsafe { GlobalSize(handle) };
                let max_u16_len = byte_size / std::mem::size_of::<u16>();
                if max_u16_len > 0 {
                    // SAFETY: UB category: Out-of-bounds/invalid pointer.
                    // Runtime invariant: ptr is non-null, aligned for u16, and valid for read access for
                    // byte_size bytes as guaranteed by GlobalLock and GlobalSize. max_u16_len is bounded by byte_size.
                    let slice = unsafe { std::slice::from_raw_parts(ptr, max_u16_len) };
                    text = decode_windows_clipboard_utf16(slice);
                }
                // SAFETY: UB category: FFI boundary UB.
                // Runtime invariant: GlobalUnlock is called exactly once for the successful GlobalLock call on the valid handle.
                unsafe {
                    GlobalUnlock(handle);
                }
            }
        }
    }

    let content = classify_windows_clipboard(text, &format_ids, &format_names);
    (content, descriptor_types)
}

/// Longest a Wayland/X11 clipboard helper may run before it is killed, so a wedged tool cannot
/// hold the paste path open.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const NATIVE_CLIPBOARD_TOOL_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// Runs one clipboard helper with a bounded wall-clock budget and hands back its stdout bytes.
///
/// The child's pipes are drained on a helper thread, so a payload larger than a pipe buffer holds
/// cannot deadlock the wait loop. A helper that is not installed, exits non-zero, or wedges is an
/// ordinary clipboard miss (`None`) rather than an error, and no path panics.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn run_clipboard_tool(
    program: &str,
    args: &[&str],
    stdin_bytes: Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    use std::io::{Read as _, Write as _};
    use std::process::Stdio;

    let mut command = crate::util::no_window_command(program);
    command
        .args(args)
        .stdin(if stdin_bytes.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = command.spawn().ok()?;
    let writing = stdin_bytes.is_some();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let (sender, receiver) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        if let (Some(mut stdin), Some(bytes)) = (stdin, stdin_bytes) {
            // Closing the pipe after the write is what tells `wl-copy`/`xclip -i` the payload ended.
            let _ = stdin.write_all(&bytes);
            let _ = stdin.flush();
        }
        let mut buffer = Vec::new();
        if let Some(mut stdout) = stdout {
            let _ = stdout.read_to_end(&mut buffer);
        }
        let _ = sender.send(buffer);
    });

    let deadline = std::time::Instant::now() + NATIVE_CLIPBOARD_TOOL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                if writing {
                    // A clipboard writer hands the payload to a background owner that can keep the
                    // inherited stdout open, so a clean exit is the whole write receipt.
                    return Some(Vec::new());
                }
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                return receiver.recv_timeout(remaining).ok();
            }
            // Wedged or unreapable: kill it so the paste fails fast instead of hanging.
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Reads the system clipboard through the Wayland helpers first, then the X11 ones.
///
/// Both families speak the same contract (type list, then text) and a missing binary is a normal
/// miss, so the two paths chain for discovery only: the backend that answers the type list is the
/// one the text read runs against, because Wayland and X11 keep separate clipboards.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn read_native_pasteboard() -> (NativeTerminalClipboardContent, Vec<String>) {
    let discovered = select_linux_clipboard_backend(|backend| {
        run_clipboard_tool(backend.program(), backend.type_list_args(), None)
    });

    let (types, text) = match discovered {
        // An empty type list means the probe tool failed, and an unadvertised text flavour means
        // the untyped read would return whatever the clipboard holds (image bytes included), so
        // skip the read in both cases. A backend that answers the types but then fails the read
        // yields no text at all rather than borrowing the other backend's clipboard.
        Some((backend, types)) if !types.is_empty() && linux_clipboard_advertises_text(&types) => {
            let text = run_clipboard_tool(backend.program(), backend.text_args(), None)
                .and_then(|bytes| decode_linux_clipboard_text(&bytes));
            (types, text)
        }
        Some((_, types)) => (types, None),
        None => (Vec::new(), None),
    };

    let content = classify_linux_clipboard(text, &types);
    (content, types)
}

#[cfg(target_os = "windows")]
fn write_native_clipboard(text: &str) -> bool {
    use windows_sys::Win32::Foundation::{GlobalFree, HWND};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };

    if text.is_empty() {
        return false;
    }

    let utf16 = encode_windows_clipboard_utf16(text);
    let bytes_len = utf16.len() * std::mem::size_of::<u16>();

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            // SAFETY: UB category: FFI boundary UB.
            // Runtime invariant: CloseClipboard is called exactly once when ClipboardGuard is dropped
            // to balance a preceding successful OpenClipboard call for the current thread/task.
            unsafe {
                CloseClipboard();
            }
        }
    }

    let mut opened = false;
    for _ in 0..5 {
        // SAFETY: UB category: FFI boundary UB.
        // Runtime invariant: Calling OpenClipboard with null HWND (0) is explicitly valid per Win32
        // specification to associate the clipboard with the current process task. Return value is checked
        // before proceeding.
        if unsafe { OpenClipboard(std::ptr::null_mut() as HWND) } != 0 {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    if !opened {
        return false;
    }

    let _guard = ClipboardGuard;

    // SAFETY: UB category: FFI boundary UB.
    // Runtime invariant: EmptyClipboard is called while holding the open clipboard.
    if unsafe { EmptyClipboard() } == 0 {
        return false;
    }

    // SAFETY: UB category: FFI boundary UB.
    // Runtime invariant: GlobalAlloc allocates GMEM_MOVEABLE memory of bytes_len.
    // If allocation fails, handle is null and we return false.
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes_len) };
    if handle.is_null() {
        return false;
    }

    // SAFETY: UB category: Invalid pointer and FFI boundary UB.
    // Runtime invariant: handle is a valid non-null allocation from GlobalAlloc.
    // GlobalLock returns a pointer valid for writing bytes_len bytes.
    let ptr = unsafe { GlobalLock(handle) } as *mut u16;
    if ptr.is_null() {
        // SAFETY: Handle must be freed if lock fails.
        unsafe {
            GlobalFree(handle);
        }
        return false;
    }

    // SAFETY: UB category: Out-of-bounds write.
    // Runtime invariant: ptr points to allocated memory of size bytes_len = utf16.len() * sizeof(u16).
    unsafe {
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr, utf16.len());
        GlobalUnlock(handle);
    }

    // SAFETY: UB category: FFI boundary UB.
    // Runtime invariant: CF_UNICODETEXT_ID (13) is a standard Win32 format constant.
    // If SetClipboardData succeeds, the system takes ownership of the memory handle.
    // If SetClipboardData fails, the caller must free the handle with GlobalFree.
    let result = unsafe { SetClipboardData(CF_UNICODETEXT_ID, handle as _) };
    if result.is_null() {
        unsafe {
            GlobalFree(handle);
        }
        return false;
    }

    true
}

/// Writes the selection to the Linux system clipboard using the provided runner.
///
/// Order: Wayland (`wl-copy`) first, then X11 (`xclip`), then `xsel`.
pub fn write_linux_clipboard_with_runner<F>(text: &str, mut runner: F) -> bool
where
    F: FnMut(&str, &[&str], Vec<u8>) -> bool,
{
    if text.is_empty() {
        return false;
    }
    let bytes = text.as_bytes().to_vec();
    if runner("wl-copy", &[], bytes.clone()) {
        return true;
    }
    if runner("xclip", &["-selection", "clipboard", "-i"], bytes.clone()) {
        return true;
    }
    runner("xsel", &["--clipboard", "--input"], bytes)
}

/// Writes the selection to the system clipboard, Wayland first and X11 second.
///
/// Reports whether a helper accepted the payload; with neither installed the caller keeps its
/// existing behaviour instead of failing the command.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn write_native_clipboard(text: &str) -> bool {
    write_linux_clipboard_with_runner(text, |program, args, bytes| {
        run_clipboard_tool(program, args, Some(bytes)).is_some()
    })
}

/// How long a redraw nudge holds the temporary size. Node/Ink caches the terminal size and
/// ignores a SIGWINCH that reports an unchanged size, so the two resizes must not collapse.
const PTY_REDRAW_NUDGE_SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

/// A queued PTY geometry request. `redraw` asks the program to repaint without changing the
/// size: the dispatcher briefly grows the PTY by one row and then restores the real size, which
/// raises two SIGWINCHs that a same-size resize would not.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PtyGeometryRequest {
    session_id: String,
    cols: u16,
    rows: u16,
    redraw: bool,
    resize: bool,
}

fn merge_pty_request(
    pending: &mut std::collections::VecDeque<PtyGeometryRequest>,
    request: PtyGeometryRequest,
) {
    if let Some(previous) = pending
        .iter_mut()
        .find(|previous| previous.session_id == request.session_id)
    {
        let redraw = previous.redraw || request.redraw;
        if !request.resize && previous.resize {
            // A redraw never sets the size; the queued resize still decides it.
            previous.redraw = redraw;
        } else {
            *previous = PtyGeometryRequest { redraw, ..request };
        }
    } else {
        pending.push_back(request);
    }
}

/// Queues a request, noting when it replaces the target of an operation already in flight so a
/// failed or restoring operation does not overwrite the newer size.
fn accept_pty_request(
    pending: &mut std::collections::VecDeque<PtyGeometryRequest>,
    in_flight: &std::collections::HashSet<String>,
    superseded: &mut std::collections::HashSet<String>,
    request: PtyGeometryRequest,
) {
    if request.resize && in_flight.contains(&request.session_id) {
        superseded.insert(request.session_id.clone());
    }
    merge_pty_request(pending, request);
}

/// One session's in-flight geometry operation. A redraw applies the nudge size and then holds it
/// for `PTY_REDRAW_NUDGE_SETTLE`; `operation` keeps the real size the dispatcher restores.
async fn run_pty_geometry_operation<Fut>(
    operation: PtyGeometryRequest,
    applying: Fut,
) -> (PtyGeometryRequest, Result<(), IpcError>)
where
    Fut: std::future::Future<Output = Result<(), IpcError>>,
{
    let result = applying.await;
    if operation.redraw {
        if let Err(error) = &result {
            tracing::warn!(session_id = %operation.session_id, %error, "PTY redraw nudge failed");
        }
        tokio::time::sleep(PTY_REDRAW_NUDGE_SETTLE).await;
    }
    (operation, result)
}

/// Applies queued PTY geometry with at most one operation in flight per session, so a slow or
/// retrying session never blocks another session while each session stays strictly ordered.
async fn dispatch_pty_resizes<F, Fut>(
    mut receiver: mpsc::UnboundedReceiver<PtyGeometryRequest>,
    mut resize: F,
) where
    F: FnMut(String, u16, u16) -> Fut,
    Fut: std::future::Future<Output = Result<(), IpcError>>,
{
    use futures_util::StreamExt as _;

    let mut pending = std::collections::VecDeque::<PtyGeometryRequest>::new();
    let mut applied = std::collections::HashMap::<String, (u16, u16)>::new();
    let mut retries = std::collections::HashMap::<String, (tokio::time::Instant, std::time::Duration)>::new();
    let mut in_flight = std::collections::HashSet::<String>::new();
    let mut superseded = std::collections::HashSet::<String>::new();
    let mut operations = futures_util::stream::FuturesUnordered::new();
    let mut receiver_open = true;
    loop {
        while receiver_open {
            match receiver.try_recv() {
                Ok(request) => accept_pty_request(&mut pending, &in_flight, &mut superseded, request),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => receiver_open = false,
            }
        }
        let now = tokio::time::Instant::now();
        let mut index = 0;
        while index < pending.len() {
            let queued = &pending[index];
            if in_flight.contains(&queued.session_id)
                || retries.get(&queued.session_id).is_some_and(|(at, _)| *at > now)
            {
                index += 1;
                continue;
            }
            let request = pending.remove(index).expect("index is within pending");
            let PtyGeometryRequest { session_id, cols, rows, redraw, resize: sets_size } = request;
            // Repaint at the size the PTY already has; the grid size is only a fallback.
            // A redraw merged into a failed resize must not discard its unapplied target.
            let (cols, rows) = if !sets_size && !retries.contains_key(&session_id) {
                applied.get(&session_id).copied().unwrap_or((cols, rows))
            } else {
                (cols, rows)
            };
            let applying_rows = if redraw { rows.saturating_add(1).min(1000) } else { rows };
            let applying = resize(session_id.clone(), cols, applying_rows);
            in_flight.insert(session_id.clone());
            operations.push(run_pty_geometry_operation(
                PtyGeometryRequest { session_id, cols, rows, redraw, resize: sets_size },
                applying,
            ));
        }
        if !receiver_open && pending.is_empty() && operations.is_empty() {
            break;
        }
        let next_retry = pending
            .iter()
            .filter(|queued| !in_flight.contains(&queued.session_id))
            .filter_map(|queued| retries.get(&queued.session_id).map(|(at, _)| *at))
            .min();
        tokio::select! {
            request = receiver.recv(), if receiver_open => match request {
                Some(request) => accept_pty_request(&mut pending, &in_flight, &mut superseded, request),
                None => receiver_open = false,
            },
            Some((operation, result)) = operations.next(), if !operations.is_empty() => {
                let PtyGeometryRequest { session_id, cols, rows, redraw, .. } = operation;
                in_flight.remove(&session_id);
                let replaced = superseded.remove(&session_id);
                if redraw {
                    // The settle window has elapsed; restore the real size, or the newer
                    // target queued while the nudge was held.
                    let mut target = (cols, rows);
                    if replaced {
                        let queued = pending.iter().position(|queued| queued.session_id == session_id);
                        if let Some(index) = queued {
                            target = (pending[index].cols, pending[index].rows);
                            if !pending[index].redraw {
                                pending.remove(index);
                            }
                        }
                    }
                    // A failed restore must retry the real size, not leave the temporary nudge applied.
                    pending.push_front(PtyGeometryRequest {
                        session_id, cols: target.0, rows: target.1, redraw: false, resize: true,
                    });
                } else {
                    match result {
                        Ok(()) => {
                            retries.remove(&session_id);
                            applied.insert(session_id, (cols, rows));
                        }
                        Err(error) => {
                            tracing::warn!(%session_id, %error, "PTY resize was not applied");
                            if error.code == IpcErrorCode::SessionNotFound {
                                retries.remove(&session_id);
                                applied.remove(&session_id);
                            } else {
                                let delay = retries.get(&session_id).map_or(
                                    std::time::Duration::from_millis(100),
                                    |(_, delay)| (*delay * 2).min(std::time::Duration::from_secs(2)),
                                );
                                retries.insert(session_id.clone(), (tokio::time::Instant::now() + delay, delay));
                                // A newer size queued meanwhile wins; otherwise retry this one,
                                // keeping a redraw queued meanwhile merged on top of it.
                                if !replaced {
                                    let queued = pending.iter().position(|queued| queued.session_id == session_id);
                                    let queued = queued.and_then(|index| pending.remove(index));
                                    pending.push_back(PtyGeometryRequest { session_id, cols, rows, redraw: false, resize: true });
                                    if let Some(queued) = queued {
                                        merge_pty_request(&mut pending, queued);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ = tokio::time::sleep_until(next_retry.unwrap_or(now)), if next_retry.is_some() => {}
            else => break,
        }
    }
}

fn install_pty_resize_dispatcher(
    state: &NativeTerminalSurfaceHostState,
    daemon_client: Arc<DaemonClient>,
) {
    let (sender, receiver) = mpsc::unbounded_channel::<PtyGeometryRequest>();
    let redraw_sender = sender.clone();
    let installed = state.set_pty_resize_sink_if_absent(Arc::new(move |session_id, cols, rows| {
        let request = PtyGeometryRequest { session_id: session_id.to_string(), cols, rows, redraw: false, resize: true };
        if let Err(error) = sender.send(request) {
            tracing::warn!(
                session_id,
                cols,
                rows,
                %error,
                "Failed to queue native terminal PTY resize"
            );
        }
    }));
    if !installed {
        return;
    }
    state.set_pty_redraw_sink_if_absent(Arc::new(move |session_id, cols, rows| {
        let request = PtyGeometryRequest { session_id: session_id.to_string(), cols, rows, redraw: true, resize: false };
        if let Err(error) = redraw_sender.send(request) {
            tracing::warn!(session_id, %error, "Failed to queue native terminal PTY redraw");
        }
    }));
    tauri::async_runtime::spawn(async move {
        dispatch_pty_resizes(receiver, |session_id, cols, rows| {
            let daemon_client = Arc::clone(&daemon_client);
            async move {
                daemon_client.resize_terminal(&session_id, cols, rows).await
            }
        })
        .await;
    });
}

/// Optional replay cursor (audit M6): when the caller knows the engine already holds
/// state through this sequence, attach replays only the tail instead of the full ring.
#[tauri::command]
pub async fn cmd_native_terminal_attach<R: Runtime>(
    app: AppHandle<R>,
    session_id: String,
    bounds: Option<NativeTerminalLogicalRect>,
    scale_factor: Option<f64>,
    after_sequence: Option<String>,
    attach_tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
    remaining_ms: Option<u64>,
) -> Result<NativeTerminalAttachReceipt, IpcError> {
    let daemon_client = app.state::<Arc<DaemonClient>>();
    let state = app.state::<NativeTerminalSurfaceHostState>();
    let receipt_state = state.inner().clone();
    let expected_tuple = attach_tuple.clone();
    // Without a GPU adapter the surface would attach and then never present: a blank pane.
    // Report a typed error so the UI mounts the DOM grid renderer instead.
    probe_native_renderer(&receipt_state).await?;
    let deadline = tokio::time::Instant::now()
        + std::time::Duration::from_millis(remaining_ms.unwrap_or(4_000).min(4_000));
    native_terminal_attach(app.clone(), daemon_client, state, NativeTerminalAttachParams {
        session_id: session_id.clone(), bounds, scale_factor, after_sequence, attach_tuple, deadline,
    }).await?;
    let active = receipt_state.session_attach_tuple(&session_id);
    if active != expected_tuple {
        return Err(IpcError::from(NativeTerminalError::SessionDetached(session_id)));
    }
    Ok(NativeTerminalAttachReceipt { session_id, attach_tuple: active, presented: false })
}

/// Resolves the shared surface GPU context on the GPU worker, the thread that would otherwise
/// create it lazily at the first frame, so an adapter that exists is initialized exactly as
/// before. The result is cached for the process lifetime, so the fallback holds for this launch.
async fn probe_native_renderer(state: &NativeTerminalSurfaceHostState) -> Result<(), IpcError> {
    use crate::native_terminal::renderer::gpu_context::GpuContext;
    GpuContext::check_forced_adapter_failure().map_err(IpcError::from)?;
    // An adapter can exist yet be unable to present (software fallback with no surface
    // formats on a driverless host). Once any window surface proved that, refuse the attach.
    GpuContext::check_surface_presentation().map_err(IpcError::from)?;
    // Unit tests drive attach against fake daemons; only the seam above may fail them here,
    // so their outcome never depends on the test host's GPU.
    if cfg!(test) {
        return Ok(());
    }
    let (sender, receiver) = oneshot::channel();
    if !state.gpu_worker().enqueue(move |_gpu| {
        let _ = sender.send(GpuContext::shared_surface_context().map(|_| ()));
    }) {
        return Ok(());
    }
    match receiver.await {
        Ok(Err(error)) => Err(IpcError::from(error)),
        _ => Ok(()),
    }
}

/// Surfaces a presentation failure discovered by a GPU frame after the pane already attached:
/// the first pane on a driverless host attaches before any surface was configured, so its
/// bounds update is where it learns the native renderer can never present.
fn check_native_surface_presentation() -> Result<(), IpcError> {
    crate::native_terminal::renderer::gpu_context::GpuContext::check_surface_presentation()
        .map_err(IpcError::from)
}

struct NativeTerminalAttachParams {
    session_id: String,
    bounds: Option<NativeTerminalLogicalRect>,
    scale_factor: Option<f64>,
    after_sequence: Option<String>,
    attach_tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
    deadline: tokio::time::Instant,
}

async fn native_terminal_attach<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    params: NativeTerminalAttachParams,
) -> Result<(), IpcError> {
    let NativeTerminalAttachParams { session_id, bounds, scale_factor, after_sequence, attach_tuple, deadline } = params;
    install_pty_resize_dispatcher(state.inner(), Arc::clone(daemon_client.inner()));
    let logical_bounds = match (bounds, scale_factor) {
        (Some(rect), Some(scale)) if scale.is_finite() && scale > 0.0 => Some(LogicalBounds {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            scale_factor: scale,
        }),
        _ => None,
    };
    if state
        .reattach_existing_session_with_bounds_and_tuple(&session_id, logical_bounds, attach_tuple.as_ref())
        .map_err(|error| IpcError::internal(error.to_string()))?
    {
        return Ok(());
    }

    // Fence the old pump and capture the replay cursor in one transaction BEFORE awaiting the
    // daemon. Reading the cursor first and awaiting afterwards lets the old pump keep applying
    // output across the await, so the response would be a delta against a cursor the grid has
    // already passed — which classifies as an overlap and destroys resident scrollback.
    if attach_tuple.as_ref().is_some_and(|tuple| tuple.backend_session_id != session_id) {
        return Err(IpcError::from(NativeTerminalError::SessionDetached(session_id)));
    }
    let is_new_reservation = attach_tuple.is_some() && state.session_attempt_generation(&session_id).is_none();
    if is_new_reservation {
        // Reserve a resident VT before awaiting the daemon, so even the first two concurrent
        // attaches have a generation and binding against which their completions are checked.
        let bounds = logical_bounds.unwrap_or(LogicalBounds {
            x: 0.0, y: 0.0, width: 800.0, height: 480.0, scale_factor: 1.0,
        });
        state.prepare_session_layout(NativeTerminalBoundsRequest {
            session_id: session_id.clone(), bounds,
        }, font_metrics_for_bounds(bounds)).map_err(IpcError::from)?;
    }

    let initial_attach_tuple = attach_tuple.clone();

    let attach_body = async {
        let token = state.begin_replay_request_with_tuple(&session_id, attach_tuple.clone());
        if token.is_none() && (attach_tuple.is_some() || state.session_attach_tuple(&session_id).is_some()) {
            return Err(IpcError::from(NativeTerminalError::SessionDetached(session_id.clone())));
        }
        let after_seq = replay_request_cursor(token.as_ref().map(|token| token.last_sequence), after_sequence);
        let mut attachment = match daemon_client.attach(&session_id, after_seq).await {
            Ok(attachment) => attachment,
            // The fence already aborted the old stream, so a failed request must not leave the pane
            // stranded: fall back to a full replay, which needs no cursor to be correct.
            Err(err) if after_seq.is_some() => {
                tracing::warn!(
                    session_id,
                    %err,
                    "tail attach failed after fencing the resident stream; retrying with full history"
                );
                daemon_client.attach(&session_id, None).await?
            }
            Err(err) => return Err(err),
        };
        // A restarted daemon numbers sequences from scratch, so a cursor minted under the previous
        // epoch selects an arbitrary point in a foreign sequence space: the tail it returns can be
        // empty while carrying no gap, which would leave the pane blank. Re-request the full history
        // once the response reveals the epoch actually served.
        // A handover successor serves a new epoch but keeps the PTY lifetime and its sequence
        // space, so a cursor from the same incarnation still addresses the same bytes.
        if let Some(token) = token.as_ref() {
            if after_seq.is_some() && attachment.epoch != token.epoch && !token.continues_pty_lifetime() {
                attachment.stream_task.abort();
                attachment = daemon_client.attach(&session_id, None).await?;
            }
        }
        if attach_tuple.as_ref().is_some_and(|tuple| tuple.daemon_epoch != attachment.epoch.to_string()) {
            attachment.stream_task.abort();
            return Err(IpcError::from(NativeTerminalError::SessionDetached(session_id.clone())));
        }
        if let Err(err) = state.attach_daemon_attachment_with_bounds_client_and_token(
            &session_id,
            attachment,
            Some(app),
            logical_bounds,
            Some(daemon_client.inner().clone()),
            token,
        ) {
            return Err(IpcError::internal(err.to_string()));
        }
        Ok(())
    };

    // Keep timeout cancellation inside the reservation owner so rollback still runs.
    let result = tokio::time::timeout_at(deadline, attach_body).await.unwrap_or_else(|_| {
        Err(IpcError::new(
            IpcErrorCode::Timeout,
            "Terminal attachment timed out. Retry reconnecting this pane.",
        ).with_details(serde_json::json!({ "sessionId": session_id, "phase": "attach" })))
    });
    if result.is_err() && is_new_reservation {
        let active = state.session_attach_tuple(&session_id);
        if active == initial_attach_tuple || active.is_none() {
            state.close_session_fenced(&session_id, active.as_ref());
        }
    }
    result
}

fn replay_request_cursor(resident: Option<Option<u64>>, explicit: Option<String>) -> Option<u64> {
    match resident {
        Some(cursor) => cursor,
        None => explicit.and_then(|value| value.parse().ok()),
    }
}

fn font_metrics_for_bounds(bounds: LogicalBounds) -> CellMetrics {
    crate::native_terminal::renderer::font_manager::derived_cell_metrics_for_scale(bounds.scale_factor)
}

#[tauri::command]
pub async fn cmd_native_terminal_detach<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    attach_tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
) -> Result<(), IpcError> {
    if !state.detach_session_fenced(&session_id, attach_tuple.as_ref()) {
        return Err(IpcError::from(NativeTerminalError::SessionDetached(session_id)));
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_native_terminal_close<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    attach_tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
) -> Result<(), IpcError> {
    if !state.close_session_fenced(&session_id, attach_tuple.as_ref()) {
        return Err(IpcError::from(NativeTerminalError::SessionDetached(session_id)));
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_native_terminal_set_bounds<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    bounds: NativeTerminalLogicalRect,
    scale_factor: f64,
    remaining_ms: Option<u64>,
    attach_tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
) -> Result<NativeTerminalBoundsReceipt, IpcError> {
    let request = NativeTerminalBoundsRequest {
        session_id: session_id.clone(),
        bounds: LogicalBounds {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
            scale_factor,
        },
    };
    let window = app
        .get_window("main")
        .ok_or_else(|| IpcError::internal("Main Ferryx window is unavailable"))?;
    let mut updates = state
        .subscribe_session_update(&session_id)
        .map_err(IpcError::from)?;
    let mut detached = state
        .subscribe_session_detach(&session_id)
        .map_err(IpcError::from)?;
    // A native surface defers every frame to the GPU worker, so the render call itself can never
    // report presentation. Subscribe to the session's presentation receipts before the first
    // dispatch so a frame that finishes early cannot be missed, and let the GPU completion —
    // rejected inside the slot when its attachment is gone — be what ends the wait.
    let slot = state
        .session_snapshot_slot(&session_id)
        .ok_or_else(|| IpcError::from(NativeTerminalError::SessionDetached(session_id.clone())))?;
    let mut presentations = slot.subscribe_presentations();
    presentations.borrow_and_update();
    let presentation_deadline = tokio::time::Instant::now()
        + remaining_ms.map_or(NATIVE_TERMINAL_PRESENTATION_WAIT, |remaining| {
            std::time::Duration::from_millis(remaining.min(2_000))
        });
    let expected_tuple = attach_tuple.or_else(|| state.session_attach_tuple(&session_id));
    let mut initial_request = Some(request);
    loop {
        check_native_surface_presentation()?;
        let state_inner = state.inner().clone();
        let surface_window = window.clone();
        let (sender, receiver) = oneshot::channel();
        let session_id_clone = session_id.clone();
        let request = initial_request.take();
        let render_tuple = expected_tuple.clone();
        // Every render path publishes the frame it just computed, so the frame carrying this
        // geometry is strictly newer than the generation observed here. Capturing it before
        // dispatch means a paint that completes while the render is still on the main thread
        // still counts, and a paint of an older frame never does.
        let awaited_generation = slot.generation();
        window
            .run_on_main_thread(move || {
                if tokio::time::Instant::now() >= presentation_deadline {
                    let _ = sender.send(Err(IpcError::new(IpcErrorCode::Timeout,
                        "Native presentation deadline exceeded; retry this pane")));
                    return;
                }
                let result = match request {
                    Some(request) => state_inner.render_fenced(&surface_window, request, render_tuple.as_ref()),
                    None => state_inner.render_current_fenced(&surface_window, &session_id_clone, render_tuple.as_ref()),
                }
                .map_err(|error| set_bounds_render_error(&state_inner, &session_id_clone, render_tuple.as_ref(), error))
                .map(|receipt| into_bound_ipc_receipt(&state_inner, session_id_clone, receipt));
                let _ = sender.send(result);
            })
            .map_err(|error| {
                IpcError::internal(format!(
                    "Could not dispatch native terminal render: {error}"
                ))
            })?;
        let receipt = tokio::time::timeout_at(presentation_deadline, receiver).await
            .map_err(|_| IpcError::new(IpcErrorCode::Timeout, "Native presentation deadline exceeded; retry this pane"))?
            .map_err(|_| {
            IpcError::internal("Main thread stopped before native terminal render completed")
        })??;
        if state.session_attach_tuple(&session_id) != expected_tuple {
            return Err(set_bounds_tuple_moved_error(state.inner(), session_id, expected_tuple.as_ref()));
        }
        if !receipt.render_deferred {
            return Ok(receipt);
        }
        if let Some(presented) = presented_after(&slot, &mut presentations, awaited_generation) {
            return Ok(into_bound_ipc_receipt(state.inner(), session_id, presented));
        }
        loop {
            tokio::select! {
                biased;
                _ = detached.changed() => {
                    return Err(IpcError::from(NativeTerminalError::SessionDetached(session_id)));
                }
                result = presentations.changed() => {
                    result.map_err(|_| {
                        IpcError::from(NativeTerminalError::SessionDetached(session_id.clone()))
                    })?;
                    if state.session_attach_tuple(&session_id) != expected_tuple {
                        return Err(set_bounds_tuple_moved_error(state.inner(), session_id, expected_tuple.as_ref()));
                    }
                    if let Some(presented) = presented_after(&slot, &mut presentations, awaited_generation) {
                        return Ok(into_bound_ipc_receipt(state.inner(), session_id, presented));
                    }
                }
                // The geometry is already applied; stop holding the caller's newer bounds back.
                _ = tokio::time::sleep_until(presentation_deadline) => {
                    check_native_surface_presentation()?;
                    return Ok(receipt);
                }
                // Terminal state moved on without a presentation (a suspended surface stops
                // retrying); re-render so the pane cannot get stuck on an obsolete frame.
                result = updates.changed() => {
                    result.map_err(|_| {
                        IpcError::from(NativeTerminalError::SessionDetached(session_id.clone()))
                    })?;
                    break;
                }
            }
        }
    }
}

/// True when the surface is still attached but bound to a different tuple than the request
/// carried: the pane rotated its attach tuple while this request was in flight.
fn attach_tuple_is_stale(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    expected: Option<&crate::daemon::protocol::PaneAttachTuple>,
) -> bool {
    let Some(expected) = expected else { return false };
    state
        .session_attach_tuple(session_id)
        .is_some_and(|active| &active != expected)
}

/// Error for a set-bounds whose tuple no longer matches the surface after dispatch. A rotated
/// tuple is reported as `stale_tuple` so the frontend re-sends with its current tuple; a
/// surface that is gone stays the benign detached error.
fn set_bounds_tuple_moved_error(
    state: &NativeTerminalSurfaceHostState,
    session_id: String,
    expected: Option<&crate::daemon::protocol::PaneAttachTuple>,
) -> IpcError {
    if attach_tuple_is_stale(state, &session_id, expected) {
        IpcError::stale_attach_tuple(&session_id)
    } else {
        IpcError::from(NativeTerminalError::SessionDetached(session_id))
    }
}

/// Maps a fenced render failure. The surface host answers a tuple mismatch with
/// `SessionDetached`; tell a rotated tuple apart from a released surface here.
fn set_bounds_render_error(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    expected: Option<&crate::daemon::protocol::PaneAttachTuple>,
    error: NativeTerminalError,
) -> IpcError {
    if matches!(error, NativeTerminalError::SessionDetached(_))
        && attach_tuple_is_stale(state, session_id, expected)
    {
        IpcError::stale_attach_tuple(session_id)
    } else {
        IpcError::from(error)
    }
}

/// Receipt of a presented frame newer than `awaited_generation`, if one already landed for the
/// attachment that is live right now.
///
/// Both ends are checked: `publish_presentation` rejects a completion whose attachment is already
/// gone, and this rejects a value that was valid when published but whose attachment has since
/// been retired, so a reader can never inherit the previous attachment's paint.
///
/// Any frame newer than the generation captured before dispatch is proof enough. Geometry is
/// applied synchronously inside the render call, so every later frame - including one the output
/// pump published - already carries this request's layout.
fn presented_after(
    slot: &SnapshotSlot,
    presentations: &mut tokio::sync::watch::Receiver<Option<PresentedFrame>>,
    awaited_generation: u64,
) -> Option<NativeTerminalSurfaceReceipt> {
    (*presentations.borrow_and_update())
        .filter(|presented| {
            presented.generation > awaited_generation
                && slot.is_attached_with_epoch(presented.attachment_epoch)
        })
        .map(|presented| presented.receipt)
}

#[tauri::command]
pub async fn cmd_native_terminal_set_focus<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    focused: bool,
) -> Result<NativeTerminalBoundsReceipt, IpcError> {
    let window = match app.get_window("main") {
        Some(window) => window,
        None => return Err(IpcError::internal("Main Ferryx window is unavailable")),
    };
    let state_inner = state.inner().clone();
    let surface_window = window.clone();
    let (sender, receiver) = oneshot::channel();
    let session_id_clone = session_id.clone();
    if let Err(error) = window.run_on_main_thread(move || {
        let result = state_inner
            .set_focus(&surface_window, &session_id_clone, focused)
            .map(|receipt| into_bound_ipc_receipt(&state_inner, session_id_clone.clone(), receipt))
            .map_err(|error| IpcError::internal(error.to_string()));
        let _ = sender.send(result);
    }) {
        return Err(IpcError::internal(format!(
            "Could not dispatch native terminal focus: {error}"
        )));
    }
    match receiver.await {
        Ok(receipt_result) => receipt_result,
        Err(_) => Err(IpcError::internal(
            "Main thread stopped before native terminal focus completed",
        )),
    }
}

#[tauri::command]
pub async fn cmd_native_terminal_set_preedit<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    preedit: Option<String>,
) -> Result<NativeTerminalBoundsReceipt, IpcError> {
    let window = match app.get_window("main") {
        Some(window) => window,
        None => return Err(IpcError::internal("Main Ferryx window is unavailable")),
    };
    let state_inner = state.inner().clone();
    let surface_window = window.clone();
    let (sender, receiver) = oneshot::channel();
    let session_id_clone = session_id.clone();
    if let Err(error) = window.run_on_main_thread(move || {
        let result = state_inner
            .set_preedit(&surface_window, &session_id_clone, preedit)
            .map(|receipt| into_bound_ipc_receipt(&state_inner, session_id_clone.clone(), receipt))
            .map_err(|error| IpcError::internal(error.to_string()));
        let _ = sender.send(result);
    }) {
        return Err(IpcError::internal(format!(
            "Could not dispatch native terminal preedit: {error}"
        )));
    }
    match receiver.await {
        Ok(receipt_result) => receipt_result,
        Err(_) => Err(IpcError::internal(
            "Main thread stopped before native terminal preedit completed",
        )),
    }
}

/// Encode input only for a daemon-backed native session that is still attached.
///
/// `NativeTerminalSurfaceHostState::encode_input` supports standalone/local prototype
/// callers by lazily creating a VT state. The production IPC boundary must not use that
/// fallback: after a lifecycle detach it would allow PTY writes while the daemon output
/// pump and compositor owner are gone, making typed bytes look silently lost.
///
/// A detached session outlives its surface so a backgrounded agent keeps streaming, so session
/// existence alone does not prove a pane owns it. `ensure_surface_attached` is the predicate that
/// distinguishes the two.
pub fn encode_attached_native_input(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    input: &NativeTerminalInput,
) -> Result<Vec<u8>, IpcError> {
    state
        .encode_attached_input(session_id, input)
        .map_err(|error| {
            IpcError::from(error).with_details(serde_json::json!({ "inputWritten": false }))
        })
}

fn require_attached_surface(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
) -> Result<(), NativeTerminalError> {
    state.ensure_surface_attached(session_id)
}

/// Encode clipboard paste text for a native session only if currently attached.
pub fn encode_attached_native_paste(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    text: &str,
) -> Result<Vec<u8>, NativeTerminalError> {
    require_attached_surface(state, session_id)?;
    state.with_session_terminal_and_context(session_id, |term, bracketed_effective| {
        let is_multiline = text.contains('\n') || text.contains('\r');
        let override_val = if bracketed_effective || is_multiline {
            Some(true)
        } else {
            None
        };
        let bytes = term.encode_paste_with_bracketed_override(text, override_val)?;
        if !bytes.is_empty() {
            let _ = term.scroll_viewport(crate::native_terminal::ScrollViewport::Bottom);
        }
        Ok(bytes)
    })
}

/// Encode a mouse event for a native session only if currently attached.
pub fn encode_attached_native_mouse(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    event: &MouseEvent,
) -> Result<Vec<u8>, NativeTerminalError> {
    require_attached_surface(state, session_id)?;
    state.with_session_terminal(session_id, |term| term.encode_mouse(event))
}

pub fn select_attached_native_terminal_with_mouse(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    event: &MouseEvent,
) -> Result<(), NativeTerminalError> {
    require_attached_surface(state, session_id)?;
    state.with_session_terminal(session_id, |term| term.handle_mouse_gesture(event))
}

/// Scroll visible viewport for an attached native session.
pub fn scroll_attached_native_terminal(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    behavior: ScrollViewport,
) -> Result<(), NativeTerminalError> {
    state.scroll_attached_terminal(session_id, behavior)
}

pub fn scrollbar_for_attached_native_terminal(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
) -> Result<NativeTerminalScrollbarReceipt, NativeTerminalError> {
    require_attached_surface(state, session_id)?;
    state.with_session_terminal(session_id, |term| {
        let scrollbar = term.scrollbar()?;
        Ok(NativeTerminalScrollbarReceipt {
            total: scrollbar.total,
            offset: scrollbar.offset,
            len: scrollbar.len,
        })
    })
}

/// Perform grid selection on an attached native session.
pub fn select_attached_native_terminal(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    mode: &NativeTerminalSelectMode,
) -> Result<NativeTerminalSelectionReceipt, NativeTerminalError> {
    require_attached_surface(state, session_id)?;
    state.with_session_terminal(session_id, |term| {
        let selection_res = match mode {
            NativeTerminalSelectMode::All => term.select_all(),
            NativeTerminalSelectMode::Word { col, row } => term.select_word_at(*col, *row),
            NativeTerminalSelectMode::Line { row } => term.select_line_at(*row),
            NativeTerminalSelectMode::Range {
                start_col,
                start_row,
                end_col,
                end_row,
                rectangle,
            } => term.select_range(
                (*start_col, *start_row),
                (*end_col, *end_row),
                *rectangle,
            ),
            NativeTerminalSelectMode::None => term.clear_selection(),
        };
        match selection_res {
            Ok(()) => {}
            Err(NativeTerminalError::NoValue) => {}
            Err(err) => return Err(err),
        }
        let text = term.selection_text()?;
        let range = term
            .selection_range()?
            .map(
                |(start_col, start_row, end_col, end_row)| NativeTerminalSelectionRange {
                    start_col,
                    start_row,
                    end_col,
                    end_row,
                },
            );
        Ok(NativeTerminalSelectionReceipt { text, range })
    })
}

/// Copy active selection text from an attached native session.
pub fn copy_attached_native_selection(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
) -> Result<String, NativeTerminalError> {
    require_attached_surface(state, session_id)?;
    state.with_session_terminal(session_id, |term| {
        let text = term.selection_text()?.unwrap_or_default();
        Ok(text)
    })
}

/// PTY cursor plus viewport top, in screen rows, for an attached native session.
pub fn cursor_for_attached_native_terminal(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
) -> Result<NativeTerminalCursorReceipt, NativeTerminalError> {
    require_attached_surface(state, session_id)?;
    state.with_session_terminal(session_id, |term| {
        let (col, row) = term.cursor_position()?;
        let active_top = term.total_rows()?.saturating_sub(usize::from(term.rows()?)) as u64;
        Ok(NativeTerminalCursorReceipt {
            col,
            row,
            screen_row: active_top + u64::from(row),
            viewport_top_row: term.scrollbar()?.offset,
            scrollback_pruned_rows: term.scrollback_pruned_rows(),
        })
    })
}

/// Search full-screen text grid for an attached native session.
pub fn search_attached_native_terminal(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    query: &str,
    case_sensitive: bool,
) -> Result<NativeTerminalSearchResult, NativeTerminalError> {
    require_attached_surface(state, session_id)?;
    state.with_session_terminal(session_id, |term| {
        let raw_matches = term.search_grid(query, case_sensitive)?;
        let matches = raw_matches
            .into_iter()
            .map(|(row, start_col, end_col)| NativeTerminalSearchMatch {
                row,
                start_col,
                end_col,
            })
            .collect::<Vec<_>>();
        let total_matches = matches.len();
        Ok(NativeTerminalSearchResult {
            matches,
            total_matches,
        })
    })
}

pub fn line_text_attached_native_terminal(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    col: u16,
    row: u16,
) -> Result<NativeTerminalLineReceipt, NativeTerminalError> {
    require_attached_surface(state, session_id)?;
    state.with_session_terminal(session_id, |term| {
        let text = term.line_text_at(col, row)?;
        Ok(NativeTerminalLineReceipt { text, col, row })
    })
}

/// Query whether mouse tracking is enabled on an attached native session.
pub fn mouse_tracking_enabled_for_attached_session(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
) -> Result<bool, NativeTerminalError> {
    state.with_session_terminal(session_id, |term| term.mouse_tracking_enabled())
}

pub async fn dispatch_native_terminal_receipt<R: Runtime>(
    app: &AppHandle<R>,
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
) -> Result<NativeTerminalBoundsReceipt, IpcError> {
    let window = match app.get_window("main") {
        Some(window) => window,
        None => return Err(IpcError::internal("Main Ferryx window is unavailable")),
    };
    let state_inner = state.clone();
    let surface_window = window.clone();
    let (sender, receiver) = oneshot::channel();
    let session_id_clone = session_id.to_string();
    if let Err(error) = crate::native_terminal::surface_host::dispatch_main_thread_receipt(
        &window,
        move || {
            let result = state_inner
                .get_receipt(&surface_window, &session_id_clone)
                .map(|receipt| into_bound_ipc_receipt(&state_inner, session_id_clone.clone(), receipt))
                .map_err(|error| IpcError::internal(error.to_string()));
            let _ = sender.send(result);
        },
    ) {
        let degraded = state.degraded_receipt(session_id);
        let degraded_receipt = into_bound_ipc_receipt(state, session_id.to_string(), degraded);
        let mut details_map = match serde_json::to_value(&degraded_receipt) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        };
        details_map.insert("inputWritten".to_string(), serde_json::Value::Bool(true));
        details_map.insert(
            "receipt".to_string(),
            serde_json::to_value(&degraded_receipt).unwrap_or_default(),
        );
        return Err(IpcError::internal(format!(
            "Could not dispatch native terminal input receipt: {error}"
        ))
        .with_details(serde_json::Value::Object(details_map)));
    }
    match tokio::time::timeout(NATIVE_TERMINAL_RECEIPT_TIMEOUT, receiver).await {
        Ok(Ok(receipt_result)) => receipt_result,
        Ok(Err(_)) => {
            let degraded = state.degraded_receipt(session_id);
            let degraded_receipt = into_bound_ipc_receipt(state, session_id.to_string(), degraded);
            let mut details_map = match serde_json::to_value(&degraded_receipt) {
                Ok(serde_json::Value::Object(map)) => map,
                _ => serde_json::Map::new(),
            };
            details_map.insert("inputWritten".to_string(), serde_json::Value::Bool(true));
            details_map.insert(
                "receipt".to_string(),
                serde_json::to_value(&degraded_receipt).unwrap_or_default(),
            );
            Err(IpcError::internal(
                "Main thread stopped before native terminal input completed",
            )
            .with_details(serde_json::Value::Object(details_map)))
        }
        Err(_) => {
            let degraded = state.degraded_receipt(session_id);
            let degraded_receipt = into_bound_ipc_receipt(state, session_id.to_string(), degraded);
            let mut details_map = match serde_json::to_value(&degraded_receipt) {
                Ok(serde_json::Value::Object(map)) => map,
                _ => serde_json::Map::new(),
            };
            details_map.insert("inputWritten".to_string(), serde_json::Value::Bool(true));
            details_map.insert(
                "receipt".to_string(),
                serde_json::to_value(&degraded_receipt).unwrap_or_default(),
            );
            Err(IpcError::new(
                IpcErrorCode::Timeout,
                "Main thread rendezvous timed out awaiting native terminal receipt",
            )
            .with_details(serde_json::Value::Object(details_map)))
        }
    }
}

pub async fn send_native_terminal_input_with_writer<R: Runtime, F, Fut>(
    app: &AppHandle<R>,
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    input: &NativeTerminalInput,
    write_op: F,
) -> Result<(), IpcError>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = Result<(), IpcError>>,
{
    let bytes = encode_attached_native_input(state, session_id, input)?;
    write_op(bytes).await?;
    // Keystroke parity with standard terminals (ghostty
    // scroll-to-bottom.keystroke=true): typing while the viewport is
    // scrolled up snaps back to the live edge so the echoed input is
    // visible. The write above already succeeded, so a snap failure is
    // cosmetic and must not fail the command.
    if let Err(err) = scroll_attached_native_terminal(state, session_id, ScrollViewport::Bottom) {
        if !matches!(err, NativeTerminalError::SessionDetached(_)) {
            tracing::warn!(
                session_id,
                %err,
                "Failed to snap native terminal viewport to bottom after input"
            );
        }
    } else {
        state.emit_scrollbar_if_changed(Some(app), session_id);
    }

    schedule_native_terminal_input_receipt(app, state, session_id);
    Ok(())
}

/// Delivers the IME-anchor receipt for a write that already landed, out of band on
/// `native_terminal_input_receipt`. Shared by keystrokes and pastes so neither holds the
/// caller's per-session input lane open for the main-thread rendezvous.
fn schedule_native_terminal_input_receipt<R: Runtime>(
    app: &AppHandle<R>,
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
) {
    // The receipt only repositions the IME candidate window. Awaiting it here held the caller's
    // per-session input queue open for the whole main-thread rendezvous, so the NEXT keystroke
    // could not start until this one's receipt arrived. Deliver it out of band instead: the write
    // is already durable, and the anchor updates a moment later.
    //
    // Coalesced per session: one delivery task runs at a time and a burst of keystrokes only marks
    // it dirty, so it loops once more and reads the latest receipt instead of queueing one main
    // thread rendezvous and emit per keystroke. One task per session keeps emits in order.
    {
        let mut pending = PENDING_INPUT_RECEIPTS.lock();
        let pending = pending.get_or_insert_with(std::collections::HashMap::new);
        if let Some(dirty) = pending.get_mut(session_id) {
            *dirty = true;
            return;
        }
        pending.insert(session_id.to_string(), true);
    }
    let receipt_app = app.clone();
    let receipt_state = state.clone();
    let receipt_session = session_id.to_string();
    tauri::async_runtime::spawn(async move {
        loop {
            {
                let mut pending = PENDING_INPUT_RECEIPTS.lock();
                let pending = pending.get_or_insert_with(std::collections::HashMap::new);
                match pending.get_mut(&receipt_session) {
                    Some(dirty) if *dirty => *dirty = false,
                    _ => {
                        pending.remove(&receipt_session);
                        break;
                    }
                }
            }
            match dispatch_native_terminal_receipt(&receipt_app, &receipt_state, &receipt_session)
                .await
            {
                Ok(receipt) => {
                    let _ = receipt_app.emit(NATIVE_TERMINAL_INPUT_RECEIPT_EVENT, receipt);
                }
                Err(error) => {
                    // A missed anchor update is cosmetic; the keystroke already landed.
                    tracing::debug!(
                        session_id = %receipt_session,
                        %error,
                        "Native terminal input receipt unavailable; IME anchor keeps its last value"
                    );
                }
            }
        }
    });
}

/// Sessions with an input-receipt delivery task in flight; the flag is true when another
/// keystroke landed since the task last read the receipt.
static PENDING_INPUT_RECEIPTS: parking_lot::Mutex<Option<std::collections::HashMap<String, bool>>> =
    parking_lot::Mutex::new(None);

pub async fn send_native_terminal_input_with_stage_logging<R: Runtime, F, Fut>(
    app: &AppHandle<R>,
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    input: &NativeTerminalInput,
    generation: Option<u64>,
    request_id: Option<String>,
    write_op: F,
) -> Result<(), IpcError>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = Result<(), IpcError>>,
{
    let start_instant = std::time::Instant::now();
    let started_at_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs_f64() * 1000.0);
    let op_id = request_id.clone();
    crate::ipc::debug::log_native_switch_debug(serde_json::json!({
        "event": "terminal.surface.input.stage.backend_write_start",
        "details": {
            "operationId": op_id,
            "sessionId": session_id,
            "generation": generation,
            "startedAt": started_at_unix_ms,
        }
    }));
    // Task 3 (local-split-qa): private QA barrier hook on the REAL write
    // stage. The channel exists only when the pane-liveness runner installed
    // a task-owned barrier dir through the inherited env; default builds and
    // normal runs never reach this code.
    #[cfg(feature = "local-split-qa")]
    let qa_write_release_outcome = match crate::ipc::qa_barrier::active_channel() {
        Some(channel) => {
            crate::ipc::qa_barrier::hold_backend_write_barrier(
                &channel,
                state,
                session_id,
                request_id.as_deref(),
            )
            .await
        }
        None => None,
    };
    let write_result = send_native_terminal_input_with_writer(
        app,
        state,
        session_id,
        input,
        write_op,
    )
    .await;
    let duration_ms = start_instant.elapsed().as_secs_f64() * 1000.0;
    match &write_result {
        Ok(()) => {
            crate::ipc::debug::log_native_switch_debug(serde_json::json!({
                "event": "terminal.surface.input.stage.backend_write",
                "details": {
                    "operationId": op_id,
                    "sessionId": session_id,
                    "durationMs": duration_ms,
                    "success": true,
                    "errorCode": None::<String>,
                }
            }));
        }
        Err(err) => {
            let code_str = serde_json::to_value(&err.code)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| format!("{:?}", err.code));
            crate::ipc::debug::log_native_switch_debug(serde_json::json!({
                "event": "terminal.surface.input.stage.backend_write",
                "details": {
                    "operationId": op_id,
                    "sessionId": session_id,
                    "durationMs": duration_ms,
                    "success": false,
                    "errorCode": Some(crate::ipc::debug::normalize_error_code(&code_str)),
                }
            }));
        }
    }
    // Task 3 (local-split-qa): settle with a FRESH real collector snapshot
    // plus actual stage-progress evidence. The verdict is never forced - an
    // Unknown recovery reports evidenceMissing with the missing-field list.
    #[cfg(feature = "local-split-qa")]
    if let Some(channel) = crate::ipc::qa_barrier::active_channel() {
        crate::ipc::qa_barrier::settle_backend_write_barrier(
            &channel,
            state,
            session_id,
            request_id.as_deref(),
            qa_write_release_outcome,
            write_result.is_ok(),
            duration_ms,
        )
        .await;
    }
    write_result
}

#[tauri::command]
pub async fn cmd_native_terminal_send_input<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    input: NativeTerminalInput,
    generation: Option<u64>,
    request_id: Option<String>,
) -> Result<(), IpcError> {
    let write_session_id = session_id.clone();
    send_native_terminal_input_with_stage_logging(
        &app,
        state.inner(),
        &session_id,
        &input,
        generation,
        request_id,
        |bytes| async move {
            if generation.is_none() && is_paste_interrupting_control(&bytes) {
                let cancel_client = Arc::clone(daemon_client.inner());
                let write_client = Arc::clone(daemon_client.inner());
                let cancel_session = write_session_id.clone();
                return write_control_past_pending_paste(
                    &write_session_id,
                    bytes,
                    |paste_id| async move { cancel_client.cancel_paste(&cancel_session, &paste_id).await },
                    |bytes| {
                        let session_id = write_session_id.clone();
                        async move { write_client.write_terminal(&session_id, bytes).await }
                    },
                )
                .await;
            }
            daemon_client
                .write_terminal_at_generation(&write_session_id, generation, bytes)
                .await
        },
    )
    .await
}

#[tauri::command]
pub fn cmd_native_terminal_pane_liveness(
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
) -> Result<Option<crate::ipc::debug::PaneLivenessSnapshot>, IpcError> {
    Ok(state.session_liveness_observation(&session_id))
}

/// Wheel context is pane-local logical pixels, matching the mouse IPC contract.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalWheelContext {
    pub position: MousePosition,
    pub modifiers: KeyModifiers,
}

/// Shared command policy seam. Layout comes from the attached surface, never the client.
pub fn native_scroll_outcome<T: TerminalEngine>(
    term: &T,
    bounds: Option<&LogicalBounds>,
    cell_metrics: Option<&CellMetrics>,
    behavior: &NativeTerminalScrollBehavior,
    wheel: Option<&NativeTerminalWheelContext>,
) -> Result<TerminalWheelOutcome, NativeTerminalError> {
    match behavior {
        NativeTerminalScrollBehavior::Delta { rows } => {
            let (lx, ly, modifiers) = match wheel {
                Some(wheel) => {
                    if !wheel.position.x.is_finite() || !wheel.position.y.is_finite() {
                        return Err(NativeTerminalError::InvalidValue(
                            "Wheel position must be finite".to_string(),
                        ));
                    }
                    let (x, y) = bounds.map(|b| (b.x, b.y)).unwrap_or((0.0, 0.0));
                    (
                        x + f64::from(wheel.position.x),
                        y + f64::from(wheel.position.y),
                        wheel.modifiers,
                    )
                }
                None => {
                    let (x, y) = bounds
                        .map(|b| (b.x + b.width / 2.0, b.y + b.height / 2.0))
                        .unwrap_or((0.0, 0.0));
                    (x, y, KeyModifiers::default())
                }
            };
            let rows = (*rows).clamp(isize::from(i16::MIN), isize::from(i16::MAX)) as i16;
            crate::native_terminal::compute_wheel_outcome(
                term,
                bounds,
                cell_metrics,
                lx,
                ly,
                rows,
                modifiers,
            )
        }
        _ => Ok(TerminalWheelOutcome::ScrollViewport(
            behavior.to_scroll_viewport(),
        )),
    }
}

#[tauri::command]
pub async fn cmd_native_terminal_scroll<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
    behavior: NativeTerminalScrollBehavior,
    wheel: Option<NativeTerminalWheelContext>,
    generation: Option<u64>,
) -> Result<NativeTerminalBoundsReceipt, IpcError> {
    require_attached_surface(state.inner(), &session_id)
        .map_err(|err| IpcError::internal(err.to_string()))?;

    let bounds = state.session_logical_bounds(&session_id);
    let cell_metrics = state.session_cell_metrics(&session_id);
    let outcome = state
        .with_session_terminal(&session_id, |term| {
            native_scroll_outcome(
                term,
                bounds.as_ref(),
                cell_metrics.as_ref(),
                &behavior,
                wheel.as_ref(),
            )
        })
        .map_err(|err| IpcError::internal(err.to_string()))?;

    let mut wrote_pty = false;
    match outcome {
        TerminalWheelOutcome::WritePty(bytes) => {
            daemon_client
                .write_terminal_at_generation(&session_id, generation, bytes)
                .await?;
            wrote_pty = true;
        }
        TerminalWheelOutcome::ScrollViewport(v_behavior) => {
            if let Err(err) =
                scroll_attached_native_terminal(state.inner(), &session_id, v_behavior)
            {
                return Err(IpcError::internal(err.to_string()));
            }
        }
        TerminalWheelOutcome::None => {}
    }

    // A post-write receipt/render failure must still report whether the PTY
    // write itself succeeded, matching send_input and paste semantics. The
    // view-only scroll path never touches the PTY, so it explicitly reports
    // inputWritten: false instead of omitting the field.
    let input_written_details = if wrote_pty {
        serde_json::json!({ "inputWritten": true })
    } else {
        serde_json::json!({ "inputWritten": false })
    };
    let window = match app.get_window("main") {
        Some(window) => window,
        None => {
            return Err(
                IpcError::internal("Main Ferryx window is unavailable")
                    .with_details(input_written_details.clone()),
            )
        }
    };
    let state_inner = state.inner().clone();
    let surface_window = window.clone();
    let (sender, receiver) = oneshot::channel();
    let session_id_clone = session_id.clone();
    let bounds = state.session_logical_bounds(&session_id);

    if let Err(error) = window.run_on_main_thread(move || {
        let result = match bounds {
            Some(logical_bounds) => state_inner
                .render(
                    &surface_window,
                    NativeTerminalBoundsRequest {
                        session_id: session_id_clone.clone(),
                        bounds: logical_bounds,
                    },
                )
                .map(|receipt| into_bound_ipc_receipt(&state_inner, session_id_clone, receipt))
                .map_err(|error| IpcError::internal(error.to_string())),
            None => state_inner
                .get_receipt(&surface_window, &session_id_clone)
                .map(|receipt| into_bound_ipc_receipt(&state_inner, session_id_clone, receipt))
                .map_err(|error| IpcError::internal(error.to_string())),
        };
        let _ = sender.send(result);
    }) {
        return Err(IpcError::internal(format!(
            "Could not dispatch native terminal scroll render: {error}"
        ))
        .with_details(input_written_details.clone()));
    }

    match receiver.await {
        Ok(receipt_result) => receipt_result
            .map_err(|error| error.with_details(input_written_details.clone())),
        Err(_) => Err(IpcError::internal(
            "Main thread stopped before native terminal scroll completed",
        )
        .with_details(input_written_details)),
    }
}

#[tauri::command]
pub async fn cmd_native_terminal_scrollbar(
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
) -> Result<NativeTerminalScrollbarReceipt, IpcError> {
    scrollbar_for_attached_native_terminal(state.inner(), &session_id)
        .map_err(|error| IpcError::internal(error.to_string()))
}

#[tauri::command]
pub async fn cmd_native_terminal_set_scrollbar_overlay<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    visible: bool,
) -> Result<(), IpcError> {
    if let Err(err) = state.set_scrollbar_overlay_visible(&session_id, visible) {
        return Err(IpcError::internal(err.to_string()));
    }

    let window = match app.get_window("main") {
        Some(window) => window,
        None => return Ok(()),
    };
    let state_inner = state.inner().clone();
    let surface_window = window.clone();
    let (sender, receiver) = oneshot::channel();
    let session_id_clone = session_id.clone();
    let bounds = state.session_logical_bounds(&session_id);

    if let Err(error) = window.run_on_main_thread(move || {
        let result = match bounds {
            Some(logical_bounds) => state_inner
                .render(
                    &surface_window,
                    NativeTerminalBoundsRequest {
                        session_id: session_id_clone.clone(),
                        bounds: logical_bounds,
                    },
                )
                .map(|_| ())
                .map_err(|error| IpcError::internal(error.to_string())),
            None => Ok(()),
        };
        let _ = sender.send(result);
    }) {
        return Err(IpcError::internal(format!(
            "Could not dispatch native terminal scrollbar overlay render: {error}"
        )));
    }

    match receiver.await {
        Ok(res) => res,
        Err(_) => Err(IpcError::internal(
            "Main thread stopped before native terminal scrollbar overlay render completed",
        )),
    }
}

#[tauri::command]
pub async fn cmd_native_terminal_set_dom_overlay_input(
    state: State<'_, NativeTerminalSurfaceHostState>,
    active: bool,
) -> Result<(), IpcError> {
    state.set_dom_overlay_owns_input(active);
    Ok(())
}

#[tauri::command]
pub async fn cmd_native_terminal_set_attention_frame<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    attention: bool,
) -> Result<(), IpcError> {
    if let Err(err) = state.set_attention_frame(&session_id, attention) {
        return Err(IpcError::internal(err.to_string()));
    }

    let window = match app.get_window("main") {
        Some(window) => window,
        None => return Ok(()),
    };
    let state_inner = state.inner().clone();
    let surface_window = window.clone();
    let (sender, receiver) = oneshot::channel();
    let session_id_clone = session_id.clone();
    let bounds = state.session_logical_bounds(&session_id);

    if let Err(error) = window.run_on_main_thread(move || {
        let result = match bounds {
            Some(logical_bounds) => state_inner
                .render(
                    &surface_window,
                    NativeTerminalBoundsRequest {
                        session_id: session_id_clone.clone(),
                        bounds: logical_bounds,
                    },
                )
                .map(|_| ())
                .map_err(|error| IpcError::internal(error.to_string())),
            None => Ok(()),
        };
        let _ = sender.send(result);
    }) {
        return Err(IpcError::internal(format!(
            "Could not dispatch native terminal attention frame render: {error}"
        )));
    }

    match receiver.await {
        Ok(res) => res,
        Err(_) => Err(IpcError::internal(
            "Main thread stopped before native terminal attention frame render completed",
        )),
    }
}

#[tauri::command]
pub async fn cmd_native_terminal_select<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    mode: NativeTerminalSelectMode,
) -> Result<NativeTerminalSelectionReceipt, IpcError> {
    let receipt = select_attached_native_terminal(state.inner(), &session_id, &mode)
        .map_err(|err| IpcError::internal(err.to_string()))?;

    // Keyboard copy mode moves the selection without any PTY output, so nothing else
    // would present the new highlight; render a frame the way mouse selection does.
    if matches!(mode, NativeTerminalSelectMode::Range { .. }) {
        if let Some(window) = app.get_window("main") {
            let state_inner = state.inner().clone();
            let surface_window = window.clone();
            let (sender, receiver) = oneshot::channel();
            let session_id_clone = session_id.clone();
            if window
                .run_on_main_thread(move || {
                    if let Some(bounds) = state_inner.session_logical_bounds(&session_id_clone) {
                        let _ = state_inner.render(
                            &surface_window,
                            NativeTerminalBoundsRequest {
                                session_id: session_id_clone,
                                bounds,
                            },
                        );
                    }
                    let _ = sender.send(());
                })
                .is_ok()
            {
                let _ = receiver.await;
            }
        }
    }

    Ok(receipt)
}

#[tauri::command]
pub async fn cmd_native_terminal_cursor(
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
) -> Result<NativeTerminalCursorReceipt, IpcError> {
    cursor_for_attached_native_terminal(state.inner(), &session_id)
        .map_err(|err| IpcError::internal(err.to_string()))
}

#[tauri::command]
pub async fn cmd_native_terminal_copy_selection<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
) -> Result<String, IpcError> {
    let text = copy_attached_native_selection(state.inner(), &session_id)
        .map_err(|err| IpcError::internal(err.to_string()))?;

    #[cfg(target_os = "macos")]
    if !text.is_empty() {
        let (sender, receiver) = oneshot::channel();
        let text_clone = text.clone();
        if let Err(error) = app.run_on_main_thread(move || {
            let pasteboard = objc2_app_kit::NSPasteboard::generalPasteboard();
            let success = write_to_pasteboard(&pasteboard, &text_clone);
            let _ = sender.send(success);
        }) {
            return Err(IpcError::internal(format!(
                "Could not dispatch native clipboard write: {error}"
            )));
        }

        match receiver.await {
            Ok(true) => {}
            Ok(false) => {
                return Err(IpcError::internal(
                    "Failed to write selection to macOS pasteboard",
                ));
            }
            Err(_) => {
                return Err(IpcError::internal(
                    "Main thread stopped before native clipboard write completed",
                ));
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        if !text.is_empty() {
            let selection = text.clone();
            let written = tokio::task::spawn_blocking(move || write_native_clipboard(&selection))
                .await
                .map_err(|err| IpcError::internal(format!("Clipboard write task failed: {err}")))?;
            if !written {
                return Err(IpcError::internal(
                    "Failed to write selection to Windows clipboard",
                ));
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
        if !text.is_empty() {
            let selection = text.clone();
            let written = tokio::task::spawn_blocking(move || write_native_clipboard(&selection))
                .await
                .map_err(|err| IpcError::internal(format!("Clipboard write task failed: {err}")))?;
            if !written {
                return Err(IpcError::internal(
                    "Failed to write selection to Linux system clipboard (no Wayland/X11 clipboard helper accepted the selection)",
                ));
            }
        }
    }

    Ok(text)
}

/// Writes arbitrary UI text (toast errors, file paths, debug info) to the system clipboard.
///
/// WebView clipboard APIs refuse writes while a native terminal child view holds first
/// responder, so desktop copy actions go through the same native writer the terminal uses.
#[tauri::command]
pub async fn cmd_clipboard_write_text<R: Runtime>(
    app: AppHandle<R>,
    text: String,
) -> Result<(), IpcError> {
    if text.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "Clipboard text must not be empty",
        ));
    }

    #[cfg(target_os = "macos")]
    let written = {
        let (sender, receiver) = oneshot::channel();
        app.run_on_main_thread(move || {
            let pasteboard = objc2_app_kit::NSPasteboard::generalPasteboard();
            let _ = sender.send(write_to_pasteboard(&pasteboard, &text));
        })
        .map_err(|error| {
            IpcError::internal(format!("Could not dispatch native clipboard write: {error}"))
        })?;
        receiver.await.map_err(|_| {
            IpcError::internal("Main thread stopped before native clipboard write completed")
        })?
    };
    #[cfg(not(target_os = "macos"))]
    let written = {
        let _ = app;
        tokio::task::spawn_blocking(move || write_native_clipboard(&text))
            .await
            .map_err(|err| IpcError::internal(format!("Clipboard write task failed: {err}")))?
    };

    if written {
        Ok(())
    } else {
        Err(IpcError::internal(
            "No system clipboard writer accepted the text",
        ))
    }
}

#[tauri::command]
pub async fn cmd_native_terminal_paste<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    text: String,
    generation: Option<u64>,
) -> Result<(), IpcError> {
    let bytes = match encode_attached_native_paste(state.inner(), &session_id, &text) {
        Ok(bytes) => bytes,
        Err(err) => return Err(IpcError::internal(err.to_string())),
    };
    state.emit_scrollbar_if_changed(Some(&app), &session_id);
    let client = daemon_client.paste_client();
    let probe_client = client.clone();
    let identified = async move { generation.is_none() && probe_client.supports_paste_cancel().await };
    let begin_client = client.clone();
    let begin_session = session_id.clone();
    let chunk_client = client.clone();
    let chunk_session = session_id.clone();
    let plain_client = client.clone();
    let plain_session = session_id.clone();
    write_native_paste(
        &session_id,
        bytes,
        identified,
        || async move { begin_client.begin_paste(&begin_session).await },
        move |paste_id, chunk, final_chunk| {
            let client = chunk_client.clone();
            let session_id = chunk_session.clone();
            async move { client.write_paste_chunk(&session_id, &paste_id, chunk, final_chunk).await }
        },
        move |chunk| {
            let client = plain_client.clone();
            let session_id = plain_session.clone();
            async move {
                client
                    .write_terminal_at_generation(&session_id, generation, chunk)
                    .await
                    .map(|_| ())
            }
        },
    )
    .await?;

    // Receipt parity with keystrokes: the paste landed, so the IME anchor follows on
    // `native_terminal_input_receipt` instead of holding the input lane for the rendezvous.
    schedule_native_terminal_input_receipt(&app, state.inner(), &session_id);
    Ok(())
}

/// Writes one encoded paste. With `identified` (a local paste on a daemon serving
/// cancellation) every chunk carries a daemon-issued paste id, so a control key can stop it
/// server-side (see `write_control_past_pending_paste`). A session that cannot take an
/// identity - a paired-host or remote pane, or a session still served by an older daemon -
/// is refused by `begin`; the paste then takes the plain path, exactly as on a daemon without
/// cancellation, instead of failing outright.
pub(crate) async fn write_native_paste<B, BF, I, IF, P, PF>(
    session_id: &str,
    bytes: Vec<u8>,
    identified: impl std::future::Future<Output = bool>,
    begin: B,
    mut identified_chunk: I,
    plain_chunk: P,
) -> Result<(), IpcError>
where
    B: FnOnce() -> BF,
    BF: std::future::Future<Output = Result<String, IpcError>>,
    I: FnMut(String, Vec<u8>, bool) -> IF,
    IF: std::future::Future<Output = Result<(), IpcError>>,
    P: FnMut(Vec<u8>) -> PF,
    PF: std::future::Future<Output = Result<(), IpcError>>,
{
    // Registered before the first await: a control key pressed while the paste is still being
    // set up (capability probe, identity registration) stops the paste before any byte of it.
    let pending = PendingPasteGuard::register(session_id);
    let paste_id = if identified.await {
        match begin().await {
            Ok(paste_id) => Some(paste_id),
            Err(error) => {
                tracing::info!(%session_id, %error, "Paste identity refused; writing the paste without cancellation");
                None
            }
        }
    } else {
        None
    };
    let Some(paste_id) = paste_id else {
        if !pending.proceed() {
            return Err(paste_stopped_before_start());
        }
        return write_native_paste_chunked(bytes, plain_chunk).await;
    };
    let total = bytes.len();
    let chunks = total.div_ceil(NATIVE_PASTE_CHUNK_BYTES);
    let bracketed = bytes.starts_with(BRACKETED_PASTE_START) && bytes.ends_with(BRACKETED_PASTE_END);
    let Some(active) = pending.promote(paste_id, total as u64, bracketed) else {
        return Err(paste_stopped_before_start());
    };
    let paste_id = active.paste_id.clone();
    let mut index = 0usize;
    let mut plain_chunk = plain_chunk;
    // Once the daemon reports the paste unknown, no identified write can land; the framing
    // close that write_native_paste_chunked issues after a partial failure goes plain instead.
    let unknown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    write_native_paste_chunked(bytes, |chunk| {
        use futures_util::future::Either;
        use std::sync::atomic::Ordering;
        if unknown.load(Ordering::Acquire) {
            return Either::Right(plain_chunk(chunk));
        }
        index += 1;
        let write = identified_chunk(paste_id.clone(), chunk, index == chunks);
        let unknown = std::sync::Arc::clone(&unknown);
        Either::Left(async move {
            let result = write.await;
            if result.as_ref().is_err_and(is_paste_unknown) {
                unknown.store(true, Ordering::Release);
            }
            result
        })
    })
    .await
}

fn is_paste_unknown(error: &IpcError) -> bool {
    error.details.as_ref().and_then(|d| d.get("pasteUnknown")) == Some(&serde_json::Value::Bool(true))
}

/// A control key stopped this paste while it was still being set up: none of it was written.
fn paste_stopped_before_start() -> IpcError {
    IpcError::new(
        crate::ipc::IpcErrorCode::Custom(crate::daemon::protocol::PASTE_CANCELLED_CODE.into()),
        "Paste cancelled before it started",
    )
    .with_details(serde_json::json!({
        "pasteCancelled": true,
        "inputWritten": false,
        "bytesWritten": 0,
        "bytesWrittenExact": true,
        "partial": false,
    }))
}

/// Pastes larger than this are written as ordered chunks so one paste never monopolises the
/// session with a single huge write.
pub(crate) const NATIVE_PASTE_CHUNK_BYTES: usize = 64 * 1024;
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";

/// A local identified paste currently being written, per session.
#[derive(Debug, Clone)]
pub(crate) struct ActivePaste {
    pub(crate) paste_id: String,
    pub(crate) total: u64,
    pub(crate) bracketed: bool,
}

/// Per session: a paste still being set up, or one being written under a daemon paste id.
#[derive(Debug, Clone)]
enum PasteSlot {
    Pending { token: u64, stopped: bool },
    Active(ActivePaste),
}

static ACTIVE_NATIVE_PASTES: parking_lot::Mutex<Option<std::collections::HashMap<String, PasteSlot>>> =
    parking_lot::Mutex::new(None);

#[cfg(test)]
fn active_native_paste(session_id: &str) -> Option<ActivePaste> {
    match ACTIVE_NATIVE_PASTES.lock().as_ref()?.get(session_id)? {
        PasteSlot::Active(active) => Some(active.clone()),
        PasteSlot::Pending { .. } => None,
    }
}

/// What a control key finds: an active paste to cancel server-side, or nothing. A paste still
/// being set up is marked stopped here, under the same lock its start is decided under.
fn control_target(session_id: &str) -> Option<ActivePaste> {
    match ACTIVE_NATIVE_PASTES.lock().as_mut()?.get_mut(session_id)? {
        PasteSlot::Active(active) => Some(active.clone()),
        PasteSlot::Pending { stopped, .. } => {
            *stopped = true;
            None
        }
    }
}

/// A paste being set up. Dropped without starting, it unregisters itself if still its own.
pub(crate) struct PendingPasteGuard {
    session_id: String,
    token: u64,
}

impl PendingPasteGuard {
    fn register(session_id: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let token = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ACTIVE_NATIVE_PASTES
            .lock()
            .get_or_insert_with(std::collections::HashMap::new)
            .insert(session_id.to_string(), PasteSlot::Pending { token, stopped: false });
        Self { session_id: session_id.to_string(), token }
    }

    /// Whether this paste may start (no control key stopped it). Unregisters either way.
    fn proceed(self) -> bool {
        let mut map = ACTIVE_NATIVE_PASTES.lock();
        let Some(map) = map.as_mut() else { return true };
        match map.get(&self.session_id) {
            Some(PasteSlot::Pending { token, stopped }) if *token == self.token => {
                let stopped = *stopped;
                map.remove(&self.session_id);
                !stopped
            }
            _ => true,
        }
    }

    /// Becomes the active identified paste unless a control key already stopped it.
    fn promote(self, paste_id: String, total: u64, bracketed: bool) -> Option<ActivePasteGuard> {
        let mut map = ACTIVE_NATIVE_PASTES.lock();
        let map = map.get_or_insert_with(std::collections::HashMap::new);
        if matches!(map.get(&self.session_id), Some(PasteSlot::Pending { token, stopped: true }) if *token == self.token) {
            map.remove(&self.session_id);
            return None;
        }
        map.insert(
            self.session_id.clone(),
            PasteSlot::Active(ActivePaste { paste_id: paste_id.clone(), total, bracketed }),
        );
        Some(ActivePasteGuard { session_id: self.session_id.clone(), paste_id })
    }
}

impl Drop for PendingPasteGuard {
    fn drop(&mut self) {
        if let Some(map) = ACTIVE_NATIVE_PASTES.lock().as_mut() {
            if matches!(map.get(&self.session_id), Some(PasteSlot::Pending { token, .. }) if *token == self.token) {
                map.remove(&self.session_id);
            }
        }
    }
}

/// Registers a paste for its whole lifetime and unregisters it on drop, only if still its own.
pub(crate) struct ActivePasteGuard {
    session_id: String,
    pub(crate) paste_id: String,
}

impl ActivePasteGuard {
    #[cfg(test)]
    pub(crate) fn register(session_id: &str, total: u64, bracketed: bool) -> Self {
        let paste_id = uuid::Uuid::new_v4().to_string();
        Self::register_id(session_id, paste_id, total, bracketed)
    }

    #[cfg(test)]
    fn register_id(session_id: &str, paste_id: String, total: u64, bracketed: bool) -> Self {
        ACTIVE_NATIVE_PASTES
            .lock()
            .get_or_insert_with(std::collections::HashMap::new)
            .insert(session_id.to_string(), PasteSlot::Active(ActivePaste { paste_id: paste_id.clone(), total, bracketed }));
        Self { session_id: session_id.to_string(), paste_id }
    }
}

impl Drop for ActivePasteGuard {
    fn drop(&mut self) {
        if let Some(map) = ACTIVE_NATIVE_PASTES.lock().as_mut() {
            if matches!(map.get(&self.session_id), Some(PasteSlot::Active(active)) if active.paste_id == self.paste_id) {
                map.remove(&self.session_id);
            }
        }
    }
}

/// A single C0 control or DEL keystroke other than Tab/LF/CR (Ctrl+C, Ctrl+D, Ctrl+Z, Esc, ...).
/// These must not wait behind a pending paste; every other input stays ordinary, ordered data.
pub(crate) fn is_paste_interrupting_control(bytes: &[u8]) -> bool {
    matches!(bytes, [byte] if (*byte < 0x20 && !matches!(*byte, b'\t' | b'\n' | b'\r')) || *byte == 0x7f)
}

/// The exact bytes that finish the bracketed-paste framing after the kernel accepted
/// `accepted` bytes of `active`: the rest of a torn start marker plus the end marker, the end
/// marker alone, the rest of a torn end marker, or nothing when the paste was not bracketed,
/// never started, or already delivered its end marker. The paste body is never resumed.
pub(crate) fn bracket_completion(active: &ActivePaste, accepted: u64) -> Vec<u8> {
    let start = BRACKETED_PASTE_START.len() as u64;
    let end = BRACKETED_PASTE_END.len() as u64;
    if !active.bracketed || accepted == 0 || accepted >= active.total {
        return Vec::new();
    }
    if accepted < start {
        let mut bytes = BRACKETED_PASTE_START[accepted as usize..].to_vec();
        bytes.extend_from_slice(BRACKETED_PASTE_END);
        return bytes;
    }
    let end_from = active.total - end;
    if accepted <= end_from {
        return BRACKETED_PASTE_END.to_vec();
    }
    BRACKETED_PASTE_END[(accepted - end_from) as usize..].to_vec()
}

/// Delivers a control keystroke without waiting behind the session's pending paste: the paste
/// is cancelled server-side first (bounded by the daemon), its framing is closed exactly from
/// the daemon's accepted-byte count, and the control byte follows in the same ordinary write.
/// That write is the daemon's bounded write: if the kernel input queue is still full and the
/// current termios makes the control byte a signal character, the daemon does what the line
/// discipline would have done; any other byte fails within its bound with an honest
/// `bytesWritten` receipt. Without an active paste the bytes are written unchanged.
pub(crate) async fn write_control_past_pending_paste<C, CF, W, WF>(
    session_id: &str,
    bytes: Vec<u8>,
    cancel: C,
    write: W,
) -> Result<(), IpcError>
where
    C: FnOnce(String) -> CF,
    CF: std::future::Future<Output = Result<(bool, u64, bool), IpcError>>,
    W: FnOnce(Vec<u8>) -> WF,
    WF: std::future::Future<Output = Result<(), IpcError>>,
{
    let Some(active) = control_target(session_id) else {
        return write(bytes).await;
    };
    let (prefix, receipt) = match cancel(active.paste_id.clone()).await {
        Ok((found, accepted, true)) => (
            bracket_completion(&active, accepted),
            serde_json::json!({ "pasteCancelled": true, "pasteFound": found,
                "pasteBytesWritten": accepted, "pasteBytesWrittenExact": true }),
        ),
        // Unsettled: the count is only a lower bound, so the framing cannot be closed exactly.
        Ok((found, accepted, false)) => (
            Vec::new(),
            serde_json::json!({ "pasteCancelled": true, "pasteFound": found,
                "pasteBytesWritten": accepted, "pasteBytesWrittenExact": false }),
        ),
        Err(error) => {
            tracing::warn!(%session_id, %error, "Paste cancellation failed; control sent as ordinary input");
            (Vec::new(), serde_json::json!({ "pasteCancelled": false }))
        }
    };
    let prefix_len = prefix.len();
    let mut frame = prefix;
    frame.extend_from_slice(&bytes);
    write(frame).await.map_err(|mut error| {
        let details = error.details.get_or_insert_with(|| serde_json::json!({}));
        if let (Some(details), Some(receipt)) = (details.as_object_mut(), receipt.as_object()) {
            details.extend(receipt.clone());
            details.insert("bracketCloseBytes".into(), serde_json::json!(prefix_len));
        }
        error
    })
}

fn with_partial_paste_details(mut error: IpcError, bytes_written: usize, exact: bool) -> IpcError {
    let input_written = if bytes_written > 0 {
        serde_json::Value::Bool(true)
    } else {
        serde_json::Value::Null
    };
    if let Some(obj) = error.details.as_mut().and_then(|d| d.as_object_mut()) {
        obj.insert("inputWritten".to_string(), input_written);
        obj.insert("partial".to_string(), serde_json::Value::Bool(true));
        obj.insert("bytesWritten".to_string(), serde_json::json!(bytes_written));
        obj.insert("bytesWrittenExact".to_string(), serde_json::Value::Bool(exact));
        return error;
    }
    error.with_details(serde_json::json!({
        "inputWritten": input_written,
        "partial": true,
        "bytesWritten": bytes_written,
        "bytesWrittenExact": exact,
    }))
}

/// Writes an encoded paste (at most one bracketed-paste pair) as ordered chunks. A failure
/// after possible delivery closes the bracket with ESC[201~. Without a failed-chunk receipt,
/// bytesWritten is a lower bound, explicitly marked by bytesWrittenExact=false.
pub(crate) async fn write_native_paste_chunked<F, Fut>(
    bytes: Vec<u8>,
    mut write: F,
) -> Result<(), IpcError>
where
    F: FnMut(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = Result<(), IpcError>>,
{
    let bracketed =
        bytes.starts_with(BRACKETED_PASTE_START) && bytes.ends_with(BRACKETED_PASTE_END);
    let mut written = 0usize;
    for chunk in bytes.chunks(NATIVE_PASTE_CHUNK_BYTES) {
        if let Err(error) = write(chunk.to_vec()).await {
            let accepted = error.details.as_ref()
                .and_then(|details| details.get("bytesWritten"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|count| usize::try_from(count).ok());
            if let Some(accepted) = accepted {
                written += accepted;
            }
            let exact = error.details.as_ref().is_some_and(|details| {
                (accepted.is_some() && details.get("bytesWrittenExact") != Some(&serde_json::Value::Bool(false)))
                    || details.get("inputWritten") == Some(&serde_json::Value::Bool(false))
            });
            // A cancelled paste is closed by the control path that cancelled it, from the
            // daemon's exact count; appending ESC[201~ here would duplicate or misplace it.
            if error.details.as_ref().and_then(|d| d.get("pasteCancelled")) == Some(&serde_json::Value::Bool(true)) {
                return Err(with_partial_paste_details(error, written, exact));
            }
            if written == 0 && exact {
                return Err(error);
            }
            if bracketed {
                if let Err(close_error) = write(BRACKETED_PASTE_END.to_vec()).await {
                    tracing::warn!(%close_error, "Failed to close bracketed paste after partial write");
                }
            }
            return Err(with_partial_paste_details(error, written, exact));
        }
        written += chunk.len();
    }
    Ok(())
}

#[cfg(test)]
mod paste_chunk_tests {
    use super::*;

    fn run(bytes: Vec<u8>, fail_at: Option<usize>) -> (Result<(), IpcError>, Vec<Vec<u8>>) {
        let attempts = std::sync::Mutex::new(Vec::<Vec<u8>>::new());
        let result = tauri::async_runtime::block_on(write_native_paste_chunked(bytes, |chunk| {
            let index = {
                let mut attempts = attempts.lock().unwrap();
                attempts.push(chunk);
                attempts.len() - 1
            };
            let fail = fail_at == Some(index);
            async move {
                if fail {
                    Err(IpcError::internal("write failed"))
                } else {
                    Ok(())
                }
            }
        }));
        (result, attempts.into_inner().unwrap())
    }

    fn bracketed(len: usize) -> Vec<u8> {
        let mut bytes = BRACKETED_PASTE_START.to_vec();
        bytes.extend(std::iter::repeat_n(b'x', len));
        bytes.extend_from_slice(BRACKETED_PASTE_END);
        bytes
    }

    /// A paired-host or remote pane on a daemon serving cancellation: the daemon refuses a paste
    /// identity (SESSION_NOT_FOUND for a non-local PTY). The paste must still be delivered whole
    /// through the plain path, not fail with nothing written.
    #[tokio::test]
    async fn refused_paste_identity_still_delivers_the_whole_paste_plainly() {
        let bytes = bracketed(3 * NATIVE_PASTE_CHUNK_BYTES);
        let plain = std::sync::Mutex::new(Vec::<u8>::new());
        let identified = std::sync::atomic::AtomicUsize::new(0);
        let result = write_native_paste(
            "daemon-session:paired-pane",
            bytes.clone(),
            async { true },
            || async { Err(IpcError::from(crate::terminal::PtyError::SessionNotFound("daemon-session:paired-pane".into()))) },
            |_, _, _| {
                identified.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { Ok(()) }
            },
            |chunk| {
                plain.lock().unwrap().extend_from_slice(&chunk);
                async { Ok(()) }
            },
        )
        .await;
        assert!(result.is_ok(), "a refused identity must not fail the paste: {result:?}");
        assert_eq!(identified.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(*plain.lock().unwrap(), bytes, "every byte must reach the plain writer, in order");
        assert!(active_native_paste("daemon-session:paired-pane").is_none(), "no active paste may be registered");
    }

    /// A paste the daemon no longer holds (evicted, expired, or registered with a predecessor
    /// before a handover) after its first chunk landed: the shell already entered bracketed
    /// paste mode, so the writer must still close it, through a path that can still write.
    #[tokio::test]
    async fn unknown_paste_mid_write_still_closes_bracketed_paste_mode() {
        let bytes = bracketed(3 * NATIVE_PASTE_CHUNK_BYTES);
        let identified = std::sync::Mutex::new(0usize);
        let plain = std::sync::Mutex::new(Vec::<Vec<u8>>::new());
        let result = write_native_paste(
            "local-unknown-after-handover",
            bytes,
            async { true },
            || async { Ok("predecessor-paste".to_string()) },
            |_, _, _| {
                let call = {
                    let mut calls = identified.lock().unwrap();
                    *calls += 1;
                    *calls
                };
                async move {
                    if call == 1 {
                        Ok(())
                    } else {
                        Err(IpcError::from(crate::terminal::PasteChunkError::Unknown))
                    }
                }
            },
            |chunk| {
                plain.lock().unwrap().push(chunk);
                async { Ok(()) }
            },
        )
        .await;
        let error = result.expect_err("the paste did not finish");
        assert_eq!(error.code.to_string(), crate::daemon::protocol::PASTE_UNKNOWN_CODE);
        assert_eq!(*identified.lock().unwrap(), 2, "no identified write after the paste is unknown");
        assert_eq!(
            *plain.lock().unwrap(),
            vec![BRACKETED_PASTE_END.to_vec()],
            "the bracketed-paste close must be written exactly once"
        );
        let details = error.details.expect("partial receipt");
        assert_eq!(details["bytesWritten"], NATIVE_PASTE_CHUNK_BYTES);
    }

    /// A control key pressed while the paste is still being set up (the identity round trip) must
    /// stop the paste before any of it is written, and goes out as the ordinary byte it is.
    #[tokio::test]
    async fn control_during_paste_setup_stops_the_paste_before_any_byte() {
        const SESSION: &str = "setup-window-session";
        let (registered_tx, registered_rx) = tokio::sync::oneshot::channel::<()>();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let written = std::sync::Mutex::new(Vec::<Vec<u8>>::new());
        let paste = write_native_paste(
            SESSION,
            bracketed(2 * NATIVE_PASTE_CHUNK_BYTES),
            async { true },
            || async move {
                registered_tx.send(()).unwrap();
                release_rx.await.unwrap();
                Ok("late-paste".to_string())
            },
            |_, chunk, _| {
                written.lock().unwrap().push(chunk);
                async { Ok(()) }
            },
            |chunk| {
                written.lock().unwrap().push(chunk);
                async { Ok(()) }
            },
        );
        let control = async {
            // Event before action: the paste is inside its identity round trip.
            registered_rx.await.unwrap();
            let mut control_frame = None;
            let result = write_control_past_pending_paste(
                SESSION,
                vec![0x03],
                |_| -> std::future::Ready<Result<(bool, u64, bool), IpcError>> {
                    panic!("no daemon paste exists yet to cancel")
                },
                |frame| {
                    control_frame = Some(frame);
                    std::future::ready(Ok(()))
                },
            )
            .await;
            release_tx.send(()).unwrap();
            (result, control_frame)
        };
        let (paste, (control, control_frame)) = tokio::join!(paste, control);
        assert!(control.is_ok());
        assert_eq!(control_frame, Some(vec![0x03]), "the control byte goes out unchanged");
        let error = paste.expect_err("a stopped paste must not run");
        assert_eq!(error.details.as_ref().unwrap()["bytesWritten"], 0);
        assert!(written.lock().unwrap().is_empty(), "no byte of a stopped paste may be written");
        assert!(active_native_paste(SESSION).is_none());
        assert!(control_target(SESSION).is_none(), "nothing is left registered for the session");
    }

    /// A local paste whose identity is granted is written only through identified chunks, and is
    /// registered as active while it runs.
    #[tokio::test]
    async fn granted_paste_identity_writes_every_chunk_identified() {
        let bytes = bracketed(2 * NATIVE_PASTE_CHUNK_BYTES);
        let seen = std::sync::Mutex::new(Vec::<(String, bool, u8)>::new());
        let delivered = std::sync::Mutex::new(Vec::<u8>::new());
        let result = write_native_paste(
            "local-identified",
            bytes.clone(),
            async { true },
            || async { Ok("paste-1".to_string()) },
            |paste_id, chunk, final_chunk| {
                let active = active_native_paste("local-identified").map(|a| a.paste_id);
                seen.lock().unwrap().push((paste_id, final_chunk, u8::from(active.as_deref() == Some("paste-1"))));
                delivered.lock().unwrap().extend_from_slice(&chunk);
                async { Ok(()) }
            },
            |_| async { panic!("a granted identity must not use the plain path") },
        )
        .await;
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(*delivered.lock().unwrap(), bytes);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), bytes.len().div_ceil(NATIVE_PASTE_CHUNK_BYTES));
        assert!(seen.iter().all(|(id, _, active)| id == "paste-1" && *active == 1));
        assert_eq!(seen.iter().filter(|(_, last, _)| *last).count(), 1);
        assert!(seen.last().unwrap().1, "only the last chunk is final");
        assert!(active_native_paste("local-identified").is_none(), "the guard unregisters when done");
    }

    fn count(haystack: &[u8], needle: &[u8]) -> usize {
        haystack.windows(needle.len()).filter(|w| *w == needle).count()
    }

    #[test]
    fn large_paste_is_written_as_ordered_chunks_inside_one_bracket_pair() {
        let bytes = bracketed(300 * 1024);
        let (result, attempts) = run(bytes.clone(), None);
        assert!(result.is_ok());
        assert_eq!(attempts.len(), bytes.len().div_ceil(NATIVE_PASTE_CHUNK_BYTES));
        assert!(attempts.iter().all(|c| c.len() <= NATIVE_PASTE_CHUNK_BYTES));
        let joined = attempts.concat();
        assert_eq!(joined, bytes);
        assert_eq!(count(&joined, BRACKETED_PASTE_START), 1);
        assert_eq!(count(&joined, BRACKETED_PASTE_END), 1);
    }

    #[test]
    fn failure_after_partial_delivery_closes_bracket_and_reports_partial() {
        let (result, attempts) = run(bracketed(300 * 1024), Some(2));
        let error = result.expect_err("third chunk fails");
        assert_eq!(attempts.last().map(Vec::as_slice), Some(BRACKETED_PASTE_END));
        let details = error.details.expect("partial details");
        assert_eq!(details["inputWritten"], serde_json::json!(true));
        assert_eq!(details["partial"], serde_json::json!(true));
        assert_eq!(details["bytesWritten"], serde_json::json!(2 * NATIVE_PASTE_CHUNK_BYTES));
    }

    #[test]
    fn failure_on_first_chunk_with_unknown_delivery_closes_possible_bracket() {
        let (result, attempts) = run(bracketed(300 * 1024), Some(0));
        let error = result.expect_err("first chunk fails");
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts.last().map(Vec::as_slice), Some(BRACKETED_PASTE_END));
        let details = error.details.unwrap();
        assert_eq!(details["partial"], true);
        assert!(details["inputWritten"].is_null());
        assert_eq!(details["bytesWrittenExact"], false);
    }

    #[test]
    fn unknown_failed_chunk_count_is_only_a_lower_bound() {
        let (result, _) = run(bracketed(300 * 1024), Some(1));
        let details = result.unwrap_err().details.unwrap();
        assert_eq!(details["bytesWritten"], NATIVE_PASTE_CHUNK_BYTES);
        assert_eq!(details["bytesWrittenExact"], false);
        assert_eq!(details["inputWritten"], true);
    }

    #[tokio::test]
    async fn partial_chunk_receipt_is_added_to_completed_chunks() {
        for completed in [0, 1] {
            let mut attempts = Vec::new();
            let result = write_native_paste_chunked(bracketed(300 * 1024), |chunk| {
                let fail = attempts.len() == completed;
                attempts.push(chunk);
                std::future::ready(if fail {
                    Err(IpcError::from(crate::terminal::PtyError::InputError {
                        bytes_written: 12, message: "stalled".into(),
                    }))
                } else { Ok(()) })
            }).await;
            let error = result.unwrap_err();
            let details = error.details.unwrap();
            assert_eq!(details["bytesWritten"], completed * NATIVE_PASTE_CHUNK_BYTES + 12);
            assert_eq!(details["bytesWrittenExact"], true);
            assert_eq!(attempts.last().map(Vec::as_slice), Some(BRACKETED_PASTE_END));
            assert_eq!(attempts.len(), completed + 2);
        }
    }

    #[tokio::test]
    async fn known_zero_byte_refusal_does_not_close_an_unopened_bracket() {
        let mut attempts = 0;
        let result = write_native_paste_chunked(bracketed(100), |_| {
            attempts += 1;
            std::future::ready(Err(IpcError::from(crate::terminal::PtyError::InputError {
                bytes_written: 0, message: "closed".into(),
            })))
        }).await;
        let details = result.unwrap_err().details.unwrap();
        assert_eq!(details["bytesWritten"], 0);
        assert_eq!(details["inputWritten"], false);
        assert_eq!(attempts, 1);
    }

    fn active(total: usize, bracketed: bool) -> ActivePaste {
        ActivePaste { paste_id: "p".into(), total: total as u64, bracketed }
    }

    #[test]
    fn only_single_c0_controls_and_del_bypass_a_pending_paste() {
        for byte in [0x03u8, 0x04, 0x1a, 0x1b, 0x1c, 0x7f] {
            assert!(is_paste_interrupting_control(&[byte]), "{byte:#x}");
        }
        let ordinary: [&[u8]; 7] = [b"\t", b"\r", b"\n", b"a", b"\x1b[A", b"\x03\x03", b""];
        for bytes in ordinary {
            assert!(!is_paste_interrupting_control(bytes), "{bytes:?}");
        }
    }

    #[test]
    fn bracket_completion_closes_exactly_from_the_accepted_count() {
        let total = BRACKETED_PASTE_START.len() + 100 + BRACKETED_PASTE_END.len();
        let paste = active(total, true);
        assert!(bracket_completion(&paste, 0).is_empty(), "nothing delivered: nothing to close");
        let mut torn_start = BRACKETED_PASTE_START[2..].to_vec();
        torn_start.extend_from_slice(BRACKETED_PASTE_END);
        assert_eq!(bracket_completion(&paste, 2), torn_start);
        assert_eq!(bracket_completion(&paste, BRACKETED_PASTE_START.len() as u64), BRACKETED_PASTE_END);
        assert_eq!(bracket_completion(&paste, 50), BRACKETED_PASTE_END);
        let end_from = (total - BRACKETED_PASTE_END.len()) as u64;
        assert_eq!(bracket_completion(&paste, end_from), BRACKETED_PASTE_END);
        assert_eq!(bracket_completion(&paste, end_from + 2), &BRACKETED_PASTE_END[2..]);
        assert!(bracket_completion(&paste, total as u64).is_empty(), "complete paste is already closed");
        assert!(bracket_completion(&active(100, false), 50).is_empty(), "unbracketed paste needs no close");
    }

    #[tokio::test]
    async fn control_cancels_pending_paste_then_writes_close_and_byte_in_one_frame() {
        let total = BRACKETED_PASTE_START.len() + 300 * 1024 + BRACKETED_PASTE_END.len();
        let guard = ActivePasteGuard::register("ctl-session", total as u64, true);
        let expected_id = guard.paste_id.clone();
        let mut cancelled = None;
        let mut written = None;
        let result = write_control_past_pending_paste(
            "ctl-session",
            vec![0x03],
            |id| { cancelled = Some(id); std::future::ready(Ok((true, 70_000, true))) },
            |frame| { written = Some(frame); std::future::ready(Ok(())) },
        ).await;
        drop(guard);
        assert!(result.is_ok());
        assert_eq!(cancelled.as_deref(), Some(expected_id.as_str()));
        let mut expected = BRACKETED_PASTE_END.to_vec();
        expected.push(0x03);
        assert_eq!(written, Some(expected));
        assert!(active_native_paste("ctl-session").is_none(), "guard unregisters the paste");
    }

    #[tokio::test]
    async fn control_without_active_paste_is_written_unchanged_and_never_cancels() {
        let mut written = None;
        let result = write_control_past_pending_paste(
            "idle-session",
            vec![0x03],
            |_| -> std::future::Ready<Result<(bool, u64, bool), IpcError>> { panic!("no paste to cancel") },
            |frame| { written = Some(frame); std::future::ready(Ok(())) },
        ).await;
        assert!(result.is_ok());
        assert_eq!(written, Some(vec![0x03]));
    }

    #[tokio::test]
    async fn control_after_unsettled_cancel_adds_no_guessed_close_and_reports_full_queue_honestly() {
        let guard = ActivePasteGuard::register("full-session", 400_000, true);
        let mut written = None;
        let result = write_control_past_pending_paste(
            "full-session",
            vec![0x03],
            |_| std::future::ready(Ok((true, 4096, false))),
            |frame| {
                written = Some(frame);
                std::future::ready(Err(IpcError::from(crate::terminal::PtyError::InputError {
                    bytes_written: 0, message: "PTY_INPUT_TIMEOUT".into(),
                })))
            },
        ).await;
        drop(guard);
        assert_eq!(written, Some(vec![0x03]), "lower-bound count must not drive a bracket close");
        let details = result.unwrap_err().details.unwrap();
        assert_eq!(details["bytesWritten"], 0);
        assert_eq!(details["inputWritten"], false);
        assert_eq!(details["pasteCancelled"], true);
        assert_eq!(details["pasteBytesWritten"], 4096);
        assert_eq!(details["pasteBytesWrittenExact"], false);
    }

    #[tokio::test]
    async fn cancelled_chunk_stops_the_paste_without_appending_a_bracket_close() {
        let mut attempts = Vec::new();
        let result = write_native_paste_chunked(bracketed(300 * 1024), |chunk| {
            let cancel = attempts.len() == 1;
            attempts.push(chunk);
            std::future::ready(if cancel {
                Err(IpcError::from(crate::terminal::PasteChunkError::Cancelled { bytes_written: 500 }))
            } else { Ok(()) })
        }).await;
        assert_eq!(attempts.len(), 2, "no chunk after cancellation and no ESC[201~ append");
        let details = result.unwrap_err().details.unwrap();
        assert_eq!(details["pasteCancelled"], true);
        assert_eq!(details["bytesWritten"], NATIVE_PASTE_CHUNK_BYTES + 500);
        assert_eq!(details["bytesWrittenExact"], true);
    }

    #[cfg(unix)]
    async fn await_output_marker(
        rx: &mut tokio::sync::mpsc::Receiver<Vec<u8>>,
        marker: &str,
        bound: std::time::Duration,
    ) -> Option<()> {
        tokio::time::timeout(bound, async {
            let mut seen = Vec::new();
            while let Some(chunk) = rx.recv().await {
                seen.extend_from_slice(&chunk);
                if String::from_utf8_lossy(&seen).contains(marker) {
                    return Some(());
                }
            }
            None
        })
        .await
        .ok()
        .flatten()
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn partial_input_timeout_reports_bytes_received_by_real_pty() {
        let fixture = tempfile::tempdir().unwrap();
        let release = fixture.path().join("release");
        let path = std::ffi::CString::new(release.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let manager = crate::terminal::PtyManager::new();
        let mut cmd = portable_pty::CommandBuilder::new("python3");
        cmd.args(["-c", "import os, sys, termios\na = termios.tcgetattr(0)\na[3] &= ~(termios.ICANON | termios.ECHO)\ntermios.tcsetattr(0, termios.TCSANOW, a)\nf = os.open(sys.argv[1], os.O_RDWR)\nprint('READY', flush=True)\nos.read(f, 1)\nos.set_blocking(0, False)\nn = 0\nwhile True:\n try:\n  b = os.read(0, 65536)\n  if not b: break\n  n += len(b)\n except BlockingIOError: break\nprint('COUNT=' + str(n), flush=True)\n"]);
        cmd.arg(release.as_os_str());
        let (id, mut output) = manager.spawn(cmd, 80, 24).unwrap();
        let session = manager.get_session(&id).unwrap();
        let ready = await_output_marker(&mut output, "READY", std::time::Duration::from_secs(10)).await;
        if ready.is_none() {
            manager.close_session(&id).await.unwrap();
            panic!("fixture did not configure raw input");
        }
        let result = session.write_input_cancellable(&vec![b'x'; 65536]).await;
        let released = tokio::task::spawn_blocking(move || {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open(release)?.write_all(b"x")
        }).await.unwrap();
        let captured = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let mut bytes = Vec::new();
            while let Some(chunk) = output.recv().await { bytes.extend_from_slice(&chunk); }
            bytes
        }).await;
        if manager.has_session(&id) { manager.close_session(&id).await.unwrap(); }
        released.unwrap();
        let text = String::from_utf8(captured.expect("fixture exits after release")).unwrap();
        let count: usize = text.lines().find_map(|line| line.trim().strip_prefix("COUNT=")).unwrap().parse().unwrap();
        let error = result.expect_err("non-reading PTY must backpressure a 64 KiB frame");
        let crate::terminal::PtyError::InputError { bytes_written, .. } = error else {
            panic!("missing partial input receipt: {error:?}");
        };
        assert!(count > 0 && count < 65536, "fixture must receive only part of the frame");
        assert_eq!(bytes_written, count);
        eprintln!("PARTIAL_PTY_ACCEPTED={bytes_written} RECEIVED={count}");
    }

    /// Real-PTY regression: a 300 KiB paste into a foreground program that never reads stdin
    /// stays pending on the full tty input queue, and typed Ctrl+C must
    /// still reach that program's foreground process group instead of queueing behind the paste.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_c_reaches_foreground_program_while_300k_paste_is_pending() {
        use crate::terminal::TerminalService;
        use std::future::Future;
        use std::task::Poll;
        const BOUND: std::time::Duration = std::time::Duration::from_secs(30);

        let service = TerminalService::default();
        let manager = service.pty_manager();
        // Non-canonical so a full input queue backpressures the master write (Linux n_tty
        // discards canonical overflow instead); ISIG stays on. Nothing ever reads stdin.
        let mut cmd = portable_pty::CommandBuilder::new("python3");
        cmd.arg("-c");
        cmd.arg(
            "import signal, termios, sys\n\
             attrs = termios.tcgetattr(0)\n\
             attrs[3] = (attrs[3] & ~(termios.ICANON | termios.ECHO)) | termios.ISIG\n\
             termios.tcsetattr(0, termios.TCSANOW, attrs)\n\
             signal.signal(signal.SIGINT, lambda *_: (print('GOT_INT', flush=True), sys.exit(0)))\n\
             print('READY', flush=True)\n\
             signal.pause()\n",
        );
        let (session_id, mut rx) = manager.spawn(cmd, 80, 24).expect("spawn foreground program");
        let session = manager.get_session(&session_id).expect("session registered");

        // Event before action: termios is configured and the INT trap is armed.
        let ready = await_output_marker(&mut rx, "READY", BOUND).await.is_some();
        if !ready {
            manager.close_session(&session_id).await.expect("close unready fixture");
            panic!("foreground program never announced READY");
        }

        let bytes = bracketed(300 * 1024);
        let total = bytes.len();
        let registered = service.begin_paste(&session_id).unwrap();
        let active = ActivePasteGuard::register_id(&session_id, registered.clone(), total as u64, true);
        let paste_service = service.clone();
        let paste_session_id = session_id.clone();
        let mut chunk_index = 0;
        let mut paste = Box::pin(write_native_paste_chunked(bytes, move |chunk| {
            chunk_index += 1;
            let last = chunk_index == total.div_ceil(NATIVE_PASTE_CHUNK_BYTES);
            let service = paste_service.clone();
            let session_id = paste_session_id.clone();
            let registered = registered.clone();
            async move {
                service
                    .write_paste_chunk(&session_id, 0, &registered, chunk, last)
                    .await
                    .map_err(IpcError::from)
            }
        }));
        let master_fd = session.raw_master_fd().expect("master fd");
        // Poll readiness is not a full-queue test on Linux: right after a refused write the
        // master reports writable again while the line discipline drains its buffer into a queue
        // that stays full. The kernel's own answer is: a nonblocking one-byte write refused with
        // EAGAIN. A paste pending on writability means its own write was refused
        // and nothing reads the queue, so the probe is refused too. A probe byte that is accepted
        // goes to a program that never reads stdin and is discarded with the queue.
        let queue_full = || {
            // SAFETY: one valid byte; session keeps the nonblocking master fd alive.
            let written = unsafe { libc::write(master_fd, b"x".as_ptr().cast(), 1) };
            written < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock
        };
        // Drive the paste only on its own wakeups until it is parked on a full tty input queue.
        let saturated = tokio::time::timeout(
            BOUND,
            std::future::poll_fn(|cx| match paste.as_mut().poll(cx) {
                Poll::Ready(result) => Poll::Ready(Err(result)),
                Poll::Pending if queue_full() => Poll::Ready(Ok(())),
                Poll::Pending => Poll::Pending,
            }),
        )
        .await;
        if !matches!(saturated, Ok(Ok(()))) {
            // The paste future may have completed; it must not be polled again.
            drop(paste);
            drop(active);
            manager.close_session(&session_id).await.expect("close unsaturated fixture");
            panic!(
                "paste must be parked on the full PTY input queue at Ctrl+C: {:?}",
                saturated.map(|r| r.map_err(|e| e.err()))
            );
        }

        let control = write_control_past_pending_paste(
            &session_id,
            vec![0x03],
            |paste_id| {
                let service = service.clone();
                let id = session_id.clone();
                async move {
                    let outcome = service.cancel_paste(&id, &paste_id).await;
                    Ok((outcome.found, outcome.bytes_written, outcome.settled))
                }
            },
            |bytes| {
                let session = session.clone();
                async move { session.write_input_cancellable(&bytes).await.map_err(IpcError::from) }
            },
        );
        let (signal, _) = tokio::join!(tokio::time::timeout(BOUND, control), paste.as_mut());
        let interrupted = if matches!(&signal, Ok(Ok(()))) {
            await_output_marker(&mut rx, "GOT_INT", BOUND).await
        } else {
            None
        };
        drop(paste);
        drop(active);
        // Cleanup before asserting so a failure cannot leak the looping program.
        manager.close_session(&session_id).await.expect("close paste fixture");

        let signal = signal
            .expect("Ctrl+C delivery must not wait behind the pending paste");
        assert!(signal.is_ok(), "Ctrl+C delivery failed: {signal:?}");
        assert!(
            interrupted.is_some(),
            "foreground program never received SIGINT while the paste was pending"
        );
    }
}

pub(crate) fn authoritative_mouse_event(
    event: &MouseEvent,
    bounds: &LogicalBounds,
    cell_metrics: &CellMetrics,
) -> Result<MouseEvent, NativeTerminalError> {
    let layout = SurfaceCompositionLayout::compute(bounds, cell_metrics)?;
    if !event.position.x.is_finite() || !event.position.y.is_finite() {
        return Err(NativeTerminalError::InvalidValue(
            "MousePosition x and y coordinates must be finite".to_string(),
        ));
    }

    let scale_factor = bounds.scale_factor as f32;
    let position = MousePosition {
        x: event.position.x * scale_factor,
        y: event.position.y * scale_factor,
    };
    if !position.x.is_finite() || !position.y.is_finite() {
        return Err(NativeTerminalError::InvalidValue(
            "MousePosition x and y coordinates must be finite".to_string(),
        ));
    }

    Ok(MouseEvent {
        position,
        size: Some(MouseRendererSize {
            screen_width: layout.physical_bounds.width,
            screen_height: layout.physical_bounds.height,
            cell_width: cell_metrics.width_px,
            cell_height: cell_metrics.height_px,
            padding_top: 0,
            padding_bottom: 0,
            padding_right: 0,
            padding_left: 0,
        }),
        ..event.clone()
    })
}

pub fn native_mouse_routes(tracking: bool, event: &MouseEvent) -> (bool, bool) {
    let report = tracking && !event.modifiers.shift;
    (!report, report)
}

#[tauri::command]
pub async fn cmd_native_terminal_mouse<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    event: MouseEvent,
    generation: Option<u64>,
) -> Result<NativeTerminalMouseReceipt, IpcError> {
    let bounds = state.session_logical_bounds(&session_id).ok_or_else(|| {
        IpcError::internal(format!(
            "Native terminal session {session_id} has no rendered layout yet: logical bounds are unavailable"
        ))
    })?;
    let cell_metrics = state.session_cell_metrics(&session_id).ok_or_else(|| {
        IpcError::internal(format!(
            "Native terminal session {session_id} has no rendered layout yet: cell metrics are unavailable"
        ))
    })?;
    let event = authoritative_mouse_event(&event, &bounds, &cell_metrics)
        .map_err(|error| IpcError::internal(error.to_string()))?;

    let tracking =
        mouse_tracking_enabled_for_attached_session(state.inner(), &session_id).unwrap_or(false);

    let (perform_selection, perform_tracking) = native_mouse_routes(tracking, &event);

    if perform_tracking {
        let bytes = match encode_attached_native_mouse(state.inner(), &session_id, &event) {
            Ok(bytes) => bytes,
            Err(err) => return Err(IpcError::internal(err.to_string())),
        };
        if !bytes.is_empty() {
            daemon_client
                .write_terminal_at_generation(&session_id, generation, bytes)
                .await?;
        }
    }

    if perform_selection {
        if let Err(err) =
            select_attached_native_terminal_with_mouse(state.inner(), &session_id, &event)
        {
            return Err(IpcError::internal(err.to_string()));
        }

        if event.action == MouseAction::Press
            && (cfg!(debug_assertions)
                || std::env::var("FERRYX_SWITCH_DEBUG").ok().as_deref() == Some("1"))
        {
            let logical_bounds = state
                .session_logical_bounds(&session_id)
                .map(|bounds| {
                    serde_json::json!({
                        "x": bounds.x,
                        "y": bounds.y,
                        "width": bounds.width,
                        "height": bounds.height,
                        "scaleFactor": bounds.scale_factor,
                    })
                })
                .unwrap_or(serde_json::Value::Null);
            let (cols, rows, selection_range) = state
                .with_session_terminal(&session_id, |term| {
                    let cols = term
                        .cols()
                        .map(serde_json::Value::from)
                        .unwrap_or_else(|error| serde_json::Value::String(error.to_string()));
                    let rows = term
                        .rows()
                        .map(serde_json::Value::from)
                        .unwrap_or_else(|error| serde_json::Value::String(error.to_string()));
                    let selection_range = match term.selection_range() {
                        Ok(range) => serde_json::to_value(range).unwrap_or(serde_json::Value::Null),
                        Err(error) => serde_json::Value::String(error.to_string()),
                    };
                    Ok((cols, rows, selection_range))
                })
                .unwrap_or_else(|error| {
                    let error = serde_json::Value::String(error.to_string());
                    (error.clone(), error.clone(), error)
                });
            let event_size = serde_json::to_value(event.size).unwrap_or(serde_json::Value::Null);
            let event_position_x = event.position.x as f64;
            let event_position_y = event.position.y as f64;
            let debug_session_id = session_id.clone();
            {
                use std::time::{SystemTime, UNIX_EPOCH};

                let wall_time_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_millis() as u64)
                    .unwrap_or(0);
                let entry = serde_json::json!({
                    "runId": "rust-mouse-geo",
                    "sequence": 0,
                    "event": "terminal.mouse.pressGeoBackend",
                    "wallTimeMs": wall_time_ms,
                    "details": {
                        "session_id": debug_session_id,
                        "event_position": { "x": event_position_x, "y": event_position_y },
                        "event_size": event_size,
                        "cols": cols,
                        "rows": rows,
                        "logical_bounds": logical_bounds,
                        "selection_range": selection_range,
                    }
                });
                crate::ipc::debug::log_native_switch_debug(entry);
            }
        }
    }

    let receipt = if let Some(window) = app.get_window("main") {
        let state_inner = state.inner().clone();
        let surface_window = window.clone();
        let (sender, receiver) = oneshot::channel();
        let session_id_clone = session_id.clone();
        if window
            .run_on_main_thread(move || {
                let res = state_inner
                    .session_logical_bounds(&session_id_clone)
                    .and_then(|bounds| {
                        state_inner
                            .render(
                                &surface_window,
                                NativeTerminalBoundsRequest {
                                    session_id: session_id_clone.clone(),
                                    bounds,
                                },
                            )
                            .map(|receipt| into_bound_ipc_receipt(&state_inner, session_id_clone, receipt))
                            .ok()
                    });
                let _ = sender.send(res);
            })
            .is_ok()
        {
            receiver.await.ok().flatten()
        } else {
            None
        }
    } else {
        None
    };

    Ok(NativeTerminalMouseReceipt {
        mouse_tracking_enabled: tracking,
        receipt,
    })
}

#[tauri::command]
pub async fn cmd_native_terminal_search<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    query: String,
    case_sensitive: bool,
) -> Result<NativeTerminalSearchResult, IpcError> {
    search_attached_native_terminal(state.inner(), &session_id, &query, case_sensitive)
        .map_err(|err| IpcError::internal(err.to_string()))
}

#[tauri::command]
pub async fn cmd_native_terminal_line_at<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    col: u16,
    row: u16,
) -> Result<NativeTerminalLineReceipt, IpcError> {
    line_text_attached_native_terminal(state.inner(), &session_id, col, row)
        .map_err(|err| IpcError::internal(err.to_string()))
}

#[tauri::command]
pub async fn cmd_native_terminal_clipboard_content<R: Runtime>(
    app: AppHandle<R>,
) -> Result<NativeTerminalClipboardContent, IpcError> {
    #[cfg(target_os = "macos")]
    {
        let window = match app.get_window("main") {
            Some(window) => window,
            None => return Err(IpcError::internal("Main Ferryx window is unavailable")),
        };
        let (sender, receiver) = oneshot::channel();
        if let Err(error) = window.run_on_main_thread(move || {
            let (content, types) = read_native_pasteboard();
            if cfg!(debug_assertions)
                || std::env::var("FERRYX_SWITCH_DEBUG").ok().as_deref() == Some("1")
            {
                use std::time::{SystemTime, UNIX_EPOCH};

                let (kind, text_length) = match &content {
                    NativeTerminalClipboardContent::Text { text } => ("text", text.len()),
                    NativeTerminalClipboardContent::Image => ("image", 0),
                    NativeTerminalClipboardContent::Empty => ("empty", 0),
                };
                let wall_time_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let entry = serde_json::json!({
                    "runId": "rust-clipboard",
                    "sequence": 0,
                    "event": "terminal.surface.paste.clipboard.classify",
                    "wallTimeMs": wall_time_ms,
                    "details": {
                        "types": types,
                        "kind": kind,
                        "textLength": text_length,
                    }
                });
                crate::ipc::debug::log_native_switch_debug(entry);
            }
            let _ = sender.send(content);
        }) {
            return Err(IpcError::internal(format!(
                "Could not dispatch native clipboard read: {error}"
            )));
        }
        match receiver.await {
            Ok(content) => Ok(content),
            Err(_) => Err(IpcError::internal(
                "Main thread stopped before native clipboard read completed",
            )),
        }
    }
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        let (content, types) = tokio::task::spawn_blocking(read_native_pasteboard)
            .await
            .unwrap_or_else(|_| (NativeTerminalClipboardContent::Empty, Vec::new()));
        if cfg!(debug_assertions)
            || std::env::var("FERRYX_SWITCH_DEBUG").ok().as_deref() == Some("1")
        {
            use std::fs::OpenOptions;
            use std::io::Write;
            use std::time::{SystemTime, UNIX_EPOCH};

            let (kind, text_length) = match &content {
                NativeTerminalClipboardContent::Text { text } => ("text", text.len()),
                NativeTerminalClipboardContent::Image => ("image", 0),
                NativeTerminalClipboardContent::Empty => ("empty", 0),
            };
            let wall_time_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let entry = serde_json::json!({
                "runId": "rust-clipboard",
                "sequence": 0,
                "event": "terminal.surface.paste.clipboard.classify",
                "wallTimeMs": wall_time_ms,
                "details": {
                    "types": types,
                    "kind": kind,
                    "textLength": text_length,
                }
            });
            let _ = crate::ipc::run_blocking(move || {
                let log_path = std::env::temp_dir().join("ferryx-switch-debug.jsonl");
                if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
                    let _ = writeln!(file, "{entry}");
                }
                Ok(())
            })
            .await;
        }
        Ok(content)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
        let (content, types) = tokio::task::spawn_blocking(read_native_pasteboard)
            .await
            .unwrap_or_else(|_| (NativeTerminalClipboardContent::Empty, Vec::new()));
        if cfg!(debug_assertions)
            || std::env::var("FERRYX_SWITCH_DEBUG").ok().as_deref() == Some("1")
        {
            use std::time::{SystemTime, UNIX_EPOCH};

            let (kind, text_length) = match &content {
                NativeTerminalClipboardContent::Text { text } => ("text", text.len()),
                NativeTerminalClipboardContent::Image => ("image", 0),
                NativeTerminalClipboardContent::Empty => ("empty", 0),
            };
            let wall_time_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let entry = serde_json::json!({
                "runId": "rust-clipboard",
                "sequence": 0,
                "event": "terminal.surface.paste.clipboard.classify",
                "wallTimeMs": wall_time_ms,
                "details": {
                    "types": types,
                    "kind": kind,
                    "textLength": text_length,
                }
            });
            crate::ipc::debug::log_native_switch_debug(entry);
        }
        Ok(content)
    }
}

fn into_ipc_receipt(
    session_id: String,
    receipt: NativeTerminalSurfaceReceipt,
) -> NativeTerminalBoundsReceipt {
    NativeTerminalBoundsReceipt {
        attach_tuple: None,
        presented: receipt.presented,
        render_deferred: receipt.render_deferred,
        session_id,
        cols: receipt.cols,
        rows: receipt.rows,
        rebuilt_rows: receipt.rebuilt_rows,
        reused_rows: receipt.reused_rows,
        cursor_col: receipt.cursor_col,
        cursor_row: receipt.cursor_row,
        cell_width_px: receipt.cell_width_px,
        cell_height_px: receipt.cell_height_px,
        effective_scale_factor: receipt.effective_scale_factor,
    }
}

fn into_bound_ipc_receipt(
    state: &NativeTerminalSurfaceHostState,
    session_id: String,
    receipt: NativeTerminalSurfaceReceipt,
) -> NativeTerminalBoundsReceipt {
    let (attach_tuple, presented) = state.bound_presentation_observation(&session_id, &receipt);
    let mut result = into_ipc_receipt(session_id, receipt);
    result.attach_tuple = attach_tuple;
    result.presented = presented;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn native_attach_reports_renderer_unavailable_when_no_gpu_adapter_exists() {
        // Given a host where every GPU adapter request fails.
        let _no_gpu = crate::native_terminal::renderer::gpu_context::GpuContext::force_adapter_unavailable_for_test();
        let root = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_builder()
            .manage(Arc::new(DaemonClient::new_with_socket(root.path().join("no-gpu-daemon"))))
            .manage(NativeTerminalSurfaceHostState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();

        // When the pane attaches its native surface.
        let error = cmd_native_terminal_attach(
            app.handle().clone(),
            "no-gpu-session".into(), None, None, None, None, None,
        )
        .await
        .expect_err("attach must not succeed into a surface that can never present");

        // Then the UI receives the typed fallback code and no blank surface was reserved.
        assert_eq!(error.code, IpcErrorCode::NativeRendererUnavailable);
        assert_eq!(
            serde_json::to_value(&error.code).unwrap(),
            serde_json::json!("NATIVE_RENDERER_UNAVAILABLE")
        );
        let state = app.state::<NativeTerminalSurfaceHostState>();
        assert!(!state.has_session("no-gpu-session"));
        state.teardown();
    }

    #[test]
    fn gpu_adapter_and_device_failures_map_to_renderer_unavailable() {
        for error in [
            NativeTerminalError::GpuAdapterUnavailable("none".into()),
            NativeTerminalError::GpuDeviceUnavailable("none".into()),
        ] {
            assert_eq!(IpcError::from(error).code, IpcErrorCode::NativeRendererUnavailable);
        }
        assert_eq!(
            IpcError::from(NativeTerminalError::GpuPipelineError("x".into())).code,
            IpcErrorCode::InternalError
        );
        let _no_gpu = crate::native_terminal::renderer::gpu_context::GpuContext::force_adapter_unavailable_for_test();
        assert!(matches!(
            crate::native_terminal::renderer::gpu_context::GpuContext::new(),
            Err(NativeTerminalError::GpuAdapterUnavailable(_))
        ));
    }

    #[test]
    fn surface_without_standard_formats_maps_to_renderer_unavailable() {
        // Given a software fallback adapter whose window surface lists no 8-bit RGBA/BGRA format.
        let adapter_info = wgpu::AdapterInfo {
            name: "llvmpipe".into(),
            ..wgpu::AdapterInfo::new(wgpu::DeviceType::Cpu, wgpu::Backend::Gl)
        };
        let (error, _reset) = crate::native_terminal::renderer::gpu_context::GpuContext::latch_surface_unsupported_for_test(
            &[], &adapter_info,
        );

        // Then the frame error itself is the typed fallback code, not an internal error.
        assert!(error.to_string().contains("Surface does not support standard 8-bit RGBA/BGRA formats"));
        let ipc = IpcError::from(error);
        assert_eq!(ipc.code, IpcErrorCode::NativeRendererUnavailable);
        assert_eq!(
            serde_json::to_value(&ipc.code).unwrap(),
            serde_json::json!("NATIVE_RENDERER_UNAVAILABLE")
        );
        // And bounds updates for the already-attached pane report it too.
        assert_eq!(
            check_native_surface_presentation().unwrap_err().code,
            IpcErrorCode::NativeRendererUnavailable
        );
    }

    #[tokio::test]
    async fn native_attach_reports_renderer_unavailable_when_surface_cannot_present() {
        // Given an adapter that exists but whose window surface proved it cannot present.
        let adapter_info = wgpu::AdapterInfo::new(wgpu::DeviceType::Cpu, wgpu::Backend::Vulkan);
        let (_error, _reset) = crate::native_terminal::renderer::gpu_context::GpuContext::latch_surface_unsupported_for_test(
            &[wgpu::TextureFormat::Rgba16Float], &adapter_info,
        );
        let root = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_builder()
            .manage(Arc::new(DaemonClient::new_with_socket(root.path().join("no-surface-daemon"))))
            .manage(NativeTerminalSurfaceHostState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();

        // When another pane attaches its native surface.
        let error = cmd_native_terminal_attach(
            app.handle().clone(),
            "no-surface-session".into(), None, None, None, None, None,
        )
        .await
        .expect_err("attach must not succeed into a surface that can never present");

        // Then the UI receives the typed fallback code and no blank surface was reserved.
        assert_eq!(error.code, IpcErrorCode::NativeRendererUnavailable);
        let state = app.state::<NativeTerminalSurfaceHostState>();
        assert!(!state.has_session("no-surface-session"));
        state.teardown();
    }

    #[test]
    fn select_range_mode_returns_the_selected_text() {
        let state = NativeTerminalSurfaceHostState::default();
        state.attach_test_session_for_liveness("range", 1, None).unwrap();
        state
            .with_session_terminal("range", |term| {
                for i in 0..40 {
                    term.feed(format!("entry {i:02}\r\n").as_bytes())?;
                }
                Ok(())
            })
            .unwrap();

        let mode: NativeTerminalSelectMode = serde_json::from_value(serde_json::json!({
            "type": "range", "startCol": 0, "startRow": 3, "endCol": 7, "endRow": 4, "rectangle": false,
        }))
        .unwrap();
        let receipt = select_attached_native_terminal(&state, "range", &mode).unwrap();
        assert_eq!(receipt.text.as_deref(), Some("entry 03\nentry 04"));

        let cursor = cursor_for_attached_native_terminal(&state, "range").unwrap();
        assert_eq!((cursor.col, cursor.row), (0, 23));
        assert_eq!(cursor.screen_row, 40);
        assert_eq!(cursor.viewport_top_row, 17);

        let out_of_bounds = NativeTerminalSelectMode::Range {
            start_col: 0,
            start_row: 0,
            end_col: 0,
            end_row: 10_000,
            rectangle: false,
        };
        assert!(matches!(
            select_attached_native_terminal(&state, "range", &out_of_bounds),
            Err(NativeTerminalError::InvalidValue(_))
        ));
        state.teardown();
    }

    #[test]
    fn stale_tuple_set_bounds_reports_reason_so_the_pane_resends() {
        // Given a surface that rotated to attempt generation 4 while a set-bounds for 3 was in flight.
        let tuple = |generation| crate::daemon::protocol::PaneAttachTuple {
            backend_session_id: "backend".into(), incarnation: Some("incarnation".into()),
            daemon_epoch: "8".into(), frontend_session_id: "frontend".into(),
            pane_identity: "pane".into(), binding_key: "binding".into(), attempt_generation: generation,
        };
        let state = NativeTerminalSurfaceHostState::default();
        state.attach_test_session_for_liveness("backend", 8, None).unwrap();
        state.begin_replay_request_with_tuple("backend", Some(tuple(4))).unwrap();

        // When the fenced render refuses the old tuple.
        let error = set_bounds_render_error(
            &state, "backend", Some(&tuple(3)),
            NativeTerminalError::SessionDetached("backend".into()),
        );
        let moved = set_bounds_tuple_moved_error(&state, "backend".into(), Some(&tuple(3)));

        // Then both paths keep the benign code and name the stale tuple.
        for error in [error, moved] {
            assert_eq!(error.code, IpcErrorCode::SessionNotFound);
            assert_eq!(error.details.as_ref().and_then(|d| d.get("reason")), Some(&serde_json::json!("stale_tuple")));
        }
        // A request carrying the current tuple, or a released surface, stays a plain detach.
        let current = set_bounds_render_error(
            &state, "backend", Some(&tuple(4)),
            NativeTerminalError::SessionDetached("backend".into()),
        );
        assert_eq!(current.details, None);
        let released = set_bounds_tuple_moved_error(&state, "gone".into(), Some(&tuple(3)));
        assert_eq!(released.code, IpcErrorCode::SessionNotFound);
        assert_eq!(released.details, None);
        state.teardown();
    }

    #[test]
    fn pane_liveness_native_binding_response_serializes_authoritative_tuple_without_presentation() {
        let tuple = crate::daemon::protocol::PaneAttachTuple {
            backend_session_id: "backend".into(), incarnation: Some("incarnation".into()),
            daemon_epoch: "8".into(), frontend_session_id: "frontend".into(),
            pane_identity: "pane".into(), binding_key: "binding".into(), attempt_generation: 3,
        };
        let response = NativeTerminalAttachReceipt {
            session_id: "backend".into(), attach_tuple: Some(tuple.clone()), presented: false,
        };
        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["attachTuple"], serde_json::to_value(&tuple).unwrap());
        assert_eq!(json["presented"], false);
        let state = NativeTerminalSurfaceHostState::default();
        state.attach_test_session_for_liveness("backend", 8, None).unwrap();
        state.begin_replay_request_with_tuple("backend", Some(tuple.clone())).unwrap();
        let mut unknown = state.degraded_receipt("backend");
        unknown.presented = true;
        let bounds = into_bound_ipc_receipt(&state, "backend".into(), unknown);
        let json = serde_json::to_value(bounds).unwrap();
        assert_eq!(json["attachTuple"], serde_json::to_value(tuple).unwrap());
        assert_eq!(json["presented"], false);
        state.teardown();
    }

    #[tokio::test(start_paused = true)]
    async fn native_reconnect_attach_settles_when_daemon_never_answers_handshake() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        // Given an isolated endpoint that accepts but never answers the handshake.
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("silent-native-attach");
        #[cfg(unix)]
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        #[cfg(not(unix))]
        let listener = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            std::fs::write(&path, listener.local_addr().unwrap().port().to_string()).unwrap();
            // The daemon publishes daemon.token beside its port, and a client bound to this
            // endpoint presents the token found beside it (read_transport_token_for). This fake
            // daemon does not check it, but without one the client refuses the pair as stale.
            std::fs::write(path.with_file_name("daemon.token"), "fixture-transport-token").unwrap();
            listener
        };
        let app = tauri::test::mock_builder()
            .manage(Arc::new(DaemonClient::new_with_socket(path)))
            .manage(NativeTerminalSurfaceHostState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let (observed, handshake) = oneshot::channel();
        let peer = async {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            assert!(reader.read_line(&mut line).await.unwrap() > 0);
            observed.send(()).unwrap();
            line.clear();
            // Cancellation releases the attachment connection without closing the PTY.
            assert_eq!(reader.read_line(&mut line).await.unwrap(), 0);
        };
        let action = async {
            // When the native IPC entry point attempts attachment.
            let tuple = crate::daemon::protocol::PaneAttachTuple {
                backend_session_id: "qa-existing-session".into(),
                incarnation: Some("qa-incarnation".into()),
                daemon_epoch: "7".into(),
                frontend_session_id: "qa-frontend".into(),
                pane_identity: "qa-pane".into(),
                binding_key: "qa-binding".into(),
                attempt_generation: 1,
            };
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(120),
                cmd_native_terminal_attach(
                    app.handle().clone(),
                    "qa-existing-session".into(), None, None, None, Some(tuple), None,
                ),
            ).await;
            // Then the command settles before the test's safety deadline.
            assert!(result.is_ok(), "native reconnect attach exceeded its bounded-error contract");
            let error = result.unwrap().expect_err("attachment must fail");
            assert_eq!(error.code, IpcErrorCode::Timeout);
            let state = app.state::<NativeTerminalSurfaceHostState>();
            assert!(!state.has_session("qa-existing-session"), "timeout must roll back the new native reservation");
            assert!(state.session_attempt_generation("qa-existing-session").is_none());
            state.teardown();
        };
        let clock = async {
            handshake.await.unwrap();
            tokio::time::advance(std::time::Duration::from_secs(60)).await;
        };
        tokio::join!(peer, action, clock);
    }

    #[tokio::test(start_paused = true)]
    async fn native_attach_failure_rolls_back_new_session_reservation() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        // This tests daemon rejection, not elapsed time during native initialization.
        // A running blocking task inhibits paused-clock auto-advance during socket I/O.
        let (release_clock, hold_clock) = std::sync::mpsc::channel::<()>();
        let clock_guard = tokio::task::spawn_blocking(move || hold_clock.recv().unwrap());
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("attach-fail-rollback");
        #[cfg(unix)]
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        #[cfg(not(unix))]
        let listener = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            std::fs::write(&path, listener.local_addr().unwrap().port().to_string()).unwrap();
            // The daemon publishes daemon.token beside its port, and a client bound to this
            // endpoint presents the token found beside it (read_transport_token_for). This fake
            // daemon does not check it, but without one the client refuses the pair as stale.
            std::fs::write(path.with_file_name("daemon.token"), "fixture-transport-token").unwrap();
            listener
        };
        let app = tauri::test::mock_builder()
            .manage(Arc::new(DaemonClient::new_with_socket(path)))
            .manage(NativeTerminalSurfaceHostState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();

        let session_id = "test-fail-rollback-session";
        let state = app.state::<NativeTerminalSurfaceHostState>();
        // Reservation layout also emits a resize; keep that separate RPC out of this peer.
        assert!(state.set_pty_resize_sink_if_absent(Arc::new(move |id, cols, rows| {
            assert_eq!(id, session_id);
            assert!(cols > 0 && rows > 0);
        })));
        let tuple = crate::daemon::protocol::PaneAttachTuple {
            backend_session_id: session_id.into(),
            incarnation: Some("inc-1".into()),
            daemon_epoch: "7".into(),
            frontend_session_id: "front-1".into(),
            pane_identity: "pane-1".into(),
            binding_key: "bind-1".into(),
            attempt_generation: 1,
        };

        let peer = async {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            // 1. Handshake
            assert!(reader.read_line(&mut line).await.unwrap() > 0);
            let hs_resp = serde_json::to_string(&crate::daemon::DaemonResponse::HandshakeOk {
                version: crate::daemon::DAEMON_PROTOCOL_VERSION,
                pid: 12345,
                epoch: 7,
                binary_path: None,
                binary_mtime_ms: None,
                daemon_version: None,
                capabilities: Vec::new(),
                admission_time_unix_ms: None,
            }).unwrap() + "\n";
            write_half.write_all(hs_resp.as_bytes()).await.unwrap();
            write_half.flush().await.unwrap();
            line.clear();

            // 2. Attach -> return SessionNotFound error
            assert!(reader.read_line(&mut line).await.unwrap() > 0);
            assert!(matches!(
                serde_json::from_str::<crate::daemon::DaemonRequest>(&line).unwrap(),
                crate::daemon::DaemonRequest::Attach { session_id: id, after_sequence: None }
                    if id == session_id
            ));
            let err_resp = serde_json::to_string(&crate::daemon::DaemonResponse::Error {
                code: Some("SESSION_NOT_FOUND".into()),
                message: "Session not found".into(),
                details: None,
            }).unwrap() + "\n";
            write_half.write_all(err_resp.as_bytes()).await.unwrap();
            write_half.flush().await.unwrap();
        };

        let action = async {
            let res = cmd_native_terminal_attach(
                app.handle().clone(),
                session_id.into(),
                None,
                None,
                None,
                Some(tuple),
                Some(1000),
            ).await;
            assert!(res.is_err(), "attach must fail when daemon returns error");
            assert_eq!(res.unwrap_err().code, IpcErrorCode::SessionNotFound);
            assert!(state.session_attempt_generation(session_id).is_none(), "session reservation must be rolled back on attach failure");
            assert!(!state.has_session(session_id), "session must not be leaked in sessions map");
        };

        tokio::join!(peer, action);
        release_clock.send(()).unwrap();
        clock_guard.await.unwrap();
        state.teardown();
    }

    #[test]
    fn resident_replay_watermark_overrides_stale_explicit_cursor() {
        assert_eq!(
            replay_request_cursor(Some(Some(5)), Some("2".into())),
            Some(5)
        );
        assert_eq!(replay_request_cursor(Some(None), Some("900".into())), None);
        assert_eq!(replay_request_cursor(None, Some("2".into())), Some(2));
    }
    use crate::native_terminal::{MouseButton, MousePosition, MouseRendererSize};

    #[test]
    fn ssh_reconnect_safety_desktop_native_command_generation_contract() {
        // Compile-time contract: every PTY-producing native command accepts the
        // generation captured by the producer, independently of platform modules.
        let _ = cmd_native_terminal_send_input::<tauri::test::MockRuntime>;
        let source = include_str!("native_terminal.rs");
        for name in ["send_input", "paste", "mouse", "scroll"] {
            let signature = source
                .split(&format!("pub async fn cmd_native_terminal_{name}<"))
                .nth(1)
                .unwrap()
                .split(") ->")
                .next()
                .unwrap();
            assert!(
                signature.contains("generation: Option<u64>"),
                "{name} must accept captured generation"
            );
        }
    }

    #[tokio::test]
    async fn pane_liveness_diagnostics_prearmed_producer_write_barrier() {
        let app = tauri::test::mock_builder()
            .manage(NativeTerminalSurfaceHostState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let state = app.state::<NativeTerminalSurfaceHostState>();

        // Attach an active session to surface host state so encode_attached_input succeeds
        // and establishes a real blocked producer on the write barrier.
        let (_output_tx, messages) = tokio::sync::mpsc::channel(1);
        let stream_task = tokio::spawn(std::future::pending());
        state
            .attach_daemon_attachment_with_bounds::<tauri::test::MockRuntime>(
                "session-test-barrier",
                crate::daemon::DaemonAttachment {
                    session_id: "session-test-barrier".into(),
                    epoch: 1,
                    start_sequence: Some(1),
                    end_sequence: Some(1),
                    gap: None,
                    history: bytes::Bytes::new(),
                    history_segments: Vec::new(),
                    pty_cols: Some(80),
                    pty_rows: Some(24),
                    remote_generation: None,
                    messages,
                    stream_task,
                },
                Some(app.handle().clone()),
                None,
            )
            .unwrap();

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let input = NativeTerminalInput::Text { text: "test input".into() };

        let recorded_entries = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink_entries = std::sync::Arc::clone(&recorded_entries);

        let _sink_guard = crate::ipc::debug::set_scoped_sink_for_operation("req-barrier-1", move |entry| {
            if entry.event.starts_with("terminal.surface.input.stage.") {
                sink_entries.lock().unwrap().push(entry.clone());
            }
        });

        let write_fut = send_native_terminal_input_with_stage_logging(
            app.handle(),
            &state,
            "session-test-barrier",
            &input,
            Some(1),
            Some("req-barrier-1".into()),
            |_bytes| async move {
                let _ = started_tx.send(());
                let _ = release_rx.await;
                Ok(())
            },
        );

        tokio::pin!(write_fut);

        tokio::select! {
            _ = &mut write_fut => panic!("write_fut completed before release"),
            res = started_rx => {
                assert!(res.is_ok(), "writer entered write_op and signaled barrier");
                let entries = recorded_entries.lock().unwrap();
                assert_eq!(entries.len(), 1, "backend_write_start must be emitted before write_op");
                assert_eq!(entries[0].event, "terminal.surface.input.stage.backend_write_start");
                assert_eq!(entries[0].details.get("operationId").and_then(|v| v.as_str()), Some("req-barrier-1"));
                assert_eq!(entries[0].details.get("sessionId").and_then(|v| v.as_str()), Some("session-test-barrier"));
                drop(entries);
                let _ = release_tx.send(());
            }
        }

        let result = write_fut.await;
        assert!(result.is_ok(), "production write pipeline settled successfully");

        let entries = recorded_entries.lock().unwrap();
        assert_eq!(entries.len(), 2, "both backend_write_start and backend_write must be emitted");
        assert_eq!(entries[1].event, "terminal.surface.input.stage.backend_write");
        assert_eq!(entries[1].details.get("operationId").and_then(|v| v.as_str()), Some("req-barrier-1"));
        assert_eq!(entries[1].details.get("success").and_then(|v| v.as_bool()), Some(true));

        state.teardown();
    }

    fn resize_req(session: &str, cols: u16, rows: u16) -> PtyGeometryRequest {
        PtyGeometryRequest { session_id: session.into(), cols, rows, redraw: false, resize: true }
    }

    fn redraw_req(session: &str, cols: u16, rows: u16) -> PtyGeometryRequest {
        PtyGeometryRequest { session_id: session.into(), cols, rows, redraw: true, resize: false }
    }

    #[tokio::test(start_paused = true)]
    async fn pty_redraw_nudges_size_and_restores_it_after_settling() {
        let (sender, receiver) = mpsc::unbounded_channel();
        sender.send(redraw_req("pane", 120, 40)).unwrap();
        drop(sender);
        let applied = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let observed = Arc::clone(&applied);
        let started = tokio::time::Instant::now();

        dispatch_pty_resizes(receiver, move |session, cols, rows| {
            observed.lock().push((session, cols, rows, started.elapsed()));
            std::future::ready(Ok(()))
        })
        .await;

        let applied = applied.lock();
        assert_eq!(applied.len(), 2, "redraw must issue a nudge and a restore: {applied:?}");
        assert_eq!((applied[0].1, applied[0].2), (120, 41), "nudge grows by one row");
        assert_eq!((applied[1].1, applied[1].2), (120, 40), "restore returns to the real size");
        assert!(
            applied[1].3 - applied[0].3 >= PTY_REDRAW_NUDGE_SETTLE,
            "the restore must wait out the settle window so Ink sees two distinct sizes"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn pty_redraw_restores_to_the_last_applied_size_not_the_grid() {
        let (sender, receiver) = mpsc::unbounded_channel();
        sender.send(resize_req("pane", 100, 30)).unwrap();
        let (applied_tx, applied_rx) = oneshot::channel();
        let mut applied_tx = Some(applied_tx);
        let applied = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let observed = Arc::clone(&applied);
        let worker = tokio::spawn(dispatch_pty_resizes(receiver, move |session, cols, rows| {
            observed.lock().push((session, cols, rows));
            if let Some(tx) = applied_tx.take() {
                tx.send(()).unwrap();
            }
            std::future::ready(Ok(()))
        }));
        tokio::time::timeout(std::time::Duration::from_secs(5), applied_rx)
            .await.unwrap().unwrap();
        sender.send(redraw_req("pane", 80, 24)).unwrap();
        drop(sender);
        worker.await.unwrap();

        assert_eq!(
            *applied.lock(),
            vec![
                ("pane".into(), 100, 30),
                ("pane".into(), 100, 31),
                ("pane".into(), 100, 30),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn pty_resize_during_redraw_settle_becomes_the_restore_size() {
        let (sender, receiver) = mpsc::unbounded_channel();
        sender.send(redraw_req("pane", 80, 24)).unwrap();
        let applied = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let observed = Arc::clone(&applied);
        let (nudged_tx, nudged_rx) = oneshot::channel();
        let mut nudged_tx = Some(nudged_tx);
        let worker = tokio::spawn(dispatch_pty_resizes(receiver, move |session, cols, rows| {
            observed.lock().push((session, cols, rows));
            if let Some(tx) = nudged_tx.take() {
                let _ = tx.send(());
            }
            std::future::ready(Ok(()))
        }));
        nudged_rx.await.unwrap();
        sender.send(resize_req("pane", 132, 50)).unwrap();
        drop(sender);
        worker.await.unwrap();

        assert_eq!(
            *applied.lock(),
            vec![("pane".into(), 80, 25), ("pane".into(), 132, 50)],
            "a resize arriving mid-settle is the final size, applied once"
        );
    }

    #[test]
    fn coalescing_keeps_the_redraw_flag_and_the_latest_size() {
        let mut pending = std::collections::VecDeque::new();
        merge_pty_request(&mut pending, resize_req("pane", 100, 30));
        merge_pty_request(&mut pending, redraw_req("pane", 80, 24));
        merge_pty_request(&mut pending, resize_req("pane", 120, 40));
        assert_eq!(pending.into_iter().collect::<Vec<_>>(), vec![PtyGeometryRequest {
            redraw: true, ..resize_req("pane", 120, 40)
        }]);
    }

    #[tokio::test(start_paused = true)]
    async fn redraw_merged_with_resize_keeps_the_new_target_after_an_applied_size() {
        let (sender, receiver) = mpsc::unbounded_channel();
        sender.send(resize_req("pane", 80, 24)).unwrap();
        let mut sender = Some(sender);
        let mut applied = Vec::new();

        dispatch_pty_resizes(receiver, |session, cols, rows| {
            applied.push((session, cols, rows));
            if let Some(sender) = sender.take() {
                sender.send(resize_req("pane", 120, 40)).unwrap();
                sender.send(redraw_req("pane", 80, 24)).unwrap();
            }
            std::future::ready(Ok(()))
        }).await;

        assert_eq!(applied, vec![
            ("pane".into(), 80, 24),
            ("pane".into(), 120, 41),
            ("pane".into(), 120, 40),
        ]);
    }


    #[tokio::test]
    async fn pty_resize_queue_keeps_latest_size_for_each_pane() {
        let (sender, receiver) = mpsc::unbounded_channel();
        sender.send(resize_req("left", 80, 24)).unwrap();
        sender.send(resize_req("right", 60, 24)).unwrap();
        sender.send(resize_req("left", 120, 40)).unwrap();
        sender.send(resize_req("right", 100, 30)).unwrap();
        drop(sender);
        let mut applied = Vec::new();

        dispatch_pty_resizes(receiver, |session, cols, rows| {
            applied.push((session, cols, rows));
            std::future::ready(Ok(()))
        })
        .await;

        assert_eq!(
            applied,
            vec![("left".into(), 120, 40), ("right".into(), 100, 30)]
        );
    }

    #[test]
    fn deferred_presentation_status_survives_the_ipc_boundary() {
        let receipt = into_ipc_receipt(
            "deferred".into(),
            NativeTerminalSurfaceReceipt {
                presented: false,
                render_deferred: true,
                render_suspended: false,
                cols: 80,
                rows: 24,
                rebuilt_rows: 0,
                reused_rows: 0,
                cursor_col: 0,
                cursor_row: 0,
                cell_width_px: 10,
                cell_height_px: 20,
                effective_scale_factor: Some(2.0),
            },
        );
        let json = serde_json::to_value(&receipt).unwrap();
        assert_eq!(json["renderDeferred"], true);
        assert_eq!(json["presented"], false);
    }

    #[tokio::test(start_paused = true)]
    async fn failed_pty_resize_retries_without_blocking_other_panes() {
        let (sender, receiver) = mpsc::unbounded_channel();
        sender.send(resize_req("left", 120, 40)).unwrap();
        sender.send(resize_req("right", 100, 30)).unwrap();
        drop(sender);
        let mut attempts = Vec::new();
        let mut failed = false;
        dispatch_pty_resizes(receiver, |session, cols, rows| {
            attempts.push((session.clone(), cols, rows));
            let result = if session == "left" && !failed {
                failed = true;
                Err(IpcError::internal("temporary transport failure"))
            } else {
                Ok(())
            };
            std::future::ready(result)
        }).await;
        assert_eq!(attempts, vec![
            ("left".into(), 120, 40),
            ("right".into(), 100, 30),
            ("left".into(), 120, 40),
        ]);
    }

    #[tokio::test(start_paused = true)]
    async fn failed_pty_redraw_restore_retries_the_real_size() {
        let (sender, receiver) = mpsc::unbounded_channel();
        sender.send(redraw_req("pane", 80, 24)).unwrap();
        drop(sender);
        let mut attempts = Vec::new();
        dispatch_pty_resizes(receiver, |session, cols, rows| {
            attempts.push((session, cols, rows));
            std::future::ready(if attempts.len() == 2 {
                Err(IpcError::internal("restore failed"))
            } else { Ok(()) })
        }).await;
        assert_eq!(attempts, vec![
            ("pane".into(), 80, 25), ("pane".into(), 80, 24), ("pane".into(), 80, 24),
        ]);
    }

    #[tokio::test]
    async fn pty_resize_queue_coalesces_updates_arriving_during_an_inflight_request() {
        let (sender, receiver) = mpsc::unbounded_channel();
        let (started, start) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let mut started = Some(started);
        let mut released = Some(released);
        let applied = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let observed = Arc::clone(&applied);
        let worker = tokio::spawn(dispatch_pty_resizes(
            receiver,
            move |session, cols, rows| {
                let observed = Arc::clone(&observed);
                let started = started.take();
                let released = released.take();
                async move {
                    observed.lock().push((session, cols, rows));
                    if let Some(started) = started {
                        started.send(()).unwrap();
                    }
                    if let Some(released) = released {
                        released.await.unwrap();
                    }
                    Ok(())
                }
            },
        ));
        sender.send(resize_req("left", 80, 24)).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), start)
            .await
            .unwrap()
            .unwrap();
        sender.send(resize_req("left", 90, 25)).unwrap();
        sender.send(resize_req("right", 60, 24)).unwrap();
        sender.send(resize_req("left", 120, 40)).unwrap();
        sender.send(resize_req("right", 100, 30)).unwrap();
        drop(sender);

        release.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .unwrap()
            .unwrap();

        let applied = applied.lock();
        let applied_for = |pane: &str| {
            applied
                .iter()
                .filter(|(session, _, _)| session == pane)
                .map(|(_, cols, rows)| (*cols, *rows))
                .collect::<Vec<_>>()
        };
        assert_eq!(applied_for("left"), vec![(80, 24), (120, 40)]);
        assert_eq!(applied_for("right"), vec![(100, 30)]);
        assert_eq!(applied.len(), 3);
    }

    #[tokio::test]
    async fn pty_resize_for_one_pane_does_not_wait_for_another_panes_inflight_resize() {
        let (sender, receiver) = mpsc::unbounded_channel();
        let (left_started_tx, left_started) = oneshot::channel();
        let (right_done_tx, right_done) = oneshot::channel();
        let (release_left, left_released) = oneshot::channel::<()>();
        let mut left_started_tx = Some(left_started_tx);
        let mut right_done_tx = Some(right_done_tx);
        let mut left_released = Some(left_released);
        let left_finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let left_finished_in_resize = Arc::clone(&left_finished);
        let worker = tokio::spawn(dispatch_pty_resizes(receiver, move |session, cols, rows| {
            let left = session == "left";
            let started = if left { left_started_tx.take() } else { None };
            let released = if left { left_released.take() } else { None };
            let done = if left { None } else { right_done_tx.take() };
            let left_finished = Arc::clone(&left_finished_in_resize);
            async move {
                if let Some(started) = started {
                    started.send(()).unwrap();
                }
                if let Some(released) = released {
                    released.await.unwrap();
                    left_finished.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                if let Some(done) = done {
                    done.send((session, cols, rows)).unwrap();
                }
                Ok(())
            }
        }));
        let bound = std::time::Duration::from_secs(5);

        sender.send(resize_req("left", 120, 40)).unwrap();
        tokio::time::timeout(bound, left_started).await.unwrap().unwrap();
        sender.send(resize_req("right", 100, 30)).unwrap();
        let right = tokio::time::timeout(bound, right_done).await.unwrap().unwrap();

        assert_eq!(right, ("right".into(), 100, 30));
        assert!(
            !left_finished.load(std::sync::atomic::Ordering::SeqCst),
            "the right pane resized while the left pane's resize was still in flight"
        );
        release_left.send(()).unwrap();
        drop(sender);
        tokio::time::timeout(bound, worker).await.unwrap().unwrap();
        assert!(left_finished.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn wire_receipt_reports_effective_presentation_scale() {
        use crate::native_terminal::composition::SurfacePresentationGeometry;

        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "wire-receipt-scale";
        let bounds = SurfacePresentationGeometry::WaylandSubsurface
            .resolve(LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 480.0,
                scale_factor: 1.5,
            })
            .expect("resolved presentation bounds");
        state
            .prepare_session_layout(
                NativeTerminalBoundsRequest {
                    session_id: session_id.into(),
                    bounds,
                },
                CellMetrics {
                    width_px: 16,
                    height_px: 32,
                },
            )
            .expect("stored layout");
        assert_eq!(
            state
                .session_logical_bounds(session_id)
                .unwrap()
                .scale_factor,
            2.0
        );

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let receipt = state
            .get_receipt(&window.as_ref().window(), session_id)
            .expect("session receipt");
        let json = serde_json::to_value(into_ipc_receipt(session_id.into(), receipt)).unwrap();
        state.teardown();

        assert_eq!(json["cellWidthPx"], 16);
        assert_eq!(json["cellHeightPx"], 32);
        assert_eq!(json["effectiveScaleFactor"], 2.0);
    }

    #[test]
    fn bounds_receipt_serializes_actual_presentation_status() {
        for (presented, effective_scale_factor) in [
            (false, None),
            (false, Some(2.0)),
            (true, Some(1.5)),
            (true, Some(2.0)),
        ] {
            let receipt = into_ipc_receipt(
                "presentation".into(),
                NativeTerminalSurfaceReceipt {
                    presented,
                    render_deferred: false,
                    render_suspended: false,
                    cols: 80,
                    rows: 24,
                    rebuilt_rows: 0,
                    reused_rows: 0,
                    cursor_col: 0,
                    cursor_row: 0,
                    cell_width_px: 10,
                    cell_height_px: 20,
                    effective_scale_factor,
                },
            );
            let json = serde_json::to_value(&receipt).unwrap();
            assert_eq!(json["presented"], presented);
            assert_eq!(
                json["effectiveScaleFactor"].as_f64(),
                effective_scale_factor
            );
            assert_eq!(
                json.get("effectiveScaleFactor").is_some(),
                effective_scale_factor.is_some(),
            );
            assert_eq!(
                serde_json::from_value::<NativeTerminalBoundsReceipt>(json).unwrap(),
                receipt,
            );
        }
    }

    fn test_mouse_event(size: Option<MouseRendererSize>) -> MouseEvent {
        MouseEvent {
            action: MouseAction::Press,
            button: Some(MouseButton::Left),
            position: MousePosition { x: 10.5, y: 20.25 },
            modifiers: Default::default(),
            size,
            timestamp_ns: None,
        }
    }

    #[test]
    fn authoritative_mouse_event_uses_rendered_physical_size_and_cell_metrics() {
        let bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
            scale_factor: 2.0,
        };
        let cell_metrics = crate::native_terminal::composition::CellMetrics {
            width_px: 10,
            height_px: 20,
        };

        let event = authoritative_mouse_event(&test_mouse_event(None), &bounds, &cell_metrics)
            .expect("authoritative mouse event");
        let size = event.size.expect("authoritative renderer size");

        assert_eq!(size.screen_width, 1600);
        assert_eq!(size.screen_height, 1200);
        assert_eq!(size.cell_width, 10);
        assert_eq!(size.cell_height, 20);
    }

    #[test]
    fn authoritative_mouse_event_scales_logical_position_to_physical_pixels() {
        let bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
            scale_factor: 2.0,
        };
        let cell_metrics = crate::native_terminal::composition::CellMetrics {
            width_px: 10,
            height_px: 20,
        };

        let event = authoritative_mouse_event(&test_mouse_event(None), &bounds, &cell_metrics)
            .expect("authoritative mouse event");

        assert_eq!(event.position.x, 21.0);
        assert_eq!(event.position.y, 40.5);
    }

    #[test]
    fn authoritative_mouse_event_replaces_frontend_renderer_size() {
        let bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
            scale_factor: 2.0,
        };
        let cell_metrics = crate::native_terminal::composition::CellMetrics {
            width_px: 10,
            height_px: 20,
        };
        let bogus_size = MouseRendererSize {
            screen_width: 1,
            screen_height: 1,
            cell_width: 999,
            cell_height: 999,
            padding_top: 8,
            padding_bottom: 8,
            padding_right: 8,
            padding_left: 8,
        };

        let event =
            authoritative_mouse_event(&test_mouse_event(Some(bogus_size)), &bounds, &cell_metrics)
                .expect("authoritative mouse event");

        assert_eq!(
            event.size,
            Some(MouseRendererSize {
                screen_width: 1600,
                screen_height: 1200,
                cell_width: 10,
                cell_height: 20,
                padding_top: 0,
                padding_bottom: 0,
                padding_right: 0,
                padding_left: 0,
            })
        );
    }

    #[test]
    fn rejected_native_input_reports_that_no_pty_write_occurred() {
        let state = NativeTerminalSurfaceHostState::default();
        let input = NativeTerminalInput::Text {
            text: "\u{3}".to_string(),
        };

        let error = encode_attached_native_input(&state, "unattached-input", &input)
            .expect_err("an unattached surface cannot accept input");
        assert_eq!(error.code, crate::ipc::error::IpcErrorCode::SessionNotFound);
        assert_eq!(
            error.details,
            Some(serde_json::json!({ "inputWritten": false }))
        );
    }

    #[test]
    fn test_classify_clipboard_content_prefers_non_empty_text() {
        assert_eq!(
            classify_clipboard_content(Some("hello".to_string()), &[]),
            NativeTerminalClipboardContent::Text {
                text: "hello".to_string()
            }
        );
        assert_eq!(
            classify_clipboard_content(Some("hello".to_string()), &["public.png".to_string()]),
            NativeTerminalClipboardContent::Text {
                text: "hello".to_string()
            }
        );
        assert_eq!(
            classify_clipboard_content(Some("hello".to_string()), &["public.file-url".to_string()]),
            NativeTerminalClipboardContent::Text {
                text: "hello".to_string()
            }
        );
    }

    #[test]
    fn test_classify_clipboard_content_image_when_image_uti_or_non_empty_types() {
        assert_eq!(
            classify_clipboard_content(None, &["public.png".to_string()]),
            NativeTerminalClipboardContent::Image
        );
        assert_eq!(
            classify_clipboard_content(Some("".to_string()), &["public.png".to_string()]),
            NativeTerminalClipboardContent::Image
        );
        assert_eq!(
            classify_clipboard_content(None, &["public.jpeg".to_string()]),
            NativeTerminalClipboardContent::Image
        );
        assert_eq!(
            classify_clipboard_content(None, &["public.file-url".to_string()]),
            NativeTerminalClipboardContent::Image
        );
        assert_eq!(
            classify_clipboard_content(Some("".to_string()), &["public.file-url".to_string()]),
            NativeTerminalClipboardContent::Image
        );
    }

    #[test]
    fn test_classify_clipboard_content_empty_when_no_text_and_empty_types() {
        assert_eq!(
            classify_clipboard_content(None, &[]),
            NativeTerminalClipboardContent::Empty
        );
        assert_eq!(
            classify_clipboard_content(Some("".to_string()), &[]),
            NativeTerminalClipboardContent::Empty
        );
    }

    #[test]
    fn test_clipboard_content_serde_tagged_format() {
        let text_payload = NativeTerminalClipboardContent::Text {
            text: "pasted value".to_string(),
        };
        let text_json = serde_json::to_string(&text_payload).expect("serialize text");
        assert_eq!(text_json, r#"{"kind":"text","text":"pasted value"}"#);
        assert_eq!(
            serde_json::from_str::<NativeTerminalClipboardContent>(&text_json)
                .expect("deserialize text"),
            text_payload
        );

        let image_payload = NativeTerminalClipboardContent::Image;
        let image_json = serde_json::to_string(&image_payload).expect("serialize image");
        assert_eq!(image_json, r#"{"kind":"image"}"#);
        assert_eq!(
            serde_json::from_str::<NativeTerminalClipboardContent>(&image_json)
                .expect("deserialize image"),
            image_payload
        );

        let empty_payload = NativeTerminalClipboardContent::Empty;
        let empty_json = serde_json::to_string(&empty_payload).expect("serialize empty");
        assert_eq!(empty_json, r#"{"kind":"empty"}"#);
        assert_eq!(
            serde_json::from_str::<NativeTerminalClipboardContent>(&empty_json)
                .expect("deserialize empty"),
            empty_payload
        );
    }

    #[test]
    fn test_decode_windows_clipboard_utf16() {
        let utf16: Vec<u16> = "Hello, world!\0".encode_utf16().collect();
        assert_eq!(
            decode_windows_clipboard_utf16(&utf16),
            Some("Hello, world!".to_string())
        );

        let utf16_emoji: Vec<u16> = "🚀 Ferryx Clipboard 🦀\0".encode_utf16().collect();
        assert_eq!(
            decode_windows_clipboard_utf16(&utf16_emoji),
            Some("🚀 Ferryx Clipboard 🦀".to_string())
        );

        let utf16_no_null: Vec<u16> = "No null terminator".encode_utf16().collect();
        assert_eq!(
            decode_windows_clipboard_utf16(&utf16_no_null),
            Some("No null terminator".to_string())
        );

        assert_eq!(decode_windows_clipboard_utf16(&[]), None);
        assert_eq!(decode_windows_clipboard_utf16(&[0, 0, 0]), None);
    }

    #[test]
    fn test_encode_windows_clipboard_utf16() {
        let text = "Hello Windows Clipboard! 🚀🦀";
        let encoded = encode_windows_clipboard_utf16(text);
        assert_eq!(encoded.last(), Some(&0u16), "must be null-terminated");
        assert_eq!(
            decode_windows_clipboard_utf16(&encoded),
            Some(text.to_string()),
            "encoded utf16 must round-trip through decode_windows_clipboard_utf16"
        );

        let empty_encoded = encode_windows_clipboard_utf16("");
        assert_eq!(empty_encoded, vec![0u16]);
    }

    #[test]
    fn test_write_linux_clipboard_with_runner() {
        // 1. Empty text immediately returns false without invoking any runner
        let mut invoked: Vec<(String, Vec<String>)> = Vec::new();
        assert!(
            !write_linux_clipboard_with_runner("", |cmd, _args, _stdin| {
                invoked.push((cmd.to_string(), Vec::new()));
                true
            }),
            "empty text must return false"
        );
        assert!(invoked.is_empty(), "runner must not be invoked for empty text");

        // 2. Wayland success short-circuits without attempting X11 helpers
        invoked.clear();
        let success = write_linux_clipboard_with_runner("selection", |cmd, args, stdin| {
            invoked.push((cmd.to_string(), args.iter().map(|a| a.to_string()).collect::<Vec<String>>()));
            assert_eq!(stdin, b"selection");
            cmd == "wl-copy"
        });
        assert!(success);
        assert_eq!(invoked, vec![("wl-copy".to_string(), vec![])]);

        // 3. Wayland failure falls back to xclip with proper flags
        invoked.clear();
        let success = write_linux_clipboard_with_runner("selection", |cmd, args, stdin| {
            invoked.push((cmd.to_string(), args.iter().map(|a| a.to_string()).collect::<Vec<String>>()));
            assert_eq!(stdin, b"selection");
            cmd == "xclip"
        });
        assert!(success);
        assert_eq!(
            invoked,
            vec![
                ("wl-copy".to_string(), vec![]),
                (
                    "xclip".to_string(),
                    vec!["-selection".to_string(), "clipboard".to_string(), "-i".to_string()]
                ),
            ]
        );

        // 4. Wayland and xclip failure falls back to xsel
        invoked.clear();
        let success = write_linux_clipboard_with_runner("selection", |cmd, args, stdin| {
            invoked.push((cmd.to_string(), args.iter().map(|a| a.to_string()).collect::<Vec<String>>()));
            assert_eq!(stdin, b"selection");
            cmd == "xsel"
        });
        assert!(success);
        assert_eq!(
            invoked,
            vec![
                ("wl-copy".to_string(), vec![]),
                (
                    "xclip".to_string(),
                    vec!["-selection".to_string(), "clipboard".to_string(), "-i".to_string()]
                ),
                (
                    "xsel".to_string(),
                    vec!["--clipboard".to_string(), "--input".to_string()]
                ),
            ]
        );

        // 5. All helpers fail returns false
        invoked.clear();
        let success = write_linux_clipboard_with_runner("selection", |cmd, args, _stdin| {
            invoked.push((cmd.to_string(), args.iter().map(|a| a.to_string()).collect::<Vec<String>>()));
            false
        });
        assert!(!success);
        assert_eq!(invoked.len(), 3);
    }

    #[test]
    fn test_is_windows_image_or_file_format() {
        assert!(is_windows_image_or_file_format(CF_BITMAP_ID, None));
        assert!(is_windows_image_or_file_format(CF_DIB_ID, None));
        assert!(is_windows_image_or_file_format(CF_DIBV5_ID, None));
        assert!(is_windows_image_or_file_format(CF_HDROP_ID, None));
        assert!(is_windows_image_or_file_format(49152, Some("PNG")));
        assert!(is_windows_image_or_file_format(49153, Some("image/png")));
        assert!(is_windows_image_or_file_format(49154, Some("image/jpeg")));
        assert!(is_windows_image_or_file_format(49155, Some("image/webp")));
        assert!(is_windows_image_or_file_format(49156, Some("image/gif")));
        assert!(is_windows_image_or_file_format(49157, Some("FileNameW")));
        assert!(is_windows_image_or_file_format(
            49158,
            Some("FileGroupDescriptorW")
        ));
        assert!(is_windows_image_or_file_format(49159, Some("FileContents")));
        assert!(!is_windows_image_or_file_format(CF_UNICODETEXT_ID, None));
        assert!(!is_windows_image_or_file_format(CF_TEXT_ID, None));
        assert!(!is_windows_image_or_file_format(
            49160,
            Some("CustomNonImageFormat")
        ));
    }

    #[test]
    fn test_classify_windows_clipboard_prefers_text() {
        let text = Some("Hello Windows".to_string());
        let format_ids = vec![CF_UNICODETEXT_ID, CF_BITMAP_ID];
        let format_names = vec!["PNG".to_string()];
        assert_eq!(
            classify_windows_clipboard(text, &format_ids, &format_names),
            NativeTerminalClipboardContent::Text {
                text: "Hello Windows".to_string()
            }
        );
    }

    #[test]
    fn test_classify_windows_clipboard_image_when_bitmap_dib_hdrop_or_custom_image() {
        assert_eq!(
            classify_windows_clipboard(None, &[CF_BITMAP_ID], &[]),
            NativeTerminalClipboardContent::Image
        );
        assert_eq!(
            classify_windows_clipboard(None, &[CF_DIB_ID], &[]),
            NativeTerminalClipboardContent::Image
        );
        assert_eq!(
            classify_windows_clipboard(None, &[CF_DIBV5_ID], &[]),
            NativeTerminalClipboardContent::Image
        );
        assert_eq!(
            classify_windows_clipboard(None, &[CF_HDROP_ID], &[]),
            NativeTerminalClipboardContent::Image
        );
        assert_eq!(
            classify_windows_clipboard(None, &[49152], &["PNG".to_string()]),
            NativeTerminalClipboardContent::Image
        );
        assert_eq!(
            classify_windows_clipboard(Some("".to_string()), &[CF_BITMAP_ID], &[]),
            NativeTerminalClipboardContent::Image
        );
    }

    #[test]
    fn test_classify_windows_clipboard_empty_when_no_text_or_image() {
        assert_eq!(
            classify_windows_clipboard(None, &[], &[]),
            NativeTerminalClipboardContent::Empty
        );
        assert_eq!(
            classify_windows_clipboard(Some("".to_string()), &[], &[]),
            NativeTerminalClipboardContent::Empty
        );
    }

    #[test]
    fn test_is_linux_image_clipboard_type() {
        assert!(is_linux_image_clipboard_type("image/png"));
        assert!(is_linux_image_clipboard_type("image/jpeg"));
        assert!(is_linux_image_clipboard_type("IMAGE/TIFF"));
        assert!(is_linux_image_clipboard_type(" image/webp "));
        assert!(!is_linux_image_clipboard_type("text/plain;charset=utf-8"));
        assert!(!is_linux_image_clipboard_type("text/uri-list"));
        assert!(!is_linux_image_clipboard_type(""));
    }

    #[test]
    fn test_linux_clipboard_advertises_text() {
        assert!(linux_clipboard_advertises_text(&["text/plain".to_string()]));
        assert!(linux_clipboard_advertises_text(&[
            "text/plain;charset=utf-8".to_string(),
            "text/html".to_string()
        ]));
        assert!(linux_clipboard_advertises_text(&["UTF8_STRING".to_string()]));
        assert!(linux_clipboard_advertises_text(&["STRING".to_string()]));
        assert!(linux_clipboard_advertises_text(&["TEXT".to_string()]));
        assert!(linux_clipboard_advertises_text(&[" text/uri-list ".to_string()]));
        assert!(!linux_clipboard_advertises_text(&["image/png".to_string()]));
        assert!(!linux_clipboard_advertises_text(&[
            "image/png".to_string(),
            "image/bmp".to_string()
        ]));
        assert!(!linux_clipboard_advertises_text(&[]));
        assert!(!linux_clipboard_advertises_text(&["TARGETS".to_string()]));
    }

    #[test]
    fn test_linux_clipboard_backend_probe_arguments_match_each_family() {
        assert_eq!(LinuxClipboardBackend::Wayland.program(), "wl-paste");
        assert_eq!(LinuxClipboardBackend::Wayland.type_list_args(), &["--list-types"]);
        assert_eq!(LinuxClipboardBackend::Wayland.text_args(), &["--no-newline"]);
        assert_eq!(LinuxClipboardBackend::X11.program(), "xclip");
        assert_eq!(
            LinuxClipboardBackend::X11.type_list_args(),
            &["-selection", "clipboard", "-t", "TARGETS", "-o"]
        );
        assert_eq!(LinuxClipboardBackend::X11.text_args(), &["-selection", "clipboard", "-o"]);
    }

    #[test]
    fn test_select_linux_clipboard_backend_keeps_the_answering_backend() {
        // The first backend that answers wins, even when it advertises nothing: consulting the
        // other family afterwards would gate on one clipboard while decoding another.
        let mut probed = Vec::new();
        let selected = select_linux_clipboard_backend(|backend| {
            probed.push(backend);
            (backend == LinuxClipboardBackend::Wayland).then_some(Vec::new())
        });
        assert_eq!(selected, Some((LinuxClipboardBackend::Wayland, Vec::new())));
        assert_eq!(probed, vec![LinuxClipboardBackend::Wayland]);

        // Wayland missing: the X11 answer is the one carried forward, descriptors included.
        let selected = select_linux_clipboard_backend(|backend| {
            (backend == LinuxClipboardBackend::X11).then_some(b"image/png\ntext/plain\n".to_vec())
        });
        let expected = (
            LinuxClipboardBackend::X11,
            vec!["image/png".to_string(), "text/plain".to_string()],
        );
        assert_eq!(selected, Some(expected));

        // Neither helper installed: no backend and no types.
        assert_eq!(select_linux_clipboard_backend(|_| None), None);
    }

    #[test]
    fn test_classify_linux_clipboard_prefers_text_over_image_types() {
        assert_eq!(
            classify_linux_clipboard(
                Some("Hello Linux".to_string()),
                &["image/png".to_string(), "text/plain".to_string()]
            ),
            NativeTerminalClipboardContent::Text {
                text: "Hello Linux".to_string()
            }
        );
    }

    #[test]
    fn test_classify_linux_clipboard_image_only_for_image_types() {
        assert_eq!(
            classify_linux_clipboard(None, &["image/png".to_string()]),
            NativeTerminalClipboardContent::Image
        );
        assert_eq!(
            classify_linux_clipboard(Some("".to_string()), &["image/jpeg".to_string()]),
            NativeTerminalClipboardContent::Image
        );
        assert_eq!(
            classify_linux_clipboard(None, &["text/uri-list".to_string()]),
            NativeTerminalClipboardContent::Empty
        );
        assert_eq!(
            classify_linux_clipboard(None, &[]),
            NativeTerminalClipboardContent::Empty
        );
    }

    #[test]
    fn test_classify_linux_clipboard_image_wins_when_only_image_types_advertised() {
        // The untyped text read can emit raw image bytes; the lossy decode makes them non-empty.
        assert_eq!(
            classify_linux_clipboard(Some("PNG\u{fffd}".to_string()), &["image/png".to_string()]),
            NativeTerminalClipboardContent::Image
        );
        assert_eq!(
            classify_linux_clipboard(
                Some("garbage".to_string()),
                &["image/jpeg".to_string(), "image/bmp".to_string()]
            ),
            NativeTerminalClipboardContent::Image
        );
    }

    #[test]
    fn test_decode_linux_clipboard_descriptors_drops_blank_lines() {
        assert_eq!(
            decode_linux_clipboard_descriptors(b"image/png\n\ntext/plain;charset=utf-8\n"),
            vec![
                "image/png".to_string(),
                "text/plain;charset=utf-8".to_string()
            ]
        );
        assert!(decode_linux_clipboard_descriptors(b"\n\n").is_empty());
        assert!(decode_linux_clipboard_descriptors(b"").is_empty());
    }

    #[test]
    fn test_decode_linux_clipboard_text_never_panics_on_invalid_utf8() {
        assert_eq!(
            decode_linux_clipboard_text(b"clipboard text"),
            Some("clipboard text".to_string())
        );
        assert_eq!(
            decode_linux_clipboard_text("\u{1F680} Ferryx".as_bytes()),
            Some("\u{1F680} Ferryx".to_string())
        );
        assert_eq!(decode_linux_clipboard_text(b""), None);
        assert_eq!(
            decode_linux_clipboard_text(&[0xff, 0xfe]),
            Some("\u{fffd}\u{fffd}".to_string())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_native_terminal_drag_selection_written_to_unique_pasteboard() {
        use crate::native_terminal::{
            MouseAction, MouseButton, MouseEvent, MouseRendererSize, NativeTerminal,
        };
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};

        // 1. Create a native terminal instance and feed it text
        let mut terminal = NativeTerminal::new(80, 24).expect("create native terminal");
        terminal
            .feed(b"first cmd-c selection to pasteboard")
            .expect("feed selectable text");

        let size = MouseRendererSize {
            screen_width: 800,
            screen_height: 480,
            cell_width: 10,
            cell_height: 20,
            padding_top: 0,
            padding_bottom: 0,
            padding_right: 0,
            padding_left: 0,
        };

        let make_event = |action, x, y| -> MouseEvent {
            serde_json::from_value(serde_json::json!({
                "action": action,
                "button": (action == MouseAction::Press).then_some(MouseButton::Left),
                "position": { "x": x, "y": y },
                "modifiers": {
                    "shift": false, "ctrl": false, "alt": false,
                    "superKey": false, "capsLock": false, "numLock": false,
                },
                "size": size,
            }))
            .expect("decode pointer event")
        };

        // 2. Perform mouse drag selection across the text
        terminal
            .handle_mouse_gesture(&make_event(MouseAction::Press, 5.0, 10.0))
            .expect("start pointer drag selection");
        terminal
            .handle_mouse_gesture(&make_event(MouseAction::Motion, 360.0, 10.0))
            .expect("extend pointer drag selection");
        terminal
            .handle_mouse_gesture(&make_event(MouseAction::Release, 360.0, 10.0))
            .expect("finish pointer drag selection");

        let selection = terminal
            .selection_text()
            .expect("extract selection text")
            .expect("selection must be present");
        assert_eq!(selection, "first cmd-c selection to pasteboard");

        // 3. Create an isolated uniquely-named pasteboard (NEVER touches user's general pasteboard)
        let pasteboard = NSPasteboard::pasteboardWithUniqueName();

        // 4. Exercise writer logic directly against the isolated pasteboard
        let written = write_to_pasteboard(&pasteboard, &selection);
        assert!(
            written,
            "writing non-empty selection to pasteboard must succeed"
        );

        // 5. Read back from the unique pasteboard and verify round-trip fidelity
        // SAFETY: NSPasteboardTypeString is AppKit's immutable, process-lifetime
        // NSString constant. The private pasteboard remains retained during the read.
        let read_back = unsafe {
            pasteboard
                .stringForType(NSPasteboardTypeString)
                .map(|s| s.to_string())
        };
        assert_eq!(
            read_back,
            Some("first cmd-c selection to pasteboard".to_string())
        );

        // 6. Verify that an empty selection does NOT clear or overwrite existing pasteboard content
        let not_written = write_to_pasteboard(&pasteboard, "");
        assert!(!not_written, "empty text must not be written");

        // SAFETY: The same immutable AppKit constant and retained private pasteboard
        // are still valid; no general pasteboard is read or modified by this test.
        let unchanged = unsafe {
            pasteboard
                .stringForType(NSPasteboardTypeString)
                .map(|s| s.to_string())
        };
        pasteboard.clearContents();
        assert_eq!(
            unchanged,
            Some("first cmd-c selection to pasteboard".to_string()),
            "pasteboard content must remain untouched on empty selection"
        );
    }

    #[tokio::test]
    async fn post_write_receipt_failure_retains_input_written_true_without_fabricated_bounds() {
        use crate::native_terminal::composition::SurfacePresentationGeometry;

        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "test-post-write-receipt";
        let bounds = SurfacePresentationGeometry::WaylandSubsurface
            .resolve(LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 480.0,
                scale_factor: 1.0,
            })
            .expect("resolved presentation bounds");
        state
            .prepare_session_layout(
                NativeTerminalBoundsRequest {
                    session_id: session_id.into(),
                    bounds,
                },
                CellMetrics {
                    width_px: 16,
                    height_px: 32,
                },
            )
            .expect("stored layout");

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();

        let input = NativeTerminalInput::Text {
            text: "a".to_string(),
        };

        let mut write_invoked = false;
        let result = send_native_terminal_input_with_writer(
            app.handle(),
            &state,
            session_id,
            &input,
            |_bytes| {
                write_invoked = true;
                async { Ok(()) }
            },
        )
        .await;

        assert!(write_invoked, "terminal write must have been executed");
        // The receipt is no longer awaited inline, so a keystroke succeeds even when no main
        // window exists to produce one. Previously this surfaced as a command error carrying
        // inputWritten: true, which forced every caller to distinguish "the key was lost" from
        // "only the IME anchor was lost" -- and made the next keystroke wait for the rendezvous.
        result.expect("a written keystroke must succeed even when no receipt can be collected");
    }

    // Task 3 (local-split-qa): the private barrier channel drives the REAL
    // write-stage producer through the production pipeline, without windows.
    #[cfg(feature = "local-split-qa")]
    mod qa_barrier_writer_tests {
        use super::*;
        use crate::ipc::qa_barrier::{
            active_channel, deactivate, install, QaBarrierChannel, PRODUCER_ID, WRITE_BARRIER,
        };
        use serde_json::Value;

        const RUN_ID: &str = "qa-run-writer";
        const OPERATION_ID: &str = "qa-op-writer";

        struct ChannelGuard;
        impl Drop for ChannelGuard {
            fn drop(&mut self) {
                deactivate();
            }
        }

        fn arm_backend_write(dir: &std::path::Path) {
            std::fs::write(
                dir.join(format!("{WRITE_BARRIER}.arm.json")),
                serde_json::to_string(&serde_json::json!({
                    "name": WRITE_BARRIER,
                    "runId": RUN_ID,
                    "operationId": OPERATION_ID,
                    "deadlineMs": 5_000,
                }))
                .unwrap(),
            )
            .unwrap();
        }

        fn release_backend_write(dir: &std::path::Path) {
            std::fs::write(
                dir.join(format!("{WRITE_BARRIER}.release.json")),
                serde_json::to_string(&serde_json::json!({
                    "name": WRITE_BARRIER,
                    "runId": RUN_ID,
                    "operationId": OPERATION_ID,
                    "releasedAt": "2026-10-03T00:00:01.000Z",
                }))
                .unwrap(),
            )
            .unwrap();
        }

        #[tokio::test(start_paused = true)]
        async fn real_write_stage_holds_and_recovers_through_private_barrier() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("barriers");
            std::fs::create_dir_all(&dir).unwrap();
            // The runner arms BEFORE launch: startup scan must ack it.
            arm_backend_write(&dir);
            install(QaBarrierChannel::new(dir.clone(), RUN_ID.to_string()));
            let channel = active_channel().expect("channel installed");
            let _guard = ChannelGuard;
            let (acked, rejected) = channel.scan_and_ack_arms();
            assert_eq!(acked, vec![WRITE_BARRIER.to_string()]);
            assert!(rejected.is_empty());

            // Prearm exact event subscribers BEFORE driving the write pipeline
            let mut held_rx = channel.subscribe_held();
            let mut receipt_rx = channel.subscribe_receipts();

            let app = tauri::test::mock_builder()
                .manage(NativeTerminalSurfaceHostState::default())
                .build(tauri::test::mock_context(tauri::test::noop_assets()))
                .unwrap();
            let state = app.state::<NativeTerminalSurfaceHostState>();
            let (_output_tx, messages) = tokio::sync::mpsc::channel(1);
            let stream_task = tokio::spawn(std::future::pending());
            state
                .attach_daemon_attachment_with_bounds::<tauri::test::MockRuntime>(
                    "session-qa-writer",
                    crate::daemon::DaemonAttachment {
                        session_id: "session-qa-writer".into(),
                        epoch: 1,
                        start_sequence: Some(1),
                        end_sequence: Some(1),
                        gap: None,
                        history: bytes::Bytes::new(),
                        history_segments: Vec::new(),
                        pty_cols: Some(80),
                        pty_rows: Some(24),
                        remote_generation: None,
                        messages,
                        stream_task,
                    },
                    Some(app.handle().clone()),
                    None,
                )
                .unwrap();

            let input = NativeTerminalInput::Text { text: "qa probe".into() };
            let write_fut = send_native_terminal_input_with_stage_logging(
                &app.handle(),
                &state,
                "session-qa-writer",
                &input,
                Some(1),
                Some(OPERATION_ID.into()),
                |_bytes| async { Ok(()) },
            );
            tokio::pin!(write_fut);

            // Await prearmed exact held event from the REAL parked producer
            let held_event = tokio::select! {
                event = QaBarrierChannel::await_held_event(&mut held_rx, WRITE_BARRIER) => event,
                _ = &mut write_fut => panic!("write settled before held event"),
            };
            let held = held_event.payload;
            assert_eq!(held["runId"], serde_json::json!(RUN_ID));
            assert_eq!(held["operationId"], serde_json::json!(OPERATION_ID));
            assert_eq!(held["producer"], serde_json::json!(PRODUCER_ID));
            assert!(held["producerPid"].as_u64().is_some());
            assert_eq!(held["sessionId"], serde_json::json!("session-qa-writer"));
            assert_eq!(held["classifierVerdict"], serde_json::json!("BlockedInIpcWrite"));
            assert!(
                held["writePendingMs"].as_u64().unwrap() > 250,
                "held verdict must rest on measured pending age past the classifier threshold"
            );
            assert!(dir.join(format!("{WRITE_BARRIER}.held.json")).exists());

            // Await prearmed exact receipt event (line 1: held)
            let held_receipt = tokio::select! {
                receipt = QaBarrierChannel::await_receipt_event(&mut receipt_rx, WRITE_BARRIER, 1) => receipt,
                _ = &mut write_fut => panic!("write settled before held receipt"),
            };
            assert_eq!(held_receipt["classifierVerdict"], serde_json::json!("BlockedInIpcWrite"));
            assert_eq!(held_receipt["operationId"], serde_json::json!(OPERATION_ID));
            assert!(held_receipt["snapshot"]["writePendingMs"].as_u64().unwrap() > 250);

            // The write stays parked until the correlated release.
            tokio::select! {
                _ = &mut write_fut => panic!("write settled before release"),
                _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {}
            }
            release_backend_write(&dir);
            write_fut.await.expect("production write pipeline settled");

            // Await prearmed exact receipt event (line 2: settled)
            // Recovery receipt: fresh truthful classification plus actual
            // stage-progress evidence; Idle is never forced.
            let settled = QaBarrierChannel::await_receipt_event(&mut receipt_rx, WRITE_BARRIER, 2).await;
            assert_eq!(settled["stage"], serde_json::json!("backend_write_settled"));
            assert_eq!(
                settled["stageProgress"]["backendWriteCompleted"],
                serde_json::json!(true)
            );
            assert_eq!(settled["stageProgress"]["success"], serde_json::json!(true));
            assert_eq!(
                settled["stageProgress"]["releaseOutcome"],
                serde_json::json!("released")
            );
            let verdict = settled["classifierVerdict"].as_str().unwrap();
            match verdict {
                "Idle" => assert_eq!(settled["evidenceMissing"], serde_json::json!(false)),
                "Unknown" => {
                    assert_eq!(settled["evidenceMissing"], serde_json::json!(true));
                    assert!(
                        !settled["evidenceMissingFields"]
                            .as_array()
                            .unwrap()
                            .is_empty()
                    );
                }
                other => panic!("unexpected recovery verdict {other}"),
            }

            state.teardown();
        }

        #[tokio::test(start_paused = true)]
        async fn wrong_operation_request_id_never_holds_the_barrier() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("barriers");
            std::fs::create_dir_all(&dir).unwrap();
            arm_backend_write(&dir);
            install(QaBarrierChannel::new(dir.clone(), RUN_ID.to_string()));
            let channel = active_channel().expect("channel installed");
            let _guard = ChannelGuard;
            channel.scan_and_ack_arms();

            let app = tauri::test::mock_builder()
                .manage(NativeTerminalSurfaceHostState::default())
                .build(tauri::test::mock_context(tauri::test::noop_assets()))
                .unwrap();
            let state = app.state::<NativeTerminalSurfaceHostState>();
            let (_output_tx, messages) = tokio::sync::mpsc::channel(1);
            let stream_task = tokio::spawn(std::future::pending());
            state
                .attach_daemon_attachment_with_bounds::<tauri::test::MockRuntime>(
                    "session-qa-wrong-op",
                    crate::daemon::DaemonAttachment {
                        session_id: "session-qa-wrong-op".into(),
                        epoch: 1,
                        start_sequence: Some(1),
                        end_sequence: Some(1),
                        gap: None,
                        history: bytes::Bytes::new(),
                        history_segments: Vec::new(),
                        pty_cols: Some(80),
                        pty_rows: Some(24),
                        remote_generation: None,
                        messages,
                        stream_task,
                    },
                    Some(app.handle().clone()),
                    None,
                )
                .unwrap();
            let input = NativeTerminalInput::Text { text: "qa probe".into() };
            // A request id that is NOT the runner's operation nonce must not
            // hold the barrier or fabricate any emission.
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                send_native_terminal_input_with_stage_logging(
                    &app.handle(),
                    &state,
                    "session-qa-wrong-op",
                    &input,
                    Some(1),
                    Some("req-unrelated".into()),
                    |_bytes| async { Ok(()) },
                ),
            )
            .await
            .expect("unrelated write must not park")
            .expect("unrelated write must succeed");
            assert!(!dir.join(format!("{WRITE_BARRIER}.held.json")).exists());
            assert!(!dir.join(format!("{WRITE_BARRIER}.receipt.jsonl")).exists());
            state.teardown();
        }

        #[tokio::test]
        async fn no_channel_installed_write_pipeline_is_unchanged() {
            deactivate();
            let root = tempfile::tempdir().unwrap();
            let untouched = root.path().join("would-be-barriers");
            std::fs::create_dir_all(&untouched).unwrap();

            let app = tauri::test::mock_builder()
                .manage(NativeTerminalSurfaceHostState::default())
                .build(tauri::test::mock_context(tauri::test::noop_assets()))
                .unwrap();
            let state = app.state::<NativeTerminalSurfaceHostState>();
            let (_output_tx, messages) = tokio::sync::mpsc::channel(1);
            let stream_task = tokio::spawn(std::future::pending());
            state
                .attach_daemon_attachment_with_bounds::<tauri::test::MockRuntime>(
                    "session-qa-no-channel",
                    crate::daemon::DaemonAttachment {
                        session_id: "session-qa-no-channel".into(),
                        epoch: 1,
                        start_sequence: Some(1),
                        end_sequence: Some(1),
                        gap: None,
                        history: bytes::Bytes::new(),
                        history_segments: Vec::new(),
                        pty_cols: Some(80),
                        pty_rows: Some(24),
                        remote_generation: None,
                        messages,
                        stream_task,
                    },
                    Some(app.handle().clone()),
                    None,
                )
                .unwrap();
            let input = NativeTerminalInput::Text { text: "qa probe".into() };
            send_native_terminal_input_with_stage_logging(
                &app.handle(),
                &state,
                "session-qa-no-channel",
                &input,
                Some(1),
                Some(OPERATION_ID.into()),
                |_bytes| async { Ok(()) },
            )
            .await
            .expect("write pipeline without the QA channel must behave as today");
            assert!(
                std::fs::read_dir(&untouched).unwrap().next().is_none(),
                "no QA control/receipt file may be written without the channel"
            );
            state.teardown();
        }
    }
}
