//! Delivers lane events that have not been handed to a lead yet.
//! This is the queue, not an in-gateway model loop.
use sqlx::SqlitePool;

use crate::chat::session::SessionKey;
use crate::error::Result;
use crate::storage::{now_rfc3339, WriteTx};

/// How long an admitted lane event suppresses an identical one for the same lead (2026-10-27:
/// the same login alert, cron failure and document request each cost a turn two or three
/// times within a few hours).
const REPEAT_WINDOW_HOURS: i64 = 6;

/// omo's batch window for ordinary events: a group is admitted once its oldest event is this
/// old, and a target gets at most one delivery per window. The window is anchored on the
/// oldest event, so continuous traffic never extends it. The supervisor's goal wakes
/// (`supervisor::wakes`) use the same policy.
pub(crate) const BATCH_WINDOW_SECS: i64 = 30;

/// True once a group whose oldest event arrived at `oldest` may be admitted at `now`.
pub(crate) fn batch_window_elapsed(
    oldest: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    (now - oldest).num_seconds() >= BATCH_WINDOW_SECS
}

/// True while a target's last delivery at `last` is within the batch window of `now`.
pub(crate) fn delivered_recently(
    last: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    last.is_some_and(|last| (now - last).num_seconds() < BATCH_WINDOW_SECS)
}

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

/// Lane event kind of a report a supervisor goal's own worker sent ([`report_from_worker`]).
pub const WORKER_REPORT: &str = "worker_report";
/// Longest worker report id and body.
const MAX_REPORT_ID_CHARS: usize = 256;
const MAX_REPORT_BODY_CHARS: usize = 4_000;

/// What [`report_from_worker`] recorded.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerReport {
    pub goal_id: String,
    pub lane_key: String,
    pub lane_event_id: i64,
    /// The wake's event id (`lane_event:<id>`).
    pub wake_event_id: String,
    /// The same report id was already recorded for this lane; nothing new was written.
    pub duplicate: bool,
}

/// Why a worker report was refused. Nothing was written.
#[derive(Debug, thiserror::Error)]
pub enum WorkerReportError {
    #[error("INVALID_REQUEST: {0}")]
    Invalid(String),
    #[error("NOT_FOUND: session {session_id} on host {host_id} is not the worker of an open supervisor goal")]
    NotBound { host_id: String, session_id: String },
    #[error(transparent)]
    Store(#[from] crate::error::HubError),
}

impl WorkerReportError {
    pub fn code(&self) -> &'static str {
        match self {
            WorkerReportError::Invalid(_) => "INVALID_REQUEST",
            WorkerReportError::NotBound { .. } => "NOT_FOUND",
            WorkerReportError::Store(_) => "STORAGE",
        }
    }
}

/// An ordinary report from the worker bound to a supervisor goal: one `worker_report` lane
/// event on that goal's own lane, handed at once to the goal's wake service through
/// [`hand_to_supervisor`], the one path every supervisor lane event takes. The lane is never
/// the caller's choice: it is the lane of the open goal whose execution binding names
/// `session_id` on `host_id`. `event_id` is the worker's own report id; the same id again (a
/// retry, or an earlier report replayed out of order) is the same lane event and so the same
/// wake. Status only: a report never moves the goal, and the kind is fixed, so a worker cannot
/// raise an owner-urgent notice.
pub async fn report_from_worker(
    pool: &SqlitePool,
    host_id: &str,
    session_id: &str,
    event_id: &str,
    body: &str,
) -> std::result::Result<WorkerReport, WorkerReportError> {
    let id_ok = |value: &str| {
        !value.is_empty()
            && value.chars().count() <= MAX_REPORT_ID_CHARS
            && value.trim() == value
            && !value.chars().any(char::is_control)
    };
    if !id_ok(session_id) || !id_ok(event_id) {
        return Err(WorkerReportError::Invalid(format!(
            "sessionId and eventId must be 1-{MAX_REPORT_ID_CHARS} printable characters"
        )));
    }
    if body.trim().is_empty() || body.chars().count() > MAX_REPORT_BODY_CHARS {
        return Err(WorkerReportError::Invalid(format!(
            "body must be 1-{MAX_REPORT_BODY_CHARS} characters"
        )));
    }
    let bound: Option<(String, String)> = sqlx::query_as(
        "SELECT g.goal_id, g.lane_key FROM supervisor_execution_bindings b
           JOIN supervisor_goals g ON g.goal_id = b.goal_id
          WHERE b.host_id = ? AND b.provider_session_id = ?
            AND g.run_state NOT IN ('completed','cancelled')
          LIMIT 1",
    )
    .bind(host_id)
    .bind(session_id)
    .fetch_optional(pool)
    .await
    .map_err(crate::error::HubError::from)?;
    let Some((goal_id, lane_key)) = bound else {
        return Err(WorkerReportError::NotBound {
            host_id: host_id.to_owned(),
            session_id: session_id.to_owned(),
        });
    };
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at, source_event_id)
         VALUES(?,?,?,?,?,?)
         ON CONFLICT(lane_key, source_event_id) WHERE source_event_id IS NOT NULL DO NOTHING
         RETURNING id",
    )
    .bind(&lane_key)
    .bind(format!("supervisor:{goal_id}"))
    .bind(WORKER_REPORT)
    .bind(body)
    .bind(now_rfc3339())
    .bind(event_id)
    .fetch_optional(pool)
    .await
    .map_err(crate::error::HubError::from)?;
    let duplicate = inserted.is_none();
    let (id, created_at, delivered_at): (i64, String, Option<String>) = sqlx::query_as(
        "SELECT id, created_at, delivered_at FROM lane_events
          WHERE lane_key = ? AND source_event_id = ?",
    )
    .bind(&lane_key)
    .bind(event_id)
    .fetch_one(pool)
    .await
    .map_err(crate::error::HubError::from)?;
    // A first report, or a replay of one whose hand-over did not finish, is handed over now;
    // one already handed over is not touched again.
    if delivered_at.is_none() {
        let now = chrono::Utc::now();
        let created = chrono::DateTime::parse_from_rfc3339(&created_at)
            .map(|at| at.with_timezone(&chrono::Utc))
            .unwrap_or(now);
        hand_to_supervisor(pool, id, Some(&lane_key), false, created, &now.to_rfc3339()).await?;
    }
    Ok(WorkerReport {
        goal_id,
        lane_key,
        lane_event_id: id,
        wake_event_id: format!("lane_event:{id}"),
        duplicate,
    })
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

/// One undelivered event as omo's `EventDispatcher` reads it.
#[derive(Debug, Clone)]
struct QueuedEvent {
    id: i64,
    lane_key: Option<String>,
    lead: String,
    kind: String,
    body: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// omo-gateway's `EventDispatcher`, as one tick. Events urgent for the owner stay on the lead
/// (the owner DM); routine ones go to the home channel session. A group is admitted when its
/// oldest event is 30s old (urgent kinds at once), the target had no delivery in the last 30s
/// and `admit` accepts the turn (false while the session is busy). Events are collapsed per
/// (lane, kind) to the newest body, non-critical ones pass the wake judgment, and the rest are
/// receipted. An event whose lead is not a session key (the old `meta` lead) can never be
/// delivered and is receipted as misrouted.
pub async fn dispatch_tick<A, F>(
    pool: &SqlitePool,
    judge: &crate::judge::Judge,
    now: chrono::DateTime<chrono::Utc>,
    mut admit: A,
) -> Result<usize>
where
    A: FnMut(SessionKey, String) -> F,
    F: std::future::Future<Output = bool>,
{
    let stamp = now.to_rfc3339();
    let rows: Vec<(i64, Option<String>, String, String, String, String)> = sqlx::query_as(
        "SELECT id, lane_key, lead_session_key, kind, body, created_at FROM lane_events
         WHERE delivered_at IS NULL AND (release_at IS NULL OR release_at <= ?)
         ORDER BY id",
    )
    .bind(&stamp)
    .fetch_all(pool)
    .await?;
    let mut misrouted = Vec::new();
    let mut groups = std::collections::BTreeMap::<String, Vec<QueuedEvent>>::new();
    for (id, lane_key, lead, kind, body, created_at) in rows {
        if kind == "cron_problem" {
            // One bad cron problem must not block every other lead's events.
            let taken = crate::cron_remediate::handle_event(&body, &lead).await;
            if let Err(error) = &taken {
                tracing::warn!(event_id = id, %error, "cron problem handling failed; event remains queued");
            }
            match cron_disposition(taken) {
                CronDisposition::Receipt => {
                    receipt(pool, &[id], &stamp, "cron_remediation").await?;
                    continue;
                }
                CronDisposition::Keep => continue,
                CronDisposition::Continue => {}
            }
        }
        let parsed_at = chrono::DateTime::parse_from_rfc3339(&created_at)
            .map(|at| at.with_timezone(&chrono::Utc))
            .unwrap_or(now);
        // A supervisor-owned lane reports to its goal's wake service, never to a second lead;
        // an owner-urgent kind also keeps its owner DM notice. A failed hand-over stays queued
        // without blocking other events; an owner-urgent one is still delivered to the owner.
        let owner_urgent = owner_dm_worthy(&kind, &body);
        let handed = hand_to_supervisor(
            pool,
            id,
            lane_key.as_deref(),
            owner_urgent,
            parsed_at,
            &stamp,
        )
        .await;
        match handed {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) if owner_urgent => {
                tracing::warn!(
                    event_id = id,
                    %error,
                    "supervisor wake was not queued; the owner-urgent event is delivered anyway"
                );
            }
            Err(error) => {
                tracing::warn!(
                    event_id = id,
                    %error,
                    "supervisor wake was not queued; event remains queued"
                );
                continue;
            }
        }
        let Ok(lead_key) = SessionKey::from_storage_key(&lead) else {
            misrouted.push(id);
            continue;
        };
        let created_at = parsed_at;
        let event = QueuedEvent {
            id,
            lane_key,
            lead,
            kind,
            body,
            created_at,
        };
        let target = if owner_dm_worthy(&event.kind, &event.body) {
            event.lead.clone()
        } else {
            match super::lead::report_target(pool, &lead_key).await {
                Some(report) => report.storage_key(),
                None => event.lead.clone(),
            }
        };
        groups.entry(target).or_default().push(event);
    }
    if !misrouted.is_empty() {
        tracing::warn!(
            events = misrouted.len(),
            "lane events addressed to a lead that is not a session key were retired"
        );
        receipt(pool, &misrouted, &stamp, "misrouted").await?;
    }
    let mut admitted = 0;
    for (target, events) in groups {
        let Some(oldest) = events.iter().map(|event| event.created_at).min() else {
            continue;
        };
        if !batch_window_elapsed(oldest, now)
            && !events.iter().any(|event| {
                matches!(
                    event.kind.as_str(),
                    "done_claimed" | "dead" | "kibitzer" | "cron_report"
                )
            })
        {
            continue;
        }
        let last: Option<String> = sqlx::query_scalar(
            "SELECT MAX(delivered_at) FROM lane_events WHERE lead_session_key = ? AND delivered_at IS NOT NULL AND (delivery_reason IS NULL OR delivery_reason NOT IN ('jev_skip', 'duplicate', 'supervisor_wake'))",
        )
        .bind(&target)
        .fetch_one(pool)
        .await?;
        let last = last
            .and_then(|last| chrono::DateTime::parse_from_rfc3339(&last).ok())
            .map(|last| last.with_timezone(&chrono::Utc));
        let recent = delivered_recently(last, now);
        if recent {
            continue;
        }
        let Ok(session) = SessionKey::from_storage_key(&target) else {
            continue;
        };
        let mut collapsed =
            std::collections::BTreeMap::<(Option<String>, String), (QueuedEvent, Vec<i64>)>::new();
        for event in events {
            let entry = collapsed
                .entry((event.lane_key.clone(), event.kind.clone()))
                .or_insert_with(|| (event.clone(), Vec::new()));
            entry.1.push(event.id);
            if event.id >= entry.0.id {
                entry.0 = event;
            }
        }
        let mut selected = Vec::new();
        for (event, ids) in collapsed.into_values() {
            if suppress_repeat(pool, &event, &ids, now).await? {
                tracing::info!(event_id = event.id, kind = %event.kind, "lane event already admitted within the repeat window; suppressed");
                continue;
            }
            if !critical(&event.kind, &event.body) {
                let state = format!(
                    "{} {}: {}",
                    event.lane_key.as_deref().unwrap_or("-"),
                    event.kind,
                    event.body
                );
                match judge
                    .noul(
                        crate::judge::Purpose::EventWake,
                        &state,
                        "Does this event need the lead agent's judgment now?",
                    )
                    .await
                {
                    Ok(score) if score < 0.5 => {
                        receipt(pool, &ids, &stamp, "jev_skip").await?;
                        continue;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, event_id = event.id, "event wake judgment unavailable; retaining event");
                        continue;
                    }
                }
            }
            selected.push((event, ids));
        }
        selected.sort_by_key(|(event, _)| event.id);
        let ids: Vec<i64> = selected
            .iter()
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect();
        if ids.is_empty() {
            continue;
        }
        let lines = selected
            .iter()
            .map(|(event, _)| {
                let body: String = event
                    .body
                    .chars()
                    .take(600)
                    .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
                    .collect();
                format!(
                    "{} {}: {}",
                    event.lane_key.as_deref().unwrap_or("-"),
                    event.kind,
                    body
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !admit(session, format!("[LANE_EVENT]\n{lines}")).await {
            continue;
        }
        tracing::info!(lead_session_key = %target, events = ids.len(), "lane event admitted to lead turn");
        receipt(pool, &ids, &stamp, "lead_turn").await?;
        admitted += 1;
    }
    Ok(admitted)
}

/// The identity of a lane event's fact: its body with whitespace collapsed, case folded and
/// clock readings masked, so a re-check of the same alert at a later time matches while an
/// event about a different item (other ids, other wording) does not.
fn normalized_body(body: &str) -> String {
    static CLOCK: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let clock = CLOCK.get_or_init(|| {
        regex::Regex::new(
            r"(\d{4}-\d{2}-\d{2}[T ])?\d{1,2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:?\d{2})?",
        )
        .expect("clock pattern compiles")
    });
    let words = body.split_whitespace().collect::<Vec<_>>().join(" ");
    clock.replace_all(&words.to_lowercase(), "<t>").into_owned()
}

/// Receipts `ids` as `duplicate` when an event with the same lead, lane, kind and normalized
/// body was admitted to a lead turn within `REPEAT_WINDOW_HOURS` of `now`. The lookup and the
/// receipt share one write transaction so two ticks cannot both admit the same fact.
async fn suppress_repeat(
    pool: &SqlitePool,
    event: &QueuedEvent,
    ids: &[i64],
    now: chrono::DateTime<chrono::Utc>,
) -> Result<bool> {
    let since = (now - chrono::Duration::hours(REPEAT_WINDOW_HOURS)).to_rfc3339();
    let mut tx = pool.begin_write().await?;
    let prior: Vec<String> = sqlx::query_scalar(
        "SELECT body FROM lane_events
         WHERE lead_session_key = ? AND lane_key IS ? AND kind = ?
           AND delivery_reason = 'lead_turn' AND delivered_at IS NOT NULL
           AND julianday(delivered_at) >= julianday(?)",
    )
    .bind(&event.lead)
    .bind(&event.lane_key)
    .bind(&event.kind)
    .bind(&since)
    .fetch_all(&mut *tx)
    .await?;
    let identity = normalized_body(&event.body);
    if !prior.iter().any(|body| normalized_body(body) == identity) {
        tx.commit().await?;
        return Ok(false);
    }
    let stamp = now.to_rfc3339();
    for id in ids {
        sqlx::query(
            "UPDATE lane_events SET delivered_at = ?, delivery_reason = 'duplicate' WHERE id = ? AND delivered_at IS NULL",
        )
        .bind(&stamp)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(true)
}

async fn receipt(pool: &SqlitePool, ids: &[i64], stamp: &str, reason: &str) -> Result<()> {
    for id in ids {
        sqlx::query(
            "UPDATE lane_events SET delivered_at = ?, delivery_reason = ? WHERE id = ? AND delivered_at IS NULL",
        )
        .bind(stamp)
        .bind(reason)
        .bind(id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Delivery reason of a lane event handed to its supervisor goal's wake service.
pub const SUPERVISOR_WAKE: &str = "supervisor_wake";

/// The supervisor goal that owns `lane_key`, if any.
async fn supervisor_goal_of_lane(pool: &SqlitePool, lane_key: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT goal_id FROM supervisor_goals WHERE lane_key = ? LIMIT 1")
            .bind(lane_key)
            .fetch_optional(pool)
            .await?,
    )
}

/// A report from a lane a supervisor goal owns goes to that goal's wake service
/// ([`crate::supervisor::wakes::report`]) instead of a lead turn: one goal/wake path, batched
/// by the same 30s policy. The wake is keyed by the lane event id, so a re-run of this tick
/// after a crash before the receipt is the same wake. True when the event was handed over
/// and receipted; an `owner_urgent` event is woken but left for the owner DM (false).
async fn hand_to_supervisor(
    pool: &SqlitePool,
    id: i64,
    lane_key: Option<&str>,
    owner_urgent: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    stamp: &str,
) -> Result<bool> {
    let Some(lane_key) = lane_key else {
        return Ok(false);
    };
    let Some(goal_id) = supervisor_goal_of_lane(pool, lane_key).await? else {
        return Ok(false);
    };
    crate::supervisor::wakes::report(pool, &goal_id, &format!("lane_event:{id}"), created_at)
        .await?;
    if owner_urgent {
        return Ok(false);
    }
    receipt(pool, &[id], stamp, SUPERVISOR_WAKE).await?;
    Ok(true)
}

/// What a `cron_problem` event does inside one tick. `Ok(true)` from the remediator is a
/// receipt; `Ok(false)` (no remediator) joins the ordinary groups; an error stays queued.
fn cron_disposition(result: Result<bool>) -> CronDisposition {
    match result {
        Ok(true) => CronDisposition::Receipt,
        Ok(false) => CronDisposition::Continue,
        Err(_) => CronDisposition::Keep,
    }
}

enum CronDisposition {
    Receipt,
    Continue,
    Keep,
}

/// omo: only urgent events wake a turn in the owner DM ("DM으로는 급한 용무만").
fn owner_dm_worthy(kind: &str, body: &str) -> bool {
    match kind {
        "inbox_urgent" | "owner_reply_failed" | "dead" | "launch_failed" | "startup_error" => true,
        // The kibitzer already judged that the owner should act now.
        "kibitzer" => true,
        "cron_problem" => body.contains("needs_owner"),
        _ => false,
    }
}

/// omo: kinds that must reach the lead without the wake judgment.
fn critical(kind: &str, body: &str) -> bool {
    match kind {
        "done_claimed" | "dead" | "not_done" | "inbox_urgent" | "owner_reply_failed"
        | "cron_report" | "unmerged_branch" | "launch_failed" | "startup_error" => true,
        "action_loop" | "no_progress" | "kibitzer" => true,
        "cron_problem" => body.contains("needs_owner"),
        _ => false,
    }
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

    #[tokio::test]
    async fn lane_events_reach_the_home_channel_or_the_owner_dm_like_omo() {
        let pool = crate::storage::memory().await.unwrap();
        let dm = SessionKey::new("slack", None::<String>, "D0DM", None::<String>, "U0OWNER")
            .with_bot_id("U0BOT")
            .storage_key();
        let old = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        for (lead, kind) in [
            (dm.as_str(), "action_loop"),
            (dm.as_str(), "action_loop"),
            (dm.as_str(), "inbox_urgent"),
            ("meta", "action_loop"),
        ] {
            sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES(NULL, ?, ?, 'item', ?)")
                .bind(lead)
                .bind(kind)
                .bind(&old)
                .execute(&pool)
                .await
                .unwrap();
        }
        let judge = crate::judge::Judge::forced("no");
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
        let record = seen.clone();
        let admitted = dispatch_tick(&pool, &judge, chrono::Utc::now(), move |key, text| {
            record.lock().unwrap().push((key.storage_key(), text));
            async { true }
        })
        .await
        .unwrap();
        // No home channel is configured in tests, so routine events stay on the DM lead too;
        // the DM group carries both kinds, collapsed to one action_loop line.
        assert_eq!(admitted, 1);
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen[0].0, dm);
        assert!(seen[0].1.starts_with("[LANE_EVENT]\n"));
        assert_eq!(seen[0].1.matches("action_loop").count(), 1);
        assert!(seen[0].1.contains("inbox_urgent"));
        let open: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE delivered_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            open, 0,
            "the misrouted meta event is retired, the rest receipted"
        );
    }

    #[tokio::test]
    async fn a_busy_lead_keeps_its_events_queued() {
        let pool = crate::storage::memory().await.unwrap();
        let dm = SessionKey::new("slack", None::<String>, "D0DM", None::<String>, "U0OWNER")
            .with_bot_id("U0BOT")
            .storage_key();
        let old = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES(NULL, ?, 'action_loop', 'item', ?)")
            .bind(&dm)
            .bind(&old)
            .execute(&pool)
            .await
            .unwrap();
        let judge = crate::judge::Judge::forced("yes");
        let admitted = dispatch_tick(&pool, &judge, chrono::Utc::now(), |_, _| async { false })
            .await
            .unwrap();
        assert_eq!(admitted, 0);
        let open: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE delivered_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(open, 1);
    }

    #[test]
    fn a_handled_cron_problem_is_receipted_and_a_failure_stays_queued() {
        assert!(matches!(
            cron_disposition(Ok(true)),
            CronDisposition::Receipt
        ));
        assert!(matches!(
            cron_disposition(Ok(false)),
            CronDisposition::Continue
        ));
        assert!(matches!(
            cron_disposition(Err(crate::error::HubError::message("down"))),
            CronDisposition::Keep
        ));
    }

    #[tokio::test]
    async fn events_wait_thirty_seconds_then_one_turn_and_a_low_score_is_skipped() {
        let pool = crate::storage::memory().await.unwrap();
        let dm = SessionKey::new("slack", None::<String>, "D0DM", None::<String>, "U0OWNER")
            .with_bot_id("U0BOT")
            .storage_key();
        let now = chrono::Utc::now();
        let recent = (now - chrono::Duration::seconds(10)).to_rfc3339();
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES('lane-a', ?, 'question', 'old', ?)")
            .bind(&dm)
            .bind(&recent)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES('lane-a', ?, 'question', 'latest', ?)")
            .bind(&dm)
            .bind(&recent)
            .execute(&pool)
            .await
            .unwrap();
        let judge = crate::judge::Judge::forced_noul(0.9);
        let admitted = dispatch_tick(&pool, &judge, now, |_, _| async { true })
            .await
            .unwrap();
        assert_eq!(admitted, 0, "younger than 30s, so the group waits");
        let later = now + chrono::Duration::seconds(31);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let record = seen.clone();
        let admitted = dispatch_tick(&pool, &judge, later, move |_, text| {
            *record.lock().unwrap() = text;
            async { true }
        })
        .await
        .unwrap();
        assert_eq!(admitted, 1);
        let text = seen.lock().unwrap().clone();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("latest"));
        assert!(!text.contains("old"));
        let reason: String = sqlx::query_scalar(
            "SELECT delivery_reason FROM lane_events WHERE kind = 'question' ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(reason, "lead_turn");

        let quiet = (later - chrono::Duration::seconds(40)).to_rfc3339();
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES('lane-b', ?, 'question', 'routine', ?)")
            .bind(&dm)
            .bind(&quiet)
            .execute(&pool)
            .await
            .unwrap();
        let low = crate::judge::Judge::forced_noul(0.49);
        let admitted = dispatch_tick(
            &pool,
            &low,
            later + chrono::Duration::seconds(31),
            |_, _| async { true },
        )
        .await
        .unwrap();
        assert_eq!(admitted, 0);
        let skip: String =
            sqlx::query_scalar("SELECT delivery_reason FROM lane_events WHERE lane_key = 'lane-b'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(skip, "jev_skip");
    }

    #[tokio::test]
    async fn a_cron_problem_without_a_remediator_still_reaches_the_lead() {
        let pool = crate::storage::memory().await.unwrap();
        let dm = SessionKey::new("slack", None::<String>, "D0DM", None::<String>, "U0OWNER")
            .with_bot_id("U0BOT")
            .storage_key();
        let old = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES('job', ?, 'cron_problem', ?, ?)")
            .bind(&dm)
            .bind(r#"{"problem_id":1,"kind":"needs_owner"}"#)
            .bind(&old)
            .execute(&pool)
            .await
            .unwrap();
        let judge = crate::judge::Judge::forced_noul(0.0);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let record = seen.clone();
        let admitted = dispatch_tick(&pool, &judge, chrono::Utc::now(), move |_, text| {
            *record.lock().unwrap() = text;
            async { true }
        })
        .await
        .unwrap();
        assert_eq!(admitted, 1);
        assert!(seen.lock().unwrap().contains("needs_owner"));
        let reason: String = sqlx::query_scalar("SELECT delivery_reason FROM lane_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(reason, "lead_turn");
    }

    #[tokio::test]
    async fn a_kibitzer_wake_reaches_the_owner_dm_without_a_second_judgment() {
        let pool = crate::storage::memory().await.unwrap();
        let dm = SessionKey::new("slack", None::<String>, "D0DM", None::<String>, "U0OWNER")
            .with_bot_id("U0BOT")
            .storage_key();
        let now = chrono::Utc::now();
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES(NULL, ?, 'kibitzer', ?, ?)")
            .bind(&dm)
            .bind(r#"{"wake":true,"reason":"r","suggested":"sign the contract","target_lane":null}"#)
            .bind(now.to_rfc3339())
            .execute(&pool)
            .await
            .unwrap();
        // A wake judgment of 0.0 would skip any non-critical event.
        let judge = crate::judge::Judge::forced_noul(0.0);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
        let record = seen.clone();
        let admitted = dispatch_tick(&pool, &judge, now, move |key, text| {
            record.lock().unwrap().push((key.storage_key(), text));
            async { true }
        })
        .await
        .unwrap();
        assert_eq!(admitted, 1);
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, dm);
        assert!(seen[0].1.contains("kibitzer"));
        assert!(seen[0].1.contains("sign the contract"));
        let reason: String = sqlx::query_scalar("SELECT delivery_reason FROM lane_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(reason, "lead_turn");
    }

    #[test]
    fn a_repeat_check_ignores_clock_readings_and_spacing_but_not_ids() {
        assert_eq!(
            normalized_body("cron  backup failed at 04:26:01"),
            normalized_body("Cron backup failed at 05:33:12")
        );
        assert_ne!(
            normalized_body("inbox:12 due"),
            normalized_body("inbox:13 due")
        );
    }

    #[tokio::test]
    async fn an_identical_event_within_the_window_is_admitted_once() {
        let pool = crate::storage::memory().await.unwrap();
        let dm = SessionKey::new("slack", None::<String>, "D0DM", None::<String>, "U0OWNER")
            .with_bot_id("U0BOT")
            .storage_key();
        let start = chrono::DateTime::parse_from_rfc3339("2026-10-27T03:36:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let insert = |body: &'static str, at: chrono::DateTime<chrono::Utc>| {
            let pool = pool.clone();
            let dm = dm.clone();
            async move {
                sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES(NULL, ?, 'inbox_urgent', ?, ?)")
                    .bind(&dm)
                    .bind(body)
                    .bind(at.to_rfc3339())
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        };
        // inbox_urgent is critical, so the wake judgment never runs.
        let judge = crate::judge::Judge::forced_noul(0.0);
        let alert = "Tumblbug new login SMS at 03:35";

        insert(alert, start - chrono::Duration::minutes(1)).await;
        let first = dispatch_tick(&pool, &judge, start, |_, _| async { true })
            .await
            .unwrap();
        assert_eq!(first, 1);

        let again = start + chrono::Duration::minutes(52);
        insert(
            "Tumblbug new login  SMS at 04:27",
            again - chrono::Duration::minutes(1),
        )
        .await;
        let repeat = dispatch_tick(&pool, &judge, again, |_, _| async { true })
            .await
            .unwrap();
        assert_eq!(repeat, 0, "the same alert inside the window is suppressed");
        let reason: String =
            sqlx::query_scalar("SELECT delivery_reason FROM lane_events ORDER BY id DESC LIMIT 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(reason, "duplicate");

        let other = again + chrono::Duration::minutes(5);
        insert(
            "KAIT document request",
            other - chrono::Duration::minutes(1),
        )
        .await;
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let record = seen.clone();
        let different = dispatch_tick(&pool, &judge, other, move |_, text| {
            *record.lock().unwrap() = text;
            async { true }
        })
        .await
        .unwrap();
        assert_eq!(different, 1, "a different event is still admitted");
        assert!(seen.lock().unwrap().contains("KAIT"));

        let expired = start + chrono::Duration::hours(REPEAT_WINDOW_HOURS + 1);
        insert(alert, expired - chrono::Duration::minutes(1)).await;
        let after = dispatch_tick(&pool, &judge, expired, |_, _| async { true })
            .await
            .unwrap();
        assert_eq!(after, 1, "past the window the alert is admitted again");
    }

    #[tokio::test]
    async fn a_supervisor_lane_report_becomes_a_goal_wake_not_a_lead_turn() {
        use crate::supervisor::admission::{
            submit_work, ActorContext, OperationClass, OwnerChoices, SubmitOutcome, SubmitWork,
        };
        use crate::supervisor::{
            bind_project_host, create_project, ensure_identity, pending_wakes, ChannelProvider,
            WakeCause,
        };
        let pool = crate::storage::memory().await.unwrap();
        let owner = "slack:T1:U-owner";
        let identity = ensure_identity(&pool, owner).await.unwrap();
        let project = create_project(
            &pool,
            &identity.supervisor_id,
            "ferryx",
            "github.com/x/ferryx",
        )
        .await
        .unwrap();
        bind_project_host(&pool, &project.project_id, "host-a", "/home/o/ferryx/.git")
            .await
            .unwrap();
        let actor = ActorContext {
            principal: owner.into(),
            source_adapter: ChannelProvider::Slack,
            source_message_id: "D1/1.1".into(),
            source_body: "Create result.txt containing READY.".into(),
            channel_binding_id: None,
            choices: OwnerChoices::default(),
        };
        let work = SubmitWork {
            project_id: None,
            host_id: None,
            kind: None,
            title: "Write result.txt".into(),
            instruction: "Create result.txt containing READY.".into(),
            requirements: vec![crate::goals::contract::RequirementSpec {
                id: "r1".into(),
                text: "result.txt contains READY".into(),
                verifier: "verify.sh exits 0".into(),
            }],
            cwd: None,
            operations: OperationClass::BASELINE.to_vec(),
            turn_limit: None,
        };
        let SubmitOutcome::Admitted(admitted) = submit_work(&pool, &actor, &work).await.unwrap()
        else {
            panic!("expected an admitted goal");
        };
        let goal_id = admitted.receipt.goal_id.clone();
        let lane_key = admitted.receipt.lane_key.clone();
        let lead = SessionKey::new("slack", None::<String>, "D1", None::<String>, "U-owner")
            .with_bot_id("UBOT")
            .storage_key();
        let old = chrono::Utc::now() - chrono::Duration::minutes(5);
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at)
             VALUES(?, ?, 'done_claimed', 'finished', ?) RETURNING id",
        )
        .bind(&lane_key)
        .bind(&lead)
        .bind(old.to_rfc3339())
        .fetch_one(&pool)
        .await
        .unwrap();
        let judge = crate::judge::Judge::forced("yes");
        let turns = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = turns.clone();
        let admitted_turns = dispatch_tick(&pool, &judge, chrono::Utc::now(), move |_, _| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async { true }
        })
        .await
        .unwrap();
        assert_eq!(
            admitted_turns, 0,
            "no lead turn for a supervisor-owned lane"
        );
        assert_eq!(turns.load(std::sync::atomic::Ordering::SeqCst), 0);
        let reason: Option<String> =
            sqlx::query_scalar("SELECT delivery_reason FROM lane_events WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(reason.as_deref(), Some(SUPERVISOR_WAKE));
        let due = chrono::Utc::now() + chrono::Duration::hours(1);
        let wakes = pending_wakes(&pool, due, 10).await.unwrap();
        assert_eq!(wakes.len(), 1, "{wakes:?}");
        assert_eq!(wakes[0].goal_id, goal_id);
        assert_eq!(wakes[0].cause, WakeCause::Report);
        assert_eq!(wakes[0].event_id, format!("lane_event:{id}"));

        // A replay of the same event (a crash before the receipt) is the same wake.
        sqlx::query(
            "UPDATE lane_events SET delivered_at = NULL, delivery_reason = NULL WHERE id = ?",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        dispatch_tick(&pool, &judge, chrono::Utc::now(), |_, _| async { true })
            .await
            .unwrap();
        assert_eq!(pending_wakes(&pool, due, 10).await.unwrap().len(), 1);

        // An owner-urgent kind wakes the goal and still reaches the owner DM.
        let dead: i64 = sqlx::query_scalar(
            "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at)
             VALUES(?, ?, 'dead', 'worker exited', ?) RETURNING id",
        )
        .bind(&lane_key)
        .bind(&lead)
        .bind(old.to_rfc3339())
        .fetch_one(&pool)
        .await
        .unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
        let record = seen.clone();
        let later = chrono::Utc::now() + chrono::Duration::minutes(2);
        let lead_turns = dispatch_tick(&pool, &judge, later, move |key, text| {
            record.lock().unwrap().push((key.storage_key(), text));
            async { true }
        })
        .await
        .unwrap();
        assert_eq!(lead_turns, 1, "the owner still hears that the worker died");
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].0, lead);
        let wakes = pending_wakes(&pool, due, 10).await.unwrap();
        assert!(
            wakes
                .iter()
                .any(|wake| wake.event_id == format!("lane_event:{dead}")),
            "{wakes:?}"
        );
        let reason: Option<String> =
            sqlx::query_scalar("SELECT delivery_reason FROM lane_events WHERE id = ?")
                .bind(dead)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_ne!(reason.as_deref(), Some(SUPERVISOR_WAKE));

        // The goal's wake cannot be queued (its execution binding is gone): an ordinary report
        // stays queued, an owner-urgent one still reaches the owner.
        sqlx::query("DELETE FROM supervisor_execution_bindings WHERE goal_id = ?")
            .bind(&goal_id)
            .execute(&pool)
            .await
            .unwrap();
        let mut failed_handover = Vec::new();
        for (kind, body) in [
            ("launch_failed", "worker could not start"),
            ("not_done", "still missing tests"),
        ] {
            let event: i64 = sqlx::query_scalar(
                "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at)
                 VALUES(?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(&lane_key)
            .bind(&lead)
            .bind(kind)
            .bind(body)
            .bind(old.to_rfc3339())
            .fetch_one(&pool)
            .await
            .unwrap();
            failed_handover.push(event);
        }
        let urgent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let heard = urgent.clone();
        let much_later = chrono::Utc::now() + chrono::Duration::minutes(10);
        let lead_turns = dispatch_tick(&pool, &judge, much_later, move |_, text| {
            heard.lock().unwrap().push(text);
            async { true }
        })
        .await
        .unwrap();
        assert_eq!(lead_turns, 1, "the owner hears the failed launch");
        let heard = urgent.lock().unwrap().clone();
        assert!(
            heard.iter().any(|text| text.contains("worker could not start")),
            "{heard:?}"
        );
        assert!(
            !heard.iter().any(|text| text.contains("still missing tests")),
            "{heard:?}"
        );
        let delivered: Vec<(i64, Option<String>)> = sqlx::query_as(
            "SELECT id, delivered_at FROM lane_events WHERE id IN (?, ?) ORDER BY id",
        )
        .bind(failed_handover[0])
        .bind(failed_handover[1])
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(delivered[0].1.is_some(), "{delivered:?}");
        assert!(
            delivered[1].1.is_none(),
            "an ordinary report stays queued: {delivered:?}"
        );
    }

    /// A worker report reaches its own goal's wake service as one `report` wake per report id:
    /// a retry or an out-of-order replay of an earlier id is the same lane event and wake; a
    /// session bound to no open goal, and a malformed report, write nothing; another goal's
    /// report never lands on this goal.
    #[tokio::test]
    async fn a_worker_report_becomes_one_wake_on_its_own_goal() {
        use crate::supervisor::admission::{
            submit_work, ActorContext, OperationClass, OwnerChoices, SubmitOutcome, SubmitWork,
        };
        use crate::supervisor::{
            attach_session, bind_project_host, create_project, ensure_identity, goal_of,
            pending_wakes, ChannelProvider, WakeCause,
        };
        let pool = crate::storage::memory().await.unwrap();
        let owner = "slack:T1:U-owner";
        let identity = ensure_identity(&pool, owner).await.unwrap();
        let project = create_project(&pool, &identity.supervisor_id, "ferryx", "github.com/x/ferryx")
            .await
            .unwrap();
        bind_project_host(&pool, &project.project_id, "host-a", "/home/o/ferryx/.git")
            .await
            .unwrap();
        let admit = |message: &'static str| {
            let pool = pool.clone();
            async move {
                let actor = ActorContext {
                    principal: owner.into(),
                    source_adapter: ChannelProvider::Slack,
                    source_message_id: message.into(),
                    source_body: "Create result.txt containing READY.".into(),
                    channel_binding_id: None,
                    choices: OwnerChoices::default(),
                };
                let work = SubmitWork {
                    project_id: None,
                    host_id: None,
                    kind: None,
                    title: "Write result.txt".into(),
                    instruction: "Create result.txt containing READY.".into(),
                    requirements: vec![crate::goals::contract::RequirementSpec {
                        id: "r1".into(),
                        text: "result.txt contains READY".into(),
                        verifier: "verify.sh exits 0".into(),
                    }],
                    cwd: None,
                    operations: OperationClass::BASELINE.to_vec(),
                    turn_limit: None,
                };
                let SubmitOutcome::Admitted(admitted) =
                    submit_work(&pool, &actor, &work).await.unwrap()
                else {
                    panic!("expected an admitted goal");
                };
                admitted.receipt
            }
        };
        let mine = admit("D1/1.1").await;
        let other = admit("D1/2.2").await;
        attach_session(&pool, &mine.goal_id, 1, "pane-mine").await.unwrap();
        attach_session(&pool, &other.goal_id, 1, "pane-other").await.unwrap();
        let revision = goal_of(&pool, &mine.goal_id).await.unwrap().unwrap().revision;
        let due = chrono::Utc::now() + chrono::Duration::hours(1);
        let wakes_of = |goal: String| {
            let pool = pool.clone();
            async move {
                pending_wakes(&pool, due, 100)
                    .await
                    .unwrap()
                    .into_iter()
                    .filter(|wake| wake.goal_id == goal)
                    .collect::<Vec<_>>()
            }
        };

        let mut first = Vec::new();
        for id in ["r-0", "r-1", "r-2", "r-3"] {
            let report = report_from_worker(&pool, "host-a", "pane-mine", id, "progress")
                .await
                .unwrap();
            assert_eq!(report.goal_id, mine.goal_id);
            assert_eq!(report.lane_key, mine.lane_key);
            assert!(!report.duplicate, "{id}");
            first.push(report);
        }
        // A retry of the newest and an out-of-order replay of the oldest.
        for id in ["r-3", "r-0"] {
            let again = report_from_worker(&pool, "host-a", "pane-mine", id, "progress")
                .await
                .unwrap();
            assert!(again.duplicate, "{id}");
            let original = first.iter().find(|report| report.wake_event_id == again.wake_event_id);
            assert!(original.is_some(), "{id} is the same lane event");
        }
        let wakes = wakes_of(mine.goal_id.clone()).await;
        assert_eq!(wakes.len(), 4, "{wakes:?}");
        assert!(wakes.iter().all(|wake| wake.cause == WakeCause::Report));
        let mut ids: Vec<String> = wakes.iter().map(|wake| wake.event_id.clone()).collect();
        ids.sort();
        let mut expected: Vec<String> = first.iter().map(|report| report.wake_event_id.clone()).collect();
        expected.sort();
        assert_eq!(ids, expected);
        let events: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT kind, delivery_reason FROM lane_events WHERE lane_key = ? ORDER BY id",
        )
        .bind(&mine.lane_key)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(events.len(), 4, "one lane event per report id");
        assert!(events
            .iter()
            .all(|(kind, reason)| kind == WORKER_REPORT && reason.as_deref() == Some(SUPERVISOR_WAKE)));
        // Status only: the goal did not move.
        let after = goal_of(&pool, &mine.goal_id).await.unwrap().unwrap();
        assert_eq!(after.revision, revision);

        // Another goal's worker reports only to its own goal.
        let theirs = report_from_worker(&pool, "host-a", "pane-other", "r-0", "elsewhere")
            .await
            .unwrap();
        assert_eq!(theirs.goal_id, other.goal_id);
        assert!(!theirs.duplicate, "report ids are per lane");
        assert_eq!(wakes_of(mine.goal_id.clone()).await.len(), 4);
        assert_eq!(wakes_of(other.goal_id.clone()).await.len(), 1);

        // Refusals write nothing.
        let count = || {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM lane_events")
                    .fetch_one(&pool)
                    .await
                    .unwrap()
            }
        };
        let before = count().await;
        let unbound = report_from_worker(&pool, "host-a", "pane-stranger", "r-9", "x")
            .await
            .unwrap_err();
        assert_eq!(unbound.code(), "NOT_FOUND");
        let wrong_host = report_from_worker(&pool, "host-b", "pane-mine", "r-9", "x")
            .await
            .unwrap_err();
        assert_eq!(wrong_host.code(), "NOT_FOUND");
        for (id, body) in [("", "x"), (" r-9", "x"), ("r-9", "  ")] {
            let bad = report_from_worker(&pool, "host-a", "pane-mine", id, body)
                .await
                .unwrap_err();
            assert_eq!(bad.code(), "INVALID_REQUEST", "{id:?} {body:?}");
        }
        assert_eq!(count().await, before);

        // A goal that ended takes no more reports.
        sqlx::query("UPDATE supervisor_goals SET run_state = 'cancelled' WHERE goal_id = ?")
            .bind(&other.goal_id)
            .execute(&pool)
            .await
            .unwrap();
        let ended = report_from_worker(&pool, "host-a", "pane-other", "r-1", "late")
            .await
            .unwrap_err();
        assert_eq!(ended.code(), "NOT_FOUND");
        assert_eq!(count().await, before);
    }
}
