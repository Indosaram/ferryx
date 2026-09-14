pub mod agent_extension;
pub(crate) mod agent_state;
pub mod client;
pub mod handover;
pub mod launchd;
pub(crate) mod logging;
pub mod manifest;
pub mod protocol;
pub mod proxy;
pub mod server;
pub mod session_service;
pub mod workspace_service;

/// Shared headless authority supplied only by the session-owning daemon.
pub struct MachineServices {
    pub sessions: std::sync::Arc<session_service::DaemonSessionService>,
    pub workspaces: std::sync::Arc<workspace_service::DaemonWorkspaceService>,
}

pub use client::*;
pub use handover::*;
pub use launchd::*;
pub use manifest::*;
pub use protocol::*;
pub use proxy::*;
pub use server::*;
