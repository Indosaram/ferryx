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

/// Judge calls and omo fallbacks in `[since, until)`. Rows of the retained mhc supervisor engine
/// (`mhc_` kinds, recorded with `used_fallback = 1`) count as judge calls but never as omo
/// fallback calls.
pub async fn report(
    pool: &SqlitePool,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<Report> {
    let (attempts, fallback_calls, capped): (i64, i64, i64) = sqlx::query_as(
        "SELECT COALESCE(sum(used_fallback=0 OR substr(kind,1,4)='mhc_'),0),
                COALESCE(sum(used_fallback=1 AND substr(kind,1,4)<>'mhc_'),0),
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
    let start = since_start(since, now)?;
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

/// The supervisor engine calls of the last `since` (Nm, Nh, Nd), as [`supervisor_calls`].
pub async fn render_supervisor_calls(
    pool: &SqlitePool,
    since: &str,
    now: DateTime<Utc>,
) -> Result<serde_json::Value> {
    let start = since_start(since, now)?;
    supervisor_calls(pool, start, now + chrono::Duration::seconds(1)).await
}

fn since_start(since: &str, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
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
    now.checked_sub_signed(duration)
        .ok_or_else(|| HubError::message("--since out of range"))
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

/// Separates a supervisor prompt's stable instruction prefix from its per-call delta payload.
/// Everything before the marker is identical across goals and calls of one purpose (so the
/// provider can reuse its cached prefix); everything after it is this call's data.
pub const DELTA_MARKER: &str = "\n<ferryx-delta>\n";

/// Splits `prompt` at [`DELTA_MARKER`]: `(prefix, delta)`. `None` when the prompt has none.
pub fn split_prompt(prompt: &str) -> Option<(&str, &str)> {
    prompt
        .split_once(DELTA_MARKER)
}

/// `prefix + DELTA_MARKER + delta`.
pub fn with_delta(prefix: &str, delta: &str) -> String {
    format!("{prefix}{DELTA_MARKER}{delta}")
}

/// Lowercase hex SHA-256 of `text`.
pub fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Why a supervisor engine call happens. Set by the caller around the call with [`with_cause`]
/// (a wake turn, a goal pass, a conversation, an owner ingest); a call nobody attributed is
/// recorded as `unattributed`, never dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallCause {
    pub cause: String,
    pub purpose: Option<String>,
    pub goal_id: Option<String>,
    pub goal_revision: Option<i64>,
    pub wake_turn_id: Option<i64>,
    /// Event ids merged into this call (a wake batch), or the owner source id.
    pub merged_event_ids: Vec<String>,
}

impl CallCause {
    pub fn new(cause: impl Into<String>) -> Self {
        Self {
            cause: cause.into(),
            ..Self::default()
        }
    }

    pub fn goal(mut self, goal_id: &str, revision: Option<i64>) -> Self {
        self.goal_id = Some(goal_id.to_owned());
        self.goal_revision = revision;
        self
    }

    pub fn events(mut self, ids: impl IntoIterator<Item = String>) -> Self {
        self.merged_event_ids = ids.into_iter().collect();
        self
    }
}

tokio::task_local! {
    static CALL_CAUSE: CallCause;
}

/// Runs `future` with `cause` as the attribution of every supervisor engine call it makes.
pub async fn with_cause<F: std::future::Future>(cause: CallCause, future: F) -> F::Output {
    CALL_CAUSE.scope(cause, future).await
}

/// Runs `future` under the current cause (or `unattributed`) with `purpose` set.
pub async fn with_purpose<F: std::future::Future>(purpose: &str, future: F) -> F::Output {
    let mut cause = current_cause().unwrap_or_else(|| CallCause::new(UNATTRIBUTED));
    cause.purpose = Some(purpose.to_owned());
    CALL_CAUSE.scope(cause, future).await
}

/// The cause of the calling task, if one was set.
pub fn current_cause() -> Option<CallCause> {
    CALL_CAUSE.try_with(Clone::clone).ok()
}

/// Cause of a call no caller attributed.
pub const UNATTRIBUTED: &str = "unattributed";

/// Token counts of one call. `None` is unknown: the provider did not report it (the engine
/// reports an unreported count as zero, so a zero is unknown too). Never summed as zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
}

impl TokenUsage {
    /// Adds one engine `usage` object (`input`, `output`, `cacheRead`, `cacheWrite`); a
    /// missing or zero field leaves the count as it was.
    pub fn add_engine_usage(&mut self, usage: &serde_json::Value) {
        let add = |slot: &mut Option<u64>, key: &str| {
            if let Some(value) = usage.get(key).and_then(serde_json::Value::as_u64) {
                if value > 0 {
                    *slot = Some(slot.unwrap_or(0).saturating_add(value));
                }
            }
        };
        add(&mut self.input, "input");
        add(&mut self.output, "output");
        add(&mut self.cache_read, "cacheRead");
        add(&mut self.cache_write, "cacheWrite");
    }
}

/// One supervisor engine call as recorded in `supervisor_model_calls`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCall {
    pub cause: CallCause,
    pub role: String,
    pub prompt_bytes: usize,
    pub prefix_sha256: Option<String>,
    pub prefix_bytes: Option<usize>,
    pub delta_bytes: Option<usize>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub usage: TokenUsage,
    /// `ok` or the typed fault code.
    pub outcome: String,
    pub ms: u64,
}

impl ModelCall {
    /// A call of `role` with `prompt`, attributed to the calling task's cause.
    pub fn start(role: &str, prompt: &str, cause: Option<CallCause>) -> Self {
        let split = split_prompt(prompt);
        Self {
            cause: cause.unwrap_or_else(|| CallCause::new(UNATTRIBUTED)),
            role: role.to_owned(),
            prompt_bytes: prompt.len(),
            prefix_sha256: split.map(|(prefix, _)| sha256_hex(prefix)),
            prefix_bytes: split.map(|(prefix, _)| prefix.len()),
            delta_bytes: split.map(|(_, delta)| delta.len()),
            provider: None,
            model: None,
            usage: TokenUsage::default(),
            outcome: String::new(),
            ms: 0,
        }
    }
}

fn to_i64(value: Option<u64>) -> Option<i64> {
    value.map(|value| i64::try_from(value).unwrap_or(i64::MAX))
}

/// Persists one supervisor engine call.
pub async fn record_call(pool: &SqlitePool, call: &ModelCall) -> Result<()> {
    let merged = serde_json::to_string(&call.cause.merged_event_ids)
        .map_err(|error| HubError::message(error.to_string()))?;
    let size = |value: usize| i64::try_from(value).unwrap_or(i64::MAX);
    sqlx::query(
        "INSERT INTO supervisor_model_calls(cause, purpose, role, goal_id, goal_revision,
            wake_turn_id, merged_event_ids, prefix_sha256, prefix_bytes, delta_bytes, prompt_bytes,
            provider, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
            outcome, ms, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&call.cause.cause)
    .bind(&call.cause.purpose)
    .bind(&call.role)
    .bind(&call.cause.goal_id)
    .bind(call.cause.goal_revision)
    .bind(call.cause.wake_turn_id)
    .bind(merged)
    .bind(&call.prefix_sha256)
    .bind(call.prefix_bytes.map(size))
    .bind(call.delta_bytes.map(size))
    .bind(size(call.prompt_bytes))
    .bind(&call.provider)
    .bind(&call.model)
    .bind(to_i64(call.usage.input))
    .bind(to_i64(call.usage.output))
    .bind(to_i64(call.usage.cache_read))
    .bind(to_i64(call.usage.cache_write))
    .bind(&call.outcome)
    .bind(i64::try_from(call.ms).unwrap_or(i64::MAX))
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Supervisor engine calls in `[since, until)`, newest last, each with its cause, prefix hash
/// and usage. Unknown counts are `null`; the totals count only known values and say how many
/// calls left each unknown.
pub async fn supervisor_calls(
    pool: &SqlitePool,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<serde_json::Value> {
    use sqlx::Row as _;
    let rows = sqlx::query(
        "SELECT id, cause, purpose, role, goal_id, goal_revision, wake_turn_id, merged_event_ids,
                prefix_sha256, prefix_bytes, delta_bytes, prompt_bytes, provider, model,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, outcome, ms,
                created_at
           FROM supervisor_model_calls
          WHERE julianday(created_at) >= julianday(?) AND julianday(created_at) < julianday(?)
          ORDER BY id",
    )
    .bind(since.to_rfc3339())
    .bind(until.to_rfc3339())
    .fetch_all(pool)
    .await?;
    let mut known = [0i64; 4];
    let mut unknown = [0i64; 4];
    let mut calls = Vec::with_capacity(rows.len());
    for row in &rows {
        let tokens: [Option<i64>; 4] = [
            row.try_get(14)?,
            row.try_get(15)?,
            row.try_get(16)?,
            row.try_get(17)?,
        ];
        for (index, value) in tokens.into_iter().enumerate() {
            match value {
                Some(value) => known[index] += value,
                None => unknown[index] += 1,
            }
        }
        let merged: String = row.try_get(7)?;
        calls.push(serde_json::json!({
            "id": row.try_get::<i64, _>(0)?,
            "cause": row.try_get::<String, _>(1)?,
            "purpose": row.try_get::<Option<String>, _>(2)?,
            "role": row.try_get::<String, _>(3)?,
            "goalId": row.try_get::<Option<String>, _>(4)?,
            "goalRevision": row.try_get::<Option<i64>, _>(5)?,
            "wakeTurnId": row.try_get::<Option<i64>, _>(6)?,
            "mergedEventIds": serde_json::from_str::<serde_json::Value>(&merged).unwrap_or(serde_json::Value::Null),
            "prefixSha256": row.try_get::<Option<String>, _>(8)?,
            "prefixBytes": row.try_get::<Option<i64>, _>(9)?,
            "deltaBytes": row.try_get::<Option<i64>, _>(10)?,
            "promptBytes": row.try_get::<i64, _>(11)?,
            "provider": row.try_get::<Option<String>, _>(12)?,
            "model": row.try_get::<Option<String>, _>(13)?,
            "usage": {"input": tokens[0], "output": tokens[1], "cacheRead": tokens[2], "cacheWrite": tokens[3]},
            "outcome": row.try_get::<String, _>(18)?,
            "ms": row.try_get::<i64, _>(19)?,
            "createdAt": row.try_get::<String, _>(20)?,
        }));
    }
    let field = |index: usize| serde_json::json!({"known": known[index], "callsUnknown": unknown[index]});
    Ok(serde_json::json!({
        "calls": calls,
        "totals": {"input": field(0), "output": field(1), "cacheRead": field(2), "cacheWrite": field(3)},
    }))
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

    #[tokio::test]
    async fn supervisor_engine_rows_are_judge_calls_but_not_omo_fallbacks() {
        let pool = crate::storage::memory().await.unwrap();
        for (kind, fallback) in [
            ("choice", 0),
            ("choice", 1),
            ("mhc_structured", 1),
            ("mhc_choice", 1),
        ] {
            sqlx::query("INSERT INTO judge_calls(id, purpose, kind, ms, outcome, used_fallback, created_at) VALUES(?, 'gap_assess', ?, 1, 'ok', ?, '2026-10-22T01:00:00+00:00')")
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(kind)
                .bind(fallback)
                .execute(&pool)
                .await
                .unwrap();
        }
        let since = Utc.with_ymd_and_hms(2026, 10, 22, 0, 0, 0).unwrap();
        let until = Utc.with_ymd_and_hms(2026, 10, 23, 0, 0, 0).unwrap();
        let report = report(&pool, since, until).await.unwrap();
        assert_eq!(report.judge_calls, 4);
        assert_eq!(report.fallback_calls, 1);
        assert_eq!(report.fallback_rate, 0.25);
        assert!(!report.cap_reached);
    }

    #[tokio::test]
    async fn engine_calls_keep_cause_and_unknown_usage_stays_unknown() {
        let pool = crate::storage::memory().await.unwrap();
        let cause = CallCause::new("wake:dependency_ready")
            .goal("g-1", Some(3))
            .events(["goal:c-1:done".to_owned(), "goal:c-2:done".to_owned()]);
        let prompt = with_delta("stable instruction", "goal g-1 evidence");
        let mut known = ModelCall::start("decision", &prompt, Some(cause.clone()));
        known.usage.add_engine_usage(&serde_json::json!({
            "input": 120, "output": 30, "cacheRead": 100, "cacheWrite": 0
        }));
        known.provider = Some("anthropic".into());
        known.model = Some("claude".into());
        known.outcome = "ok".into();
        record_call(&pool, &known).await.unwrap();
        // A provider that reported nothing: every count unknown, not zero.
        let mut silent = with_cause(cause, async {
            ModelCall::start("decision", "no marker", current_cause())
        })
        .await;
        silent
            .usage
            .add_engine_usage(&serde_json::json!({"input": 0, "output": 0}));
        silent.outcome = "ok".into();
        record_call(&pool, &silent).await.unwrap();
        let unattributed = ModelCall::start("conversation", "x", None);
        assert_eq!(unattributed.cause.cause, UNATTRIBUTED);

        let since = Utc::now() - chrono::Duration::minutes(1);
        let until = Utc::now() + chrono::Duration::minutes(1);
        let report = supervisor_calls(&pool, since, until).await.unwrap();
        let calls = report["calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["cause"], "wake:dependency_ready");
        assert_eq!(calls[0]["goalId"], "g-1");
        assert_eq!(calls[0]["goalRevision"], 3);
        assert_eq!(
            calls[0]["mergedEventIds"],
            serde_json::json!(["goal:c-1:done", "goal:c-2:done"])
        );
        assert_eq!(calls[0]["prefixSha256"], sha256_hex("stable instruction"));
        assert_eq!(calls[0]["usage"]["input"], 120);
        assert_eq!(calls[0]["usage"]["cacheRead"], 100);
        assert!(calls[0]["usage"]["cacheWrite"].is_null());
        assert!(calls[1]["usage"]["input"].is_null());
        assert!(calls[1]["prefixSha256"].is_null());
        assert_eq!(calls[1]["cause"], "wake:dependency_ready");
        assert_eq!(report["totals"]["input"]["known"], 120);
        assert_eq!(report["totals"]["input"]["callsUnknown"], 1);
        assert_eq!(report["totals"]["cacheWrite"]["callsUnknown"], 2);
    }
}
