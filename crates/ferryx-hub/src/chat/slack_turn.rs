//! Slack message → omo app-server turn → threaded Slack reply, the path omo-gateway served.
//!
//! Admission mirrors omo-gateway's `map_event`: only allowed users (`SLACK_ALLOWED_USERS`, the
//! hub owner, or omo's default owner); DMs always; in channels, an `app_mention`, any message in a
//! free-response channel (`SLACK_HOME_CHANNEL`, `SLACK_FREE_RESPONSE_CHANNELS`), or a reply in a
//! thread the bot already talks in. A top-level channel message roots its own thread.
//!
//! The session key is omo-gateway's, so a Slack thread omo already answered resumes the same
//! engine thread (`metadata.omo_thread_id` in `sessions`). One turn per session runs at a time.
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::session::{SessionKey, SessionState};
use crate::error::Result;
use crate::meta::omo_client::{self, OmoClientConfig, TurnRequest};

pub const DEFAULT_OWNER_ID: &str = "U04M1JNJM8T";
const PLATFORM: &str = "slack";

#[derive(Debug, Clone)]
pub struct SlackTurnConfig {
    pub omo: OmoClientConfig,
    /// Re-read before each turn so a restarted app-server's new token is picked up.
    pub token_path: Option<std::path::PathBuf>,
    pub allowed_users: Vec<String>,
    pub free_channels: Vec<String>,
    pub default_model: Option<String>,
}

fn split_list(raw: Option<String>) -> Vec<String> {
    raw.unwrap_or_default()
        .split([',', ' ', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

impl SlackTurnConfig {
    pub fn from_env(
        url: &str,
        token_path: Option<std::path::PathBuf>,
        owner: Option<&str>,
    ) -> Self {
        let var = |name: &str| crate::hub_env::var(name).ok();
        let mut allowed_users = split_list(var("SLACK_ALLOWED_USERS"));
        if let Some(owner) = owner.map(str::trim).filter(|owner| !owner.is_empty()) {
            if !allowed_users.iter().any(|id| id == owner) {
                allowed_users.push(owner.to_owned());
            }
        }
        if allowed_users.is_empty() {
            allowed_users.push(DEFAULT_OWNER_ID.into());
        }
        let mut free_channels = split_list(var("SLACK_HOME_CHANNEL"));
        for channel in split_list(var("SLACK_FREE_RESPONSE_CHANNELS")) {
            if !free_channels.contains(&channel) {
                free_channels.push(channel);
            }
        }
        let mut omo = OmoClientConfig::new(url).with_env_timeouts();
        omo.workspace = omo_client::slack_workspace();
        Self {
            omo,
            token_path,
            allowed_users,
            free_channels,
            default_model: var("OMON_DEFAULT_MODEL")
                .or_else(|| var("DEFAULT_MODEL"))
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty()),
        }
    }
}

static INSTALLED: OnceLock<SlackTurnConfig> = OnceLock::new();

/// Turns Slack messages into omo turns for the rest of the process.
pub fn install(config: SlackTurnConfig) -> bool {
    INSTALLED.set(config).is_ok()
}

pub fn installed() -> Option<&'static SlackTurnConfig> {
    INSTALLED.get()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted {
    pub key: SessionKey,
    pub channel: String,
    /// Thread the reply goes to. `None` only for a top-level DM message.
    pub reply_thread: Option<String>,
    pub message_ts: String,
    pub user: String,
    pub text: String,
}

fn str_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

fn is_dm(event: &Value, channel: &str) -> bool {
    match str_field(event, "channel_type") {
        Some(kind) => kind == "im",
        None => channel.starts_with('D'),
    }
}

/// `thread_ts` when it differs from the message's own ts (a reply inside a thread).
pub fn reply_thread_ts(event: &Value) -> Option<&str> {
    let thread = str_field(event, "thread_ts")?;
    (Some(thread) != str_field(event, "ts")).then_some(thread)
}

pub fn bot_user_id(payload: &Value) -> Option<&str> {
    payload
        .pointer("/authorizations/0/user_id")
        .and_then(Value::as_str)
}

/// Pure admission of one `events_api` payload. `known_thread` says whether the bot already
/// has a session for the event's thread.
pub fn admit(
    config: &SlackTurnConfig,
    payload: &Value,
    bot_user: &str,
    known_thread: bool,
) -> Option<Admitted> {
    let event = payload.get("event")?;
    let kind = str_field(event, "type")?;
    match kind {
        "message" => {
            if !matches!(
                str_field(event, "subtype"),
                None | Some("file_share" | "thread_broadcast")
            ) {
                return None;
            }
        }
        "app_mention" => {}
        _ => return None,
    }
    if event.get("bot_id").is_some() {
        return None;
    }
    let user = str_field(event, "user")?;
    if user == bot_user || !config.allowed_users.iter().any(|id| id == user) {
        return None;
    }
    let channel = str_field(event, "channel")?;
    let ts = str_field(event, "ts")?;
    let text = str_field(event, "text").unwrap_or_default();
    let mention = format!("<@{bot_user}>");
    let mentioned = !bot_user.is_empty() && text.contains(&mention);
    let dm = is_dm(event, channel);
    let reply_thread = reply_thread_ts(event);
    let team = str_field(payload, "team_id")
        .or_else(|| str_field(event, "team"))
        .unwrap_or_default();
    let (key, thread) = if dm {
        (
            SessionKey::new(PLATFORM, None::<String>, channel, reply_thread, user),
            reply_thread.map(str::to_owned),
        )
    } else {
        if kind == "message" {
            // The same message also arrives as an app_mention; answer that one.
            if mentioned {
                return None;
            }
            let free = config.free_channels.iter().any(|id| id == channel);
            if !free && !known_thread {
                return None;
            }
        }
        let thread = reply_thread.unwrap_or(ts).to_owned();
        (
            SessionKey::new(PLATFORM, Some(team), channel, Some(thread.clone()), ""),
            Some(thread),
        )
    };
    let key = if bot_user.is_empty() {
        key
    } else {
        key.with_bot_id(bot_user)
    };
    let content = if bot_user.is_empty() {
        text.trim().to_owned()
    } else {
        text.replace(&mention, "").trim().to_owned()
    };
    if content.is_empty() {
        return None;
    }
    Some(Admitted {
        key,
        channel: channel.to_owned(),
        reply_thread: thread,
        message_ts: ts.to_owned(),
        user: user.to_owned(),
        text: content,
    })
}

/// Whether the bot already has a session for this channel thread.
pub async fn known_thread(pool: &SqlitePool, payload: &Value) -> bool {
    let Some(event) = payload.get("event") else {
        return false;
    };
    let Some(channel) = str_field(event, "channel") else {
        return false;
    };
    let Some(thread) = reply_thread_ts(event) else {
        // omo-gateway: a top-level message is known when the channel has a channel session.
        if is_dm(event, channel) {
            return false;
        }
        return sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sessions WHERE platform = 'slack' AND channel_id = ? \
             AND (thread_id IS NULL OR thread_id = '')",
        )
        .bind(channel)
        .fetch_one(pool)
        .await
        .unwrap_or(0)
            > 0;
    };
    // omo-gateway: a thread rooted by the bot (a cron or lane report card) has no thread
    // session yet, but a reply under it is addressed to the bot.
    if rooted_by_bot(payload, event) {
        return true;
    }
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sessions WHERE platform = 'slack' AND channel_id = ? AND thread_id = ?",
    )
    .bind(channel)
    .bind(thread)
    .fetch_one(pool)
    .await
    .unwrap_or(0)
        > 0
}

fn rooted_by_bot(payload: &Value, event: &Value) -> bool {
    let Some(bot) = bot_user_id(payload) else {
        return false;
    };
    reply_thread_ts(event).is_some() && str_field(event, "parent_user_id") == Some(bot)
}

fn seen_events() -> &'static Mutex<(HashSet<String>, VecDeque<String>)> {
    static SEEN: OnceLock<Mutex<(HashSet<String>, VecDeque<String>)>> = OnceLock::new();
    SEEN.get_or_init(Default::default)
}

/// True the first time a channel/ts pair is offered; Slack redelivers unacked events.
fn first_delivery(channel: &str, ts: &str) -> bool {
    let id = format!("{channel}/{ts}");
    let mut seen = seen_events()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if !seen.0.insert(id.clone()) {
        return false;
    }
    seen.1.push_back(id);
    while seen.1.len() > 512 {
        if let Some(old) = seen.1.pop_front() {
            seen.0.remove(&old);
        }
    }
    true
}

fn session_lock(key: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    locks.entry(key.to_owned()).or_default().clone()
}

async fn load_state(pool: &SqlitePool, key: &SessionKey) -> Option<SessionState> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT state_json FROM sessions WHERE session_key = ?")
            .bind(key.storage_key())
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    raw.and_then(|raw| serde_json::from_str(&raw).ok())
}

/// System prompt a new Slack thread starts with: the one the newest Slack session of the same
/// bot carries (omo-gateway resolved it from the bot persona).
async fn inherited_system_prompt(pool: &SqlitePool, key: &SessionKey) -> Option<String> {
    let suffix = key
        .bot_id
        .as_deref()
        .map(|bot| format!("%|{}:{bot}", bot.len()))
        .unwrap_or_else(|| "%".into());
    sqlx::query_scalar::<_, String>(
        "SELECT json_extract(state_json, '$.system_prompt') FROM sessions
         WHERE platform = 'slack' AND session_key LIKE ? AND json_extract(state_json, '$.system_prompt') IS NOT NULL
         ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(suffix)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

/// omo-gateway writes `updated_at` as RFC 3339 with microseconds (`...+00:00`). SQLite's
/// `CURRENT_TIMESTAMP` (`2026-10-23 05:58:20`) sorts before every omo row, so recency
/// queries over the shared table went wrong.
pub fn omo_timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
}

async fn save_state(pool: &SqlitePool, key: &SessionKey, state: &SessionState) -> Result<()> {
    let state_json = serde_json::to_string(state).unwrap_or_else(|_| "{}".into());
    sqlx::query(
        "INSERT INTO sessions (session_key, platform, guild_id, channel_id, thread_id, user_id, state_json, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(session_key) DO UPDATE SET state_json = excluded.state_json, updated_at = excluded.updated_at",
    )
    .bind(key.storage_key())
    .bind(&key.platform)
    .bind(&key.guild_id)
    .bind(&key.channel_id)
    .bind(&key.thread_id)
    .bind(&key.user_id)
    .bind(state_json)
    .bind(omo_timestamp())
    .execute(pool)
    .await?;
    Ok(())
}

/// omo-gateway's egress filters before rendering: report-claim markers, then reasoning tags.
pub fn outgoing_text(text: &str) -> String {
    let stripped = if super::claims::report_claims_enabled() {
        super::claims::strip_and_extract_claims(text).0
    } else {
        text.to_owned()
    };
    omo_client::filter_reasoning(&stripped)
}

async fn post(channel: &str, thread: Option<&str>, text: &str) {
    let text = outgoing_text(text);
    if text.is_empty() || omo_client::is_explicit_silence(&text) {
        return;
    }
    match super::slack::deliver_omo(channel, thread, &text).await {
        Ok(true) => {}
        Ok(false) => tracing::warn!("slack reply was not posted: no bot token"),
        Err(error) => tracing::warn!(%error, "slack reply was not posted"),
    }
}

async fn react(channel: &str, ts: &str, name: &str, add: bool) {
    let method = if add {
        "reactions.add"
    } else {
        "reactions.remove"
    };
    if let Err(error) = super::slack::call_method(
        method,
        json!({"channel": channel, "timestamp": ts, "name": name}),
    )
    .await
    {
        tracing::debug!(%error, method, "slack status reaction failed");
    }
}

/// Runs one admitted message through omo and posts the reply in its thread.
pub async fn handle(config: &SlackTurnConfig, pool: &SqlitePool, admitted: Admitted) {
    if !first_delivery(&admitted.channel, &admitted.message_ts) {
        return;
    }
    let lock = session_lock(&admitted.key.storage_key());
    let _turn = lock.lock().await;
    run_turn(
        config,
        pool,
        &admitted.key,
        &admitted.channel,
        admitted.reply_thread.as_deref(),
        &admitted.text,
        Some(&admitted.message_ts),
    )
    .await;
}

/// omo's system admission (`EventOrigin::System`): a lane event becomes a turn in `key`'s
/// session and the reply is posted where that session lives (the owner DM or the home channel,
/// top level). Returns false without running when the session already has a turn.
pub async fn system_turn(
    config: &'static SlackTurnConfig,
    pool: SqlitePool,
    key: SessionKey,
    text: String,
) -> bool {
    let lock = session_lock(&key.storage_key());
    let Ok(guard) = lock.clone().try_lock_owned() else {
        return false;
    };
    tokio::spawn(async move {
        let _turn = guard;
        let channel = key.channel_id.clone();
        let thread = key.thread_id.clone().filter(|thread| !thread.is_empty());
        run_turn(
            config,
            &pool,
            &key,
            &channel,
            thread.as_deref(),
            &text,
            None,
        )
        .await;
    });
    true
}

async fn run_turn(
    config: &SlackTurnConfig,
    pool: &SqlitePool,
    key: &SessionKey,
    channel_id: &str,
    reply_thread: Option<&str>,
    text: &str,
    message_ts: Option<&str>,
) {
    let storage_key = key.storage_key();
    if let Some(ts) = message_ts {
        react(channel_id, ts, "eyes", true).await;
    }
    let mut state = load_state(pool, key).await.unwrap_or_default();
    if state.system_prompt.is_none() {
        state.system_prompt = inherited_system_prompt(pool, key).await;
    }
    let mut omo = config.omo.clone();
    if let Some(path) = &config.token_path {
        omo.token = omo_client::token_from_file(path);
    }
    let request = TurnRequest {
        thread_id: state
            .metadata
            .get("omo_thread_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        system_prompt: state.system_prompt.clone(),
        model: state
            .active_model
            .clone()
            .or_else(|| config.default_model.clone()),
        toolsets: state.enabled_toolsets.clone(),
        text: text.to_owned(),
    };
    let (progress, mut interim) = tokio::sync::mpsc::unbounded_channel::<String>();
    let channel = channel_id.to_owned();
    let thread = reply_thread.map(str::to_owned);
    let relay = super::slack::spawn_carrying_test_api(async move {
        while let Some(text) = interim.recv().await {
            post(&channel, thread.as_deref(), &text).await;
        }
    });
    tracing::info!(session = %storage_key, resume = request.thread_id.is_some(), system = message_ts.is_none(), "slack turn started on omo app-server");
    let started = std::time::Instant::now();
    let result = omo_client::run_turn(&omo, &request, Some(progress)).await;
    let _ = relay.await;
    if let Some(ts) = message_ts {
        react(channel_id, ts, "eyes", false).await;
    }
    let failed = |error: String| async move {
        tracing::warn!(session = %storage_key, %error, "slack turn failed");
        post(
            channel_id,
            reply_thread,
            &format!("⚠️ omo turn failed: {error}"),
        )
        .await;
        if let Some(ts) = message_ts {
            react(channel_id, ts, "x", true).await;
        }
    };
    match result {
        Ok(outcome) => {
            state
                .metadata
                .insert("omo_thread_id".into(), json!(outcome.thread_id));
            if let Err(error) = save_state(pool, key, &state).await {
                tracing::warn!(%error, "slack session state was not saved");
            }
            match omo_client::final_reply(&outcome) {
                Ok(reply) => {
                    if let Some(text) = &reply {
                        post(channel_id, reply_thread, text).await;
                    }
                    if let Some(ts) = message_ts {
                        react(channel_id, ts, "white_check_mark", true).await;
                    }
                    tracing::info!(
                        session = %key.storage_key(),
                        resumed = outcome.resumed,
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        interim = outcome.messages_delivered,
                        tool_calls = outcome.tool_calls,
                        approvals_allowed = outcome.approvals_allowed,
                        approvals_denied = outcome.approvals_denied,
                        silent = reply.is_none(),
                        "slack turn replied"
                    );
                }
                Err(error) => failed(error.to_string()).await,
            }
        }
        Err(error) => failed(error.to_string()).await,
    }
}

/// omo-gateway's lane-event dispatcher on the Slack turn path: every 5s queued lane events
/// become a system turn in the owner DM (urgent) or the home channel (routine), and owner alerts
/// queued in `chat_outbox` are posted to their session's channel.
pub async fn serve_lane_events(config: &'static SlackTurnConfig, pool: SqlitePool, lead: String) {
    let judge = crate::judge::Judge::from_config(crate::judge::typesafe_from_env());
    let mut ticks = tokio::time::interval(std::time::Duration::from_secs(5));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        let turn_pool = pool.clone();
        if let Err(error) =
            crate::lanes::dispatch::dispatch_tick(&pool, &judge, chrono::Utc::now(), |key, text| {
                system_turn(config, turn_pool.clone(), key, text)
            })
            .await
        {
            tracing::warn!(%error, "lane event dispatch tick failed; events remain queued");
        }
        if let Err(error) = drain_outbox(&pool, &lead).await {
            tracing::warn!(%error, "slack outbox was not drained");
        }
    }
}

/// Posts queued Slack outbox rows from the last day. A row addressed to a name that is not a
/// session key (1022.8 queued owner alerts under `meta`) goes to the lead.
pub async fn drain_outbox(pool: &SqlitePool, lead: &str) -> Result<usize> {
    let since = (chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, session_key, text FROM chat_outbox
         WHERE delivered_at IS NULL AND platform = 'slack' AND created_at >= ? ORDER BY id",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;
    let mut sent = 0;
    for (id, session_key, text) in rows {
        let Some(key) = SessionKey::from_storage_key(&session_key)
            .ok()
            .or_else(|| SessionKey::from_storage_key(lead).ok())
            .filter(|key| key.platform == PLATFORM)
        else {
            continue;
        };
        let thread = key.thread_id.as_deref().filter(|thread| !thread.is_empty());
        match super::slack::deliver_omo(&key.channel_id, thread, &outgoing_text(&text)).await {
            Ok(true) => {
                super::reply::mark_delivered(pool, id).await?;
                sent += 1;
            }
            Ok(false) => return Ok(sent),
            Err(error) => {
                tracing::warn!(%error, outbox_id = id, "slack outbox row was not posted");
                return Ok(sent);
            }
        }
    }
    Ok(sent)
}

/// Entry from the Socket Mode frame handler. Returns false when this frame is not an event the
/// omo path takes (the caller keeps its usual handling).
pub async fn intercept(config: &'static SlackTurnConfig, pool: &SqlitePool, frame: &Value) -> bool {
    if frame.get("type").and_then(Value::as_str) != Some("events_api") {
        return false;
    }
    let Some(payload) = frame.get("payload") else {
        return false;
    };
    let bot = bot_user_id(payload)
        .map(str::to_owned)
        .or_else(|| crate::hub_env::var("SLACK_BOT_USER_ID").ok())
        .unwrap_or_default();
    let known = known_thread(pool, payload).await;
    let Some(admitted) = admit(config, payload, &bot, known) else {
        return false;
    };
    let pool = pool.clone();
    super::slack::spawn_carrying_test_api(async move { handle(config, &pool, admitted).await });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SlackTurnConfig {
        SlackTurnConfig {
            omo: OmoClientConfig::new("ws://127.0.0.1:9"),
            token_path: None,
            allowed_users: vec!["UOWNER".into()],
            free_channels: vec!["CHOME".into()],
            default_model: None,
        }
    }

    fn payload(event: Value) -> Value {
        json!({"team_id": "T1", "authorizations": [{"user_id": "UBOT"}], "event": event})
    }

    #[test]
    fn admission_mirrors_omo_gateway() {
        let c = config();
        let home = payload(
            json!({"type":"message","user":"UOWNER","channel":"CHOME","ts":"1.1","text":"hi"}),
        );
        let a = admit(&c, &home, "UBOT", false).unwrap();
        assert_eq!(a.reply_thread.as_deref(), Some("1.1"));
        assert_eq!(a.key.storage_key(), "5:slack|2:T1|5:CHOME|3:1.1|0:|4:UBOT");
        // other users, bots, other channels without mention are ignored
        assert!(admit(
            &c,
            &payload(
                json!({"type":"message","user":"UX","channel":"CHOME","ts":"1.2","text":"hi"})
            ),
            "UBOT",
            false
        )
        .is_none());
        assert!(admit(&c, &payload(json!({"type":"message","bot_id":"B1","user":"UOWNER","channel":"CHOME","ts":"1.3","text":"hi"})), "UBOT", false).is_none());
        let other = payload(
            json!({"type":"message","user":"UOWNER","channel":"COTHER","ts":"1.4","thread_ts":"1.0","text":"hi"}),
        );
        assert!(admit(&c, &other, "UBOT", false).is_none());
        assert_eq!(
            admit(&c, &other, "UBOT", true)
                .unwrap()
                .reply_thread
                .as_deref(),
            Some("1.0")
        );
        // a mention arrives twice: the message copy is dropped, the app_mention is answered
        let mention =
            json!({"user":"UOWNER","channel":"COTHER","ts":"1.5","text":"<@UBOT> status?"});
        let mut as_message = mention.clone();
        as_message["type"] = json!("message");
        let mut as_mention = mention;
        as_mention["type"] = json!("app_mention");
        assert!(admit(&c, &payload(as_message), "UBOT", true).is_none());
        let a = admit(&c, &payload(as_mention), "UBOT", false).unwrap();
        assert_eq!(a.text, "status?");
        // a DM is answered at top level
        let dm = admit(&c, &payload(json!({"type":"message","channel_type":"im","user":"UOWNER","channel":"D1","ts":"1.6","text":"yo"})), "UBOT", false).unwrap();
        assert_eq!(dm.reply_thread, None);
        assert_eq!(dm.key.user_id, "UOWNER");
        // edits and joins are not turns
        assert!(admit(&c, &payload(json!({"type":"message","subtype":"message_changed","user":"UOWNER","channel":"CHOME","ts":"1.7","text":"x"})), "UBOT", false).is_none());
    }

    #[test]
    fn replies_render_like_omo_gateway() {
        let text = format!("{}\n{}", "a".repeat(3000), "b".repeat(3000));
        let parts = super::super::mrkdwn::render(&text);
        assert_eq!(parts.len(), 2);
        assert!(parts
            .iter()
            .all(|p| p.len() <= super::super::mrkdwn::MESSAGE_LIMIT));
        assert_eq!(
            super::super::mrkdwn::render("**done** see [docs](https://x.y)"),
            ["*done* see <https://x.y|docs>"]
        );
        assert_eq!(
            outgoing_text("<think>plan</think>Ok. [[measure [\"uptime\"] expect~\"up\"]]"),
            "Ok."
        );
    }

    #[test]
    fn redelivered_events_run_once() {
        assert!(first_delivery("C9", "9.9"));
        assert!(!first_delivery("C9", "9.9"));
    }
}
