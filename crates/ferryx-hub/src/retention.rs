//! Storage retention, ported from omo-gateway (`storage/retention.rs`, `cron/store.rs`).
//! Without it the action-loop bookkeeping, watch notices, cron run history and the hub log
//! grow forever. Runs at boot and once a day.
use chrono::{DateTime, TimeDelta, Utc};
use sqlx::SqlitePool;

use crate::error::{HubError, Result};

pub const DEFAULT_RETENTION_DAYS: i64 = 30;
pub const RETENTION_DAYS_ENV: &str = "OMON_STORAGE_RETENTION_DAYS";
pub const DEFAULT_CRON_RUNS_RETENTION_DAYS: i64 = 14;
pub const CRON_RUNS_RETENTION_DAYS_ENV: &str = "CRON_RUNS_RETENTION_DAYS";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub action_loop_seen: u64,
    pub action_loop_outcomes: u64,
    pub session_watch_notices: u64,
    pub chat_outbox: u64,
    pub hub_log: u64,
    pub cron_runs: u64,
}

impl PruneReport {
    pub fn total(&self) -> u64 {
        self.action_loop_seen
            + self.action_loop_outcomes
            + self.session_watch_notices
            + self.chat_outbox
            + self.hub_log
            + self.cron_runs
    }
}

/// `OMON_STORAGE_RETENTION_DAYS`: a whole number of days, at least 1. Unset means 30.
pub fn parse_retention_days(value: Option<&str>) -> Result<i64> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(DEFAULT_RETENTION_DAYS);
    };
    match value.parse::<i64>() {
        Ok(days) if days >= 1 => Ok(days),
        _ => Err(HubError::message(format!(
            "{RETENTION_DAYS_ENV} must be a whole number of days of at least 1, got `{value}`"
        ))),
    }
}

/// `CRON_RUNS_RETENTION_DAYS`: a non-negative integer. Unset means 14; 0 keeps only live runs.
pub fn parse_cron_runs_retention_days(value: Option<&str>) -> Result<i64> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(DEFAULT_CRON_RUNS_RETENTION_DAYS);
    };
    match value.parse::<i64>() {
        Ok(days) if days >= 0 => Ok(days),
        _ => Err(HubError::message(format!(
            "{CRON_RUNS_RETENTION_DAYS_ENV} must be a non-negative integer, got `{value}`"
        ))),
    }
}

fn cutoff(now: DateTime<Utc>, days: i64) -> Result<String> {
    TimeDelta::try_days(days)
        .and_then(|span| now.checked_sub_signed(span))
        .map(|at| at.to_rfc3339())
        .ok_or_else(|| HubError::message(format!("retention of {days} days is out of range")))
}

async fn delete_before(pool: &SqlitePool, sql: &str, cutoff: &str) -> Result<u64> {
    Ok(sqlx::query(sql)
        .bind(cutoff)
        .execute(pool)
        .await?
        .rows_affected())
}

pub async fn prune_older_than(
    pool: &SqlitePool,
    retention_days: i64,
    cron_runs_days: i64,
    now: DateTime<Utc>,
) -> Result<PruneReport> {
    if retention_days < 1 {
        return Err(HubError::message(format!(
            "storage retention days must be at least 1, got {retention_days}"
        )));
    }
    if cron_runs_days < 0 {
        return Err(HubError::message(
            "cron run retention days must be non-negative",
        ));
    }
    let cut = cutoff(now, retention_days)?;
    // Only seen-markers whose inbox row is gone; a live row keeps its marker so it is
    // never surfaced twice.
    let action_loop_seen = delete_before(
        pool,
        "DELETE FROM action_loop_seen
          WHERE julianday(seen_at) < julianday(?)
            AND (
              (item_key LIKE 'inbox:%' AND NOT EXISTS (
                  SELECT 1 FROM inbox_items i WHERE action_loop_seen.item_key = 'inbox:' || i.id))
              OR (item_key LIKE 'lane_event:%' AND NOT EXISTS (
                  SELECT 1 FROM lane_events e WHERE action_loop_seen.item_key = 'lane_event:' || e.id))
            )",
        &cut,
    )
    .await?;
    let action_loop_outcomes = delete_before(
        pool,
        "DELETE FROM action_loop_outcomes
          WHERE status IN ('verified', 'failed') AND julianday(updated_at) < julianday(?)",
        &cut,
    )
    .await?;
    let session_watch_notices = delete_before(
        pool,
        "DELETE FROM session_watch_notices WHERE julianday(sent_at) < julianday(?)",
        &cut,
    )
    .await?;
    let chat_outbox = delete_before(
        pool,
        "DELETE FROM chat_outbox
          WHERE delivered_at IS NOT NULL AND julianday(created_at) < julianday(?)",
        &cut,
    )
    .await?;
    let hub_log = delete_before(
        pool,
        "DELETE FROM hub_log WHERE julianday(at) < julianday(?)",
        &cut,
    )
    .await?;
    let cron_runs = delete_before(
        pool,
        "DELETE FROM cron_runs
          WHERE status IN ('succeeded', 'failed')
            AND julianday(COALESCE(finished_at, started_at)) < julianday(?)",
        &cutoff(now, cron_runs_days)?,
    )
    .await?;
    Ok(PruneReport {
        action_loop_seen,
        action_loop_outcomes,
        session_watch_notices,
        chat_outbox,
        hub_log,
        cron_runs,
    })
}

/// One sweep with the configured windows. A bad env value is reported, not guessed around.
pub async fn prune(pool: &SqlitePool, now: DateTime<Utc>) -> Result<PruneReport> {
    let days = parse_retention_days(crate::hub_env::var(RETENTION_DAYS_ENV).ok().as_deref())?;
    let runs = parse_cron_runs_retention_days(
        crate::hub_env::var(CRON_RUNS_RETENTION_DAYS_ENV)
            .ok()
            .as_deref(),
    )?;
    prune_older_than(pool, days, runs, now).await
}

/// Boot sweep, then one a day, for the life of the hub.
pub async fn serve(pool: SqlitePool) {
    loop {
        match prune(&pool, Utc::now()).await {
            Ok(report) => tracing::info!(
                total = report.total(),
                action_loop = report.action_loop_seen,
                outcomes = report.action_loop_outcomes,
                notices = report.session_watch_notices,
                outbox = report.chat_outbox,
                hub_log = report.hub_log,
                cron_runs = report.cron_runs,
                "storage retention sweep"
            ),
            Err(error) => tracing::warn!(%error, "storage retention sweep failed"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_values_follow_omo() {
        assert_eq!(parse_retention_days(None).unwrap(), 30);
        assert_eq!(parse_retention_days(Some("90")).unwrap(), 90);
        for bad in ["0", "-1", "x", "1.5"] {
            assert!(parse_retention_days(Some(bad)).is_err(), "{bad}");
        }
        assert_eq!(parse_cron_runs_retention_days(None).unwrap(), 14);
        assert_eq!(parse_cron_runs_retention_days(Some("0")).unwrap(), 0);
        assert!(parse_cron_runs_retention_days(Some("-1")).is_err());
    }

    #[tokio::test]
    async fn only_finished_old_rows_are_pruned() {
        let pool = crate::storage::memory().await.unwrap();
        let now = Utc::now();
        let old = (now - TimeDelta::days(40)).to_rfc3339();
        let mid = (now - TimeDelta::days(20)).to_rfc3339();
        let new = now.to_rfc3339();
        for (id, status, started) in [
            ("old-ok", "succeeded", &old),
            ("old-running", "running", &old),
            ("mid-failed", "failed", &mid),
            ("new-ok", "succeeded", &new),
        ] {
            sqlx::query(
                "INSERT INTO cron_runs(id, job_id, status, started_at) VALUES(?, 'j', ?, ?)",
            )
            .bind(id)
            .bind(status)
            .bind(started)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO action_loop_outcomes(item_key, kind, source, line, status, enqueued_at, updated_at) VALUES('inbox:a','k','mail','l','verified',?,?), ('inbox:b','k','mail','l','enqueued',?,?)")
            .bind(&old).bind(&old).bind(&old).bind(&old)
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO action_loop_seen(item_key, seen_at) VALUES('inbox:gone', ?), ('other:x', ?)")
            .bind(&old).bind(&old)
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO session_watch_notices(dedupe_key, target, kind, sent_at) VALUES('d1','t','k',?), ('d2','t','k',?)")
            .bind(&old).bind(&new)
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO chat_outbox(session_key, platform, text, created_at, delivered_at) VALUES('k','slack','sent',?,?), ('k','slack','unsent',?,NULL)")
            .bind(&old).bind(&old).bind(&old)
            .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO hub_log(at, level, message) VALUES(?, 'info', 'old'), (?, 'info', 'new')",
        )
        .bind(&old)
        .bind(&new)
        .execute(&pool)
        .await
        .unwrap();

        let report = prune_older_than(&pool, 30, 14, now).await.unwrap();
        assert_eq!(
            report,
            PruneReport {
                action_loop_seen: 1,
                action_loop_outcomes: 1,
                session_watch_notices: 1,
                chat_outbox: 1,
                hub_log: 1,
                cron_runs: 2,
            }
        );
        let runs: Vec<String> = sqlx::query_scalar("SELECT id FROM cron_runs ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(runs, vec!["new-ok", "old-running"]);
        let unsent: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chat_outbox WHERE delivered_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(unsent, 1, "an undelivered reply is never pruned");
    }
}
