use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime, State};
use tokio::sync::oneshot;

use crate::daemon::DaemonClient;
use crate::ipc::IpcError;
use crate::native_terminal::composition::LogicalBounds;
use crate::native_terminal::surface_host::{
    NativeTerminalBoundsRequest, NativeTerminalSurfaceHostState, NativeTerminalSurfaceReceipt,
};
use crate::native_terminal::{
    MouseEvent, NativeTerminalError, NativeTerminalInput, ScrollViewport, TerminalEngine,
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

#[tauri::command]
pub async fn cmd_native_terminal_attach<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
) -> Result<(), IpcError> {
    let attachment = match daemon_client.attach(&session_id, None).await {
        Ok(attachment) => attachment,
        Err(err) => return Err(err),
    };
    if let Err(err) = state.attach_daemon_attachment(&session_id, attachment, Some(app)) {
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
pub async fn cmd_native_terminal_set_bounds<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
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
                .map_err(|error| IpcError::internal(error.to_string()));
            let _ = sender.send(result);
        })
        .map_err(|error| {
            IpcError::internal(format!(
                "Could not dispatch native terminal render: {error}"
            ))
        })?;
    let receipt = receiver.await.map_err(|_| {
        IpcError::internal("Main thread stopped before native terminal render completed")
    })??;

    let _ = daemon_client
        .resize_terminal(&session_id, receipt.cols, receipt.rows)
        .await;

    Ok(receipt)
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

/// Encode input only for a daemon-backed native session that is still attached.
///
/// `NativeTerminalSurfaceHostState::encode_input` supports standalone/local prototype
/// callers by lazily creating a VT state. The production IPC boundary must not use that
/// fallback: after a lifecycle detach it would allow PTY writes while the daemon output
/// pump and compositor owner are gone, making typed bytes look silently lost.
pub fn encode_attached_native_input(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    input: &NativeTerminalInput,
) -> Result<Vec<u8>, NativeTerminalError> {
    if state.snapshot_for_session(session_id)?.is_none() {
        return Err(NativeTerminalError::NoValue);
    }
    state.encode_input(session_id, input)
}

/// Encode clipboard paste text for a native session only if currently attached.
pub fn encode_attached_native_paste(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    text: &str,
) -> Result<Vec<u8>, NativeTerminalError> {
    state.with_session_terminal(session_id, |term| term.encode_paste(text))
}

/// Encode a mouse event for a native session only if currently attached.
pub fn encode_attached_native_mouse(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    event: &MouseEvent,
) -> Result<Vec<u8>, NativeTerminalError> {
    state.with_session_terminal(session_id, |term| term.encode_mouse(event))
}

/// Scroll visible viewport for an attached native session.
pub fn scroll_attached_native_terminal(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    behavior: ScrollViewport,
) -> Result<(), NativeTerminalError> {
    state.with_session_terminal(session_id, |term| term.scroll_viewport(behavior))
}

/// Perform grid selection on an attached native session.
pub fn select_attached_native_terminal(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    mode: &NativeTerminalSelectMode,
) -> Result<NativeTerminalSelectionReceipt, NativeTerminalError> {
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

#[tauri::command]
pub async fn cmd_native_terminal_mouse<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    state: State<'_, NativeTerminalSurfaceHostState>,
    session_id: String,
    event: MouseEvent,
) -> Result<NativeTerminalMouseReceipt, IpcError> {
    let bytes = match encode_attached_native_mouse(state.inner(), &session_id, &event) {
        Ok(bytes) => bytes,
        Err(err) => return Err(IpcError::internal(err.to_string())),
    };
    if !bytes.is_empty() {
        if let Err(err) = daemon_client.write_terminal(&session_id, bytes).await {
            return Err(err);
        }
    }
    let tracking =
        mouse_tracking_enabled_for_attached_session(state.inner(), &session_id).unwrap_or(false);

    let receipt = if let Some(window) = app.get_webview_window("main") {
        let state_inner = state.inner().clone();
        let surface_window = window.clone();
        let (sender, receiver) = oneshot::channel();
        let session_id_clone = session_id.clone();
        if window
            .run_on_main_thread(move || {
                let res = state_inner
                    .get_receipt(&surface_window, &session_id_clone)
                    .map(|r| into_ipc_receipt(session_id_clone, r))
                    .ok();
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
