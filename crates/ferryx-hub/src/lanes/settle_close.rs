//! The close half of the settlement gate: the durable close intent, the non-forced worktree
//! removal, the scheduled window wake, and the notices the lead and the lane receive.
//!
//! The intent is what makes a close crash-safe. It is recorded before anything is stopped or
//! removed, and once recorded it is finished rather than re-decided: a restart resumes the same
//! close instead of re-running the live checks to a different outcome.
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::{SqliteConnection, SqlitePool};

use super::settle_stop::{stop_lane_session, SessionStopper};
use super::state::{transition, LaneStatus, GOAL_CONVERGED_REASON};
use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

/// Lane event kinds of a recorded claim and of an objection (automatic or the lead's).
pub const CLAIM_EVENT: &str = "settlement_claim";
pub const OBJECTION_EVENT: &str = "settlement_objection";

/// Event-id prefix of the scheduled wake that re-opens a lane at its objection deadline.
pub const SETTLE_WAKE_PREFIX: &str = "settle:";

/// The scheduled wake's event id for one claim: `settle:<lane key>:<claim time>`.
pub fn settle_wake_event(key: &str, claim_at: &str) -> String {
    format!("{SETTLE_WAKE_PREFIX}{key}:{claim_at}")
}

/// The lane a settlement wake names, or `None` for any other deadline.
pub fn settle_wake_lane(event_id: &str) -> Option<&str> {
    event_id
        .strip_prefix(SETTLE_WAKE_PREFIX)
        .and_then(|rest| rest.split(':').next())
        .filter(|lane| !lane.is_empty())
}

/// What a recorded intent is bound to: the claim and the contract revision it was decided for.
/// An intent from an earlier cycle matches neither and is never resumed or short-circuited.
#[derive(Debug, Deserialize)]
struct IntentBinding {
    #[serde(default)]
    claim_at: Option<String>,
    #[serde(default)]
    contract_revision: Option<i64>,
}

/// The recorded close intent of a lane, if its close was decided for the lane's current claim
/// and contract revision.
pub async fn close_intent(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row: Option<(Option<String>, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT close_intent_json, claim_at,
                (SELECT revision FROM lane_goal WHERE lane_key = lanes.key)
           FROM lanes WHERE key = ?",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    let Some((Some(intent), claim_at, revision)) = row else {
        return Ok(None);
    };
    let current = serde_json::from_str::<IntentBinding>(&intent)
        .is_ok_and(|bound| bound.claim_at == claim_at && bound.contract_revision == revision);
    Ok(current.then_some(intent))
}

/// The close intent as the lane row stores it, bound to the row's current claim and contract
/// revision. A goal path writes this in the same transaction that completes its goal, so a
/// skipped completion leaves no intent behind.
pub async fn close_intent_json_for(
    conn: &mut SqliteConnection,
    key: &str,
    evidence: &str,
    close_at: DateTime<Utc>,
) -> sqlx::Result<String> {
    let claim_at: Option<String> = sqlx::query_scalar("SELECT claim_at FROM lanes WHERE key = ?")
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?
        .flatten();
    let revision: Option<i64> =
        sqlx::query_scalar("SELECT revision FROM lane_goal WHERE lane_key = ?")
            .bind(key)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(serde_json::json!({
        "at": now_rfc3339(),
        "evidence": evidence,
        "close_at": close_at.to_rfc3339(),
        "claim_at": claim_at,
        "contract_revision": revision,
    })
    .to_string())
}

/// Persists the close intent before anything is stopped or removed, so a crash resumes it.
pub async fn record_close_intent(
    pool: &SqlitePool,
    key: &str,
    evidence: &str,
    close_at: DateTime<Utc>,
) -> Result<()> {
    let mut conn = pool.acquire().await?;
    let intent = close_intent_json_for(&mut conn, key, evidence, close_at).await?;
    sqlx::query("UPDATE lanes SET close_intent_json = ?, claim_objection_until = NULL WHERE key = ?")
        .bind(intent)
        .bind(key)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// One close to finish: the lane, the transition reason, whether a crash interrupted it, and
/// the hook that stops the lane's session.
#[derive(Clone, Copy)]
pub struct CloseRequest<'a> {
    pub key: &'a str,
    pub reason: &'a str,
    pub resumed: bool,
    pub stopper: Option<&'a dyn SessionStopper>,
}

/// Marks the lane settled, stops its session and closes its worktree without force, from a
/// recorded intent. The intent is final, so any live lane that holds one is finished: a goal
/// path commits it with the goal's completion, before the lane's own status moves.
pub async fn finish_close(pool: &SqlitePool, request: CloseRequest<'_>) -> Result<()> {
    let CloseRequest {
        key,
        reason,
        resumed,
        stopper,
    } = request;
    match super::state::status_of(pool, key).await? {
        Some(LaneStatus::DoneClaimed) => {
            transition(pool, key, LaneStatus::VerifiedDone, Some(reason)).await?;
        }
        Some(LaneStatus::Working | LaneStatus::Blocked | LaneStatus::NotDone) => {
            transition(
                pool,
                key,
                LaneStatus::VerifiedDone,
                Some(GOAL_CONVERGED_REASON),
            )
            .await?;
        }
        Some(LaneStatus::VerifiedDone) => {}
        other => {
            return Err(HubError::message(format!(
                "close intent recorded but lane {key} is {other:?}, not a live lane"
            )));
        }
    }
    if resumed {
        tracing::info!(lane = key, "a recorded close intent was resumed");
    }
    if let Some(stopper) = stopper {
        stop_lane_session(pool, key, stopper).await;
    }
    // No --force: a tree that became dirty after the checks stays in place and is named.
    match super::worktree::remove_settled(pool, key).await {
        Ok(kept) if !kept.is_empty() => {
            tracing::warn!(lane = key, ?kept, "worktrees with unsent work were kept")
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(lane = key, %error, "the lane's worktree was not removed; the close intent stands")
        }
    }
    Ok(())
}

/// Wakes the goal's scheduler at `close_at` instead of polling the lane row. A lane without a
/// supervisor goal has no wake service; its durable deadline is read by its own driver.
pub async fn schedule_window(
    pool: &SqlitePool,
    key: &str,
    close_at: DateTime<Utc>,
) -> Result<Option<String>> {
    let goal: Option<String> = sqlx::query_scalar(
        "SELECT goal_id FROM supervisor_goals WHERE lane_key = ? AND run_state NOT IN ('completed','cancelled')",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    let Some(goal) = goal else {
        return Ok(None);
    };
    let claim_at: Option<String> = sqlx::query_scalar("SELECT claim_at FROM lanes WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?
        .flatten();
    let event = settle_wake_event(key, claim_at.as_deref().unwrap_or("claim"));
    match crate::supervisor::wakes::schedule_deadline(pool, &goal, &event, close_at).await {
        Ok(_) => Ok(Some(event)),
        Err(error) => {
            tracing::warn!(lane = key, %error, "the settlement wake was not scheduled; the deadline stays durable");
            Ok(None)
        }
    }
}

/// Records a notice for the lead and the lane's own transcript, once per `source_event_id`, and
/// once per body when it has none: the lead reads an objection once.
pub async fn announce(
    pool: &SqlitePool,
    key: &str,
    kind: &str,
    body: &str,
    source_event_id: Option<&str>,
) -> Result<()> {
    let lead: String = sqlx::query_scalar("SELECT lead_session_key FROM lanes WHERE key = ?")
        .bind(key)
        .fetch_one(pool)
        .await?;
    let body: String = body.chars().take(2000).collect();
    if source_event_id.is_none() {
        let already: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM lane_events WHERE lane_key = ? AND kind = ? AND body = ?)",
        )
        .bind(key)
        .bind(kind)
        .bind(&body)
        .fetch_one(pool)
        .await?;
        if already {
            return Ok(());
        }
    }
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at, source_event_id)
         VALUES(?,?,?,?,?,?)
         ON CONFLICT(lane_key, source_event_id) WHERE source_event_id IS NOT NULL DO NOTHING
         RETURNING id",
    )
    .bind(key)
    .bind(&lead)
    .bind(kind)
    .bind(&body)
    .bind(now_rfc3339())
    .bind(source_event_id)
    .fetch_optional(pool)
    .await?;
    if inserted.is_none() && source_event_id.is_some() {
        return Ok(());
    }
    sqlx::query("INSERT INTO lane_transcript(lane_key, at, role, text) VALUES(?,?,?,?)")
        .bind(key)
        .bind(now_rfc3339())
        .bind(kind)
        .bind(&body)
        .execute(pool)
        .await?;
    Ok(())
}
