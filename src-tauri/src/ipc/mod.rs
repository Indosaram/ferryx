pub mod error;
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
#[cfg(test)]
mod tests;

pub use error::*;
pub use terminal::*;
pub use worktree::*;
