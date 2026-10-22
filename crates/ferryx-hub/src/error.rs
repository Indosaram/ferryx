use crate::lanes::state::LaneTransitionError;

/// Recoverable hub failure. Callers log it; a hub error never takes down the daemon.
#[derive(Debug, thiserror::Error)]
pub enum HubError {
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error(transparent)]
    Lane(#[from] LaneTransitionError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, HubError>;

impl HubError {
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}
