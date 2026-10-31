//! Slack message → mhc RPC turn → threaded Slack reply.
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
    pub allowed_users: Vec<String>,
    pub free_channels: Vec<String>,
    pub default_model: Option<String>,
    /// omo's channel scoping: a non-empty allowlist restricts channels (not DMs); ignored
    /// channels always reject.
    pub allowed_channels: Vec<String>,
    pub ignored_channels: Vec<String>,
    /// `SLACK_THREAD_REQUIRE_MENTION`: a reply in a known thread needs a mention.
    pub thread_require_mention: bool,
    /// `SLACK_AUTO_THREAD` (default on): a top-level channel message roots its own thread.
    pub auto_thread: bool,
    /// `SLACK_ALLOW_BOTS`: none (default), mentions, all.
    pub allow_bots: AllowBots,
    /// `SLACK_ALLOW_ALL_USERS`.
    pub allow_all_users: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AllowBots {
    #[default]
    None,
    Mentions,
    All,
}

impl AllowBots {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("all") => Self::All,
            Some("mentions" | "mention") => Self::Mentions,
            _ => Self::None,
        }
    }
}

fn parse_flag(raw: Option<String>, default: bool) -> bool {
    match raw.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("1" | "true" | "yes" | "on") => true,
        Some("0" | "false" | "no" | "off") => false,
        _ => default,
    }
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
        binary: &str,
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
        let mut omo = OmoClientConfig::new(binary).with_env_timeouts();
        omo.workspace = omo_client::slack_workspace();
        Self {
            omo,
            allowed_users,
            free_channels,
            default_model: var("OMON_DEFAULT_MODEL")
                .or_else(|| var("DEFAULT_MODEL"))
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty()),
            allowed_channels: split_list(var("SLACK_ALLOWED_CHANNELS")),
            ignored_channels: split_list(var("SLACK_IGNORED_CHANNELS")),
            thread_require_mention: parse_flag(var("SLACK_THREAD_REQUIRE_MENTION"), false),
            auto_thread: parse_flag(var("SLACK_AUTO_THREAD"), true),
            allow_bots: AllowBots::parse(var("SLACK_ALLOW_BOTS").as_deref()),
            allow_all_users: parse_flag(var("SLACK_ALLOW_ALL_USERS"), false),
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
    /// Files of the message and of what it forwards.
    pub files: Vec<super::slack_ingress::SlackFile>,
    /// The raw Slack event (for reply and channel context); `Null` for synthetic turns.
    pub event: Value,
    pub dm: bool,
    pub bot_user: String,
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
                None | Some("file_share" | "thread_broadcast" | "bot_message")
            ) {
                return None;
            }
        }
        "app_mention" => {}
        _ => return None,
    }
    let text = str_field(event, "text").unwrap_or_default();
    let mention = format!("<@{bot_user}>");
    let mentioned = !bot_user.is_empty() && text.contains(&mention);
    let bot_id = str_field(event, "bot_id");
    if bot_id.is_some() || str_field(event, "subtype") == Some("bot_message") {
        match config.allow_bots {
            AllowBots::None => return None,
            AllowBots::Mentions if !mentioned => return None,
            AllowBots::Mentions | AllowBots::All => {}
        }
    }
    let user = str_field(event, "user").or(bot_id)?;
    if user == bot_user
        || !(config.allow_all_users || config.allowed_users.iter().any(|id| id == user))
    {
        return None;
    }
    let channel = str_field(event, "channel")?;
    let ts = str_field(event, "ts")?;
    let dm = is_dm(event, channel);
    if config.ignored_channels.iter().any(|id| id == channel)
        || !(dm
            || config.allowed_channels.is_empty()
            || config.allowed_channels.iter().any(|id| id == channel))
    {
        return None;
    }
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
            let implicit_thread = known_thread && !config.thread_require_mention;
            if !free && !implicit_thread {
                return None;
            }
        }
        let thread = reply_thread
            .or_else(|| config.auto_thread.then_some(ts))
            .map(str::to_owned);
        (
            SessionKey::new(PLATFORM, Some(team), channel, thread.clone(), ""),
            // Without auto-threading omo still answered a top-level message in a thread.
            Some(thread.unwrap_or_else(|| ts.to_owned())),
        )
    };
    let key = if bot_user.is_empty() {
        key
    } else {
        key.with_bot_id(bot_user)
    };
    let stripped = if bot_user.is_empty() {
        text.trim().to_owned()
    } else {
        text.replace(&mention, "").trim().to_owned()
    };
    let (content, files) = super::slack_ingress::content_and_files(event, &stripped);
    if content.is_empty() && files.is_empty() {
        return None;
    }
    Some(Admitted {
        key,
        channel: channel.to_owned(),
        reply_thread: thread,
        message_ts: ts.to_owned(),
        user: user.to_owned(),
        text: content,
        files,
        event: event.clone(),
        dm,
        bot_user: bot_user.to_owned(),
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

/// Persisted twin of [`first_delivery`]: the in-process set is empty after a restart, so a
/// redelivered or re-swept event `ts` that was already answered must not start a second turn.
async fn first_delivery_persisted(pool: &SqlitePool, channel: &str, ts: &str) -> bool {
    let id = format!("{channel}/{ts}");
    let now = chrono::Utc::now();
    let recorded = async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS slack_inbound_seen (id TEXT PRIMARY KEY NOT NULL, seen_at TEXT NOT NULL)",
        )
        .execute(pool)
        .await?;
        sqlx::query("DELETE FROM slack_inbound_seen WHERE seen_at < ?")
            .bind((now - chrono::Duration::days(7)).to_rfc3339())
            .execute(pool)
            .await?;
        let inserted =
            sqlx::query("INSERT OR IGNORE INTO slack_inbound_seen(id, seen_at) VALUES(?, ?)")
                .bind(&id)
                .bind(now.to_rfc3339())
                .execute(pool)
                .await?;
        Ok::<_, sqlx::Error>(inserted.rows_affected() == 1)
    }
    .await;
    recorded.unwrap_or_else(|error| {
        tracing::warn!(%error, "slack inbound dedupe was not recorded; answering anyway");
        true
    })
}

pub(crate) fn session_lock(key: &str) -> Arc<tokio::sync::Mutex<()>> {
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
    strip_handoff_block(&omo_client::filter_reasoning(&stripped))
}

/// Binding reply rules every Slack session turn carries: in the thread's developer
/// instructions when the thread starts, and once on the next turn of a thread that started
/// before they existed. `TURN_RULES_MARKER` is the machine-checkable sentinel.
pub const TURN_RULES_MARKER: &str = "[ferryx-turn-rules v1]";
const TURN_RULES: &str = "[ferryx-turn-rules v1] Binding rules for every reply in this conversation:
1. Approval: only an explicit approval word or sentence from the owner (e.g. '승인', '확정해', '그렇게 진행해') approves something. The owner quoting your own caveat or plan, asking a question, or answering 'ㅇㅋ'/'ㅇㅇ' right after you asked a question is NOT approval of a commitment to third parties: ask once, plainly, instead of recording it as confirmed. Never promise anything to third parties (rewards, deadlines, deliverables, public notices) without that explicit approval; a proposal stays labelled a proposal.
2. Done means verified: report an action as done only after you checked its outcome (the delivery is visible, the file exists, the page shows it). Until then say what you sent or started and that verification is pending; do not use done/complete/접수 완료 wording.
3. Slack format: reply to the owner in plain, natural Korean prose. Never write handoff labels ('Ask:', 'wanted:', 'For you:', 'Now:', 'Next:') or routing preambles ('I read this as ...').";

/// The developer instructions a session's thread starts with: its persona prompt plus the
/// binding reply rules.
fn with_turn_rules(system_prompt: Option<&str>) -> String {
    match system_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        Some(prompt) if prompt.contains(TURN_RULES_MARKER) => prompt.to_owned(),
        Some(prompt) => format!("{prompt}\n\n{TURN_RULES}"),
        None => TURN_RULES.to_owned(),
    }
}

/// Drops a leaked agent handoff block (`Ask: ... For you: ... Now: ... Next: ...`) at the start
/// of a reply, keeping its `For you` report and everything after the block.
fn strip_handoff_block(text: &str) -> String {
    let trimmed = text.trim_start();
    if !trimmed.trim_start_matches('*').starts_with("Ask:") {
        return text.to_owned();
    }
    let (block, rest) = trimmed.split_once("\n\n").unwrap_or((trimmed, ""));
    let Some((_, report)) = block.split_once("For you:") else {
        return text.to_owned();
    };
    let report = report
        .split_once("Now:")
        .map_or(report, |(report, _)| report)
        .trim_matches(|c: char| c == '*' || c == '—' || c.is_whitespace());
    [report, rest.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) async fn post(channel: &str, thread: Option<&str>, text: &str) {
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

pub(crate) async fn react(channel: &str, ts: &str, name: &str, add: bool) {
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

/// One omo turn for a session, from Slack or the dashboard.
#[derive(Debug, Clone)]
pub struct SessionTurn {
    pub key: SessionKey,
    /// Slack channel the replies go to; `None` for a dashboard-only (web) session.
    pub reply_channel: Option<String>,
    pub reply_thread: Option<String>,
    /// Slack message that gets the 👀/✅/❌ status reactions.
    pub status_ts: Option<String>,
    pub text: String,
    /// A turn the hub started itself (a lane event outside the owner DM). It runs on the
    /// background model whatever the session's model is.
    pub background: bool,
}

/// Where a session's assistant output goes: its Slack thread, or (for a web session) only the
/// transcript the dashboard streams.
async fn emit(pool: &SqlitePool, turn: &SessionTurn, text: &str) {
    match &turn.reply_channel {
        Some(channel) => post(channel, turn.reply_thread.as_deref(), text).await,
        None => {
            if let Err(error) =
                super::transcript::record(pool, &turn.key.storage_key(), "assistant", text).await
            {
                tracing::warn!(%error, "dashboard reply was not stored");
            }
        }
    }
}

async fn status(turn: &SessionTurn, name: &str, add: bool) {
    if let (Some(channel), Some(ts)) = (&turn.reply_channel, &turn.status_ts) {
        react(channel, ts, name, add).await;
    }
}

/// omo's status message for the owner's long turns: posted and edited on its interval while
/// the turn runs, closed by [`TurnStatus::finish`]. Shared by the omo and supervisor turns.
pub(crate) struct TurnStatus {
    driver: Arc<tokio::sync::Mutex<Option<super::turn_status::StatusMessage>>>,
    ticker: tokio::task::JoinHandle<()>,
}

impl TurnStatus {
    /// `None` unless the turn replies in Slack, status messages are on and the session is the
    /// owner's lead (owner DM or home channel).
    pub(crate) fn start(
        config: &SlackTurnConfig,
        key: &SessionKey,
        channel: Option<&str>,
        thread: Option<&str>,
        text: &str,
    ) -> Option<Self> {
        let channel = channel?;
        if !super::turn_status::enabled()
            || !config
                .allowed_users
                .first()
                .is_some_and(|owner| super::turn_status::is_owner_lead(key, owner))
        {
            return None;
        }
        let driver = Arc::new(tokio::sync::Mutex::new(Some(
            super::turn_status::StatusMessage::new(
                &key.storage_key(),
                channel,
                thread,
                text,
                chrono::Utc::now(),
                super::turn_status::interval(),
            ),
        )));
        let ticking = driver.clone();
        let ticker = super::slack::spawn_carrying_test_api(async move {
            loop {
                let due = match ticking.lock().await.as_ref() {
                    Some(message) => message.next_due(),
                    None => return,
                };
                let wait = (due - chrono::Utc::now()).to_std().unwrap_or_default();
                tokio::time::sleep(wait).await;
                if let Some(message) = ticking.lock().await.as_mut() {
                    message.tick(chrono::Utc::now()).await;
                }
            }
        });
        Some(Self { driver, ticker })
    }

    /// Stops the ticker and closes the message with the turn's outcome.
    pub(crate) async fn finish(self, outcome: std::result::Result<(), String>) {
        self.ticker.abort();
        let _ = self.ticker.await;
        if let Some(message) = self.driver.lock().await.take() {
            message.finish(chrono::Utc::now(), outcome).await;
        }
    }
}

/// Runs one message through the session's omo thread, as omo-gateway's multiplexer did: the
/// user message and the final reply land in the session transcript, the reply goes to the
/// session's Slack thread (or the dashboard), and the engine thread id is kept for resume.
/// Returns the final reply.
pub async fn run_session_turn(
    config: &SlackTurnConfig,
    pool: &SqlitePool,
    turn: SessionTurn,
) -> Option<String> {
    let storage_key = turn.key.storage_key();
    let lock = session_lock(&storage_key);
    let _turn = lock.lock().await;
    if let Err(error) = super::transcript::record(pool, &storage_key, "user", &turn.text).await {
        tracing::warn!(%error, "session message was not stored");
    }
    status(&turn, "eyes", true).await;
    let mut state = load_state(pool, &turn.key).await.unwrap_or_default();
    if state.system_prompt.is_none() {
        state.system_prompt = inherited_system_prompt(pool, &turn.key).await;
    }
    let omo = config.omo.clone();
    let mut request = TurnRequest {
        thread_id: state
            .metadata
            .get("omo_thread_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        system_prompt: Some(with_turn_rules(state.system_prompt.as_deref())),
        model: if turn.background {
            Some(crate::model_policy::background_model())
        } else {
            state
                .active_model
                .clone()
                .or_else(|| config.default_model.clone())
        },
        toolsets: state.enabled_toolsets.clone(),
        text: turn.text.clone(),
        pin_model: turn.background,
    };
    // omo: cron deliveries mirrored into this session ride on the next turn as context,
    // consumed only when that turn succeeds.
    let briefings = crate::mirror::peek_pending_cron_briefings(pool, &storage_key)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "cron briefings were not loaded");
            None
        });
    if let Some(briefings) = &briefings {
        request.text = format!("{}\n{}", briefings.text, request.text);
    }
    // A thread started before the rules existed never saw them in its developer instructions.
    let rules_known =
        state.metadata.get("turn_rules").and_then(Value::as_str) == Some(TURN_RULES_MARKER);
    if request.thread_id.is_some() && !rules_known {
        request.text = format!("{TURN_RULES}\n\n{}", request.text);
    }
    let (progress, mut interim) = tokio::sync::mpsc::unbounded_channel::<String>();
    let relay_pool = pool.clone();
    let relay_turn = turn.clone();
    // omo's report claims: `[[measure ...]]` markers never reach the user; a Slack reply's
    // claims are re-measured and corrected in its thread.
    let claim_target = turn
        .reply_channel
        .clone()
        .map(|channel| super::report_claims::Target {
            session_key: storage_key.clone(),
            channel,
            thread: turn.reply_thread.clone(),
            cwd: state
                .metadata
                .get("cwd")
                .and_then(Value::as_str)
                .map(std::path::PathBuf::from)
                .or_else(|| crate::hub_env::var_os("HOME").map(std::path::PathBuf::from))
                .unwrap_or_default(),
        });
    let claim_delay = super::report_claims::delay();
    let relay_target = claim_target.clone();
    let relay = super::slack::spawn_carrying_test_api(async move {
        while let Some(text) = interim.recv().await {
            let (text, claims) = if super::report_claims::enabled() {
                super::report_claims::strip_interim(&text)
            } else {
                (text, Vec::new())
            };
            if !text.trim().is_empty() {
                emit(&relay_pool, &relay_turn, &text).await;
            }
            if let Some(target) = &relay_target {
                super::report_claims::record_and_schedule(&relay_pool, target, claims, claim_delay)
                    .await;
            }
        }
    });
    tracing::info!(session = %storage_key, resume = request.thread_id.is_some(), "session turn started on mhc RPC");
    let started = std::time::Instant::now();
    // omo's status message for the owner's long turns.
    let status_message = TurnStatus::start(
        config,
        &turn.key,
        turn.reply_channel.as_deref(),
        turn.reply_thread.as_deref(),
        &turn.text,
    );
    let channel = turn.reply_channel.clone().unwrap_or_default();
    let registration = super::turns::begin(&storage_key, &channel);
    // Boxed: the RPC turn plus the approval wrapper is a large future, and an unboxed
    // copy on each await level overflowed a 2 MiB worker stack in debug builds.
    let run = Box::pin(async {
        // Owned here so the relay ends when the turn's last sender drops.
        let progress = progress;
        let mut result =
            omo_client::run_turn_recovering(&omo, &request, Some(progress.clone())).await;
        let Some(primary) = request.model.clone() else {
            return result;
        };
        // omo-gateway retries a quota/overload failure on the fallback models, in order.
        for fallback in omo_client::fallback_models(&primary) {
            let Some(retry_kind) = omo_client::fallback_retry(&result) else {
                break;
            };
            tracing::warn!(session = %storage_key, model = %fallback, error = ?result.as_ref().err().map(ToString::to_string), "retrying turn failure with fallback model");
            // A resend runs the request again where it started; a continue resumes the session
            // the failed run worked in.
            let thread_id = match (&result, retry_kind) {
                (Ok(outcome), _) => Some(outcome.thread_id.clone()),
                (Err(_), omo_client::FallbackRetry::Resend) => request.thread_id.clone(),
                (Err(_), omo_client::FallbackRetry::Continue) => super::turns::ids(&storage_key)
                    .map(|(thread, _)| thread)
                    .or_else(|| request.thread_id.clone()),
            };
            let retry = omo_client::TurnRequest {
                thread_id,
                model: Some(fallback),
                text: match retry_kind {
                    omo_client::FallbackRetry::Resend => request.text.clone(),
                    omo_client::FallbackRetry::Continue => omo_client::CONTINUE_PROMPT.to_owned(),
                },
                ..request.clone()
            };
            result = omo_client::run_turn(&omo, &retry, Some(progress.clone())).await;
        }
        result
    });
    let result = match &turn.reply_channel {
        // omo's approval cards: a request full access would not allow is asked in the thread.
        Some(channel) => {
            let approver = std::sync::Arc::new(super::turn_approval::SlackApprover::new(
                pool.clone(),
                storage_key.clone(),
                channel.clone(),
                turn.reply_thread.clone(),
            ));
            super::turns::scope(
                storage_key.clone(),
                omo_client::with_approver(approver, run),
            )
            .await
        }
        None => super::turns::scope(storage_key.clone(), run).await,
    };
    drop(registration);
    let _ = relay.await;
    if let Some(status_message) = status_message {
        let outcome = match &result {
            Ok(outcome) => omo_client::final_reply(outcome)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            Err(error) if error.to_string().contains(omo_client::STOPPED) => {
                Err("stopped by the owner".to_owned())
            }
            Err(error) => Err(error.to_string()),
        };
        status_message.finish(outcome).await;
    }
    status(&turn, "eyes", false).await;
    if matches!(&result, Err(error) if error.to_string().contains(omo_client::STOPPED)) {
        tracing::info!(session = %storage_key, "session turn stopped by the owner");
        return None;
    }
    let failure = match result {
        Ok(outcome) => {
            if let Some(briefings) = &briefings {
                if let Err(error) =
                    crate::mirror::mark_cron_briefings_injected(pool, &briefings.message_ids).await
                {
                    tracing::warn!(%error, "cron briefings were not marked injected");
                }
            }
            state
                .metadata
                .insert("omo_thread_id".into(), json!(outcome.thread_id));
            state
                .metadata
                .insert("turn_rules".into(), json!(TURN_RULES_MARKER));
            if let Err(error) = save_state(pool, &turn.key, &state).await {
                tracing::warn!(%error, "session state was not saved");
            }
            match omo_client::final_reply(&outcome) {
                Ok(reply) => {
                    let (reply, claims) = match reply {
                        Some(text) => {
                            let (clean, claims) = super::report_claims::strip_reply(&text);
                            (Some(clean).filter(|clean| !clean.trim().is_empty()), claims)
                        }
                        None => (None, Vec::new()),
                    };
                    if let Some(text) = &reply {
                        emit(pool, &turn, text).await;
                        if turn.reply_channel.is_some() {
                            if let Err(error) =
                                super::transcript::record(pool, &storage_key, "assistant", text)
                                    .await
                            {
                                tracing::warn!(%error, "session reply was not stored");
                            }
                        }
                    }
                    if let Some(target) = &claim_target {
                        super::report_claims::record_and_schedule(
                            pool,
                            target,
                            claims,
                            claim_delay,
                        )
                        .await;
                    }
                    status(&turn, "white_check_mark", true).await;
                    tracing::info!(
                        session = %storage_key,
                        resumed = outcome.resumed,
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        interim = outcome.messages_delivered,
                        tool_calls = outcome.tool_calls,
                        approvals_allowed = outcome.approvals_allowed,
                        approvals_denied = outcome.approvals_denied,
                        silent = reply.is_none(),
                        "session turn replied"
                    );
                    return reply;
                }
                Err(error) => error.to_string(),
            }
        }
        Err(error) => error.to_string(),
    };
    tracing::warn!(session = %storage_key, error = %failure, "session turn failed");
    emit(pool, &turn, &omo_client::owner_failure_text(&failure)).await;
    status(&turn, "x", true).await;
    None
}

/// omo's ingress enrichment and prompt rendering for a real Slack message (synthetic turns,
/// with a `Null` event, go through as typed).
pub async fn prepare_prompt(pool: &SqlitePool, admitted: &Admitted) -> String {
    prepare_prompt_in(pool, admitted, &super::slack_ingress::attachment_dir()).await
}

/// [`prepare_prompt`] with attachments saved under `dir`.
pub async fn prepare_prompt_in(
    pool: &SqlitePool,
    admitted: &Admitted,
    dir: &std::path::Path,
) -> String {
    if admitted.event.is_null() {
        return admitted.text.clone();
    }
    let mut content = admitted.text.clone();
    let mut files = admitted.files.clone();
    let root_known = load_state(pool, &admitted.key).await.is_some();
    super::slack_ingress::enrich(
        &super::slack_ingress::Enrich {
            event: &admitted.event,
            channel: &admitted.channel,
            ts: &admitted.message_ts,
            dm: admitted.dm,
            bot_user: &admitted.bot_user,
            root_known,
        },
        &mut content,
        &mut files,
    )
    .await;
    super::slack_ingress::hydrate(
        dir,
        &admitted.channel,
        &admitted.message_ts,
        &mut content,
        &mut files,
    )
    .await;
    super::slack_ingress::render_prompt(
        &content,
        &files,
        super::slack_ingress::ts_to_datetime(&admitted.message_ts),
    )
}

/// Runs one admitted message through omo and posts the reply in its thread. Returns false when
/// the message was already delivered (live and backfill both see it). Afterwards the Slack
/// cursor moves past it, as omo's `advance_cursor` did, so a restart's backfill skips it.
pub async fn handle(config: &SlackTurnConfig, pool: &SqlitePool, admitted: Admitted) -> bool {
    if !first_delivery(&admitted.channel, &admitted.message_ts) {
        return false;
    }
    if !first_delivery_persisted(pool, &admitted.channel, &admitted.message_ts).await {
        tracing::info!(channel = %admitted.channel, ts = %admitted.message_ts, "slack message already answered before a restart; not replayed");
        return false;
    }
    let text = Box::pin(prepare_prompt(pool, &admitted)).await;
    let cursor = (
        admitted.bot_user.clone(),
        admitted.channel.clone(),
        admitted.key.thread_id.clone().unwrap_or_default(),
        admitted.message_ts.clone(),
    );
    // The owner's DM and the channels/threads the owner bound to the supervisor are the
    // supervisor's conversation; every other message keeps the omo app-server turn.
    let supervised = match super::slack_supervisor::conversation() {
        Some(conversation) => Box::pin(super::slack_supervisor::try_handle(
            config,
            pool,
            &admitted,
            &text,
            conversation.as_ref(),
        ))
        .await
        .is_some(),
        // After a cutover the owner's messages belong to the supervisor; with no engine they
        // get an explicit unavailable reply, never a silent legacy turn.
        None => super::slack_supervisor::refuse_without_engine(pool, &admitted).await,
    };
    if !supervised {
        Box::pin(run_session_turn(
            config,
            pool,
            SessionTurn {
                key: admitted.key,
                reply_channel: Some(admitted.channel),
                reply_thread: admitted.reply_thread,
                status_ts: Some(admitted.message_ts),
                text,
                background: false,
            },
        ))
        .await;
    }
    let (bot, channel, thread, ts) = cursor;
    if !bot.is_empty() {
        // A top-level message that roots its own thread is read back from channel history.
        let roots_own = !thread.is_empty() && thread == ts;
        for thread in std::iter::once(thread.as_str()).chain(roots_own.then_some("")) {
            if let Err(error) =
                super::slack_backfill::write_cursor(pool, &bot, &channel, thread, &ts).await
            {
                tracing::debug!(%error, channel, "failed to advance Slack cursor");
            }
        }
    }
    true
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
    // Only the owner DM (the supervisor's own conversation) keeps the session's model.
    let background = !config
        .allowed_users
        .first()
        .is_some_and(|owner| is_owner_dm(&key, owner));
    tokio::spawn(async move {
        // The admission lock is held until this task starts, then released before
        // `run_session_turn` takes the same lock for the whole turn.
        drop(guard);
        let channel = key.channel_id.clone();
        let thread = key.thread_id.clone().filter(|thread| !thread.is_empty());
        let _ = run_session_turn(
            config,
            &pool,
            SessionTurn {
                reply_channel: Some(channel),
                reply_thread: thread,
                status_ts: None,
                key,
                text,
                background,
            },
        )
        .await;
    });
    true
}

/// The owner's direct-message session, the one conversation that is not background work.
fn is_owner_dm(key: &SessionKey, owner: &str) -> bool {
    key.guild_id.is_none() && key.thread_id.is_none() && key.user_id == owner
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

/// The dashboard's direct chat (omo `POST /api/sessions/{id}/chat` and the websocket
/// `message`): the owner's message runs as a turn of that session. A Slack session answers in
/// its Slack thread; a web session answers into the transcript the dashboard streams.
pub fn submit_dashboard_message(pool: &SqlitePool, key: SessionKey, text: String) -> bool {
    let Some(config) = installed() else {
        return false;
    };
    let pool = pool.clone();
    super::slack::spawn_carrying_test_api(async move {
        run_dashboard_turn(config, &pool, key, text).await;
    });
    true
}

pub async fn run_dashboard_turn(
    config: &SlackTurnConfig,
    pool: &SqlitePool,
    key: SessionKey,
    text: String,
) -> Option<String> {
    let slack = key.platform == PLATFORM;
    let turn = SessionTurn {
        reply_channel: slack.then(|| key.channel_id.clone()),
        reply_thread: if slack { key.thread_id.clone() } else { None },
        status_ts: None,
        key,
        text,
        background: false,
    };
    Box::pin(run_session_turn(config, pool, turn)).await
}

/// Undelivered Slack outbox rows from the last day that are older than
/// `reply::OUTBOX_SETTLE`. A younger row may still be in its writer's own first post
/// (`deliver_stored` right after the insert); taking it here posted it twice (2026-10-28: the
/// same cron owner notice appeared twice in the home channel within one second).
async fn drainable_outbox(
    pool: &SqlitePool,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<(i64, String, String)>> {
    let rows = sqlx::query_as(
        "SELECT id, session_key, text FROM chat_outbox
         WHERE delivered_at IS NULL AND platform = 'slack'
           AND julianday(created_at) >= julianday(?) AND julianday(created_at) <= julianday(?)
         ORDER BY id",
    )
    .bind((now - chrono::Duration::hours(24)).to_rfc3339())
    .bind((now - super::reply::OUTBOX_SETTLE).to_rfc3339())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Posts queued Slack outbox rows from the last day. A row addressed to a name that is not a
/// session key (1022.8 queued owner alerts under `meta`) goes to the lead.
pub async fn drain_outbox(pool: &SqlitePool, lead: &str) -> Result<usize> {
    let rows = drainable_outbox(pool, chrono::Utc::now()).await?;
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
    super::slack::spawn_carrying_test_api(async move {
        handle(config, &pool, admitted).await;
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SlackTurnConfig {
        SlackTurnConfig {
            omo: OmoClientConfig::new("mhc"),
            allowed_users: vec!["UOWNER".into()],
            free_channels: vec!["CHOME".into()],
            default_model: None,
            allowed_channels: Vec::new(),
            ignored_channels: Vec::new(),
            thread_require_mention: false,
            auto_thread: true,
            allow_bots: AllowBots::None,
            allow_all_users: false,
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

    #[tokio::test]
    async fn the_outbox_drain_leaves_a_row_its_writer_is_still_posting() {
        let pool = crate::storage::memory().await.unwrap();
        let now = chrono::Utc::now();
        for (text, at) in [
            ("fresh", now),
            ("settled", now - chrono::Duration::minutes(3)),
            ("expired", now - chrono::Duration::hours(25)),
        ] {
            sqlx::query("INSERT INTO chat_outbox(session_key, platform, text, created_at) VALUES('slack:C1', 'slack', ?, ?)")
                .bind(text)
                .bind(at.to_rfc3339())
                .execute(&pool)
                .await
                .unwrap();
        }
        let rows = drainable_outbox(&pool, now).await.unwrap();
        let texts: Vec<&str> = rows.iter().map(|row| row.2.as_str()).collect();
        assert_eq!(texts, vec!["settled"]);
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

    #[test]
    fn only_the_owner_dm_is_not_background_work() {
        let dm = SessionKey::new("slack", None::<String>, "D1", None::<String>, "UOWNER");
        assert!(is_owner_dm(&dm, "UOWNER"));
        let dm_thread = SessionKey::new("slack", None::<String>, "D1", Some("1.1"), "UOWNER");
        assert!(!is_owner_dm(&dm_thread, "UOWNER"));
        let home = SessionKey::new("slack", Some("T1"), "CHOME", None::<String>, "");
        assert!(!is_owner_dm(&home, "UOWNER"));
        let stranger = SessionKey::new("slack", None::<String>, "D2", None::<String>, "U2");
        assert!(!is_owner_dm(&stranger, "UOWNER"));
    }

    #[test]
    fn every_thread_starts_with_the_turn_rules_and_a_leaked_handoff_is_dropped() {
        assert!(with_turn_rules(None).starts_with(TURN_RULES_MARKER));
        let persona = with_turn_rules(Some("You are wawabot."));
        assert!(persona.starts_with("You are wawabot."));
        assert_eq!(persona.matches(TURN_RULES_MARKER).count(), 1);
        assert_eq!(with_turn_rules(Some(&persona)), persona);
        assert_eq!(
            outgoing_text("Ask: x - wanted: y. For you: 사진 3장 보냈어요. Now: a. Next: b.\n\n확인은 아직이에요."),
            "사진 3장 보냈어요.\n\n확인은 아직이에요."
        );
        assert_eq!(outgoing_text("Ask me anything."), "Ask me anything.");
        assert_eq!(
            outgoing_text(
                "**Ask:** 저장 — **For you:** 15장 저장했어요. **Now:** 완료. **Next:** 없음."
            ),
            "15장 저장했어요."
        );
    }

    #[tokio::test]
    async fn a_message_answered_before_a_restart_is_not_replayed() {
        let pool = crate::storage::memory().await.unwrap();
        assert!(first_delivery_persisted(&pool, "C1", "1.000001").await);
        assert!(!first_delivery_persisted(&pool, "C1", "1.000001").await);
        assert!(first_delivery_persisted(&pool, "C1", "1.000002").await);
        assert!(first_delivery_persisted(&pool, "C2", "1.000001").await);
    }
}
