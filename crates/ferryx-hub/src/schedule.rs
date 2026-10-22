//! Scheduled jobs ported from omo-gateway's cron scheduler.
//!
//! A due job is claimed with a lease. An expired lease is reclaimed when its owner
//! is gone, or after the stale-lease safety net. Retries back off. Delivery targets
//! come from the job payload (`slack:CHANNEL:THREAD`, `discord:ID`, `origin`).
//! The scheduler sleeps until the next `next_run_at`; it does not poll.
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use serde_json::Value;
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

pub const LEASE: TimeDelta = TimeDelta::minutes(30);
pub const STALE_LEASE: TimeDelta = TimeDelta::minutes(120);
pub const ONESHOT_GRACE: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Delivery {
    pub platform: String,
    pub chat_id: String,
    pub thread_id: Option<String>,
}

pub fn should_reclaim<F>(
    owner_pid: Option<u32>,
    lease_expired: bool,
    lease_age: TimeDelta,
    is_alive: F,
) -> bool
where
    F: Fn(u32) -> bool,
{
    if !lease_expired {
        return false;
    }
    if lease_age >= STALE_LEASE {
        return true;
    }
    match owner_pid {
        None => true,
        Some(0) => true,
        Some(pid) => !is_alive(pid),
    }
}

pub fn failure_backoff(consecutive_failures: u32) -> Duration {
    if consecutive_failures == 0 {
        return Duration::ZERO;
    }
    let exponent = (consecutive_failures - 1).min(10);
    let multiplier = 1u64.checked_shl(exponent).unwrap_or(1024);
    Duration::from_secs((10u64.saturating_mul(multiplier)).clamp(10, 3600))
}

pub fn next_run(expression: &str, after: DateTime<Utc>) -> Result<DateTime<Utc>> {
    next_run_tz(expression, after, None)
}

pub fn next_run_tz(
    expression: &str,
    after: DateTime<Utc>,
    timezone: Option<&str>,
) -> Result<DateTime<Utc>> {
    if let Some(timestamp) = expression.strip_prefix("once:") {
        let ts = DateTime::parse_from_rfc3339(timestamp)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|error| {
                HubError::config(format!("invalid one-shot timestamp `{timestamp}`: {error}"))
            })?;
        if ts >= after {
            return Ok(ts);
        }
        if after - ts <= TimeDelta::from_std(ONESHOT_GRACE).unwrap_or(TimeDelta::seconds(120)) {
            return Ok(ts);
        }
        return Err(HubError::config(format!(
            "one-shot timestamp `{timestamp}` is more than 120s in the past and cannot be scheduled"
        )));
    }
    if let Some(interval) = parse_interval(expression)? {
        let delta = TimeDelta::from_std(interval)
            .map_err(|_| HubError::config("cron interval is too large"))?;
        return after
            .checked_add_signed(delta)
            .ok_or_else(|| HubError::config("cron interval is too large"));
    }
    let normalized = normalize_cron_expression(expression);
    let schedule = Schedule::from_str(&normalized).map_err(|error| {
        HubError::config(format!("invalid cron expression `{expression}`: {error}"))
    })?;
    let Some(tz_name) = timezone.map(str::trim).filter(|name| !name.is_empty()) else {
        return schedule.after(&after).next().ok_or_else(|| {
            HubError::config(format!("cron expression `{expression}` has no next run"))
        });
    };
    let tz: Tz = tz_name
        .parse()
        .map_err(|_| HubError::config(format!("invalid timezone `{tz_name}`")))?;
    schedule
        .after(&after.with_timezone(&tz))
        .next()
        .map(|instant| instant.with_timezone(&Utc))
        .ok_or_else(|| HubError::config(format!("cron expression `{expression}` has no next run")))
}

pub fn next_run_after_failure(
    expression: &str,
    now: DateTime<Utc>,
    consecutive_failures: u32,
    timezone: Option<&str>,
) -> Result<Option<DateTime<Utc>>> {
    if expression.starts_with("once:") {
        return Ok(None);
    }
    let scheduled = next_run_tz(expression, now, timezone)?;
    let backoff = TimeDelta::from_std(failure_backoff(consecutive_failures))
        .map_err(|_| HubError::config("backoff duration out of range"))?;
    Ok(Some(std::cmp::max(scheduled, now + backoff)))
}

/// `deliver` may be a string or a list. `slack:C1:1700.1` is a thread. `local` is dropped.
pub fn delivery_targets(payload: &Value) -> Vec<Delivery> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let push = |out: &mut Vec<Delivery>,
                seen: &mut std::collections::BTreeSet<(String, String, String)>,
                item: Delivery| {
        let key = (
            item.platform.clone(),
            item.chat_id.clone(),
            item.thread_id.clone().unwrap_or_default(),
        );
        if seen.insert(key) {
            out.push(item);
        }
    };
    if let Some(chat) = payload.get("channel_id").and_then(|value| {
        value
            .as_u64()
            .map(|n| n.to_string())
            .or_else(|| value.as_str().map(str::to_owned))
    }) {
        let chat = chat.trim().to_owned();
        if !chat.is_empty() {
            push(
                &mut out,
                &mut seen,
                Delivery {
                    platform: "discord".into(),
                    chat_id: chat,
                    thread_id: None,
                },
            );
        }
    }
    let deliver = match payload.get("deliver") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(","),
        _ => String::new(),
    };
    for part in deliver.split(',') {
        let part = part.trim();
        if part.is_empty() || part == "local" {
            continue;
        }
        if let Some(rest) = part
            .get(6..)
            .filter(|_| part[..6].eq_ignore_ascii_case("slack:"))
        {
            let (chat, thread) = match rest.split_once(':') {
                Some((chat, thread)) if !thread.trim().is_empty() => {
                    (chat, Some(thread.trim().to_owned()))
                }
                _ => (rest, None),
            };
            let chat = chat.trim().trim_start_matches('#').to_owned();
            if !chat.is_empty() {
                push(
                    &mut out,
                    &mut seen,
                    Delivery {
                        platform: "slack".into(),
                        chat_id: chat,
                        thread_id: thread,
                    },
                );
            }
            continue;
        }
        if part == "slack" || part == "origin" || part == "all" || part == "discord" {
            if let Some(origin) = payload.get("origin") {
                let platform = origin
                    .get("platform")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let chat = origin
                    .get("chat_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_owned();
                let thread = origin
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if matches!(platform.as_str(), "slack" | "discord") && !chat.is_empty() {
                    push(
                        &mut out,
                        &mut seen,
                        Delivery {
                            platform,
                            chat_id: chat,
                            thread_id: thread,
                        },
                    );
                }
            } else if part == "slack" {
                if let Some(home) = payload.get("home_channel").and_then(Value::as_str) {
                    push(
                        &mut out,
                        &mut seen,
                        Delivery {
                            platform: "slack".into(),
                            chat_id: home.trim().trim_start_matches('#').to_owned(),
                            thread_id: None,
                        },
                    );
                }
            }
            continue;
        }
        if let Some(channel) = part.strip_prefix("discord:") {
            let chat = channel.trim().trim_start_matches('#').to_owned();
            if !chat.is_empty() {
                push(
                    &mut out,
                    &mut seen,
                    Delivery {
                        platform: "discord".into(),
                        chat_id: chat,
                        thread_id: None,
                    },
                );
            }
            continue;
        }
        if part.contains(':') {
            continue;
        }
        let chat = part.trim_start_matches('#').trim().to_owned();
        if !chat.is_empty() {
            push(
                &mut out,
                &mut seen,
                Delivery {
                    platform: "discord".into(),
                    chat_id: chat,
                    thread_id: None,
                },
            );
        }
    }
    out
}

pub fn map_payload(payload_json: &str) -> Result<(Value, String)> {
    let payload: Value = serde_json::from_str(payload_json)
        .map_err(|error| HubError::message(format!("invalid cron payload: {error}")))?;
    let delivery =
        serde_json::to_string(&delivery_targets(&payload)).unwrap_or_else(|_| "[]".into());
    Ok((payload, delivery))
}

/// A failed run, a dropped delivery, or an expired lease becomes an incident kind.
pub fn incident_kind(
    status: &str,
    error: Option<&str>,
    delivery_failed: bool,
) -> Option<(&'static str, String)> {
    if delivery_failed {
        return Some((
            "delivery_failed",
            error.unwrap_or("delivery failed").to_owned(),
        ));
    }
    match status {
        "failed" if error == Some("lease expired before completion") => {
            Some(("stuck", "lease expired before completion".into()))
        }
        "failed" => Some(("failed", error.unwrap_or("cron run failed").to_owned())),
        _ => None,
    }
}

pub fn remediation_due(consecutive_failures: u32, open_incident: bool) -> bool {
    consecutive_failures >= 3 && !open_incident
}

pub fn socket_paths(home: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = home.join(".ferryx").join("hub");
    (dir.join("cron.sock"), dir.join("cron-run-now.sock"))
}

pub fn parse_run_now(line: &str) -> Result<String> {
    let value: Value = serde_json::from_str(line)
        .map_err(|error| HubError::message(format!("cron-run-now: {error}")))?;
    value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| HubError::message("cron-run-now needs an id"))
}

pub async fn claim_due(
    pool: &SqlitePool,
    now: DateTime<Utc>,
    owner_pid: u32,
) -> Result<Vec<String>> {
    let now_text = now.to_rfc3339();
    let lease = (now + LEASE).to_rfc3339();
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM scheduled_jobs WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ? ORDER BY next_run_at",
    )
    .bind(&now_text)
    .fetch_all(pool)
    .await?;
    let mut claimed = Vec::new();
    for (id,) in rows {
        let run_id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO cron_runs(id, job_id, status, owner_pid, lease_until, started_at) VALUES(?,?, 'running', ?, ?, ?)")
            .bind(&run_id)
            .bind(&id)
            .bind(owner_pid)
            .bind(&lease)
            .bind(&now_text)
            .execute(pool)
            .await?;
        sqlx::query("UPDATE scheduled_jobs SET next_run_at = NULL, updated_at = ? WHERE id = ?")
            .bind(&now_text)
            .bind(&id)
            .execute(pool)
            .await?;
        claimed.push(id);
    }
    Ok(claimed)
}

pub async fn finish_run(
    pool: &SqlitePool,
    job_id: &str,
    ok: bool,
    error: Option<&str>,
    output: &str,
) -> Result<()> {
    let now = Utc::now();
    let status = if ok { "succeeded" } else { "failed" };
    sqlx::query(
        "UPDATE cron_runs SET status = ?, finished_at = ?, error = ?, output = ? WHERE job_id = ? AND status = 'running'",
    )
    .bind(status)
    .bind(now.to_rfc3339())
    .bind(error)
    .bind(output)
    .bind(job_id)
    .execute(pool)
    .await?;
    let row: Option<(String, i64, String)> = sqlx::query_as(
        "SELECT expression, consecutive_failures, payload_json FROM scheduled_jobs WHERE id = ?",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await?;
    let Some((expression, failures, payload_json)) = row else {
        return Ok(());
    };
    let payload: Value = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
    let tz = payload
        .get("timezone")
        .or_else(|| payload.get("tz"))
        .and_then(Value::as_str);
    let (next, consecutive) = if ok {
        (next_run_tz(&expression, now, tz).ok(), 0_i64)
    } else {
        let consecutive = failures.saturating_add(1);
        (
            next_run_after_failure(&expression, now, consecutive as u32, tz)?,
            consecutive,
        )
    };
    sqlx::query("UPDATE scheduled_jobs SET next_run_at = ?, consecutive_failures = ?, updated_at = ? WHERE id = ?")
        .bind(next.map(|t| t.to_rfc3339()))
        .bind(consecutive)
        .bind(now_rfc3339())
        .bind(job_id)
        .execute(pool)
        .await?;
    if let Some((kind, detail)) = incident_kind(status, error, false) {
        if remediation_due(consecutive as u32, false) || !ok {
            sqlx::query("INSERT INTO cron_incidents(id, job_id, kind, detail, status, created_at) VALUES(?,?,?,?, 'open', ?)")
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(job_id)
                .bind(kind)
                .bind(detail)
                .bind(now.to_rfc3339())
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

pub fn report(jobs: &[(String, String, bool)]) -> String {
    let mut lines = vec!["# cron".to_owned()];
    for (id, expression, enabled) in jobs {
        lines.push(format!("- {id} {expression} enabled={enabled}"));
    }
    lines.join("\n")
}

fn normalize_cron_expression(expression: &str) -> String {
    let parts: Vec<&str> = expression.split_whitespace().collect();
    if parts.len() == 5 {
        let day = unix_day_of_week_to_crate(parts[4]);
        format!(
            "0 {} {} {} {} {day}",
            parts[0], parts[1], parts[2], parts[3]
        )
    } else {
        expression.to_string()
    }
}

fn unix_day_of_week_to_crate(field: &str) -> String {
    fn unix_days(item: &str) -> Option<Vec<u32>> {
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => (range, step.parse::<u32>().ok().filter(|step| *step > 0)?),
            None => (item, 1),
        };
        let (start, end) = match range.split_once('-') {
            Some((start, end)) => (start.parse::<u32>().ok()?, end.parse::<u32>().ok()?),
            None if item.contains('/') => (range.parse::<u32>().ok()?, 6),
            None => {
                let day = range.parse::<u32>().ok()?;
                (day, day)
            }
        };
        (start <= end && end <= 7).then(|| (start..=end).step_by(step as usize).collect())
    }
    field
        .split(',')
        .map(|item| match unix_days(item) {
            Some(days) => {
                let mut crate_days: Vec<u32> = days
                    .into_iter()
                    .map(|day| if day == 7 { 1 } else { day + 1 })
                    .collect();
                crate_days.sort_unstable();
                crate_days.dedup();
                crate_days
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            }
            None => item.to_string(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_interval(expression: &str) -> Result<Option<Duration>> {
    let value = expression
        .strip_prefix("interval:")
        .or_else(|| expression.strip_prefix("@every "))
        .or_else(|| expression.strip_prefix("every "));
    let Some(value) = value.map(str::trim) else {
        return Ok(None);
    };
    let split = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let amount: u64 = number
        .parse()
        .map_err(|_| HubError::config(format!("invalid interval expression `{expression}`")))?;
    if amount == 0 {
        return Err(HubError::config("cron interval must be greater than zero"));
    }
    let duration = match unit.trim() {
        "ms" => Duration::from_millis(amount),
        "s" | "sec" | "secs" => Duration::from_secs(amount),
        "m" | "min" | "mins" => Duration::from_secs(amount.saturating_mul(60)),
        "h" | "hr" | "hrs" => Duration::from_secs(amount.saturating_mul(3_600)),
        "d" | "day" | "days" => Duration::from_secs(amount.saturating_mul(86_400)),
        _ => {
            return Err(HubError::config(format!(
                "invalid interval unit in `{expression}`"
            )))
        }
    };
    Ok(Some(duration))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leases_retries_and_slack_threads_follow_the_gateway() {
        assert!(!should_reclaim(
            Some(4),
            false,
            TimeDelta::minutes(1),
            |_| true
        ));
        assert!(should_reclaim(Some(4), true, TimeDelta::minutes(1), |_| {
            false
        }));
        assert!(!should_reclaim(
            Some(4),
            true,
            TimeDelta::minutes(1),
            |_| true
        ));
        assert!(should_reclaim(None, true, TimeDelta::minutes(1), |_| true));
        assert!(should_reclaim(
            Some(4),
            true,
            TimeDelta::minutes(121),
            |_| true
        ));
        assert_eq!(failure_backoff(1), Duration::from_secs(10));
        assert_eq!(failure_backoff(2), Duration::from_secs(20));
        assert_eq!(failure_backoff(20), Duration::from_secs(3600));
        let targets = delivery_targets(
            &serde_json::json!({"deliver": "slack:C0WORK:1700.0001, discord:42, local"}),
        );
        assert_eq!(targets[0].platform, "slack");
        assert_eq!(targets[0].thread_id.as_deref(), Some("1700.0001"));
        assert_eq!(
            targets[1],
            Delivery {
                platform: "discord".into(),
                chat_id: "42".into(),
                thread_id: None
            }
        );
        let (_, delivery) = map_payload(r#"{"deliver":"slack:C1"}"#).unwrap();
        assert!(delivery.contains("C1"));
        assert_eq!(
            incident_kind("failed", Some("lease expired before completion"), false)
                .unwrap()
                .0,
            "stuck"
        );
        assert!(remediation_due(3, false));
        assert!(!remediation_due(3, true));
        assert_eq!(parse_run_now(r#"{"id":"job-1"}"#).unwrap(), "job-1");
    }

    #[test]
    fn five_field_cron_and_intervals_schedule() {
        let saturday = DateTime::parse_from_rfc3339("2026-10-10T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let next = next_run("0 9 * * 1", saturday).unwrap();
        assert!(next > saturday);
        let interval = next_run("every 15m", saturday).unwrap();
        assert_eq!(interval, saturday + TimeDelta::minutes(15));
        let report = report(&[("daily".into(), "0 9 * * *".into(), true)]);
        assert!(report.contains("daily"));
    }

    #[tokio::test]
    async fn a_due_job_is_claimed_once_and_a_failure_opens_an_incident() {
        let pool = crate::storage::memory().await.unwrap();
        let due = (Utc::now() - TimeDelta::minutes(1)).to_rfc3339();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, next_run_at, delivery_json) VALUES('job','every 10m','{\"deliver\":\"slack:C1\"}',1,?, '[]')")
            .bind(&due)
            .execute(&pool)
            .await
            .unwrap();
        let claimed = claim_due(&pool, Utc::now(), 9).await.unwrap();
        assert_eq!(claimed, vec!["job".to_string()]);
        assert!(claim_due(&pool, Utc::now(), 9).await.unwrap().is_empty());
        finish_run(&pool, "job", false, Some("boom"), "")
            .await
            .unwrap();
        let incidents: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM cron_incidents WHERE job_id = 'job'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(incidents, 1);
        let failures: i64 =
            sqlx::query_scalar("SELECT consecutive_failures FROM scheduled_jobs WHERE id = 'job'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(failures, 1);
    }
}
