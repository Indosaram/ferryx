//! omo-gateway's usage report (usage.rs): judge calls, fallbacks and caps from `judge_calls`,
//! inbox items collected against those that passed the prefilter. omo's `omon usage --since
//! Nm|Nh|Nd [--json]` is `GET /api/usage?since=..&json=1` on the hub, and the daily digest
//! carries yesterday's line.
use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::{HubError, Result};

#[derive(Debug, Serialize, PartialEq)]
pub struct Report {
    pub judge_calls: i64,
    pub fallback_calls: i64,
    pub fallback_rate: f64,
    pub cap_reached: bool,
    pub inbox_collected: i64,
    pub inbox_passed: i64,
}

fn kst_day(at: DateTime<Utc>) -> String {
    at.with_timezone(&chrono_tz::Asia::Seoul)
        .date_naive()
        .to_string()
}

pub async fn report(
    pool: &SqlitePool,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<Report> {
    let (attempts, fallback_calls, capped): (i64, i64, i64) = sqlx::query_as(
        "SELECT COALESCE(sum(used_fallback=0),0), COALESCE(sum(used_fallback=1),0),
                COALESCE(sum(outcome LIKE 'capped:%'),0)
           FROM judge_calls
          WHERE julianday(created_at) >= julianday(?) AND julianday(created_at) < julianday(?)",
    )
    .bind(since.to_rfc3339())
    .bind(until.to_rfc3339())
    .fetch_one(pool)
    .await?;
    let judge_calls = attempts + fallback_calls;
    let inbox_passed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM inbox_items
          WHERE julianday(collected_at) >= julianday(?) AND julianday(collected_at) < julianday(?)",
    )
    .bind(since.to_rfc3339())
    .bind(until.to_rfc3339())
    .fetch_one(pool)
    .await?;
    let dropped: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(count),0) FROM inbox_filter_stats WHERE day >= ? AND day <= ?",
    )
    .bind(kst_day(since))
    .bind(kst_day(until))
    .fetch_one(pool)
    .await?;
    Ok(Report {
        judge_calls,
        fallback_calls,
        fallback_rate: if judge_calls == 0 {
            0.0
        } else {
            fallback_calls as f64 / judge_calls as f64
        },
        cap_reached: capped > 0,
        inbox_collected: inbox_passed + dropped,
        inbox_passed,
    })
}

impl Report {
    pub fn digest_line(&self) -> String {
        format!(
            "어제: jev {}회(폴백 {}, 상한 도달 {}), 인박스 수집 {}건 중 필터 통과 {}건",
            self.judge_calls,
            self.fallback_calls,
            self.cap_reached,
            self.inbox_collected,
            self.inbox_passed
        )
    }
}

/// omo `usage::render`: `since` is a positive Nm, Nh or Nd.
pub async fn render(
    pool: &SqlitePool,
    since: &str,
    json: bool,
    now: DateTime<Utc>,
) -> Result<String> {
    let seconds = match since.chars().last() {
        Some('m') => 60,
        Some('h') => 3600,
        Some('d') => 86_400,
        _ => return Err(HubError::message("--since requires positive Nm, Nh or Nd")),
    };
    let duration = since[..since.len() - 1]
        .parse::<i64>()
        .ok()
        .filter(|n| *n > 0)
        .and_then(|n| n.checked_mul(seconds))
        .and_then(chrono::Duration::try_seconds)
        .ok_or_else(|| HubError::message("invalid --since duration"))?;
    let start = now
        .checked_sub_signed(duration)
        .ok_or_else(|| HubError::message("--since out of range"))?;
    let report = report(pool, start, now).await?;
    if json {
        serde_json::to_string(&report).map_err(|error| HubError::message(error.to_string()))
    } else {
        Ok(format!(
            "jev: {} calls, {} fallbacks, rate {:.3}, cap reached {}",
            report.judge_calls, report.fallback_calls, report.fallback_rate, report.cap_reached
        ))
    }
}

/// omo `usage::yesterday`: the previous KST day, for the digest.
pub async fn yesterday(pool: &SqlitePool, now: DateTime<Utc>) -> Result<Report> {
    let day = now.with_timezone(&chrono_tz::Asia::Seoul).date_naive();
    let end = day
        .and_hms_opt(0, 0, 0)
        .and_then(|midnight| {
            chrono_tz::Asia::Seoul
                .from_local_datetime(&midnight)
                .single()
        })
        .map(|at| at.with_timezone(&Utc))
        .ok_or_else(|| HubError::message("invalid usage day"))?;
    let mut report = report(pool, end - chrono::Duration::days(1), end).await?;
    let previous = day
        .pred_opt()
        .ok_or_else(|| HubError::message("invalid previous day"))?;
    let dropped: i64 =
        sqlx::query_scalar("SELECT COALESCE(sum(count),0) FROM inbox_filter_stats WHERE day = ?")
            .bind(previous.to_string())
            .fetch_one(pool)
            .await?;
    report.inbox_collected = report.inbox_passed + dropped;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn yesterdays_judge_and_inbox_counts_make_omos_digest_line() {
        let pool = crate::storage::memory().await.unwrap();
        // 2026-10-23 09:00 KST; yesterday is 2026-10-22 KST (2026-10-21T15:00Z..2026-10-22T15:00Z).
        let now = Utc.with_ymd_and_hms(2026, 10, 23, 0, 0, 0).unwrap();
        for (outcome, fallback, at) in [
            ("hold", 0, "2026-10-22T01:00:00+00:00"),
            ("failed", 0, "2026-10-22T02:00:00+00:00"),
            ("fyi", 1, "2026-10-22T02:00:01+00:00"),
            ("capped:wake", 0, "2026-10-22T03:00:00+00:00"),
            ("hold", 0, "2026-10-20T03:00:00+00:00"),
        ] {
            sqlx::query("INSERT INTO judge_calls(id, purpose, kind, ms, outcome, used_fallback, created_at) VALUES(?, 'inbox_class', 'choice', 1, ?, ?, ?)")
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(outcome)
                .bind(fallback)
                .bind(at)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO inbox_items(id, source, source_ref, snippet, received_at, raw_hash, collected_at) VALUES('mail:1', 'mail', '1', 'hi', '2026-10-22T05:00:00+00:00', '1', '2026-10-22T05:00:00+00:00')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO inbox_filter_stats(day, source, rule, count) VALUES('2026-10-22', 'mail', 'mail_otp', 3), ('2026-10-21', 'mail', 'mail_otp', 9)")
            .execute(&pool)
            .await
            .unwrap();
        let report = yesterday(&pool, now).await.unwrap();
        assert_eq!(
            report.digest_line(),
            "어제: jev 4회(폴백 1, 상한 도달 true), 인박스 수집 4건 중 필터 통과 1건"
        );
        let json = render(&pool, "2d", true, now).await.unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["judge_calls"], 4);
        assert_eq!(value["fallback_rate"], 0.25);
        assert_eq!(
            render(&pool, "90m", false, now).await.unwrap(),
            "jev: 0 calls, 0 fallbacks, rate 0.000, cap reached false"
        );
        assert!(render(&pool, "0h", false, now).await.is_err());
        assert!(render(&pool, "5w", false, now).await.is_err());
    }
}
