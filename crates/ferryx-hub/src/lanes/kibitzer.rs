//! Owner-intent nudge gate. Quiet hours, spacing, the daily wake cap, and the
//! strict decision JSON are the gateway's rules. A model turn runs only after
//! `next_due` says the generation is armed; this module does not poll.
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Timelike, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::judge::{Judge, Purpose};

const SPACING: TimeDelta = TimeDelta::minutes(30);
const KST: TimeDelta = TimeDelta::hours(9);

#[derive(Debug, Clone, Copy)]
pub struct NudgeClock {
    pub quiet_start: u32,
    pub quiet_end: u32,
}

impl Default for NudgeClock {
    fn default() -> Self {
        Self {
            quiet_start: 0,
            quiet_end: 8,
        }
    }
}

impl NudgeClock {
    pub fn allows(self, now: DateTime<Utc>, last_nudge: Option<DateTime<Utc>>) -> bool {
        let hour = (now + KST).hour();
        let quiet = if self.quiet_start <= self.quiet_end {
            (self.quiet_start..self.quiet_end).contains(&hour)
        } else {
            hour >= self.quiet_start || hour < self.quiet_end
        };
        if quiet {
            return false;
        }
        match last_nudge {
            Some(last) => now.signed_duration_since(last) >= SPACING,
            None => true,
        }
    }
}

/// Records a nudge when the clock allows one. The caller invokes this from an idle
/// agent event, not from a timer.
pub fn take_nudge(clock: NudgeClock, now: DateTime<Utc>, last: &mut Option<DateTime<Utc>>) -> bool {
    if clock.allows(now, *last) {
        *last = Some(now);
        true
    } else {
        false
    }
}

pub fn take_nudge_now(last: &mut Option<DateTime<Utc>>) -> bool {
    take_nudge(NudgeClock::default(), Utc::now(), last)
}

#[derive(Debug, Clone, Copy)]
pub struct ArmState {
    pub generation: i64,
    pub consumed: i64,
    pub admitted: Option<DateTime<Utc>>,
    pub wakes_today: i64,
    pub enabled: bool,
}

/// `None` means unarmed. No timer is installed in that state.
pub fn next_due(now: DateTime<Utc>, state: &ArmState) -> Option<DateTime<Utc>> {
    if !state.enabled || state.generation == state.consumed {
        return None;
    }
    let mut due = now;
    if let Some(last) = state.admitted {
        due = due.max(last + SPACING);
    }
    if state.wakes_today >= 6 {
        due = due.max(day_start(now) + TimeDelta::days(1));
    }
    if (due + KST).hour() < 8 {
        due = day_start(due) + TimeDelta::hours(8);
    }
    Some(due)
}

fn day_start(now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now + KST;
    local
        - TimeDelta::seconds(i64::from(local.num_seconds_from_midnight()))
        - TimeDelta::nanoseconds(i64::from(local.nanosecond()))
        - KST
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct TargetLane(pub Option<String>);

impl<'de> Deserialize<'de> for TargetLane {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        serde_json::from_value(value)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub wake: bool,
    pub reason: String,
    pub suggested: String,
    pub target_lane: TargetLane,
}

pub fn parse_decision(raw: &str) -> std::result::Result<Decision, serde_json::Error> {
    serde_json::from_str(strip_code_fence(raw.trim()))
}

fn strip_code_fence(text: &str) -> &str {
    let Some(inner) = text
        .strip_prefix("```")
        .and_then(|text| text.strip_suffix("```"))
    else {
        return text;
    };
    let inner = inner.strip_prefix("json").unwrap_or(inner);
    inner.trim()
}

pub async fn ensure_schema(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS kibitzer_state (
            lead TEXT PRIMARY KEY,
            generation INTEGER NOT NULL DEFAULT 0,
            consumed INTEGER NOT NULL DEFAULT 0,
            admitted_ms INTEGER
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS kibitzer_inbox_insert AFTER INSERT ON inbox_items
         BEGIN UPDATE kibitzer_state SET generation = generation + 1; END",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS kibitzer_lane_insert AFTER INSERT ON lane_events
         WHEN NEW.kind IN ('blocked','question','stale','not_done','cron_problem')
         BEGIN UPDATE kibitzer_state SET generation = generation + 1
           WHERE lead = NEW.lead_session_key OR NEW.kind = 'cron_problem'; END",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn ensure_lead(pool: &SqlitePool, lead: &str) -> Result<()> {
    sqlx::query("INSERT INTO kibitzer_state(lead) VALUES (?) ON CONFLICT DO NOTHING")
        .bind(lead)
        .execute(pool)
        .await?;
    Ok(())
}

/// One omo app-server turn. The production backend is the websocket client in
/// `meta::backend::UrlModel`.
#[async_trait]
pub trait ModelTurn: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String>;

    /// Cron agent jobs pass the Hermes model id and `enabled_toolsets`.
    /// The default ignores both and runs [`Self::complete`].
    async fn complete_with(
        &self,
        prompt: &str,
        model: Option<&str>,
        toolsets: &[String],
    ) -> Result<String> {
        let _ = (model, toolsets);
        self.complete(prompt).await
    }

    /// Typed turn for the supervisor judge. A backend that can name its failure (the retained
    /// mhc supervisor) returns [`TurnFault::Supervisor`](crate::meta::mhc_fault::TurnFault) and
    /// honors `context`; the default runs [`Self::complete`] and reports a legacy error.
    async fn complete_typed(
        &self,
        prompt: &str,
        context: crate::meta::mhc_fault::TurnContext,
    ) -> std::result::Result<String, crate::meta::mhc_fault::TurnFault> {
        let _ = context;
        self.complete(prompt)
            .await
            .map_err(crate::meta::mhc_fault::TurnFault::Other)
    }

    /// True when the backend bounds its own turns and reports a typed timeout, so a caller
    /// must not cut the turn short with its own deadline (the retained mhc supervisor).
    fn owns_deadline(&self) -> bool {
        false
    }

    /// True only for the retained mhc supervisor engine. The judge then skips the
    /// typesafe-judge breaker and the omo fallback daily cap (neither is this engine's), does
    /// not count its calls toward that cap, and applies only an explicitly configured
    /// supervisor budget, reported as a typed fault.
    fn is_supervisor_engine(&self) -> bool {
        false
    }
}

const KIBITZER_PROMPT: &str = "Return only JSON with exactly wake (boolean), reason (string), suggested (string), target_lane (string or null). Decide whether the owner needs proactive action. Do not perform actions. Set wake to true only when there is something concrete the owner should act on now (from the open owner items, recent events and lanes); otherwise set wake to false. When wake is true, write suggested as a short Korean message to the owner that names the 1-3 most important concrete things to do now and why each matters.";

/// Consumes one armed generation. A judge or model failure puts the generation back.
/// A hold, or a decision whose `wake` is false, stays consumed.
pub async fn evaluate(
    pool: &SqlitePool,
    lead: &str,
    judge: &Judge,
    model: &dyn ModelTurn,
) -> Result<bool> {
    evaluate_at(pool, lead, judge, model, Utc::now()).await
}

pub async fn evaluate_at(
    pool: &SqlitePool,
    lead: &str,
    judge: &Judge,
    model: &dyn ModelTurn,
    now: DateTime<Utc>,
) -> Result<bool> {
    ensure_lead(pool, lead).await?;
    let state = load_arm(pool, lead, now).await?;
    if next_due(now, &state).is_none_or(|due| due > now) {
        return Ok(false);
    }
    let admitted = now.timestamp_millis();
    let reserved = sqlx::query(
        "UPDATE kibitzer_state SET consumed = generation, admitted_ms = ? WHERE lead = ? AND generation > consumed AND (admitted_ms IS NULL OR admitted_ms <= ?)",
    )
    .bind(admitted)
    .bind(lead)
    .bind((now - SPACING).timestamp_millis())
    .execute(pool)
    .await?;
    if reserved.rows_affected() != 1 {
        return Ok(false);
    }
    let document = match state_document(pool, lead).await {
        Ok(document) => document,
        Err(error) => {
            restore_admitted(pool, lead, admitted).await?;
            return Err(error);
        }
    };
    let choice = match judge
        .choice(
            Purpose::KibitzerWake,
            &crate::judge::scrub_for_external(&document),
            "Would the owner want the agent to proactively do or ask something right now?",
            &["wake", "hold"],
        )
        .await
    {
        Ok(choice) => choice,
        Err(_) => {
            restore_admitted(pool, lead, admitted).await?;
            return Ok(false);
        }
    };
    if choice.as_str() != "wake" {
        return Ok(false);
    }
    let reply = match model
        .complete(&format!("{KIBITZER_PROMPT}\n\n{document}"))
        .await
    {
        Ok(reply) => reply,
        Err(_) => {
            restore_admitted(pool, lead, admitted).await?;
            return Ok(false);
        }
    };
    let decision = match parse_decision(&reply) {
        Ok(decision) => decision,
        Err(_) => {
            restore_admitted(pool, lead, admitted).await?;
            return Ok(false);
        }
    };
    if !decision.wake {
        return Ok(false);
    }
    let body =
        serde_json::to_string(&decision).map_err(|error| HubError::message(error.to_string()))?;
    let start = day_start(now);
    let inserted = sqlx::query(
        "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at)
         SELECT NULL, ?, 'kibitzer', ?, ?
         WHERE (SELECT count(*) FROM lane_events WHERE kind = 'kibitzer' AND lead_session_key = ? AND julianday(created_at) >= julianday(?) AND julianday(created_at) < julianday(?)) < 6",
    )
    .bind(lead)
    .bind(body)
    .bind(now.to_rfc3339())
    .bind(lead)
    .bind(start.to_rfc3339())
    .bind((start + TimeDelta::days(1)).to_rfc3339())
    .execute(pool)
    .await?;
    Ok(inserted.rows_affected() == 1)
}

async fn load_arm(pool: &SqlitePool, lead: &str, now: DateTime<Utc>) -> Result<ArmState> {
    let row: Option<(i64, i64, Option<i64>)> = sqlx::query_as(
        "SELECT generation, consumed, admitted_ms FROM kibitzer_state WHERE lead = ?",
    )
    .bind(lead)
    .fetch_optional(pool)
    .await?;
    let (generation, consumed, admitted_ms) = row.unwrap_or((0, 0, None));
    let wakes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM lane_events WHERE kind = 'kibitzer' AND lead_session_key = ? AND julianday(created_at) >= julianday(?) AND julianday(created_at) < julianday(?)",
    )
    .bind(lead)
    .bind(day_start(now).to_rfc3339())
    .bind((day_start(now) + TimeDelta::days(1)).to_rfc3339())
    .fetch_one(pool)
    .await?;
    Ok(ArmState {
        generation,
        consumed,
        admitted: admitted_ms.and_then(DateTime::from_timestamp_millis),
        wakes_today: wakes,
        enabled: true,
    })
}

async fn restore_admitted(pool: &SqlitePool, lead: &str, admitted: i64) -> Result<()> {
    sqlx::query(
        "UPDATE kibitzer_state SET consumed = consumed - 1 WHERE lead = ? AND admitted_ms = ? AND consumed = generation",
    )
    .bind(lead)
    .bind(admitted)
    .execute(pool)
    .await?;
    Ok(())
}

async fn state_document(pool: &SqlitePool, lead: &str) -> Result<String> {
    let lanes: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT key, status, title FROM lanes WHERE lead_session_key = ? AND status IN ('queued','working','blocked','done_claimed','not_done','verified_done') ORDER BY created_at, key",
    )
    .bind(lead)
    .fetch_all(pool)
    .await?;
    let events: Vec<(String, String)> = sqlx::query_as(
        "SELECT kind, body FROM lane_events WHERE lead_session_key = ? OR kind = 'cron_problem' ORDER BY created_at DESC, id DESC LIMIT 10",
    )
    .bind(lead)
    .fetch_all(pool)
    .await?;
    let mut messages: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM messages WHERE session_key = ? ORDER BY created_at DESC, id DESC LIMIT 30",
    )
    .bind(lead)
    .fetch_all(pool)
    .await?;
    messages.reverse();
    let memories: Vec<(String,)> = sqlx::query_as(
        "SELECT content FROM memories WHERE session_key = ? ORDER BY updated_at DESC LIMIT 10",
    )
    .bind(lead)
    .fetch_all(pool)
    .await?;
    let owner_items: Vec<OwnerItemRow> = sqlx::query_as(
        "SELECT i.source, COALESCE(i.thread_ref, i.title, i.id), i.title, i.author, i.snippet, i.received_at
         FROM inbox_decisions d JOIN inbox_items i ON i.id = d.item_id
         WHERE d.class = 'owner' AND d.status = 'open'
         ORDER BY i.received_at DESC, i.id DESC",
    )
    .fetch_all(pool)
    .await?;
    let mut document = String::from("Open lanes:\n");
    for (key, status, title) in lanes {
        document.push_str(&format!("{key} {status}: {title}\n"));
    }
    document.push_str(&owner_items_section(&owner_items));
    document.push_str("Recent events:\n");
    for (kind, body) in events.into_iter().rev() {
        document.push_str(&format!("{kind}: {body}\n"));
    }
    document.push_str("Owner preferences:\n");
    for (memory,) in memories {
        document.push_str(&memory);
        document.push('\n');
    }
    let precedents: Vec<(String, String)> =
        sqlx::query_as("SELECT signature, decision FROM action_precedents ORDER BY id LIMIT 10")
            .fetch_all(pool)
            .await?;
    let lines: Vec<String> = precedents
        .iter()
        .map(|(signature, decision)| format!("{signature} {decision}"))
        .collect();
    document.push_str(&crate::context::render_pack(
        &[("Precedents:", &lines)],
        crate::context::MAX_PACK_CHARS,
    ));
    document.push_str("Recent messages:\n");
    for (role, content) in messages {
        document.push_str(&format!("{role}: {content}\n"));
    }
    Ok(document)
}

/// (source, conversation, title, author, snippet, received_at) of one open owner item.
type OwnerItemRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
);

/// Open owner inbox items grouped per conversation, newest first. `rows` arrive newest first.
fn owner_items_section(rows: &[OwnerItemRow]) -> String {
    // (source, conversation) -> (newest row index, count), in newest-first order.
    let mut groups: Vec<((&str, &str), usize, usize)> = Vec::new();
    for (index, (source, conversation, ..)) in rows.iter().enumerate() {
        let key = (source.as_str(), conversation.as_str());
        match groups.iter_mut().find(|(group, _, _)| *group == key) {
            Some((_, _, count)) => *count += 1,
            None => groups.push((key, index, 1)),
        }
    }
    let mut section = format!(
        "Open owner items:\nTotal open owner items: {}\n",
        rows.len()
    );
    for ((source, _), newest, count) in groups.into_iter().take(15) {
        let (_, _, title, author, snippet, received_at) = &rows[newest];
        let name = title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .or(author.as_deref())
            .unwrap_or("-");
        let line: String = snippet
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(120)
            .collect();
        section.push_str(&format!(
            "[{source}] {name} x{count} latest {received_at}: {line}\n"
        ));
    }
    section
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_and_spacing_both_hold_a_nudge() {
        let clock = NudgeClock::default();
        let quiet = DateTime::parse_from_rfc3339("2026-10-21T16:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!clock.allows(quiet, None));
        let day = DateTime::parse_from_rfc3339("2026-10-21T02:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(clock.allows(day, None));
        let recent = day - TimeDelta::minutes(10);
        assert!(!clock.allows(day, Some(recent)));
        assert!(clock.allows(day, Some(day - TimeDelta::minutes(30))));
    }

    #[test]
    fn an_unarmed_generation_has_no_due_time_and_the_seventh_wake_waits() {
        let now = DateTime::parse_from_rfc3339("2026-10-21T02:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let idle = ArmState {
            generation: 1,
            consumed: 1,
            admitted: None,
            wakes_today: 0,
            enabled: true,
        };
        assert!(next_due(now, &idle).is_none());
        let armed = ArmState {
            consumed: 0,
            wakes_today: 6,
            ..idle
        };
        let due = next_due(now, &armed).unwrap();
        assert!(due > now);
        let parsed = parse_decision("```json{\"wake\":true,\"reason\":\"ask\",\"suggested\":\"ping\",\"target_lane\":null}```").unwrap();
        assert!(parsed.wake);
        assert!(parse_decision("{\"wake\":true,\"reason\":\"x\",\"suggested\":\"y\"}").is_err());
    }

    struct Scripted {
        text: String,
        fail: bool,
        expect: &'static [&'static str],
        reject: &'static [&'static str],
    }

    #[async_trait]
    impl ModelTurn for Scripted {
        async fn complete(&self, prompt: &str) -> Result<String> {
            assert!(prompt.contains("Open lanes"));
            for needle in self.expect {
                assert!(
                    prompt.contains(needle),
                    "prompt lacks {needle:?}:\n{prompt}"
                );
            }
            for needle in self.reject {
                assert!(!prompt.contains(needle), "prompt has {needle:?}:\n{prompt}");
            }
            if self.fail {
                return Err(HubError::message("model down"));
            }
            Ok(self.text.clone())
        }
    }

    #[tokio::test]
    async fn an_armed_generation_runs_the_model_and_a_failure_restores_it() {
        let pool = crate::storage::memory().await.unwrap();
        ensure_schema(&pool).await.unwrap();
        ensure_lead(&pool, "meta").await.unwrap();
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, snippet, received_at, raw_hash, collected_at) VALUES('i1','slack','r1','ping','2026-10-21T02:00:00Z','h','2026-10-21T02:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let now = DateTime::parse_from_rfc3339("2026-10-21T02:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let judge = Judge::forced("wake");
        let model = Scripted {
            text: r#"{"wake":true,"reason":"ask","suggested":"ping","target_lane":null}"#.into(),
            fail: false,
            expect: &[],
            reject: &[],
        };
        assert!(evaluate_at(&pool, "meta", &judge, &model, now)
            .await
            .unwrap());
        let body: String = sqlx::query_scalar(
            "SELECT body FROM lane_events WHERE kind = 'kibitzer' AND lead_session_key = 'meta'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(body.contains("\"wake\":true"));
        assert!(!evaluate_at(&pool, "meta", &judge, &model, now)
            .await
            .unwrap());

        sqlx::query("UPDATE kibitzer_state SET generation = generation + 1 WHERE lead = 'meta'")
            .execute(&pool)
            .await
            .unwrap();
        let failing = Scripted {
            text: String::new(),
            fail: true,
            expect: &[],
            reject: &[],
        };
        assert!(!evaluate_at(&pool, "meta", &judge, &failing, now)
            .await
            .unwrap());
        let (generation, consumed): (i64, i64) =
            sqlx::query_as("SELECT generation, consumed FROM kibitzer_state WHERE lead = 'meta'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(generation > consumed);
    }

    #[tokio::test]
    async fn the_state_document_groups_open_owner_items_and_skips_dismissed_ones() {
        let pool = crate::storage::memory().await.unwrap();
        ensure_schema(&pool).await.unwrap();
        ensure_lead(&pool, "meta").await.unwrap();
        for (id, thread, title, snippet, received, status) in [
            (
                "o1",
                "t1",
                "Contract",
                "first draft",
                "2026-10-21T01:00:00Z",
                "open",
            ),
            (
                "o2",
                "t1",
                "Contract",
                "please\nsign today",
                "2026-10-21T01:30:00Z",
                "open",
            ),
            (
                "o3",
                "t2",
                "Old invoice",
                "already handled",
                "2026-10-21T01:40:00Z",
                "dismissed",
            ),
        ] {
            sqlx::query(
                "INSERT INTO inbox_items(id, source, source_ref, thread_ref, author, title, snippet, received_at, raw_hash, collected_at) VALUES(?, 'mail', ?, ?, 'boss', ?, ?, ?, 'h', ?)",
            )
            .bind(id)
            .bind(id)
            .bind(thread)
            .bind(title)
            .bind(snippet)
            .bind(received)
            .bind(received)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO inbox_decisions(item_id, class, reason, status, decided_at) VALUES(?, 'owner', 'needs you', ?, ?)",
            )
            .bind(id)
            .bind(status)
            .bind(received)
            .execute(&pool)
            .await
            .unwrap();
        }
        let now = DateTime::parse_from_rfc3339("2026-10-21T02:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let model = Scripted {
            text: r#"{"wake":true,"reason":"sign","suggested":"sign the contract","target_lane":null}"#.into(),
            fail: false,
            expect: &[
                "Open owner items:",
                "Total open owner items: 2",
                "[mail] Contract x2 latest 2026-10-21T01:30:00Z: please sign today",
            ],
            reject: &["Old invoice", "already handled"],
        };
        assert!(
            evaluate_at(&pool, "meta", &Judge::forced("wake"), &model, now)
                .await
                .unwrap()
        );
    }
}
