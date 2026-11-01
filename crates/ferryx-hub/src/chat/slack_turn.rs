//! Slack message → mhc RPC turn → threaded Slack reply.
//!
//! Admission mirrors omo-gateway's `map_event`: only allowed users (`SLACK_ALLOWED_USERS` and
//! the configured hub owner; with neither, nobody); DMs always; in channels, an `app_mention`, any message in a
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
1. Approval: only an explicit approval word or sentence from the owner (e.g. 'approved', 'confirm it', 'go ahead with that'; '승인', '확정해' in Korean) approves something. The owner quoting your own caveat or plan, asking a question, or answering a bare 'ok'/'ㅇㅋ' right after you asked a question is NOT approval of a commitment to third parties: ask once, plainly, instead of recording it as confirmed. Never promise anything to third parties (rewards, deadlines, deliverables, public notices) without that explicit approval; a proposal stays labelled a proposal.
2. Done means verified: report an action as done only after you checked its outcome (the delivery is visible, the file exists, the page shows it). Until then say what you sent or started and that verification is pending; do not use done/complete/접수 완료 wording.
3. Slack format: reply to the owner in plain, natural prose in the owner's language (named on the owner-language line below). Never write handoff labels ('Ask:', 'wanted:', 'For you:', 'Now:', 'Next:') or routing preambles ('I read this as ...').";

/// The binding reply rules with the owner's language named.
fn turn_rules() -> String {
    format!(
        "{TURN_RULES}\n4. Owner language: {}. Every message the owner sees, including short progress notes, is in this language.",
        crate::locale::current().language_name()
    )
}

/// The developer instructions a session's thread starts with: its persona prompt plus the
/// binding reply rules.
fn with_turn_rules(system_prompt: Option<&str>) -> String {
    match system_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        Some(prompt) if prompt.contains(TURN_RULES_MARKER) => prompt.to_owned(),
        Some(prompt) => format!("{prompt}\n\n{}", turn_rules()),
        None => turn_rules(),
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

/// What the commitment ledger did with a reply before it is gated.
#[derive(Debug, Clone, Copy, Default)]
pub struct Commitments {
    /// The reply delivered or abandoned an open promise.
    pub fulfils: bool,
    /// The reply opened a promise the ledger now follows up.
    pub opens: bool,
}

impl From<&crate::commitments::Tracked> for Commitments {
    fn from(tracked: &crate::commitments::Tracked) -> Self {
        Self {
            fulfils: !tracked.fulfilled.is_empty() || !tracked.abandoned.is_empty(),
            opens: tracked.recorded.is_some(),
        }
    }
}

/// Binds the ledger's verdict to the report the gate will judge: a delivered promise is a
/// result, and a reply that opens a promise is marked so that promise-only text stays internal.
pub fn apply_commitments(report: &mut crate::report_gate::Report, commitments: Commitments) {
    // A lane report keeps its event identity (inbox item, open-items state): fulfilling a promise
    // must not hide that the same event was already reported.
    if commitments.fulfils && report.source == "slack_turn" {
        report.source = "slack_turn".into();
        report.topic_key = format!("commitment-result:{}", report.id);
        report.salience = crate::report_gate::Salience::Result;
        // The commitment judge already found the promised result in this text.
        report.turn = None;
    } else if let Some(context) = report.turn.as_mut() {
        context.opens_commitment = commitments.opens;
    }
}

/// Returns `true` when the reply was withheld because it only promised work the commitment
/// ledger now owns; the owner's message then shows that the work is pending, not done.
async fn emit_result(pool: &SqlitePool, turn: &SessionTurn, text: &str, commitments: Commitments, final_answer: bool) -> bool {
    match &turn.reply_channel {
        Some(channel) => {
            let text = outgoing_text(text);
            if text.is_empty() || omo_client::is_explicit_silence(&text) {
                return false;
            }
            let mut report = match delivery_report(pool, turn, &text, final_answer).await {
                Ok(report) => report,
                Err(error) => {
                    tracing::warn!(%error, "turn report source could not be resolved");
                    return false;
                }
            };
            apply_commitments(&mut report, commitments);
            let session = SessionKey::new(
                "slack",
                None::<String>,
                channel,
                turn.reply_thread.clone(),
                "owner",
            );
            let judge = crate::judge::Judge::from_config(crate::judge::typesafe_from_env())
                .with_pool(pool.clone());
            if let Err(error) = crate::report_gate::consolidate_turn_report(
                pool, &judge, &session.storage_key(), &mut report,
            ).await {
                tracing::warn!(%error, "turn report comparison unavailable; preserving new report");
            }
            let deferred = report.turn.as_ref().is_some_and(|context| context.opens_commitment)
                && report.salience == crate::report_gate::Salience::Internal;
            match crate::owner_alert::notify_report(pool, &session.storage_key(), &report).await {
                Ok(posted) => deferred && !posted,
                Err(error) => {
                    tracing::warn!(%error, "turn report could not be delivered");
                    false
                }
            }
        }
        None => {
            if let Err(error) =
                super::transcript::record(pool, &turn.key.storage_key(), "assistant", text).await
            {
                tracing::warn!(%error, "dashboard reply was not stored");
            }
            false
        }
    }
}

pub async fn delivery_report(pool: &SqlitePool, turn: &SessionTurn, text: &str, final_answer: bool) -> crate::Result<crate::report_gate::Report> {
    let mut report = turn_report(turn, text);
    // Routing identity: only the final message answers the owner's message. Its importance is
    // judged later from the content, with the phase and the request as inputs.
    report.reply_to_owner &= final_answer;
    report.turn = Some(crate::report_gate::TurnContext {
        final_answer, request: turn.text.clone(), salience_judged: false, delivered_in_turn: false,
        opens_commitment: false,
    });
    if report.source == "inbox" {
        if let Some(item) = report.topic_key.strip_prefix("inbox:") {
            let source: Option<(String, Option<String>)> = sqlx::query_as(
                "SELECT source,author FROM inbox_items WHERE id=?")
                .bind(item).fetch_optional(pool).await?;
            if let Some((source, author)) = source {
                report.source = source;
                report.sender = author;
            } else {
                return Err(crate::HubError::message("urgent inbox report has no source item"));
            }
        }
    }
    Ok(report)
}

pub fn turn_report(turn: &SessionTurn, text: &str) -> crate::report_gate::Report {
    use crate::report_gate::{Report, Salience};
    let id = uuid::Uuid::new_v4().to_string();
    let mut report = Report::new(
        id,
        "slack_turn",
        format!("turn:{}:{}", turn.key.storage_key(),
            turn.status_ts.as_deref().unwrap_or(&turn.text)),
        Salience::Result,
        text,
    );
    if !turn.text.starts_with("[LANE_EVENT]") {
        report.reply_to_owner = turn.status_ts.is_some();
        return report;
    }
    // Event identity, not model phrasing, shares a topic with the original inbox alert.
    if let Some((_, event)) = turn.text.split_once("inbox_urgent: inbox:") {
        if let Some(item) = event.split_whitespace().next() {
            report.source = "inbox".into();
            report.topic_key = format!("inbox:{item}");
            report = report.with_state(&format!("open:{item}"));
        }
    } else if let Some((_, event)) = turn.text.split_once("- kibitzer: ") {
        // Every kibitzer wake reminds the owner of the same open items: one topic per lead, so a
        // reminder is compared with the last one delivered. The factual open-items state, when
        // the lane supplies it, decides an unchanged repeat without a judgment.
        report.source = "kibitzer".into();
        report.topic_key = format!("kibitzer:open-items:{}", turn.key.storage_key());
        if let Some((state, _)) = event.strip_prefix("[report_state=").and_then(|rest| rest.split_once(']')) {
            report = report.with_state(state);
        }
    } else if turn.text.contains("action_loop:") {
        report.source = "action_loop".into();
        report.salience = Salience::Action;
    }
    report
}

async fn status(turn: &SessionTurn, name: &str, add: bool) {
    if let (Some(channel), Some(ts)) = (&turn.reply_channel, &turn.status_ts) {
        react(channel, ts, name, add).await;
    }
}

/// A running turn's status message, shared by its ticker and its interim relay.
type StatusDriver = Arc<tokio::sync::Mutex<Option<super::turn_status::StatusMessage>>>;

/// The reason a status message closes with when the owner stopped the turn.
const STOPPED_REASON: &str = "stopped by the owner";

/// omo's status message for the owner's long turns: posted and edited on its interval while
/// the turn runs, closed by [`TurnStatus::finish`]. Shared by the omo and supervisor turns.
pub(crate) struct TurnStatus {
    driver: StatusDriver,
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

    fn driver(&self) -> StatusDriver {
        self.driver.clone()
    }

    /// Stops the ticker and closes the message with the turn's outcome. Every outcome but a
    /// stop by the owner (`Err("stopped by the owner")`) is followed by the turn's own answer
    /// or notice, so the status message is deleted then.
    pub(crate) async fn finish(self, outcome: std::result::Result<(), String>) {
        let answered = !matches!(&outcome, Err(reason) if reason == STOPPED_REASON);
        self.finish_answered(outcome, answered).await;
    }

    /// [`Self::finish`] with `answered` saying whether the turn posts its own message.
    pub(crate) async fn finish_answered(self, outcome: std::result::Result<(), String>, answered: bool) {
        self.ticker.abort();
        let _ = self.ticker.await;
        if let Some(message) = self.driver.lock().await.take() {
            message.finish(chrono::Utc::now(), outcome, answered).await;
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
        // Each turn launches its own engine, so its instructions carry the owner's clock at
        // this turn's start: relative dates ("today", "tomorrow 09:30") are the owner's.
        system_prompt: Some(format!(
            "{}\n{}",
            with_turn_rules(state.system_prompt.as_deref()),
            owner_clock_line(chrono::Utc::now())
        )),
        model: if turn.background {
            crate::model_policy::background_model()
                .or_else(|| state.active_model.clone())
                .or_else(|| config.default_model.clone())
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
    // Owner answers given in other threads of this conversation since its last reply, so a
    // lead turn never asks again what the owner already decided elsewhere.
    if let Some(context) = thread_replies_context(pool, &turn.key, chrono::Utc::now()).await {
        request.text = format!("{context}\n{}", request.text);
    }
    // A thread started before the rules existed never saw them in its developer instructions.
    let rules_known =
        state.metadata.get("turn_rules").and_then(Value::as_str) == Some(TURN_RULES_MARKER);
    if request.thread_id.is_some() && !rules_known {
        request.text = format!("{}\n\n{}", turn_rules(), request.text);
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
    // omo's status message for the owner's long turns. Interim progress notes edit it instead
    // of becoming messages of their own.
    let status_message = TurnStatus::start(
        config,
        &turn.key,
        turn.reply_channel.as_deref(),
        turn.reply_thread.as_deref(),
        &turn.text,
    );
    let relay_status = status_message.as_ref().map(TurnStatus::driver);
    // The latest interim text that was folded into the status message, not posted.
    let held: Arc<Mutex<Option<String>>> = Arc::default();
    let relay_held = held.clone();
    let relay = super::slack::spawn_carrying_test_api(async move {
        let judge = crate::judge::Judge::from_config(crate::judge::typesafe_from_env())
            .with_pool(relay_pool.clone());
        while let Some(text) = interim.recv().await {
            let (text, claims) = if super::report_claims::enabled() {
                super::report_claims::strip_interim(&text)
            } else {
                (text, Vec::new())
            };
            if !text.trim().is_empty() {
                relay_interim(&relay_pool, &judge, &relay_turn, relay_status.as_ref(), &relay_held, &text)
                    .await;
            }
            if let Some(target) = &relay_target {
                super::report_claims::record_and_schedule(&relay_pool, target, claims, claim_delay)
                    .await;
            }
        }
    });
    tracing::info!(session = %storage_key, resume = request.thread_id.is_some(), "session turn started on mhc RPC");
    let started = std::time::Instant::now();
    let channel = turn.reply_channel.clone().unwrap_or_default();
    let registration = super::turns::begin(&storage_key, &channel);
    // Boxed: the RPC turn plus the approval wrapper is a large future, and an unboxed
    // copy on each await level overflowed a 2 MiB worker stack in debug builds.
    let run = Box::pin(async {
        // Owned here so the relay ends when the turn's last sender drops.
        let progress = progress;
        let mut result =
            omo_client::run_turn_recovering(&omo, &request, Some(progress.clone())).await;
        // omo-gateway retries a quota/overload failure on the fallback models, in order.
        if let Some(primary) = request.model.clone() {
            for fallback in omo_client::fallback_models(&primary) {
                let Some(retry_kind) = omo_client::fallback_retry(&result) else {
                    break;
                };
                tracing::warn!(session = %storage_key, model = %fallback, error = ?result.as_ref().err().map(ToString::to_string), "retrying turn failure with fallback model");
                // A resend runs the request again where it started; a continue resumes the
                // session the failed run worked in.
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
        }
        // Any other failed attempt (or one the fallback chain did not recover) is retried once
        // by the hub itself, resuming the attempt on the background model (or the turn's own
        // model), so the owner never has to resend.
        let Some(error) = omo_client::attempt_failure(&result).filter(|_| omo_client::retry_eligible(&result)) else {
            return result;
        };
        let retry = omo_client::retry_request(&request, &result, omo_client::fallback_model());
        tracing::warn!(session = %storage_key, model = ?retry.model, %error, "retrying the failed turn once");
        omo_client::run_turn(&omo, &retry, Some(progress)).await
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
    let held = held.lock().unwrap_or_else(|poison| poison.into_inner()).take();
    let stopped = matches!(&result, Err(error) if error.to_string().contains(omo_client::STOPPED));
    let failure = omo_client::attempt_failure(&result);
    if let Some(status_message) = status_message {
        let outcome = match (&failure, stopped) {
            (_, true) => Err(crate::locale::text("중단됨", "stopped")),
            (Some(error), false) => Err(error.clone()),
            (None, false) => Ok(()),
        };
        // Every turn that was not stopped posts its answer or a notice below, so its status
        // message never stays behind as a bare status post.
        status_message.finish_answered(outcome, !stopped).await;
    }
    status(&turn, "eyes", false).await;
    if stopped {
        tracing::info!(session = %storage_key, "session turn stopped by the owner");
        return None;
    }
    if let Some(failure) = failure {
        tracing::warn!(session = %storage_key, error = %failure, "session turn failed after its retry");
        let notice = if schedule_resume(pool, &turn, &failure).await {
            omo_client::owner_resume_text(&failure, RESUME_AFTER_MINUTES)
        } else {
            omo_client::owner_failure_text(&failure)
        };
        emit_result(pool, &turn, &notice, Commitments::default(), true).await;
        status(&turn, "x", true).await;
        return None;
    }
    let Ok(outcome) = result else {
        return None;
    };
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
    // `attempt_failure` found no failure, so the outcome has a reply or chose silence.
    let reply = omo_client::final_reply(&outcome).ok().flatten();
    // A turn whose last words were an interim note folded into the status message answers
    // with that note: it is the answer, and the status message is gone.
    let reply = final_or_held(reply, held, omo_client::is_explicit_silence(&outcome.text));
    let mut pending = false;
    let (reply, claims) = match reply {
        Some(text) => {
            let (clean, claims) = super::report_claims::strip_reply(&text);
            (Some(clean).filter(|clean| !clean.trim().is_empty()), claims)
        }
        None => (None, Vec::new()),
    };
    if let Some(text) = &reply {
        let commitments = track_commitments(config, pool, &turn, text).await;
        pending = emit_result(pool, &turn, text, commitments, true).await;
        if turn.reply_channel.is_some() {
            if let Err(error) =
                super::transcript::record(pool, &storage_key, "assistant", text).await
            {
                tracing::warn!(%error, "session reply was not stored");
            }
        }
    }
    if let Some(target) = &claim_target {
        super::report_claims::record_and_schedule(pool, target, claims, claim_delay).await;
    }
    // A withheld promise is not a finished request: its result is still owed.
    status(&turn, if pending { "hourglass_flowing_sand" } else { "white_check_mark" }, true).await;
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
    reply
}

/// The final answer of a turn: its reply, else (unless it chose silence) the last interim
/// note that was folded into the status message instead of being posted.
fn final_or_held(reply: Option<String>, held: Option<String>, silent: bool) -> Option<String> {
    reply.or(if silent { None } else { held })
}

/// How an interim message of a turn reaches the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterimRoute {
    /// A question or decision for the owner: posted through the report gate as before.
    Post,
    /// Progress: folded into the turn's status message, never a message of its own.
    Coalesce,
    /// Prose in a language the owner does not read: working narration, never shown.
    Drop,
}

/// The routing of one interim text, given its judged salience (`None` when unjudged).
fn interim_route(text: &str, salience: Option<crate::report_gate::Salience>) -> InterimRoute {
    use crate::report_gate::Salience;
    if is_foreign_interim(text) {
        return InterimRoute::Drop;
    }
    match salience {
        // An approval or choice always reaches the owner. Any other "action" is posted only
        // when it is not working narration: an unavailable judge answers "action"
        // conservatively, and narration such as "확인하겠습니다." must still fold into the status.
        Some(Salience::Decision) => InterimRoute::Post,
        Some(Salience::Action) if !crate::report_gate::is_leak(text) => InterimRoute::Post,
        _ => InterimRoute::Coalesce,
    }
}

/// True when an interim text's prose is not in the owner's language. Code spans, links and
/// identifiers (`wishket.ts`, paths, ids) are not prose and do not count.
fn is_foreign_interim(text: &str) -> bool {
    static NOT_PROSE: OnceLock<regex::Regex> = OnceLock::new();
    let not_prose = NOT_PROSE.get_or_init(|| {
        regex::Regex::new(r"`[^`]*`|<[^>\s]*>|https?://\S+|\S*\w[/._:@#=]\w\S*")
            .expect("valid prose regex")
    });
    crate::locale::current().is_foreign_prose(&not_prose.replace_all(text, " "))
}

/// The session the report gate files a Slack turn's reports under.
fn gate_session(channel: &str, turn: &SessionTurn) -> String {
    SessionKey::new("slack", None::<String>, channel, turn.reply_thread.clone(), "owner")
        .storage_key()
}

/// One interim message of a running turn. Foreign-language narration is dropped; an owner
/// question or decision is posted through the same gate as final reports; everything else
/// updates the turn's status message and is remembered as the turn's latest progress. A
/// dashboard session keeps every interim message in its transcript.
async fn relay_interim(
    pool: &SqlitePool,
    judge: &crate::judge::Judge,
    turn: &SessionTurn,
    status: Option<&StatusDriver>,
    held: &Mutex<Option<String>>,
    text: &str,
) {
    let text = outgoing_text(text);
    if text.is_empty() || omo_client::is_explicit_silence(&text) {
        return;
    }
    let Some(channel) = &turn.reply_channel else {
        if let Err(error) =
            super::transcript::record(pool, &turn.key.storage_key(), "assistant", &text).await
        {
            tracing::warn!(%error, "dashboard reply was not stored");
        }
        return;
    };
    if is_foreign_interim(&text) {
        tracing::info!(session = %turn.key.storage_key(), "interim narration not in the owner's language was not posted");
        return;
    }
    let session = gate_session(channel, turn);
    let judged = match delivery_report(pool, turn, &text, false).await {
        Ok(mut report) => {
            match crate::report_gate::consolidate_turn_report(pool, judge, &session, &mut report).await {
                Ok(()) => Some(report),
                Err(error) => {
                    tracing::warn!(%error, "interim message was not judged; kept as progress");
                    None
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "interim report source could not be resolved");
            None
        }
    };
    match interim_route(&text, judged.as_ref().map(|report| report.salience)) {
        InterimRoute::Post => {
            let Some(report) = judged else { return };
            *held.lock().unwrap_or_else(|poison| poison.into_inner()) = None;
            if let Err(error) = crate::owner_alert::notify_report(pool, &session, &report).await {
                tracing::warn!(%error, "interim owner question could not be delivered");
            }
        }
        InterimRoute::Coalesce => {
            *held.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(text.clone());
            if let Some(status) = status {
                if let Some(message) = status.lock().await.as_mut() {
                    message.note_progress(&text, chrono::Utc::now()).await;
                }
            }
        }
        InterimRoute::Drop => {}
    }
}

/// Minutes after which a turn that failed twice is resumed by the hub.
pub const RESUME_AFTER_MINUTES: i64 = 5;
/// Sentinel of the hub's resume of a failed turn; a resume that fails is not resumed again.
pub const RESUME_MARKER: &str = "[hub resume]";

/// Lane event body that resumes a failed turn in its own conversation.
fn resume_body(turn: &SessionTurn, failure: &str) -> String {
    let error: String = failure.chars().take(120).collect();
    let message: String = turn.text.chars().take(300).collect();
    format!(
        "{RESUME_MARKER} The hub could not answer the message below even after one retry ({error}). Answer it now in this conversation, continuing any work already done. Message: {message}"
    )
}

/// Queues the hub's own resume of a turn that failed twice: an owner-urgent lane event for the
/// turn's session, released after [`RESUME_AFTER_MINUTES`], which the lane dispatcher runs as
/// a turn in the same conversation. Returns false (nothing queued) for a resume that failed
/// again, a dashboard turn, or a store error.
async fn schedule_resume(pool: &SqlitePool, turn: &SessionTurn, failure: &str) -> bool {
    if turn.reply_channel.is_none()
        || turn.text.contains(RESUME_MARKER)
        || failure.contains(omo_client::DENIAL_LIMIT)
    {
        return false;
    }
    let now = chrono::Utc::now();
    let release = now + chrono::Duration::minutes(RESUME_AFTER_MINUTES);
    let queued = sqlx::query(
        "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at, release_at) VALUES(NULL,?,'owner_reply_failed',?,?,?)",
    )
    .bind(turn.key.storage_key())
    .bind(resume_body(turn, failure))
    .bind(now.to_rfc3339())
    .bind(release.to_rfc3339())
    .execute(pool)
    .await;
    match queued {
        Ok(_) => true,
        Err(error) => {
            tracing::warn!(%error, "the failed turn could not be queued for resume");
            false
        }
    }
}

/// The owner's current local date and time with zone, at the head of every turn prompt.
fn owner_clock_line(now: chrono::DateTime<chrono::Utc>) -> String {
    let locale = crate::locale::current();
    format!(
        "[Owner local time: {} ({}). Resolve today, tomorrow and other relative dates against this.]",
        locale.format(now, "%A %Y-%m-%d %H:%M"),
        locale.timezone
    )
}

/// How far back a top-level conversation looks for its threads' messages.
const THREAD_CONTEXT_HOURS: i64 = 24;
const THREAD_CONTEXT_LINES: usize = 12;
const THREAD_CONTEXT_CHARS: usize = 300;

/// The latest messages of the other threads of a top-level conversation (the owner DM or the
/// home channel) within the last day: the owner's answers to questions asked there, with the
/// bot's text they answer. Every lead turn carries them, so a decision made in a thread stays
/// visible to later lead turns. `None` for a thread session or when the owner said nothing
/// in a thread.
async fn thread_replies_context(
    pool: &SqlitePool,
    key: &SessionKey,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    if key.thread_id.as_deref().is_some_and(|thread| !thread.is_empty()) {
        return None;
    }
    let storage_key = key.storage_key();
    let since = now - chrono::Duration::hours(THREAD_CONTEXT_HOURS);
    let rows: Vec<(String, String, String)> = match sqlx::query_as(
        "SELECT session_key, role, content FROM messages
         WHERE session_key <> ? AND role IN ('user', 'assistant')
           AND julianday(created_at) > julianday(?)
         ORDER BY sequence",
    )
    .bind(&storage_key)
    .bind(since.to_rfc3339())
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, "thread replies were not loaded for the lead turn");
            return None;
        }
    };
    let lines: Vec<String> = rows
        .into_iter()
        .filter_map(|(session, role, content)| {
            let session = SessionKey::from_storage_key(&session).ok()?;
            let thread = session.thread_id.as_deref().filter(|thread| !thread.is_empty())?;
            if session.platform != key.platform || session.channel_id != key.channel_id {
                return None;
            }
            thread_context_line(thread, &role, &content)
        })
        .collect();
    thread_context(&lines)
}

fn thread_context_line(thread: &str, role: &str, content: &str) -> Option<String> {
    let text: String = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let text: String = text.chars().take(THREAD_CONTEXT_CHARS).collect();
    if text.is_empty() {
        return None;
    }
    let who = if role == "user" { "owner" } else { "bot" };
    Some(format!("- thread {thread} {who}: {text}"))
}

/// The prompt block for [`thread_replies_context`]: the newest lines, oldest first.
fn thread_context(lines: &[String]) -> Option<String> {
    if !lines.iter().any(|line| line.contains(" owner: ")) {
        return None;
    }
    let newest = &lines[lines.len().saturating_sub(THREAD_CONTEXT_LINES)..];
    Some(format!(
        "[Recent messages in this conversation's threads (last 24 hours). The owner's answers there are decisions already made: do not ask them again.]\n{}",
        newest.join("\n")
    ))
}

/// How often overdue owner commitments are swept (docs/design/hub-owner-reporting.md section 5).
const COMMITMENT_SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(600);

/// The commitment thread of a Slack reply: `channel` or `channel:thread_ts`.
fn commitment_thread_ref(channel: &str, thread: Option<&str>) -> String {
    match thread.filter(|thread| !thread.is_empty()) {
        Some(thread) => format!("{channel}:{thread}"),
        None => channel.to_owned(),
    }
}

/// The Slack channel and thread a [`commitment_thread_ref`] names.
fn split_commitment_thread_ref(thread_ref: &str) -> (&str, Option<&str>) {
    match thread_ref.split_once(':') {
        Some((channel, thread)) => (channel, Some(thread)),
        None => (thread_ref, None),
    }
}

/// The commitment ledger on a bot reply in an owner conversation ([`crate::commitments::track`]):
/// the reply closes every open promise of this conversation it delivers, and a reply that
/// promises later work opens one.
async fn track_commitments(config: &SlackTurnConfig, pool: &SqlitePool, turn: &SessionTurn, text: &str) -> Commitments {
    let judge = crate::judge::Judge::from_config(crate::judge::typesafe_from_env());
    let Some(owner) = config.allowed_users.first() else {
        return Commitments::default();
    };
    match track_reply_commitments(pool, &judge, owner, turn, text, chrono::Utc::now()).await {
        Ok(commitments) => commitments,
        Err(error) => {
            tracing::warn!(%error, "commitments were not tracked for this reply");
            Commitments::default()
        }
    }
}

/// The commitment ledger step of an owner-conversation reply, shared by the live turn and the
/// report replay so both gate the same verdict.
pub async fn track_reply_commitments(
    pool: &SqlitePool,
    judge: &crate::judge::Judge,
    owner: &str,
    turn: &SessionTurn,
    text: &str,
    at: chrono::DateTime<chrono::Utc>,
) -> crate::Result<Commitments> {
    let Some(channel) = turn.reply_channel.as_deref() else {
        return Ok(Commitments::default());
    };
    // A reply to a person's own message is owed wherever it was asked (`status_ts` names that
    // admitted message); unprompted turns are tracked only in the owner's lead conversations.
    if turn.status_ts.is_none() && !super::turn_status::is_owner_lead(&turn.key, owner) {
        return Ok(Commitments::default());
    }
    let thread_ref = commitment_thread_ref(channel, turn.reply_thread.as_deref());
    // The post path returns no ts: the answered owner message names the reply, else a fresh id.
    let message_ref = match &turn.status_ts {
        Some(ts) => format!("{channel}:{ts}:reply"),
        None => format!("{channel}:{}", uuid::Uuid::new_v4()),
    };
    let tracked = crate::commitments::track(pool, judge, channel, &thread_ref, &message_ref, text, at).await?;
    Ok(Commitments::from(&tracked))
}

/// Report lessons injected into reports nobody objected to for a week count as helped.
async fn sweep_report_lessons(pool: &SqlitePool) {
    match crate::supervisor::lessons::sweep_report_outcomes(pool, chrono::Utc::now()).await {
        Ok(0) => {}
        Ok(settled) => tracing::info!(settled, "report lessons settled helped"),
        Err(error) => tracing::warn!(%error, "report lesson sweep failed"),
    }
}

/// Posts one follow-up reply per overdue commitment in its thread.
async fn sweep_commitments(pool: &SqlitePool) {
    match crate::commitments::sweep(pool, chrono::Utc::now()).await {
        Ok(follow_ups) => {
            for follow_up in follow_ups {
                let Some(outbox_id) = follow_up.outbox_id else { continue; };
                let (channel, thread) = split_commitment_thread_ref(&follow_up.thread_ref);
                let session = SessionKey::new("slack", None::<String>, channel, thread, "owner");
                if let Err(error) = crate::owner_alert::post_queued(
                    pool, outbox_id, &session.storage_key(), &follow_up.body_ko,
                ).await {
                    tracing::warn!(%error, "commitment follow-up remains in the delivery outbox");
                }
            }
        }
        Err(error) => tracing::warn!(%error, "commitment sweep failed"),
    }
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
    let cursor = (
        admitted.bot_user.clone(),
        admitted.channel.clone(),
        admitted.key.thread_id.clone().unwrap_or_default(),
        admitted.message_ts.clone(),
    );
    // Owner alert feedback ("이거 그만 보내") is handled by the bot itself, before and instead
    // of an agent turn.
    owner_commitment_reply(config, pool, &admitted).await;
    if !owner_report_feedback(config, pool, &admitted).await {
        Box::pin(run_admitted_turn(config, pool, admitted)).await;
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

/// The agent turn of an admitted message: the supervisor's conversation or an omo turn.
async fn run_admitted_turn(config: &SlackTurnConfig, pool: &SqlitePool, admitted: Admitted) {
    let text = Box::pin(prepare_prompt(pool, &admitted)).await;
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
}

/// The hub owner: the first allowed user.
fn owner_id(config: &SlackTurnConfig) -> Option<&str> {
    config.allowed_users.first().map(String::as_str)
}

/// The owner's reply in a thread where the bot promised later work: a promise he calls off is
/// closed so the sweeper never chases it ([`crate::commitments::owner_reply`]).
async fn owner_commitment_reply(config: &SlackTurnConfig, pool: &SqlitePool, admitted: &Admitted) {
    let Some(owner) = owner_id(config) else {
        return;
    };
    // Every thread where the bot can owe the owner an answer is covered, matching where
    // [`track_reply_commitments`] records promises.
    if admitted.user != owner {
        return;
    }
    let Some(thread) = reply_thread_ts(&admitted.event) else {
        return;
    };
    let thread_ref = commitment_thread_ref(&admitted.channel, Some(thread));
    let judge = crate::judge::Judge::from_config(crate::judge::typesafe_from_env());
    match crate::commitments::owner_reply(pool, &judge, &thread_ref, &admitted.text).await {
        Ok(closed) if !closed.is_empty() => {
            tracing::info!(count = closed.len(), "owner called off commitments");
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "owner reply was not checked against commitments"),
    }
}

/// Owner alert feedback (docs/design/hub-owner-reporting.md section 3, Capture): an owner
/// message in an owner conversation that the judge reads as "stop/keep sending this kind of
/// alert" stores the rule and gets the Korean confirmation in its thread. Returns whether the
/// message was feedback, in which case no agent turn runs. A failure leaves the message to
/// the normal turn.
async fn owner_report_feedback(
    config: &SlackTurnConfig,
    pool: &SqlitePool,
    admitted: &Admitted,
) -> bool {
    let Some(owner) = owner_id(config) else {
        return false;
    };
    if admitted.user != owner || !super::turn_status::is_owner_lead(&admitted.key, owner) {
        return false;
    }
    let thread = reply_thread_ts(&admitted.event);
    let judge = crate::judge::Judge::from_config(crate::judge::typesafe_from_env());
    match crate::report_feedback::handle_owner_message(
        pool,
        &judge,
        &admitted.channel,
        thread,
        &admitted.text,
    )
    .await
    {
        Ok(Some(reply)) => {
            let thread = thread.unwrap_or(admitted.message_ts.as_str());
            post(&admitted.channel, Some(thread), &reply).await;
            true
        }
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(%error, "owner report feedback was not handled; running the normal turn");
            false
        }
    }
}

/// Whether an `events_api` payload is a `reaction_added` event.
fn is_reaction(payload: &Value) -> bool {
    payload
        .get("event")
        .is_some_and(|event| str_field(event, "type") == Some("reaction_added"))
}

/// The owner's `:no_bell:` reaction on a delivered alert mutes that kind of alert and confirms
/// in the alert's thread. Other users' reactions and other reactions do nothing.
async fn owner_reaction(config: &SlackTurnConfig, pool: &SqlitePool, payload: &Value) {
    let Some(event) = payload.get("event") else {
        return;
    };
    let (Some(user), Some(reaction), Some(item)) = (
        str_field(event, "user"),
        str_field(event, "reaction"),
        event.get("item"),
    ) else {
        return;
    };
    let (Some(channel), Some(ts)) = (str_field(item, "channel"), str_field(item, "ts")) else {
        return;
    };
    if owner_id(config) != Some(user) {
        return;
    }
    let judge = crate::judge::Judge::from_config(crate::judge::typesafe_from_env());
    match crate::report_feedback::handle_reaction(pool, &judge, channel, ts, reaction).await {
        Ok(Some(reply)) => post(channel, Some(ts), &reply).await,
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "owner alert reaction was not applied"),
    }
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
    let mut last_sweep: Option<std::time::Instant> = None;
    loop {
        ticks.tick().await;
        if last_sweep.is_none_or(|at| at.elapsed() >= COMMITMENT_SWEEP_EVERY) {
            last_sweep = Some(std::time::Instant::now());
            sweep_commitments(&pool).await;
            sweep_report_lessons(&pool).await;
        }
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
         WHERE delivered_at IS NULL AND cancelled_at IS NULL AND platform = 'slack'
           AND julianday(COALESCE(report_not_before, created_at)) >= julianday(?)
           AND julianday(COALESCE(report_not_before, created_at)) <= julianday(?)
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
        match super::reply::deliver_stored_posted(pool, id, &key.storage_key(), &text).await {
            Ok(Some(posted)) => {
                crate::report_feedback::posted_from_outbox(pool, id, &posted).await?;
                sent += 1;
            }
            Ok(None) => {},
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
    if is_reaction(payload) {
        let (pool, payload) = (pool.clone(), payload.clone());
        super::slack::spawn_carrying_test_api(async move {
            owner_reaction(config, &pool, &payload).await;
        });
        return true;
    }
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

    #[tokio::test]
    async fn direct_owner_answer_is_not_filtered_as_background_narration() {
        use crate::report_gate::{queue, Decision};
        let pool = crate::storage::memory().await.unwrap();
        let mut turn = SessionTurn {
            key: SessionKey::new("slack", None::<String>, "D1", None::<String>, "U1"),
            reply_channel: Some("D1".into()), reply_thread: None,
            status_ts: Some("1.0".into()), text: "Why did the build fail?".into(), background: false,
        };
        let text = "The build failed because the runner was offline.";
        let mut interim = delivery_report(&pool, &turn, &"중간 처리 내용을 확인하고 있습니다. ".repeat(30), false).await.unwrap();
        assert!(!interim.reply_to_owner);
        crate::report_gate::consolidate_turn_report(&pool, &crate::judge::Judge::forced("internal"), "owner", &mut interim)
            .await.unwrap();
        assert_eq!(queue(&pool, "owner", &interim, false).await.unwrap().0, Decision::Internal);
        let report = delivery_report(&pool, &turn, text, true).await.unwrap();
        assert_eq!(queue(&pool, "owner", &report, false).await.unwrap().0, Decision::Sent);
        assert_eq!(queue(&pool, "owner", &report, false).await.unwrap().0, Decision::Duplicate);
        let scope = crate::report_rules::scope_from_alert("slack_turn", None, None, Some(&report.topic_key));
        crate::report_rules::record(&pool, &scope, crate::report_rules::Effect::Mute, "stop", None).await.unwrap();
        assert_eq!(queue(&pool, "owner", &report, false).await.unwrap().0, Decision::Muted);
        turn.status_ts = None;
        turn.text = "[LANE_EVENT] build finished".into();
        assert_eq!(queue(&pool, "owner", &turn_report(&turn, text), false).await.unwrap().0, Decision::Internal);
    }

    fn owner_turn() -> SessionTurn {
        SessionTurn {
            key: SessionKey::new("slack", None::<String>, "D1", None::<String>, "U1"),
            reply_channel: Some("D1".into()), reply_thread: None,
            status_ts: Some("1.0".into()), text: "배포가 왜 실패했는지 알려줘".into(), background: false,
        }
    }

    /// A judge that answers the salience call with `salience` and keeps every same-turn report new.
    fn salience_judge(salience: &'static str) -> crate::judge::Judge {
        crate::judge::Judge::scripted(move |purpose, _, _| match purpose {
            crate::judge::Purpose::ReportSalience => salience.into(),
            _ => "new_information".into(),
        })
    }

    #[tokio::test]
    async fn a_final_that_only_opens_a_commitment_is_left_to_the_ledger() {
        use crate::report_gate::Decision;
        let pool = crate::storage::memory().await.unwrap();
        let judge = crate::judge::Judge::scripted(|purpose, _, _| match purpose {
            crate::judge::Purpose::Commitment => "promise".into(),
            crate::judge::Purpose::ReportSalience => "internal".into(),
            _ => "new_information".into(),
        });
        let turn = owner_turn();
        let text = "로그를 모으는 중이에요. 결과가 나오면 바로 이어서 원인을 정리할게요.";
        let commitments = track_reply_commitments(&pool, &judge, &turn.key.user_id, &turn, text, chrono::Utc::now())
            .await.unwrap();
        assert!(commitments.opens);
        let mut report = delivery_report(&pool, &turn, text, true).await.unwrap();
        apply_commitments(&mut report, commitments);
        crate::report_gate::consolidate_turn_report(&pool, &judge, "owner", &mut report).await.unwrap();
        assert_eq!(crate::report_gate::queue(&pool, "owner", &report, false).await.unwrap().0, Decision::Internal);
        let open: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM commitments WHERE state='open'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(open, 1);

        // An owner-facing answer that also promises more is still sent.
        let mut report = delivery_report(&pool, &turn, "인증서 만료가 원인이었습니다. 재배포 결과도 알려드릴게요.", true)
            .await.unwrap();
        apply_commitments(&mut report, Commitments { fulfils: false, opens: true });
        crate::report_gate::consolidate_turn_report(&pool, &salience_judge("result"), "owner", &mut report).await.unwrap();
        assert_eq!(crate::report_gate::queue(&pool, "owner", &report, false).await.unwrap().0, Decision::Sent);
    }

    #[tokio::test]
    async fn commitment_scope_and_event_identity_survive_the_ledger() {
        use crate::report_gate::Decision;
        let pool = crate::storage::memory().await.unwrap();
        let judge = crate::judge::Judge::scripted(|_, _, _| "promise".into());
        // A question the owner asked in a shared channel thread is owed an answer there too.
        let asked = SessionTurn {
            key: SessionKey::new("slack", Some("T1"), "C1", Some("9.0"), ""),
            reply_channel: Some("C1".into()), reply_thread: Some("9.0".into()),
            status_ts: Some("9.1".into()), text: "근본 원인이 뭐지".into(), background: false,
        };
        let tracked = track_reply_commitments(&pool, &judge, "UOWNER", &asked,
            "로그를 모으는 중이에요. 결과가 나오면 원인을 정리할게요.", chrono::Utc::now()).await.unwrap();
        assert!(tracked.opens);
        // An unprompted turn in a channel that is not the owner's lead is still not tracked.
        let unprompted = SessionTurn { status_ts: None, ..asked.clone() };
        assert!(!track_reply_commitments(&pool, &judge, "UOWNER", &unprompted,
            "결과가 나오면 알려드릴게요.", chrono::Utc::now()).await.unwrap().opens);

        // A lane event that also fulfils a promise keeps its inbox identity, so a second
        // report about the same item is still a duplicate.
        crate::inbox::ingest(&pool, "mail:1", "mail", "mail:1", "", "2026-10-30T00:00:00Z").await.unwrap();
        sqlx::query("UPDATE inbox_items SET author='GitLab' WHERE id='mail:1'").execute(&pool).await.unwrap();
        let event = SessionTurn {
            key: SessionKey::new("slack", None::<String>, "D1", None::<String>, "U1"),
            reply_channel: Some("D1".into()), reply_thread: None, status_ts: None,
            text: "[LANE_EVENT]\n- inbox_urgent: inbox:mail:1 due 2026-10-30".into(), background: true,
        };
        let first = delivery_report(&pool, &event, "GitLab 새 위치 로그인 알림을 확인해 주세요.", true).await.unwrap();
        assert_eq!(crate::report_gate::queue(&pool, "owner", &first, false).await.unwrap().0, Decision::Sent);
        let mut second = delivery_report(&pool, &event, "그 로그인은 대표님 맥에서 한 것이라 하실 일은 없습니다.", true).await.unwrap();
        apply_commitments(&mut second, Commitments { fulfils: true, opens: false });
        assert_eq!(second.topic_key, "inbox:mail:1");
        assert_eq!(crate::report_gate::queue(&pool, "owner", &second, false).await.unwrap().0, Decision::Duplicate);
    }

    /// The report `emit_result` hands to the gate: built, judged, then queued.
    async fn gated(pool: &SqlitePool, salience: &'static str, text: &str, final_answer: bool) -> (crate::report_gate::Decision, Option<i64>) {
        let mut report = delivery_report(pool, &owner_turn(), text, final_answer).await.unwrap();
        crate::report_gate::consolidate_turn_report(pool, &salience_judge(salience), "owner", &mut report)
            .await.unwrap();
        assert_eq!(report.salience.as_str(), salience);
        crate::report_gate::queue(pool, "owner", &report, false).await.unwrap()
    }

    #[tokio::test]
    async fn a_progress_only_final_is_suppressed_only_after_the_turn_was_received() {
        use crate::report_gate::Decision;
        let pool = crate::storage::memory().await.unwrap();
        let progress = "원인 분석을 계속 진행하고 있고, 정리되는 대로 다시 공유드리려고 합니다. 로그와 배포 기록을 차례로 살펴보는 중이며 관련 설정도 함께 비교하고 있습니다.";
        // An interim message carrying the answer is judged on its content and delivered.
        let (decision, id) = gated(&pool, "result", "배포는 서명 인증서 만료로 실패했습니다.", false).await;
        assert_eq!(decision, Decision::Sent);
        // Admitted but not yet posted: the owner has received nothing, so the final reply is sent.
        assert_eq!(gated(&pool, "internal", progress, true).await.0, Decision::Sent);
        // Once the answer has a transport receipt, a final that adds only progress is suppressed
        // although it is final and replies to the owner.
        crate::chat::reply::mark_delivered(&pool, id.unwrap()).await.unwrap();
        let progress = "근거 자료를 더 모으는 작업을 이어서 진행하고 있고, 확인되는 대로 다시 공유드리려고 합니다. 추가 로그와 설정 비교도 함께 살펴보고 있습니다.";
        assert_eq!(gated(&pool, "internal", progress, true).await.0, Decision::Internal);

        // Without any delivered message in the turn, the owner still gets the real final reply.
        let pool = crate::storage::memory().await.unwrap();
        assert_eq!(gated(&pool, "internal", progress, true).await.0, Decision::Sent);
    }

    #[tokio::test]
    async fn an_action_bearing_interim_message_reaches_the_owner() {
        use crate::report_gate::Decision;
        let pool = crate::storage::memory().await.unwrap();
        let text = "배포를 계속하려면 Apple 개발자 계정에서 새 인증서를 승인해 주세요.";
        assert_eq!(gated(&pool, "action", text, false).await.0, Decision::Sent);
        // Tool wording trips the leak heuristic, but it never overrides the judged verdict.
        let tooled = "Bash(cargo test) 결과 서명 단계에서 막혔습니다. 키체인 접근을 허용해 주세요.";
        assert!(crate::report_gate::is_leak(tooled));
        let (decision, outbox) = gated(&pool, "action", tooled, false).await;
        assert_eq!(decision, Decision::Sent);
        assert!(crate::report_gate::prepare_delivery_at(&pool, outbox.unwrap(), chrono::Utc::now())
            .await.unwrap().is_some());
        // A pure-progress interim message is still kept internal by the same judgment.
        let progress = "인증서 상태를 확인하고 배포 로그를 다시 살펴보는 작업을 이어서 진행하고 있으며, 끝나면 정리해서 공유드리려고 합니다. 조금 더 시간이 걸릴 수 있습니다.";
        assert_eq!(gated(&pool, "internal", progress, false).await.0, Decision::Internal);
        // Judged tool chatter stays internal too.
        assert_eq!(gated(&pool, "internal", "Running Bash(cargo test) now", false).await.0, Decision::Internal);
    }

    #[tokio::test]
    async fn the_requested_final_answer_is_sent_and_never_deferred_to_the_digest() {
        use crate::report_gate::{Decision, Salience};
        let pool = crate::storage::memory().await.unwrap();
        let text = "배포는 서명 인증서가 어제 만료되어 실패했습니다.";
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = seen.clone();
        let judge = crate::judge::Judge::scripted(move |_, state, options| {
            record.lock().unwrap().push((state.to_owned(), options.iter().map(|o| o.to_string()).collect::<Vec<_>>()));
            "result".into()
        });
        let mut report = delivery_report(&pool, &owner_turn(), text, true).await.unwrap();
        crate::report_gate::consolidate_turn_report(&pool, &judge, "owner", &mut report).await.unwrap();
        assert_eq!(report.salience, Salience::Result);
        assert_eq!(crate::report_gate::queue(&pool, "owner", &report, false).await.unwrap().0, Decision::Sent);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        let state: serde_json::Value = serde_json::from_str(&seen[0].0).unwrap();
        assert_eq!(state["phase"], "final");
        assert_eq!(state["answers_owner_message"], true);
        assert_eq!(state["request"], "배포가 왜 실패했는지 알려줘");
        assert!(!seen[0].1.iter().any(|option| option == "fyi"));
    }

    #[tokio::test]
    async fn an_unavailable_salience_judge_sends_turn_reports() {
        use crate::report_gate::{Decision, Salience};
        let pool = crate::storage::memory().await.unwrap();
        let judge = crate::judge::Judge::forced("unused").with_jev_enabled(false);
        for (text, final_answer) in [
            ("배포를 계속하려면 새 인증서를 승인해 주세요.", false),
            ("배포는 서명 인증서 만료로 실패했습니다.", true),
        ] {
            let mut report = delivery_report(&pool, &owner_turn(), text, final_answer).await.unwrap();
            assert!(crate::report_gate::consolidate_turn_report(&pool, &judge, "owner", &mut report).await.is_err());
            assert_eq!(report.salience, Salience::Result);
            assert_eq!(crate::report_gate::queue(&pool, "owner", &report, false).await.unwrap().0, Decision::Sent);
        }
    }

    #[test]
    fn lane_reports_share_inbox_identity_and_do_not_leak_bot_work() {
        use crate::report_gate::{decide, Decision, Salience, TopicRow};
        let mut turn = SessionTurn {
            key: SessionKey::new("slack", None::<String>, "D1", None::<String>, "U1"),
            reply_channel: Some("D1".into()),
            reply_thread: None,
            status_ts: None,
            text: "[LANE_EVENT]\n- inbox_urgent: inbox:mail:1 due 2026-10-30".into(),
            background: true,
        };
        let report = turn_report(&turn, "인증을 갱신해 주세요.");
        assert_eq!(report.topic_key, "inbox:mail:1");
        let prior = TopicRow {
            topic_key: report.topic_key.clone(),
            last_state_hash: crate::report_gate::state_hash("open:mail:1"),
            last_sent_at: String::new(),
            salience: "action".into(),
        };
        assert_eq!(decide(&report, Some(&prior), None, false), Decision::Duplicate);
        turn.text = "[LANE_EVENT]\n- action_loop: [GitHub] CI failed".into();
        assert_eq!(turn_report(&turn, "검사가 실패했습니다.").source, "action_loop");
        turn.text = "[LANE_EVENT]\n- action_loop: [카톡 · 봇 처리] 요청".into();
        assert_eq!(turn_report(&turn, "요청을 처리했습니다.").source, "action_loop");
        turn.text = "결과를 알려줘".into();
        assert_eq!(turn_report(&turn, "검사를 마쳤습니다.").salience, Salience::Result);
        turn.text = "[LANE_EVENT]\n- kibitzer: [report_state=abc] {}".into();
        let first = turn_report(&turn, "서류를 제출해 주세요.");
        let prior = TopicRow {
            topic_key: first.topic_key,
            last_state_hash: first.state_hash,
            last_sent_at: String::new(),
            salience: "action".into(),
        };
        let reworded = turn_report(&turn, "신청 서류를 보내 주세요.");
        assert_eq!(decide(&reworded, Some(&prior), None, false), Decision::Duplicate);
        turn.text = "[LANE_EVENT]\n- kibitzer: [report_state=changed] {}".into();
        assert_eq!(decide(&turn_report(&turn, "새 신청을 확인해 주세요."),
            Some(&prior), None, false), Decision::Sent);
        // A wake without the state marker still shares the lead's open-items topic, so its
        // repeat is compared with the last delivered reminder instead of passing as new.
        turn.text = "[LANE_EVENT]\n- kibitzer: {\"wake\":true}".into();
        let unmarked = turn_report(&turn, "서류를 제출해 주세요.");
        assert_eq!((unmarked.source.as_str(), unmarked.topic_key.as_str()),
            ("kibitzer", prior.topic_key.as_str()));
    }

    #[tokio::test]
    async fn urgent_lane_keeps_source_mute_without_a_prior_topic() {
        use crate::report_gate::{queue, Decision};
        let pool = crate::storage::memory().await.unwrap();
        crate::inbox::ingest(&pool, "mail:1", "mail", "1", "결제가 필요합니다.", "2026-10-30T00:00:00Z").await.unwrap();
        sqlx::query("UPDATE inbox_items SET author='billing@example.test' WHERE id='mail:1'")
            .execute(&pool).await.unwrap();
        let turn = SessionTurn {
            key: SessionKey::new("slack", None::<String>, "D1", None::<String>, "U1"),
            reply_channel: Some("D1".into()), reply_thread: None, status_ts: None,
            text: "[LANE_EVENT]\n- inbox_urgent: inbox:mail:1 due 2026-10-30".into(), background: true,
        };
        let scope = crate::report_rules::scope_from_alert("mail", Some("billing@example.test"), None, None);
        crate::report_rules::record(&pool, &scope, crate::report_rules::Effect::Mute, "stop", None).await.unwrap();
        let report = delivery_report(&pool, &turn, "결제를 확인해 주세요.", true).await.unwrap();
        assert_eq!(report.source, "mail");
        assert_eq!(report.sender.as_deref(), Some("billing@example.test"));
        assert_eq!(queue(&pool, "owner", &report, false).await.unwrap().0, Decision::Muted);
    }

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

    #[test]
    fn interim_narration_is_coalesced_and_foreign_narration_is_dropped() {
        use crate::report_gate::Salience;
        // Transcript 37, 41: English working narration never reaches a Korean owner.
        for text in [
            "I'll wait for the Wishket Google login attempt result.",
            "Now add `ensureLogin` to wishket.ts plus the price fix.",
        ] {
            assert_eq!(interim_route(text, Some(Salience::Action)), InterimRoute::Drop, "{text}");
        }
        // Code and file names inside Korean prose do not make it foreign.
        assert!(!is_foreign_interim("`wishket.ts`에 `ensureLogin`을 추가했습니다."));
        // Transcript 50-60: progress notes update the status message, never post.
        for salience in [None, Some(Salience::Internal), Some(Salience::Fyi), Some(Salience::Result)] {
            assert_eq!(interim_route("자료를 찾아보겠습니다.", salience), InterimRoute::Coalesce);
        }
        // A question or decision for the owner keeps its own message.
        assert_eq!(interim_route("어느 파일인지 알려 주세요.", Some(Salience::Action)), InterimRoute::Post);
        assert_eq!(interim_route("1번과 2번 중 골라 주세요.", Some(Salience::Decision)), InterimRoute::Post);
        crate::locale::with(crate::locale::OwnerLocale::english_utc(), || {
            assert_eq!(interim_route("로그인 결과를 기다리겠습니다.", None), InterimRoute::Drop);
            assert_eq!(
                interim_route("Which file do you mean?", Some(Salience::Action)),
                InterimRoute::Post
            );
        });
    }

    #[test]
    fn a_turn_whose_last_words_were_folded_into_the_status_answers_with_them() {
        assert_eq!(final_or_held(Some("답".into()), Some("진행".into()), false).as_deref(), Some("답"));
        assert_eq!(final_or_held(None, Some("결과입니다.".into()), false).as_deref(), Some("결과입니다."));
        assert_eq!(final_or_held(None, Some("진행".into()), true), None);
    }

    #[test]
    fn every_turn_carries_the_owner_clock_and_language() {
        // 08:04 in Seoul is still the same day as a 09:30 deadline there.
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-31T23:04:00Z").unwrap().with_timezone(&chrono::Utc);
        let line = owner_clock_line(at);
        assert!(line.contains("Sunday 2026-11-01 08:04 KST (Asia/Seoul)"), "{line}");
        assert!(with_turn_rules(None).contains("Owner language: Korean."));
        assert!(!TURN_RULES.contains("natural Korean"));
        crate::locale::with(crate::locale::OwnerLocale::english_utc(), || {
            assert!(owner_clock_line(at).contains("Saturday 2026-10-31 23:04 UTC"), "{}", owner_clock_line(at));
            assert!(with_turn_rules(Some("persona")).contains("Owner language: English."));
        });
    }

    #[tokio::test]
    async fn a_lead_turn_sees_owner_answers_given_in_its_threads() {
        let pool = crate::storage::memory().await.unwrap();
        let lead = SessionKey::new("slack", None::<String>, "D1", None::<String>, "UOWNER");
        let thread = SessionKey::new("slack", None::<String>, "D1", Some("77.1"), "UOWNER");
        let other = SessionKey::new("slack", None::<String>, "D2", Some("5.1"), "UOWNER");
        let now = chrono::Utc::now();
        assert_eq!(thread_replies_context(&pool, &lead, now).await, None);
        super::super::transcript::record(&pool, &lead.storage_key(), "assistant", "어떻게 할지 정해 주세요. 1. 빼기 2. 두기").await.unwrap();
        super::super::transcript::record(&pool, &thread.storage_key(), "user", "1").await.unwrap();
        super::super::transcript::record(&pool, &thread.storage_key(), "assistant", "이번 발행에서 뺐습니다.").await.unwrap();
        super::super::transcript::record(&pool, &other.storage_key(), "user", "다른 대화").await.unwrap();
        let context = thread_replies_context(&pool, &lead, now).await.unwrap();
        assert!(context.contains("- thread 77.1 owner: 1\n- thread 77.1 bot: 이번 발행에서 뺐습니다."), "{context}");
        assert!(!context.contains("다른 대화"), "{context}");
        // The context stays visible to later lead turns, not only the next one.
        super::super::transcript::record(&pool, &lead.storage_key(), "assistant", "크론은 정상입니다.").await.unwrap();
        assert!(thread_replies_context(&pool, &lead, now).await.is_some());
        // A thread session gets none, and a day later the answers have aged out.
        assert_eq!(thread_replies_context(&pool, &thread, now).await, None);
        assert_eq!(thread_replies_context(&pool, &lead, now + chrono::Duration::hours(25)).await, None);
    }

    #[tokio::test]
    async fn a_turn_that_failed_twice_is_resumed_by_the_hub_once() {
        let pool = crate::storage::memory().await.unwrap();
        let turn = owner_turn();
        assert!(schedule_resume(&pool, &turn, "mhc turn failed: boom").await);
        let (lead, kind, body, release): (String, String, String, String) = sqlx::query_as(
            "SELECT lead_session_key, kind, body, release_at FROM lane_events",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(lead, turn.key.storage_key());
        assert_eq!(kind, "owner_reply_failed");
        assert!(body.starts_with(RESUME_MARKER) && body.ends_with("배포가 왜 실패했는지 알려줘"), "{body}");
        let release = chrono::DateTime::parse_from_rfc3339(&release).unwrap().with_timezone(&chrono::Utc);
        assert!(release > chrono::Utc::now() + chrono::Duration::minutes(RESUME_AFTER_MINUTES - 1));
        // The resumed turn failing again, a declined-approvals stop and a dashboard turn are
        // not resumed again.
        let resumed = SessionTurn { text: format!("[LANE_EVENT]\n- owner_reply_failed: {body}"), ..owner_turn() };
        assert!(!schedule_resume(&pool, &resumed, "boom").await);
        assert!(!schedule_resume(&pool, &turn, omo_client::DENIAL_LIMIT).await);
        let web = SessionTurn { reply_channel: None, ..owner_turn() };
        assert!(!schedule_resume(&pool, &web, "boom").await);
        let queued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lane_events").fetch_one(&pool).await.unwrap();
        assert_eq!(queued, 1);
    }

    #[test]
    fn without_a_configured_owner_nobody_is_admitted() {
        let mut c = config();
        c.allowed_users.clear();
        let dm = payload(json!({"type":"message","channel_type":"im","user":"UOWNER","channel":"D1","ts":"1.6","text":"yo"}));
        assert!(admit(&c, &dm, "UBOT", false).is_none());
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
