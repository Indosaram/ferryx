//! omo-gateway's missed-message backfill (`slack/backfill.rs`): replays Slack messages that
//! arrived while the hub was down, and re-sweeps every two minutes because a Socket Mode
//! connection can stay open while it stops delivering events.
//!
//! Targets are the free-response channels, the owner DM, every stored cursor and every thread
//! session of this bot; thread targets untouched for 7 days leave the sweep, ignored channels
//! never enter it. Each target is read after its cursor (`conversations.history` for channels,
//! `conversations.replies` for threads), deduped by `ts`, oldest first, mapped with the live
//! admission rules and run through the same turn path. The cursor only moves forward. A target
//! with no cursor yet is seeded at its newest message instead of replaying its history, and a
//! backlog deeper than the page budget is held (not truncated) so nothing is stepped over.
//! Live turns advance the cursor too, and the in-process delivery set keeps a message the
//! socket and a sweep both see from running twice.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Duration;

use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::session::SessionKey;
use super::slack_ingress::ts_key;
use super::slack_turn::SlackTurnConfig;
use crate::error::Result;

const PAGE_LIMIT: &str = "200";
const MAX_PAGES: usize = 50;
const THREAD_TARGET_WINDOW_DAYS: i64 = 7;
/// omo `RECONCILE_INTERVAL` / `RECONCILE_SETTLE`.
pub const RECONCILE_INTERVAL: Duration = Duration::from_secs(120);
pub const RECONCILE_SETTLE: Duration = Duration::from_secs(60);

pub async fn read_cursor(
    pool: &SqlitePool,
    bot: &str,
    channel: &str,
    thread: &str,
) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT last_ts FROM slack_cursors WHERE bot_id = ? AND channel_id = ? AND thread_ts = ?",
    )
    .bind(bot)
    .bind(channel)
    .bind(thread)
    .fetch_optional(pool)
    .await?)
}

/// Moves the cursor forward only; an older `ts` never rewinds it.
pub async fn write_cursor(
    pool: &SqlitePool,
    bot: &str,
    channel: &str,
    thread: &str,
    ts: &str,
) -> Result<()> {
    if let Some(current) = read_cursor(pool, bot, channel, thread).await? {
        if ts_key(&current) >= ts_key(ts) {
            return Ok(());
        }
    }
    sqlx::query(
        "INSERT INTO slack_cursors (bot_id, channel_id, thread_ts, last_ts, updated_at) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(bot_id, channel_id, thread_ts) DO UPDATE SET last_ts = excluded.last_ts, updated_at = excluded.updated_at",
    )
    .bind(bot)
    .bind(channel)
    .bind(thread)
    .bind(ts)
    .bind(crate::storage::now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Who the bot is: `auth.test`'s user and team.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotIdentity {
    pub user_id: String,
    pub team_id: String,
}

pub async fn identity() -> Result<BotIdentity> {
    let auth = super::slack_web::post_form("auth.test", &[]).await?;
    let field = |name: &str| {
        auth.get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let user_id = field("user_id");
    if user_id.is_empty() {
        return Err(crate::HubError::message("auth.test returned no user_id"));
    }
    Ok(BotIdentity {
        user_id,
        team_id: field("team_id"),
    })
}

/// One pass over every target. `settle` leaves messages younger than it to live ingress.
/// Returns how many messages ran as turns.
pub async fn run(
    config: &SlackTurnConfig,
    pool: &SqlitePool,
    bot: &BotIdentity,
    settle: Option<Duration>,
) -> Result<usize> {
    let window = format!("-{THREAD_TARGET_WINDOW_DAYS} days");
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT channel_id, thread_ts, last_ts FROM slack_cursors
         WHERE bot_id = ?
           AND (thread_ts = '' OR julianday(updated_at) IS NULL OR julianday(updated_at) >= julianday('now', ?))",
    )
    .bind(&bot.user_id)
    .bind(&window)
    .fetch_all(pool)
    .await?;
    let mut snapshot: HashMap<(String, String), String> = HashMap::new();
    let mut targets: BTreeSet<(String, String)> = BTreeSet::new();
    for (channel, thread, last) in rows {
        targets.insert((channel.clone(), thread.clone()));
        snapshot.insert((channel, thread), last);
    }
    for channel in &config.free_channels {
        targets.insert((channel.clone(), String::new()));
    }
    if let Some(owner) = config.allowed_users.first() {
        match super::slack_web::post_form("conversations.open", &[("users", owner)]).await {
            Ok(opened) => {
                if let Some(channel) = opened.pointer("/channel/id").and_then(Value::as_str) {
                    targets.insert((channel.to_owned(), String::new()));
                }
            }
            Err(error) => tracing::warn!(%error, "slack backfill could not open the owner DM"),
        }
    }
    let sessions: Vec<String> = sqlx::query_scalar(
        "SELECT session_key FROM sessions
         WHERE platform = 'slack' AND thread_id IS NOT NULL
           AND (julianday(updated_at) IS NULL OR julianday(updated_at) >= julianday('now', ?))",
    )
    .bind(&window)
    .fetch_all(pool)
    .await?;
    for key in sessions
        .iter()
        .filter_map(|raw| SessionKey::from_storage_key(raw).ok())
    {
        if key.bot_id.as_deref() == Some(bot.user_id.as_str()) {
            if let Some(thread) = key.thread_id {
                targets.insert((key.channel_id, thread));
            }
        }
    }
    targets.retain(|(channel, _)| !config.ignored_channels.contains(channel));

    let mut routed = 0;
    let mut failed = 0usize;
    for (channel, thread) in targets {
        let cursor = match snapshot.get(&(channel.clone(), thread.clone())) {
            Some(cursor) => Some(cursor.clone()),
            None => read_cursor(pool, &bot.user_id, &channel, &thread).await?,
        }
        .filter(|cursor| !cursor.trim().is_empty());
        match backfill_target(
            config, pool, bot, &channel, &thread, cursor, settle, MAX_PAGES,
        )
        .await
        {
            Ok(count) => routed += count,
            Err(error) => {
                failed += 1;
                tracing::warn!(%error, channel, thread, "slack backfill target failed");
            }
        }
    }
    if failed > 0 {
        tracing::warn!(
            failed,
            routed,
            "slack backfill pass left targets unread; the next pass retries them"
        );
    }
    Ok(routed)
}

#[allow(clippy::too_many_arguments)]
async fn backfill_target(
    config: &SlackTurnConfig,
    pool: &SqlitePool,
    bot: &BotIdentity,
    channel: &str,
    thread: &str,
    cursor: Option<String>,
    settle: Option<Duration>,
    pages: usize,
) -> Result<usize> {
    let Some(cursor) = cursor else {
        seed_target(pool, &bot.user_id, channel, thread).await?;
        return Ok(0);
    };
    let settled_before = settle.map(|settle| {
        let cutoff = chrono::Utc::now() - chrono::Duration::from_std(settle).unwrap_or_default();
        (
            cutoff.timestamp().max(0) as u64,
            cutoff.timestamp_subsec_micros(),
        )
    });
    let Some(mut messages) = fetch_messages(channel, thread, &cursor, pages).await? else {
        tracing::warn!(
            channel,
            thread,
            pages,
            "slack backfill window did not reach the cursor; holding it so nothing is skipped"
        );
        return Ok(0);
    };
    messages.retain(|m| {
        m.get("ts")
            .and_then(Value::as_str)
            .is_some_and(|ts| ts != thread && ts_key(ts) > ts_key(&cursor))
    });
    let mut routed = 0;
    for message in messages {
        let ts = message["ts"].as_str().unwrap_or_default().to_owned();
        if settled_before.is_some_and(|cutoff| ts_key(&ts) > cutoff) {
            break;
        }
        let event = replay_event(message, channel, thread, &bot.user_id);
        let payload = json!({
            "team_id": bot.team_id,
            "authorizations": [{"user_id": bot.user_id}],
            "event": event,
        });
        let known = super::slack_turn::known_thread(pool, &payload).await;
        if let Some(admitted) = super::slack_turn::admit(config, &payload, &bot.user_id, known) {
            if super::slack_turn::handle(config, pool, admitted).await {
                routed += 1;
            }
        }
        write_cursor(pool, &bot.user_id, channel, thread, &ts).await?;
    }
    Ok(routed)
}

async fn seed_target(pool: &SqlitePool, bot: &str, channel: &str, thread: &str) -> Result<()> {
    let page = if thread.is_empty() {
        super::slack_web::post_form(
            "conversations.history",
            &[("channel", channel), ("limit", PAGE_LIMIT)],
        )
        .await?
    } else {
        super::slack_web::post_form(
            "conversations.replies",
            &[("channel", channel), ("ts", thread), ("limit", PAGE_LIMIT)],
        )
        .await?
    };
    if let Some(newest) = messages_of(&page)
        .iter()
        .filter_map(|m| m.get("ts").and_then(Value::as_str))
        .max_by_key(|ts| ts_key(ts))
    {
        write_cursor(pool, bot, channel, thread, newest).await?;
    }
    Ok(())
}

fn messages_of(page: &Value) -> Vec<Value> {
    page.get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Messages after `cursor`, deduped and oldest first; `None` when the page budget ran out
/// before a channel walk (newest first) reached back to the cursor.
async fn fetch_messages(
    channel: &str,
    thread: &str,
    cursor: &str,
    pages: usize,
) -> Result<Option<Vec<Value>>> {
    let mut messages: Vec<Value> = Vec::new();
    if thread.is_empty() {
        let mut latest: Option<String> = None;
        let mut older_unfetched = false;
        for page_index in 0..pages {
            let mut fields = vec![
                ("channel", channel),
                ("oldest", cursor),
                ("limit", PAGE_LIMIT),
            ];
            if let Some(latest) = latest.as_deref() {
                fields.push(("latest", latest));
            }
            let page = super::slack_web::post_form("conversations.history", &fields).await?;
            let batch = messages_of(&page);
            let oldest = batch
                .iter()
                .filter_map(|m| m.get("ts").and_then(Value::as_str))
                .min_by_key(|ts| ts_key(ts))
                .map(str::to_owned);
            let has_more = page
                .get("has_more")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            messages.extend(batch);
            match oldest {
                Some(oldest) if has_more && latest.as_deref() != Some(oldest.as_str()) => {
                    latest = Some(oldest);
                    older_unfetched = page_index + 1 == pages;
                }
                _ => {
                    older_unfetched = false;
                    break;
                }
            }
        }
        if older_unfetched {
            return Ok(None);
        }
        return Ok(Some(sorted(dedupe(messages))));
    }
    let mut next: Option<String> = None;
    for index in 0..pages {
        let mut fields = vec![("channel", channel), ("ts", thread), ("limit", PAGE_LIMIT)];
        if index == 0 {
            fields.push(("oldest", cursor));
        }
        if let Some(next) = next.as_deref() {
            fields.push(("cursor", next));
        }
        let page = super::slack_web::post_form("conversations.replies", &fields).await?;
        messages.extend(messages_of(&page));
        let next_cursor = page
            .pointer("/response_metadata/next_cursor")
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
            .map(str::to_owned);
        if next_cursor.is_none() || next_cursor == next {
            break;
        }
        next = next_cursor;
    }
    Ok(Some(sorted(dedupe(messages))))
}

/// Page boundaries can repeat a message; keep one copy of each `ts`.
fn dedupe(messages: Vec<Value>) -> Vec<Value> {
    let mut seen = HashSet::new();
    messages
        .into_iter()
        .filter(|m| {
            m.get("ts")
                .and_then(Value::as_str)
                .is_none_or(|ts| seen.insert(ts.to_owned()))
        })
        .collect()
}

fn sorted(mut messages: Vec<Value>) -> Vec<Value> {
    messages.sort_by_key(|m| ts_key(m.get("ts").and_then(Value::as_str).unwrap_or_default()));
    messages
}

/// History messages carry no channel; a channel @mention is replayed as `app_mention` because
/// live ingress answers channel mentions through that event.
pub fn replay_event(mut message: Value, channel: &str, thread: &str, bot: &str) -> Value {
    let dm = channel.starts_with('D');
    let mentioned = message
        .get("text")
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains(&format!("<@{bot}>")));
    let kind = if mentioned && !dm {
        "app_mention"
    } else {
        "message"
    };
    if let Some(map) = message.as_object_mut() {
        map.insert("type".into(), json!(kind));
        map.insert("channel".into(), json!(channel));
        if !thread.is_empty() {
            map.entry("thread_ts").or_insert_with(|| json!(thread));
        }
        map.insert(
            "channel_type".into(),
            json!(if dm { "im" } else { "channel" }),
        );
    }
    message
}

/// The startup pass, then a settled sweep every [`RECONCILE_INTERVAL`] (omo
/// `SLACK_MISSED_BACKFILL`, default on).
pub async fn run_forever(config: &'static SlackTurnConfig, pool: SqlitePool) {
    if !super::slack_ingress::flag("MISSED_BACKFILL", true) {
        tracing::info!("slack missed-message backfill is off (SLACK_MISSED_BACKFILL)");
        return;
    }
    let bot = loop {
        match identity().await {
            Ok(bot) => break bot,
            Err(error) => {
                tracing::warn!(%error, "slack backfill could not identify the bot; retrying");
                tokio::time::sleep(RECONCILE_INTERVAL).await;
            }
        }
    };
    let mut settle = None;
    loop {
        match run(config, &pool, &bot, settle).await {
            Ok(0) => {}
            Ok(routed) if settle.is_none() => {
                tracing::info!(routed, "slack missed-message backfill routed messages");
            }
            Ok(routed) => tracing::warn!(
                routed,
                "slack reconcile routed messages the socket never delivered"
            ),
            Err(error) => tracing::warn!(%error, "slack backfill failed"),
        }
        settle = Some(RECONCILE_SETTLE);
        tokio::time::sleep(RECONCILE_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_cursor_only_moves_forward() {
        let pool = crate::storage::memory().await.unwrap();
        write_cursor(&pool, "UBOT", "C1", "", "10.000002")
            .await
            .unwrap();
        write_cursor(&pool, "UBOT", "C1", "", "9.9").await.unwrap();
        assert_eq!(
            read_cursor(&pool, "UBOT", "C1", "")
                .await
                .unwrap()
                .as_deref(),
            Some("10.000002")
        );
        write_cursor(&pool, "UBOT", "C1", "", "10.5").await.unwrap();
        assert_eq!(
            read_cursor(&pool, "UBOT", "C1", "")
                .await
                .unwrap()
                .as_deref(),
            Some("10.5")
        );
        assert_eq!(read_cursor(&pool, "UBOT", "C1", "1.0").await.unwrap(), None);
    }

    #[test]
    fn replayed_history_looks_like_live_events() {
        let mention = replay_event(
            json!({"ts": "2.0", "user": "U", "text": "<@UBOT> hi"}),
            "C1",
            "",
            "UBOT",
        );
        assert_eq!(mention["type"], "app_mention");
        assert_eq!(mention["channel"], "C1");
        assert_eq!(mention["channel_type"], "channel");
        let dm = replay_event(
            json!({"ts": "2.0", "user": "U", "text": "<@UBOT> hi"}),
            "D1",
            "1.0",
            "UBOT",
        );
        assert_eq!(dm["type"], "message");
        assert_eq!(dm["thread_ts"], "1.0");
        assert_eq!(dm["channel_type"], "im");
        let pages = vec![
            json!({"ts": "3.0"}),
            json!({"ts": "1.0"}),
            json!({"ts": "3.0"}),
        ];
        let ordered: Vec<_> = sorted(dedupe(pages))
            .iter()
            .map(|m| m["ts"].clone())
            .collect();
        assert_eq!(ordered, vec![json!("1.0"), json!("3.0")]);
    }
}
