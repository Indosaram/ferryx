//! Delivers lane events that have not been handed to a lead yet.
//! This is the queue, not an in-gateway model loop.
use sqlx::SqlitePool;

use crate::error::Result;
use crate::storage::now_rfc3339;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneEvent {
    pub id: i64,
    pub lane_key: Option<String>,
    pub lead_session_key: String,
    pub kind: String,
    pub body: String,
}

pub async fn enqueue(
    pool: &SqlitePool,
    lane_key: &str,
    lead: &str,
    kind: &str,
    body: &str,
) -> Result<i64> {
    let result = sqlx::query(
        "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES(?,?,?,?,?)",
    )
    .bind(lane_key)
    .bind(lead)
    .bind(kind)
    .bind(body)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn take_undelivered(pool: &SqlitePool) -> Result<Vec<LaneEvent>> {
    let now = now_rfc3339();
    let rows: Vec<(i64, Option<String>, String, String, String)> = sqlx::query_as(
        "SELECT id, lane_key, lead_session_key, kind, body FROM lane_events
         WHERE delivered_at IS NULL AND (release_at IS NULL OR release_at <= ?)
         ORDER BY id",
    )
    .bind(&now)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    for (id, _, _, _, _) in &rows {
        sqlx::query("UPDATE lane_events SET delivered_at = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(rows
        .into_iter()
        .map(|(id, lane_key, lead_session_key, kind, body)| LaneEvent {
            id,
            lane_key,
            lead_session_key,
            kind,
            body,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_event_is_delivered_once() {
        let pool = crate::storage::memory().await.unwrap();
        enqueue(&pool, "lane-a", "lead", "note", "hello")
            .await
            .unwrap();
        let first = take_undelivered(&pool).await.unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].body, "hello");
        let second = take_undelivered(&pool).await.unwrap();
        assert!(second.is_empty());
        let later = "2999-01-01T00:00:00Z";
        sqlx::query(
            "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at, release_at) VALUES(NULL,'lead','action_loop','held',?,?)",
        )
        .bind(crate::storage::now_rfc3339())
        .bind(later)
        .execute(&pool)
        .await
        .unwrap();
        assert!(take_undelivered(&pool).await.unwrap().is_empty());
    }
}
