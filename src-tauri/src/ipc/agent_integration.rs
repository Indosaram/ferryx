//! Daemon autostart setting: whether the OS service manager owns the daemon's lifetime at login.
//! Turning it off only removes the login registration; it never stops the running daemon.

use super::error::IpcError;
use serde::{Deserialize, Serialize};

/// Whether the OS service manager owns the daemon's lifetime on this target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonSupervisorStatus {
    pub supported: bool,
    pub enabled: bool,
}

#[tauri::command]
pub async fn cmd_daemon_supervisor_get() -> Result<DaemonSupervisorStatus, IpcError> {
    super::run_blocking(|| {
        let supported = crate::daemon::supervisor::supported();
        Ok(DaemonSupervisorStatus {
            supported,
            enabled: supported
                && !crate::daemon::supervisor::user_opted_out()
                && crate::daemon::supervisor::installed(),
        })
    })
    .await
}

#[tauri::command]
pub async fn cmd_daemon_supervisor_set(enabled: bool) -> Result<DaemonSupervisorStatus, IpcError> {
    super::run_blocking(move || {
        // The opt-out is recorded first so every later ensure() (app startup, daemon client)
        // honors it; disabling removes autostart only and never stops the running daemon.
        // Enabling goes through owner admission: a live daemon is never started over or replaced.
        if enabled {
            crate::daemon::supervisor::set_user_opt_out(false)
                .and_then(|()| crate::daemon::supervisor::ensure_if_absent().map(|_| ()))
        } else {
            crate::daemon::supervisor::set_user_opt_out(true)
                .and_then(|()| crate::daemon::supervisor::remove_autostart())
        }
        .map_err(|e| IpcError::internal(format!("daemon supervisor: {e}")))?;
        let supported = crate::daemon::supervisor::supported();
        Ok(DaemonSupervisorStatus {
            supported,
            enabled: supported && enabled,
        })
    })
    .await
}
