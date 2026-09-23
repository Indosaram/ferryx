pub mod account_protocol;
pub mod attach_identity;
pub mod auth;
pub mod backend;
pub mod browser_admission;
pub mod browser_backend;
pub mod browser_protocol;
pub mod browser_security;
pub mod browser_ws;
pub mod dag_api;
pub mod discovery;
pub(crate) mod filesystem;
pub mod machine_events;
pub mod machine_operation_journal;
pub mod machine_protocol;
pub mod mirror;
pub mod protocol;
pub mod push;
pub mod relay_client;
pub mod relay_server;
pub mod server;
pub(crate) mod session_api;
mod ssh;
pub mod state;
pub mod terminal_wire;
pub mod workspace_api;
pub mod workspace_catalog;

pub use auth::*;
pub use backend::*;
pub use browser_admission::{
    AdmissionController, AdmissionOutcome, DriverBroker, DriverLease, SubscriberQueue,
    MAX_CAPTURED_BROWSERS, MAX_GLOBAL_DRIVERS, MAX_VIEWERS_PER_BROWSER,
};
pub use browser_backend::{
    BoxFuture, BrowserCapabilities, BrowserCommandContext, BrowserCommandResult,
    BrowserRemoteState, DesktopScope, InProcessBrowserServiceBackend, InProcessTestBackend,
    LocalIpcBrowserBackend, RemoteBrowserBackend, RemoteBrowserError, RemoteBrowserSessionSummary,
    UnavailableBrowserBackend,
};
pub use browser_protocol::{
    BinaryFrameHeader, BrowserCaptureRect, BrowserFrameMetadata, BrowserImageFormat,
    BrowserSubscribeOptions, ClientMessage, DecodedBrowserFrame, ProtocolCodecError, ServerMessage,
};
pub use browser_security::{require_permission, sanitize_url, RequestDeduplicator, SecurityError};
pub use browser_ws::{BrowserWsSession, WsConnectionState};
pub use mirror::*;
pub use protocol::*;
pub use server::*;
pub use state::*;

#[cfg(all(test, unix))]
mod tests;

pub mod design_mode;

#[cfg(test)]
mod filesystem_tests;

#[cfg(test)]
mod workspace_api_tests;

#[cfg(all(test, unix))]
#[path = "dag_paired_tests.rs"]
mod dag_paired_tests;

#[cfg(test)]
mod browser_protocol_tests;

#[cfg(test)]
mod browser_security_tests;

#[cfg(test)]
mod browser_lifecycle_tests;
