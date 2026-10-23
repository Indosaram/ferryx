//! Action loop and effect execution.
//!
//! `OMON_ACTION_LOOP_ENABLED` arms the coalesce loop that turns unseen inbox rows into
//! one `action_loop` lane event. `OMON_APPROVAL_LEDGER_ENABLED` is what lets a proposal
//! run: a read, an allowlisted signature, or yolo on a clean action. Spend, delete,
//! protected git, and a tainted outward action stay pending until the owner approves.
//! `verified` is written only after the calendar command prints the item key back.
use std::time::Duration;

use chrono::{DateTime, Timelike, Utc};
use chrono_tz::Asia::Seoul;
use serde_json::Value;
use sqlx::SqlitePool;

use crate::effect::{classify_command, requires_always_ask, Effect};
use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

pub const ENABLED_ENV: &str = "OMON_ACTION_LOOP_ENABLED";
pub const LEDGER_ENV: &str = "OMON_APPROVAL_LEDGER_ENABLED";
pub const DAILY_CAP_ENV: &str = "OMON_ACTION_LOOP_DAILY_CAP";
pub const CALENDAR_CMD_ENV: &str = "OMON_ACTION_LOOP_CALENDAR_CMD";
pub const DEFAULT_DAILY_CAP: i64 = 24;
pub const COALESCE_WINDOW: Duration = Duration::from_secs(120);
pub const REQUEUE_AFTER: Duration = Duration::from_secs(30 * 60);
pub const MAX_ATTEMPTS: i64 = 3;
pub const MAX_LISTED_ITEMS: usize = 20;
pub const DEFAULT_CALENDAR_CMD: &str = "zele";

pub fn flag_on(raw: Option<&str>) -> bool {
    matches!(
        raw.map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

pub fn action_loop_enabled() -> bool {
    flag_on(crate::hub_env::var(ENABLED_ENV).ok().as_deref())
}

pub fn approval_ledger_enabled() -> bool {
    flag_on(crate::hub_env::var(LEDGER_ENV).ok().as_deref())
}

pub fn daily_cap_from_env() -> i64 {
    crate::hub_env::var(DAILY_CAP_ENV)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_DAILY_CAP)
}

#[derive(Clone, Debug)]
struct LoopItem {
    item_key: String,
    kind: String,
    source: String,
    line: String,
    link: Option<String>,
    attempts: i64,
}

impl LoopItem {
    fn format_entry(&self) -> String {
        match &self.link {
            Some(link) if !link.is_empty() => {
                format!("[{}:{}] {} ({})", self.kind, self.source, self.line, link)
            }
            _ => format!("[{}:{}] {}", self.kind, self.source, self.line),
        }
    }
}

#[derive(sqlx::FromRow)]
struct ReadyProposal {
    id: i64,
    signature: String,
    effect: String,
    summary: String,
    payload_json: String,
    tainted: i64,
    state: String,
    session_key: String,
}

pub struct ActionLoop {
    pub pool: SqlitePool,
    pub lead_session_key: String,
    /// Where owner alerts go (the Slack home channel or the owner DM). Falls back to the lead.
    pub owner_session_key: Option<String>,
    pub daily_cap: i64,
    pub coalesce_window: Duration,
}

impl ActionLoop {
    pub fn new(pool: SqlitePool, lead_session_key: impl Into<String>) -> Self {
        Self {
            pool,
            lead_session_key: lead_session_key.into(),
            owner_session_key: None,
            daily_cap: daily_cap_from_env(),
            coalesce_window: COALESCE_WINDOW,
        }
    }

    /// Unseen inbox rows become one lane event and `enqueued` outcomes.
    /// A full daily cap leaves them unseen so the next flush can still emit them.
    pub async fn flush(&self, now: DateTime<Utc>) -> Result<Option<i64>> {
        let owner = self
            .owner_session_key
            .as_deref()
            .unwrap_or(&self.lead_session_key);
        let _ = crate::owner_alert::sweep(&self.pool, owner, now).await?;
        let mut items = unseen_inbox(&self.pool).await?;
        items.extend(requeue_stale(&self.pool, now).await?);
        let foreign = foreign_lead_items(&self.pool, &self.lead_session_key).await?;
        let stamp = now.to_rfc3339();
        let mut owned = Vec::new();
        for item in items {
            if foreign.iter().any(|key| key == &item.item_key) {
                sqlx::query(
                    "INSERT INTO action_loop_seen(item_key, seen_at) VALUES(?,?)
                     ON CONFLICT(item_key) DO NOTHING",
                )
                .bind(&item.item_key)
                .bind(&stamp)
                .execute(&self.pool)
                .await?;
                continue;
            }
            owned.push(item);
        }
        let items = owned;
        if items.is_empty() {
            return Ok(None);
        }
        let start = kst_day_start(now);
        let end = start + chrono::Duration::days(1);
        let today: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM lane_events WHERE kind = 'action_loop' AND lead_session_key = ? AND created_at >= ? AND created_at < ?",
        )
        .bind(&self.lead_session_key)
        .bind(start.to_rfc3339())
        .bind(end.to_rfc3339())
        .fetch_one(&self.pool)
        .await?;
        if today >= self.daily_cap {
            return Ok(None);
        }
        let shown: Vec<_> = items.iter().take(MAX_LISTED_ITEMS).collect();
        let mut body = shown
            .iter()
            .map(|item| item.format_entry())
            .collect::<Vec<_>>()
            .join("\n");
        if items.len() > MAX_LISTED_ITEMS {
            body.push_str(&format!("\n… {} more", items.len() - MAX_LISTED_ITEMS));
        }
        let release_at = in_quiet_hours_kst(now).then(|| next_0800_kst(now).to_rfc3339());
        let event_id = insert_event(
            &self.pool,
            &self.lead_session_key,
            "action_loop",
            &body,
            release_at.as_deref(),
        )
        .await?;
        let stamp = now.to_rfc3339();
        for item in &items {
            sqlx::query(
                "INSERT INTO action_loop_seen(item_key, seen_at) VALUES(?,?)
                 ON CONFLICT(item_key) DO NOTHING",
            )
            .bind(&item.item_key)
            .bind(&stamp)
            .execute(&self.pool)
            .await?;
            sqlx::query(
                "INSERT INTO action_loop_outcomes(item_key, lane_event_id, kind, source, line, link, status, attempts, enqueued_at, updated_at)
                 VALUES(?,?,?,?,?,?, 'enqueued', ?,?,?)
                 ON CONFLICT(item_key) DO UPDATE SET
                    attempts = excluded.attempts,
                    lane_event_id = excluded.lane_event_id,
                    status = 'enqueued',
                    updated_at = excluded.updated_at",
            )
            .bind(&item.item_key)
            .bind(event_id)
            .bind(&item.kind)
            .bind(&item.source)
            .bind(&item.line)
            .bind(&item.link)
            .bind(item.attempts)
            .bind(&stamp)
            .bind(&stamp)
            .execute(&self.pool)
            .await?;
        }
        Ok(Some(event_id))
    }

    /// Runs proposals the ledger allows, then writes an `action_report` event.
    pub async fn execute_ready(&self, ledger_on: bool) -> Result<usize> {
        let rows: Vec<ReadyProposal> = sqlx::query_as(
            "SELECT id, signature, effect, summary, payload_json, tainted, state, session_key
             FROM action_proposals
             WHERE state IN ('pending', 'approved', 'auto_allowed')
             ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut ran = 0usize;
        for row in rows {
            let ReadyProposal {
                id,
                signature,
                effect,
                summary,
                payload_json,
                tainted,
                state,
                session_key,
            } = row;
            let key = format!("proposal:{id}");
            let done: Option<String> = sqlx::query_scalar(
                "SELECT status FROM action_loop_outcomes WHERE item_key = ? AND status IN ('executed', 'verified', 'failed')",
            )
            .bind(&key)
            .fetch_optional(&self.pool)
            .await?;
            if done.is_some() {
                continue;
            }
            let payload: Value = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
            let command = payload
                .get("command")
                .or_else(|| payload.get("script"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let class = classify_command(command.as_deref().unwrap_or(&summary), None);
            let parsed = effect.parse::<Effect>().unwrap_or(class.effect);
            let yolo_on = crate::ledger::yolo(&self.pool, &session_key).await?;
            let listed = crate::ledger::is_allowed(&self.pool, &signature).await?;
            if !may_execute(
                state.as_str(),
                parsed,
                tainted != 0,
                ledger_on,
                listed,
                yolo_on,
            ) {
                continue;
            }
            let Some(command) = command else {
                continue;
            };
            let approved = state == "approved" || listed;
            let outcome =
                crate::tools::run_allowed(&command, approved || parsed == Effect::Read).await;
            let (status, detail) = match &outcome {
                Ok(output) => ("executed", output.clone()),
                Err(error) => ("failed", error.to_string()),
            };
            let stamp = now_rfc3339();
            let body = format!("{status} {key}: {detail}");
            let event_id = insert_event(
                &self.pool,
                &self.lead_session_key,
                "action_report",
                &body,
                None,
            )
            .await?;
            sqlx::query(
                "INSERT INTO action_loop_outcomes(item_key, lane_event_id, kind, source, line, status, detail, attempts, enqueued_at, updated_at)
                 VALUES(?,?, 'proposal', 'ledger', ?, ?, ?, 1, ?, ?)
                 ON CONFLICT(item_key) DO UPDATE SET status = excluded.status, detail = excluded.detail, lane_event_id = excluded.lane_event_id, updated_at = excluded.updated_at",
            )
            .bind(&key)
            .bind(event_id)
            .bind(&command)
            .bind(status)
            .bind(&detail)
            .bind(&stamp)
            .bind(&stamp)
            .execute(&self.pool)
            .await?;
            if state == "pending" {
                sqlx::query("UPDATE action_proposals SET state = 'auto_allowed', decided_at = ? WHERE id = ?")
                    .bind(&stamp)
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
            }
            ran += 1;
        }
        Ok(ran)
    }

    /// Coalesces for `coalesce_window`, then flushes and executes until the task is aborted.
    pub async fn serve(self) -> Result<()> {
        match retire_unfiltered_kakao_backlog(&self.pool).await {
            Ok(0) => {}
            Ok(count) => tracing::info!(
                items = count,
                "KakaoTalk items triaged before the omo prefilter were retired from the action loop"
            ),
            Err(error) => tracing::warn!(%error, "unfiltered KakaoTalk backlog was not retired"),
        }
        let mut deadline: Option<tokio::time::Instant> = None;
        let mut sweep = tokio::time::interval(Duration::from_secs(30));
        sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        sweep.tick().await;
        loop {
            let sleep_fut = async {
                match deadline {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending::<()>().await,
                }
            };
            tokio::select! {
                _ = sleep_fut => {
                    deadline = None;
                    if let Err(error) = self.flush(Utc::now()).await {
                        tracing::warn!(%error, "action loop flush failed");
                    }
                    let ledger_on = approval_ledger_enabled();
                    if let Err(error) = self.execute_ready(ledger_on).await {
                        tracing::warn!(%error, "action loop effect pass failed");
                    }
                }
                _ = sweep.tick() => {
                    let pending = unseen_inbox(&self.pool).await.unwrap_or_default();
                    if !pending.is_empty() && deadline.is_none() {
                        deadline = Some(tokio::time::Instant::now() + self.coalesce_window);
                    }
                }
            }
        }
    }
}

pub async fn serve(pool: SqlitePool, lead: String) -> Result<()> {
    ActionLoop::new(pool, lead).serve().await
}

/// The action loop with owner alerts addressed to a real chat session (a storage key),
/// not the lead lane name, which no transport can deliver to.
pub async fn serve_with_owner(
    pool: SqlitePool,
    lead: String,
    owner_session_key: String,
) -> Result<()> {
    let mut action_loop = ActionLoop::new(pool, lead);
    action_loop.owner_session_key = Some(owner_session_key);
    action_loop.serve().await
}

fn may_execute(
    state: &str,
    effect: Effect,
    tainted: bool,
    ledger_on: bool,
    allowlisted: bool,
    yolo_on: bool,
) -> bool {
    if state == "approved" || allowlisted {
        return true;
    }
    if !ledger_on {
        return state == "auto_allowed";
    }
    if requires_always_ask(effect, tainted) {
        return false;
    }
    if state == "auto_allowed" || effect == Effect::Read {
        return true;
    }
    yolo_on && !tainted && effect.allows_class_precedent()
}

fn kst_day_start(now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now.with_timezone(&Seoul);
    local
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|midnight| midnight.and_local_timezone(Seoul).single())
        .map(|at| at.with_timezone(&Utc))
        .unwrap_or(now)
}

fn in_quiet_hours_kst(now: DateTime<Utc>) -> bool {
    now.with_timezone(&Seoul).hour() < 8
}

fn next_0800_kst(now: DateTime<Utc>) -> DateTime<Utc> {
    let kst = now.with_timezone(&Seoul);
    let eight = chrono::NaiveTime::from_hms_opt(8, 0, 0).unwrap_or_default();
    let date = if kst.hour() < 8 {
        kst.date_naive()
    } else {
        kst.date_naive() + chrono::Duration::days(1)
    };
    date.and_time(eight)
        .and_local_timezone(Seoul)
        .single()
        .map(|at| at.with_timezone(&Utc))
        .unwrap_or(now)
}

async fn foreign_lead_items(pool: &SqlitePool, lead: &str) -> Result<Vec<String>> {
    let rows = sqlx::query_scalar(
        "SELECT o.item_key FROM action_loop_outcomes o
         JOIN lane_events e ON e.id = o.lane_event_id
         WHERE e.lead_session_key <> ?",
    )
    .bind(lead)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

const KAKAO_BACKLOG_MARK: &str = "migration:2026.1022.8.1:kakao-unfiltered";

/// 1022.8 stored and triaged KakaoTalk without omo's prefilter (open-chat noise rooms were never
/// dropped), so its unseen KakaoTalk items are not omo candidates. They are marked seen once,
/// so the action loop does not hand the lead hundreds of noise lines.
pub async fn retire_unfiltered_kakao_backlog(pool: &SqlitePool) -> Result<u64> {
    let done: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM action_loop_seen WHERE item_key = ?")
        .bind(KAKAO_BACKLOG_MARK)
        .fetch_one(pool)
        .await?;
    if done > 0 {
        return Ok(0);
    }
    let stamp = now_rfc3339();
    let retired = sqlx::query(
        "INSERT OR IGNORE INTO action_loop_seen(item_key, seen_at)
         SELECT 'inbox:' || d.item_id, ? FROM inbox_decisions d JOIN inbox_items i ON i.id = d.item_id
         WHERE i.source = 'kakao' AND d.class IN ('bot', 'owner')",
    )
    .bind(&stamp)
    .execute(pool)
    .await?
    .rows_affected();
    sqlx::query("INSERT OR IGNORE INTO action_loop_seen(item_key, seen_at) VALUES(?, ?)")
        .bind(KAKAO_BACKLOG_MARK)
        .bind(&stamp)
        .execute(pool)
        .await?;
    Ok(retired)
}

/// omo-gateway's `scan_unseen_candidates`: triaged `bot`/`owner` items (never FYI, ignored or
/// untriaged noise) and GitHub items that triage did not take, each once.
async fn unseen_inbox(pool: &SqlitePool) -> Result<Vec<LoopItem>> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(String, String, String, String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT d.item_id, d.class, COALESCE(d.reason, ''), i.source, COALESCE(i.author, ''), COALESCE(i.title, ''), i.link
         FROM inbox_decisions d
         JOIN inbox_items i ON i.id = d.item_id
         WHERE d.class IN ('bot', 'owner')
           AND NOT EXISTS (SELECT 1 FROM action_loop_seen s WHERE s.item_key = 'inbox:' || d.item_id)
           AND NOT EXISTS (SELECT 1 FROM action_loop_outcomes o WHERE o.item_key = 'inbox:' || d.item_id)
           AND NOT EXISTS (SELECT 1 FROM action_loop_seen s WHERE s.item_key = 'github:' || d.item_id)
         ORDER BY d.decided_at ASC",
    )
    .fetch_all(pool)
    .await?;
    let mut out: Vec<LoopItem> = rows
        .into_iter()
        .map(|(id, class, reason, source, author, title, link)| {
            let author = if author.trim().is_empty() {
                "unknown".to_owned()
            } else {
                author.trim().to_owned()
            };
            LoopItem {
                item_key: format!("inbox:{id}"),
                kind: format!("inbox_{class}"),
                source,
                line: format!(
                    "content from {author}: {} ({})",
                    one_line(&title, 120),
                    one_line(&reason, 80)
                ),
                link,
                attempts: 1,
            }
        })
        .collect();
    let github: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT i.id, COALESCE(i.author, ''), COALESCE(i.title, ''), i.link FROM inbox_items i
         WHERE i.source = 'github'
           AND NOT EXISTS (SELECT 1 FROM action_loop_seen s WHERE s.item_key = 'github:' || i.id)
           AND NOT EXISTS (SELECT 1 FROM action_loop_seen s WHERE s.item_key = 'inbox:' || i.id)
           AND NOT EXISTS (SELECT 1 FROM inbox_decisions d WHERE d.item_id = i.id AND d.class IN ('bot', 'owner'))
         ORDER BY i.received_at ASC",
    )
    .fetch_all(pool)
    .await?;
    out.extend(
        github
            .into_iter()
            .map(|(id, author, title, link)| LoopItem {
                item_key: format!("github:{id}"),
                kind: "github".into(),
                source: "github".into(),
                line: if author.trim().is_empty() {
                    one_line(&title, 160)
                } else {
                    format!("content from {}: {}", author.trim(), one_line(&title, 140))
                },
                link,
                attempts: 1,
            }),
    );
    Ok(out)
}

fn one_line(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let mut cut: String = flat.chars().take(max.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

async fn requeue_stale(pool: &SqlitePool, now: DateTime<Utc>) -> Result<Vec<LoopItem>> {
    let cutoff = (now - chrono::Duration::from_std(REQUEUE_AFTER).unwrap_or_default()).to_rfc3339();
    let rows: Vec<(String, String, String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT item_key, kind, source, line, link, attempts FROM action_loop_outcomes
         WHERE status = 'enqueued' AND attempts < ? AND updated_at < ?",
    )
    .bind(MAX_ATTEMPTS)
    .bind(cutoff)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(item_key, kind, source, line, link, attempts)| LoopItem {
            item_key,
            kind,
            source,
            line,
            link,
            attempts: attempts + 1,
        })
        .collect())
}

async fn insert_event(
    pool: &SqlitePool,
    lead: &str,
    kind: &str,
    body: &str,
    release_at: Option<&str>,
) -> Result<i64> {
    let id = sqlx::query(
        "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at, release_at) VALUES(NULL,?,?,?,?,?)",
    )
    .bind(lead)
    .bind(kind)
    .bind(body)
    .bind(now_rfc3339())
    .bind(release_at)
    .execute(pool)
    .await?
    .last_insert_rowid();
    Ok(id)
}

#[derive(Clone, Debug)]
pub struct Readback {
    pub program: String,
}

impl Readback {
    pub fn from_env() -> Self {
        Self {
            program: crate::hub_env::var(CALENDAR_CMD_ENV)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| DEFAULT_CALENDAR_CMD.to_owned()),
        }
    }
}

/// `verified` sticks only when `{program} cal get {external_id}` prints the item key.
pub async fn record_outcome(
    pool: &SqlitePool,
    item_key: &str,
    status: &str,
    external_id: Option<&str>,
    detail: Option<&str>,
    readback: &Readback,
) -> Result<()> {
    if !matches!(status, "executed" | "verified" | "failed") {
        return Err(HubError::message(format!(
            "invalid outcome status {status}"
        )));
    }
    let current: Option<(String,)> =
        sqlx::query_as("SELECT status FROM action_loop_outcomes WHERE item_key = ?")
            .bind(item_key)
            .fetch_optional(pool)
            .await?;
    let Some((current_status,)) = current else {
        return Err(HubError::message(format!(
            "unknown action-loop item {item_key}"
        )));
    };
    if current_status == "verified" && status != "verified" {
        return Err(HubError::message(format!(
            "{item_key} is already verified and cannot be overwritten with {status}"
        )));
    }
    if status == "executed" && external_id.is_none() {
        if let Some(item_id) = item_key.strip_prefix("inbox:") {
            let kind: Option<String> =
                sqlx::query_scalar("SELECT kind FROM action_loop_outcomes WHERE item_key = ?")
                    .bind(item_key)
                    .fetch_optional(pool)
                    .await?;
            if kind.as_deref() == Some("inbox_owner")
                && !crate::owner_alert::delivered(pool, item_id).await?
            {
                return Err(HubError::message(format!(
                    "`{item_key}` needs the owner and the gateway has no delivered owner alert for it yet, so `executed` would claim he was told when he was not"
                )));
            }
        }
    }
    let proof = if status == "verified" {
        let id = external_id.ok_or_else(|| HubError::message("verified needs an external id"))?;
        let output = crate::child_env::command(&readback.program)
            .args(["cal", "get", id])
            .output()
            .await
            .map_err(|error| HubError::message(error.to_string()))?;
        if !output.status.success() {
            return Err(HubError::message(format!(
                "calendar readback failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        if !text.contains(item_key) {
            return Err(HubError::message(
                "calendar event did not contain the action-loop item key",
            ));
        }
        Some(text)
    } else {
        None
    };
    let stored = proof.or_else(|| detail.map(str::to_owned));
    let changed = sqlx::query(
        "UPDATE action_loop_outcomes
         SET status = ?, external_id = COALESCE(?, external_id), detail = COALESCE(?, detail), updated_at = ?
         WHERE item_key = ? AND (status <> 'verified' OR ? = 'verified')",
    )
    .bind(status)
    .bind(external_id)
    .bind(stored.as_deref())
    .bind(now_rfc3339())
    .bind(item_key)
    .bind(status)
    .execute(pool)
    .await?;
    if changed.rows_affected() == 0 {
        return Err(HubError::message(
            "outcome was verified before this report was written",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_two_flags_default_off() {
        assert!(flag_on(Some("on")));
        assert!(!flag_on(None));
        assert!(!flag_on(Some("0")));
        assert_eq!(COALESCE_WINDOW, Duration::from_secs(120));
        assert_eq!(DEFAULT_DAILY_CAP, 24);
    }

    #[tokio::test]
    async fn a_flush_enqueues_once_and_a_read_proposal_runs_when_the_ledger_is_on() {
        let pool = crate::storage::memory().await.unwrap();
        let now = DateTime::parse_from_rfc3339("2026-10-22T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, snippet, title, received_at, raw_hash, collected_at) VALUES('i1','mail','m1','ping','Ping',?,'h',?)",
        )
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        let loop_ = ActionLoop::new(pool.clone(), "lead").with_cap(24);
        assert!(
            loop_.flush(now).await.unwrap().is_none(),
            "like omo, an untriaged item is not an action-loop candidate"
        );
        sqlx::query("INSERT INTO inbox_items(id, source, source_ref, snippet, title, received_at, raw_hash, collected_at) VALUES('i2','kakao','k2','noise','Room',?,'h2',?)")
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO inbox_decisions(item_id, class, reason, status, decided_at) VALUES('i2','fyi','fyi','open',?)")
            .bind(now.to_rfc3339())
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            loop_.flush(now).await.unwrap().is_none(),
            "FYI items stay out"
        );
        sqlx::query("INSERT INTO inbox_decisions(item_id, class, reason, status, decided_at) VALUES('i1','bot','bot can reply','open',?)")
            .bind(now.to_rfc3339())
            .execute(&pool)
            .await
            .unwrap();
        let event = loop_.flush(now).await.unwrap();
        assert!(event.is_some());
        let body: String =
            sqlx::query_scalar("SELECT body FROM lane_events WHERE kind = 'action_loop'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            body.starts_with("[inbox_bot:mail] content from unknown: Ping (bot can reply)"),
            "{body}"
        );
        assert!(loop_.flush(now).await.unwrap().is_none());
        let status: String = sqlx::query_scalar(
            "SELECT status FROM action_loop_outcomes WHERE item_key = 'inbox:i1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "enqueued");
        sqlx::query(
            "INSERT INTO action_proposals(origin, session_key, signature, effect, summary, payload_json, state) VALUES('loop','s','read:printf:','read','printf hi',?, 'pending')",
        )
        .bind(json!({"command": "printf hi"}).to_string())
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(loop_.execute_ready(false).await.unwrap(), 0);
        assert_eq!(loop_.execute_ready(true).await.unwrap(), 1);
        let detail: String = sqlx::query_scalar(
            "SELECT detail FROM action_loop_outcomes WHERE item_key = 'proposal:1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(detail, "hi");
        let report: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE kind = 'action_report'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(report, 1);
    }

    #[tokio::test]
    async fn spend_stays_pending_and_verified_needs_the_calendar_readback() {
        let pool = crate::storage::memory().await.unwrap();
        let loop_ = ActionLoop::new(pool.clone(), "lead");
        sqlx::query(
            "INSERT INTO action_proposals(origin, session_key, signature, effect, summary, payload_json, state) VALUES('loop','s','spend','spend','stripe',?, 'pending')",
        )
        .bind(json!({"command": "stripe charges create --amount 1000"}).to_string())
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(loop_.execute_ready(true).await.unwrap(), 0);
        sqlx::query(
            "INSERT INTO action_loop_outcomes(item_key, kind, source, line, status, attempts, enqueued_at, updated_at) VALUES('inbox:i9','inbox','mail','ping','enqueued',1,?,?)",
        )
        .bind(now_rfc3339())
        .bind(now_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        let missing = record_outcome(
            &pool,
            "inbox:i9",
            "verified",
            Some("evt"),
            None,
            &Readback {
                program: "false".into(),
            },
        )
        .await;
        assert!(missing.is_err());
        let script = std::env::temp_dir().join(format!("ferryx-cal-{}", std::process::id()));
        std::fs::write(&script, "#!/bin/sh\nprintf '%s\\n' \"$3\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }
        record_outcome(
            &pool,
            "inbox:i9",
            "verified",
            Some("inbox:i9"),
            None,
            &Readback {
                program: script.to_string_lossy().into_owned(),
            },
        )
        .await
        .unwrap();
        let status: String = sqlx::query_scalar(
            "SELECT status FROM action_loop_outcomes WHERE item_key = 'inbox:i9'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "verified");
        let _ = std::fs::remove_file(script);
    }

    #[test]
    fn quiet_hours_are_before_0800_kst() {
        let quiet = DateTime::parse_from_rfc3339("2026-10-21T22:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(in_quiet_hours_kst(quiet));
        assert_eq!(
            next_0800_kst(quiet),
            DateTime::parse_from_rfc3339("2026-10-21T23:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
        let day = DateTime::parse_from_rfc3339("2026-10-22T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!in_quiet_hours_kst(day));
    }

    #[tokio::test]
    async fn a_foreign_lead_keeps_the_item_and_quiet_hours_hold_delivery() {
        let pool = crate::storage::memory().await.unwrap();
        let quiet = DateTime::parse_from_rfc3339("2026-10-21T22:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, snippet, title, received_at, raw_hash, collected_at) VALUES('i1','mail','m1','ping','Ping',?,'h',?)",
        )
        .bind(quiet.to_rfc3339())
        .bind(quiet.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO inbox_decisions(item_id, class, reason, status, decided_at) VALUES('i1','owner','needs you','open',?)",
        )
        .bind(quiet.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        let first = ActionLoop::new(pool.clone(), "lead-a").with_cap(24);
        assert!(first.flush(quiet).await.unwrap().is_some());
        let kind: String =
            sqlx::query_scalar("SELECT kind FROM action_loop_outcomes WHERE item_key = 'inbox:i1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(kind, "inbox_owner");
        let release: String =
            sqlx::query_scalar("SELECT release_at FROM lane_events WHERE kind = 'action_loop'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(release, next_0800_kst(quiet).to_rfc3339());
        sqlx::query("DELETE FROM action_loop_seen")
            .execute(&pool)
            .await
            .unwrap();
        let second = ActionLoop::new(pool.clone(), "lead-b").with_cap(24);
        assert!(second.flush(quiet).await.unwrap().is_none());
        let events: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE kind = 'action_loop'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(events, 1);
        sqlx::query("DELETE FROM delivery_obligations")
            .execute(&pool)
            .await
            .unwrap();
        let refused = record_outcome(
            &pool,
            "inbox:i1",
            "executed",
            None,
            Some("surfaced"),
            &Readback::from_env(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(refused.contains("no delivered owner alert"));
        sqlx::query(
            "INSERT INTO delivery_obligations(id, state, body, updated_at) VALUES('obl:owner-alert:i1','delivered','told',?)",
        )
        .bind(quiet.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        record_outcome(
            &pool,
            "inbox:i1",
            "executed",
            None,
            Some("surfaced"),
            &Readback::from_env(),
        )
        .await
        .unwrap();
    }

    impl ActionLoop {
        fn with_cap(mut self, cap: i64) -> Self {
            self.daily_cap = cap;
            self
        }
    }

    #[tokio::test]
    async fn the_unfiltered_kakao_backlog_is_retired_once() {
        let pool = crate::storage::memory().await.unwrap();
        let now = Utc::now().to_rfc3339();
        for (id, source) in [("k1", "kakao"), ("m1", "mail")] {
            sqlx::query("INSERT INTO inbox_items(id, source, source_ref, snippet, received_at, raw_hash, collected_at) VALUES(?,?,?,'x',?,'h',?)")
                .bind(id).bind(source).bind(id).bind(&now).bind(&now)
                .execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO inbox_decisions(item_id, class, reason, status, decided_at) VALUES(?, 'owner', 'r', 'open', ?)")
                .bind(id).bind(&now)
                .execute(&pool).await.unwrap();
        }
        assert_eq!(retire_unfiltered_kakao_backlog(&pool).await.unwrap(), 1);
        assert_eq!(retire_unfiltered_kakao_backlog(&pool).await.unwrap(), 0);
        let keys: Vec<String> = unseen_inbox(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|item| item.item_key)
            .collect();
        assert_eq!(keys, vec!["inbox:m1".to_owned()]);
    }
}
