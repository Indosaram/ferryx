use serde::{Deserialize, Serialize};

/// Every refusal the keeper can hand back. These are typed, never silent drops, and they travel
/// on the wire by name so a daemon built against an older keeper still recognizes a known code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum KeeperErrorCode {
    /// No session with that id exists. Authoritative discovery answer.
    UnknownSession,
    /// The caller's epoch is older than the newest attached one; it may no longer read or write.
    EpochMismatch,
    /// The replay log dropped frames past the requested offset. Carries the resume offset.
    ReplayGap,
    /// A checkpoint arrived whose sequence offset is not newer than the committed one.
    StaleCheckpoint,
    /// Installing the checkpoint would exceed the aggregate memory cap. The previous checkpoint
    /// is preserved.
    CheckpointBudgetExhausted,
    /// The replay sub-cap (640 MiB of the aggregate) refused a session reservation, so the keeper
    /// would not open the PTY. Distinct from [`Self::MemoryBudgetExhausted`], where the aggregate
    /// cap is what refused.
    ///
    /// This is *not* a replay-log overflow: a log that must drop retained frames publishes
    /// [`Self::ReplayGap`] to the reader instead of refusing anything.
    ReplayBudgetExhausted,
    /// Aggregate keeper memory is exhausted; the spawn/checkpoint was refused. A checkpoint that
    /// alone overflows the aggregate is reported as [`Self::CheckpointBudgetExhausted`] instead.
    MemoryBudgetExhausted,
    /// Frame larger than the 1 MiB cap.
    FrameTooLarge,
    /// More than 32 in-flight frames on one connection.
    TooManyInflightFrames,
    /// More than 8 concurrent connections.
    TooManyConnections,
    /// Malformed or unknown frame kind, bad magic, bad version.
    BadFrame,
    /// A chunked checkpoint upload referenced an unknown or already-committed upload id.
    UnknownCheckpointUpload,
    /// A chunk arrived out of order, or did not fill the declared total.
    CheckpointChunkMismatch,
    /// The peer's uid is not the keeper's uid.
    PeerUidMismatch,
    /// Spawn argument the keeper cannot honour (bad size, empty argv).
    InvalidSpawn,
    /// The keeper is shutting down.
    ShuttingDown,
}

impl KeeperErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnknownSession => "UNKNOWN_SESSION",
            Self::EpochMismatch => "EPOCH_MISMATCH",
            Self::ReplayGap => "REPLAY_GAP",
            Self::StaleCheckpoint => "STALE_CHECKPOINT",
            Self::CheckpointBudgetExhausted => "CHECKPOINT_BUDGET_EXHAUSTED",
            Self::ReplayBudgetExhausted => "REPLAY_BUDGET_EXHAUSTED",
            Self::MemoryBudgetExhausted => "MEMORY_BUDGET_EXHAUSTED",
            Self::FrameTooLarge => "FRAME_TOO_LARGE",
            Self::TooManyInflightFrames => "TOO_MANY_INFLIGHT_FRAMES",
            Self::TooManyConnections => "TOO_MANY_CONNECTIONS",
            Self::BadFrame => "BAD_FRAME",
            Self::UnknownCheckpointUpload => "UNKNOWN_CHECKPOINT_UPLOAD",
            Self::CheckpointChunkMismatch => "CHECKPOINT_CHUNK_MISMATCH",
            Self::PeerUidMismatch => "PEER_UID_MISMATCH",
            Self::InvalidSpawn => "INVALID_SPAWN",
            Self::ShuttingDown => "SHUTTING_DOWN",
        }
    }
}

/// The wire code, rendered as itself. `KeeperError` interpolates `{code}` in its `thiserror`
/// message, so the three-origin error text a daemon logs is `CODE: message` and a caller that
/// matches on the code never has to parse prose.
impl std::fmt::Display for KeeperErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct KeeperError {
    pub code: KeeperErrorCode,
    pub message: String,
    /// Present on `REPLAY_GAP`: the offset the consumer must resume at. Present on
    /// `CHECKPOINT_BUDGET_EXHAUSTED`/`STALE_CHECKPOINT`: the newest committed offset.
    pub offset: Option<u64>,
}

impl KeeperError {
    pub fn new(code: KeeperErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), offset: None }
    }

    pub fn at(code: KeeperErrorCode, message: impl Into<String>, offset: u64) -> Self {
        Self { code, message: message.into(), offset: Some(offset) }
    }

    pub fn unknown_session(id: &str) -> Self {
        Self::new(KeeperErrorCode::UnknownSession, format!("session {id} is not owned by this keeper"))
    }

    pub fn epoch_mismatch(id: &str, presented: u64, newest: u64) -> Self {
        Self::new(
            KeeperErrorCode::EpochMismatch,
            format!("session {id}: epoch {presented} is older than attached epoch {newest}"),
        )
    }
}

pub type KeeperResult<T> = Result<T, KeeperError>;
