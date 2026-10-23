//! omo-gateway's mirror.rs: a cron delivery posted to a Slack channel or thread is mirrored
//! into that conversation's session transcript, and the next omo turn of the session gets
//! the pending deliveries as context, so the agent knows what "it" posted there and can
//! answer follow-ups. The inbox digest (it can carry a one-time owner token) is never
//! mirrored into a prompt, and owner tokens are redacted from every mirrored copy.
use std::collections::HashSet;

use serde_json::json;
use sqlx::SqlitePool;

use crate::chat::session::SessionKey;
use crate::error::Result;
use crate::storage::now_rfc3339;

/// Upper bound on briefing text carried into one turn, so a backlog of cron output cannot
/// crowd out the user's own message.
pub const MAX_BRIEFING_CHARS: usize = 6000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CronBriefings {
    pub text: String,
    pub message_ids: Vec<String>,
}

/// Appends an out-of-band record to an existing session's transcript.
pub async fn mirror_to_session(
    pool: &SqlitePool,
    session_key: &str,
    role: &str,
    content: &str,
    source_label: Option<&str>,
) -> Result<bool> {
    let text = content.trim();
    if text.is_empty() {
        return Ok(false);
    }
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions WHERE session_key = ?)")
            .bind(session_key)
            .fetch_one(pool)
            .await?;
    if !exists {
        return Ok(false);
    }
    let metadata = json!({
        "mirror": true,
        "mirror_source": source_label.unwrap_or("out_of_band"),
    });
    let mut tx = pool.begin().await?;
    let sequence: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages")
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO messages (id, session_key, role, content, metadata_json, sequence, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(session_key)
    .bind(role)
    .bind(text)
    .bind(metadata.to_string())
    .bind(sequence)
    .bind(now_rfc3339())
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE sessions SET updated_at = CURRENT_TIMESTAMP WHERE session_key = ?")
        .bind(session_key)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

fn bot_of(key: &str) -> Option<String> {
    SessionKey::from_storage_key(key)
        .ok()
        .and_then(|key| key.bot_id)
}

/// The most recently active session in this channel (or thread), preferring the given user.
/// With several bots in the conversation and none named, nothing is picked.
pub async fn find_session_by_origin(
    pool: &SqlitePool,
    platform: &str,
    chat_id: &str,
    thread_id: Option<&str>,
    user_id: Option<&str>,
    bot_id: Option<&str>,
) -> Result<Option<String>> {
    let platform = platform.to_ascii_lowercase();
    let rows: Vec<(String, Option<String>)> = match thread_id {
        Some(thread) => {
            sqlx::query_as(
                "SELECT session_key, user_id FROM sessions
                 WHERE lower(platform) = ? AND channel_id = ? AND thread_id = ?
                 ORDER BY updated_at DESC",
            )
            .bind(&platform)
            .bind(chat_id)
            .bind(thread)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT session_key, user_id FROM sessions
                 WHERE lower(platform) = ? AND channel_id = ? AND (thread_id IS NULL OR thread_id = '')
                 ORDER BY updated_at DESC",
            )
            .bind(&platform)
            .bind(chat_id)
            .fetch_all(pool)
            .await?
        }
    };
    let matching: Vec<&(String, Option<String>)> = rows
        .iter()
        .filter(|row| bot_id.is_none_or(|bot| bot_of(&row.0).as_deref() == Some(bot)))
        .collect();
    let Some(first) = matching.first() else {
        return Ok(None);
    };
    if let Some(user) = user_id {
        if let Some(row) = matching.iter().find(|row| row.1.as_deref() == Some(user)) {
            return Ok(Some(row.0.clone()));
        }
    }
    if bot_id.is_none() {
        let bots: HashSet<Option<String>> = matching.iter().map(|row| bot_of(&row.0)).collect();
        if bots.len() > 1 {
            return Ok(None);
        }
    }
    Ok(Some(first.0.clone()))
}

/// omo's `mirror_cron_delivery_to_session`: the job's own session when it is this
/// conversation, otherwise the conversation's session; false when there is none.
pub async fn mirror_cron_delivery(
    pool: &SqlitePool,
    job_id: &str,
    platform: &str,
    chat_id: &str,
    thread_id: Option<&str>,
    content: &str,
) -> Result<bool> {
    if job_id == crate::inbox::DIGEST_JOB_ID {
        return Ok(false);
    }
    let job_session: Option<String> =
        sqlx::query_scalar("SELECT session_key FROM scheduled_jobs WHERE id = ?")
            .bind(job_id)
            .fetch_optional(pool)
            .await?
            .flatten();
    let own = job_session.filter(|key| {
        SessionKey::from_storage_key(key).is_ok_and(|parsed| {
            parsed.channel_id == chat_id && parsed.thread_id.as_deref() == thread_id
        })
    });
    let key = match own {
        Some(key) => Some(key),
        None => {
            find_session_by_origin(
                pool,
                platform,
                chat_id,
                thread_id,
                Some(&format!("cron:{job_id}")),
                None,
            )
            .await?
        }
    };
    let Some(key) = key else {
        return Ok(false);
    };
    let content = crate::cron_remediate::redact_token(content);
    mirror_to_session(
        pool,
        &key,
        "assistant",
        &content,
        Some(&format!("cron:{job_id}")),
    )
    .await
}

/// Mirrored cron deliveries not yet handed to the agent, rendered as context, newest kept
/// when the backlog is over [`MAX_BRIEFING_CHARS`] (the rest stay pending). Not consumed:
/// the caller marks them once the turn carrying them succeeded.
pub async fn peek_pending_cron_briefings(
    pool: &SqlitePool,
    session_key: &str,
) -> Result<Option<CronBriefings>> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT id, content, created_at,
                COALESCE(CASE WHEN json_valid(metadata_json)
                         THEN json_extract(metadata_json, '$.mirror_source') END, '')
         FROM messages
         WHERE session_key = ?
           AND CASE WHEN json_valid(metadata_json) THEN json_extract(metadata_json, '$.mirror') END = 1
           AND CASE WHEN json_valid(metadata_json)
               THEN json_extract(metadata_json, '$.mirror_source') END LIKE 'cron:%'
           AND CASE WHEN json_valid(metadata_json)
               THEN COALESCE(json_extract(metadata_json, '$.mirror_injected'), 0) END = 0
           AND CASE WHEN json_valid(metadata_json)
               THEN json_extract(metadata_json, '$.mirror_source') END <> ?
         ORDER BY created_at ASC, sequence ASC",
    )
    .bind(session_key)
    .bind(format!("cron:{}", crate::inbox::DIGEST_JOB_ID))
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let header = "[Context: messages you already posted to this conversation as scheduled (cron) deliveries. \
         The user may refer to them.]\n";
    let mut used = header.chars().count();
    let mut selected: Vec<(String, String)> = Vec::new();
    let mut truncated = false;
    for (id, content, created_at, source) in rows.iter().rev() {
        let job = source.strip_prefix("cron:").unwrap_or(source);
        let mut entry = format!("--- {created_at} ({job})\n{}\n", content.trim());
        let mut len = entry.chars().count();
        if selected.is_empty() && used + len > MAX_BRIEFING_CHARS {
            // A single oversized delivery keeps its tail so it is still delivered once.
            let keep = MAX_BRIEFING_CHARS.saturating_sub(used);
            entry = entry.chars().skip(len - keep.min(len)).collect();
            len = entry.chars().count();
            truncated = true;
        } else if used + len > MAX_BRIEFING_CHARS {
            truncated = true;
            break;
        }
        used += len;
        selected.push((id.clone(), entry));
    }
    selected.reverse();
    let mut text = String::new();
    if truncated {
        text.push_str("[Context: earlier scheduled deliveries truncated]\n");
    }
    text.push_str(header);
    let mut message_ids = Vec::with_capacity(selected.len());
    for (id, entry) in selected {
        text.push_str(&entry);
        message_ids.push(id);
    }
    Ok(Some(CronBriefings { text, message_ids }))
}

pub async fn mark_cron_briefings_injected(pool: &SqlitePool, message_ids: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for id in message_ids {
        sqlx::query(
            "UPDATE messages SET metadata_json = json_set(metadata_json, '$.mirror_injected', json('true'))
             WHERE id = ? AND json_valid(metadata_json)",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn session(pool: &SqlitePool, key: &str, channel: &str, thread: Option<&str>) {
        sqlx::query(
            "INSERT INTO sessions (session_key, platform, channel_id, thread_id, user_id, state_json) VALUES (?, 'slack', ?, ?, 'U1', '{}')",
        )
        .bind(key)
        .bind(channel)
        .bind(thread)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_cron_delivery_is_mirrored_and_handed_to_the_next_turn_exactly_once() {
        let pool = crate::storage::memory().await.unwrap();
        let key =
            SessionKey::new("slack", None::<String>, "C1", None::<String>, "U1").storage_key();
        session(&pool, &key, "C1", None).await;
        assert!(mirror_cron_delivery(
            &pool,
            "morning",
            "slack",
            "C1",
            None,
            "Weather: sunny. run --owner-token abc123 now"
        )
        .await
        .unwrap());
        // No session in that channel: nothing to mirror into. The digest is never mirrored.
        assert!(
            !mirror_cron_delivery(&pool, "morning", "slack", "C9", None, "x")
                .await
                .unwrap()
        );
        assert!(!mirror_cron_delivery(
            &pool,
            crate::inbox::DIGEST_JOB_ID,
            "slack",
            "C1",
            None,
            "digest"
        )
        .await
        .unwrap());

        let briefings = peek_pending_cron_briefings(&pool, &key)
            .await
            .unwrap()
            .unwrap();
        assert!(briefings
            .text
            .starts_with("[Context: messages you already posted"));
        assert!(briefings.text.contains("(morning)\nWeather: sunny."));
        assert!(
            !briefings.text.contains("abc123"),
            "owner tokens never reach a prompt"
        );
        assert_eq!(briefings.message_ids.len(), 1);
        // Peeking does not consume; marking does.
        assert!(peek_pending_cron_briefings(&pool, &key)
            .await
            .unwrap()
            .is_some());
        mark_cron_briefings_injected(&pool, &briefings.message_ids)
            .await
            .unwrap();
        assert!(peek_pending_cron_briefings(&pool, &key)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn a_backlog_keeps_the_newest_deliveries_and_leaves_the_rest_pending() {
        let pool = crate::storage::memory().await.unwrap();
        session(&pool, "k", "C1", Some("1.1")).await;
        for n in 0..5 {
            let body = format!("delivery {n} {}", "x".repeat(2000));
            assert!(
                mirror_to_session(&pool, "k", "assistant", &body, Some("cron:job"))
                    .await
                    .unwrap()
            );
        }
        assert_eq!(
            find_session_by_origin(&pool, "slack", "C1", Some("1.1"), None, None)
                .await
                .unwrap(),
            Some("k".into())
        );
        let briefings = peek_pending_cron_briefings(&pool, "k")
            .await
            .unwrap()
            .unwrap();
        assert!(briefings
            .text
            .starts_with("[Context: earlier scheduled deliveries truncated]"));
        assert!(briefings.text.contains("delivery 4"));
        assert!(!briefings.text.contains("delivery 0"));
        assert!(briefings.text.chars().count() <= MAX_BRIEFING_CHARS + 60);
        mark_cron_briefings_injected(&pool, &briefings.message_ids)
            .await
            .unwrap();
        let rest = peek_pending_cron_briefings(&pool, "k")
            .await
            .unwrap()
            .unwrap();
        // The next turn gets the next-newest pair; the oldest still waits its turn.
        assert!(rest.text.contains("delivery 2 ") && rest.text.contains("delivery 1 "));
        assert!(!rest.text.contains("delivery 3 ") && !rest.text.contains("delivery 0 "));
        mark_cron_briefings_injected(&pool, &rest.message_ids)
            .await
            .unwrap();
        let last = peek_pending_cron_briefings(&pool, "k")
            .await
            .unwrap()
            .unwrap();
        assert!(last.text.contains("delivery 0 ") && !last.text.contains("truncated"));
    }
}
