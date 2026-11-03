//! Undo of one supervisor action-log entry (the panel's "Undo" on a log line).
//!
//! The panel and the gateway call [`ActionUndo`]; a real implementation arrives with the answer
//! history (an answer the agent can take back) and the rules store (a rule change). Until then
//! [`NoUndo`] answers every request with [`UndoError::NotSupported`], so the route and the
//! command exist and fail in a typed way instead of silently doing nothing.
use async_trait::async_trait;

/// Why an undo did not run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UndoError {
    /// No undo is implemented for this action yet.
    #[error("undoing this action is not supported yet")]
    NotSupported,
    /// The entry does not exist or has no undo handle left.
    #[error("action {0} has nothing to undo")]
    Nothing(i64),
    /// The screen moved on (a newer prompt or revision, or a card is already open): nothing
    /// changed and the undo handle stays.
    #[error("action {0} can no longer be undone: the screen moved on")]
    Stale(i64),
    /// The undo could not be stored; nothing was restored.
    #[error("undoing the action failed: {0}")]
    Failed(String),
}

impl UndoError {
    /// The stable wire code a refusal carries.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotSupported => "UNSUPPORTED",
            Self::Nothing(_) => "NOT_FOUND",
            Self::Stale(_) => "STALE",
            Self::Failed(_) => "FAILED",
        }
    }
}

/// Reverses what action-log entry `id` did, using its undo handle.
#[async_trait]
pub trait ActionUndo: Send + Sync {
    async fn undo(&self, id: i64) -> Result<(), UndoError>;
}

/// The stub: no action can be undone yet.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoUndo;

#[async_trait]
impl ActionUndo for NoUndo {
    async fn undo(&self, _id: i64) -> Result<(), UndoError> {
        Err(UndoError::NotSupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_stub_refuses_every_undo_as_not_supported() {
        let error = NoUndo.undo(7).await.unwrap_err();
        assert_eq!(error, UndoError::NotSupported);
        assert_eq!(error.code(), "UNSUPPORTED");
    }
}
