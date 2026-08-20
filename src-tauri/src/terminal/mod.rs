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

pub mod pty;
pub mod session;

pub use pty::*;
pub use session::*;

#[cfg(test)]
mod tests;
