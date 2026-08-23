use crate::remote::auth::{write_private_json, AuthManager};
use crate::remote::protocol::{RemoteActiveDesktopSelection, RemoteEventMessage};
use crate::terminal::TerminalService;
use crate::worktree::WorkspaceRegistry;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, watch};

pub type DesktopEventSink = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteNetworkMode {
    Off,
    LocalNetwork,
    Tailscale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RemoteRestartPolicy {
    SessionOnly,
    #[default]
    RestoreListener,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGatewayConfig {
    pub mode: RemoteNetworkMode,
    pub port: u16,
    pub allow_control: bool,
}

impl Default for RemoteGatewayConfig {
    fn default() -> Self {
        Self {
            mode: RemoteNetworkMode::Off,
            port: 43821,
            allow_control: true,
        }
    }
}

impl RemoteGatewayConfig {
    pub fn restart_policy(&self) -> RemoteRestartPolicy {
        RemoteRestartPolicy::RestoreListener
    }

    fn persisted_snapshot(&self) -> PersistedRemoteGatewayConfig {
        PersistedRemoteGatewayConfig {
            mode: self.mode,
            port: self.port,
            allow_control: self.allow_control,
            restart_policy: self.restart_policy(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedRemoteGatewayConfig {
    mode: RemoteNetworkMode,
    port: u16,
    allow_control: bool,
    #[serde(default)]
    restart_policy: RemoteRestartPolicy,
}

pub struct RemoteGatewayState {
    pub config: RwLock<RemoteGatewayConfig>,
    pub auth_manager: Arc<AuthManager>,
    pub terminal_service: Arc<TerminalService>,
    pub workspace_registry: WorkspaceRegistry,
    pub active_selection: RwLock<Option<RemoteActiveDesktopSelection>>,
    pub active_session_tx: watch::Sender<Option<String>>,
    pub event_tx: broadcast::Sender<String>,
    pub is_running: RwLock<bool>,
    pub bound_address: RwLock<Option<String>>,
    config_path: Option<PathBuf>,
    pub desktop_event_sink: RwLock<Option<DesktopEventSink>>,
}

impl RemoteGatewayState {
    pub fn new(
        terminal_service: Arc<TerminalService>,
        workspace_registry: WorkspaceRegistry,
    ) -> Self {
        Self::new_with_paths(terminal_service, workspace_registry, None, None)
    }

    pub fn new_persistent(
        terminal_service: Arc<TerminalService>,
        workspace_registry: WorkspaceRegistry,
    ) -> Self {
        let base = remote_data_dir();
        Self::new_with_paths(
            terminal_service,
            workspace_registry,
            Some(base.join("remote-config.json")),
            Some(base.join("remote-auth.json")),
        )
    }

    pub fn new_with_paths(
        terminal_service: Arc<TerminalService>,
        workspace_registry: WorkspaceRegistry,
        config_path: Option<PathBuf>,
        auth_path: Option<PathBuf>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        let (active_session_tx, _) = watch::channel(None);
        let config = config_path
            .as_deref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<PersistedRemoteGatewayConfig>(&bytes).ok())
            .map(|persisted| RemoteGatewayConfig {
                mode: persisted.mode,
                port: persisted.port,
                allow_control: persisted.allow_control,
            })
            .unwrap_or_default();
        Self {
            config: RwLock::new(config),
            auth_manager: Arc::new(AuthManager::with_persistence(auth_path)),
            terminal_service,
            workspace_registry,
            active_selection: RwLock::new(None),
            active_session_tx,
            event_tx,
            is_running: RwLock::new(false),
            bound_address: RwLock::new(None),
            config_path,
            desktop_event_sink: RwLock::new(None),
        }
    }

    pub fn set_active_selection(&self, selection: RemoteActiveDesktopSelection) {
        let session_id = selection.session_id.clone();
        *self.active_selection.write() = Some(selection);
        let _ = self.active_session_tx.send(session_id);
    }

    pub fn clear_active_selection(&self) {
        *self.active_selection.write() = None;
        let _ = self.active_session_tx.send(None);
    }

    pub fn set_active_selection_opt(&self, selection: Option<RemoteActiveDesktopSelection>) {
        match selection {
            Some(sel) => self.set_active_selection(sel),
            None => self.clear_active_selection(),
        }
    }

    pub fn active_session_watch_rx(&self) -> watch::Receiver<Option<String>> {
        self.active_session_tx.subscribe()
    }

    pub fn active_selection(&self) -> Option<RemoteActiveDesktopSelection> {
        self.active_selection.read().clone()
    }

    pub fn set_desktop_event_sink(&self, sink: DesktopEventSink) {
        *self.desktop_event_sink.write() = Some(sink);
    }

    pub fn emit_desktop_event(&self, event: &str, payload: serde_json::Value) {
        if let Some(sink) = self.desktop_event_sink.read().as_ref() {
            sink(event, payload.clone());
        }
        let _ = self.event_tx.send(
            serde_json::to_string(&RemoteEventMessage {
                event: event.to_string(),
                payload,
            })
            .unwrap_or_default(),
        );
    }

    pub fn emit_event(&self, event_json: String) {
        let _ = self.event_tx.send(event_json);
    }

    pub fn persist_config(&self) -> std::io::Result<()> {
        let Some(path) = self.config_path.as_deref() else {
            return Ok(());
        };
        write_private_json(path, &self.config.read().persisted_snapshot())
    }
}

fn remote_data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("FERRYX_DATA_DIR") {
        return PathBuf::from(path).join("remote");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".ferryx").join("remote");
    }
    std::env::temp_dir().join("ferryx").join("remote")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::auth::DevicePermission;
    use crate::terminal::{PtyManager, TerminalOutputHub};

    fn test_state(
        config_path: PathBuf,
        auth_path: PathBuf,
    ) -> (RemoteGatewayState, WorkspaceRegistry, Arc<TerminalService>) {
        let pty = Arc::new(PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal = Arc::new(TerminalService::new(pty, hub));
        let registry = WorkspaceRegistry::new();
        let state = RemoteGatewayState::new_with_paths(
            Arc::clone(&terminal),
            registry.clone(),
            Some(config_path),
            Some(auth_path),
        );
        (state, registry, terminal)
    }

    #[test]
    fn enabled_gateway_persists_config_and_restores_on_reopen() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let config_path = dir.path().join("config.json");
        let auth_path = dir.path().join("auth.json");
        let (state, registry, terminal) = test_state(config_path.clone(), auth_path.clone());

        {
            let mut config = state.config.write();
            config.mode = RemoteNetworkMode::LocalNetwork;
            config.port = 45678;
            config.allow_control = false;
        }
        *state.is_running.write() = true;
        *state.bound_address.write() = Some("0.0.0.0:45678".into());
        let code = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
        let (token, device) = state
            .auth_manager
            .exchange_pairing_code(&code, "Phone")
            .expect("pair");
        state.persist_config().expect("persist");

        let on_disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&config_path).expect("read persisted config"))
                .expect("parse persisted config");
        assert_eq!(on_disk["mode"], "localNetwork");
        assert_eq!(on_disk["port"], 45678);
        assert_eq!(on_disk["allowControl"], false);
        assert_eq!(on_disk["restartPolicy"], "restoreListener");

        let reopened = RemoteGatewayState::new_with_paths(
            terminal,
            registry,
            Some(config_path),
            Some(auth_path),
        );
        let config = reopened.config.read().clone();
        assert_eq!(config.mode, RemoteNetworkMode::LocalNetwork);
        assert_eq!(config.port, 45678);
        assert!(!config.allow_control);
        assert_eq!(
            config.restart_policy(),
            RemoteRestartPolicy::RestoreListener
        );
        assert!(!*reopened.is_running.read());
        assert!(reopened.bound_address.read().is_none());

        let devices = reopened.auth_manager.list_devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, device.id);
        assert!(reopened.auth_manager.validate_token(&token).is_ok());
    }

    #[test]
    fn daemon_default_state_is_off_and_uses_supplied_terminal_service() {
        let pty = Arc::new(PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal = Arc::new(TerminalService::new(pty, hub));
        let state = RemoteGatewayState::new(Arc::clone(&terminal), WorkspaceRegistry::new());

        assert_eq!(state.config.read().mode, RemoteNetworkMode::Off);
        assert!(!*state.is_running.read());
        assert!(Arc::ptr_eq(&state.terminal_service, &terminal));
    }

    #[test]
    fn persisted_enabled_config_file_loads_enabled_mode_with_unbound_initial_state() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let config_path = dir.path().join("config.json");
        let auth_path = dir.path().join("auth.json");
        std::fs::write(
            &config_path,
            r#"{"mode":"localNetwork","port":41234,"allowControl":true}"#,
        )
        .expect("write enabled config");

        let (state, _, _) = test_state(config_path, auth_path);
        let config = state.config.read().clone();
        assert_eq!(config.mode, RemoteNetworkMode::LocalNetwork);
        assert_eq!(config.port, 41234);
        assert!(config.allow_control);
        assert_eq!(
            config.restart_policy(),
            RemoteRestartPolicy::RestoreListener
        );
        assert!(!*state.is_running.read());
        assert!(state.bound_address.read().is_none());
    }
}
