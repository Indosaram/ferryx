#[cfg(feature = "desktop")]
pub mod account;
pub mod agent_history;
#[cfg(feature = "desktop")]
pub mod agent_integration;
pub mod agents;
#[cfg(feature = "desktop")]
pub mod browser;
pub mod browser_cli;
pub mod cli_install;
#[cfg(feature = "desktop")]
pub mod dag;
#[cfg(feature = "desktop")]
pub mod daemon_update;
pub mod debug;
#[cfg(feature = "desktop")]
pub mod design_feedback;
#[cfg(feature = "desktop")]
pub mod diagnostics;
pub mod error;
pub mod file_link;
#[cfg(feature = "desktop")]
pub mod file_open_with;
#[cfg(feature = "desktop")]
pub mod file_preview;
#[cfg(feature = "desktop")]
pub mod file_preview_contract;
#[cfg(feature = "desktop")]
pub mod github_issue;
#[cfg(feature = "desktop")]
pub mod host;
#[cfg(feature = "desktop")]
pub mod native_menu;
#[cfg(feature = "native-terminal")]
pub mod native_terminal;
#[cfg(all(feature = "desktop", not(feature = "native-terminal")))]
pub mod native_terminal_disabled;
#[cfg(feature = "native-terminal")]
pub mod native_terminal_hyperlink;
#[cfg(feature = "desktop")]
pub mod notifications;
pub mod paired_host;
#[cfg(feature = "desktop")]
pub mod permissions;
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
pub mod qa_barrier;
#[cfg(feature = "desktop")]
pub mod preferences;
pub mod project;
#[cfg(feature = "desktop")]
pub mod project_remote;
pub mod remote;
#[cfg(feature = "desktop")]
pub mod remote_drop;
pub mod remote_file;
#[cfg(feature = "desktop")]
pub mod session;
pub mod ssh;
#[cfg(feature = "desktop")]
pub mod supervisor;
#[cfg(feature = "desktop")]
pub mod system_resources;
pub mod terminal;
#[cfg(test)]
pub mod pane_liveness_contract;
#[cfg(feature = "desktop")]
pub mod updater;
pub mod worktree;
#[cfg(feature = "desktop")]
pub mod worktree_disk;
#[cfg(test)]
mod worktree_disk_tests;

pub(crate) async fn run_blocking<T, F>(operation: F) -> Result<T, error::IpcError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, error::IpcError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| error::IpcError::internal(format!("blocking task failed: {error}")))?
}

#[cfg(test)]
mod blocking_contract_tests;
#[cfg(all(test, unix, feature = "desktop"))]
mod tests;

pub use agents::*;
#[cfg(feature = "desktop")]
pub use browser::*;
pub use cli_install::*;
#[cfg(feature = "desktop")]
pub use dag::*;
pub use debug::*;
#[cfg(feature = "desktop")]
pub use diagnostics::*;
pub use error::*;
#[cfg(feature = "desktop")]
pub use github_issue::*;
#[cfg(feature = "desktop")]
pub use host::*;
#[cfg(feature = "desktop")]
pub use native_menu::*;
#[cfg(feature = "native-terminal")]
pub use native_terminal::*;
#[cfg(all(feature = "desktop", not(feature = "native-terminal")))]
pub use native_terminal_disabled::*;
#[cfg(feature = "native-terminal")]
pub use native_terminal_hyperlink::*;
#[cfg(feature = "desktop")]
pub use notifications::*;
#[cfg(feature = "desktop")]
pub use permissions::*;
#[cfg(feature = "desktop")]
pub use preferences::*;
pub use project::*;
pub use remote::*;
#[cfg(feature = "desktop")]
pub use session::*;
pub use terminal::*;
pub use worktree::*;
#[cfg(feature = "desktop")]
pub use worktree_disk::*;
