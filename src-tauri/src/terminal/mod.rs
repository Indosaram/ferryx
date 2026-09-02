use thiserror::Error;

#[derive(Error, Debug)]
pub enum PtyError {
    #[error("PTY session '{0}' not found")]
    SessionNotFound(String),

    #[error("Failed to create PTY: {0}")]
    PtyCreationError(String),

    #[error("Failed to spawn process: {0}")]
    SpawnError(String),

    #[error("PTY I/O error: {0}")]
    IoError(String),

    #[error("PTY resize error: {0}")]
    ResizeError(String),

    #[error("PTY kill error: {0}")]
    KillError(String),

    #[error("Channel error: {0}")]
    ChannelError(String),

    #[error("General error: {0}")]
    Other(String),
}

pub(crate) mod metrics;
pub mod history_store;
pub mod output_hub;
pub mod preferences;
pub mod pty;
pub mod service;
pub mod session;
pub mod shell;

pub use output_hub::*;
pub use preferences::*;
pub use pty::*;
pub use service::*;
pub use session::*;
pub use shell::*;

#[cfg(test)]
mod tests;
