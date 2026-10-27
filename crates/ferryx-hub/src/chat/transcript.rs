//! The per-session transcript in `messages`, as omo-gateway's multiplexer kept it: every user
//! message and assistant reply of a Slack or dashboard session, in order. The dashboard's
//! session view and websocket read it.
use crate::storage::WriteTx;
use sqlx::SqlitePool;

use crate::error::Result;
use crate::storage::now_rfc3339;

/// Appends one message with the next global sequence number. Returns its id.
pub async fn record(
    pool: &SqlitePool,
    session_key: &str,
    role: &str,
    content: &str,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let mut tx = pool.begin_write().await?;
    let sequence: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages")
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO messages (id, session_key, role, content, sequence, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(session_key)
    .bind(role)
    .bind(content)
    .bind(sequence)
    .bind(now_rfc3339())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn messages_are_kept_in_order() {
        let pool = crate::storage::memory().await.unwrap();
        record(&pool, "k", "user", "hi").await.unwrap();
        record(&pool, "k", "assistant", "hello").await.unwrap();
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT role, content FROM messages WHERE session_key = 'k' ORDER BY sequence",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                ("user".into(), "hi".into()),
                ("assistant".into(), "hello".into())
            ]
        );
    }
}
