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

    #[error("PTY input failed after {bytes_written} bytes: {message}")]
    InputError { bytes_written: usize, message: String },

    #[error(transparent)]
    RemoteInput(#[from] remote::RemoteFailure),

    #[error("PTY resize error: {0}")]
    ResizeError(String),

    #[error("PTY kill error: {0}")]
    KillError(String),

    /// The daemon-wide scrollback budget admits no further session. `limit` is
    /// `floor(budget_bytes / min_session_bytes)`: a session beyond it is refused rather than
    /// admitted with a silent zero-history allotment.
    #[error(
        "SESSION_CAPACITY_EXHAUSTED: this daemon admits at most {limit} concurrent sessions \
         (scrollback budget {budget_bytes} bytes / {min_session_bytes} bytes minimum history)"
    )]
    SessionCapacityExhausted {
        limit: usize,
        budget_bytes: usize,
        min_session_bytes: usize,
    },

    /// The durable admission log cannot be trusted, so no create is admitted.
    ///
    /// This is a refusal, not a transient failure: a damaged log hides an unknown number of
    /// reservations, so admitting would mean guessing the count.
    #[error("ADMISSION_STORE_UNUSABLE: {0}")]
    AdmissionStoreUnusable(String),

    #[error("Channel error: {0}")]
    ChannelError(String),

    #[error("General error: {0}")]
    Other(String),
}

impl PtyError {
    /// A refused admission is either the budget's typed capacity error or an unusable store.
    ///
    /// The second arm is not retryable and is not "unknown": the daemon cannot know its own
    /// reservation count, so the create is refused rather than admitted against a guess.
    pub fn from_admission_refusal(
        refusal: admission_store::AdmissionRefusal,
    ) -> Self {
        match refusal {
            admission_store::AdmissionRefusal::Capacity(error) => error,
            admission_store::AdmissionRefusal::StoreUnusable(error) => {
                Self::AdmissionStoreUnusable(error.to_string())
            }
        }
    }
}

pub(crate) mod foreground;
pub mod admission_store;
pub mod manual_ssh;
pub(crate) mod metrics;
pub mod output_hub;
pub mod paired_daemon;
pub mod paired_runtime;
pub mod preferences;
pub mod program_status;
pub mod program_status_ingest;
pub mod protocol_dto;
pub mod pty;
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
pub(crate) mod qa_liveness;
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
pub use qa_liveness::run_exact_pid_probe;
pub mod remote;
#[cfg(test)]
mod remote_runtime_tests;
pub(crate) mod continue_startup;
pub(crate) mod resume_cwd;
pub mod screen_mirror;
pub mod scrollback_budget;
pub mod service;
pub mod session;
pub mod shell;
pub mod suspension;
#[cfg(windows)]
pub use suspension::windows::install_ownership_verifier;

pub use output_hub::*;
pub use preferences::*;
pub use protocol_dto::*;
pub use pty::*;
pub use scrollback_budget::*;
pub use service::*;
pub use session::*;
pub use shell::*;
pub use suspension::{ActuationReceipt, StopGuarantee, SuspensionError, SuspensionSource, SuspensionTarget,
    classify_stop_source, resume_owned, stop_for_owned_suspension};

/// The retained-scrollback ceiling a freshly created native terminal starts with, before the
/// output hub narrows it to its session's share. `create_native_terminal` sets the byte option to
/// this instead of leaving it unset, so even a parser built outside a session's accounting is
/// bounded rather than unbounded.
pub const NATIVE_SCROLLBACK_CEILING_BYTES: usize = DEFAULT_SCROLLBACK_BUDGET_BYTES;

/// The hard lower bound for any per-session byte ceiling: strictly above libghostty-vt's ~10 KiB
/// default, because `lifecycle.rs` replaces that default precisely to stop its premature pruning.
/// A ceiling at or below it would silently restore the pruning that change removed.
pub fn native_scrollback_floor_bytes() -> usize {
    LIBGHOSTTY_DEFAULT_SCROLLBACK_BYTES + 1
}

#[cfg(test)]
mod scrollback_budget_tests;
#[cfg(test)]
mod tests;
