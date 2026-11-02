use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::session::{
    clear_session_from_path, load_session_from_path, save_session_to_path,
    PersistedWorkspaceSession,
};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

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

fn get_session_file_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, IpcError> {
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

#[cfg(test)]
mod tests {
    use super::session_file_path;
    use std::ffi::OsStr;
    use std::path::PathBuf;

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
}
