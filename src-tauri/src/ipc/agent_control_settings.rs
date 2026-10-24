//! The Settings card for agent control: whether paired devices with a machine Control grant may
//! type into agent panes, which devices are excluded, and the newest audit lines. The gateway
//! state owns the setting and saves it to `agent-control.json`; in the desktop app that state
//! lives in the daemon.

use super::error::{IpcError, IpcErrorCode};
use super::remote::RemoteGatewayManager;
use crate::agent_control::policy::AgentControlSettings;
use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
use std::sync::Arc;
use tauri::State;

/// A daemon that knows these requests types its failures, so an untyped error is an older daemon.
fn settings_from(response: DaemonResponse) -> Result<AgentControlSettings, IpcError> {
    match response {
        DaemonResponse::AgentControlSettingsOk { settings } => Ok(settings),
        DaemonResponse::Error { message, code: Some(code), .. } => {
            Err(IpcError::new(IpcErrorCode::from_code_str(&code), message))
        }
        DaemonResponse::Error { .. } => Err(IpcError::new(
            IpcErrorCode::DaemonUpgradeRequired,
            "The running Ferryx daemon predates the agent control setting; it appears after the daemon restarts",
        )),
        _ => Err(IpcError::internal("Unexpected daemon agent control response")),
    }
}

#[tauri::command]
pub async fn cmd_agent_control_get(
    manager: State<'_, Arc<RemoteGatewayManager>>,
) -> Result<AgentControlSettings, IpcError> {
    if let Some(client) = manager.daemon_client() {
        return settings_from(client.send_request(DaemonRequest::AgentControlSettingsGet).await?);
    }
    let state = Arc::clone(manager.state().expect("a manager wraps a daemon or a state"));
    Ok(state.agent_control_settings())
}

#[tauri::command]
pub async fn cmd_agent_control_set(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    enabled: bool,
    excluded_device_ids: Vec<String>,
) -> Result<AgentControlSettings, IpcError> {
    if let Some(client) = manager.daemon_client() {
        return settings_from(
            client
                .send_request(DaemonRequest::AgentControlSettingsSet { enabled, excluded_device_ids })
                .await?,
        );
    }
    let state = Arc::clone(manager.state().expect("a manager wraps a daemon or a state"));
    super::run_blocking(move || {
        state.set_agent_control(enabled, excluded_device_ids).map_err(|error| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to save the agent control setting: {error}"),
            )
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;

    #[test]
    fn an_untyped_daemon_error_means_the_daemon_is_older() {
        let old = DaemonResponse::Error { message: "Malformed request".into(), code: None, details: None };
        assert_eq!(settings_from(old).unwrap_err().code, IpcErrorCode::DaemonUpgradeRequired);
        let typed = DaemonResponse::Error { message: "disk full".into(), code: Some("IO_ERROR".into()), details: None };
        assert_eq!(settings_from(typed).unwrap_err().code, IpcErrorCode::IoError);
    }

    #[tokio::test]
    async fn agent_control_commands_read_and_save_the_gateway_setting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = Arc::new(crate::remote::state::RemoteGatewayState::new_with_paths(
            Arc::new(crate::terminal::TerminalService::default()),
            crate::worktree::WorkspaceRegistry::new(),
            Some(dir.path().join("config.json")),
            Some(dir.path().join("auth.json")),
        ));
        let app = tauri::test::mock_builder()
            .manage(Arc::new(RemoteGatewayManager::new(Arc::clone(&state))))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        assert!(!cmd_agent_control_get(app.state()).await.expect("get").enabled);
        let saved = cmd_agent_control_set(app.state(), true, vec!["dev-1".into()]).await.expect("set");
        assert!(saved.enabled);
        assert_eq!(saved.excluded_device_ids, vec!["dev-1".to_string()]);
        assert!(state.agent_control_policy.permits("dev-2"));
        assert!(!state.agent_control_policy.permits("dev-1"));
        assert!(dir.path().join(crate::agent_control::policy::AGENT_CONTROL_FILE).exists());
    }
}
