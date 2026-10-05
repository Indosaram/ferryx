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

pub(crate) mod foreground;
pub mod manual_ssh;
pub(crate) mod metrics;
pub mod output_hub;
pub mod paired_daemon;
pub mod paired_runtime;
pub mod preferences;
pub mod pty;
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
pub(crate) mod qa_liveness;
pub mod remote;
#[cfg(test)]
mod remote_runtime_tests;
pub(crate) mod resume_cwd;
pub mod service;
pub mod session;
pub mod shell;
pub mod suspension;
#[cfg(windows)]
pub use suspension::windows::install_ownership_verifier;

pub use output_hub::*;
pub use preferences::*;
pub use pty::*;
pub use service::*;
pub use session::*;
pub use shell::*;
pub use suspension::{ActuationReceipt, StopGuarantee, SuspensionError, SuspensionSource, SuspensionTarget,
    classify_stop_source, resume_owned, stop_for_owned_suspension};

#[cfg(test)]
mod tests;
