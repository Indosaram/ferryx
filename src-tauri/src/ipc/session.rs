use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::session::{
    clear_session_from_path, load_session_from_path, load_session_reporting_corruption,
    save_session_to_path, session_path_for_window,
    PersistedWorkspaceSession, PRIMARY_SESSION_WINDOW_LABEL,
};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, Runtime, Window};

/// The UI session file: `FERRYX_SESSION_DIR`, else the `FERRYX_DATA_DIR` isolation root (one
/// file for dev and release, as the root is already per run), else the app data dir with its
/// release/dev split. `app_dir` is only called in the last case.
fn session_file_path<E>(
    session_override: Option<PathBuf>,
    data_env: Option<&OsStr>,
    is_dev: bool,
    app_dir: impl FnOnce() -> Result<PathBuf, E>,
) -> Result<PathBuf, E> {
    let env_dir = session_override.or_else(|| crate::browser::data_paths::isolated_root(data_env));
    if env_dir.is_some() {
        return Ok(crate::daemon::server::resolve_session_path(
            env_dir.as_deref(),
            is_dev,
            Path::new(""),
            Path::new(""),
        ));
    }
    let app_dir = app_dir()?;
    Ok(crate::daemon::server::resolve_session_path(
        None,
        is_dev,
        &app_dir.join("session_state.json"),
        &app_dir.join("dev").join("session_state.json"),
    ))
}

/// Today's single-window session file. The primary window keeps exactly this path.
pub(crate) fn get_session_file_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, IpcError> {
    session_file_path(
        crate::daemon::server::session_dir_override(),
        std::env::var_os("FERRYX_DATA_DIR").as_deref(),
        crate::daemon::server::is_dev_runtime(),
        || {
            app.path().app_data_dir().map_err(|e| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Failed to resolve app data dir: {}", e),
                )
            })
        },
    )
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
    // The entry point takes the gate itself; wrapping it again here would only add a second
    // (reentrant, but panicking) admission on the same key.
    run_blocking(move || save_session_to_path(&path, &session)).await
}

/// Emitted to the window whose session file could not be parsed, so the UI can say where the
/// quarantined copy went. The payload is the backup path as a string.
pub const SESSION_CORRUPTED_EVENT: &str = "session_corrupted";

#[tauri::command]
pub async fn cmd_session_load<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
) -> Result<Option<PersistedWorkspaceSession>, IpcError> {
    let path = get_window_session_file_path(&app, window.label())?;
    let (session, quarantined) =
        run_blocking(move || load_session_reporting_corruption(&path)).await?;
    if let Some(backup) = quarantined {
        // The layout could not be read and was preserved at `backup`. Tell the person where it
        // went instead of silently booting an empty window.
        let _ = app.emit_to(
            window.label(),
            SESSION_CORRUPTED_EVENT,
            backup.to_string_lossy().to_string(),
        );
    }
    Ok(session)
}

#[tauri::command]
pub async fn cmd_session_clear<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
) -> Result<(), IpcError> {
    let path = get_window_session_file_path(&app, window.label())?;
    run_blocking(move || clear_session_from_path(&path)).await
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
    // Both entry points take their own gate, so the pair cannot deadlock on a shared key and
    // neither runs unprotected.
    let loaded = load_session_from_path(source)?;
    let Some(session) = loaded else {
        return Ok(());
    };
    save_session_to_path(target, &session)
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
    run_blocking(move || clear_session_from_path(&path)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{PersistedLayout, PersistedTab, PersistedWorkspace};
    use std::collections::HashMap;
    use std::ffi::OsStr;
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// An app data dir that must not be consulted.
    fn no_app_dir() -> Result<PathBuf, ()> {
        Err(())
    }

    #[test]
    fn data_dir_alone_isolates_the_session_file_in_dev_and_release() {
        let data = Some(OsStr::new("/qa/data"));
        for is_dev in [false, true] {
            assert_eq!(
                session_file_path(None, data, is_dev, no_app_dir),
                Ok(PathBuf::from("/qa/data/session_state.json"))
            );
        }
    }

    #[test]
    fn session_dir_wins_over_data_dir() {
        assert_eq!(
            session_file_path(Some(PathBuf::from("/qa/session")), Some(OsStr::new("/qa/data")), true, no_app_dir),
            Ok(PathBuf::from("/qa/session/session_state.json"))
        );
    }

    #[test]
    fn without_overrides_the_app_data_dir_paths_are_unchanged() {
        let app = || Ok::<_, ()>(PathBuf::from("/app"));
        for data in [None, Some(OsStr::new(""))] {
            assert_eq!(
                session_file_path(None, data, false, app),
                Ok(PathBuf::from("/app/session_state.json"))
            );
            assert_eq!(
                session_file_path(None, data, true, app),
                Ok(PathBuf::from("/app/dev/session_state.json"))
            );
        }
    }

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
