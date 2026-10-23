//! omo-gateway `tracker/lanes.rs` `sync`: every lane is mirrored as an issue in the Agents
//! (AGT) team, its state follows the lane status, and the owner is notified when a lane
//! blocks, claims done, verifies, or fails. Run on every tracker request, like omo's `prepare`.
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::error::Result;

fn now_utc() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn model_short_name(model: &str) -> String {
    model.rsplit('/').next().unwrap_or(model).to_string()
}

pub fn deterministic_avatar_color(seed: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    let hash = hasher.finalize();
    let r = 40 + (hash[0] % 180);
    let g = 40 + (hash[1] % 180);
    let b = 40 + (hash[2] % 180);
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub async fn ensure_agent_user(pool: &SqlitePool, model: Option<&str>) -> Result<String> {
    let full_model = model.unwrap_or("agent");
    let short_name = model_short_name(full_model);

    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM tracker_users WHERE kind = 'agent' AND (model = ? OR (model IS NULL AND name = ?)) LIMIT 1",
    )
    .bind(full_model)
    .bind(&short_name)
    .fetch_optional(pool)
    .await?;

    if let Some(id) = existing {
        return Ok(id);
    }

    let id = Uuid::new_v4().to_string();
    let color = deterministic_avatar_color(&short_name);
    let now = now_utc();

    sqlx::query(
        "INSERT INTO tracker_users (id, name, display_name, email, avatar_color, kind, model, is_me, created_at)
         VALUES (?, ?, ?, NULL, ?, 'agent', ?, 0, ?)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(&id)
    .bind(&short_name)
    .bind(&short_name)
    .bind(&color)
    .bind(full_model)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(id)
}

pub fn map_lane_status_to_type(status: &str) -> (&'static str, &'static str) {
    match status {
        "queued" => ("unstarted", "Queued"),
        "working" => ("started", "Working"),
        "blocked" => ("started", "Blocked"),
        "done_claimed" => ("started", "In Review"),
        "not_done" => ("started", "Changes Requested"),
        "verified_done" => ("completed", "Verified"),
        "closed" => ("completed", "Done"),
        "failed" => ("canceled", "Failed"),
        _ => ("started", "Working"),
    }
}

pub async fn read_brief_content(brief_path: &str) -> String {
    if brief_path.is_empty() {
        return String::new();
    }
    match tokio::fs::read(brief_path).await {
        Ok(bytes) => {
            let limit = bytes.len().min(20 * 1024);
            String::from_utf8_lossy(&bytes[..limit]).into_owned()
        }
        Err(_) => String::new(),
    }
}

static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn sync_inner(pool: &SqlitePool) -> Result<()> {
    let lanes = sqlx::query(
        "SELECT key, title, cwd, brief_path, NULL AS model, status FROM lanes ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;

    let agt_team: Option<(String, i64)> =
        sqlx::query_as("SELECT id, issue_seq FROM tracker_teams WHERE key = 'AGT'")
            .fetch_optional(pool)
            .await?;

    let Some((agt_team_id, mut agt_seq)) = agt_team else {
        return Ok(());
    };

    let agt_states = sqlx::query(
        "SELECT id, type, lane_status FROM tracker_states WHERE team_id = ? AND lane_status IS NOT NULL",
    )
    .bind(&agt_team_id)
    .fetch_all(pool)
    .await?;

    let mut state_by_lane_status = std::collections::HashMap::<String, (String, String)>::new();
    for row in agt_states {
        let id: String = row.get("id");
        let state_type: String = row.get("type");
        let lane_status: String = row.get("lane_status");
        state_by_lane_status.insert(lane_status, (id, state_type));
    }

    let paused_state: Option<(String, String)> = sqlx::query_as(
        "SELECT id, type FROM tracker_states WHERE team_id = ? AND lane_paused = 1 LIMIT 1",
    )
    .bind(&agt_team_id)
    .fetch_optional(pool)
    .await?;

    let paused_lane_keys = crate::lanes::pause::paused_keys(pool).await?;

    let me_user_id: Option<String> =
        sqlx::query_scalar("SELECT id FROM tracker_users WHERE is_me = 1 LIMIT 1")
            .fetch_optional(pool)
            .await?;

    for lane_row in lanes {
        let lane_key: String = lane_row.get("key");
        let lane_title: String = lane_row.get("title");
        let brief_path: String = lane_row.get("brief_path");
        let lane_model: Option<String> = lane_row.get("model");
        let lane_status: String = lane_row.get("status");

        let is_paused = paused_lane_keys.contains(&lane_key)
            && matches!(
                lane_status.as_str(),
                "queued" | "working" | "blocked" | "done_claimed" | "not_done"
            );
        let effective_status = if is_paused {
            "paused"
        } else {
            lane_status.as_str()
        };

        let existing_issue = sqlx::query(
            "SELECT id, team_id, state_id, started_at
             FROM tracker_issues WHERE lane_key = ?",
        )
        .bind(&lane_key)
        .fetch_optional(pool)
        .await?;

        let now = now_utc();

        if let Some(issue) = existing_issue {
            let issue_id: String = issue.get("id");
            let issue_team_id: String = issue.get("team_id");
            let prev_state_id: String = issue.get("state_id");
            let mut started_at: Option<String> = issue.get("started_at");
            let completed_at: Option<String>;
            let canceled_at: Option<String>;

            // A human move the lane accepted stays visible while its close/reopen request is
            // pending; otherwise the issue always converges back to the lane's real status.
            let last_lane_status: Option<String> = sqlx::query_scalar(
                "SELECT to_value FROM tracker_activity WHERE issue_id = ? AND kind = 'lane_status' ORDER BY id DESC LIMIT 1",
            )
            .bind(&issue_id)
            .fetch_optional(pool)
            .await?;
            let status_changed = last_lane_status.as_deref() != Some(effective_status);
            // Resuming restores the pre-pause status; the owner was already told about it,
            // but a status reached while paused (e.g. a settled claim) is new.
            let pre_pause_status: Option<String> = if last_lane_status.as_deref() == Some("paused")
            {
                sqlx::query_scalar(
                    "SELECT to_value FROM tracker_activity WHERE issue_id = ? AND kind = 'lane_status' AND to_value <> 'paused' ORDER BY id DESC LIMIT 1",
                )
                .bind(&issue_id)
                .fetch_optional(pool)
                .await?
            } else {
                None
            };
            // omo skipped issues with a pending close intent or owner reply here; ferryx's
            // lane-action applies a transition at once, so there is never one pending.

            let target_state_id = if is_paused && issue_team_id == agt_team_id {
                paused_state.as_ref().map(|(id, _)| id.clone())
            } else if issue_team_id == agt_team_id {
                state_by_lane_status
                    .get(&lane_status)
                    .map(|(id, _)| id.clone())
            } else {
                let (mapped_type, _) = map_lane_status_to_type(effective_status);
                sqlx::query_scalar::<_, String>(
                    "SELECT id FROM tracker_states WHERE team_id = ? AND type = ? ORDER BY position ASC LIMIT 1",
                )
                .bind(&issue_team_id)
                .bind(mapped_type)
                .fetch_optional(pool)
                .await?
            };

            if let Some(new_state_id) = target_state_id {
                let new_state_type: String =
                    sqlx::query_scalar("SELECT type FROM tracker_states WHERE id = ?")
                        .bind(&new_state_id)
                        .fetch_one(pool)
                        .await?;

                if new_state_id != prev_state_id {
                    match new_state_type.as_str() {
                        "started" => {
                            if started_at.is_none() {
                                started_at = Some(now.clone());
                            }
                            completed_at = None;
                            canceled_at = None;
                        }
                        "completed" => {
                            completed_at = Some(now.clone());
                            canceled_at = None;
                        }
                        "canceled" => {
                            canceled_at = Some(now.clone());
                            completed_at = None;
                        }
                        _ => {
                            completed_at = None;
                            canceled_at = None;
                        }
                    }

                    sqlx::query(
                        "UPDATE tracker_issues
                         SET state_id = ?, started_at = ?, completed_at = ?, canceled_at = ?, updated_at = ?
                         WHERE id = ?",
                    )
                    .bind(&new_state_id)
                    .bind(&started_at)
                    .bind(&completed_at)
                    .bind(&canceled_at)
                    .bind(&now)
                    .bind(&issue_id)
                    .execute(pool)
                    .await?;
                }
            }

            if !status_changed {
                continue;
            }

            sqlx::query(
                "INSERT INTO tracker_activity (issue_id, actor_id, kind, from_value, to_value, created_at)
                 VALUES (?, NULL, 'lane_status', ?, ?, ?)",
            )
            .bind(&issue_id)
            .bind(last_lane_status.as_deref())
            .bind(effective_status)
            .bind(&now)
            .execute(pool)
            .await?;

            if !is_paused
                && pre_pause_status.as_deref() != Some(lane_status.as_str())
                && matches!(
                    lane_status.as_str(),
                    "blocked" | "done_claimed" | "verified_done" | "failed"
                )
            {
                if let Some(ref me_id) = me_user_id {
                    let notif_id = Uuid::new_v4().to_string();
                    let kind = format!("lane_{lane_status}");
                    let body = format!("Lane {lane_key} entered status {lane_status}");
                    sqlx::query(
                        "INSERT INTO tracker_notifications (id, user_id, issue_id, project_id, kind, actor_id, body, created_at)
                         VALUES (?, ?, ?, NULL, ?, NULL, ?, ?)",
                    )
                    .bind(notif_id)
                    .bind(me_id)
                    .bind(&issue_id)
                    .bind(kind)
                    .bind(body)
                    .bind(&now)
                    .execute(pool)
                    .await?;
                }
            }
        } else {
            let agent_user_id = ensure_agent_user(pool, lane_model.as_deref()).await?;
            let brief_text = read_brief_content(&brief_path).await;

            let (target_state_id, state_type) = if is_paused {
                if let Some((id, t)) = &paused_state {
                    (id.clone(), t.clone())
                } else if let Some((id, t)) = state_by_lane_status.get(&lane_status) {
                    (id.clone(), t.clone())
                } else {
                    let default_state: (String, String) = sqlx::query_as(
                        "SELECT id, type FROM tracker_states WHERE team_id = ? ORDER BY position ASC LIMIT 1",
                    )
                    .bind(&agt_team_id)
                    .fetch_one(pool)
                    .await?;
                    default_state
                }
            } else if let Some((id, t)) = state_by_lane_status.get(&lane_status) {
                (id.clone(), t.clone())
            } else {
                let default_state: (String, String) = sqlx::query_as(
                    "SELECT id, type FROM tracker_states WHERE team_id = ? ORDER BY position ASC LIMIT 1",
                )
                .bind(&agt_team_id)
                .fetch_one(pool)
                .await?;
                default_state
            };

            agt_seq += 1;
            let identifier = format!("AGT-{agt_seq}");
            let issue_id = Uuid::new_v4().to_string();

            let (started_at, completed_at, canceled_at) = match state_type.as_str() {
                "started" => (Some(now.clone()), None, None),
                "completed" => (None, Some(now.clone()), None),
                "canceled" => (None, None, Some(now.clone())),
                _ => (None, None, None),
            };

            sqlx::query("UPDATE tracker_teams SET issue_seq = ? WHERE id = ?")
                .bind(agt_seq)
                .bind(&agt_team_id)
                .execute(pool)
                .await?;

            sqlx::query(
                "INSERT INTO tracker_issues (
                    id, team_id, number, identifier, title, description, state_id,
                    priority, estimate, assignee_id, creator_id, project_id, milestone_id,
                    cycle_id, parent_id, due_date, sort_order, lane_key, created_at, updated_at,
                    started_at, completed_at, canceled_at, archived_at, snoozed_until
                 ) VALUES (
                    ?, ?, ?, ?, ?, ?, ?,
                    0, NULL, ?, ?, NULL, NULL,
                    NULL, NULL, NULL, 0.0, ?, ?, ?,
                    ?, ?, ?, NULL, NULL
                 )",
            )
            .bind(&issue_id)
            .bind(&agt_team_id)
            .bind(agt_seq)
            .bind(&identifier)
            .bind(&lane_title)
            .bind(&brief_text)
            .bind(&target_state_id)
            .bind(&agent_user_id)
            .bind(&agent_user_id)
            .bind(&lane_key)
            .bind(&now)
            .bind(&now)
            .bind(&started_at)
            .bind(&completed_at)
            .bind(&canceled_at)
            .execute(pool)
            .await?;

            sqlx::query(
                "INSERT INTO tracker_subscribers (issue_id, user_id) VALUES (?, ?) ON CONFLICT DO NOTHING",
            )
            .bind(&issue_id)
            .bind(&agent_user_id)
            .execute(pool)
            .await?;

            sqlx::query(
                "INSERT INTO tracker_activity (issue_id, actor_id, kind, from_value, to_value, created_at)
                 VALUES (?, ?, 'created', NULL, NULL, ?)",
            )
            .bind(&issue_id)
            .bind(&agent_user_id)
            .bind(&now)
            .execute(pool)
            .await?;

            sqlx::query(
                "INSERT INTO tracker_activity (issue_id, actor_id, kind, from_value, to_value, created_at)
                 VALUES (?, NULL, 'lane_status', NULL, ?, ?)",
            )
            .bind(&issue_id)
            .bind(effective_status)
            .bind(&now)
            .execute(pool)
            .await?;

            if !is_paused
                && matches!(
                    lane_status.as_str(),
                    "blocked" | "done_claimed" | "verified_done" | "failed"
                )
            {
                if let Some(ref me_id) = me_user_id {
                    let notif_id = Uuid::new_v4().to_string();
                    let kind = format!("lane_{lane_status}");
                    let body = format!("Lane {lane_key} entered status {lane_status}");
                    sqlx::query(
                        "INSERT INTO tracker_notifications (id, user_id, issue_id, project_id, kind, actor_id, body, created_at)
                         VALUES (?, ?, ?, NULL, ?, NULL, ?, ?)",
                    )
                    .bind(notif_id)
                    .bind(me_id)
                    .bind(&issue_id)
                    .bind(kind)
                    .bind(body)
                    .bind(&now)
                    .execute(pool)
                    .await?;
                }
            }
        }
    }

    Ok(())
}

pub async fn sync(pool: &SqlitePool) -> Result<()> {
    let _guard = SYNC_LOCK.lock().await;
    sync_inner(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seeded() -> SqlitePool {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO tracker_users(id, name, display_name, avatar_color, kind, is_me, created_at) VALUES('me','Me','Me','#000','human',1,'2020-01-01T00:00:00Z')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO tracker_teams(id, key, name, icon, color, system, created_at) VALUES('agt','AGT','Agents','bot','#111',1,'2020-01-01T00:00:00Z')")
            .execute(&pool).await.unwrap();
        for (position, (name, kind, status)) in [
            ("Queued", "unstarted", "queued"),
            ("Working", "started", "working"),
            ("Blocked", "started", "blocked"),
            ("In Review", "started", "done_claimed"),
            ("Changes Requested", "started", "not_done"),
            ("Verified", "completed", "verified_done"),
            ("Done", "completed", "closed"),
            ("Failed", "canceled", "failed"),
        ]
        .into_iter()
        .enumerate()
        {
            sqlx::query("INSERT INTO tracker_states(id, team_id, name, type, color, position, lane_status) VALUES(?, 'agt', ?, ?, '#222', ?, ?)")
                .bind(format!("s-{status}")).bind(name).bind(kind).bind(position as f64).bind(status)
                .execute(&pool).await.unwrap();
        }
        pool
    }

    async fn lane(pool: &SqlitePool, key: &str, status: &str) {
        sqlx::query(
            "INSERT INTO lanes(key, title, lead_session_key, cwd, brief_path, session_key, status, done_signals, last_activity_at)
             VALUES(?, ?, 'lead', '/tmp', '', 'lane', ?, '[]', '2020-01-01T00:00:00Z')
             ON CONFLICT(key) DO UPDATE SET status = excluded.status",
        )
        .bind(key).bind(format!("Lane {key}")).bind(status)
        .execute(pool).await.unwrap();
    }

    async fn issue(pool: &SqlitePool, key: &str) -> (String, String, Option<String>) {
        sqlx::query_as(
            "SELECT identifier, state_id, completed_at FROM tracker_issues WHERE lane_key = ?",
        )
        .bind(key)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn lanes_become_agt_issues_that_follow_their_status() {
        let pool = seeded().await;
        lane(&pool, "fix-a", "working").await;
        sync(&pool).await.unwrap();
        sync(&pool).await.unwrap();
        let (identifier, state, _) = issue(&pool, "fix-a").await;
        assert_eq!(
            (identifier.as_str(), state.as_str()),
            ("AGT-1", "s-working")
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracker_issues")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);

        lane(&pool, "fix-a", "verified_done").await;
        sync(&pool).await.unwrap();
        let (_, state, completed) = issue(&pool, "fix-a").await;
        assert_eq!(state, "s-verified_done");
        assert!(completed.is_some());
        let notices: Vec<String> =
            sqlx::query_scalar("SELECT kind FROM tracker_notifications WHERE user_id = 'me'")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(notices, vec!["lane_verified_done".to_string()]);
        let activity: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tracker_activity WHERE kind = 'lane_status'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(activity, 2);
        let agent: String = sqlx::query_scalar(
            "SELECT u.name FROM tracker_issues i JOIN tracker_users u ON u.id = i.assignee_id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(agent, "agent");
    }

    #[tokio::test]
    async fn a_board_without_the_agents_team_is_left_alone() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "fix-a", "working").await;
        sync(&pool).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracker_issues")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn a_paused_lane_shows_paused_and_returns_to_its_status_on_resume() {
        let pool = seeded().await;
        sqlx::query("INSERT INTO tracker_states(id, team_id, name, type, color, position, lane_paused) VALUES('s-paused', 'agt', 'Paused', 'started', '#9b8afb', 2.5, 1)")
            .execute(&pool).await.unwrap();
        lane(&pool, "fix-p", "working").await;
        sync(&pool).await.unwrap();
        crate::lanes::pause::pause(&pool, "fix-p", "session:lead")
            .await
            .unwrap();
        sync(&pool).await.unwrap();
        assert_eq!(issue(&pool, "fix-p").await.1, "s-paused");
        crate::lanes::pause::resume(&pool, "fix-p").await.unwrap();
        sync(&pool).await.unwrap();
        assert_eq!(issue(&pool, "fix-p").await.1, "s-working");
    }
}
