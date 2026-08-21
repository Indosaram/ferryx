use crate::remote::auth::AuthManager;
use crate::terminal::TerminalService;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteNetworkMode {
    Off,
    LocalNetwork,
    Tailscale,
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

pub struct RemoteGatewayState {
    pub config: RwLock<RemoteGatewayConfig>,
    pub auth_manager: Arc<AuthManager>,
    pub terminal_service: Arc<TerminalService>,
    pub is_running: RwLock<bool>,
    pub bound_address: RwLock<Option<String>>,
}

impl RemoteGatewayState {
    pub fn new(terminal_service: Arc<TerminalService>) -> Self {
        Self {
            config: RwLock::new(RemoteGatewayConfig::default()),
            auth_manager: Arc::new(AuthManager::new()),
            terminal_service,
            is_running: RwLock::new(false),
            bound_address: RwLock::new(None),
        }
    }
}
