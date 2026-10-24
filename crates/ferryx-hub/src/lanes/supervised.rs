//! What the desktop shows as supervised: every live lane bound to a pane, and every pane on the
//! watch list. Read-only, except for the pause a person sets from the desktop.
use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::Result;

/// Statuses that are history, not supervision.
const ENDED: &str = "('closed', 'failed', 'verified_done')";

/// One lane the supervisor drives, as the desktop's Supervisor view lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisedLane {
    pub key: String,
    pub title: String,
    pub status: String,
    /// The pane the lane runs in, once launched or adopted.
    pub session_id: Option<String>,
    pub cwd: String,
    /// The adopted session's goal; launched lanes carry their title only.
    pub goal: Option<String>,
    pub paused: bool,
    pub last_activity_at: String,
    /// The reason on the lane's newest history row (a transition, a pause or a resume).
    pub last_reason: Option<String>,
}

type LaneRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    bool,
    String,
    Option<String>,
);

/// Lanes that are still live, most recently active first.
pub async fn supervised_lanes(pool: &SqlitePool) -> Result<Vec<SupervisedLane>> {
    let rows: Vec<LaneRow> = sqlx::query_as(&format!(
        "SELECT lanes.key, lanes.title, lanes.status, lanes.omo_thread_id, lanes.cwd, external_sessions.goal,
                EXISTS(SELECT 1 FROM lane_pauses WHERE lane_pauses.lane_key = lanes.key AND lane_pauses.resumed_at IS NULL),
                COALESCE(lanes.last_activity_at, lanes.updated_at),
                (SELECT lane_history.reason FROM lane_history WHERE lane_history.lane_key = lanes.key
                 ORDER BY lane_history.id DESC LIMIT 1)
         FROM lanes LEFT JOIN external_sessions ON external_sessions.lane_key = lanes.key
         WHERE lanes.status NOT IN {ENDED}
         ORDER BY 8 DESC, lanes.key"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(key, title, status, session_id, cwd, goal, paused, last_activity_at, last_reason)| SupervisedLane {
                key,
                title,
                status,
                session_id: session_id.filter(|id| !id.is_empty()),
                cwd,
                goal,
                paused,
                last_activity_at,
                last_reason,
            },
        )
        .collect())
}

/// Local panes on the watch list (targets of the form `local:<session>`).
pub async fn watched_local_sessions(pool: &SqlitePool) -> Result<Vec<String>> {
    let targets: Vec<String> =
        sqlx::query_scalar("SELECT target FROM session_watches ORDER BY added_at")
            .fetch_all(pool)
            .await?;
    Ok(targets
        .into_iter()
        .filter_map(|target| target.strip_prefix("local:").map(str::to_owned))
        .collect())
}

/// True when `session_id` is the pane of a live lane a person paused.
pub async fn session_paused(pool: &SqlitePool, session_id: &str) -> Result<bool> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT EXISTS(SELECT 1 FROM lanes JOIN lane_pauses ON lane_pauses.lane_key = lanes.key
         WHERE lanes.omo_thread_id = ? AND lane_pauses.resumed_at IS NULL
           AND lanes.status NOT IN {ENDED})"
    ))
    .bind(session_id)
    .fetch_one(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanes::pause::pause;
    use crate::lanes::state::{insert_lane, set_pane, transition, LaneStatus};

    #[tokio::test]
    async fn live_lanes_carry_their_pane_goal_and_pause() {
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "ledger", "Ledger", "lead", "/repo", "file marker")
            .await
            .unwrap();
        transition(&pool, "ledger", LaneStatus::Working, Some("launched"))
            .await
            .unwrap();
        set_pane(&pool, "ledger", "pane-1").await.unwrap();
        insert_lane(&pool, "old", "Old", "lead", "/repo", "file marker")
            .await
            .unwrap();
        transition(&pool, "old", LaneStatus::Working, Some("launched"))
            .await
            .unwrap();
        transition(&pool, "old", LaneStatus::Failed, Some("gave up"))
            .await
            .unwrap();
        sqlx::query("INSERT INTO external_sessions(lane_key, goal, send_key, cursor_revision, cursor_text) VALUES('ledger','Get tests green','ext-ledger','r','')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO session_watches(target, added_at) VALUES('local:pane-2','2026-10-23T00:00:00Z'),('ssh:box:pane-3','2026-10-23T00:00:01Z')")
            .execute(&pool)
            .await
            .unwrap();

        let lanes = supervised_lanes(&pool).await.unwrap();
        assert_eq!(lanes.len(), 1, "a failed lane is not supervised: {lanes:?}");
        assert_eq!(lanes[0].session_id.as_deref(), Some("pane-1"));
        assert_eq!(lanes[0].goal.as_deref(), Some("Get tests green"));
        assert!(!lanes[0].paused);
        assert_eq!(lanes[0].last_reason.as_deref(), Some("launched"));
        assert!(!session_paused(&pool, "pane-1").await.unwrap());

        assert!(pause(&pool, "ledger", "desktop").await.unwrap());
        let paused = &supervised_lanes(&pool).await.unwrap()[0];
        assert!(paused.paused);
        assert_eq!(paused.last_reason.as_deref(), Some("paused"));
        assert!(session_paused(&pool, "pane-1").await.unwrap());
        assert!(!session_paused(&pool, "pane-2").await.unwrap());

        assert_eq!(
            watched_local_sessions(&pool).await.unwrap(),
            vec!["pane-2".to_string()]
        );
    }

    #[tokio::test]
    async fn a_lane_without_history_has_no_last_reason() {
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "fresh", "Fresh", "lead", "/repo", "file marker")
            .await
            .unwrap();

        let lanes = supervised_lanes(&pool).await.unwrap();

        assert_eq!(lanes[0].last_reason, None);
    }
}
