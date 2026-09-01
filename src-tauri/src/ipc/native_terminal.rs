use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime, State};
use tokio::sync::{mpsc, oneshot};

use crate::daemon::DaemonClient;
use crate::ipc::IpcError;
use crate::native_terminal::composition::{CellMetrics, LogicalBounds, SurfaceCompositionLayout};
use crate::native_terminal::surface_host::{
    NativeTerminalBoundsRequest, NativeTerminalSurfaceHostState, NativeTerminalSurfaceReceipt,
};
use crate::native_terminal::{
    MouseAction, MouseEvent, MousePosition, MouseRendererSize, NativeTerminalError,
    NativeTerminalInput, ScrollViewport, TerminalEngine,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalLogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalBoundsReceipt {
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
    pub rebuilt_rows: u16,
    pub reused_rows: u16,
    pub cursor_col: u16,
    pub cursor_row: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn read_native_pasteboard() -> (NativeTerminalClipboardContent, Vec<String>) {
    (NativeTerminalClipboardContent::Empty, Vec::new())
}

fn install_pty_resize_dispatcher(
    state: &NativeTerminalSurfaceHostState,
    daemon_client: Arc<DaemonClient>,
) {
    let (sender, mut receiver) = mpsc::unbounded_channel::<(String, u16, u16)>();
    let installed = state.set_pty_resize_sink_if_absent(Arc::new(move |session_id, cols, rows| {
        if let Err(error) = sender.send((session_id.to_string(), cols, rows)) {
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
    tauri::async_runtime::spawn(async move {
        while let Some((session_id, cols, rows)) = receiver.recv().await {
            if let Err(error) = daemon_client.resize_terminal(&session_id, cols, rows).await {
                tracing::warn!(
                    session_id,
                    cols,
                    rows,
                    %error,
                    "Failed to resize native terminal PTY"
                );
            }
        }
    });
}

#[tauri::command]
pub async fn cmd_native_terminal_attach<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    bounds: Option<NativeTerminalLogicalRect>,
    scale_factor: Option<f64>,
) -> Result<(), IpcError> {
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
    match state
        .reattach_existing_session_with_bounds(&session_id, logical_bounds)
        .map_err(|error| IpcError::internal(error.to_string()))?
    {
        true => return Ok(()),
        false => {}
    }

    let attachment = match daemon_client.attach(&session_id, None).await {
        Ok(attachment) => attachment,
        Err(err) => return Err(err),
    };
    if let Err(err) = state.attach_daemon_attachment_with_bounds(
        &session_id,
        attachment,
        Some(app),
        logical_bounds,
    ) {
        return Err(IpcError::internal(err.to_string()));
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_native_terminal_detach<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
) -> Result<(), IpcError> {
    state.detach_session(&session_id);
    Ok(())
}

#[tauri::command]
pub async fn cmd_native_terminal_close<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
) -> Result<(), IpcError> {
    state.close_session(&session_id);
    Ok(())
}

#[tauri::command]
pub async fn cmd_native_terminal_set_bounds<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    bounds: NativeTerminalLogicalRect,
    scale_factor: f64,
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
        .get_webview_window("main")
        .ok_or_else(|| IpcError::internal("Main Ferryx window is unavailable"))?;
    let state_inner = state.inner().clone();
    let surface_window = window.clone();
    let (sender, receiver) = oneshot::channel();
    let session_id_clone = session_id.clone();
    window
        .run_on_main_thread(move || {
            let result = state_inner
                .render(&surface_window, request)
                .map(|receipt| into_ipc_receipt(session_id_clone, receipt))
                .map_err(IpcError::from);
            let _ = sender.send(result);
        })
        .map_err(|error| {
            IpcError::internal(format!(
                "Could not dispatch native terminal render: {error}"
            ))
        })?;
    receiver.await.map_err(|_| {
        IpcError::internal("Main thread stopped before native terminal render completed")
    })?
}

#[tauri::command]
pub async fn cmd_native_terminal_set_focus<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    focused: bool,
) -> Result<NativeTerminalBoundsReceipt, IpcError> {
    let window = match app.get_webview_window("main") {
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
            .map(|receipt| into_ipc_receipt(session_id_clone.clone(), receipt))
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
    let window = match app.get_webview_window("main") {
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
            .map(|receipt| into_ipc_receipt(session_id_clone.clone(), receipt))
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
) -> Result<Vec<u8>, NativeTerminalError> {
    require_attached_surface(state, session_id)?;
    state.encode_input(session_id, input)
}

fn require_attached_surface(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
) -> Result<(), NativeTerminalError> {
    match state.ensure_surface_attached(session_id) {
        Ok(()) => Ok(()),
        Err(NativeTerminalError::SessionDetached(_)) => Err(NativeTerminalError::NoValue),
        Err(err) => Err(err),
    }
}

/// Encode clipboard paste text for a native session only if currently attached.
pub fn encode_attached_native_paste(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    text: &str,
) -> Result<Vec<u8>, NativeTerminalError> {
    require_attached_surface(state, session_id)?;
    state.with_session_terminal(session_id, |term| term.encode_paste(text))
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
    require_attached_surface(state, session_id)?;
    state.with_session_terminal(session_id, |term| term.scroll_viewport(behavior))
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

/// Query whether mouse tracking is enabled on an attached native session.
pub fn mouse_tracking_enabled_for_attached_session(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
) -> Result<bool, NativeTerminalError> {
    state.with_session_terminal(session_id, |term| term.mouse_tracking_enabled())
}

#[tauri::command]
pub async fn cmd_native_terminal_send_input<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    input: NativeTerminalInput,
) -> Result<NativeTerminalBoundsReceipt, IpcError> {
    let bytes = match encode_attached_native_input(state.inner(), &session_id, &input) {
        Ok(bytes) => bytes,
        Err(err) => return Err(IpcError::internal(err.to_string())),
    };
    if let Err(err) = daemon_client.write_terminal(&session_id, bytes).await {
        return Err(err);
    }

    let window = match app.get_webview_window("main") {
        Some(window) => window,
        None => return Err(IpcError::internal("Main Ferryx window is unavailable")),
    };
    let state_inner = state.inner().clone();
    let surface_window = window.clone();
    let (sender, receiver) = oneshot::channel();
    let session_id_clone = session_id.clone();
    if let Err(error) = window.run_on_main_thread(move || {
        let result = state_inner
            .get_receipt(&surface_window, &session_id_clone)
            .map(|receipt| into_ipc_receipt(session_id_clone.clone(), receipt))
            .map_err(|error| IpcError::internal(error.to_string()));
        let _ = sender.send(result);
    }) {
        return Err(IpcError::internal(format!(
            "Could not dispatch native terminal input receipt: {error}"
        )));
    }
    match receiver.await {
        Ok(receipt_result) => receipt_result,
        Err(_) => Err(IpcError::internal(
            "Main thread stopped before native terminal input completed",
        )),
    }
}

#[tauri::command]
pub async fn cmd_native_terminal_scroll<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    behavior: NativeTerminalScrollBehavior,
) -> Result<NativeTerminalBoundsReceipt, IpcError> {
    if let Err(err) =
        scroll_attached_native_terminal(state.inner(), &session_id, behavior.to_scroll_viewport())
    {
        return Err(IpcError::internal(err.to_string()));
    }

    let window = match app.get_webview_window("main") {
        Some(window) => window,
        None => return Err(IpcError::internal("Main Ferryx window is unavailable")),
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
                .map(|receipt| into_ipc_receipt(session_id_clone, receipt))
                .map_err(|error| IpcError::internal(error.to_string())),
            None => state_inner
                .get_receipt(&surface_window, &session_id_clone)
                .map(|receipt| into_ipc_receipt(session_id_clone, receipt))
                .map_err(|error| IpcError::internal(error.to_string())),
        };
        let _ = sender.send(result);
    }) {
        return Err(IpcError::internal(format!(
            "Could not dispatch native terminal scroll render: {error}"
        )));
    }

    match receiver.await {
        Ok(receipt_result) => receipt_result,
        Err(_) => Err(IpcError::internal(
            "Main thread stopped before native terminal scroll completed",
        )),
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
pub async fn cmd_native_terminal_select<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    mode: NativeTerminalSelectMode,
) -> Result<NativeTerminalSelectionReceipt, IpcError> {
    select_attached_native_terminal(state.inner(), &session_id, &mode)
        .map_err(|err| IpcError::internal(err.to_string()))
}

#[tauri::command]
pub async fn cmd_native_terminal_copy_selection<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
) -> Result<String, IpcError> {
    copy_attached_native_selection(state.inner(), &session_id)
        .map_err(|err| IpcError::internal(err.to_string()))
}

#[tauri::command]
pub async fn cmd_native_terminal_paste<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    text: String,
) -> Result<NativeTerminalBoundsReceipt, IpcError> {
    let bytes = match encode_attached_native_paste(state.inner(), &session_id, &text) {
        Ok(bytes) => bytes,
        Err(err) => return Err(IpcError::internal(err.to_string())),
    };
    if let Err(err) = daemon_client.write_terminal(&session_id, bytes).await {
        return Err(err);
    }

    let window = match app.get_webview_window("main") {
        Some(window) => window,
        None => return Err(IpcError::internal("Main Ferryx window is unavailable")),
    };
    let state_inner = state.inner().clone();
    let surface_window = window.clone();
    let (sender, receiver) = oneshot::channel();
    let session_id_clone = session_id.clone();
    if let Err(error) = window.run_on_main_thread(move || {
        let result = state_inner
            .get_receipt(&surface_window, &session_id_clone)
            .map(|receipt| into_ipc_receipt(session_id_clone.clone(), receipt))
            .map_err(|error| IpcError::internal(error.to_string()));
        let _ = sender.send(result);
    }) {
        return Err(IpcError::internal(format!(
            "Could not dispatch native terminal paste receipt: {error}"
        )));
    }
    match receiver.await {
        Ok(receipt_result) => receipt_result,
        Err(_) => Err(IpcError::internal(
            "Main thread stopped before native terminal paste completed",
        )),
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

#[tauri::command]
pub async fn cmd_native_terminal_mouse<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    event: MouseEvent,
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

    if tracking {
        let bytes = match encode_attached_native_mouse(state.inner(), &session_id, &event) {
            Ok(bytes) => bytes,
            Err(err) => return Err(IpcError::internal(err.to_string())),
        };
        if !bytes.is_empty() {
            if let Err(err) = daemon_client.write_terminal(&session_id, bytes).await {
                return Err(err);
            }
        }
    } else {
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
            tauri::async_runtime::spawn_blocking(move || {
                use std::fs::OpenOptions;
                use std::io::Write;
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
                if let Ok(mut file) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/ferryx-switch-debug.jsonl")
                {
                    let _ = writeln!(file, "{entry}");
                }
            });
        }
    }

    let receipt = if let Some(window) = app.get_webview_window("main") {
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
                            .map(|receipt| into_ipc_receipt(session_id_clone, receipt))
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
pub async fn cmd_native_terminal_clipboard_content<R: Runtime>(
    app: AppHandle<R>,
) -> Result<NativeTerminalClipboardContent, IpcError> {
    #[cfg(target_os = "macos")]
    {
        let window = match app.get_webview_window("main") {
            Some(window) => window,
            None => return Err(IpcError::internal("Main Ferryx window is unavailable")),
        };
        let (sender, receiver) = oneshot::channel();
        if let Err(error) = window.run_on_main_thread(move || {
            let (content, types) = read_native_pasteboard();
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
                if let Ok(mut file) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/ferryx-switch-debug.jsonl")
                {
                    let _ = writeln!(file, "{entry}");
                }
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
            let log_path = std::env::temp_dir().join("ferryx-switch-debug.jsonl");
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
                let _ = writeln!(file, "{entry}");
            }
        }
        Ok(content)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
        Ok(NativeTerminalClipboardContent::Empty)
    }
}

fn into_ipc_receipt(
    session_id: String,
    receipt: NativeTerminalSurfaceReceipt,
) -> NativeTerminalBoundsReceipt {
    NativeTerminalBoundsReceipt {
        session_id,
        cols: receipt.cols,
        rows: receipt.rows,
        rebuilt_rows: receipt.rebuilt_rows,
        reused_rows: receipt.reused_rows,
        cursor_col: receipt.cursor_col,
        cursor_row: receipt.cursor_row,
        cell_width_px: receipt.cell_width_px,
        cell_height_px: receipt.cell_height_px,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_terminal::{MouseButton, MousePosition, MouseRendererSize};

    fn test_mouse_event(size: Option<MouseRendererSize>) -> MouseEvent {
        MouseEvent {
            action: MouseAction::Press,
            button: Some(MouseButton::Left),
            position: MousePosition { x: 10.5, y: 20.25 },
            modifiers: Default::default(),
            size,
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
}
