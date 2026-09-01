use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::session::{
    clear_session_from_path, load_session_from_path, save_session_to_path,
    PersistedWorkspaceSession,
};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

fn get_session_file_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, IpcError> {
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

#[tauri::command]
pub async fn cmd_session_save<R: Runtime>(
    app: AppHandle<R>,
    session: PersistedWorkspaceSession,
) -> Result<(), IpcError> {
    let path = get_session_file_path(&app)?;
    run_blocking(move || save_session_to_path(&path, &session)).await
}

#[tauri::command]
pub async fn cmd_session_load<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<PersistedWorkspaceSession>, IpcError> {
    let path = get_session_file_path(&app)?;
    run_blocking(move || load_session_from_path(&path)).await
}

#[tauri::command]
pub async fn cmd_session_clear<R: Runtime>(app: AppHandle<R>) -> Result<(), IpcError> {
    let path = get_session_file_path(&app)?;
    run_blocking(move || clear_session_from_path(&path)).await
}
