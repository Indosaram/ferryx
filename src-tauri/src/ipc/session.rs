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
pub(crate) fn get_window_session_file_path<R: Runtime>(
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

/// Copies one window's session file onto another's so a newly opened window boots with exactly
/// the source window's projects, workspace, tabs and split layout. Live PTY sessions are handed
/// over separately (`cmd_window_move_sessions`); nothing here spawns, closes or kills anything.
pub(crate) async fn clone_window_session<R: Runtime>(
    app: &AppHandle<R>,
    source_label: &str,
    target_label: &str,
) -> Result<(), IpcError> {
    let source = get_window_session_file_path(app, source_label)?;
    let target = get_window_session_file_path(app, target_label)?;
    run_blocking(move || clone_session_file(&source, &target)).await
}

fn clone_session_file(source: &Path, target: &Path) -> Result<(), IpcError> {
    let loaded = with_session_path_lock(source, || load_session_from_path(source))?;
    let Some(session) = loaded else {
        return Ok(());
    };
    with_session_path_lock(target, || save_session_to_path(target, &session))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{PersistedLayout, PersistedTab, PersistedWorkspace};
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn clone_session_file_copies_the_complete_workspace_layout() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("session_state.json");
        let target = session_path_for_window(&source, "ws-clone").unwrap();
        let mut workspace = PersistedWorkspace {
            workspace_id: "alpha".into(),
            repo_root: PathBuf::from("/repo"),
            ..Default::default()
        };
        workspace.layout = PersistedLayout {
            tabs: vec![PersistedTab {
                id: "tab-agent".into(),
                label: "Agent".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let original = PersistedWorkspaceSession {
            version: 3,
            timestamp: 42,
            active_workspace_id: "alpha".into(),
            workspaces: HashMap::from([("alpha".into(), workspace)]),
            extra: HashMap::new(),
        };
        save_session_to_path(&source, &original).unwrap();

        clone_session_file(&source, &target).unwrap();

        assert_eq!(load_session_from_path(&target).unwrap(), Some(original));
        assert!(source.exists());
    }

    #[test]
    fn clone_session_file_without_a_source_leaves_the_target_empty() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("session_state.json");
        let target = session_path_for_window(&source, "ws-clone").unwrap();

        clone_session_file(&source, &target).unwrap();

        assert!(!target.exists());
    }
}
