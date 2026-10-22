//! Adopt an existing pane and drive it until it prints `[CLAIM_DONE]`.
//! History lines win when a ferryx transcript parser supplied them. Otherwise the
//! new screen text since the last revision is what gets scanned.
use sqlx::SqlitePool;

use super::watch::compact;
use crate::agent_control::{AgentControl, AgentOp};
use crate::error::{HubError, Result};
use crate::lanes::runner::claim_from_text;
use crate::lanes::state::{self, set_pane, transition, LaneStatus};
use crate::storage::now_rfc3339;

#[derive(Debug, Clone, Default)]
pub struct PaneSnapshot {
    pub revision: String,
    pub screen: String,
    /// Assistant lines from a ferryx `history_*` parser, when the bridge had a transcript.
    pub history: Vec<String>,
}

/// Lines that were not in the previous snapshot.
pub fn screen_diff(previous: &str, current: &str) -> String {
    if previous.is_empty() {
        return String::new();
    }
    if let Some(suffix) = current.strip_prefix(previous) {
        return suffix.to_owned();
    }
    current.to_owned()
}

pub struct AdoptRequest<'a> {
    pub key: &'a str,
    pub goal: &'a str,
    pub lead: &'a str,
    pub cwd: &'a str,
    pub session_id: &'a str,
    pub done_signals: &'a str,
    pub baseline: &'a PaneSnapshot,
}

pub async fn adopt(pool: &SqlitePool, request: AdoptRequest<'_>) -> Result<()> {
    let AdoptRequest {
        key,
        goal,
        lead,
        cwd,
        session_id,
        done_signals,
        baseline,
    } = request;
    if goal.trim().is_empty() || lead.trim().is_empty() {
        return Err(HubError::config("goal and lead must not be empty"));
    }
    if crate::chat::session::SessionKey::from_storage_key(lead.trim()).is_err() {
        return Err(HubError::config("lead must be a session storage key"));
    }
    state::insert_lane(pool, key, goal, lead, cwd, done_signals).await?;
    transition(pool, key, LaneStatus::Working, Some("adopted")).await?;
    set_pane(pool, key, session_id).await?;
    sqlx::query(
        "INSERT INTO external_sessions(lane_key, goal, send_key, cursor_revision, cursor_text) VALUES(?,?,?,?,?)",
    )
    .bind(key)
    .bind(goal)
    .bind(format!("ext-{key}"))
    .bind(&baseline.revision)
    .bind(&baseline.screen)
    .execute(pool)
    .await?;
    for (index, line) in baseline.history.iter().enumerate() {
        sqlx::query("INSERT OR IGNORE INTO external_session_items(lane_key, item_id) VALUES(?,?)")
            .bind(key)
            .bind(format!("h{index}:{}", compact(line, 80)))
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn drive<C: AgentControl + ?Sized>(
    pool: &SqlitePool,
    control: &C,
    key: &str,
) -> Result<Option<String>> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT lanes.omo_thread_id, external_sessions.cursor_revision, external_sessions.cursor_text
         FROM lanes JOIN external_sessions ON external_sessions.lane_key = lanes.key WHERE lanes.key = ?",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    let Some((session_id, previous_revision, previous_text)) = row else {
        return Err(HubError::message(format!(
            "external lane {key} is not adopted"
        )));
    };
    let value = control
        .execute(
            None,
            AgentOp::Read {
                session_id: session_id.clone(),
            },
        )
        .await?;
    let snapshot = snapshot_from_read(&value);
    let fresh = if snapshot.history.is_empty() {
        if snapshot.revision == previous_revision {
            String::new()
        } else {
            screen_diff(&previous_text, &snapshot.screen)
        }
    } else {
        snapshot.history.join("\n")
    };
    sqlx::query(
        "UPDATE external_sessions SET cursor_revision = ?, cursor_text = ? WHERE lane_key = ?",
    )
    .bind(&snapshot.revision)
    .bind(&snapshot.screen)
    .bind(key)
    .execute(pool)
    .await?;
    let _ = now_rfc3339();
    Ok(claim_from_text(key, &fresh))
}

pub fn snapshot_from_read(value: &serde_json::Value) -> PaneSnapshot {
    PaneSnapshot {
        revision: value
            .pointer("/session/screen/revision")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_owned(),
        screen: value
            .pointer("/session/screen/text")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_owned(),
        history: value
            .pointer("/session/history")
            .and_then(|item| item.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;
    use crate::chat::session::SessionKey;
    use serde_json::json;

    #[test]
    fn an_unchanged_revision_adds_no_screen_text_and_a_suffix_is_the_diff() {
        assert_eq!(screen_diff("hello", "hello\nnext"), "\nnext");
        assert_eq!(screen_diff("", "hello"), "");
    }

    #[tokio::test]
    async fn adopt_ignores_history_and_drive_reads_a_new_claim() {
        let pool = crate::storage::memory().await.unwrap();
        let lead =
            SessionKey::new("slack", None::<String>, "C1", None::<String>, "U1").storage_key();
        let baseline = PaneSnapshot {
            revision: "1".into(),
            screen: "old [CLAIM_DONE] ext-1 | historical".into(),
            history: vec!["[CLAIM_DONE] ext-1 | historical".into()],
        };
        adopt(
            &pool,
            AdoptRequest {
                key: "ext-1",
                goal: "goal",
                lead: &lead,
                cwd: "/tmp",
                session_id: "pane",
                done_signals: "file marker",
                baseline: &baseline,
            },
        )
        .await
        .unwrap();
        let control = RecordingControl::default();
        *control.result.lock().unwrap() = json!({
            "session": {
                "screen": {"revision": "2", "text": "old [CLAIM_DONE] ext-1 | historical\n[CLAIM_DONE] ext-1 | fresh"},
                "history": []
            }
        });
        let claim = drive(&pool, &control, "ext-1").await.unwrap();
        assert_eq!(claim.as_deref(), Some("fresh"));
        let ops = control.ops();
        assert!(matches!(ops[0].1, AgentOp::Read { .. }));
    }
}
