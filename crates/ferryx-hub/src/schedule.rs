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

/// Existing `omo-gateway cron run-now` clients connect to `~/.omon/cron-run-now.sock`.
/// The ferryx names carry the same line protocol.
pub fn native_socket_paths(home: &std::path::Path) -> Vec<std::path::PathBuf> {
    let (cron, run_now) = socket_paths(home);
    vec![home.join(".omon").join("cron-run-now.sock"), cron, run_now]
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
    let outcome =
        crate::cron_exec::run_payload(pool, job_id, &payload, &model, skills.as_deref()).await;
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
        tokio::process::Command::new("sh")
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
    if stored.as_deref() != Some(token) {
        return Ok(failed_receipt(&job_id, "owner token does not match"));
    }
    sqlx::query("UPDATE cron_problems SET status = 'approved' WHERE id = ?")
        .bind(problem_id.to_string())
        .execute(pool)
        .await?;
    run_native_job(pool, &job_id, true, true).await
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
            _ = stop.changed() => {
                if *stop.borrow() {
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
}

pub async fn claim_due(
    pool: &SqlitePool,
    now: DateTime<Utc>,
    owner_pid: u32,
) -> Result<Vec<ClaimedJob>> {
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
        claimed.push(ClaimedJob { id, occurrence });
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

pub async fn finish_run(
    pool: &SqlitePool,
    job_id: &str,
    ok: bool,
    error: Option<&str>,
    output: &str,
    occurrence: Option<&str>,
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
    let (next, consecutive, enabled) = if expression.starts_with("once:") || limit_reached {
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
    let Some(next) = next else {
        return Ok(Duration::from_secs(24 * 60 * 60));
    };
    let when = DateTime::parse_from_rfc3339(&next)
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or(now);
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

fn cron_model() -> crate::meta::backend::UrlModel {
    let url = std::env::var("FERRYX_HUB_OMO_URL")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| crate::cron_exec::OMO_APP_SERVER.to_owned());
    crate::meta::backend::UrlModel::new(url)
}

fn skills_root_from_env() -> Option<std::path::PathBuf> {
    if let Some(root) = std::env::var_os("HERMES_HOME") {
        return Some(std::path::PathBuf::from(root).join("skills"));
    }
    std::env::var_os("HOME").map(|home| {
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
    let mut deliveries = Vec::new();
    for ClaimedJob {
        id: job_id,
        occurrence,
    } in claimed
    {
        let payload_json: Option<String> =
            sqlx::query_scalar("SELECT payload_json FROM scheduled_jobs WHERE id = ?")
                .bind(&job_id)
                .fetch_optional(pool)
                .await?;
        let run_id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM cron_runs WHERE job_id = ? AND status = 'running' ORDER BY started_at DESC LIMIT 1",
        )
        .bind(&job_id)
        .fetch_optional(pool)
        .await?;
        let (Some(payload_json), Some(run_id)) = (payload_json, run_id) else {
            continue;
        };
        let payload: Value = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
        let executed =
            crate::cron_exec::run_payload(pool, &job_id, &payload, model, skills_root).await;
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
        finish_run(
            pool,
            &job_id,
            ok,
            error.as_deref(),
            &output,
            occurrence.as_deref(),
        )
        .await?;
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

async fn note_run_health(
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
        sqlx::query(
            "UPDATE cron_problems SET status = 'fixed', updated_at = ? WHERE job_id = ? AND status IN ('open', 'remediating')",
        )
        .bind(now_rfc3339())
        .bind(job_id)
        .execute(pool)
        .await?;
        return Ok(());
    };
    let signature = problem_signature(kind, &detail);
    if let Some(problem_id) =
        record_problem(pool, job_id, run_id, kind, &signature, &detail).await?
    {
        let failures: i64 =
            sqlx::query_scalar("SELECT consecutive_failures FROM scheduled_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_one(pool)
                .await?;
        let open_lane: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM cron_problems WHERE job_id = ? AND lane_key IS NOT NULL AND status = 'remediating'",
        )
        .bind(job_id)
        .fetch_one(pool)
        .await?;
        if remediation_due(failures as u32, open_lane > 0) {
            open_remediation(pool, job_id, problem_id, &detail).await?;
        }
    }
    Ok(())
}

async fn record_problem(
    pool: &SqlitePool,
    job_id: &str,
    run_id: &str,
    kind: &str,
    signature: &str,
    detail: &str,
) -> Result<Option<i64>> {
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(id AS INTEGER) FROM cron_problems WHERE job_id = ? AND kind = ? AND signature = ?",
    )
    .bind(job_id)
    .bind(kind)
    .bind(signature)
    .fetch_optional(pool)
    .await?;
    if existing.is_some() {
        return Ok(None);
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
    let lead = std::env::var("OMON_LANE_DEFAULT_LEAD")
        .ok()
        .filter(|lead| !lead.trim().is_empty())
        .unwrap_or_else(|| "meta".into());
    crate::lanes::dispatch::enqueue(
        pool,
        job_id,
        &lead,
        "cron_problem",
        &serde_json::json!({"problem_id": id, "job_id": job_id, "kind": kind, "detail": detail})
            .to_string(),
    )
    .await?;
    Ok(Some(id))
}

async fn open_remediation(
    pool: &SqlitePool,
    job_id: &str,
    problem_id: i64,
    detail: &str,
) -> Result<()> {
    let key = remediation_lane_key(job_id);
    let existing: Option<String> = sqlx::query_scalar("SELECT status FROM lanes WHERE key = ?")
        .bind(&key)
        .fetch_optional(pool)
        .await?;
    if existing.is_none() {
        let _ = crate::lanes::state::insert_lane(pool, &key, job_id, "meta", ".", "[]").await;
    }
    let token = uuid::Uuid::new_v4().to_string().replace('-', "");
    sqlx::query(
        "UPDATE cron_problems SET status = 'remediating', lane_key = ?, owner_token = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&key)
    .bind(&token)
    .bind(now_rfc3339())
    .bind(problem_id.to_string())
    .execute(pool)
    .await?;
    let _ = detail;
    Ok(())
}

fn remediation_lane_key(job_id: &str) -> String {
    let key: String = job_id
        .chars()
        .map(|character| match character {
            'a'..='z' | '0'..='9' | '-' => character,
            'A'..='Z' => character.to_ascii_lowercase(),
            _ => '-',
        })
        .collect();
    let key: String = key.trim_matches('-').chars().take(48).collect();
    if key.is_empty() {
        "cron-job".into()
    } else {
        key
    }
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
    let error = post_targets(&targets, text).await;
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

async fn post_targets(targets: &[Delivery], text: &str) -> Option<String> {
    let mut errors = Vec::new();
    for target in targets {
        let posted = match target.platform.as_str() {
            "slack" => post_slack_target(&target.chat_id, target.thread_id.as_deref(), text).await,
            "discord" => post_discord_target(&target.chat_id, text).await,
            other => Err(HubError::message(format!(
                "unconfigured delivery transport: {other}"
            ))),
        };
        match posted {
            Ok(true) => {}
            Ok(false) => errors.push(format!(
                "unconfigured delivery transport: {}",
                target.platform
            )),
            Err(error) => errors.push(error.to_string()),
        }
    }
    if errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    }
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
        finish_run(&pool, "job", false, Some("boom"), "", occurrence.as_deref())
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
        let lane: String =
            sqlx::query_scalar("SELECT lane_key FROM cron_problems WHERE job_id = 'broken'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(lane, "broken");
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
            .bind(r#"{"command":"printf hi","deliver":"origin"}"#)
            .execute(&pool)
            .await
            .unwrap();
        let receipt = run_native_job(&pool, "job-1", false, false).await.unwrap();
        assert_eq!(receipt.status, "succeeded");
        assert_eq!(receipt.output.as_deref(), Some("hi"));
        assert_eq!(
            receipt.last_delivery_error.as_deref(),
            Some("unconfigured delivery transport: slack")
        );
        let stored: String =
            sqlx::query_scalar("SELECT payload_json FROM scheduled_jobs WHERE id = 'job-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(stored.contains("unconfigured delivery transport: slack"));
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
}
