//! omo-gateway's lane pause (`omo-gateway lanes pause|resume`, storage/lanes.rs): the lead
//! stops driving a live lane without changing its status, and the tracker shows it Paused.
use crate::storage::WriteTx;
use std::collections::HashSet;

use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

/// True while the lane has an unresumed pause row.
pub async fn is_paused(pool: &SqlitePool, key: &str) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM lane_pauses WHERE lane_key = ? AND resumed_at IS NULL)",
    )
    .bind(key)
    .fetch_one(pool)
    .await?)
}

pub async fn paused_keys(pool: &SqlitePool) -> Result<HashSet<String>> {
    let keys: Vec<String> =
        sqlx::query_scalar("SELECT lane_key FROM lane_pauses WHERE resumed_at IS NULL")
            .fetch_all(pool)
            .await?;
    Ok(keys.into_iter().collect())
}

/// Pauses a live lane; false when it was already paused. The status is left unchanged and
/// the history row records the pause with from = to = current status, as omo.
pub async fn pause(pool: &SqlitePool, key: &str, by: &str) -> Result<bool> {
    let mut tx = pool.begin_write().await?;
    let status: String = sqlx::query_scalar("SELECT status FROM lanes WHERE key = ?")
        .bind(key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| HubError::message(format!("lane not found: {key}")))?;
    if matches!(status.as_str(), "closed" | "failed" | "verified_done") {
        return Err(HubError::message(format!("cannot pause a {status} lane")));
    }
    let paused: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM lane_pauses WHERE lane_key = ? AND resumed_at IS NULL)",
    )
    .bind(key)
    .fetch_one(&mut *tx)
    .await?;
    if paused {
        return Ok(false);
    }
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO lane_pauses(lane_key, paused_at, paused_by, resumed_at) VALUES (?, ?, ?, NULL)
         ON CONFLICT(lane_key) DO UPDATE SET paused_at = excluded.paused_at, paused_by = excluded.paused_by, resumed_at = NULL",
    )
    .bind(key)
    .bind(&now)
    .bind(by)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO lane_history(lane_key, at, from_status, to_status, reason) VALUES (?, ?, ?, ?, 'paused')")
        .bind(key)
        .bind(&now)
        .bind(&status)
        .bind(&status)
        .execute(&mut *tx)
        .await?;
    // A supervisor dispatch of this lane already claimed may be waiting for its pane: the
    // pause revokes it in the typing fence, so nothing of it is typed.
    let claimed: Vec<String> = sqlx::query_scalar(
        "SELECT d.request_id FROM supervisor_dispatches d
           JOIN supervisor_goals g ON g.goal_id = d.goal_id
          WHERE g.lane_key = ? AND d.state = 'claimed'",
    )
    .bind(key)
    .fetch_all(&mut *tx)
    .await?;
    crate::supervisor::delivery::revoke(claimed);
    tx.commit().await?;
    Ok(true)
}

/// Clears an active pause; false when the lane was not paused.
pub async fn resume(pool: &SqlitePool, key: &str) -> Result<bool> {
    let mut tx = pool.begin_write().await?;
    let now = now_rfc3339();
    let cleared = sqlx::query(
        "UPDATE lane_pauses SET resumed_at = ? WHERE lane_key = ? AND resumed_at IS NULL",
    )
    .bind(&now)
    .bind(key)
    .execute(&mut *tx)
    .await?;
    if cleared.rows_affected() != 1 {
        return Ok(false);
    }
    let status: String = sqlx::query_scalar("SELECT status FROM lanes WHERE key = ?")
        .bind(key)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO lane_history(lane_key, at, from_status, to_status, reason) VALUES (?, ?, ?, ?, 'resumed')")
        .bind(key)
        .bind(&now)
        .bind(&status)
        .bind(&status)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// A pause is a throttle for the run it was set in, not a property of the lane: close does
/// not clear it, so a reopen must, or the reopened lane is skipped forever (omo).
pub async fn clear_on_reopen(pool: &SqlitePool, key: &str) -> Result<()> {
    sqlx::query("UPDATE lane_pauses SET resumed_at = ? WHERE lane_key = ? AND resumed_at IS NULL")
        .bind(now_rfc3339())
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanes::state::{insert_lane, transition, LaneStatus};

    #[tokio::test]
    async fn a_paused_lane_keeps_its_status_and_a_reopen_clears_the_pause() {
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "ledger", "Ledger", "lead", "/tmp", "file marker")
            .await
            .unwrap();
        transition(&pool, "ledger", LaneStatus::Working, Some("launched"))
            .await
            .unwrap();
        assert!(pause(&pool, "ledger", "session:lead").await.unwrap());
        assert!(!pause(&pool, "ledger", "session:lead").await.unwrap());
        assert!(is_paused(&pool, "ledger").await.unwrap());
        assert!(paused_keys(&pool).await.unwrap().contains("ledger"));
        let status: String = sqlx::query_scalar("SELECT status FROM lanes WHERE key = 'ledger'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "working");
        assert!(resume(&pool, "ledger").await.unwrap());
        assert!(!resume(&pool, "ledger").await.unwrap());
        assert!(pause(&pool, "nope", "x").await.is_err());

        // Paused, then force-closed: the pause survives the close, and a reopen clears it.
        assert!(pause(&pool, "ledger", "session:lead").await.unwrap());
        transition(
            &pool,
            "ledger",
            LaneStatus::Closed,
            Some("forced: done by hand"),
        )
        .await
        .unwrap();
        assert!(is_paused(&pool, "ledger").await.unwrap());
        assert!(pause(&pool, "ledger", "x").await.is_err());
        transition(&pool, "ledger", LaneStatus::Working, Some("note: reopened"))
            .await
            .unwrap();
        assert!(!is_paused(&pool, "ledger").await.unwrap());
        let reasons: Vec<Option<String>> = sqlx::query_scalar(
            "SELECT reason FROM lane_history WHERE lane_key = 'ledger' ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(reasons.contains(&Some("paused".into())));
        assert!(reasons.contains(&Some("resumed".into())));
    }
}
