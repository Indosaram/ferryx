//! Scheduled jobs ported from omo-gateway's cron scheduler.
//!
//! A due job is claimed with a lease. An expired lease is reclaimed when its owner
//! is gone, or after the stale-lease safety net. Retries back off. Delivery targets
//! come from the job payload (`slack:CHANNEL:THREAD`, `discord:ID`, `origin`).
//! The scheduler sleeps until the next `next_run_at`; it does not poll.
use crate::storage::WriteTx;
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

/// The one owner inbox item a failing cron job holds, however often it fails.
pub(crate) fn cron_owner_item_id(job_id: &str) -> String {
    format!("cron:job:{job_id}")
}

/// The latest failure of a job, as its owner inbox item shows it.
pub(crate) struct CronOwnerItem<'a> {
    pub job_id: &'a str,

    pub title: &'a str,
    /// Snippet and proposed action.
    pub action: &'a str,
    pub reason: &'a str,
    pub lane_key: Option<&'a str>,
    pub at: &'a str,
}

/// Upserts the job's single owner item to the latest failure. An item that was not open
/// reopens with a fresh `decided_at` and loses its alert obligation, so the owner is told
/// once; an open item keeps both, so repeat failures do not re-alert.
pub(crate) async fn upsert_cron_owner_item(
    conn: &mut sqlx::SqliteConnection,
    item: &CronOwnerItem<'_>,
) -> Result<()> {
    let id = cron_owner_item_id(item.job_id);
    sqlx::query(
        "DELETE FROM delivery_obligations WHERE id = ?
           AND NOT EXISTS (SELECT 1 FROM inbox_decisions WHERE item_id = ? AND status = 'open')",
    )
    .bind(format!("{}{id}", crate::owner_alert::OBLIGATION_PREFIX))
    .bind(&id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO inbox_items(id, source, source_ref, author, title, snippet, received_at, raw_hash, collected_at) VALUES(?, 'cron', ?, ?, ?, ?, ?, 'cron', ?)
         ON CONFLICT(id) DO UPDATE SET title = excluded.title, snippet = excluded.snippet, received_at = excluded.received_at",
    )
    .bind(&id)
    .bind(&id)
    .bind(item.job_id)
    .bind(item.title)
    .bind(item.action)
    .bind(item.at)
    .bind(item.at)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO inbox_decisions(item_id, class, reason, proposed_action, lane_key, status, decided_at) VALUES(?, 'owner', ?, ?, ?, 'open', ?)
         ON CONFLICT(item_id) DO UPDATE SET class = 'owner', reason = excluded.reason, proposed_action = excluded.proposed_action,
           lane_key = COALESCE(excluded.lane_key, inbox_decisions.lane_key),
           decided_at = CASE WHEN inbox_decisions.status = 'open' THEN inbox_decisions.decided_at ELSE excluded.decided_at END,
           status = 'open'",
    )
    .bind(&id)
    .bind(item.reason)
    .bind(item.action)
    .bind(item.lane_key)
    .bind(item.at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// A clean run settles the job's owner item and any legacy per-incident items it left.
pub(crate) async fn close_cron_owner_items(pool: &SqlitePool, job_id: &str) -> Result<()> {
    sqlx::query(
        "UPDATE inbox_decisions SET status = 'done'
          WHERE status = 'open' AND class = 'owner'
            AND (item_id = ?
                 OR (item_id LIKE 'cron:%' AND item_id IN (SELECT id FROM inbox_items WHERE source = 'cron' AND author = ?)))",
    )
    .bind(cron_owner_item_id(job_id))
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// The newest legacy owner item of a job, which its collapsed item inherits.
#[derive(sqlx::FromRow)]
struct LegacyOwnerRow {
    title: Option<String>,
    snippet: String,
    received_at: String,
    reason: String,
    proposed_action: Option<String>,
    lane_key: Option<String>,
    decided_at: String,
}

/// Collapses the open legacy per-incident/per-problem cron owner items (`cron:<id>`) into one
/// `cron:job:<job_id>` item per job, marked already alerted, and dismisses the legacy ones.
/// Idempotent; returns how many legacy decisions were dismissed.
pub async fn reconcile_cron_owner_items(pool: &SqlitePool) -> Result<u64> {
    const LEGACY: &str = "d.status = 'open' AND d.class = 'owner' AND i.source = 'cron'
        AND d.item_id LIKE 'cron:%' AND d.item_id NOT LIKE 'cron:job:%' AND d.item_id NOT LIKE 'cron:env:%'";
    let mut tx = pool.begin_write().await?;
    // Remediation problem items (`cron:<problem id>`) were written without an author; take the
    // job from the problem row so they collapse with the rest of the job.
    sqlx::query(
        "UPDATE inbox_items SET author = (SELECT p.job_id FROM cron_problems p WHERE p.id = inbox_items.source_ref)
          WHERE source = 'cron' AND (author IS NULL OR author = '') AND id NOT LIKE 'cron:job:%'
            AND EXISTS (SELECT 1 FROM cron_problems p WHERE p.id = inbox_items.source_ref)",
    )
    .execute(&mut *tx)
    .await?;
    let jobs: Vec<String> = sqlx::query_scalar(&format!(
        "SELECT DISTINCT i.author FROM inbox_decisions d JOIN inbox_items i ON i.id = d.item_id
          WHERE {LEGACY} AND i.author IS NOT NULL AND i.author != ''"
    ))
    .fetch_all(&mut *tx)
    .await?;
    let mut dismissed = 0_u64;
    for job_id in jobs {
        let id = cron_owner_item_id(&job_id);
        let already_open: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM inbox_decisions WHERE item_id = ? AND status = 'open'",
        )
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;
        if already_open == 0 {
            let newest: LegacyOwnerRow = sqlx::query_as(&format!(
                "SELECT i.title, i.snippet, i.received_at, d.reason, d.proposed_action, d.lane_key, d.decided_at
                   FROM inbox_decisions d JOIN inbox_items i ON i.id = d.item_id
                  WHERE {LEGACY} AND i.author = ?
                  ORDER BY julianday(d.decided_at) DESC, d.item_id DESC LIMIT 1"
            ))
            .bind(&job_id)
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO inbox_items(id, source, source_ref, author, title, snippet, received_at, raw_hash, collected_at) VALUES(?, 'cron', ?, ?, ?, ?, ?, 'cron', ?)
                 ON CONFLICT(id) DO UPDATE SET title = excluded.title, snippet = excluded.snippet, received_at = excluded.received_at",
            )
            .bind(&id)
            .bind(&id)
            .bind(&job_id)
            .bind(newest.title)
            .bind(newest.snippet)
            .bind(newest.received_at)
            .bind(now_rfc3339())
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO inbox_decisions(item_id, class, reason, proposed_action, lane_key, status, decided_at) VALUES(?, 'owner', ?, ?, ?, 'open', ?)
                 ON CONFLICT(item_id) DO UPDATE SET class = 'owner', reason = excluded.reason, proposed_action = excluded.proposed_action,
                   lane_key = excluded.lane_key, status = 'open', decided_at = excluded.decided_at",
            )
            .bind(&id)
            .bind(newest.reason)
            .bind(newest.proposed_action)
            .bind(newest.lane_key)
            .bind(newest.decided_at)
            .execute(&mut *tx)
            .await?;
            // The backlog was alerted item by item already; the collapsed item is not news.
            sqlx::query(
                "INSERT INTO delivery_obligations(id, state, body, updated_at) VALUES(?, 'delivered', 'collapsed legacy cron owner items', ?)
                 ON CONFLICT(id) DO UPDATE SET state = 'delivered', updated_at = excluded.updated_at",
            )
            .bind(format!("{}{id}", crate::owner_alert::OBLIGATION_PREFIX))
            .bind(now_rfc3339())
            .execute(&mut *tx)
            .await?;
        }
        let done = sqlx::query(&format!(
            "UPDATE inbox_decisions SET status = 'dismissed'
              WHERE item_id IN (SELECT d.item_id FROM inbox_decisions d JOIN inbox_items i ON i.id = d.item_id
                                 WHERE {LEGACY} AND i.author = ?)"
        ))
        .bind(&job_id)
        .execute(&mut *tx)
        .await?;
        dismissed += done.rows_affected();
    }
    tx.commit().await?;
    Ok(dismissed)
}

/// omo-gateway's `needs_owner`: a cron failure refreshes the job's one open owner item
/// (`cron:job:<job_id>`), so the owner alert sweep posts it once and the digest lists it. A
/// host/environment fault refreshes the fault's one item instead, naming every job it fails.
async fn escalate_incident(
    pool: &SqlitePool,
    job_id: &str,
    kind: &str,
    detail: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    let at = now.to_rfc3339();
    if let Some(fault) = crate::cron_exec::EnvironmentFault::parse(detail) {
        let mut tx = pool.begin_write().await?;
        crate::cron_remediate::upsert_environment_item(&mut tx, &fault, &at).await?;
        tx.commit().await?;
        return Ok(());
    }
    let action = crate::judge::scrub_for_external(&crate::cron_remediate::strip_ansi(detail));
    // The digest groups cron items by the text before ": 확인 필요" / ": needs you".
    let title = crate::locale::text(
        &format!("{job_id}: 확인 필요 - {kind}: {action}"),
        &format!("{job_id}: needs you - {kind}: {action}"),
    );
    let mut tx = pool.begin_write().await?;
    upsert_cron_owner_item(
        &mut tx,
        &CronOwnerItem {
            job_id,
            title: &title,
            action: &action,
            reason: kind,
            lane_key: None,
            at: &at,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub fn remediation_due(consecutive_failures: u32, open_incident: bool) -> bool {
    consecutive_failures >= 3 && !open_incident
}

pub fn socket_paths(home: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = home.join(".ferryx").join("hub");
    (dir.join("cron.sock"), dir.join("cron-run-now.sock"))
}

/// The sockets the hub serves: `cron.sock` and `cron-run-now.sock` under `~/.ferryx/hub`.
pub fn native_socket_paths(home: &std::path::Path) -> Vec<std::path::PathBuf> {
    let (cron, run_now) = socket_paths(home);
    vec![cron, run_now]
}

pub const PUBLISHING_JOBS: [&str; 3] = [
    "omon-playstore-daily-publish",
    "omon-vpn-mall-daily-publish-v2",
    "omon-content-intel-publish-ready",
];

pub fn publishing(id: &str, payload: &Value) -> bool {
    PUBLISHING_JOBS.contains(&id)
        || payload.get("external_effect").and_then(Value::as_bool) == Some(true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeRequest {
    Run {
        id: String,
        force: bool,
    },
    Approve {
        problem_id: i64,
        owner_token: String,
    },
}

/// One JSON line. A job id is a JSON string. An approval is an object.
/// `{"id":"..."}` is accepted for the ferryx helper.
pub fn parse_native_request(line: &str) -> Result<NativeRequest> {
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| HubError::message(format!("cron socket: {error}")))?;
    if let Some(id) = value.as_str() {
        if id.is_empty() {
            return Err(HubError::message("cron socket job id is empty"));
        }
        return Ok(NativeRequest::Run {
            id: id.to_owned(),
            force: false,
        });
    }
    if let Some(problem_id) = value.get("approve_problem").and_then(Value::as_i64) {
        let owner_token = value
            .get("owner_token")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if owner_token.is_empty() {
            return Err(HubError::message("cron approval needs an owner token"));
        }
        return Ok(NativeRequest::Approve {
            problem_id,
            owner_token,
        });
    }
    if let Some(id) = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    {
        let force = value.get("force").and_then(Value::as_bool) == Some(true);
        return Ok(NativeRequest::Run {
            id: id.to_owned(),
            force,
        });
    }
    Err(HubError::message(
        "cron socket expected a job id string or an approval",
    ))
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct RunReceipt {
    pub job_id: String,
    pub run_id: String,
    pub status: String,
    pub output: Option<String>,
    pub last_delivery_error: Option<String>,
    pub error: Option<String>,
}

pub fn script_from_payload(payload: &Value) -> Option<String> {
    ["command", "script", "shell"].iter().find_map(|key| {
        payload
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    })
}

pub async fn health_lines(pool: &SqlitePool) -> Result<Vec<String>> {
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, expression, next_run_at FROM scheduled_jobs WHERE enabled = 1 ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, expression, next)| {
            format!(
                "• `{id}`: `{expression}` (Next: `{}`)",
                next.unwrap_or_else(|| "paused".into())
            )
        })
        .collect())
}

/// Runs one scheduled job and writes a `cron_runs` row. A publishing job is refused
/// until `approve` has matched its owner token. A disabled job is refused unless
/// `force` is set. A successful run is delivered the same way a scheduled run is.
pub async fn run_native_job(
    pool: &SqlitePool,
    job_id: &str,
    approved: bool,
    force: bool,
) -> Result<RunReceipt> {
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT payload_json, enabled FROM scheduled_jobs WHERE id = ?")
            .bind(job_id)
            .fetch_optional(pool)
            .await?;
    let Some((payload_json, enabled)) = row else {
        return Ok(RunReceipt {
            job_id: job_id.to_owned(),
            run_id: String::new(),
            status: "failed".into(),
            output: None,
            last_delivery_error: None,
            error: Some(format!("cron job {job_id} not found")),
        });
    };
    let payload: Value = serde_json::from_str(&payload_json)
        .map_err(|error| HubError::message(format!("invalid cron payload: {error}")))?;
    if enabled != 1 && !force {
        return Ok(RunReceipt {
            job_id: job_id.to_owned(),
            run_id: String::new(),
            status: "failed".into(),
            output: None,
            last_delivery_error: None,
            error: Some(format!(
                "cron job {job_id} is disabled; pass force to run it"
            )),
        });
    }
    if publishing(job_id, &payload) && !approved {
        return Ok(RunReceipt {
            job_id: job_id.to_owned(),
            run_id: String::new(),
            status: "failed".into(),
            output: None,
            last_delivery_error: None,
            error: Some("publishing rerun requires owner approval through remediation".into()),
        });
    }
    let run_id = uuid::Uuid::new_v4().to_string();
    let started = now_rfc3339();
    sqlx::query(
        "INSERT INTO cron_runs(id, job_id, status, started_at, output) VALUES(?,?, 'running', ?, '')",
    )
    .bind(&run_id)
    .bind(job_id)
    .bind(&started)
    .execute(pool)
    .await?;
    let model = cron_model();
    let skills = skills_root_from_env();
    let outcome = crate::cron_exec::with_run(&run_id,
        crate::cron_exec::run_payload(pool, job_id, &payload, &model, skills.as_deref())).await;
    if let Ok(executed) = &outcome {
        if let Some(hash) = &executed.monitor_hash {
            let _ = crate::cron_exec::commit_monitor(pool, job_id, hash).await;
        }
    }
    let (status, output, error, deliver) = match outcome {
        Ok(executed) => ("succeeded", Some(executed.output), None, executed.deliver),
        Err(error) => ("failed", None, Some(error.to_string()), false),
    };
    sqlx::query(
        "UPDATE cron_runs SET status = ?, finished_at = ?, output = ?, error = ? WHERE id = ?",
    )
    .bind(status)
    .bind(now_rfc3339())
    .bind(output.clone().unwrap_or_default())
    .bind(error.clone())
    .bind(&run_id)
    .execute(pool)
    .await?;
    let last_delivery_error = if status == "succeeded" && deliver {
        match output.as_deref() {
            Some(text) if should_deliver(text) => {
                deliver_run_output(pool, job_id, &payload, text).await
            }
            _ => None,
        }
    } else {
        None
    };
    // A run-now or remediation rerun that succeeded and delivered settles the job like a
    // scheduled clean run does.
    if status == "succeeded"
        && last_delivery_error.is_none()
        && expected_output_miss(&payload, output.as_deref().unwrap_or("")).is_none()
    {
        resolve_after_success(pool, job_id, &now_rfc3339()).await?;
    }
    Ok(RunReceipt {
        job_id: job_id.to_owned(),
        run_id,
        status: status.into(),
        output,
        last_delivery_error,
        error,
    })
}

async fn run_shell(script: &str) -> Result<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        crate::child_env::command("sh")
            .arg("-c")
            .arg(script)
            .output(),
    )
    .await
    .map_err(|_| HubError::message("cron script timed out"))?
    .map_err(|error| HubError::message(error.to_string()))?;
    if !output.status.success() {
        return Err(HubError::message(format!(
            "cron script failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub async fn approve_problem(
    pool: &SqlitePool,
    problem_id: i64,
    token: &str,
) -> Result<RunReceipt> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT job_id, owner_token FROM cron_problems WHERE id = ?")
            .bind(problem_id.to_string())
            .fetch_optional(pool)
            .await?;
    let Some((job_id, stored)) = row else {
        return Ok(failed_receipt(
            "",
            &format!("problem {problem_id} not found"),
        ));
    };
    // A problem the remediator escalated carries only its token's hash. The remediator spends
    // the token, grants one more verified rerun, and settles the problem.
    if crate::cron_remediate::installed().is_some() && stored.is_none() {
        return match crate::cron_remediate::approve_rerun(problem_id, token).await {
            Ok(receipt) => Ok(receipt),
            Err(error) => Ok(failed_receipt(&job_id, &error.to_string())),
        };
    }
    if stored.as_deref() != Some(token) {
        return Ok(failed_receipt(&job_id, "owner token does not match"));
    }
    sqlx::query("UPDATE cron_problems SET status = 'approved' WHERE id = ?")
        .bind(problem_id.to_string())
        .execute(pool)
        .await?;
    // Approval does not override `enabled`. A disabled job stays unrun until
    // someone passes force on the cron socket.
    run_native_job(pool, &job_id, true, false).await
}

fn failed_receipt(job_id: &str, error: &str) -> RunReceipt {
    RunReceipt {
        job_id: job_id.to_owned(),
        run_id: String::new(),
        status: "failed".into(),
        output: None,
        last_delivery_error: None,
        error: Some(error.to_owned()),
    }
}

pub async fn handle_native_line(pool: &SqlitePool, line: &str) -> String {
    let response = match parse_native_request(line) {
        Ok(NativeRequest::Run { id, force }) => run_native_job(pool, &id, false, force).await,
        Ok(NativeRequest::Approve {
            problem_id,
            owner_token,
        }) => approve_problem(pool, problem_id, &owner_token).await,
        Err(error) => Ok(failed_receipt("", &error.to_string())),
    };
    match response {
        Ok(receipt) => serde_json::to_string(&receipt).unwrap_or_else(|_| {
            "{\"status\":\"failed\",\"error\":\"receipt encoding failed\"}".into()
        }),
        Err(error) => serde_json::json!({"status":"failed","error":error.to_string()}).to_string(),
    }
}

/// Binds one socket. A live peer means another gateway owns it and this process leaves it alone.
#[cfg(unix)]
pub async fn bind_native_socket(path: &std::path::Path) -> Result<tokio::net::UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| HubError::message(error.to_string()))?;
    }
    if tokio::fs::try_exists(path).await.unwrap_or(false) {
        if tokio::net::UnixStream::connect(path).await.is_ok() {
            return Err(HubError::message(format!(
                "cron socket {} is already owned",
                path.display()
            )));
        }
        let _ = tokio::fs::remove_file(path).await;
    }
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let compact = uuid::Uuid::new_v4().to_string().replace('-', "");
    let staging = parent.join(format!(".cs{}", &compact[..8]));
    tokio::fs::create_dir(&staging)
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    let staged = staging.join("s");
    let bound = async {
        tokio::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700)).await?;
        let listener = tokio::net::UnixListener::bind(&staged)?;
        tokio::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600)).await?;
        tokio::fs::rename(&staged, path).await?;
        std::io::Result::Ok(listener)
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    bound.map_err(|error| HubError::message(error.to_string()))
}

#[cfg(unix)]
pub async fn serve_native_socket(
    path: std::path::PathBuf,
    pool: SqlitePool,
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    let listener = bind_native_socket(&path).await?;
    loop {
        tokio::select! {
            changed = stop.changed() => {
                // A dropped sender is a stop. `changed()` is ready on every poll after the
                // sender is gone, so waiting for `true` here would spin a worker thread.
                if changed.is_err() || *stop.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let stream = match accepted {
                    Ok((stream, _)) => stream,
                    Err(error) => {
                        tracing::warn!(%error, "cron socket accept failed");
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        continue;
                    }
                };
                let pool = pool.clone();
                tokio::spawn(async move {
                    let mut stream = tokio::io::BufReader::new(stream);
                    let mut line = String::new();
                    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
                    let read = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        stream.read_line(&mut line),
                    )
                    .await;
                    if !matches!(read, Ok(Ok(_))) {
                        return;
                    }
                    let response = handle_native_line(&pool, &line).await;
                    let _ = stream.get_mut().write_all(format!("{response}\n").as_bytes()).await;
                });
            }
        }
    }
    let _ = tokio::fs::remove_file(&path).await;
    Ok(())
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedJob {
    pub id: String,
    /// `next_run_at` captured before the claim clears it. That instant is `last_occurrence`.
    pub occurrence: Option<String>,
    /// The `cron_runs` row this claim opened. The run is finished by this id only.
    pub run_id: String,
}

pub const LEASE_EXPIRED_ERROR: &str = "lease expired before completion";
/// How often a running batch pushes its leases forward.
pub const LEASE_RENEW_EVERY: Duration = Duration::from_secs(60);

/// Whether a process with this pid currently exists. A direct, non-blocking OS query (no
/// child process, no shell), safe to call on the async runtime. pid 0 is never live.
/// An answer the OS cannot give errs toward live: a lease holder is never abandoned on a guess.
///
/// Unix: signal 0 tests existence without delivering a signal. EPERM means the pid exists
/// but belongs to another user, which is still a live owner; any other failure is gone.
///
/// Windows: the process object is opened for `SYNCHRONIZE` and waited on with a zero timeout:
/// live while unsignaled, gone once signaled (it exited; the object outlives the exit while any
/// handle is open). `OpenProcess` failing with `ERROR_INVALID_PARAMETER` means no such pid;
/// every other failure (`ERROR_ACCESS_DENIED`: another account's process) is reported live.
///
/// Other targets have no query and report every pid gone.
pub fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        // SAFETY: kill with signal 0 performs only the existence and permission check.
        let result = unsafe { sys::kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(1)
    }
    #[cfg(windows)]
    {
        const SYNCHRONIZE: u32 = 0x0010_0000;
        const ERROR_INVALID_PARAMETER: u32 = 87;
        const WAIT_OBJECT_0: u32 = 0;
        // SAFETY: OpenProcess returns a handle or null; it is closed exactly once below and
        // never escapes this block.
        let handle = unsafe { sys::OpenProcess(SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            // SAFETY: reads this thread's last error right after the failed call.
            return unsafe { sys::GetLastError() } != ERROR_INVALID_PARAMETER;
        }
        // SAFETY: a zero timeout never blocks; the handle is valid until CloseHandle.
        let signaled = unsafe { sys::WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0;
        // SAFETY: closes the handle opened above.
        unsafe { sys::CloseHandle(handle) };
        !signaled
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        pub fn kill(pid: i32, sig: i32) -> i32;
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        pub fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        pub fn CloseHandle(handle: *mut c_void) -> i32;
        pub fn GetLastError() -> u32;
    }
}

fn parse_instant(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

/// Fails every `running` row whose lease expired and whose owner `should_reclaim` gives up
/// on (dead owner, or past `STALE_LEASE`). A daemon that died or handed over mid-batch left
/// those rows behind with the job's `next_run_at` cleared, so the job never ran again.
/// The job gets its next FUTURE occurrence: the missed one is never replayed, because a
/// script may already have had external effects. A disabled job stays disabled and a job
/// someone rescheduled keeps its newer `next_run_at`. Returns the number of recovered runs.
pub async fn recover_stale_runs<F>(
    pool: &SqlitePool,
    now: DateTime<Utc>,
    is_alive: F,
) -> Result<u64>
where
    F: Fn(u32) -> bool,
{
    let rows: Vec<(String, String, Option<i64>, String)> = sqlx::query_as(
        "SELECT id, job_id, owner_pid, lease_until FROM cron_runs WHERE status = 'running' AND lease_until IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    let stale: Vec<_> = rows
        .into_iter()
        .filter(|(_, _, owner_pid, lease_until)| {
            let Some(lease) = parse_instant(lease_until) else {
                return false;
            };
            let owner = owner_pid.and_then(|pid| u32::try_from(pid).ok());
            should_reclaim(owner, lease <= now, now - lease, &is_alive)
        })
        .collect();
    if stale.is_empty() {
        return Ok(0);
    }
    let now_text = now.to_rfc3339();
    let mut recovered = 0;
    let mut tx = pool.begin().await?;
    for (run_id, job_id, _, lease_until) in stale {
        // Only a row still running on the lease we judged moves; a renewal or a finish that
        // landed in between wins.
        let changed = sqlx::query(
            "UPDATE cron_runs SET status = 'failed', finished_at = ?, error = ? WHERE id = ? AND status = 'running' AND lease_until = ?",
        )
        .bind(&now_text)
        .bind(LEASE_EXPIRED_ERROR)
        .bind(&run_id)
        .bind(&lease_until)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed == 0 {
            continue;
        }
        recovered += 1;
        tracing::warn!(%job_id, %run_id, "cron run lease expired; marked failed and rescheduled");
        let job: Option<(String, String)> = sqlx::query_as(
            "SELECT expression, payload_json FROM scheduled_jobs WHERE id = ? AND enabled = 1 AND next_run_at IS NULL",
        )
        .bind(&job_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((expression, payload_json)) = job else {
            continue;
        };
        let payload: Value = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
        let expression = job_expression(&expression, &payload);
        let next = match next_run_tz(&expression, now, payload_timezone(&payload)) {
            Ok(next) if next > now => next,
            Ok(_) | Err(_) => {
                tracing::warn!(%job_id, "recovered cron job has no future occurrence; left unscheduled");
                continue;
            }
        };
        sqlx::query(
            "UPDATE scheduled_jobs SET next_run_at = ?, updated_at = ? WHERE id = ? AND enabled = 1 AND next_run_at IS NULL",
        )
        .bind(next.to_rfc3339())
        .bind(&now_text)
        .bind(&job_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(recovered)
}

/// Pushes the lease of every still-running run in `run_ids` to `now + LEASE`.
pub async fn renew_leases(
    pool: &SqlitePool,
    run_ids: &[String],
    now: DateTime<Utc>,
) -> Result<u64> {
    let lease = (now + LEASE).to_rfc3339();
    let mut renewed = 0;
    for run_id in run_ids {
        renewed +=
            sqlx::query("UPDATE cron_runs SET lease_until = ? WHERE id = ? AND status = 'running'")
                .bind(&lease)
                .bind(run_id)
                .execute(pool)
                .await?
                .rows_affected();
    }
    Ok(renewed)
}

/// Claims every due job for `owner_pid`. A job is claimed by one conditional UPDATE that
/// clears `next_run_at` only while it still holds the occurrence this scheduler read, so two
/// schedulers on one database (a draining predecessor next to its successor) never both run
/// the same occurrence: the loser's UPDATE changes no row and it opens no run.
/// Stale runs a dead or handed-over owner left behind are recovered first.
pub async fn claim_due(
    pool: &SqlitePool,
    now: DateTime<Utc>,
    owner_pid: u32,
) -> Result<Vec<ClaimedJob>> {
    recover_stale_runs(pool, now, process_alive).await?;
    let now_text = now.to_rfc3339();
    let lease = (now + LEASE).to_rfc3339();
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT id, next_run_at FROM scheduled_jobs WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ? ORDER BY next_run_at",
    )
    .bind(&now_text)
    .fetch_all(pool)
    .await?;
    let mut claimed = Vec::new();
    for (id, occurrence) in rows {
        let Some(observed) = occurrence.clone() else {
            continue;
        };
        let mut tx = pool.begin_write().await?;
        let won = sqlx::query(
            "UPDATE scheduled_jobs SET next_run_at = NULL, updated_at = ? WHERE id = ? AND enabled = 1 AND next_run_at = ?",
        )
        .bind(&now_text)
        .bind(&id)
        .bind(&observed)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if !won {
            tx.rollback().await?;
            tracing::debug!(job_id = %id, "cron occurrence was claimed by another scheduler");
            continue;
        }
        let run_id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO cron_runs(id, job_id, status, owner_pid, lease_until, started_at) VALUES(?,?, 'running', ?, ?, ?)")
            .bind(&run_id)
            .bind(&id)
            .bind(owner_pid)
            .bind(&lease)
            .bind(&now_text)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        claimed.push(ClaimedJob {
            id,
            occurrence,
            run_id,
        });
    }
    Ok(claimed)
}

/// Cron wall clock uses `schedule.timezone` when the payload has one, then `timezone` / `tz`.
/// An empty name means UTC, which is what the cron crate uses when no zone is stored.
pub fn payload_timezone(payload: &Value) -> Option<&str> {
    payload
        .pointer("/schedule/timezone")
        .or_else(|| payload.get("timezone"))
        .or_else(|| payload.get("tz"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

fn job_expression(column: &str, payload: &Value) -> String {
    let column = column.trim();
    if !column.is_empty() {
        return column.to_owned();
    }
    payload
        .pointer("/schedule/expr")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|expr| !expr.is_empty())
        .unwrap_or("")
        .to_owned()
}

fn should_disable_after(completed: u64, times: Option<u64>) -> bool {
    matches!(times, Some(limit) if limit > 0 && completed >= limit)
}

/// Bumps `repeat.completed` the way omo does, creating `repeat` when the payload has none.
fn increment_repeat_completed(payload: &mut Value) -> (Option<u64>, u64) {
    if let Some(repeat) = payload.get_mut("repeat") {
        if let Some(obj) = repeat.as_object_mut() {
            let times = obj.get("times").and_then(Value::as_u64);
            let completed = obj.get("completed").and_then(Value::as_u64).unwrap_or(0) + 1;
            obj.insert("completed".to_owned(), Value::from(completed));
            return (times, completed);
        }
    }
    let times = payload.get("times").and_then(Value::as_u64);
    let completed = payload
        .get("completed")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    if let Some(obj) = payload.as_object_mut() {
        if times.is_some() {
            obj.insert("completed".to_owned(), Value::from(completed));
        } else {
            let mut repeat_obj = serde_json::Map::new();
            repeat_obj.insert("completed".to_owned(), Value::from(completed));
            obj.insert("repeat".to_owned(), Value::Object(repeat_obj));
        }
    }
    (times, completed)
}

/// Finishes a run this scheduler still owns. Returns `Ok(false)` without touching the job
/// when the run is no longer `running` (stale recovery already failed it): a late finish
/// from an old owner must not re-enable, reschedule, or relabel anything.
pub async fn finish_run(
    pool: &SqlitePool,
    job_id: &str,
    run_id: &str,
    ok: bool,
    error: Option<&str>,
    output: &str,
    occurrence: Option<&str>,
) -> Result<bool> {
    let now = Utc::now();
    let status = if ok { "succeeded" } else { "failed" };
    let changed = sqlx::query(
        "UPDATE cron_runs SET status = ?, finished_at = ?, error = ?, output = ? WHERE id = ? AND job_id = ? AND status = 'running'",
    )
    .bind(status)
    .bind(now.to_rfc3339())
    .bind(error)
    .bind(output)
    .bind(run_id)
    .bind(job_id)
    .execute(pool)
    .await?
    .rows_affected();
    if changed == 0 {
        tracing::warn!(%job_id, %run_id, "cron run was already settled; late finish ignored");
        return Ok(false);
    }
    let row: Option<(String, i64, String, i64)> = sqlx::query_as(
        "SELECT expression, consecutive_failures, payload_json, enabled FROM scheduled_jobs WHERE id = ?",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await?;
    let Some((expression, failures, payload_json, job_enabled)) = row else {
        return Ok(true);
    };
    let mut payload: Value = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
    let expression = job_expression(&expression, &payload);
    let tz = payload_timezone(&payload).map(str::to_owned);
    let (times, completed) = increment_repeat_completed(&mut payload);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("last_status".to_owned(), Value::String(status.to_owned()));
        obj.insert("last_run_at".to_owned(), Value::String(now.to_rfc3339()));
        if let Some(occurrence) = occurrence {
            obj.insert(
                "last_occurrence".to_owned(),
                Value::String(occurrence.to_owned()),
            );
        }
        if ok {
            obj.insert("last_error".to_owned(), Value::Null);
            obj.insert("last_delivery_error".to_owned(), Value::Null);
            obj.remove("retry_at");
        } else {
            let err = error.unwrap_or("cron run failed").to_owned();
            let lower = err.to_lowercase();
            obj.insert("last_error".to_owned(), Value::String(err.clone()));
            if lower.contains("deliver") || lower.contains("discord") {
                obj.insert("last_delivery_error".to_owned(), Value::String(err));
            } else {
                obj.insert("last_delivery_error".to_owned(), Value::Null);
            }
        }
    }
    let limit_reached = should_disable_after(completed, times);
    let (next, consecutive, enabled) = if job_enabled == 0 {
        // Disabled while it ran: stays disabled and unscheduled.
        (
            None,
            if ok {
                0_i64
            } else {
                failures.saturating_add(1)
            },
            0_i64,
        )
    } else if expression.starts_with("once:") || limit_reached {
        (
            None,
            if ok {
                0_i64
            } else {
                failures.saturating_add(1)
            },
            0_i64,
        )
    } else if ok {
        match next_run_tz(&expression, now, tz.as_deref()) {
            Ok(next) => (Some(next), 0_i64, 1_i64),
            Err(_) => (None, 0_i64, 0_i64),
        }
    } else {
        let consecutive = failures.saturating_add(1);
        (
            next_run_after_failure(&expression, now, consecutive as u32, tz.as_deref())?,
            consecutive,
            1_i64,
        )
    };
    if !ok {
        if let Some(next) = next {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("retry_at".to_owned(), Value::String(next.to_rfc3339()));
            }
        }
    }
    let payload_json = serde_json::to_string(&payload).unwrap_or(payload_json);
    sqlx::query("UPDATE scheduled_jobs SET next_run_at = ?, consecutive_failures = ?, enabled = ?, payload_json = ?, updated_at = ? WHERE id = ?")
        .bind(next.map(|t| t.to_rfc3339()))
        .bind(consecutive)
        .bind(enabled)
        .bind(payload_json)
        .bind(now_rfc3339())
        .bind(job_id)
        .execute(pool)
        .await?;
    if let Some((kind, detail)) = incident_kind(status, error, false) {
        if remediation_due(consecutive as u32, false) || !ok {
            let incident = uuid::Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO cron_incidents(id, job_id, kind, detail, status, created_at) VALUES(?,?,?,?, 'open', ?)")
                .bind(&incident)
                .bind(job_id)
                .bind(kind)
                .bind(&detail)
                .bind(now.to_rfc3339())
                .execute(pool)
                .await?;
            // With a remediator running, the failure is triaged first: an auto-fix is only
            // reported in the home channel, and the remediator itself escalates to the owner
            // when it cannot fix the job. Without one, the owner is told at once.
            if crate::cron_remediate::installed().is_none() {
                escalate_incident(pool, job_id, kind, &detail, now).await?;
            }
        }
    }
    Ok(true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueDelivery {
    pub job_id: String,
    pub text: String,
    pub targets: Vec<Delivery>,
    /// Run after every target was posted. Empty when the scheduler is not delivering.
    pub ack: Option<String>,
}

/// Same fault with different pids, timestamps, or hex ids shares one signature.
pub fn problem_signature(kind: &str, detail: &str) -> String {
    fn normalize(line: &str) -> String {
        static UUID_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        static HEX_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        static DIGITS_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let uuid = UUID_RE.get_or_init(|| {
            regex::Regex::new(r"\b[0-9a-fA-F]{8}-(?:[0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}\b").unwrap()
        });
        let hex = HEX_RE.get_or_init(|| regex::Regex::new(r"\b[0-9a-fA-F]{8,}\b").unwrap());
        let digits = DIGITS_RE.get_or_init(|| regex::Regex::new(r"[0-9]+").unwrap());
        let line = uuid.replace_all(line.trim(), "<uuid>");
        let line = hex.replace_all(&line, "<hex>");
        let line = digits.replace_all(&line, "#");
        line.to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(160)
            .collect()
    }
    let mut lines = detail
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let first = lines.next().map(normalize).unwrap_or_default();
    match lines.next_back().map(normalize) {
        Some(last) if last != first => format!("{kind}:{first} | {last}"),
        _ => format!("{kind}:{first}"),
    }
}

pub fn scheduler_wake() -> &'static tokio::sync::Notify {
    static WAKE: std::sync::OnceLock<tokio::sync::Notify> = std::sync::OnceLock::new();
    WAKE.get_or_init(tokio::sync::Notify::new)
}

pub fn poke_scheduler() {
    scheduler_wake().notify_one();
}

pub async fn next_wait(pool: &SqlitePool, now: DateTime<Utc>) -> Result<Duration> {
    let next: Option<String> = sqlx::query_scalar(
        "SELECT MIN(next_run_at) FROM scheduled_jobs WHERE enabled = 1 AND next_run_at IS NOT NULL",
    )
    .fetch_one(pool)
    .await?;
    // A running row becomes reclaimable when its lease expires (dead owner) and, at the
    // latest, `STALE_LEASE` after that (live owner). The scheduler wakes for that instant so
    // a database whose only jobs are stuck behind orphan runs is repaired in minutes.
    let leases: Vec<String> = sqlx::query_scalar(
        "SELECT lease_until FROM cron_runs WHERE status = 'running' AND lease_until IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    let recovery = leases
        .iter()
        .filter_map(|lease| parse_instant(lease))
        .map(|lease| {
            if lease > now {
                lease
            } else {
                lease + STALE_LEASE
            }
        })
        .min();
    let scheduled = next.map(|next| parse_instant(&next).unwrap_or(now));
    let Some(when) = [scheduled, recovery].into_iter().flatten().min() else {
        return Ok(Duration::from_secs(24 * 60 * 60));
    };
    let delta = when.signed_duration_since(now);
    if delta <= TimeDelta::zero() {
        return Ok(Duration::ZERO);
    }
    Ok(delta.to_std().unwrap_or(Duration::from_secs(24 * 60 * 60)))
}

fn should_deliver(output: &str) -> bool {
    let trimmed = output.trim();
    !trimmed.is_empty()
        && crate::cron::parse_wake_gate(trimmed)
        && !crate::cron::is_cron_silence_response(trimmed)
}

fn expected_output_miss(payload: &Value, output: &str) -> Option<String> {
    let pattern = payload
        .pointer("/expect/output_regex")
        .and_then(Value::as_str)?;
    let regex = regex::Regex::new(pattern).ok()?;
    (!regex.is_match(output)).then(|| format!("output did not match expected regex: {pattern}"))
}

/// Claims every due job, runs its command, and returns the texts that should be posted.
/// Silence and `wakeAgent: false` stay in the run row and are not delivered.
pub async fn drive_due_now(pool: &SqlitePool) -> Result<Vec<DueDelivery>> {
    let model = cron_model();
    let skills = skills_root_from_env();
    drive_due_with(pool, Utc::now(), &model, skills.as_deref()).await
}

fn cron_model() -> crate::meta::backend::RpcModel {
    crate::meta::backend::RpcModel::for_cron(crate::meta::supervisor::mhc_bin())
}

fn skills_root_from_env() -> Option<std::path::PathBuf> {
    if let Some(root) = crate::hub_env::var_os("HERMES_HOME") {
        return Some(std::path::PathBuf::from(root).join("skills"));
    }
    crate::hub_env::var_os("HOME").map(|home| {
        std::path::PathBuf::from(home)
            .join(".hermes")
            .join("skills")
    })
}

pub async fn next_wait_now(pool: &SqlitePool) -> Result<Duration> {
    next_wait(pool, Utc::now()).await
}

pub async fn drive_due(pool: &SqlitePool, now: DateTime<Utc>) -> Result<Vec<DueDelivery>> {
    drive_due_with(
        pool,
        now,
        &crate::cron_exec::StaticModel(String::new()),
        None,
    )
    .await
}

pub async fn drive_due_with(
    pool: &SqlitePool,
    now: DateTime<Utc>,
    model: &dyn crate::lanes::kibitzer::ModelTurn,
    skills_root: Option<&std::path::Path>,
) -> Result<Vec<DueDelivery>> {
    let claimed = claim_due(pool, now, std::process::id()).await?;
    if claimed.is_empty() {
        return Ok(Vec::new());
    }
    // The batch runs its jobs one after another; meanwhile every still-running run of the
    // batch has its lease pushed forward, so a long batch never looks stale while alive.
    let run_ids: Vec<String> = claimed.iter().map(|job| job.run_id.clone()).collect();
    let batch = run_claimed(pool, claimed, model, skills_root);
    tokio::pin!(batch);
    let mut renew = tokio::time::interval_at(
        tokio::time::Instant::now() + LEASE_RENEW_EVERY,
        LEASE_RENEW_EVERY,
    );
    renew.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            result = &mut batch => return result,
            _ = renew.tick() => {
                if let Err(error) = renew_leases(pool, &run_ids, Utc::now()).await {
                    tracing::warn!(%error, "cron lease renewal failed");
                }
            }
        }
    }
}

async fn run_claimed(
    pool: &SqlitePool,
    claimed: Vec<ClaimedJob>,
    model: &dyn crate::lanes::kibitzer::ModelTurn,
    skills_root: Option<&std::path::Path>,
) -> Result<Vec<DueDelivery>> {
    let mut deliveries = Vec::new();
    for ClaimedJob {
        id: job_id,
        occurrence,
        run_id,
    } in claimed
    {
        let payload_json: Option<String> =
            sqlx::query_scalar("SELECT payload_json FROM scheduled_jobs WHERE id = ?")
                .bind(&job_id)
                .fetch_optional(pool)
                .await?;
        let Some(payload_json) = payload_json else {
            continue;
        };
        let payload: Value = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
        let executed = crate::cron_exec::with_run(&run_id,
            crate::cron_exec::run_payload(pool, &job_id, &payload, model, skills_root)).await;
        let (ok, output, error, deliver, monitor_hash) = match executed {
            Ok(executed) => (
                true,
                executed.output,
                None,
                executed.deliver,
                executed.monitor_hash,
            ),
            Err(error) => (false, String::new(), Some(error.to_string()), false, None),
        };
        if ok {
            if let Some(hash) = monitor_hash.as_deref() {
                crate::cron_exec::commit_monitor(pool, &job_id, hash).await?;
            }
        }
        let owned = finish_run(
            pool,
            &job_id,
            &run_id,
            ok,
            error.as_deref(),
            &output,
            occurrence.as_deref(),
        )
        .await?;
        if !owned {
            // Stale recovery already failed this run and rescheduled the job; its output
            // is not delivered and its health is not recorded twice.
            continue;
        }
        note_run_health(
            pool,
            &job_id,
            &run_id,
            if ok { "succeeded" } else { "failed" },
            &output,
            error.as_deref(),
            &payload,
        )
        .await?;
        if ok && deliver && should_deliver(&output) {
            let targets = targets_for_job(pool, &job_id, &payload).await?;
            if !targets.is_empty() {
                deliveries.push(DueDelivery {
                    job_id,
                    text: output,
                    targets,
                    ack: ack_command(&payload),
                });
            } else if let Some(ack) = ack_command(&payload) {
                let _ = run_ack(&ack).await;
            }
        } else if ok {
            if let Some(ack) = ack_command(&payload) {
                let _ = run_ack(&ack).await;
            }
        }
    }
    Ok(deliveries)
}

pub fn ack_command(payload: &Value) -> Option<String> {
    payload
        .get("ack_command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub async fn run_ack(command: &str) -> Result<()> {
    run_shell(command).await.map(|_| ())
}

pub(crate) async fn note_run_health(
    pool: &SqlitePool,
    job_id: &str,
    run_id: &str,
    status: &str,
    output: &str,
    error: Option<&str>,
    payload: &Value,
) -> Result<()> {
    let problem = match status {
        "failed" if error == Some("lease expired before completion") => {
            Some(("stuck", "lease expired before completion".to_owned()))
        }
        "failed" => Some(("failed", error.unwrap_or("cron run failed").to_owned())),
        "succeeded" => {
            expected_output_miss(payload, output).map(|detail| ("empty_success", detail))
        }
        _ => None,
    };
    let Some((kind, detail)) = problem else {
        // A clean run ends the job's known outage: the owner hears "recovered" once.
        // The outage key is the failure signature, which is not prose, so the text is built here.
        if crate::outages::observe(pool, job_id, "", false, Utc::now())
            .await?
            .is_some()
        {
            let recovered = crate::locale::text("다시 정상적으로 실행됐습니다.", "runs normally again.");
            announce_cron_failure(pool, job_id, &recovered, false).await;
        }
        return resolve_after_success(pool, job_id, &now_rfc3339()).await;
    };
    let signature = problem_signature(kind, &detail);
    let opened = record_problem(pool, job_id, run_id, kind, &signature, &detail).await?;
    if crate::cron_exec::EnvironmentFault::parse(&detail).is_some() {
        // A host/environment fault is one incident for every job it fails: its one owner item
        // (remediation or `escalate_incident`) tells the owner, not a per-job outage notice.
        return Ok(());
    }
    // The outage row, keyed by the failure signature, decides whether the owner hears about
    // this failure: once when it starts, never while it repeats, again only on a new cause.
    let notice = crate::outages::observe(pool, job_id, &signature, true, Utc::now()).await?;
    if let (Some(_), Some(_)) = (opened, notice) {
        // omo-gateway triages every new problem (cron_remediate via the queued event), so
        // the announcement says remediation is under way when a remediator is running.
        let remediating = crate::cron_remediate::installed().is_some();
        let detail = crate::cron_remediate::strip_ansi(&detail);
        let text = crate::locale::text(
            &format!("실행이 실패했습니다. 오류: {detail}\n같은 원인으로 반복되는 동안은 다시 알리지 않고, 복구되면 알려드립니다."),
            &format!("failed. Error: {detail}\nI will not repeat this while the same cause continues, and will tell you when it recovers."),
        );
        announce_cron_failure(pool, job_id, &text, remediating).await;
    }
    Ok(())
}

/// Resolves the job's open incidents and fixes its open, remediating, and owner-waiting
/// problems recorded no later than `at`, the finish of a clean run.
async fn settle_failures_before(pool: &SqlitePool, job_id: &str, at: &str) -> Result<()> {
    sqlx::query(
        "UPDATE cron_incidents SET status = 'resolved', resolved_at = ?
          WHERE job_id = ? AND status = 'open' AND julianday(created_at) <= julianday(?)",
    )
    .bind(now_rfc3339())
    .bind(job_id)
    .bind(at)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE cron_problems SET status = 'fixed', updated_at = ?
          WHERE job_id = ? AND status IN ('open', 'remediating', 'needs_owner')
            AND julianday(COALESCE(updated_at, created_at)) <= julianday(?)",
    )
    .bind(now_rfc3339())
    .bind(job_id)
    .bind(at)
    .execute(pool)
    .await?;
    sqlx::query("UPDATE inbox_decisions SET status = 'done' WHERE status = 'open' AND item_id IN (SELECT 'cron:' || CAST(id AS INTEGER) FROM cron_problems WHERE job_id = ? AND status = 'fixed')")
        .bind(job_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// A clean run of a job settles every failure it recorded before `at`: its open incidents are
/// resolved, its open, remediating, and owner-waiting problems are fixed, and its owner items
/// and remediation lanes close. Every path that records a succeeded run ends here.
pub(crate) async fn resolve_after_success(pool: &SqlitePool, job_id: &str, at: &str) -> Result<()> {
    settle_failures_before(pool, job_id, at).await?;
    close_cron_owner_items(pool, job_id).await?;
    crate::cron_remediate::reconcile_fixed(pool, job_id).await?;
    // A clean run after an environment fault whose path is back closes the fault's one item and
    // reports its one recovery result.
    crate::cron_remediate::settle_environment_faults(pool, job_id).await?;
    Ok(())
}

/// Earlier builds left a job's incidents open after a later clean run (only problems were
/// fixed), and runs through the cron sockets settled nothing, so owner items kept returning to
/// the digest (2026-10-28: 85 open incidents, most for jobs that had succeeded since). For each
/// job with a clean run, settles the failures recorded before its latest one; when that run is
/// also the job's latest finished run, the owner item and lanes close too. Idempotent; returns
/// how many jobs it touched.
pub async fn reconcile_resolved_cron_failures(pool: &SqlitePool) -> Result<u64> {
    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT s.job_id, s.at,
                NOT EXISTS (SELECT 1 FROM cron_runs later
                             WHERE later.job_id = s.job_id AND later.status != 'succeeded'
                               AND later.finished_at IS NOT NULL
                               AND julianday(later.finished_at) > julianday(s.at))
           FROM (SELECT job_id, MAX(finished_at) AS at FROM cron_runs
                  WHERE status = 'succeeded' AND finished_at IS NOT NULL GROUP BY job_id) s
          WHERE EXISTS (SELECT 1 FROM cron_incidents i WHERE i.job_id = s.job_id AND i.status = 'open'
                          AND julianday(i.created_at) <= julianday(s.at))
             OR EXISTS (SELECT 1 FROM cron_problems p WHERE p.job_id = s.job_id
                          AND p.status IN ('open', 'remediating', 'needs_owner')
                          AND julianday(COALESCE(p.updated_at, p.created_at)) <= julianday(s.at))",
    )
    .fetch_all(pool)
    .await?;
    let mut settled = 0;
    for (job_id, at, latest) in rows {
        if latest != 0 {
            resolve_after_success(pool, &job_id, &at).await?;
        } else {
            settle_failures_before(pool, &job_id, &at).await?;
        }
        settled += 1;
    }
    Ok(settled)
}

async fn announce_cron_failure(
    pool: &SqlitePool,
    job_id: &str,
    notice: &str,
    remediation_opened: bool,
) {
    let mut text = crate::locale::text(
        &format!("[크론 {job_id}] {notice}"),
        &format!("[cron {job_id}] {notice}"),
    );
    if remediation_opened {
        text.push('\n');
        text.push_str(&crate::locale::text(
            "원인 분석과 자동 복구를 시작했습니다.",
            "I started diagnosing the cause and repairing it automatically.",
        ));
    }
    if let Err(error) = crate::http::record_log(pool, "error", &text).await {
        tracing::warn!(%error, %job_id, "cron failure was not written to the activity log");
    }
    // The remediator either verifies a repair or escalates a concrete owner action. Do not
    // send its input error first, nor a recovery for an outage the owner never had to handle.
    if remediation_opened || crate::cron_remediate::installed().is_some() {
        return;
    }
    let mode = crate::sessions::watch::effective_notify_mode(pool)
        .await
        .unwrap_or(crate::sessions::watch::NotifyMode::Actionable);
    if !crate::sessions::watch::owner_post(mode, "failure") {
        return;
    }
    let channel = match crate::import::imported_slack_home(pool).await {
        Ok(Some(channel)) => channel,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(%error, %job_id, "cron failure has no slack home to post to");
            return;
        }
    };
    let session = crate::chat::session::SessionKey::new(
        "slack",
        None::<String>,
        channel,
        None::<String>,
        "owner",
    );
    let report = crate::report_gate::Report::new(
        uuid::Uuid::new_v4().to_string(), "cron", format!("cron-outage:{job_id}"),
        crate::report_gate::Salience::Action,
        text,
    ).with_state(&problem_signature("notice", notice));
    if let Err(error) = crate::owner_alert::notify_report(pool, &session.storage_key(), &report).await {
        tracing::warn!(%error, %job_id, "cron failure was logged but not posted");
    }
}

async fn record_problem(
    pool: &SqlitePool,
    job_id: &str,
    run_id: &str,
    kind: &str,
    signature: &str,
    detail: &str,
) -> Result<Option<i64>> {
    type ProblemRow = (i64, String, Option<String>, Option<String>, Option<String>);
    let existing: Option<ProblemRow> = sqlx::query_as(
        "SELECT CAST(id AS INTEGER), status, run_id, updated_at, lane_key FROM cron_problems WHERE job_id = ? AND kind = ? AND signature = ?",
    )
    .bind(job_id)
    .bind(kind)
    .bind(signature)
    .fetch_optional(pool)
    .await?;
    if let Some((id, status, last_run, updated, lane)) = existing {
        // omo reopens a fixed or ignored problem, or one stranded in remediating with no
        // live lane, when a different run fails the same way. Anything else is a duplicate.
        let stale = status == "remediating"
            && updated
                .as_deref()
                .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                .is_some_and(|at| {
                    Utc::now() - at.with_timezone(&Utc)
                        > chrono::TimeDelta::seconds(crate::cron_remediate::STALE_REMEDIATING_SECS)
                });
        let live_lane = match &lane {
            Some(key) => sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM lanes WHERE key = ? AND status NOT IN ('verified_done','closed')",
            )
            .bind(key)
            .fetch_one(pool)
            .await? > 0,
            None => false,
        };
        let reopen = matches!(status.as_str(), "fixed" | "ignored") || (stale && !live_lane);
        if !reopen || last_run.as_deref() == Some(run_id) {
            return Ok(None);
        }
        let now = now_rfc3339();
        sqlx::query("UPDATE cron_problems SET status = 'open', run_id = ?, detail = ?, summary = ?, lane_key = NULL, created_at = ?, updated_at = ? WHERE CAST(id AS INTEGER) = ?")
            .bind(run_id)
            .bind(detail)
            .bind(detail)
            .bind(&now)
            .bind(&now)
            .bind(id)
            .execute(pool)
            .await?;
        enqueue_problem(pool, id, job_id, kind, run_id, detail).await?;
        return Ok(Some(id));
    }
    let id: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(CAST(id AS INTEGER)), 0) + 1 FROM cron_problems")
            .fetch_one(pool)
            .await?;
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO cron_problems(id, job_id, summary, created_at, status, kind, signature, detail, run_id, updated_at) VALUES(?,?,?,?, 'open', ?,?,?,?,?)",
    )
    .bind(id.to_string())
    .bind(job_id)
    .bind(detail)
    .bind(&now)
    .bind(kind)
    .bind(signature)
    .bind(detail)
    .bind(run_id)
    .bind(&now)
    .execute(pool)
    .await?;
    enqueue_problem(pool, id, job_id, kind, run_id, detail).await?;
    Ok(Some(id))
}

async fn enqueue_problem(
    pool: &SqlitePool,
    id: i64,
    job_id: &str,
    kind: &str,
    run_id: &str,
    detail: &str,
) -> Result<()> {
    let lead = crate::lanes::lead::default_lead_or("meta");
    crate::lanes::dispatch::enqueue(
        pool,
        job_id,
        &lead,
        "cron_problem",
        &serde_json::json!({"problem_id": id, "job_id": job_id, "kind": kind, "run_id": run_id, "detail": detail})
            .to_string(),
    )
    .await?;
    Ok(())
}

fn deliver_wants_home(payload: &Value) -> bool {
    let parts: Vec<&str> = match payload.get("deliver") {
        Some(Value::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect(),
        _ => return false,
    };
    parts
        .iter()
        .any(|part| matches!(*part, "origin" | "slack" | "all" | "discord"))
}

/// Concrete `deliver` targets, then the stored delivery column, then the Slack
/// home channel copied at import when the job asked for origin, slack, all, or
/// discord and named no chat.
pub async fn targets_for_job(
    pool: &SqlitePool,
    job_id: &str,
    payload: &Value,
) -> Result<Vec<Delivery>> {
    let mut targets = delivery_targets(payload);
    if targets.is_empty() {
        if let Ok(delivery_json) =
            sqlx::query_scalar::<_, String>("SELECT delivery_json FROM scheduled_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_one(pool)
                .await
        {
            if let Ok(parsed) = serde_json::from_str::<Value>(&delivery_json) {
                targets = delivery_targets(&parsed);
            }
        }
    }
    if targets.is_empty() && deliver_wants_home(payload) {
        if let Some(home) = crate::import::imported_slack_home(pool).await? {
            let chat = home.trim().trim_start_matches('#').to_owned();
            if !chat.is_empty() {
                targets.push(Delivery {
                    platform: "slack".into(),
                    chat_id: chat,
                    thread_id: None,
                });
            }
        }
    }
    Ok(targets)
}

/// Cron producers may supply a stable state (for example a recommendation id), independent
/// of a run timestamp or wording. Ordinary scheduled information is collected in the digest.
pub fn cron_report(job_id: &str, payload: &Value, text: &str) -> crate::report_gate::Report {
    use crate::report_gate::{Report, Salience};
    let salience = match payload.get("report_salience").and_then(Value::as_str) {
        Some("action") => Salience::Action,
        Some("decision") => Salience::Decision,
        Some("result") => Salience::Result,
        Some("internal") => Salience::Internal,
        Some("fyi") => Salience::Fyi,
        _ => Salience::Action,
    };
    let empty_benchmark = text.contains("채택 0") && text.contains("보류 0")
        && text.contains("새 PR 없음");
    let mut report = Report::new(uuid::Uuid::new_v4().to_string(), "cron",
        format!("cron:{job_id}"), salience, if empty_benchmark { "" } else { text });
    if let Some(state) = payload.get("report_state").and_then(Value::as_str) {
        report = report.with_state(state);
    }
    report
}

async fn deliver_run_output(
    pool: &SqlitePool,
    job_id: &str,
    payload: &Value,
    text: &str,
) -> Option<String> {
    let targets = match targets_for_job(pool, job_id, payload).await {
        Ok(targets) => targets,
        Err(error) => return Some(error.to_string()),
    };
    if targets.is_empty() {
        return match ack_command(payload) {
            Some(ack) => run_ack(&ack).await.err().map(|error| error.to_string()),
            None => None,
        };
    }
    let mut report = cron_report(job_id, payload, text);
    if payload.get("report_salience").is_none()
        && !crate::report_gate::is_empty_body(&report.body)
        && !crate::report_gate::is_leak(&report.body)
    {
        let judge = crate::judge::Judge::from_config(crate::judge::typesafe_from_env()).with_pool(pool.clone());
        match crate::report_gate::classify_salience(&judge, &report).await {
            Ok(salience) => report.salience = salience,
            Err(error) => tracing::warn!(%error, job_id, "cron salience unavailable; preserving owner report"),
        }
    }
    let mut errors = Vec::new();
    let mut delivered = Vec::new();
    for target in &targets {
        let mut report = report.clone();
        report.topic_key = format!("{}:{}:{}:{}", report.topic_key, target.platform,
            target.chat_id, target.thread_id.as_deref().unwrap_or(""));
        if target.platform == "slack" {
            let session = crate::chat::session::SessionKey::new("slack", None::<String>,
                &target.chat_id, target.thread_id.clone(), "cron");
            match crate::report_gate::queue(pool, &session.storage_key(), &report, false).await {
                Ok((_, Some(id))) => {
                    match crate::chat::reply::deliver_stored_posted(pool, id, &session.storage_key(), &report.body).await {
                        Ok(Some(posted)) => {
                            delivered.push(target);
                            if let Err(error) = crate::report_feedback::posted_from_outbox(pool, id, &posted).await {
                                errors.push(error.to_string());
                            }
                        }
                        Ok(None) => errors.push("Slack delivery has no receipt yet; queued for retry".into()),
                        Err(error) => errors.push(error.to_string()),
                    }
                }
                Ok((_, None)) => {},
                Err(error) => errors.push(error.to_string()),
            }
        } else {
            match crate::report_gate::submit(pool, &report).await {
                Ok(crate::report_gate::Decision::Sent) => {
                    let (error, sent) = post_targets(std::slice::from_ref(target), text).await;
                    if let Some(error) = error { errors.push(error); }
                    delivered.extend(sent);
                }
                Ok(_) => {},
                Err(error) => errors.push(error.to_string()),
            }
        }
    }
    let error = (!errors.is_empty()).then(|| errors.join("; "));
    // omo's attach_to_session (default on): what the job posted joins that conversation.
    if payload
        .get("attach_to_session")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        for target in delivered {
            if let Err(mirror) = crate::mirror::mirror_cron_delivery(
                pool,
                job_id,
                &target.platform,
                &target.chat_id,
                target.thread_id.as_deref(),
                text,
            )
            .await
            {
                tracing::warn!(job_id, error = %mirror, "cron delivery was not mirrored into its session");
            }
        }
    }
    if let Err(record) = remember_delivery(pool, job_id, error.as_deref()).await {
        return Some(record.to_string());
    }
    if error.is_none() {
        if let Some(ack) = ack_command(payload) {
            if let Err(ack_error) = run_ack(&ack).await {
                return Some(ack_error.to_string());
            }
        }
    }
    error
}

async fn post_targets<'a>(
    targets: &'a [Delivery],
    text: &str,
) -> (Option<String>, Vec<&'a Delivery>) {
    let mut errors = Vec::new();
    let mut delivered = Vec::new();
    for target in targets {
        let posted = match target.platform.as_str() {
            "slack" => post_slack_target(&target.chat_id, target.thread_id.as_deref(), text).await,
            "discord" => post_discord_target(&target.chat_id, text).await,
            other => Err(HubError::message(format!(
                "unconfigured delivery transport: {other}"
            ))),
        };
        match posted {
            Ok(true) => delivered.push(target),
            Ok(false) => errors.push(format!(
                "unconfigured delivery transport: {}",
                target.platform
            )),
            Err(error) => errors.push(error.to_string()),
        }
    }
    let error = (!errors.is_empty()).then(|| errors.join("; "));
    (error, delivered)
}

async fn post_slack_target(channel: &str, thread: Option<&str>, text: &str) -> Result<bool> {
    #[cfg(feature = "hub-slack")]
    {
        crate::chat::slack::deliver(channel, thread, text).await
    }
    #[cfg(not(feature = "hub-slack"))]
    {
        let _ = (channel, thread, text);
        Ok(false)
    }
}

async fn post_discord_target(channel: &str, text: &str) -> Result<bool> {
    #[cfg(feature = "hub-discord")]
    {
        crate::chat::discord::deliver(channel, text).await
    }
    #[cfg(not(feature = "hub-discord"))]
    {
        let _ = (channel, text);
        Ok(false)
    }
}

pub async fn remember_delivery(pool: &SqlitePool, job_id: &str, error: Option<&str>) -> Result<()> {
    if error.is_none() && job_id == crate::inbox::DIGEST_JOB_ID {
        crate::cron_remediate::acknowledge_digest(pool).await?;
    }
    let payload_json: Option<String> =
        sqlx::query_scalar("SELECT payload_json FROM scheduled_jobs WHERE id = ?")
            .bind(job_id)
            .fetch_optional(pool)
            .await?;
    let Some(payload_json) = payload_json else {
        return Ok(());
    };
    let mut payload: Value = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
    if let Some(object) = payload.as_object_mut() {
        match error {
            Some(text) => {
                object.insert("last_delivery_error".into(), Value::String(text.to_owned()));
            }
            None => {
                object.insert("last_delivery_error".into(), Value::Null);
            }
        }
    }
    sqlx::query("UPDATE scheduled_jobs SET payload_json = ?, updated_at = ? WHERE id = ?")
        .bind(payload.to_string())
        .bind(now_rfc3339())
        .bind(job_id)
        .execute(pool)
        .await?;
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

    #[cfg(any(unix, windows))]
    #[test]
    fn process_alive_sees_this_process_and_never_pid_zero() {
        assert!(process_alive(std::process::id()));
        assert!(!process_alive(0));
    }

    /// The Child keeps its process handle open, so the exited pid cannot be reused while this
    /// test still asks about it. (On Unix a reaped pid may be reused at once, so no such test.)
    #[cfg(windows)]
    #[test]
    fn process_alive_reports_an_exited_windows_process_gone() {
        let mut child = std::process::Command::new("cmd").args(["/C", "exit"]).spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!process_alive(pid));
    }

    #[tokio::test]
    async fn cron_output_is_deferred_with_content_and_empty_runs_are_suppressed() {
        let pool = crate::storage::memory().await.unwrap();
        let payload = serde_json::json!({"deliver":"slack:D1", "report_salience":"fyi"});
        let body = "새 게시글을 발행했습니다.";
        assert!(deliver_run_output(&pool, "publish", &payload, body).await.is_none());
        let pending: String = sqlx::query_scalar("SELECT body FROM report_digest_pending")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(pending, body);
        let queued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_outbox")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(queued, 0);
        let report = cron_report("benchmark", &payload, "판정 4건: 채택 0 · 보류 0 · 제외 4 새 PR 없음");
        assert_eq!(crate::report_gate::decide(&report, None, None, false), crate::report_gate::Decision::Empty);
    }

    #[test]
    fn cron_recommendation_identity_does_not_depend_on_wording() {
        let payload = serde_json::json!({"report_state":"recommendation:58"});
        let first = cron_report("recommend", &payload, "새 글 추천입니다.");
        let second = cron_report("recommend", &payload, "이 글을 읽어 보세요.");
        assert_eq!(first.topic_key, second.topic_key);
        assert_eq!(first.state_hash, second.state_hash);
    }

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
    async fn two_schedulers_on_one_database_claim_an_occurrence_once() {
        let dir = std::env::temp_dir().join(format!(
            "ferryx-claim-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hub.sqlite");
        let first = crate::storage::connect(&path).await.unwrap();
        let second = crate::storage::connect(&path).await.unwrap();
        for round in 0..20 {
            let due = (Utc::now() - TimeDelta::minutes(1)).to_rfc3339();
            sqlx::query("INSERT OR REPLACE INTO scheduled_jobs(id, expression, payload_json, enabled, next_run_at, delivery_json) VALUES('job','every 5m','{}',1,?, '[]')")
                .bind(&due)
                .execute(&first)
                .await
                .unwrap();
            let now = Utc::now();
            let (a, b) = tokio::join!(claim_due(&first, now, 111), claim_due(&second, now, 222));
            let total = a.unwrap().len() + b.unwrap().len();
            assert_eq!(
                total, 1,
                "round {round}: one scheduler claims the occurrence"
            );
        }
        let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cron_runs WHERE job_id = 'job'")
            .fetch_one(&first)
            .await
            .unwrap();
        assert_eq!(runs, 20);
        first.close().await;
        second.close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn finishing_a_run_updates_only_that_run() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, next_run_at, delivery_json) VALUES('job','every 5m','{}',1,NULL, '[]')")
            .execute(&pool)
            .await
            .unwrap();
        for (id, pid) in [("mine", 1_i64), ("theirs", 2)] {
            sqlx::query("INSERT INTO cron_runs(id, job_id, status, owner_pid, started_at) VALUES(?, 'job', 'running', ?, ?)")
                .bind(id)
                .bind(pid)
                .bind(Utc::now().to_rfc3339())
                .execute(&pool)
                .await
                .unwrap();
        }
        finish_run(&pool, "job", "mine", true, None, "done", None)
            .await
            .unwrap();
        let rows: Vec<(String, String, Option<String>)> =
            sqlx::query_as("SELECT id, status, output FROM cron_runs ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            rows[0],
            ("mine".into(), "succeeded".into(), Some("done".into()))
        );
        assert_eq!(rows[1].0, "theirs");
        assert_eq!(rows[1].1, "running");
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
        assert_eq!(
            claimed
                .iter()
                .map(|job| job.id.as_str())
                .collect::<Vec<_>>(),
            vec!["job"]
        );
        let occurrence = claimed[0].occurrence.clone();
        assert_eq!(occurrence.as_deref(), Some(due.as_str()));
        assert!(claim_due(&pool, Utc::now(), 9).await.unwrap().is_empty());
        finish_run(
            &pool,
            "job",
            &claimed[0].run_id,
            false,
            Some("boom"),
            "",
            occurrence.as_deref(),
        )
        .await
        .unwrap();
        let incidents: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM cron_incidents WHERE job_id = 'job'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(incidents, 1);
        // As omo's needs_owner: the failure becomes an owner item the alert sweep posts.
        let alerts = crate::owner_alert::pending_alerts(&pool, Utc::now())
            .await
            .unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].item_id, cron_owner_item_id("job"));
        assert_eq!(
            crate::owner_alert::alert_line(&alerts[0]),
            "📬 [크론] job — job: 확인 필요 - failed: boom"
        );
        let failures: i64 =
            sqlx::query_scalar("SELECT consecutive_failures FROM scheduled_jobs WHERE id = 'job'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(failures, 1);
        let payload: String =
            sqlx::query_scalar("SELECT payload_json FROM scheduled_jobs WHERE id = 'job'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["last_status"], "failed");
        assert_eq!(payload["last_error"], "boom");
        assert!(payload["last_delivery_error"].is_null());
        assert_eq!(payload["last_occurrence"], due);
        assert_eq!(payload["repeat"]["completed"], 1);
        assert!(payload["retry_at"].as_str().is_some());
    }

    #[test]
    fn seoul_cron_wall_clock_is_midnight_utc_and_a_missing_zone_stays_utc() {
        let after = DateTime::parse_from_rfc3339("2026-10-22T00:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let seoul = next_run_tz("0 9 * * *", after, Some("Asia/Seoul")).unwrap();
        assert_eq!(
            seoul,
            DateTime::parse_from_rfc3339("2026-10-23T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
        let utc = next_run_tz("0 9 * * *", after, None).unwrap();
        assert_eq!(
            utc,
            DateTime::parse_from_rfc3339("2026-10-22T09:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
        let mut payload = serde_json::json!({"schedule": {"timezone": "Asia/Seoul"}});
        assert_eq!(payload_timezone(&payload), Some("Asia/Seoul"));
        payload = serde_json::json!({"timezone": "UTC"});
        assert_eq!(payload_timezone(&payload), Some("UTC"));
    }

    #[test]
    fn the_native_socket_speaks_the_gateway_line() {
        assert_eq!(
            parse_native_request("\"job-1\"\n").unwrap(),
            NativeRequest::Run {
                id: "job-1".into(),
                force: false
            }
        );
        assert_eq!(
            parse_native_request(r#"{"id":"job-1","force":true}"#).unwrap(),
            NativeRequest::Run {
                id: "job-1".into(),
                force: true
            }
        );
        assert_eq!(
            parse_native_request(r#"{"approve_problem":4,"owner_token":"tok"}"#).unwrap(),
            NativeRequest::Approve {
                problem_id: 4,
                owner_token: "tok".into()
            }
        );
        assert!(publishing(
            "omon-playstore-daily-publish",
            &serde_json::json!({})
        ));
        assert!(publishing(
            "other",
            &serde_json::json!({"external_effect": true})
        ));
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_native_socket_whose_stop_sender_is_dropped_returns_instead_of_spinning() {
        // A dropped sender makes `changed()` ready on every poll. The 1022.5 daemon kept the
        // listener alive in that state and burned a full core (37 CPU minutes in 40).
        let pool = crate::storage::memory().await.unwrap();
        let dir = std::env::temp_dir().join(format!("fx-cron-drop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("run.sock");
        let (tx, rx) = tokio::sync::watch::channel(false);
        drop(tx);
        let server = tokio::spawn(serve_native_socket(path.clone(), pool, rx));
        let finished = tokio::time::timeout(std::time::Duration::from_secs(5), server).await;
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            matches!(finished, Ok(Ok(Ok(())))),
            "serve_native_socket kept running after its stop sender was dropped"
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_native_socket_stops_when_true_is_sent() {
        let pool = crate::storage::memory().await.unwrap();
        let dir = std::env::temp_dir().join(format!("fx-cron-stop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("run.sock");
        let (tx, rx) = tokio::sync::watch::channel(false);
        let server = tokio::spawn(serve_native_socket(path.clone(), pool, rx));
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !path.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        // Still serving while the sender is alive and false.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(!server.is_finished());
        tx.send(true).unwrap();
        let finished = tokio::time::timeout(std::time::Duration::from_secs(5), server).await;
        let gone = !path.exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(matches!(finished, Ok(Ok(Ok(())))));
        assert!(gone, "a clean stop removes the socket file");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_native_client_runs_a_job_and_gets_a_receipt() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json) VALUES('job-1','every 1m','{\"command\":\"printf hi\"}')")
            .execute(&pool)
            .await
            .unwrap();
        let path =
            std::env::temp_dir().join(format!("ferryx-cron-{}-run.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let (tx, rx) = tokio::sync::watch::channel(false);
        let server_pool = pool.clone();
        let server_path = path.clone();
        let server = tokio::spawn(async move {
            serve_native_socket(server_path, server_pool, rx)
                .await
                .unwrap();
        });
        let mut client = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Ok(stream) = tokio::net::UnixStream::connect(&path).await {
                    return stream;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        client.write_all(b"\"job-1\"\n").await.unwrap();
        let mut line = String::new();
        tokio::io::BufReader::new(client)
            .read_line(&mut line)
            .await
            .unwrap();
        let receipt: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(receipt["status"], "succeeded");
        assert_eq!(receipt["job_id"], "job-1");
        assert_eq!(receipt["output"], "hi");
        assert!(receipt["last_delivery_error"].is_null());
        let noisy = problem_signature(
            "failed",
            "cron script failed with Some(1): 2026-10-13T01:02:03Z pid 4242 req 9f8e7d6c5b4a connect refused",
        );
        let again = problem_signature(
            "failed",
            "cron script failed with Some(1): 2026-10-14T11:12:13Z pid 17 req 00112233aabb connect refused",
        );
        assert_eq!(noisy, again);
        tx.send(true).unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_due_job_delivers_and_the_third_failure_opens_a_lane() {
        let pool = crate::storage::memory().await.unwrap();
        let due = (Utc::now() - TimeDelta::minutes(1)).to_rfc3339();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, next_run_at, delivery_json) VALUES('daily','every 10m',?,1,?, '[]')")
            .bind(r#"{"command":"printf hi","deliver":"slack:C1:1700.1"}"#)
            .bind(&due)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, next_run_at, delivery_json) VALUES('quiet','every 10m',?,1,?, '[]')")
            .bind(r#"{"command":"printf '[SILENT]'","deliver":"slack:C1"}"#)
            .bind(&due)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, next_run_at, delivery_json, consecutive_failures) VALUES('broken','every 10m',?,1,?, '[]', 2)")
            .bind(r#"{"command":"exit 1","deliver":"slack:C9"}"#)
            .bind(&due)
            .execute(&pool)
            .await
            .unwrap();
        let deliveries = drive_due(&pool, Utc::now()).await.unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].job_id, "daily");
        assert_eq!(deliveries[0].text, "hi");
        assert_eq!(deliveries[0].targets[0].chat_id, "C1");
        assert_eq!(
            deliveries[0].targets[0].thread_id.as_deref(),
            Some("1700.1")
        );
        let kind: String =
            sqlx::query_scalar("SELECT kind FROM cron_problems WHERE job_id = 'broken'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(kind, "failed");
        // The remediator (cron_remediate) triages the queued problem; the scheduler opens
        // no lane on its own any more.
        let (state, lane): (String, Option<String>) =
            sqlx::query_as("SELECT status, lane_key FROM cron_problems WHERE job_id = 'broken'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((state.as_str(), lane), ("open", None));
        let event: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE kind = 'cron_problem'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(event, 1);
        remember_delivery(
            &pool,
            "daily",
            Some("unconfigured delivery transport: slack"),
        )
        .await
        .unwrap();
        let stored: String =
            sqlx::query_scalar("SELECT payload_json FROM scheduled_jobs WHERE id = 'daily'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(stored.contains("unconfigured delivery transport: slack"));
        let wait = next_wait(&pool, Utc::now()).await.unwrap();
        assert!(wait > Duration::ZERO);
    }

    #[tokio::test]
    async fn a_disabled_job_does_not_run_unless_forced() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES('job-1','every 1m','{\"command\":\"printf hi\"}',0)")
            .execute(&pool)
            .await
            .unwrap();
        let refused = run_native_job(&pool, "job-1", false, false).await.unwrap();
        assert_eq!(refused.status, "failed");
        assert_eq!(
            refused.error.as_deref(),
            Some("cron job job-1 is disabled; pass force to run it")
        );
        assert!(refused.run_id.is_empty());
        let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cron_runs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(runs, 0);
        let forced = run_native_job(&pool, "job-1", false, true).await.unwrap();
        assert_eq!(forced.status, "succeeded");
        assert_eq!(forced.output.as_deref(), Some("hi"));
    }

    #[tokio::test]
    async fn run_now_posts_an_originless_job_to_the_imported_home_channel() {
        assert!(
            crate::chat::slack::bot_token().is_none(),
            "refusing to post a run-now delivery while a Slack token is configured"
        );
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO hub_meta(key, value) VALUES('slack_home_channel', 'C0HOME')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES('job-1','every 1m',?,1)")
            .bind(r#"{"command":"printf 확인해주세요","deliver":"origin","report_salience":"action"}"#)
            .execute(&pool)
            .await
            .unwrap();
        let receipt = run_native_job(&pool, "job-1", false, false).await.unwrap();
        assert_eq!(receipt.status, "succeeded");
        assert_eq!(receipt.output.as_deref(), Some("확인해주세요"));
        assert_eq!(
            receipt.last_delivery_error.as_deref(),
            Some("Slack delivery has no receipt yet; queued for retry")
        );
        let stored: String =
            sqlx::query_scalar("SELECT payload_json FROM scheduled_jobs WHERE id = 'job-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(stored.contains("Slack delivery has no receipt yet; queued for retry"));
    }

    #[tokio::test]
    async fn a_due_originless_job_targets_the_imported_home_channel() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO hub_meta(key, value) VALUES('slack_home_channel', 'C0HOME')")
            .execute(&pool)
            .await
            .unwrap();
        let due = (Utc::now() - TimeDelta::minutes(1)).to_rfc3339();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, next_run_at, delivery_json) VALUES('home','every 10m',?,1,?, '[]')")
            .bind(r#"{"command":"printf hi","deliver":"origin"}"#)
            .bind(&due)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, next_run_at, delivery_json) VALUES('local','every 10m',?,1,?, '[]')")
            .bind(r#"{"command":"printf hi","deliver":"local"}"#)
            .bind(&due)
            .execute(&pool)
            .await
            .unwrap();
        let deliveries = drive_due(&pool, Utc::now()).await.unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].job_id, "home");
        assert_eq!(deliveries[0].targets[0].platform, "slack");
        assert_eq!(deliveries[0].targets[0].chat_id, "C0HOME");
    }

    #[tokio::test]
    async fn approving_a_problem_does_not_run_a_disabled_job() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES('paused','every 1m','{\"command\":\"printf hi\"}',0), ('live','every 1m','{\"command\":\"printf hi\"}',1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO cron_problems(id, job_id, summary, created_at, owner_token, status) VALUES('7','paused','boom','2020-01-01T00:00:00Z','tok','open'), ('8','live','boom','2020-01-01T00:00:00Z','tok','open')")
            .execute(&pool)
            .await
            .unwrap();
        let refused = approve_problem(&pool, 7, "tok").await.unwrap();
        assert_eq!(refused.status, "failed");
        assert_eq!(
            refused.error.as_deref(),
            Some("cron job paused is disabled; pass force to run it")
        );
        let runs: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM cron_runs WHERE job_id = 'paused'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(runs, 0);
        let allowed = approve_problem(&pool, 8, "tok").await.unwrap();
        assert_eq!(allowed.status, "succeeded");
        assert_eq!(allowed.output.as_deref(), Some("hi"));
    }

    #[tokio::test]
    async fn a_new_cron_failure_is_logged_and_posted_only_to_a_stored_home_channel() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, consecutive_failures) VALUES('broken','every 1m','{}',1,3)")
            .execute(&pool)
            .await
            .unwrap();
        note_run_health(
            &pool,
            "broken",
            "run-1",
            "failed",
            "",
            Some("disk full"),
            &serde_json::json!({}),
        )
        .await
        .unwrap();
        let logged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM hub_log WHERE message LIKE '[크론 broken]%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(logged, 1);
        let message: String = sqlx::query_scalar("SELECT message FROM hub_log")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(message.starts_with("[크론 broken] 실행이 실패했습니다. 오류: disk full"), "{message}");
        let outbox: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_outbox")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(outbox, 0);
        note_run_health(
            &pool,
            "broken",
            "run-1",
            "failed",
            "",
            Some("disk full"),
            &serde_json::json!({}),
        )
        .await
        .unwrap();
        let logged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM hub_log WHERE message LIKE '[크론 broken]%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(logged, 1, "a duplicate problem is not announced again");
        sqlx::query("INSERT INTO hub_meta(key, value) VALUES('slack_home_channel', 'CHOME')")
            .execute(&pool)
            .await
            .unwrap();
        note_run_health(
            &pool,
            "broken",
            "run-2",
            "failed",
            "",
            Some("socket refused"),
            &serde_json::json!({}),
        )
        .await
        .unwrap();
        let queued: Vec<(String, String)> =
            sqlx::query_as("SELECT session_key, text FROM chat_outbox")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(queued.len(), 1, "{queued:?}");
        assert!(queued[0].1.contains("오류: socket refused"), "{queued:?}");
        let session = crate::chat::session::SessionKey::from_storage_key(&queued[0].0).unwrap();
        assert_eq!(session.channel_id, "CHOME");
    }

    /// A failing job is one outage: one notice when it starts, nothing while the same cause
    /// repeats on new runs, one notice when a clean run recovers it, and nothing after that.
    #[tokio::test]
    async fn a_repeating_cron_failure_is_reported_once_and_its_recovery_once() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, consecutive_failures) VALUES('collect','every 1m','{}',1,3)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO hub_meta(key, value) VALUES('slack_home_channel', 'CHOME')")
            .execute(&pool)
            .await
            .unwrap();
        let fail = |run: &'static str| {
            let pool = pool.clone();
            async move {
                note_run_health(
                    &pool,
                    "collect",
                    run,
                    "failed",
                    "",
                    Some("LOGIN_FAILED Error: CDP command timeout: Page.navigate"),
                    &serde_json::json!({}),
                )
                .await
                .unwrap();
            }
        };
        let posted = |pool: SqlitePool| async move {
            sqlx::query_scalar::<_, String>("SELECT text FROM chat_outbox ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap()
        };
        fail("run-1").await;
        // The problem row is fixed by a later clean run in production; here the same cause
        // simply keeps failing on new runs, which reopens nothing and posts nothing.
        sqlx::query("UPDATE cron_problems SET status = 'fixed'")
            .execute(&pool)
            .await
            .unwrap();
        fail("run-2").await;
        fail("run-3").await;
        assert_eq!(posted(pool.clone()).await.len(), 1, "{:?}", posted(pool.clone()).await);
        let succeed = |run: &'static str| {
            let pool = pool.clone();
            async move {
                note_run_health(&pool, "collect", run, "succeeded", "ok", None, &serde_json::json!({}))
                    .await
                    .unwrap();
            }
        };
        succeed("run-4").await;
        succeed("run-5").await;
        let texts = posted(pool.clone()).await;
        assert_eq!(texts.len(), 2, "{texts:?}");
        assert!(texts[1].contains("다시 정상적으로 실행됐습니다"), "{texts:?}");
        fail("run-6").await;
        assert_eq!(posted(pool.clone()).await.len(), 3, "a recovered job that fails again is new");
    }

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    async fn insert_job(
        pool: &SqlitePool,
        id: &str,
        enabled: i64,
        next_run_at: Option<DateTime<Utc>>,
    ) {
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, next_run_at, delivery_json) VALUES(?, 'every 10m', '{\"command\":\"printf hi\"}', ?, ?, '[]')")
            .bind(id)
            .bind(enabled)
            .bind(next_run_at.map(|t| t.to_rfc3339()))
            .execute(pool)
            .await
            .unwrap();
    }

    async fn insert_running(
        pool: &SqlitePool,
        run_id: &str,
        job_id: &str,
        owner_pid: i64,
        lease_until: DateTime<Utc>,
    ) {
        sqlx::query("INSERT INTO cron_runs(id, job_id, status, owner_pid, lease_until, started_at) VALUES(?, ?, 'running', ?, ?, ?)")
            .bind(run_id)
            .bind(job_id)
            .bind(owner_pid)
            .bind(lease_until.to_rfc3339())
            .bind((lease_until - LEASE).to_rfc3339())
            .execute(pool)
            .await
            .unwrap();
    }

    async fn run_row(pool: &SqlitePool, run_id: &str) -> (String, Option<String>, Option<String>) {
        sqlx::query_as("SELECT status, finished_at, error FROM cron_runs WHERE id = ?")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn job_row(pool: &SqlitePool, id: &str) -> (i64, Option<String>) {
        sqlx::query_as("SELECT enabled, next_run_at FROM scheduled_jobs WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn an_orphan_run_of_a_dead_owner_is_recovered_without_replaying_the_occurrence() {
        let pool = crate::storage::memory().await.unwrap();
        let now = at("2026-10-26T01:00:00Z");
        insert_job(&pool, "omocat", 1, None).await;
        insert_running(
            &pool,
            "orphan",
            "omocat",
            69535,
            now - TimeDelta::minutes(5),
        )
        .await;
        let recovered = recover_stale_runs(&pool, now, |_| false).await.unwrap();
        assert_eq!(recovered, 1);
        let (status, finished, error) = run_row(&pool, "orphan").await;
        assert_eq!(status, "failed");
        assert_eq!(finished.as_deref(), Some(now.to_rfc3339().as_str()));
        assert_eq!(error.as_deref(), Some("lease expired before completion"));
        let (enabled, next) = job_row(&pool, "omocat").await;
        assert_eq!(enabled, 1);
        let next = at(&next.expect("the job is rescheduled"));
        assert!(next > now, "the next run is in the future: {next}");
        // The missed occurrence is not replayed: nothing is due at `now`.
        assert!(claim_due(&pool, now, 1).await.unwrap().is_empty());
        let runs: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM cron_runs WHERE job_id = 'omocat'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(runs, 1);
    }

    #[tokio::test]
    async fn a_fresh_lease_of_a_live_owner_is_not_reclaimed() {
        let pool = crate::storage::memory().await.unwrap();
        let now = at("2026-10-26T01:00:00Z");
        insert_job(&pool, "busy", 1, None).await;
        insert_running(&pool, "fresh", "busy", 4242, now + TimeDelta::minutes(20)).await;
        insert_job(&pool, "slow", 1, None).await;
        insert_running(
            &pool,
            "expired-live",
            "slow",
            4243,
            now - TimeDelta::minutes(5),
        )
        .await;
        let recovered = recover_stale_runs(&pool, now, |_| true).await.unwrap();
        assert_eq!(recovered, 0);
        assert_eq!(run_row(&pool, "fresh").await.0, "running");
        assert_eq!(run_row(&pool, "expired-live").await.0, "running");
        assert_eq!(job_row(&pool, "busy").await, (1, None));
        // Past the stale-lease safety net even a live owner is reclaimed.
        let later = now + STALE_LEASE;
        assert_eq!(recover_stale_runs(&pool, later, |_| true).await.unwrap(), 1);
        assert_eq!(run_row(&pool, "expired-live").await.0, "failed");
        assert_eq!(run_row(&pool, "fresh").await.0, "running");
    }

    #[tokio::test]
    async fn recovery_keeps_a_disabled_job_disabled_and_a_newer_next_run() {
        let pool = crate::storage::memory().await.unwrap();
        let now = at("2026-10-26T01:00:00Z");
        let newer = now + TimeDelta::hours(5);
        insert_job(&pool, "paused", 0, None).await;
        insert_running(&pool, "r-paused", "paused", 1, now - TimeDelta::minutes(1)).await;
        insert_job(&pool, "moved", 1, Some(newer)).await;
        insert_running(&pool, "r-moved", "moved", 1, now - TimeDelta::minutes(1)).await;
        assert_eq!(recover_stale_runs(&pool, now, |_| false).await.unwrap(), 2);
        assert_eq!(job_row(&pool, "paused").await, (0, None));
        assert_eq!(job_row(&pool, "moved").await, (1, Some(newer.to_rfc3339())));
        assert_eq!(run_row(&pool, "r-paused").await.0, "failed");
        assert_eq!(run_row(&pool, "r-moved").await.0, "failed");
    }

    #[tokio::test]
    async fn recovery_is_idempotent() {
        let pool = crate::storage::memory().await.unwrap();
        let now = at("2026-10-26T01:00:00Z");
        insert_job(&pool, "job", 1, None).await;
        insert_running(&pool, "orphan", "job", 55197, now - TimeDelta::minutes(1)).await;
        assert_eq!(recover_stale_runs(&pool, now, |_| false).await.unwrap(), 1);
        let first = job_row(&pool, "job").await;
        let later = now + TimeDelta::minutes(1);
        assert_eq!(
            recover_stale_runs(&pool, later, |_| false).await.unwrap(),
            0
        );
        assert_eq!(job_row(&pool, "job").await, first);
        let (_, finished, _) = run_row(&pool, "orphan").await;
        assert_eq!(finished.as_deref(), Some(now.to_rfc3339().as_str()));
    }

    #[tokio::test]
    async fn a_late_finish_after_recovery_changes_nothing() {
        let pool = crate::storage::memory().await.unwrap();
        let now = at("2026-10-26T01:00:00Z");
        insert_job(&pool, "job", 1, None).await;
        insert_running(&pool, "orphan", "job", 60118, now - TimeDelta::minutes(1)).await;
        assert_eq!(recover_stale_runs(&pool, now, |_| false).await.unwrap(), 1);
        // The owner disables the job after recovery; the late finish must not re-enable it.
        sqlx::query("UPDATE scheduled_jobs SET enabled = 0 WHERE id = 'job'")
            .execute(&pool)
            .await
            .unwrap();
        let job_before: (i64, Option<String>, i64, String) = sqlx::query_as(
            "SELECT enabled, next_run_at, consecutive_failures, payload_json FROM scheduled_jobs WHERE id = 'job'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let run_before = run_row(&pool, "orphan").await;
        let accepted = finish_run(&pool, "job", "orphan", true, None, "late", None)
            .await
            .unwrap();
        assert!(!accepted);
        let job_after: (i64, Option<String>, i64, String) = sqlx::query_as(
            "SELECT enabled, next_run_at, consecutive_failures, payload_json FROM scheduled_jobs WHERE id = 'job'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(job_after, job_before);
        assert_eq!(run_row(&pool, "orphan").await, run_before);
        let incidents: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cron_incidents")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(incidents, 0);
    }

    #[tokio::test]
    async fn a_job_disabled_while_running_stays_disabled_after_finish() {
        let pool = crate::storage::memory().await.unwrap();
        let now = Utc::now();
        insert_job(&pool, "job", 0, None).await;
        insert_running(&pool, "run", "job", 1, now + LEASE).await;
        let accepted = finish_run(&pool, "job", "run", true, None, "done", None)
            .await
            .unwrap();
        assert!(accepted);
        assert_eq!(run_row(&pool, "run").await.0, "succeeded");
        assert_eq!(job_row(&pool, "job").await, (0, None));
    }

    #[tokio::test]
    async fn next_wait_wakes_for_a_stale_run_when_no_job_is_scheduled() {
        let pool = crate::storage::memory().await.unwrap();
        let now = at("2026-10-26T01:00:00Z");
        insert_job(&pool, "job", 1, None).await;
        insert_running(&pool, "orphan", "job", 55197, now - TimeDelta::minutes(10)).await;
        let wait = next_wait(&pool, now).await.unwrap();
        assert!(wait < Duration::from_secs(3 * 60 * 60), "{wait:?}");
        assert!(
            wait > Duration::ZERO,
            "a live-owner row must not spin the loop"
        );
        // A lease that has not expired yet wakes when it expires.
        insert_job(&pool, "other", 1, None).await;
        insert_running(&pool, "fresh", "other", 1, now + TimeDelta::minutes(7)).await;
        let wait = next_wait(&pool, now).await.unwrap();
        assert_eq!(wait, Duration::from_secs(7 * 60));
    }

    #[tokio::test]
    async fn renewing_leases_extends_only_running_runs_of_the_batch() {
        let pool = crate::storage::memory().await.unwrap();
        let now = at("2026-10-26T01:00:00Z");
        insert_job(&pool, "job", 1, None).await;
        insert_running(&pool, "mine", "job", 1, now + TimeDelta::minutes(1)).await;
        insert_running(&pool, "other", "job", 2, now + TimeDelta::minutes(1)).await;
        let renewed = renew_leases(&pool, &["mine".to_owned()], now)
            .await
            .unwrap();
        assert_eq!(renewed, 1);
        let lease: String =
            sqlx::query_scalar("SELECT lease_until FROM cron_runs WHERE id = 'mine'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(at(&lease), now + LEASE);
        let other: String =
            sqlx::query_scalar("SELECT lease_until FROM cron_runs WHERE id = 'other'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(at(&other), now + TimeDelta::minutes(1));
    }

    async fn owner_items(pool: &SqlitePool) -> Vec<(String, String, String)> {
        sqlx::query_as("SELECT item_id, status, decided_at FROM inbox_decisions WHERE item_id LIKE 'cron:%' ORDER BY item_id")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn repeat_failures_of_a_job_refresh_one_open_owner_item() {
        let pool = crate::storage::memory().await.unwrap();
        let first = Utc::now() - TimeDelta::minutes(10);
        escalate_incident(&pool, "job", "failed", "boom", first)
            .await
            .unwrap();
        escalate_incident(&pool, "job", "stuck", "again", Utc::now())
            .await
            .unwrap();
        let items = owner_items(&pool).await;
        assert_eq!(
            items,
            vec![(
                cron_owner_item_id("job"),
                "open".to_owned(),
                first.to_rfc3339()
            )]
        );
        let (title, reason): (String, String) = sqlx::query_as(
            "SELECT i.title, d.reason FROM inbox_items i JOIN inbox_decisions d ON d.item_id = i.id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(title, "job: 확인 필요 - stuck: again");
        assert_eq!(reason, "stuck");
    }

    /// Without a remediator, one missing path is one owner item naming every job it fails, and
    /// a job's own clean run does not close it while the path is still missing.
    #[tokio::test]
    async fn an_environment_fault_holds_one_owner_item_for_all_its_jobs() {
        let pool = crate::storage::memory().await.unwrap();
        let root = std::env::temp_dir().join(format!("fx-env-{}", uuid::Uuid::new_v4().simple()));
        let fault = crate::cron_exec::EnvironmentFault {
            missing: crate::cron_exec::MissingKind::WorkingDirectory,
            path: root.to_string_lossy().into_owned(),
        };
        for job in ["alerts", "collect"] {
            sqlx::query("INSERT INTO cron_incidents(id, job_id, kind, detail, status, created_at) VALUES(?, ?, 'failed', ?, 'open', ?)")
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(job)
                .bind(fault.message())
                .bind(Utc::now().to_rfc3339())
                .execute(&pool)
                .await
                .unwrap();
            escalate_incident(&pool, job, "failed", &fault.message(), Utc::now()).await.unwrap();
        }
        let items = owner_items(&pool).await;
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0].0, crate::cron_remediate::environment_item_id(&fault));
        let title: String = sqlx::query_scalar("SELECT title FROM inbox_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(title.contains("크론 2개가 실행되지 못했습니다: alerts, collect"), "{title}");
        assert!(!title.contains("확인 필요 - failed"), "{title}");
        close_cron_owner_items(&pool, "alerts").await.unwrap();
        assert_eq!(owner_items(&pool).await[0].1, "open");
    }

    #[tokio::test]
    async fn an_english_owner_reads_cron_failure_texts_in_english() {
        crate::locale::scope(crate::locale::OwnerLocale::english_utc(), async {
            let pool = crate::storage::memory().await.unwrap();
            escalate_incident(&pool, "job", "failed", "\u{1b}[31mboom\u{1b}[0m", Utc::now()).await.unwrap();
            let title: String = sqlx::query_scalar("SELECT title FROM inbox_items")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(title, "job: needs you - failed: boom");
            sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES('broken','every 1m','{}',1)")
                .execute(&pool)
                .await
                .unwrap();
            note_run_health(&pool, "broken", "run-1", "failed", "", Some("disk full"), &serde_json::json!({}))
                .await
                .unwrap();
            let logged: String = sqlx::query_scalar("SELECT message FROM hub_log")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert!(logged.starts_with("[cron broken] failed. Error: disk full"), "{logged}");
            assert!(!logged.chars().any(crate::locale::is_hangul), "{logged}");
        })
        .await;
    }

    #[tokio::test]
    async fn a_succeeded_run_closes_the_jobs_owner_item() {
        let pool = crate::storage::memory().await.unwrap();
        escalate_incident(&pool, "job", "failed", "boom", Utc::now())
            .await
            .unwrap();
        escalate_incident(&pool, "other", "failed", "boom", Utc::now())
            .await
            .unwrap();
        note_run_health(
            &pool,
            "job",
            "r1",
            "succeeded",
            "ok",
            None,
            &serde_json::json!({}),
        )
        .await
        .unwrap();
        let states: Vec<(String, String)> = owner_items(&pool)
            .await
            .into_iter()
            .map(|(id, state, _)| (id, state))
            .collect();
        assert_eq!(
            states,
            vec![
                (cron_owner_item_id("job"), "done".to_owned()),
                (cron_owner_item_id("other"), "open".to_owned()),
            ]
        );
    }

    async fn incident_states(pool: &SqlitePool) -> Vec<(String, String, bool)> {
        sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT job_id, status, resolved_at FROM cron_incidents ORDER BY job_id, created_at",
        )
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|(job, status, at)| (job, status, at.is_some()))
        .collect()
    }

    async fn seed_failed_job(pool: &SqlitePool, job: &str) {
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES(?, 'every 1h', '{\"command\":\"printf ok\"}', 1)")
            .bind(job)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO cron_runs(id, job_id, status, started_at, output) VALUES(?, ?, 'running', ?, '')")
            .bind(format!("{job}-fail"))
            .bind(job)
            .bind(now_rfc3339())
            .execute(pool)
            .await
            .unwrap();
        finish_run(
            pool,
            job,
            &format!("{job}-fail"),
            false,
            Some("boom"),
            "",
            None,
        )
        .await
        .unwrap();
        note_run_health(
            pool,
            job,
            &format!("{job}-fail"),
            "failed",
            "",
            Some("boom"),
            &serde_json::json!({}),
        )
        .await
        .unwrap();
        sqlx::query("UPDATE cron_problems SET status = 'needs_owner' WHERE job_id = ?")
            .bind(job)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_later_clean_run_resolves_incidents_problems_and_the_owner_item() {
        let pool = crate::storage::memory().await.unwrap();
        seed_failed_job(&pool, "job").await;
        seed_failed_job(&pool, "other").await;
        assert_eq!(
            incident_states(&pool).await,
            vec![
                ("job".to_owned(), "open".to_owned(), false),
                ("other".to_owned(), "open".to_owned(), false),
            ]
        );
        // A run-now through the cron socket is a later run of the same job.
        let receipt = run_native_job(&pool, "job", false, false).await.unwrap();
        assert_eq!(receipt.status, "succeeded");
        assert_eq!(
            incident_states(&pool).await,
            vec![
                ("job".to_owned(), "resolved".to_owned(), true),
                ("other".to_owned(), "open".to_owned(), false),
            ]
        );
        let problems: Vec<(String, String)> =
            sqlx::query_as("SELECT job_id, status FROM cron_problems ORDER BY job_id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            problems,
            vec![
                ("job".to_owned(), "fixed".to_owned()),
                ("other".to_owned(), "needs_owner".to_owned()),
            ]
        );
        let states: Vec<(String, String)> = owner_items(&pool)
            .await
            .into_iter()
            .map(|(id, state, _)| (id, state))
            .collect();
        assert_eq!(
            states,
            vec![
                (cron_owner_item_id("job"), "done".to_owned()),
                (cron_owner_item_id("other"), "open".to_owned()),
            ]
        );
    }

    #[tokio::test]
    async fn the_backlog_of_failures_a_later_success_resolved_is_reconciled_once() {
        let pool = crate::storage::memory().await.unwrap();
        let at = |minutes: i64| (Utc::now() - TimeDelta::minutes(minutes)).to_rfc3339();
        // healed: failed, then succeeded (as the pre-fix build left it). still: succeeded long
        // ago, failing since. never: no success at all.
        for (job, run, status, finished) in [
            ("healed", "h1", "failed", at(60)),
            ("healed", "h2", "succeeded", at(30)),
            ("still", "s1", "succeeded", at(90)),
            ("still", "s2", "failed", at(20)),
            ("never", "n1", "failed", at(20)),
        ] {
            sqlx::query("INSERT INTO cron_runs(id, job_id, status, started_at, finished_at, output) VALUES(?,?,?,?,?, '')")
                .bind(run)
                .bind(job)
                .bind(status)
                .bind(&finished)
                .bind(&finished)
                .execute(&pool)
                .await
                .unwrap();
        }
        for (id, job, created) in [
            ("i1", "healed", at(61)),
            ("i2", "healed", at(59)),
            ("i3", "still", at(100)),
            ("i4", "still", at(21)),
            ("i5", "never", at(21)),
        ] {
            sqlx::query("INSERT INTO cron_incidents(id, job_id, kind, detail, status, created_at) VALUES(?,?, 'failed', 'boom', 'open', ?)")
                .bind(id)
                .bind(job)
                .bind(&created)
                .execute(&pool)
                .await
                .unwrap();
        }
        for (id, job, updated) in [("134", "healed", at(59)), ("135", "still", at(21))] {
            sqlx::query("INSERT INTO cron_problems(id, job_id, summary, created_at, status, kind, signature, detail, updated_at) VALUES(?,?, 'boom', ?, 'needs_owner', 'failed', 'sig', 'boom', ?)")
                .bind(id)
                .bind(job)
                .bind(&updated)
                .bind(&updated)
                .execute(&pool)
                .await
                .unwrap();
        }
        for job in ["healed", "still", "never"] {
            escalate_incident(
                &pool,
                job,
                "failed",
                "boom",
                Utc::now() - TimeDelta::minutes(59),
            )
            .await
            .unwrap();
        }

        assert_eq!(reconcile_resolved_cron_failures(&pool).await.unwrap(), 2);
        assert_eq!(
            reconcile_resolved_cron_failures(&pool).await.unwrap(),
            0,
            "idempotent"
        );

        let incidents: Vec<(String, String)> =
            sqlx::query_as("SELECT id, status FROM cron_incidents ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            incidents,
            vec![
                ("i1".to_owned(), "resolved".to_owned()),
                ("i2".to_owned(), "resolved".to_owned()),
                ("i3".to_owned(), "resolved".to_owned()),
                ("i4".to_owned(), "open".to_owned()),
                ("i5".to_owned(), "open".to_owned()),
            ]
        );
        let problems: Vec<(String, String)> =
            sqlx::query_as("SELECT id, status FROM cron_problems ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            problems,
            vec![
                ("134".to_owned(), "fixed".to_owned()),
                ("135".to_owned(), "needs_owner".to_owned()),
            ]
        );
        let states: Vec<(String, String)> = owner_items(&pool)
            .await
            .into_iter()
            .map(|(id, state, _)| (id, state))
            .collect();
        assert_eq!(
            states,
            vec![
                (cron_owner_item_id("healed"), "done".to_owned()),
                (cron_owner_item_id("never"), "open".to_owned()),
                (cron_owner_item_id("still"), "open".to_owned()),
            ]
        );
    }

    #[tokio::test]
    async fn a_failure_after_a_close_reopens_the_item_and_clears_its_alert() {
        let pool = crate::storage::memory().await.unwrap();
        let obligation = format!(
            "{}{}",
            crate::owner_alert::OBLIGATION_PREFIX,
            cron_owner_item_id("job")
        );
        let first = Utc::now() - TimeDelta::minutes(10);
        escalate_incident(&pool, "job", "failed", "boom", first)
            .await
            .unwrap();
        sqlx::query("INSERT INTO delivery_obligations(id, state, body, updated_at) VALUES(?, 'delivered', 'told', ?)")
            .bind(&obligation)
            .bind(now_rfc3339())
            .execute(&pool)
            .await
            .unwrap();
        note_run_health(
            &pool,
            "job",
            "r1",
            "succeeded",
            "ok",
            None,
            &serde_json::json!({}),
        )
        .await
        .unwrap();
        let later = Utc::now();
        escalate_incident(&pool, "job", "failed", "boom", later)
            .await
            .unwrap();
        assert_eq!(
            owner_items(&pool).await,
            vec![(
                cron_owner_item_id("job"),
                "open".to_owned(),
                later.to_rfc3339()
            )]
        );
        let kept: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM delivery_obligations WHERE id = ?")
                .bind(&obligation)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(kept, 0);
        let alerts = crate::owner_alert::pending_alerts(&pool, Utc::now())
            .await
            .unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].item_id, cron_owner_item_id("job"));
    }

    #[tokio::test]
    async fn reconcile_collapses_legacy_cron_owner_items_once() {
        let pool = crate::storage::memory().await.unwrap();
        for (n, minutes) in [(1, 30_i64), (2, 10), (3, 20)] {
            let at = (Utc::now() - TimeDelta::minutes(minutes)).to_rfc3339();
            let id = format!("cron:legacy-{n}");
            sqlx::query("INSERT INTO inbox_items(id, source, source_ref, author, title, snippet, received_at, raw_hash, collected_at) VALUES(?, 'cron', ?, 'job', ?, ?, ?, 'cron', ?)")
                .bind(&id)
                .bind(&id)
                .bind(format!("job: needs you - failed: boom {n}"))
                .bind(format!("boom {n}"))
                .bind(&at)
                .bind(&at)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO inbox_decisions(item_id, class, reason, proposed_action, status, decided_at) VALUES(?, 'owner', 'failed', ?, 'open', ?)")
                .bind(&id)
                .bind(format!("boom {n}"))
                .bind(&at)
                .execute(&pool)
                .await
                .unwrap();
        }
        assert_eq!(reconcile_cron_owner_items(&pool).await.unwrap(), 3);
        let states: Vec<(String, String)> = owner_items(&pool)
            .await
            .into_iter()
            .map(|(id, state, _)| (id, state))
            .collect();
        assert_eq!(
            states,
            vec![
                (cron_owner_item_id("job"), "open".to_owned()),
                ("cron:legacy-1".to_owned(), "dismissed".to_owned()),
                ("cron:legacy-2".to_owned(), "dismissed".to_owned()),
                ("cron:legacy-3".to_owned(), "dismissed".to_owned()),
            ]
        );
        let (title, action): (String, String) = sqlx::query_as(
            "SELECT i.title, d.proposed_action FROM inbox_items i JOIN inbox_decisions d ON d.item_id = i.id WHERE i.id = ?",
        )
        .bind(cron_owner_item_id("job"))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(title, "job: needs you - failed: boom 2");
        assert_eq!(action, "boom 2");
        let obligation: String =
            sqlx::query_scalar("SELECT state FROM delivery_obligations WHERE id = ?")
                .bind(format!(
                    "{}{}",
                    crate::owner_alert::OBLIGATION_PREFIX,
                    cron_owner_item_id("job")
                ))
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(obligation, "delivered");
        assert!(crate::owner_alert::pending_alerts(&pool, Utc::now())
            .await
            .unwrap()
            .is_empty());
        assert_eq!(reconcile_cron_owner_items(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn reconcile_takes_the_job_of_an_authorless_problem_item_from_its_problem_row() {
        let pool = crate::storage::memory().await.unwrap();
        let at = Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO cron_problems(id, job_id, created_at) VALUES('40', 'job', ?)")
            .bind(&at)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO inbox_items(id, source, source_ref, title, snippet, received_at, raw_hash, collected_at) VALUES('cron:40', 'cron', '40', 'job', 'boom', ?, 'cron', ?)")
            .bind(&at)
            .bind(&at)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO inbox_decisions(item_id, class, reason, status, decided_at) VALUES('cron:40', 'owner', 'failed', 'open', ?)")
            .bind(&at)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(reconcile_cron_owner_items(&pool).await.unwrap(), 1);
        let states: Vec<(String, String)> = owner_items(&pool)
            .await
            .into_iter()
            .map(|(id, state, _)| (id, state))
            .collect();
        assert_eq!(
            states,
            vec![
                ("cron:40".to_owned(), "dismissed".to_owned()),
                (cron_owner_item_id("job"), "open".to_owned()),
            ]
        );
    }
}
