//! Stopping a closing lane's session: the hook the settlement close calls after the close intent
//! is recorded and before the lane's worktree is removed, so no agent is left typing into a tree
//! that is about to go.
use std::sync::Arc;

use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::agent_control::{AgentControl, AgentOp};
use crate::error::Result;

/// Stops one lane session. Production wires agent control; tests inject a recorder.
#[async_trait]
pub trait SessionStopper: Send + Sync {
    async fn stop(&self, host_id: Option<&str>, session_id: &str, lane_key: &str) -> Result<()>;
}

/// [`SessionStopper`] over agent control's `stop` operation.
pub struct AgentControlStopper(pub Arc<dyn AgentControl>);

#[async_trait]
impl SessionStopper for AgentControlStopper {
    async fn stop(&self, host_id: Option<&str>, session_id: &str, lane_key: &str) -> Result<()> {
        self.0
            .execute(
                host_id,
                AgentOp::Stop {
                    session_id: session_id.to_owned(),
                    request_id: format!("settle-close:{lane_key}"),
                },
            )
            .await
            .map(drop)
    }
}

/// Stops the lane's session if it has one. A failed stop is logged, not fatal: the worktree is
/// still removed without force, so a session that kept typing leaves a dirty tree that stays.
pub(super) async fn stop_lane_session(pool: &SqlitePool, key: &str, stopper: &dyn SessionStopper) {
    let row: Result<Option<(Option<String>, Option<String>)>> =
        sqlx::query_as("SELECT omo_thread_id, host_id FROM lanes WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
            .map_err(Into::into);
    let (session, host) = match row {
        Ok(Some((Some(session), host))) if !session.is_empty() => (session, host),
        Ok(_) => return,
        Err(error) => {
            tracing::warn!(lane = key, %error, "the lane's session was not looked up for stopping");
            return;
        }
    };
    let host = host.filter(|host| !host.is_empty());
    if let Err(error) = stopper.stop(host.as_deref(), &session, key).await {
        tracing::warn!(lane = key, %error, "the lane's session was not stopped; its worktree is removed without force");
    }
}
