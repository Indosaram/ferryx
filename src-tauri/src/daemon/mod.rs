pub mod agent_extension;
pub(crate) mod agent_state;
pub mod client;
pub mod dag_service;
pub mod handover;
#[cfg(unix)]
pub mod handover_socket;
#[cfg(unix)]
pub mod handover_transaction;
#[cfg(unix)]
pub mod handover_wire;
pub mod launchd;
pub(crate) mod logging;
pub mod manifest;
pub mod protocol;
pub mod proxy;
pub mod server;
pub mod session_lifecycle;
pub mod session_service;
pub mod workspace_service;

/// Shared headless authority supplied only by the session-owning daemon.
pub struct MachineServices {
    pub sessions: std::sync::Arc<session_service::DaemonSessionService>,
    pub workspaces: std::sync::Arc<workspace_service::DaemonWorkspaceService>,
}

pub use client::*;
pub use handover::*;
#[cfg(unix)]
pub use handover_socket::*;
#[cfg(unix)]
pub use handover_transaction::*;
#[cfg(unix)]
pub use handover_wire::*;
pub use launchd::*;
pub use manifest::*;
pub use protocol::*;
pub use proxy::*;
pub use server::*;
pub use session_lifecycle::*;

