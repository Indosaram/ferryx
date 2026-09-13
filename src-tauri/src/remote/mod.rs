pub mod auth;
pub mod backend;
pub mod discovery;
pub(crate) mod filesystem;
pub mod machine_protocol;
pub mod machine_events;
pub mod workspace_catalog;
pub mod workspace_api;
pub mod machine_operation_journal;
pub mod mirror;
pub mod protocol;
pub mod push;
pub mod relay_client;
pub mod relay_server;
pub mod server;
pub mod terminal_wire;
pub(crate) mod session_api;
mod ssh;
pub mod state;

pub use auth::*;
pub use backend::*;
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
