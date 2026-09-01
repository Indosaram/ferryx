pub mod agents;
pub mod browser;
pub mod browser_cli;
pub mod cli_install;
pub mod dag;
pub mod debug;
pub mod diagnostics;
pub mod error;
#[cfg(feature = "native-terminal")]
pub mod native_terminal;
#[cfg(not(feature = "native-terminal"))]
pub mod native_terminal_disabled;
pub mod notifications;
pub mod preferences;
pub mod project;
pub mod remote;
pub mod session;
pub mod ssh;
pub mod terminal;
pub mod worktree;

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
#[cfg(all(test, unix))]
mod tests;

pub use agents::*;
pub use browser::*;
pub use cli_install::*;
pub use dag::*;
pub use debug::*;
pub use diagnostics::*;
pub use error::*;
#[cfg(feature = "native-terminal")]
pub use native_terminal::*;
#[cfg(not(feature = "native-terminal"))]
pub use native_terminal_disabled::*;
pub use notifications::*;
pub use preferences::*;
pub use project::*;
pub use remote::*;
pub use session::*;
pub use terminal::*;
pub use worktree::*;
