//! Outbound chat replies. The text is what the caller supplies. Nothing here reads a pane.
use sqlx::SqlitePool;

use super::router::{Outbound, PlatformRouter};
use super::session::SessionKey;
use crate::error::Result;
use crate::storage::now_rfc3339;

pub async fn chat_reply(
    pool: &SqlitePool,
    session: &SessionKey,
    text: &str,
    platforms: Option<&PlatformRouter>,
) -> Result<i64> {
    if let Some(platforms) = platforms {
        platforms.send(session, text).await?;
    }
    let inserted = sqlx::query(
        "INSERT INTO chat_outbox(session_key, platform, text, created_at) VALUES(?,?,?,?)",
    )
    .bind(session.storage_key())
    .bind(&session.platform)
    .bind(text)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    let id = inserted.last_insert_rowid();
    if publish(session, text).await? {
        mark_delivered(pool, id).await?;
    }
    Ok(id)
}

/// An owner notice. On Slack a detected prompt is posted as answer buttons under the text;
/// the stored outbox row is the text alone, so a later flush never replays a stale card.
pub async fn chat_reply_card(
    pool: &SqlitePool,
    session: &SessionKey,
    text: &str,
    approval: Option<&crate::sessions::watch::ApprovalCard>,
) -> Result<i64> {
    let Some(card) = approval else {
        return chat_reply(pool, session, text, None).await;
    };
    let inserted = sqlx::query(
        "INSERT INTO chat_outbox(session_key, platform, text, created_at) VALUES(?,?,?,?)",
    )
    .bind(session.storage_key())
    .bind(&session.platform)
    .bind(text)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    let id = inserted.last_insert_rowid();
    let posted = if session.platform.eq_ignore_ascii_case("slack") {
        super::slack::publish_card(session, text, card).await?
    } else {
        publish(session, text).await?
    };
    if posted {
        mark_delivered(pool, id).await?;
    }
    Ok(id)
}

async fn publish(session: &SessionKey, text: &str) -> Result<bool> {
    if session.platform.eq_ignore_ascii_case("slack") {
        return super::slack::publish(session, text).await;
    }
    if session.platform.eq_ignore_ascii_case("discord") {
        return super::discord::publish(session, text).await;
    }
    let _ = text;
    Ok(false)
}

/// Delivers replies that were stored before a token was available. One pass, not a poll.
pub async fn flush_pending(pool: &SqlitePool) -> Result<u64> {
    let mut delivered = 0u64;
    for (id, key, text) in pending(pool).await? {
        let Ok(session) = SessionKey::from_storage_key(&key) else {
            continue;
        };
        if publish(&session, &text).await? {
            mark_delivered(pool, id).await?;
            delivered += 1;
        }
    }
    Ok(delivered)
}

/// Publishes one stored outbox row now and marks it delivered when the transport took it.
/// A key that is not a chat session (a lane name) cannot be delivered and reports false.
pub async fn deliver_stored(pool: &SqlitePool, id: i64, key: &str, text: &str) -> Result<bool> {
    let Ok(session) = SessionKey::from_storage_key(key) else {
        return Ok(false);
    };
    if publish(&session, text).await? {
        mark_delivered(pool, id).await?;
        return Ok(true);
    }
    Ok(false)
}

/// How old an undelivered outbox row must be before a retry may post it. Its writer posts it
/// right after the insert; a retry that takes it sooner posts it a second time.
pub const OUTBOX_SETTLE: chrono::Duration = chrono::Duration::minutes(2);

/// The periodic retry of the outbox. Only rows older than `settle` are tried, so a reply
/// whose first post is still in flight is not posted twice, and only rows younger than
/// `max_age`, so a channel that is gone does not retry forever. One failure does not stop
/// the pass.
pub async fn flush_stale(
    pool: &SqlitePool,
    settle: chrono::Duration,
    max_age: chrono::Duration,
) -> Result<u64> {
    let now = chrono::Utc::now();
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, session_key, text FROM chat_outbox
          WHERE delivered_at IS NULL
            AND julianday(created_at) <= julianday(?)
            AND julianday(created_at) >= julianday(?)
          ORDER BY id",
    )
    .bind((now - settle).to_rfc3339())
    .bind((now - max_age).to_rfc3339())
    .fetch_all(pool)
    .await?;
    let mut delivered = 0u64;
    for (id, key, text) in rows {
        match deliver_stored(pool, id, &key, &text).await {
            Ok(true) => delivered += 1,
            Ok(false) => {}
            Err(error) => tracing::warn!(id, %error, "stored chat reply was not delivered"),
        }
    }
    Ok(delivered)
}

pub async fn pending(pool: &SqlitePool) -> Result<Vec<(i64, String, String)>> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, session_key, text FROM chat_outbox WHERE delivered_at IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_delivered(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("UPDATE chat_outbox SET delivered_at = ? WHERE id = ?")
        .bind(now_rfc3339())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Records a reply without a platform transport. Used by `chat.reply` when Slack is not connected.
pub struct OutboxOutbound {
    pub pool: SqlitePool,
}

#[async_trait::async_trait]
impl Outbound for OutboxOutbound {
    async fn send(&self, _session: &SessionKey, _text: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_reply_is_stored_and_does_not_need_a_screen() {
        let pool = crate::storage::memory().await.unwrap();
        let session = SessionKey::new("slack", None::<String>, "C1", None::<String>, "U1");
        let id = chat_reply(&pool, &session, "shipped", None).await.unwrap();
        let queued = pending(&pool).await.unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, id);
        assert_eq!(queued[0].2, "shipped");
        mark_delivered(&pool, id).await.unwrap();
        assert!(pending(&pool).await.unwrap().is_empty());
    }
}
