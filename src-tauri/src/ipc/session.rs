use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::session::{
    clear_session_from_path, load_session_from_path, save_session_to_path,
    session_path_for_window, with_session_path_lock, PersistedWorkspaceSession,
    PRIMARY_SESSION_WINDOW_LABEL,
};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime, Window};

/// Today's single-window session file. The primary window keeps exactly this path.
pub(crate) fn get_session_file_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, IpcError> {
    let override_dir = crate::daemon::server::session_dir_override();
    if override_dir.is_some() {
        return Ok(crate::daemon::server::resolve_session_path(
            override_dir.as_deref(),
            crate::daemon::server::is_dev_runtime(),
            Path::new(""),
            Path::new(""),
        ));
    }

    let app_dir = app.path().app_data_dir().map_err(|e| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to resolve app data dir: {}", e),
        )
    })?;
    Ok(crate::daemon::server::resolve_session_path(
        None,
        crate::daemon::server::is_dev_runtime(),
        &app_dir.join("session_state.json"),
        &app_dir.join("dev").join("session_state.json"),
    ))
}

/// The session file owned by `label`: "main" keeps today's path, "ws-<id>" maps to
/// `session-ws-<id>.json` in the same directory. Any other label is rejected.
fn get_window_session_file_path<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
) -> Result<PathBuf, IpcError> {
    let primary = get_session_file_path(app)?;
    session_path_for_window(&primary, label)
}

#[tauri::command]
pub async fn cmd_session_save<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    session: PersistedWorkspaceSession,
) -> Result<(), IpcError> {
    let path = get_window_session_file_path(&app, window.label())?;
    run_blocking(move || with_session_path_lock(&path, || save_session_to_path(&path, &session)))
        .await
}

#[tauri::command]
pub async fn cmd_session_load<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
) -> Result<Option<PersistedWorkspaceSession>, IpcError> {
    let path = get_window_session_file_path(&app, window.label())?;
    run_blocking(move || with_session_path_lock(&path, || load_session_from_path(&path))).await
}

#[tauri::command]
pub async fn cmd_session_clear<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
) -> Result<(), IpcError> {
    let path = get_window_session_file_path(&app, window.label())?;
    run_blocking(move || with_session_path_lock(&path, || clear_session_from_path(&path))).await
}

/// Deletes a closed secondary window's session file after its tabs were adopted by another
/// window. The primary window's file is never discarded through this path.
#[tauri::command]
pub async fn cmd_session_discard_window<R: Runtime>(
    app: AppHandle<R>,
    label: String,
) -> Result<(), IpcError> {
    if label == PRIMARY_SESSION_WINDOW_LABEL {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "The primary window session cannot be discarded",
        ));
    }
    let path = get_window_session_file_path(&app, &label)?;
    run_blocking(move || with_session_path_lock(&path, || clear_session_from_path(&path))).await
}
