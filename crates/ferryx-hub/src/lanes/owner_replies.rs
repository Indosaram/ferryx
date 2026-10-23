//! omo-gateway's lane owner replies (lanes/slack_threads.rs intercept_owner_reply,
//! storage/lanes.rs record_owner_reply, runner.rs apply_owner_replies): when the owner
//! replies in the Slack thread of a closed, failed or verified-done lane, the reply is
//! consumed before chat routing, the lane is reopened, and the text goes to its pane.
//! A reply that cannot be applied stays recorded with its error and the lead is told once.
use sqlx::SqlitePool;

use super::state::{transition, LaneStatus};
use crate::agent_control::{AgentControl, AgentOp};
use crate::error::Result;
use crate::storage::now_rfc3339;

pub struct OwnerReply<'a> {
    pub channel: &'a str,
    pub thread_ts: &'a str,
    pub author: &'a str,
    pub owner: &'a str,
    pub message_id: &'a str,
    pub text: &'a str,
}

/// True when the message was an owner reply to a finished lane and has been consumed
/// (including a redelivered duplicate). False leaves the message to normal routing.
pub async fn intercept(
    pool: &SqlitePool,
    control: &dyn AgentControl,
    reply: &OwnerReply<'_>,
) -> Result<bool> {
    let text = reply.text.trim();
    if reply.owner.is_empty()
        || reply.author != reply.owner
        || text.is_empty()
        || text.eq_ignore_ascii_case("/stop")
        || reply.message_id.is_empty()
    {
        return Ok(false);
    }
    // A redelivered reply was consumed the first time, whatever the lane is doing now.
    let seen: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM lane_owner_replies WHERE message_id = ?)")
            .bind(reply.message_id)
            .fetch_one(pool)
            .await?;
    if seen {
        return Ok(true);
    }
    let thread = super::threads::thread_ref(reply.channel, reply.thread_ts);
    let lane: Option<(String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT key, status, omo_thread_id, lead_session_key FROM lanes WHERE thread_channel_id = ?
         AND (status IN ('closed', 'failed', 'verified_done')
              OR EXISTS(SELECT 1 FROM lane_owner_replies r WHERE r.lane_key = lanes.key AND r.state = 'pending'))
         LIMIT 1",
    )
    .bind(&thread)
    .fetch_optional(pool)
    .await?;
    let Some((key, status, pane, lead)) = lane else {
        return Ok(false);
    };
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO lane_owner_replies(message_id, lane_key, author_id, text, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(reply.message_id)
    .bind(&key)
    .bind(reply.author)
    .bind(text)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    if inserted.rows_affected() == 0 {
        return Ok(true);
    }
    match apply(
        pool,
        control,
        &key,
        &status,
        pane.as_deref(),
        reply.message_id,
        text,
    )
    .await
    {
        Ok(()) => {
            sqlx::query("UPDATE lane_owner_replies SET state = 'applied', error = NULL WHERE message_id = ?")
                .bind(reply.message_id)
                .execute(pool)
                .await?;
        }
        Err(error) => {
            tracing::warn!(%key, %error, "owner reply remains pending");
            sqlx::query("UPDATE lane_owner_replies SET error = ? WHERE message_id = ?")
                .bind(error.to_string())
                .bind(reply.message_id)
                .execute(pool)
                .await?;
            let body: String = format!("owner reply to {key} could not be applied: {error}")
                .chars()
                .take(2000)
                .collect();
            let now = now_rfc3339();
            sqlx::query(
                "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at, release_at) VALUES(?,?,'owner_reply_failed',?,?,?)",
            )
            .bind(&key)
            .bind(&lead)
            .bind(body)
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await?;
        }
    }
    Ok(true)
}

async fn apply(
    pool: &SqlitePool,
    control: &dyn AgentControl,
    key: &str,
    status: &str,
    pane: Option<&str>,
    message_id: &str,
    text: &str,
) -> Result<()> {
    // omo closes a verified-done lane before reopening it from the owner's reply.
    if status == "verified_done" {
        super::worktree::close(pool, key, None).await?;
    }
    if matches!(status, "closed" | "failed" | "verified_done") {
        transition(pool, key, LaneStatus::Working, Some("note: reopened"))
            .await
            .map_err(|error| crate::error::HubError::message(error.to_string()))?;
        super::worktree::restore(pool, key).await?;
    }
    if let Some(session_id) = pane.filter(|pane| !pane.is_empty()) {
        control
            .execute(
                None,
                AgentOp::Send {
                    session_id: session_id.to_owned(),
                    text: format!(
                        "The owner replied in this lane's thread and the lane was reopened:\n\n{text}"
                    ),
                    request_id: format!("lane-{key}-owner-{message_id}"),
                },
            )
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;
    use crate::lanes::state::{insert_lane, set_pane};

    async fn finished_lane(pool: &SqlitePool, key: &str, to: LaneStatus) {
        insert_lane(pool, key, "Ledger", "lead-key", "/tmp", "file marker")
            .await
            .unwrap();
        set_pane(pool, key, "pane-7").await.unwrap();
        transition(pool, key, LaneStatus::Working, Some("launched"))
            .await
            .unwrap();
        if to != LaneStatus::Working {
            transition(pool, key, to, Some("forced: test"))
                .await
                .unwrap();
        }
        sqlx::query("UPDATE lanes SET thread_channel_id = 'slack:C1:111.1' WHERE key = ?")
            .bind(key)
            .execute(pool)
            .await
            .unwrap();
    }

    fn reply<'a>(author: &'a str, id: &'a str, text: &'a str) -> OwnerReply<'a> {
        OwnerReply {
            channel: "C1",
            thread_ts: "111.1",
            author,
            owner: "UOWNER",
            message_id: id,
            text,
        }
    }

    #[tokio::test]
    async fn an_owner_reply_in_a_closed_lane_thread_reopens_it_and_reaches_the_pane_once() {
        let pool = crate::storage::memory().await.unwrap();
        finished_lane(&pool, "ledger", LaneStatus::Failed).await;
        let control = RecordingControl::default();

        // Someone else, or /stop, is left to normal routing.
        assert!(!intercept(&pool, &control, &reply("UOTHER", "m0", "again"))
            .await
            .unwrap());
        assert!(!intercept(&pool, &control, &reply("UOWNER", "m0", "/stop"))
            .await
            .unwrap());

        assert!(intercept(
            &pool,
            &control,
            &reply("UOWNER", "m1", "also fix the footer")
        )
        .await
        .unwrap());
        // Slack redelivery is consumed without a second send.
        assert!(intercept(
            &pool,
            &control,
            &reply("UOWNER", "m1", "also fix the footer")
        )
        .await
        .unwrap());
        let status: String = sqlx::query_scalar("SELECT status FROM lanes WHERE key = 'ledger'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "working");
        let sends: Vec<String> = control
            .ops()
            .into_iter()
            .filter_map(|(_, op)| match op {
                AgentOp::Send {
                    session_id, text, ..
                } => {
                    assert_eq!(session_id, "pane-7");
                    Some(text)
                }
                _ => None,
            })
            .collect();
        assert_eq!(sends.len(), 1);
        assert!(sends[0].contains("also fix the footer"));
        let state: String =
            sqlx::query_scalar("SELECT state FROM lane_owner_replies WHERE message_id = 'm1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, "applied");
        let reasons: Vec<Option<String>> = sqlx::query_scalar(
            "SELECT reason FROM lane_history WHERE lane_key = 'ledger' ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(reasons.last().unwrap().as_deref(), Some("note: reopened"));

        // Now working: a further owner message is the lane's normal chat, not a reopen.
        assert!(
            !intercept(&pool, &control, &reply("UOWNER", "m2", "status?"))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn an_owner_reply_to_a_verified_lane_closes_then_reopens_it() {
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "gate", "Gate", "lead-key", "/tmp", "file marker")
            .await
            .unwrap();
        for (to, reason) in [
            (LaneStatus::Working, "launched"),
            (LaneStatus::DoneClaimed, "claim"),
            (LaneStatus::VerifiedDone, "verified"),
        ] {
            transition(&pool, "gate", to, Some(reason)).await.unwrap();
        }
        sqlx::query("UPDATE lanes SET thread_channel_id = 'slack:C1:111.1' WHERE key = 'gate'")
            .execute(&pool)
            .await
            .unwrap();
        let control = RecordingControl::default();
        assert!(
            intercept(&pool, &control, &reply("UOWNER", "m9", "one more check"))
                .await
                .unwrap()
        );
        let path: Vec<String> = sqlx::query_scalar(
            "SELECT to_status FROM lane_history WHERE lane_key = 'gate' ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(&path[path.len() - 2..], ["closed", "working"]);
        // No pane yet: nothing to send, the reopened lane is driven by the runner.
        assert!(control.ops().is_empty());
    }
}
