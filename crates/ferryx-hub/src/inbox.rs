//! Event-armed inbox. A message older than 24 hours is filed as fyi without a model call.
//! The production path has no model fallback: a missing classification is an error.
use chrono::{DateTime, NaiveDate, TimeDelta, TimeZone, Utc};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::storage::WriteTx;
use crate::storage::now_rfc3339;

pub const STALE_AFTER_HOURS: i64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Owner,
    Bot,
    Fyi,
    Ignore,
}

impl Class {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "owner_must_act" | "owner" => Ok(Self::Owner),
            "bot_can_handle" | "bot" => Ok(Self::Bot),
            "fyi" => Ok(Self::Fyi),
            "ignore" => Ok(Self::Ignore),
            _ => Err(HubError::message("inbox judge returned no classification")),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Bot => "bot",
            Self::Fyi => "fyi",
            Self::Ignore => "ignore",
        }
    }
}

pub fn is_stale(received: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now.signed_duration_since(received) > TimeDelta::hours(STALE_AFTER_HOURS)
}

/// Stale mail is fyi. A fresh item uses the model class and does not invent one.
pub fn classify(
    received: DateTime<Utc>,
    now: DateTime<Utc>,
    model_class: Option<&str>,
) -> Result<Class> {
    if is_stale(received, now) {
        return Ok(Class::Fyi);
    }
    Class::parse(model_class.unwrap_or(""))
}

/// JSON, then NDJSON, then YAML. Empty output is an empty list.
pub fn decode_structured_output(bytes: &[u8]) -> Result<Value> {
    if bytes.is_empty() || bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Array(Vec::new()));
    }
    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        return Ok(value);
    }
    let text = String::from_utf8_lossy(bytes);
    let mut items = Vec::new();
    let mut ndjson = true;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        match serde_json::from_str::<Value>(line) {
            Ok(item) => items.push(item),
            Err(_) => {
                ndjson = false;
                break;
            }
        }
    }
    if ndjson && !items.is_empty() {
        return Ok(Value::Array(items));
    }
    serde_yaml::from_slice::<Value>(bytes)
        .map_err(|error| HubError::message(format!("invalid structured output: {error}")))
}

pub fn digest(items: &[(String, Class)]) -> String {
    let mut lines = vec!["# inbox".to_owned()];
    for class in [Class::Owner, Class::Bot, Class::Fyi, Class::Ignore] {
        let rows: Vec<_> = items
            .iter()
            .filter(|(_, item)| *item == class)
            .map(|(title, _)| title.as_str())
            .collect();
        if rows.is_empty() {
            continue;
        }
        lines.push(format!("## {}", class.as_str()));
        for title in rows {
            lines.push(format!("- {title}"));
        }
    }
    lines.join("\n")
}

pub async fn ingest(
    pool: &SqlitePool,
    id: &str,
    source: &str,
    source_ref: &str,
    snippet: &str,
    received_at: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO inbox_items(id, source, source_ref, snippet, received_at, raw_hash, collected_at) VALUES(?,?,?,?,?,?,?)",
    )
    .bind(id)
    .bind(source)
    .bind(source_ref)
    .bind(snippet)
    .bind(received_at)
    .bind(source_ref)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn decide(
    pool: &SqlitePool,
    item_id: &str,
    class: Class,
    reason: &str,
    proposed_action: Option<&str>,
) -> Result<()> {
    let status = if proposed_action.is_some() {
        "proposed"
    } else {
        "open"
    };
    sqlx::query(
        "INSERT OR REPLACE INTO inbox_decisions(item_id, class, reason, proposed_action, status, decided_at) VALUES(?,?,?,?,?,?)",
    )
    .bind(item_id)
    .bind(class.as_str())
    .bind(reason)
    .bind(proposed_action)
    .bind(status)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    if class == Class::Bot {
        if let Some(action) = proposed_action {
            crate::ledger::propose(pool, "inbox", item_id, action, action, false).await?;
        }
    }
    Ok(())
}

pub const TRIAGE_OPTIONS: &[&str] = &["owner_must_act", "bot_can_handle", "fyi", "ignore"];
pub const TRIAGE_INSTRUCTIONS: &str = "owner_must_act = a specific person is waiting for the owner's own reply, decision, signature or payment, OR an account security/billing notice (new sign-in or suspicious login, password or 2FA change, failed or overdue payment, suspended or expiring account, domain or subscription expiry); surveys, reminders, newsletters, receipts, routine notices, verification/one-time codes, unsolicited recruiter/headhunter/job-board scout pitches and other automated mail are never owner_must_act; bot_can_handle = a coding/ops task an agent can do alone without speaking as the owner; fyi = information only; ignore = ads/promotions. Classify only the message marked as the message to classify, using the earlier conversation for context; small talk, reactions and group-chat chatter are fyi.";

/// Earlier messages of the same conversation shown to the judge with a new one.
const THREAD_CONTEXT_MESSAGES: i64 = 6;
/// The whole judged text for a conversation message; the newest lines are kept.
const TRIAGE_TEXT_MAX_CHARS: usize = 1500;
/// One earlier message never takes more than this, so a long paste does not hide the rest.
const CONTEXT_LINE_MAX_CHARS: usize = 300;

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn spoken(author: Option<&str>, text: &str) -> String {
    match author.map(str::trim).filter(|author| !author.is_empty()) {
        Some(author) => format!("{author}: {}", one_line(text)),
        None => one_line(text),
    }
}

/// The text the judge reads for a conversation message: the room title, up to
/// `THREAD_CONTEXT_MESSAGES` earlier lines (oldest first), then the marked message.
/// Capped at `TRIAGE_TEXT_MAX_CHARS`; the oldest lines go first, then the title.
fn triage_text(
    title: Option<&str>,
    earlier: &[(Option<String>, String)],
    author: Option<&str>,
    snippet: &str,
) -> String {
    let marked: String = format!("Message to classify:\n>>> {}", spoken(author, snippet))
        .chars()
        .take(TRIAGE_TEXT_MAX_CHARS)
        .collect();
    let mut budget = TRIAGE_TEXT_MAX_CHARS.saturating_sub(marked.chars().count());
    let mut parts = Vec::new();
    if let Some(title) = title.map(one_line).filter(|title| !title.is_empty()) {
        let heading = format!("Conversation: {title}");
        let cost = heading.chars().count() + 1;
        if cost <= budget {
            budget -= cost;
            parts.push(heading);
        }
    }
    let label = "Earlier messages (oldest first):";
    let mut room = budget.saturating_sub(label.chars().count() + 1);
    let mut lines = Vec::new();
    for (who, text) in earlier.iter().rev() {
        let line: String = format!("- {}", spoken(who.as_deref(), text))
            .chars()
            .take(CONTEXT_LINE_MAX_CHARS)
            .collect();
        let cost = line.chars().count() + 1;
        if cost > room {
            break;
        }
        room -= cost;
        lines.push(line);
    }
    if !lines.is_empty() {
        lines.reverse();
        parts.push(label.to_owned());
        parts.extend(lines);
    }
    parts.push(marked);
    parts.join("\n")
}

/// Up to `THREAD_CONTEXT_MESSAGES` messages of the same conversation received no later than
/// `received_at`, oldest first, as (author, snippet).
async fn earlier_in_thread(
    pool: &SqlitePool,
    id: &str,
    source: &str,
    thread_ref: &str,
    received_at: &str,
) -> Result<Vec<(Option<String>, String)>> {
    let mut rows: Vec<(Option<String>, String)> = sqlx::query_as(
        "SELECT author, snippet FROM inbox_items
         WHERE source = ? AND thread_ref = ? AND id != ? AND julianday(received_at) <= julianday(?)
         ORDER BY julianday(received_at) DESC, id DESC LIMIT ?",
    )
    .bind(source)
    .bind(thread_ref)
    .bind(id)
    .bind(received_at)
    .bind(THREAD_CONTEXT_MESSAGES)
    .fetch_all(pool)
    .await?;
    rows.reverse();
    Ok(rows)
}

/// One open owner item per conversation: a new owner item dismisses the older (or same-time)
/// open owner items of its thread, and is itself dismissed if a newer one is already open.
async fn supersede_in_thread(
    pool: &SqlitePool,
    id: &str,
    source: &str,
    thread_ref: &str,
    received_at: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE inbox_decisions SET status = 'dismissed'
         WHERE status = 'open' AND class = 'owner' AND item_id != ? AND item_id IN (
             SELECT id FROM inbox_items
             WHERE source = ? AND thread_ref = ? AND julianday(received_at) <= julianday(?))",
    )
    .bind(id)
    .bind(source)
    .bind(thread_ref)
    .bind(received_at)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE inbox_decisions SET status = 'dismissed'
         WHERE item_id = ? AND status = 'open' AND EXISTS (
             SELECT 1 FROM inbox_items i JOIN inbox_decisions d ON d.item_id = i.id
             WHERE d.status = 'open' AND d.class = 'owner' AND i.source = ? AND i.thread_ref = ?
               AND julianday(i.received_at) > julianday(?))",
    )
    .bind(id)
    .bind(source)
    .bind(thread_ref)
    .bind(received_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Collapses a backlog to the newest open owner decision per (source, thread_ref) and dismisses
/// the rest. Items without a thread are left alone. Returns how many were dismissed; idempotent.
pub async fn reconcile_owner_threads(pool: &SqlitePool) -> Result<u64> {
    let mut tx = pool.begin_write().await?;
    let older: Vec<String> = sqlx::query_scalar(
        "SELECT i.id FROM inbox_items i JOIN inbox_decisions d ON d.item_id = i.id
         WHERE d.status = 'open' AND d.class = 'owner' AND i.thread_ref IS NOT NULL
           AND EXISTS (
               SELECT 1 FROM inbox_items j JOIN inbox_decisions e ON e.item_id = j.id
               WHERE e.status = 'open' AND e.class = 'owner'
                 AND j.source = i.source AND j.thread_ref = i.thread_ref
                 AND (julianday(j.received_at) > julianday(i.received_at)
                      OR (julianday(j.received_at) = julianday(i.received_at) AND j.id > i.id)))",
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut dismissed = 0u64;
    for id in &older {
        dismissed += sqlx::query(
            "UPDATE inbox_decisions SET status = 'dismissed' WHERE item_id = ? AND status = 'open'",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    }
    tx.commit().await?;
    Ok(dismissed)
}

#[derive(sqlx::FromRow)]
struct PendingRow {
    id: String,
    source: String,
    snippet: String,
    title: Option<String>,
    received_at: String,
    thread_ref: Option<String>,
    author: Option<String>,
}

/// Classifies inbox rows that have no decision yet.
/// A message older than 24 hours is fyi, except a GitHub `[CI failed]` snippet.
/// A fallback judge choice leaves every row in the batch pending.
///
/// `lead` is the session that receives an `inbox_urgent` event when an owner item is due
/// within 24 hours (omo triage). Pass the owner DM; tests pass an explicit key so they do
/// not depend on the process-wide default lead.
pub async fn classify_pending<F, Fut>(
    pool: &SqlitePool,
    judge: &crate::judge::Judge,
    now: DateTime<Utc>,
    lead: &str,
    mut action_for: F,
) -> Result<usize>
where
    F: FnMut(&str, Class) -> Fut,
    Fut: std::future::Future<Output = Result<Option<String>>>,
{
    let rows: Vec<PendingRow> = sqlx::query_as(
        "SELECT id, source, snippet, title, received_at, thread_ref, author FROM inbox_items
         WHERE id NOT IN (SELECT item_id FROM inbox_decisions)
         ORDER BY julianday(received_at), id",
    )
    .fetch_all(pool)
    .await?;
    let mut ready = Vec::new();
    for PendingRow {
        id,
        source,
        snippet,
        title,
        received_at,
        thread_ref,
        author,
    } in rows
    {
        let received = DateTime::parse_from_rfc3339(&received_at)
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or(now);
        let due = parse_due_at(
            &format!("{} {}", title.as_deref().unwrap_or_default(), snippet),
            received,
        )
        .filter(|due| *due >= now);
        let thread = thread_ref.map(|thread| (source.clone(), thread, received_at.clone()));
        let ci_failure = source == "github" && snippet.starts_with("[CI failed]");
        if !ci_failure && is_stale(received, now) {
            ready.push((id, Class::Fyi, "stale".to_owned(), None, due, thread));
            continue;
        }
        let judged = match &thread {
            Some((_, thread_ref, _)) => {
                let earlier =
                    earlier_in_thread(pool, &id, &source, thread_ref, &received_at).await?;
                triage_text(title.as_deref(), &earlier, author.as_deref(), &snippet)
            }
            None => snippet,
        };
        let choice = judge
            .choice(
                crate::judge::Purpose::InboxClass,
                &crate::judge::scrub_for_external(&judged),
                TRIAGE_INSTRUCTIONS,
                TRIAGE_OPTIONS,
            )
            .await?;
        // omo triage: a capped or conservative answer is not a jev classification.
        if choice.is_fallback || choice.is_capped {
            return Err(HubError::message(
                "inbox jev unavailable; items remain pending",
            ));
        }
        let class = Class::parse(choice.as_str())?;
        let action = match class {
            Class::Owner | Class::Bot => action_for(&id, class).await?,
            Class::Fyi | Class::Ignore => None,
        };
        ready.push((id, class, choice.as_str().to_owned(), action, due, thread));
    }
    let stored = ready.len();
    for (id, class, reason, action, due, thread) in ready {
        decide(pool, &id, class, &reason, action.as_deref()).await?;
        if class == Class::Owner {
            if let Some((source, thread_ref, received_at)) = &thread {
                supersede_in_thread(pool, &id, source, thread_ref, received_at).await?;
            }
        }
        if let Some(due) = due {
            sqlx::query("UPDATE inbox_decisions SET due_at = ? WHERE item_id = ?")
                .bind(due.to_rfc3339())
                .bind(&id)
                .execute(pool)
                .await?;
            // omo: an owner item due within a day wakes the lead. Anything later stays on the row.
            if class == Class::Owner && due <= now + TimeDelta::hours(24) {
                sqlx::query(
                    "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at)
                     VALUES(NULL, ?, 'inbox_urgent', ?, ?)",
                )
                .bind(lead)
                .bind(format!("inbox:{id} due {}", due.to_rfc3339()))
                .bind(now.to_rfc3339())
                .execute(pool)
                .await?;
            }
        }
    }
    if let Some(action) = crate::inbox_cards::trusted_action() {
        crate::inbox_cards::present_ready(pool, &action, lead).await?;
    }
    Ok(stored)
}

/// ISO dates/timestamps and explicit today/tomorrow deadlines, in owner KST.
/// Relative words resolve against `sent_at`, the moment the message was received.
/// No inference of dates from a model response. omo `parse_due_at`.
pub fn parse_due_at(text: &str, sent_at: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let re =
        regex::Regex::new(r"\b\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}:\d{2}(?:Z|[+-]\d{2}:\d{2}))?\b")
            .ok()?;
    if let Some(found) = re.find(text) {
        if let Ok(date) = DateTime::parse_from_rfc3339(found.as_str()) {
            return Some(date.with_timezone(&Utc));
        }
        let date = NaiveDate::parse_from_str(found.as_str(), "%Y-%m-%d").ok()?;
        return chrono_tz::Asia::Seoul
            .from_local_datetime(&date.and_hms_opt(23, 59, 59)?)
            .single()
            .map(|d| d.with_timezone(&Utc));
    }
    let lower = text.to_lowercase();
    let offset = if lower.contains("tomorrow") || text.contains("내일") {
        1
    } else if lower.contains("today") || text.contains("오늘") {
        0
    } else {
        return None;
    };
    let date =
        sent_at.with_timezone(&chrono_tz::Asia::Seoul).date_naive() + TimeDelta::days(offset);
    let clock = regex::Regex::new(r"\b([0-2]\d):([0-5]\d)\b").ok()?;
    let (hour, minute) = match clock.captures(text) {
        Some(found) => (
            found.get(1)?.as_str().parse().ok()?,
            found.get(2)?.as_str().parse().ok()?,
        ),
        None => (23, 59),
    };
    chrono_tz::Asia::Seoul
        .from_local_datetime(&date.and_hms_opt(hour, minute, 0)?)
        .single()
        .map(|d| d.with_timezone(&Utc))
}

pub const DIGEST_JOB_ID: &str = "omon:inbox-digest";
/// Conversations shown per digest section; the rest collapse into one count line.
const DIGEST_MAX_GROUPS: usize = 15;
/// Recent message previews kept per conversation.
const DIGEST_PREVIEWS: usize = 2;

#[derive(sqlx::FromRow)]
struct DigestRow {
    source: String,
    author: Option<String>,
    title: Option<String>,
    snippet: String,
    received_at: String,
    link: Option<String>,
}

struct DigestGroup {
    source: String,
    name: String,
    who: Option<String>,
    count: usize,
    latest: String,
    previews: Vec<String>,
    link: Option<String>,
}

/// Which conversation an item belongs to: a kakao room, a cron job, or a mail/GitHub thread.
fn digest_key(row: &DigestRow) -> String {
    let title = row
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());
    match row.source.as_str() {
        "cron" => title
            .map(|t| t.split_once(": needs you").map_or(t, |(job, _)| job))
            .unwrap_or("cron")
            .to_owned(),
        _ => title
            .or(row.author.as_deref())
            .unwrap_or(row.snippet.as_str())
            .to_owned(),
    }
}

fn kst_short(at: &str) -> String {
    DateTime::parse_from_rfc3339(at)
        .map(|d| {
            d.with_timezone(&chrono_tz::Asia::Seoul)
                .format("%m/%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| at.to_owned())
}

/// Rows arrive newest first; one entry per conversation keeps the newest previews and link.
fn group_digest_rows(rows: Vec<DigestRow>) -> Vec<DigestGroup> {
    use crate::owner_alert::one_line;
    let mut groups: Vec<DigestGroup> = Vec::new();
    for row in rows {
        let key = digest_key(&row);
        let snippet = match row.source.as_str() {
            "cron" => strip_cron_boilerplate(&row.snippet),
            _ => row.snippet.as_str(),
        };
        let preview = one_line(snippet, 80);
        let index = match groups
            .iter()
            .position(|g| g.source == row.source && g.name == key)
        {
            Some(index) => index,
            None => {
                let who = row
                    .author
                    .as_deref()
                    .map(|a| one_line(a, 40))
                    .filter(|a| !a.is_empty() && *a != key && row.source != "cron");
                groups.push(DigestGroup {
                    source: row.source.clone(),
                    name: key,
                    who,
                    count: 0,
                    latest: row.received_at.clone(),
                    previews: Vec::new(),
                    link: None,
                });
                groups.len() - 1
            }
        };
        let group = &mut groups[index];
        group.count += 1;
        if group.link.is_none() {
            group.link = row.link.filter(|l| !l.trim().is_empty());
        }
        if group.previews.len() < DIGEST_PREVIEWS
            && !preview.is_empty()
            && !group.previews.contains(&preview)
        {
            group.previews.push(preview);
        }
    }
    groups
}

/// A cron failure snippet opens with "cron script failed with Some(N): "; the cause follows it.
fn strip_cron_boilerplate(snippet: &str) -> &str {
    let trimmed = snippet.trim_start();
    trimmed
        .strip_prefix("cron script failed with ")
        .and_then(|rest| rest.split_once(": "))
        .map_or(trimmed, |(_, cause)| cause)
}

/// One readable block per conversation: where, who, how many, when, and what was said.
fn render_digest_section(heading: &str, rows: Vec<DigestRow>) -> String {
    use crate::owner_alert::{one_line, source_label};
    let total = rows.len();
    let groups = group_digest_rows(rows);
    if groups.is_empty() {
        return format!("## {heading}\n없음");
    }
    let mut out = format!("## {heading} — {}곳, {total}건", groups.len());
    for group in groups.iter().take(DIGEST_MAX_GROUPS) {
        out.push_str(&format!(
            "\n- [{}] {}",
            source_label(&group.source),
            one_line(&group.name, 60)
        ));
        if let Some(who) = &group.who {
            out.push_str(&format!(" ({who})"));
        }
        if group.count > 1 {
            out.push_str(&format!(" · {}건", group.count));
        }
        out.push_str(&format!(" · 최근 {}", kst_short(&group.latest)));
        for preview in &group.previews {
            out.push_str(&format!("\n  > {preview}"));
        }
        if let Some(link) = &group.link {
            out.push_str(&format!("\n  {link}"));
        }
    }
    if groups.len() > DIGEST_MAX_GROUPS {
        let mut rest: Vec<&DigestGroup> = groups[DIGEST_MAX_GROUPS..].iter().collect();
        let count: usize = rest.iter().map(|g| g.count).sum();
        out.push_str(&format!("\n- 외 {}곳, {count}건", rest.len()));
        rest.sort_by_key(|g| std::cmp::Reverse(g.count));
        let busiest: Vec<String> = rest
            .iter()
            .take(3)
            .map(|g| format!("{} {}", one_line(&g.name, 30), g.count))
            .collect();
        out.push_str(&format!(" ({} …)", busiest.join(", ")));
    }
    out
}

/// One KST-day digest. A succeeded run already stored today suppresses a second copy.
/// Open owner and bot decisions, auth-expired cursors, and yesterday's cron count are the body.
pub async fn build_scheduled_digest(
    pool: &SqlitePool,
    now: DateTime<Utc>,
) -> Result<Option<String>> {
    let local = now.with_timezone(&chrono_tz::Asia::Seoul);
    let midnight = local
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|naive| naive.and_local_timezone(chrono_tz::Asia::Seoul).single())
        .map(|at| at.with_timezone(&Utc))
        .unwrap_or(now);
    let delivered: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cron_runs
         WHERE job_id = ? AND status = 'succeeded' AND output <> ''
           AND finished_at IS NOT NULL AND julianday(finished_at) >= julianday(?)",
    )
    .bind(DIGEST_JOB_ID)
    .bind(midnight.to_rfc3339())
    .fetch_one(pool)
    .await?;
    if delivered > 0 {
        return Ok(None);
    }
    let open_rows = |class: &'static str| {
        sqlx::query_as::<_, DigestRow>(
            "SELECT i.source, i.author, i.title, i.snippet, i.received_at, i.link
               FROM inbox_decisions d JOIN inbox_items i ON i.id = d.item_id
              WHERE d.class = ? AND d.status = 'open'
              ORDER BY julianday(i.received_at) DESC, i.id",
        )
        .bind(class)
        .fetch_all(pool)
    };
    let owner = open_rows("owner").await?;
    let bot = open_rows("bot").await?;
    let errors: Vec<(String,)> = sqlx::query_as(
        "SELECT source FROM inbox_cursors WHERE error IS NOT NULL AND (
            lower(error) LIKE '%auth_expired%' OR lower(error) LIKE '%unauthorized%'
            OR lower(error) LIKE '%login%' OR error LIKE '%401%') ORDER BY source",
    )
    .fetch_all(pool)
    .await?;
    let since = (now - chrono::Duration::hours(24)).to_rfc3339();
    let runs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cron_runs WHERE finished_at IS NOT NULL AND julianday(finished_at) >= julianday(?)",
    )
    .bind(&since)
    .fetch_one(pool)
    .await?;
    let failed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cron_runs WHERE status = 'failed' AND finished_at IS NOT NULL AND julianday(finished_at) >= julianday(?)",
    )
    .bind(&since)
    .fetch_one(pool)
    .await?;
    if owner.is_empty() && bot.is_empty() && errors.is_empty() && runs == 0 {
        return Ok(None);
    }
    let mut message = format!(
        "{}\n\n{}",
        render_digest_section("대표 확인 필요", owner),
        render_digest_section("봇 처리 제안", bot)
    );
    message.push_str(&format!("\n\nUsage: {runs} cron runs in the last day"));
    // omo digest: yesterday's judge and inbox prefilter counts.
    let usage = crate::usage::yesterday(pool, now).await?;
    message.push_str(&format!("\n{}", usage.digest_line()));
    let remediation = crate::cron_remediate::digest_section(pool, now).await?;
    if failed > 0 || remediation.is_some() {
        message.push_str("\n\n## Cron");
        if failed > 0 {
            message.push_str(&format!("\n- {failed} failed"));
        }
        if let Some(section) = remediation {
            for line in section.lines().skip(1) {
                message.push('\n');
                message.push_str(line);
            }
        }
    }
    if !errors.is_empty() {
        message.push_str("\n\n## Source authentication\n");
        message.push_str(
            &errors
                .iter()
                .map(|(source,)| format!("- [{source}] login required"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    Ok(Some(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_mail_is_fyi_and_a_fresh_item_needs_a_real_class() {
        let now = DateTime::parse_from_rfc3339("2026-10-22T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let old = now - TimeDelta::hours(25);
        assert_eq!(classify(old, now, None).unwrap(), Class::Fyi);
        assert_eq!(
            classify(old, now, Some("owner_must_act")).unwrap(),
            Class::Fyi
        );
        let fresh = now - TimeDelta::hours(1);
        assert!(classify(fresh, now, None).is_err());
        assert_eq!(
            classify(fresh, now, Some("bot_can_handle")).unwrap(),
            Class::Bot
        );
        let decoded = decode_structured_output(b"{\"id\":1}\n{\"id\":2}\n").unwrap();
        assert_eq!(decoded.as_array().unwrap().len(), 2);
        let yaml = decode_structured_output(b"- owner_must_act\n- fyi\n").unwrap();
        assert_eq!(yaml[0], "owner_must_act");
        let digest = digest(&[("pay".into(), Class::Owner), ("ci".into(), Class::Fyi)]);
        assert!(digest.contains("pay"));
        assert!(digest.contains("## fyi"));
    }

    #[test]
    fn the_owner_digest_groups_a_room_and_shows_who_when_and_what() {
        let row = |source: &str, author: &str, title: &str, snippet: &str, at: &str| DigestRow {
            source: source.into(),
            author: Some(author.into()),
            title: Some(title.into()),
            snippet: snippet.into(),
            received_at: at.into(),
            link: None,
        };
        let rows = vec![
            row(
                "kakao",
                "여신님",
                "여신님",
                "도착 시간 톡해주세요",
                "2026-10-26T07:20:59+00:00",
            ),
            row(
                "kakao",
                "김진옥",
                "이모님",
                "청약서 보낼까",
                "2026-10-26T05:00:00+00:00",
            ),
            row(
                "kakao",
                "여신님",
                "여신님",
                "긴급",
                "2026-10-26T02:37:52+00:00",
            ),
            row(
                "kakao",
                "여신님",
                "여신님",
                "호시는?",
                "2026-10-26T01:00:00+00:00",
            ),
            row(
                "cron",
                "omon",
                "job-a: needs you - failed: boom",
                "cron script failed with Some(1): boom",
                "2026-10-26T00:00:00+00:00",
            ),
            row(
                "cron",
                "omon",
                "job-a: needs you - failed: again",
                "again",
                "2026-10-25T00:00:00+00:00",
            ),
        ];
        let text = render_digest_section("대표 확인 필요", rows);
        assert_eq!(
            text,
            "## 대표 확인 필요 — 3곳, 6건\n\
             - [카톡] 여신님 · 3건 · 최근 10/26 16:20\n  > 도착 시간 톡해주세요\n  > 긴급\n\
             - [카톡] 이모님 (김진옥) · 최근 10/26 14:00\n  > 청약서 보낼까\n\
             - [크론] job-a · 2건 · 최근 10/26 09:00\n  > boom\n  > again"
        );
        assert_eq!(
            render_digest_section("봇 처리 제안", Vec::new()),
            "## 봇 처리 제안\n없음"
        );
        let mut many: Vec<DigestRow> = (0..DIGEST_MAX_GROUPS)
            .map(|n| {
                row(
                    "mail",
                    "a",
                    &format!("t{n}"),
                    "s",
                    "2026-10-26T00:00:00+00:00",
                )
            })
            .collect();
        many.push(row("kakao", "x", "방1", "s", "2026-10-25T00:00:00+00:00"));
        many.push(row("kakao", "y", "방2", "s", "2026-10-25T00:00:00+00:00"));
        many.push(row("kakao", "y", "방2", "s2", "2026-10-24T00:00:00+00:00"));
        assert!(render_digest_section("x", many).ends_with("\n- 외 2곳, 3건 (방2 2, 방1 1 …)"));
    }

    #[tokio::test]
    async fn a_bot_item_becomes_a_proposal_and_a_stale_one_does_not() {
        let pool = crate::storage::memory().await.unwrap();
        ingest(
            &pool,
            "in1",
            "github",
            "ref",
            "[CI failed] red",
            "2026-10-22T00:00:00Z",
        )
        .await
        .unwrap();
        decide(
            &pool,
            "in1",
            Class::Bot,
            "agent can rerun",
            Some("rerun checks"),
        )
        .await
        .unwrap();
        let proposals: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM action_proposals WHERE origin = 'inbox'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(proposals, 1);
        ingest(
            &pool,
            "in2",
            "mail",
            "old",
            "newsletter",
            "2020-01-01T00:00:00Z",
        )
        .await
        .unwrap();
        let now = Utc::now();
        let received = DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let class = classify(received, now, None).unwrap();
        decide(&pool, "in2", class, "stale", None).await.unwrap();
        let stored: String =
            sqlx::query_scalar("SELECT class FROM inbox_decisions WHERE item_id = 'in2'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, "fyi");
    }

    #[tokio::test]
    async fn triage_files_stale_mail_and_keeps_an_old_ci_failure() {
        let pool = crate::storage::memory().await.unwrap();
        let now = DateTime::parse_from_rfc3339("2026-10-22T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        ingest(
            &pool,
            "old",
            "mail",
            "m1",
            "newsletter",
            "2020-01-01T00:00:00Z",
        )
        .await
        .unwrap();
        ingest(
            &pool,
            "ci",
            "github",
            "g1",
            "[CI failed] red",
            "2020-01-01T00:00:00Z",
        )
        .await
        .unwrap();
        ingest(
            &pool,
            "fresh",
            "slack",
            "s1",
            "please look",
            "2026-10-21T23:00:00Z",
        )
        .await
        .unwrap();
        let judge = crate::judge::Judge::forced("bot_can_handle");
        let stored = classify_pending(&pool, &judge, now, "meta", |_id, class| async move {
            assert_eq!(class, Class::Bot);
            Ok(Some("investigate failure".into()))
        })
        .await
        .unwrap();
        assert_eq!(stored, 3);
        let stale: String =
            sqlx::query_scalar("SELECT class FROM inbox_decisions WHERE item_id = 'old'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stale, "fyi");
        let ci: String =
            sqlx::query_scalar("SELECT class FROM inbox_decisions WHERE item_id = 'ci'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(ci, "bot");
        let proposals: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM action_proposals WHERE origin = 'inbox'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(proposals, 2);
    }

    #[test]
    fn a_due_date_is_the_stamp_in_the_text() {
        let sent = DateTime::parse_from_rfc3339("2026-10-22T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let iso = parse_due_at("need this by 2026-10-23T03:00:00Z", sent).unwrap();
        assert_eq!(iso.to_rfc3339(), "2026-10-23T03:00:00+00:00");
        let today = parse_due_at("오늘 18:30", sent).unwrap();
        assert_eq!(
            today
                .with_timezone(&chrono_tz::Asia::Seoul)
                .format("%H:%M")
                .to_string(),
            "18:30"
        );
        assert!(parse_due_at("no deadline here", sent).is_none());
    }

    #[tokio::test]
    async fn an_owner_item_due_within_a_day_enqueues_inbox_urgent_once() {
        let pool = crate::storage::memory().await.unwrap();
        let now = DateTime::parse_from_rfc3339("2026-10-22T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let soon = (now + TimeDelta::hours(2)).to_rfc3339();
        let later = (now + TimeDelta::hours(48)).to_rfc3339();
        ingest(
            &pool,
            "soon",
            "mail",
            "s",
            &format!("reply {soon}"),
            &now.to_rfc3339(),
        )
        .await
        .unwrap();
        ingest(
            &pool,
            "later",
            "mail",
            "l",
            &format!("reply {later}"),
            &now.to_rfc3339(),
        )
        .await
        .unwrap();
        let lead = "slack:dm:D0OWNER";
        let judge = crate::judge::Judge::forced("owner_must_act");
        let stored = classify_pending(&pool, &judge, now, lead, |_id, class| async move {
            assert_eq!(class, Class::Owner);
            Ok(None)
        })
        .await
        .unwrap();
        assert_eq!(stored, 2);
        let events: Vec<(String, String, String)> =
            sqlx::query_as("SELECT kind, lead_session_key, body FROM lane_events ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "inbox_urgent");
        assert_eq!(events[0].1, lead);
        assert!(events[0].2.starts_with("inbox:soon due "));
        assert!(events[0].2.contains(&soon));
        let due: String =
            sqlx::query_scalar("SELECT due_at FROM inbox_decisions WHERE item_id = 'later'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(due.contains("2026-10-24"));
    }

    async fn chat(pool: &SqlitePool, id: &str, thread: &str, author: &str, text: &str, at: &str) {
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, thread_ref, author, title, snippet, received_at, raw_hash, collected_at)
             VALUES(?, 'kakao', ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(id)
        .bind(thread)
        .bind(author)
        .bind(format!("room {thread}"))
        .bind(text)
        .bind(at)
        .bind(id)
        .bind(at)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn open_ids(pool: &SqlitePool) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT item_id FROM inbox_decisions WHERE status = 'open' AND class = 'owner' ORDER BY item_id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_new_owner_item_supersedes_the_older_open_ones_of_its_conversation() {
        let pool = crate::storage::memory().await.unwrap();
        let now = DateTime::parse_from_rfc3339("2026-10-26T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        chat(
            &pool,
            "a1",
            "A",
            "여신님",
            "호시는?",
            "2026-10-26T01:00:00+00:00",
        )
        .await;
        chat(
            &pool,
            "a2",
            "A",
            "여신님",
            "긴급",
            "2026-10-26T02:00:00+00:00",
        )
        .await;
        chat(
            &pool,
            "b1",
            "B",
            "김진옥",
            "청약서 보낼까",
            "2026-10-26T01:30:00+00:00",
        )
        .await;
        let judge = crate::judge::Judge::forced("owner_must_act");
        let stored = classify_pending(
            &pool,
            &judge,
            now,
            "meta",
            |_id, _class| async move { Ok(None) },
        )
        .await
        .unwrap();
        assert_eq!(stored, 3);
        assert_eq!(
            open_ids(&pool).await,
            vec!["a2".to_owned(), "b1".to_owned()]
        );
        chat(
            &pool,
            "a3",
            "A",
            "여신님",
            "도착 시간 톡해주세요",
            "2026-10-26T03:00:00+00:00",
        )
        .await;
        classify_pending(
            &pool,
            &judge,
            now,
            "meta",
            |_id, _class| async move { Ok(None) },
        )
        .await
        .unwrap();
        assert_eq!(
            open_ids(&pool).await,
            vec!["a3".to_owned(), "b1".to_owned()]
        );
        let dismissed: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM inbox_decisions WHERE status = 'dismissed'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(dismissed, 2);
    }

    #[tokio::test]
    async fn reconciliation_keeps_the_newest_open_owner_item_per_conversation_once() {
        let pool = crate::storage::memory().await.unwrap();
        for (id, thread, at) in [
            ("a1", "A", "2026-10-26T01:00:00+00:00"),
            ("a2", "A", "2026-10-26T02:00:00+00:00"),
            ("a3", "A", "2026-10-26T03:00:00+00:00"),
            ("b1", "B", "2026-10-26T01:00:00+00:00"),
            ("b2", "B", "2026-10-26T04:00:00+00:00"),
        ] {
            chat(&pool, id, thread, "friend", "hi", at).await;
            decide(&pool, id, Class::Owner, "owner_must_act", None)
                .await
                .unwrap();
        }
        ingest(
            &pool,
            "m1",
            "mail",
            "m1",
            "pay",
            "2026-10-26T00:00:00+00:00",
        )
        .await
        .unwrap();
        decide(&pool, "m1", Class::Owner, "owner_must_act", None)
            .await
            .unwrap();
        assert_eq!(reconcile_owner_threads(&pool).await.unwrap(), 3);
        assert_eq!(
            open_ids(&pool).await,
            vec!["a3".to_owned(), "b2".to_owned(), "m1".to_owned()]
        );
        assert_eq!(reconcile_owner_threads(&pool).await.unwrap(), 0);
        assert_eq!(open_ids(&pool).await.len(), 3);
    }

    #[tokio::test]
    async fn the_judged_text_carries_the_room_title_and_earlier_lines() {
        let pool = crate::storage::memory().await.unwrap();
        chat(
            &pool,
            "a1",
            "A",
            "여신님",
            "내일 저녁\n어때?",
            "2026-10-26T01:00:00+00:00",
        )
        .await;
        chat(
            &pool,
            "a2",
            "A",
            "윤인도",
            "좋아",
            "2026-10-26T02:00:00+00:00",
        )
        .await;
        chat(
            &pool,
            "a3",
            "A",
            "여신님",
            "호시는?",
            "2026-10-26T03:00:00+00:00",
        )
        .await;
        chat(
            &pool,
            "b1",
            "B",
            "other",
            "unrelated",
            "2026-10-26T01:30:00+00:00",
        )
        .await;
        let earlier = earlier_in_thread(&pool, "a3", "kakao", "A", "2026-10-26T03:00:00+00:00")
            .await
            .unwrap();
        let text = triage_text(Some("room A"), &earlier, Some("여신님"), "호시는?");
        assert_eq!(
            text,
            "Conversation: room A\n\
             Earlier messages (oldest first):\n\
             - 여신님: 내일 저녁 어때?\n\
             - 윤인도: 좋아\n\
             Message to classify:\n>>> 여신님: 호시는?"
        );
        let long: Vec<(Option<String>, String)> = (0..6)
            .map(|n| (None, format!("{n}{}", "x".repeat(400))))
            .collect();
        let capped = triage_text(Some("room A"), &long, None, "new");
        assert!(capped.chars().count() <= TRIAGE_TEXT_MAX_CHARS);
        assert!(capped.ends_with(">>> new"));
        assert!(capped.contains("- 5x"), "the newest earlier line is kept");
        assert!(
            !capped.contains("- 0x"),
            "the oldest earlier line is dropped"
        );
    }
}
