//! Ledger of who drove each lane turn. A person's relayed words and raw typing are recorded
//! as `human_correction`, a confirmed supervisor send as `supervisor_proposal`. Raw keystrokes
//! are never stored as text, and the ledger is never summarized into a profile.
//!
//! `lane_turns` references `lanes(key)`, not `lane_goal`: the ledger is lane provenance and is
//! kept whether or not a goal contract exists, so a contract set later can tell a person's
//! earlier corrections from supervisor sends. Only the goal loop reads it, and the loop never
//! touches a lane without a contract (IS-9), so recording here adds no supervision.
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::types::{GoalError, TurnOrigin};
use crate::storage::now_rfc3339;

/// Longest text excerpt a turn row keeps.
pub const TURN_EXCERPT_MAX_CHARS: usize = 240;

/// Request-id prefix of every supervisor goal-loop send: `hub-goal-{lane}-{turn}`.
pub const SUPERVISOR_REQUEST_PREFIX: &str = "hub-goal-";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub lane_key: String,
    pub seq: i64,
    pub origin: TurnOrigin,
    pub text_excerpt: Option<String>,
    pub screen_revision: Option<String>,
    pub at: String,
}

/// The hub also relays a person's own words into panes over the owner socket: a Slack
/// approval click, a chat message, a reply in a lane thread.
pub fn relayed_from_a_person(request_id: Option<&str>) -> bool {
    let Some(id) = request_id else { return false };
    id.starts_with("slack-approval:")
        || id.starts_with("chat-")
        || (id.starts_with("lane-") && id.contains("-owner-"))
}

/// Who a send with this request id speaks for, when it drives a lane turn at all.
pub fn origin_for_request_id(request_id: Option<&str>) -> Option<TurnOrigin> {
    if relayed_from_a_person(request_id) {
        return Some(TurnOrigin::HumanCorrection);
    }
    request_id
        .filter(|id| id.starts_with(SUPERVISOR_REQUEST_PREFIX))
        .map(|_| TurnOrigin::SupervisorProposal)
}

/// One line, secrets redacted, at most [`TURN_EXCERPT_MAX_CHARS`] characters.
pub fn excerpt(text: &str) -> Option<String> {
    let scrubbed = crate::cron_remediate::redact_token(&crate::judge::scrub_for_external(text));
    let line = scrubbed.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return None;
    }
    if line.chars().count() <= TURN_EXCERPT_MAX_CHARS {
        return Some(line);
    }
    let mut short: String = line.chars().take(TURN_EXCERPT_MAX_CHARS - 1).collect();
    short.push('…');
    Some(short)
}

/// The live lane whose pane is `session_id`, if any.
pub async fn lane_of_session(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Option<String>, GoalError> {
    Ok(sqlx::query_scalar(
        "SELECT key FROM lanes WHERE omo_thread_id = ?
           AND status NOT IN ('closed', 'failed', 'verified_done')
         ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?)
}

/// Appends one turn. `text` is redacted and capped before it is stored; `None` stores no
/// text at all (raw keystrokes).
pub async fn record_turn(
    pool: &SqlitePool,
    lane_key: &str,
    origin: TurnOrigin,
    text: Option<&str>,
    screen_revision: Option<&str>,
) -> Result<Turn, GoalError> {
    record_turn_at(pool, lane_key, origin, text, screen_revision, &now_rfc3339()).await
}

pub(crate) async fn record_turn_at(
    pool: &SqlitePool,
    lane_key: &str,
    origin: TurnOrigin,
    text: Option<&str>,
    screen_revision: Option<&str>,
    at: &str,
) -> Result<Turn, GoalError> {
    let text_excerpt = text.and_then(excerpt);
    let seq: i64 = sqlx::query_scalar(
        "INSERT INTO lane_turns(lane_key, seq, origin, text_excerpt, screen_revision, at)
         SELECT ?, COALESCE(MAX(seq), 0) + 1, ?, ?, ?, ? FROM lane_turns WHERE lane_key = ?
         RETURNING seq",
    )
    .bind(lane_key)
    .bind(origin.as_str())
    .bind(text_excerpt.as_deref())
    .bind(screen_revision)
    .bind(at)
    .bind(lane_key)
    .fetch_one(pool)
    .await?;
    Ok(Turn {
        lane_key: lane_key.to_owned(),
        seq,
        origin,
        text_excerpt,
        screen_revision: screen_revision.map(str::to_owned),
        at: at.to_owned(),
    })
}

/// Records the turn an accepted send drove on a lane pane: a person's relayed words as
/// `human_correction`, a `hub-goal-*` supervisor send as `supervisor_proposal`. Any other
/// request id (auto-answers, lane drive, programs) records nothing. Returns the recorded
/// turn, or `None` when nothing drove a lane turn.
pub async fn record_send_turn(
    pool: &SqlitePool,
    session_id: &str,
    request_id: Option<&str>,
    text: &str,
) -> Result<Option<Turn>, GoalError> {
    let Some(origin) = origin_for_request_id(request_id) else {
        return Ok(None);
    };
    let Some(lane_key) = lane_of_session(pool, session_id).await? else {
        return Ok(None);
    };
    record_turn(pool, &lane_key, origin, Some(text), None)
        .await
        .map(Some)
}

/// Records one raw-typing turn on the lane whose pane is `session_id`, with no text.
/// The caller coalesces a typing burst into one call.
pub async fn record_keystroke_turn(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Option<Turn>, GoalError> {
    let Some(lane_key) = lane_of_session(pool, session_id).await? else {
        return Ok(None);
    };
    record_turn(pool, &lane_key, TurnOrigin::HumanCorrection, None, None)
        .await
        .map(Some)
}

/// Every turn of a lane, oldest first.
pub async fn turns_of(pool: &SqlitePool, lane_key: &str) -> Result<Vec<Turn>, GoalError> {
    type TurnRow = (i64, String, Option<String>, Option<String>, String);
    let rows: Vec<TurnRow> = sqlx::query_as(
        "SELECT seq, origin, text_excerpt, screen_revision, at FROM lane_turns
         WHERE lane_key = ? ORDER BY seq",
    )
    .bind(lane_key)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(
            |(seq, origin, text_excerpt, screen_revision, at)| -> Result<Turn, GoalError> {
            Ok(Turn {
                lane_key: lane_key.to_owned(),
                seq,
                origin: origin.parse::<TurnOrigin>()?,
                text_excerpt,
                screen_revision,
                at,
            })
            },
        )
        .collect()
}

fn parse_at(text: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|value| value.with_timezone(&chrono::Utc))
}

/// The newest human_correction turn recorded strictly after `at` (RFC 3339), if any.
pub async fn latest_human_after(
    pool: &SqlitePool,
    lane_key: &str,
    at: &str,
) -> Result<Option<Turn>, GoalError> {
    let Some(after) = parse_at(at) else {
        return Err(GoalError::InvalidContract(format!(
            "not an RFC 3339 time: {at}"
        )));
    };
    let row: Option<(i64, Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT seq, text_excerpt, screen_revision, at FROM lane_turns
         WHERE lane_key = ? AND origin = 'human_correction' ORDER BY seq DESC LIMIT 1",
    )
    .bind(lane_key)
    .fetch_optional(pool)
    .await?;
    // Turns are appended in time order, so the newest human turn is the only candidate.
    Ok(row
        .filter(|(_, _, _, turn_at)| parse_at(turn_at).is_some_and(|value| value > after))
        .map(|(seq, text_excerpt, screen_revision, at)| Turn {
            lane_key: lane_key.to_owned(),
            seq,
            origin: TurnOrigin::HumanCorrection,
            text_excerpt,
            screen_revision,
            at,
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanes::state::{insert_lane, set_pane};

    async fn lane_pool() -> SqlitePool {
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "ledger", "Ledger", "lead", "/tmp", "file marker")
            .await
            .unwrap();
        set_pane(&pool, "ledger", "pane-1").await.unwrap();
        pool
    }

    async fn turn_rows(pool: &SqlitePool) -> Vec<(i64, String, Option<String>)> {
        sqlx::query_as("SELECT seq, origin, text_excerpt FROM lane_turns ORDER BY seq")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[test]
    fn request_ids_map_to_turn_origins() {
        for id in [
            "slack-approval:local:pane-1:rev-42:prompt-9",
            "chat-lane:ledger-1700",
            "lane-ledger-owner-msg7",
        ] {
            assert_eq!(
                origin_for_request_id(Some(id)),
                Some(TurnOrigin::HumanCorrection),
                "{id}"
            );
        }
        assert_eq!(
            origin_for_request_id(Some("hub-goal-ledger-3")),
            Some(TurnOrigin::SupervisorProposal)
        );
        assert_eq!(origin_for_request_id(Some("hub-auto-prompt-3")), None);
        assert_eq!(origin_for_request_id(Some("lane-ledger-drive-2")), None);
        assert_eq!(origin_for_request_id(None), None);
    }

    #[tokio::test]
    async fn a_relayed_send_is_a_human_correction_with_a_capped_redacted_excerpt() {
        let pool = lane_pool().await;
        let text = format!(
            "use token sk-abcdefghijklmnop and run --owner-token secret123 then {}",
            "word ".repeat(100)
        );
        let turn = record_send_turn(&pool, "pane-1", Some("lane-ledger-owner-msg7"), &text)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(turn.origin, TurnOrigin::HumanCorrection);
        assert_eq!(turn.seq, 1);
        let stored = turn.text_excerpt.unwrap();
        assert!(stored.chars().count() <= TURN_EXCERPT_MAX_CHARS);
        assert!(!stored.contains("sk-abcdefghijklmnop"));
        assert!(!stored.contains("secret123"));
        assert!(stored.contains("[redacted]"));
        let rows = turn_rows(&pool).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "human_correction");
        assert_eq!(rows[0].2.as_deref(), Some(stored.as_str()));
    }

    #[tokio::test]
    async fn a_supervisor_send_is_a_supervisor_proposal_and_other_ids_record_nothing() {
        let pool = lane_pool().await;
        let sent = record_send_turn(&pool, "pane-1", Some("hub-goal-ledger-1"), "run the tests")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(sent.origin, TurnOrigin::SupervisorProposal);
        assert_eq!(sent.text_excerpt.as_deref(), Some("run the tests"));
        assert!(record_send_turn(&pool, "pane-1", Some("hub-auto-prompt-3"), "1")
            .await
            .unwrap()
            .is_none());
        assert!(record_send_turn(&pool, "pane-1", None, "1")
            .await
            .unwrap()
            .is_none());
        assert!(record_send_turn(&pool, "pane-9", Some("chat-x-1"), "hi")
            .await
            .unwrap()
            .is_none());
        let turns = turns_of(&pool, "ledger").await.unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].origin, TurnOrigin::SupervisorProposal);
    }

    #[tokio::test]
    async fn a_supervisor_turn_and_a_keystroke_turn_are_recorded_in_sequence() {
        let pool = lane_pool().await;
        let sent = record_turn(
            &pool,
            "ledger",
            TurnOrigin::SupervisorProposal,
            Some("run the tests"),
            Some("r3"),
        )
        .await
        .unwrap();
        assert_eq!(sent.seq, 1);
        let typed = record_keystroke_turn(&pool, "pane-1").await.unwrap().unwrap();
        assert_eq!(typed.seq, 2);
        assert_eq!(typed.text_excerpt, None);
        assert!(record_keystroke_turn(&pool, "not-a-lane").await.unwrap().is_none());
        let rows = turn_rows(&pool).await;
        assert_eq!(
            rows,
            vec![
                (1, "supervisor_proposal".to_owned(), Some("run the tests".to_owned())),
                (2, "human_correction".to_owned(), None),
            ]
        );
    }

    #[tokio::test]
    async fn the_latest_human_turn_after_a_time_is_found() {
        let pool = lane_pool().await;
        record_turn_at(&pool, "ledger", TurnOrigin::HumanCorrection, Some("first"), None, "2026-10-24T10:00:00+00:00")
            .await
            .unwrap();
        record_turn_at(&pool, "ledger", TurnOrigin::SupervisorProposal, Some("next"), None, "2026-10-24T10:05:00+00:00")
            .await
            .unwrap();
        assert!(latest_human_after(&pool, "ledger", "2026-10-24T10:01:00+00:00")
            .await
            .unwrap()
            .is_none());
        record_turn_at(&pool, "ledger", TurnOrigin::HumanCorrection, None, None, "2026-10-24T10:06:00Z")
            .await
            .unwrap();
        let found = latest_human_after(&pool, "ledger", "2026-10-24T10:01:00+00:00")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.seq, 3);
        assert_eq!(found.text_excerpt, None);
        assert!(latest_human_after(&pool, "ledger", "2026-10-24T10:06:00+00:00")
            .await
            .unwrap()
            .is_none());
        assert!(latest_human_after(&pool, "ledger", "yesterday").await.is_err());
    }

    #[tokio::test]
    async fn the_schema_refuses_an_overlong_excerpt_and_an_unknown_origin() {
        let pool = lane_pool().await;
        let long = "x".repeat(TURN_EXCERPT_MAX_CHARS + 1);
        assert!(sqlx::query("INSERT INTO lane_turns(lane_key, seq, origin, text_excerpt, at) VALUES('ledger', 1, 'other', ?, 'now')")
            .bind(long)
            .execute(&pool)
            .await
            .is_err());
        assert!(sqlx::query("INSERT INTO lane_turns(lane_key, seq, origin, at) VALUES('ledger', 1, 'robot', 'now')")
            .execute(&pool)
            .await
            .is_err());
    }
}
