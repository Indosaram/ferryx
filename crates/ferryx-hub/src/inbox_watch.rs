//! Inbox watchers. The same events omo raises: startup, `imsg watch`, `zele mail watch`,
//! a Kakao timer (first tick at 120s, then every 120s), and a GitHub tick every 300s.
//! Each event collects only its source, advances that source's cursor, then classifies,
//! writes a digest, and opens proposals for bot items.
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::inbox::{classify_pending, digest, Class};
use crate::inbox_sources::{
    commit_source_items, parse_imessage, parse_kakao_messages, parse_mail_at, SourceItem,
};
use crate::judge::Judge;
use crate::storage::now_rfc3339;

pub const KAKAO_FIRST: Duration = Duration::from_secs(120);
pub const KAKAO_EVERY: Duration = Duration::from_secs(120);
pub const GITHUB_EVERY: Duration = Duration::from_secs(300);
pub const MAIL_ACCOUNT_DEFAULT: &str = "freedomzero91@gmail.com";
pub const MAIL_FILTER: &str = "is:unread -category:promotions -category:social -category:updates";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InboxWatcherEvent {
    Startup,
    GatewayMessage,
    IMessage { rowid: Option<i64> },
    Mail,
    Periodic,
    Github,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Mail,
    Kakao,
    Discord,
    Imessage,
    Slack,
    Github,
}

impl SourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mail => "mail",
            Self::Kakao => "kakao",
            Self::Discord => "discord",
            Self::Imessage => "imessage",
            Self::Slack => "slack",
            Self::Github => "github",
        }
    }
}

pub fn filter_sources(event: &InboxWatcherEvent) -> &'static [SourceKind] {
    match event {
        InboxWatcherEvent::Startup => &[
            SourceKind::Mail,
            SourceKind::Kakao,
            SourceKind::Imessage,
            SourceKind::Github,
            SourceKind::Discord,
            SourceKind::Slack,
        ],
        InboxWatcherEvent::GatewayMessage => &[SourceKind::Discord, SourceKind::Slack],
        InboxWatcherEvent::IMessage { .. } => &[SourceKind::Imessage],
        InboxWatcherEvent::Mail => &[SourceKind::Mail],
        InboxWatcherEvent::Periodic => &[SourceKind::Kakao],
        InboxWatcherEvent::Github => &[SourceKind::Github],
    }
}

pub fn imsg_watch_args(since_rowid: Option<i64>) -> Vec<String> {
    let mut args = vec!["watch".into(), "--json".into()];
    if let Some(rowid) = since_rowid {
        args.push("--since-rowid".into());
        args.push(rowid.to_string());
    }
    args
}

pub fn imsg_chats_args() -> Vec<String> {
    vec![
        "chats".into(),
        "--limit".into(),
        "50".into(),
        "--json".into(),
    ]
}

pub fn parse_imsg_watch_line(line: &str) -> Option<i64> {
    let parsed: Value = serde_json::from_str(line).ok()?;
    parsed
        .get("id")
        .or_else(|| parsed.get("rowid"))
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
}

pub fn is_mail_event_line(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && !trimmed.starts_with('#')
}

pub fn mail_account_from(raw: Option<&str>) -> String {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(MAIL_ACCOUNT_DEFAULT)
        .to_owned()
}

pub fn mail_list_args(account: &str, cursor: Option<&str>) -> Vec<String> {
    let mut filter = MAIL_FILTER.to_owned();
    if let Some(timestamp) = cursor.and_then(|value| value.parse::<i64>().ok()) {
        filter = format!("{MAIL_FILTER} after:{timestamp}");
    }
    vec![
        "mail".into(),
        "list".into(),
        "--folder".into(),
        "inbox".into(),
        "--account".into(),
        account.into(),
        "--filter".into(),
        filter,
        "--limit".into(),
        "100".into(),
    ]
}

pub fn github_enabled_from(raw: Option<&str>) -> bool {
    matches!(
        raw.map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

pub async fn latest_imessage_rowid(pool: &SqlitePool) -> Result<Option<i64>> {
    let refs: Vec<String> = sqlx::query_scalar(
        "SELECT source_ref FROM inbox_items WHERE source = 'imessage' AND source_ref GLOB '[0-9]*'",
    )
    .fetch_all(pool)
    .await?;
    Ok(refs
        .iter()
        .filter_map(|value| value.parse::<i64>().ok())
        .max())
}

pub async fn cursor_of(pool: &SqlitePool, source: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT cursor FROM inbox_cursors WHERE source = ?")
            .bind(source)
            .fetch_optional(pool)
            .await?,
    )
}

async fn store_cursor(pool: &SqlitePool, source: &str, cursor: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO inbox_cursors(source, cursor, updated_at) VALUES(?,?,?)
         ON CONFLICT(source) DO UPDATE SET cursor = excluded.cursor, updated_at = excluded.updated_at",
    )
    .bind(source)
    .bind(cursor)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Parses one source payload with that source's cursor, commits new rows, and
/// classifies anything that still has no decision.
pub async fn ingest_parsed(
    pool: &SqlitePool,
    source: SourceKind,
    items: Vec<SourceItem>,
    cursor: &str,
    judge: &Judge,
    now: DateTime<Utc>,
) -> Result<String> {
    commit_source_items(pool, &items).await?;
    if !cursor.is_empty() {
        store_cursor(pool, source.as_str(), cursor).await?;
    }
    classify_pending(pool, judge, now, |_id, class| async move {
        Ok((class == Class::Bot).then(|| "handle inbox item".to_owned()))
    })
    .await?;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT COALESCE(inbox_items.title, ''), inbox_decisions.class FROM inbox_items
         JOIN inbox_decisions ON inbox_decisions.item_id = inbox_items.id
         ORDER BY inbox_items.received_at",
    )
    .fetch_all(pool)
    .await?;
    let titled: Vec<(String, Class)> = rows
        .into_iter()
        .filter_map(|(title, class)| {
            let class = match class.as_str() {
                "owner" => Class::Owner,
                "bot" => Class::Bot,
                "fyi" => Class::Fyi,
                "ignore" => Class::Ignore,
                _ => return None,
            };
            Some((
                if title.is_empty() {
                    source.as_str().to_owned()
                } else {
                    title
                },
                class,
            ))
        })
        .collect();
    let body = digest(&titled);
    sqlx::query("INSERT INTO inbox_digests(body, created_at) VALUES(?,?)")
        .bind(&body)
        .bind(now_rfc3339())
        .execute(pool)
        .await?;
    Ok(body)
}

pub fn imessage_items(
    chats: &Value,
    histories: &[(String, Value)],
    cursor: Option<&str>,
) -> Result<(Vec<SourceItem>, String)> {
    parse_imessage(chats, histories, cursor)
}

pub fn mail_items(
    value: &Value,
    cursor: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(Vec<SourceItem>, String)> {
    parse_mail_at(value, cursor, now)
}

pub fn kakao_items(
    room_id: &str,
    room_title: Option<&str>,
    room_type: &str,
    page: &Value,
    floor: &str,
) -> Result<(Vec<SourceItem>, Option<String>, Option<String>)> {
    parse_kakao_messages(room_id, room_title, room_type, page, floor)
}

/// Drains watcher events until the channel closes. Timers and child processes
/// send on `events`; this task is what the daemon aborts on `ferryx hub stop`.
pub async fn serve<F, Fut>(
    pool: SqlitePool,
    mut events: tokio::sync::mpsc::Receiver<InboxWatcherEvent>,
    judge: Judge,
    mut fetch: F,
) -> Result<()>
where
    F: FnMut(SourceKind, Option<String>) -> Fut + Send,
    Fut: std::future::Future<Output = Result<Option<(Vec<SourceItem>, String)>>> + Send,
{
    while let Some(event) = events.recv().await {
        let now = Utc::now();
        for source in filter_sources(&event) {
            let cursor = cursor_of(&pool, source.as_str()).await?;
            match fetch(*source, cursor).await {
                Ok(Some((items, next))) => {
                    // Items are committed before triage. A judge timeout or a missing judge
                    // leaves them pending for the next pass; it must not end the inbox loop.
                    if let Err(error) =
                        ingest_parsed(&pool, *source, items, &next, &judge, now).await
                    {
                        tracing::warn!(source = source.as_str(), %error, "inbox items stored but not triaged");
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(source = source.as_str(), %error, "inbox source collection failed");
                }
            }
        }
    }
    Ok(())
}

pub fn watcher_channel() -> (
    tokio::sync::mpsc::Sender<InboxWatcherEvent>,
    tokio::sync::mpsc::Receiver<InboxWatcherEvent>,
) {
    tokio::sync::mpsc::channel(64)
}

pub fn on_gateway_message(sender: &tokio::sync::mpsc::Sender<InboxWatcherEvent>) -> bool {
    match sender.try_send(InboxWatcherEvent::GatewayMessage) {
        Ok(()) | Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => true,
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => false,
    }
}

/// Kakao's first collection is 120s after start, then every 120s. GitHub is every 300s.
/// Both skip a missed tick instead of bursting.
pub async fn run_source_timers(
    sender: tokio::sync::mpsc::Sender<InboxWatcherEvent>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut kakao =
        tokio::time::interval_at(tokio::time::Instant::now() + KAKAO_FIRST, KAKAO_EVERY);
    kakao.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut github =
        tokio::time::interval_at(tokio::time::Instant::now() + GITHUB_EVERY, GITHUB_EVERY);
    github.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if *shutdown.borrow() {
            break;
        }
        tokio::select! {
            _ = kakao.tick() => {
                if sender.send(InboxWatcherEvent::Periodic).await.is_err() {
                    break;
                }
            }
            _ = github.tick() => {
                if sender.send(InboxWatcherEvent::Github).await.is_err() {
                    break;
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }
}

pub fn imsg_command(since_rowid: Option<i64>) -> (&'static str, Vec<String>) {
    ("imsg", imsg_watch_args(since_rowid))
}

pub fn mail_watch_command(account: &str) -> Vec<String> {
    vec![
        "mail".into(),
        "watch".into(),
        "--interval".into(),
        "60".into(),
        "--folder".into(),
        "inbox".into(),
        "--account".into(),
        account.into(),
        "--filter".into(),
        MAIL_FILTER.into(),
    ]
}

pub fn imsg_history_args(chat_id: &str) -> Vec<String> {
    vec![
        "history".into(),
        "--chat-id".into(),
        chat_id.into(),
        "--limit".into(),
        "50".into(),
        "--json".into(),
    ]
}

pub const BACKOFF_INITIAL: Duration = Duration::from_secs(5);
pub const BACKOFF_MAX: Duration = Duration::from_secs(300);
pub const HEALTHY_RUN: Duration = Duration::from_secs(60);
pub const STDERR_TAIL_MAX: usize = 2048;
const GITHUB_PR_QUERY: &str = "query{search(query:\"is:pr author:@me archived:false sort:updated-desc\",type:ISSUE,first:50){nodes{... on PullRequest{url title number updatedAt state repository{nameWithOwner} commits(last:1){nodes{commit{oid statusCheckRollup{state}}}}}}}}";

static GATEWAY_EVENTS: std::sync::OnceLock<tokio::sync::mpsc::Sender<InboxWatcherEvent>> =
    std::sync::OnceLock::new();

pub fn inbox_enabled() -> bool {
    crate::hub_env::var("OMON_INBOX_ENABLED")
        .map(|value| !matches!(value.to_lowercase().as_str(), "0" | "false" | "no"))
        .unwrap_or(true)
}

pub fn notify_gateway() {
    if let Some(sender) = GATEWAY_EVENTS.get() {
        let _ = sender.try_send(InboxWatcherEvent::GatewayMessage);
    }
}

pub async fn note_gateway(pool: &SqlitePool, inbound: &crate::chat::router::Inbound) -> Result<()> {
    let id = inbound
        .event_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    sqlx::query(
        "INSERT OR IGNORE INTO messages(id, session_key, role, content, created_at) VALUES(?,?, 'user', ?, ?)",
    )
    .bind(&id)
    .bind(format!("{}:gateway", inbound.platform))
    .bind(&inbound.text)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    notify_gateway();
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatcherRun {
    Completed,
    Failed,
    AuthFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
    pub delay: Duration,
    pub auth_failures: u32,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            delay: BACKOFF_INITIAL,
            auth_failures: 0,
        }
    }
}

impl Backoff {
    pub fn step(&mut self, run: WatcherRun, ran_for: Duration) -> Duration {
        match run {
            WatcherRun::Completed => {
                self.auth_failures = 0;
                self.delay = BACKOFF_INITIAL;
                self.delay
            }
            WatcherRun::Failed => {
                self.auth_failures = 0;
                if ran_for >= HEALTHY_RUN {
                    self.delay = BACKOFF_INITIAL;
                }
                let wait = self.delay;
                self.delay = (self.delay * 2).min(BACKOFF_MAX);
                wait
            }
            WatcherRun::AuthFailed => {
                self.auth_failures = self.auth_failures.saturating_add(1);
                let wait = self.delay;
                self.delay = (self.delay * 2).min(BACKOFF_MAX);
                wait
            }
        }
    }
}

pub fn is_auth_failure(output_tail: &str) -> bool {
    let lower = output_tail.to_ascii_lowercase();
    [
        "auth_expired",
        "invalid_grant",
        "unauthorized",
        "not logged in",
        "zele login",
        "invalid credentials",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatcherExit {
    Stop,
    Failed { stderr_tail: String },
    Completed,
}

pub async fn run_json(program: &str, args: &[String]) -> Result<Value> {
    let output = crate::child_env::command(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|error| HubError::message(format!("{program} could not start: {error}")))?;
    if !output.status.success() {
        let stderr: String = String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(500)
            .collect();
        return Err(HubError::message(format!(
            "{program} exited {}: {stderr}",
            output.status
        )));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| HubError::message(format!("{program} returned invalid JSON: {error}")))
}

pub async fn fetch_imessage_with(
    program: &str,
    cursor: Option<&str>,
) -> Result<(Vec<SourceItem>, String)> {
    let chats = run_json(program, &imsg_chats_args()).await?;
    let since = cursor
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    let rows = match &chats {
        Value::Array(values) => values.clone(),
        Value::Null => Vec::new(),
        other => vec![other.clone()],
    };
    let mut kept = Vec::new();
    let mut histories = Vec::new();
    for chat in rows {
        let id = match chat.get("id") {
            Some(Value::String(text)) if !text.is_empty() => text.clone(),
            Some(Value::Number(number)) => number.to_string(),
            _ => continue,
        };
        let at = chat
            .get("last_message_at")
            .and_then(Value::as_str)
            .unwrap_or("");
        let chat_time = DateTime::parse_from_rfc3339(at)
            .ok()
            .map(|value| value.with_timezone(&Utc));
        if since.is_some_and(|cursor| chat_time.is_some_and(|at| at <= cursor)) {
            continue;
        }
        histories.push((
            id.clone(),
            run_json(program, &imsg_history_args(&id)).await?,
        ));
        kept.push(chat);
    }
    if kept.is_empty() {
        return Ok((Vec::new(), cursor.unwrap_or("").to_owned()));
    }
    imessage_items(&Value::Array(kept), &histories, cursor)
}

pub async fn fetch_mail_with(
    program: &str,
    cursor: Option<&str>,
    account: &str,
) -> Result<(Vec<SourceItem>, String)> {
    let value = run_json(program, &mail_list_args(account, cursor)).await?;
    mail_items(&value, cursor, Utc::now())
}

pub async fn fetch_github_with(
    program: &str,
    cursor: Option<&str>,
) -> Result<(Vec<SourceItem>, String)> {
    let notifications = run_json(
        program,
        &[
            "api".into(),
            "notifications?participating=true&per_page=50".into(),
        ],
    )
    .await?;
    let query = format!("query={GITHUB_PR_QUERY}");
    let prs = run_json(
        program,
        &["api".into(), "graphql".into(), "-f".into(), query],
    )
    .await?;
    let poll = crate::inbox_sources::github_diff(cursor, &notifications, &prs)?;
    Ok((poll.items, poll.cursor))
}

pub async fn fetch_kind(
    pool: &SqlitePool,
    kind: SourceKind,
    cursor: Option<&str>,
) -> Result<Option<(Vec<SourceItem>, String)>> {
    match kind {
        SourceKind::Imessage => fetch_imessage_with("imsg", cursor).await.map(Some),
        SourceKind::Mail => {
            let account =
                mail_account_from(crate::hub_env::var("OMON_MAIL_ACCOUNT").ok().as_deref());
            fetch_mail_with("zele", cursor, &account).await.map(Some)
        }
        SourceKind::Kakao => fetch_kakao(cursor).await.map(Some),
        SourceKind::Github => {
            if !github_enabled_from(
                crate::hub_env::var("OMON_GITHUB_SOURCE_ENABLED")
                    .ok()
                    .as_deref(),
            ) {
                return Ok(None);
            }
            fetch_github_with("gh", cursor).await.map(Some)
        }
        SourceKind::Discord | SourceKind::Slack => fetch_gateway(pool, kind, cursor).await,
    }
}

async fn fetch_gateway(
    pool: &SqlitePool,
    kind: SourceKind,
    cursor: Option<&str>,
) -> Result<Option<(Vec<SourceItem>, String)>> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, content, created_at FROM messages
         WHERE session_key = ? AND (? IS NULL OR created_at > ?)
         ORDER BY created_at",
    )
    .bind(format!("{}:gateway", kind.as_str()))
    .bind(cursor)
    .bind(cursor)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut newest = cursor.unwrap_or("").to_owned();
    let mut items = Vec::new();
    for (id, content, created_at) in rows {
        let received = DateTime::parse_from_rfc3339(&created_at)
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        if created_at > newest {
            newest = created_at;
        }
        items.push(SourceItem {
            source: kind.as_str(),
            source_ref: id,
            snippet: content,
            received_at: received,
            thread_ref: None,
            author: None,
            title: None,
            link: None,
        });
    }
    Ok(Some((items, newest)))
}

pub async fn fetch_kakao(cursor: Option<&str>) -> Result<(Vec<SourceItem>, String)> {
    let base = crate::hub_env::var("OMON_PIRATETALK_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://indo.taila0cef2.ts.net".to_owned());
    let rooms = crate::hub_env::var("OMON_INBOX_KAKAO_ROOMS").ok();
    fetch_kakao_at(&base, rooms.as_deref(), cursor).await
}

pub async fn fetch_kakao_at(
    base: &str,
    rooms: Option<&str>,
    cursor: Option<&str>,
) -> Result<(Vec<SourceItem>, String)> {
    #[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
    {
        let client = reqwest::Client::new();
        let configured: Option<std::collections::HashSet<String>> = rooms.map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|room| !room.is_empty())
                .map(str::to_owned)
                .collect()
        });
        let rooms_value: Value = client
            .get(format!("{base}/api/rooms"))
            .send()
            .await
            .map_err(|error| HubError::message(format!("PirateTalk rooms: {error}")))?
            .error_for_status()
            .map_err(|error| HubError::message(format!("PirateTalk rooms: {error}")))?
            .json()
            .await
            .map_err(|error| HubError::message(format!("PirateTalk rooms JSON: {error}")))?;
        let rooms = rooms_value.as_array().cloned().unwrap_or_default();
        let mut cursor_map = crate::inbox_sources::parse_kakao_cursor(cursor);
        let mut items = Vec::new();
        for room in rooms {
            let id = room.get("chatId").and_then(Value::as_str).unwrap_or("");
            if id.is_empty() {
                continue;
            }
            if configured.as_ref().is_some_and(|set| !set.contains(id)) {
                continue;
            }
            let title = room.get("title").and_then(Value::as_str);
            let room_type = room.get("type").and_then(Value::as_str).unwrap_or("");
            let floor = cursor_map.get(id).cloned().unwrap_or_default();
            let mut before = String::new();
            let mut newest = floor.clone();
            loop {
                let url = if before.is_empty() {
                    format!("{base}/api/rooms/{}/messages", path_escape(id))
                } else {
                    format!(
                        "{base}/api/rooms/{}/messages?before_id={}",
                        path_escape(id),
                        path_escape(&before)
                    )
                };
                let page: Value = client
                    .get(url)
                    .send()
                    .await
                    .map_err(|error| HubError::message(format!("PirateTalk messages: {error}")))?
                    .error_for_status()
                    .map_err(|error| HubError::message(format!("PirateTalk messages: {error}")))?
                    .json()
                    .await
                    .map_err(|error| {
                        HubError::message(format!("PirateTalk messages JSON: {error}"))
                    })?;
                let (page_items, oldest, top) = kakao_items(id, title, room_type, &page, &floor)?;
                if let Some(top) = top {
                    if top > newest {
                        newest = top;
                    }
                }
                items.extend(page_items);
                let Some(oldest) = oldest else { break };
                if !before.is_empty() && oldest >= before {
                    return Err(HubError::message(
                        "PirateTalk paging cursor did not move backwards",
                    ));
                }
                before = oldest;
            }
            if !newest.is_empty() {
                cursor_map.insert(id.to_owned(), newest);
            }
        }
        let next = serde_json::to_string(&cursor_map).unwrap_or_else(|_| "{}".into());
        Ok((items, next))
    }
    #[cfg(not(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice")))]
    {
        let _ = (base, rooms, cursor);
        Err(HubError::message(
            "kakao client needs hub-slack, hub-discord, or hub-voice",
        ))
    }
}

#[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
fn path_escape(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Starts the collectors the daemon aborts on `ferryx hub stop`: startup collect,
/// `imsg watch`, `zele mail watch`, and the Kakao and GitHub timers.
pub async fn spawn(pool: SqlitePool) -> Result<()> {
    if !inbox_enabled() {
        return Ok(());
    }
    let (events, receiver) = watcher_channel();
    let _ = GATEWAY_EVENTS.set(events.clone());
    let _ = events.try_send(InboxWatcherEvent::Startup);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let judge = Judge::from_config(crate::judge::typesafe_from_env());
    let serve_pool = pool.clone();
    let fetch_pool = pool.clone();
    let serve_task = tokio::spawn(async move {
        let _ = serve(serve_pool, receiver, judge, move |kind, cursor| {
            let pool = fetch_pool.clone();
            async move { fetch_kind(&pool, kind, cursor.as_deref()).await }
        })
        .await;
    });
    let timer_events = events.clone();
    let timer_shutdown = shutdown_rx.clone();
    let timers = tokio::spawn(async move { run_source_timers(timer_events, timer_shutdown).await });
    let imsg_pool = pool.clone();
    let imsg_events = events.clone();
    let imsg_shutdown = shutdown_rx.clone();
    let imsg = tokio::spawn(async move {
        let health_pool = imsg_pool.clone();
        supervise_command("imessage", &health_pool, imsg_shutdown, move || {
            let pool = imsg_pool.clone();
            let events = imsg_events.clone();
            async move { run_imsg_once(&pool, &events).await }
        })
        .await;
    });
    let mail_pool = pool.clone();
    let mail_events = events;
    let mail = tokio::spawn(async move {
        supervise_command("mail", &mail_pool, shutdown_rx, move || {
            let events = mail_events.clone();
            async move { run_mail_once(&events).await }
        })
        .await;
    });
    let _ = shutdown_tx;
    let _ = tokio::join!(serve_task, timers, imsg, mail);
    Ok(())
}

/// How long a watcher process must stay up before its health reads `running`.
pub const WATCHER_RUNNING_AFTER: std::time::Duration = std::time::Duration::from_secs(3);

/// How often a running watcher refreshes its `running` health row.
pub const WATCHER_HEARTBEAT: std::time::Duration = std::time::Duration::from_secs(60);

async fn write_watcher_health(pool: &SqlitePool, source: &str, status: &str, detail: Option<&str>) {
    let _ = sqlx::query(
        "INSERT INTO inbox_watcher_health(source, status, detail, updated_at) VALUES(?,?,?,?)
         ON CONFLICT(source) DO UPDATE SET status = excluded.status, detail = excluded.detail, updated_at = excluded.updated_at",
    )
    .bind(source)
    .bind(status)
    .bind(detail)
    .bind(crate::storage::now_rfc3339())
    .execute(pool)
    .await;
}

async fn supervise_command<F, Fut>(
    source: &str,
    pool: &SqlitePool,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    mut run_once: F,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = WatcherExit>,
{
    let mut backoff = Backoff::default();
    loop {
        if *shutdown.borrow() {
            return;
        }
        let started = std::time::Instant::now();
        let run = run_once();
        tokio::pin!(run);
        let running_mark = tokio::time::sleep(WATCHER_RUNNING_AFTER);
        tokio::pin!(running_mark);
        let outcome = loop {
            tokio::select! {
                exit = &mut run => break exit,
                // A watcher that stays up past the mark started fine: replace a stale failure.
                // The mark repeats every heartbeat so the row stays current while it runs.
                _ = &mut running_mark => {
                    write_watcher_health(pool, source, "running", None).await;
                    running_mark
                        .as_mut()
                        .reset(tokio::time::Instant::now() + WATCHER_HEARTBEAT);
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return;
                    }
                }
            }
        };
        let (run, tail) = match outcome {
            WatcherExit::Stop => return,
            WatcherExit::Completed => (WatcherRun::Completed, String::new()),
            WatcherExit::Failed { stderr_tail } => {
                let run = if is_auth_failure(&stderr_tail) {
                    WatcherRun::AuthFailed
                } else {
                    WatcherRun::Failed
                };
                (run, stderr_tail)
            }
        };
        let wait = backoff.step(run, started.elapsed());
        let status = match run {
            WatcherRun::Completed => "ok",
            WatcherRun::AuthFailed => "auth_failed",
            WatcherRun::Failed => "failed",
        };
        let detail = if tail.is_empty() {
            None
        } else {
            Some(tail.as_str())
        };
        write_watcher_health(pool, source, status, detail).await;
        if !tail.is_empty() {
            tracing::warn!(%tail, "inbox watcher exited");
        }
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
        }
    }
}

async fn run_imsg_once(
    pool: &SqlitePool,
    sender: &tokio::sync::mpsc::Sender<InboxWatcherEvent>,
) -> WatcherExit {
    let since = latest_imessage_rowid(pool).await.ok().flatten();
    let (program, args) = imsg_command(since);
    run_lines(
        program,
        &args,
        |line| {
            parse_imsg_watch_line(line)
                .map(|rowid| InboxWatcherEvent::IMessage { rowid: Some(rowid) })
        },
        sender,
    )
    .await
}

async fn run_mail_once(sender: &tokio::sync::mpsc::Sender<InboxWatcherEvent>) -> WatcherExit {
    let account = mail_account_from(crate::hub_env::var("OMON_MAIL_ACCOUNT").ok().as_deref());
    let args = mail_watch_command(&account);
    run_lines(
        "zele",
        &args,
        |line| is_mail_event_line(line).then_some(InboxWatcherEvent::Mail),
        sender,
    )
    .await
}

async fn run_lines<F>(
    program: &str,
    args: &[String],
    mut classify: F,
    sender: &tokio::sync::mpsc::Sender<InboxWatcherEvent>,
) -> WatcherExit
where
    F: FnMut(&str) -> Option<InboxWatcherEvent>,
{
    let mut child = match crate::child_env::command(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return WatcherExit::Failed {
                stderr_tail: error.to_string(),
            }
        }
    };
    let stderr = child.stderr.take();
    let stderr_task = tokio::spawn(async move { tail_text(stderr).await });
    let stdout = child.stdout.take();
    let mut saw_event = false;
    if let Some(stdout) = stdout {
        use tokio::io::AsyncBufReadExt;
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(event) = classify(&line) {
                saw_event = true;
                if sender.send(event).await.is_err() {
                    let _ = child.kill().await;
                    return WatcherExit::Stop;
                }
            }
        }
    }
    let status = child.wait().await;
    let stderr_tail = tokio::time::timeout(Duration::from_millis(500), stderr_task)
        .await
        .ok()
        .and_then(|result| result.ok())
        .unwrap_or_default();
    let tail: String = stderr_tail
        .chars()
        .rev()
        .take(STDERR_TAIL_MAX)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    match status {
        Ok(status) if status.success() || saw_event => WatcherExit::Completed,
        _ => WatcherExit::Failed { stderr_tail: tail },
    }
}

async fn tail_text(stderr: Option<tokio::process::ChildStderr>) -> String {
    let Some(stderr) = stderr else {
        return String::new();
    };
    use tokio::io::AsyncBufReadExt;
    let mut lines = tokio::io::BufReader::new(stderr).lines();
    let mut out = String::new();
    while let Ok(Some(line)) = lines.next_line().await {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&line);
        if out.len() > STDERR_TAIL_MAX {
            let extra = out.len() - STDERR_TAIL_MAX;
            let mut end = extra;
            while end < out.len() && !out.is_char_boundary(end) {
                end += 1;
            }
            out.replace_range(..end, "");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_failing_judge_leaves_items_pending_and_the_loop_keeps_serving() {
        let pool = crate::storage::memory().await.unwrap();
        let (events, receiver) = watcher_channel();
        events.try_send(InboxWatcherEvent::Mail).unwrap();
        events
            .try_send(InboxWatcherEvent::IMessage { rowid: Some(1) })
            .unwrap();
        drop(events);
        let fetched = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = fetched.clone();
        serve(
            pool.clone(),
            receiver,
            Judge::forced("not-an-option"),
            move |kind, _cursor| {
                seen.lock().unwrap().push(kind);
                let source = match kind {
                    SourceKind::Mail => "mail",
                    _ => "imessage",
                };
                async move {
                    Ok(Some((
                        vec![SourceItem {
                            source,
                            source_ref: format!("{source}-1"),
                            snippet: "hello".into(),
                            received_at: Utc::now(),
                            thread_ref: None,
                            author: None,
                            title: None,
                            link: None,
                        }],
                        String::new(),
                    )))
                }
            },
        )
        .await
        .expect("a judge failure must not end serve");
        assert_eq!(
            *fetched.lock().unwrap(),
            [SourceKind::Mail, SourceKind::Imessage]
        );
        let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inbox_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, 2);
    }

    #[test]
    fn events_select_the_gateway_source_and_the_clis_match() {
        assert_eq!(
            filter_sources(&InboxWatcherEvent::GatewayMessage),
            &[SourceKind::Discord, SourceKind::Slack]
        );
        assert_eq!(
            filter_sources(&InboxWatcherEvent::Periodic),
            &[SourceKind::Kakao]
        );
        assert_eq!(
            filter_sources(&InboxWatcherEvent::Github),
            &[SourceKind::Github]
        );
        assert_eq!(
            imsg_watch_args(Some(9)),
            ["watch", "--json", "--since-rowid", "9"]
        );
        assert_eq!(parse_imsg_watch_line(r#"{"rowid":12}"#), Some(12));
        assert!(!is_mail_event_line("# Watching inbox every 60s"));
        assert!(is_mail_event_line("{\"id\":\"m1\"}"));
        assert_eq!(mail_account_from(None), MAIL_ACCOUNT_DEFAULT);
        assert_eq!(KAKAO_FIRST, Duration::from_secs(120));
        assert_eq!(GITHUB_EVERY, Duration::from_secs(300));
        assert!(github_enabled_from(Some("on")));
        assert!(!github_enabled_from(None));
        let args = mail_list_args("me@example.com", Some("10"));
        assert!(args.iter().any(|arg| arg.contains("after:10")));
    }

    #[tokio::test]
    async fn a_full_channel_does_not_block_a_gateway_message() {
        let (tx, _rx) = watcher_channel();
        for _ in 0..64 {
            tx.try_send(InboxWatcherEvent::Startup).unwrap();
        }
        assert!(on_gateway_message(&tx));
    }

    #[tokio::test]
    async fn an_imessage_event_commits_triages_and_writes_a_digest() {
        let pool = crate::storage::memory().await.unwrap();
        let now = DateTime::parse_from_rfc3339("2026-10-22T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let chats = serde_json::json!([{"id": 7, "last_message_at": "2026-10-22T02:00:00Z", "name": "Ada"}]);
        let history = serde_json::json!([{"id": "4", "text": "ping the owner", "created_at": "2026-10-22T02:00:00Z", "is_from_me": false}]);
        let (items, cursor) =
            imessage_items(&chats, &[("7".into(), history.clone())], None).unwrap();
        assert_eq!(items.len(), 1);
        let judge = Judge::forced("bot_can_handle");
        let digest = ingest_parsed(&pool, SourceKind::Imessage, items, &cursor, &judge, now)
            .await
            .unwrap();
        assert!(digest.contains("bot"));
        let proposals: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM action_proposals WHERE origin = 'inbox'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(proposals, 1);
        let stored: String =
            sqlx::query_scalar("SELECT cursor FROM inbox_cursors WHERE source = 'imessage'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, cursor);
        let again = ingest_parsed(
            &pool,
            SourceKind::Imessage,
            imessage_items(&chats, &[("7".into(), history)], None)
                .unwrap()
                .0,
            &cursor,
            &judge,
            now,
        )
        .await
        .unwrap();
        assert!(again.contains("bot"));
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inbox_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn watcher_backoff_doubles_until_the_cap_and_auth_does_not_reset() {
        let mut backoff = Backoff::default();
        assert_eq!(
            backoff.step(WatcherRun::Failed, Duration::from_secs(1)),
            Duration::from_secs(5)
        );
        assert_eq!(
            backoff.step(WatcherRun::Failed, Duration::from_secs(1)),
            Duration::from_secs(10)
        );
        assert!(is_auth_failure("zele login required"));
        assert!(!is_auth_failure("connection reset"));
        assert!(inbox_enabled());
    }

    #[tokio::test]
    async fn collection_runs_the_imsg_and_mail_commands_and_a_gateway_row() {
        let root = std::env::temp_dir().join(format!("ferryx-inbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("source");
        std::fs::write(
            &script,
            "#!/bin/sh\nif [ \"$1\" = chats ]; then printf '%s\\n' '[{\"id\":7,\"last_message_at\":\"2026-10-22T02:00:00Z\",\"name\":\"Ada\"}]'; elif [ \"$1\" = history ]; then printf '%s\\n' '[{\"id\":\"4\",\"text\":\"ping the owner\",\"created_at\":\"2026-10-22T02:00:00Z\",\"is_from_me\":false}]'; elif [ \"$1\" = mail ]; then printf '%s\\n' '{\"items\":[{\"id\":\"m1\",\"snippet\":\"hello\",\"date\":\"2026-10-22T01:00:00Z\"}]}'; elif [ \"$2\" = graphql ]; then printf '%s\\n' '{\"data\":{\"search\":{\"nodes\":[]}}}'; else printf '%s\\n' '[]'; fi\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }
        let program = script.to_string_lossy().into_owned();
        let (items, _) = fetch_imessage_with(&program, None).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].snippet, "ping the owner");
        let (mail, cursor) = fetch_mail_with(&program, None, "me@example.com")
            .await
            .unwrap();
        assert_eq!(mail.len(), 1);
        assert!(!cursor.is_empty());
        let (github, _) = fetch_github_with(&program, None).await.unwrap();
        assert!(github.is_empty());
        let pool = crate::storage::memory().await.unwrap();
        note_gateway(
            &pool,
            &crate::chat::router::Inbound {
                platform: "slack".into(),
                guild_id: None,
                channel_id: "C1".into(),
                thread_id: None,
                user_id: "U1".into(),
                text: "from slack".into(),
                parent_chat_id: None,
                event_id: Some("evt-1".into()),
            },
        )
        .await
        .unwrap();
        let stored = fetch_kind(&pool, SourceKind::Slack, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.0[0].snippet, "from slack");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
    #[tokio::test]
    async fn kakao_collects_a_room_page_and_keeps_the_newest_id() {
        let app = axum::Router::new()
            .route(
                "/api/rooms",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!([{
                        "chatId": "room-1",
                        "title": "Ada",
                        "type": "DirectChat",
                        "unreadCount": 1
                    }]))
                }),
            )
            .route(
                "/api/rooms/{id}/messages",
                axum::routing::get(
                    |query: axum::extract::Query<std::collections::HashMap<String, String>>| async move {
                        if query.contains_key("before_id") {
                            axum::Json(serde_json::json!([]))
                        } else {
                            axum::Json(serde_json::json!([{
                                "id": "m9",
                                "text": "kakao ping",
                                "time": 1_761_100_000
                            }]))
                        }
                    },
                ),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let (items, cursor) = fetch_kakao_at(&format!("http://{addr}"), None, None)
            .await
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].snippet, "kakao ping");
        assert!(cursor.contains("m9"));
    }
}
