//! Discord gateway dispatch parsing. The live connection is compiled only with `hub-discord`.
//! The bot token comes from `DISCORD_TOKEN` or `DISCORD_TOKEN_FILE`.
use serde_json::Value;

use super::approval::{parse_omon_approval, ApprovalClick};
use super::router::Inbound;
use crate::config::secret_from_env;
use crate::error::{HubError, Result};

pub const PLATFORM: &str = "discord";
pub const INTENT_GUILDS: u64 = 1;
pub const INTENT_GUILD_VOICE_STATES: u64 = 1 << 7;
pub const INTENT_GUILD_MESSAGES: u64 = 1 << 9;
pub const INTENT_DIRECT_MESSAGES: u64 = 1 << 12;
pub const INTENT_MESSAGE_CONTENT: u64 = 1 << 15;
pub const PAIRING_TTL_SECONDS: i64 = 3600;
pub const PAIRING_RATE_LIMIT_SECONDS: i64 = 600;
pub const PAIRING_MAX_ATTEMPTS: i64 = 5;
pub const PAIRING_LOCKOUT_SECONDS: i64 = 900;

/// `GUILDS | GUILD_MESSAGES | DIRECT_MESSAGES | MESSAGE_CONTENT`, plus voice when asked.
pub fn gateway_intents(voice_receive: bool) -> u64 {
    let base =
        INTENT_GUILDS | INTENT_GUILD_MESSAGES | INTENT_DIRECT_MESSAGES | INTENT_MESSAGE_CONTENT;
    if voice_receive {
        base | INTENT_GUILD_VOICE_STATES
    } else {
        base
    }
}

pub fn voice_receive_from_env() -> bool {
    matches!(
        std::env::var("DISCORD_VOICE_RECEIVE")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

pub fn heartbeat_millis(frame: &serde_json::Value) -> Option<u64> {
    if frame.get("op").and_then(serde_json::Value::as_i64) != Some(10) {
        return None;
    }
    frame
        .pointer("/d/heartbeat_interval")
        .and_then(serde_json::Value::as_u64)
}

pub fn heartbeat_frame(seq: Option<i64>) -> serde_json::Value {
    serde_json::json!({"op": 1, "d": seq})
}

pub fn identify_body(token: &str, intents: u64) -> serde_json::Value {
    serde_json::json!({
        "op": 2,
        "d": {
            "token": token,
            "intents": intents,
            "properties": {"os": "linux", "browser": "ferryx-hub", "device": "ferryx-hub"}
        }
    })
}

/// 2s, doubling, capped at 60s. Attempt 0 waits 2s.
pub fn reconnect_backoff(attempt: u32) -> std::time::Duration {
    let shift = attempt.min(5);
    std::time::Duration::from_secs((2u64 << shift).min(60))
}

/// Discord's channel message limit: 5 posts per 5 seconds.
pub struct ChannelRate {
    limit: usize,
    window: std::time::Duration,
    hits: std::collections::HashMap<String, std::collections::VecDeque<std::time::Instant>>,
}

impl ChannelRate {
    pub fn discord_messages() -> Self {
        Self {
            limit: 5,
            window: std::time::Duration::from_secs(5),
            hits: std::collections::HashMap::new(),
        }
    }

    pub fn reserve(&mut self, key: &str, now: std::time::Instant) -> std::time::Duration {
        let hits = self.hits.entry(key.to_owned()).or_default();
        let mut at = now;
        loop {
            let in_window = hits
                .iter()
                .filter(|hit| **hit <= at && at.saturating_duration_since(**hit) < self.window)
                .count();
            if in_window < self.limit {
                hits.push_back(at);
                return at.saturating_duration_since(now);
            }
            let oldest = hits
                .iter()
                .copied()
                .filter(|hit| *hit <= at)
                .min()
                .unwrap_or(at);
            let next = oldest + self.window;
            at = if next > at { next } else { at + self.window };
        }
    }
}

pub fn should_auto_create_thread(
    auto_thread_enabled: bool,
    is_guild_text: bool,
    is_explicit_mention: bool,
    is_free_channel: bool,
    is_reply: bool,
) -> bool {
    auto_thread_enabled && is_guild_text && is_explicit_mention && !is_free_channel && !is_reply
}

pub fn derive_auto_thread_name(content: &str, bot_user_id: &str) -> String {
    let mention = format!("<@{bot_user_id}>");
    let stripped = content.replace(&mention, " ");
    let collapsed = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        "Conversation".to_owned()
    } else if trimmed.chars().count() > 80 {
        let capped: String = trimmed.chars().take(77).collect();
        format!("{capped}...")
    } else {
        trimmed.to_owned()
    }
}

pub fn thread_start_body(name: &str) -> serde_json::Value {
    serde_json::json!({"name": name, "auto_archive_duration": 1440})
}

pub fn bot_token() -> Option<String> {
    secret_from_env("DISCORD_BOT_TOKEN").or_else(|| secret_from_env("DISCORD_TOKEN"))
}

pub fn should_auto_thread(enabled: bool, in_thread: bool) -> bool {
    enabled && !in_thread
}

pub fn auto_thread_name(text: &str) -> String {
    let name: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = name.chars().take(80).collect();
    if name.is_empty() {
        "thread".into()
    } else {
        name
    }
}

pub fn approval_components(options: &[(&str, &str)]) -> serde_json::Value {
    serde_json::json!({
        "type": 1,
        "components": options.iter().map(|(label, custom_id)| serde_json::json!({
            "type": 2,
            "style": 1,
            "label": label,
            "custom_id": custom_id
        })).collect::<Vec<_>>()
    })
}

pub async fn issue_pairing(
    pool: &sqlx::SqlitePool,
    platform: &str,
    user_id: &str,
    code: &str,
) -> crate::error::Result<()> {
    let now = chrono::Utc::now();
    let locked: Option<String> = sqlx::query_scalar(
        "SELECT locked_until FROM pairing_codes WHERE platform = ? AND user_id = ? AND locked_until IS NOT NULL ORDER BY locked_until DESC LIMIT 1",
    )
    .bind(platform)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    if locked
        .is_some_and(|until| chrono::DateTime::parse_from_rfc3339(&until).is_ok_and(|at| at > now))
    {
        return Err(HubError::message("pairing locked"));
    }
    let existing: Option<(String, String)> = sqlx::query_as(
        "SELECT expires_at, created_at FROM pairing_codes
         WHERE platform = ? AND user_id = ? AND consumed_at IS NULL
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(platform)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    if let Some((expires_at, created_at)) = existing {
        let expires = chrono::DateTime::parse_from_rfc3339(&expires_at).ok();
        let created = chrono::DateTime::parse_from_rfc3339(&created_at).ok();
        let still_valid = expires.is_some_and(|at| at > now);
        let recent = created.is_some_and(|at| {
            now.signed_duration_since(at.with_timezone(&chrono::Utc))
                < chrono::TimeDelta::seconds(PAIRING_RATE_LIMIT_SECONDS)
        });
        if still_valid && recent {
            return Ok(());
        }
    }
    let expires = (now + chrono::TimeDelta::seconds(PAIRING_TTL_SECONDS)).to_rfc3339();
    sqlx::query(
        "INSERT OR REPLACE INTO pairing_codes(code, platform, user_id, created_at, expires_at) VALUES(?,?,?,?,?)",
    )
    .bind(code)
    .bind(platform)
    .bind(user_id)
    .bind(now.to_rfc3339())
    .bind(expires)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn consume_pairing(
    pool: &sqlx::SqlitePool,
    code: &str,
) -> crate::error::Result<Option<String>> {
    let row: Option<(String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT user_id, expires_at, attempts, locked_until FROM pairing_codes WHERE code = ? AND consumed_at IS NULL",
    )
    .bind(code)
    .fetch_optional(pool)
    .await?;
    let Some((user, expires_at, _attempts, locked_until)) = row else {
        return Ok(None);
    };
    let now = chrono::Utc::now();
    if locked_until
        .as_deref()
        .is_some_and(|until| chrono::DateTime::parse_from_rfc3339(until).is_ok_and(|at| at > now))
    {
        return Ok(None);
    }
    let expires = chrono::DateTime::parse_from_rfc3339(&expires_at).ok();
    if !expires.is_some_and(|at| at > now) {
        return Ok(None);
    }
    sqlx::query("UPDATE pairing_codes SET consumed_at = ? WHERE code = ?")
        .bind(crate::storage::now_rfc3339())
        .bind(code)
        .execute(pool)
        .await?;
    Ok(Some(user))
}

/// Counts one failed guess against this code only. An unknown code does not burn another row.
/// Five failures lock the code for 900 seconds.
pub async fn note_pairing_failure(pool: &sqlx::SqlitePool, code: &str) -> crate::error::Result<()> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT attempts FROM pairing_codes WHERE code = ? AND consumed_at IS NULL")
            .bind(code)
            .fetch_optional(pool)
            .await?;
    let Some((attempts,)) = row else {
        return Ok(());
    };
    let attempts = attempts.saturating_add(1);
    let locked_until = (attempts >= PAIRING_MAX_ATTEMPTS).then(|| {
        (chrono::Utc::now() + chrono::TimeDelta::seconds(PAIRING_LOCKOUT_SECONDS)).to_rfc3339()
    });
    sqlx::query("UPDATE pairing_codes SET attempts = ?, locked_until = COALESCE(?, locked_until) WHERE code = ?")
        .bind(attempts)
        .bind(locked_until)
        .bind(code)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiscordDispatch {
    Inbound(Inbound),
    Approval(ApprovalClick),
    Command { name: String, data: Value },
    Ignore,
}

pub fn handle_dispatch(frame: &Value) -> Result<DiscordDispatch> {
    if frame.get("op").and_then(Value::as_i64) != Some(0) {
        return Ok(DiscordDispatch::Ignore);
    }
    let kind = frame.get("t").and_then(Value::as_str).unwrap_or("");
    let data = frame.get("d").cloned().unwrap_or(Value::Null);
    match kind {
        "MESSAGE_CREATE" => Ok(message_create(&data)
            .map(DiscordDispatch::Inbound)
            .unwrap_or(DiscordDispatch::Ignore)),
        "INTERACTION_CREATE" if data.get("type").and_then(Value::as_i64) == Some(2) => {
            let name = crate::discord_surface::command_name(&data)
                .unwrap_or("")
                .to_owned();
            Ok(DiscordDispatch::Command { name, data })
        }
        "INTERACTION_CREATE" => Ok(interaction(&data)?
            .map(DiscordDispatch::Approval)
            .unwrap_or(DiscordDispatch::Ignore)),
        _ => Ok(DiscordDispatch::Ignore),
    }
}

fn message_create(data: &Value) -> Option<Inbound> {
    if data.pointer("/author/bot").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let mut text = data
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let snapshots = data
        .get("message_snapshots")
        .cloned()
        .unwrap_or(Value::Null);
    let (forwarded, _files) = crate::discord_surface::extract_forwarded_snapshots(&snapshots);
    if !forwarded.is_empty() {
        text = if text.trim().is_empty() {
            forwarded
        } else {
            format!("{text}\n{forwarded}")
        };
    }
    if text.trim().is_empty() {
        return None;
    }
    if crate::discord_surface::is_forum_message(data) {
        let title = crate::discord_surface::derive_forum_post_title(&text);
        text = format!("[{title}]\n{text}");
    }
    Some(Inbound {
        platform: PLATFORM.into(),
        guild_id: data
            .get("guild_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        channel_id: data.get("channel_id").and_then(Value::as_str)?.to_owned(),
        thread_id: None,
        user_id: data
            .pointer("/author/id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        text,
        parent_chat_id: None,
        event_id: data.get("id").and_then(Value::as_str).map(str::to_owned),
    })
}

fn interaction(data: &Value) -> Result<Option<ApprovalClick>> {
    let custom_id = data
        .pointer("/data/custom_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !custom_id.starts_with("omon:approval:") && !custom_id.starts_with("ferryx:approval:") {
        return Ok(None);
    }
    let session_id = data
        .pointer("/ferryx/sessionId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let revision = data
        .pointer("/ferryx/screenRevision")
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(click) = super::approval::parse_approval_value(custom_id) {
        return Ok(Some(click));
    }
    Ok(parse_omon_approval(custom_id, session_id, revision))
}

/// REST body for a channel message. The caller posts it with the bot token.
pub fn channel_message_body(text: &str) -> Value {
    serde_json::json!({ "content": text })
}

/// An inbound message is fenced and sent. An approval click becomes one Answer.
pub async fn apply_dispatch(
    pool: &sqlx::SqlitePool,
    frame: &Value,
    meta_session_id: &str,
    profiles: &super::router::ProfileRouter,
    control: &dyn crate::agent_control::AgentControl,
) -> Result<DiscordDispatch> {
    let parsed = handle_dispatch(frame)?;
    match &parsed {
        DiscordDispatch::Inbound(inbound) => {
            super::router::route_inbound(inbound, profiles, meta_session_id, control).await?;
        }
        DiscordDispatch::Approval(click) => {
            let op =
                super::approval::answer_op(click, &format!("discord-approval-{}", click.prompt_id));
            control.execute(None, op).await?;
        }
        DiscordDispatch::Command { data, .. } => {
            let text = answer_command(pool, data, control).await?;
            if !text.is_empty() {
                sqlx::query(
                    "INSERT INTO chat_outbox(session_key, platform, text, created_at) VALUES(?, 'discord', ?, ?)",
                )
                .bind(meta_session_id)
                .bind(&text)
                .bind(crate::storage::now_rfc3339())
                .execute(pool)
                .await?;
            }
        }
        DiscordDispatch::Ignore => {}
    }
    Ok(parsed)
}

fn command_values(data: &Value) -> Vec<&str> {
    data.pointer("/data/options/0/options")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("value").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default()
}

async fn answer_voice(data: &Value) -> Result<String> {
    #[cfg(feature = "hub-discord")]
    {
        if crate::discord_surface::voice_subcommand(data) == Some("join") {
            let owned = data.clone();
            tokio::spawn(async move {
                let text = crate::discord_voice::answer(&owned)
                    .await
                    .unwrap_or_else(|error| error);
                if let Some(channel) = crate::discord_surface::interaction_channel(&owned) {
                    let _ = deliver(&channel, &text).await;
                }
            });
            Ok(String::new())
        } else {
            crate::discord_voice::answer(data)
                .await
                .map_err(HubError::message)
        }
    }
    #[cfg(not(feature = "hub-discord"))]
    {
        let _ = data;
        Ok("Discord voice is compiled with hub-discord.".into())
    }
}

async fn answer_command(
    pool: &sqlx::SqlitePool,
    data: &Value,
    control: &dyn crate::agent_control::AgentControl,
) -> Result<String> {
    let name = crate::discord_surface::command_name(data).unwrap_or("");
    match name {
        "sessions" => answer_sessions(pool, data, control).await,
        "status" => {
            let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
                .fetch_one(pool)
                .await?;
            let memories: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories")
                .fetch_one(pool)
                .await?;
            let ledger: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM action_proposals")
                .fetch_one(pool)
                .await?;
            Ok(format!(
                "Active sessions: {sessions}\nMemory entries: {memories}\nUptime: {}s\nLedger entries: {ledger}",
                crate::chat::slash::uptime_secs()
            ))
        }
        "skill" | "skills" => {
            let names =
                crate::tools::list_skills(crate::tools::skills_root(&serde_json::json!({})));
            let total = names.len();
            Ok(format!(
                "📚 *Available OMO Skills* ({total} total):\n`{}`",
                names.join("`, `")
            ))
        }
        "voice" => answer_voice(data).await,
        "pair" => {
            let code = data
                .pointer("/data/options/0/value")
                .and_then(Value::as_str)
                .unwrap_or("");
            match consume_pairing(pool, code).await? {
                Some(user_id) => Ok(format!("Paired {user_id}")),
                None => {
                    note_pairing_failure(pool, code).await?;
                    Ok("Invalid pairing code.".into())
                }
            }
        }
        other => Ok(format!("/{other} is registered")),
    }
}

async fn answer_sessions(
    pool: &sqlx::SqlitePool,
    data: &Value,
    control: &dyn crate::agent_control::AgentControl,
) -> Result<String> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT session_key, channel_id FROM sessions ORDER BY updated_at DESC LIMIT 20",
    )
    .fetch_all(pool)
    .await?;
    let sub = data
        .pointer("/data/options/0/name")
        .and_then(Value::as_str)
        .unwrap_or("overview");
    let values = command_values(data);
    let target = values.first().copied();
    match sub {
        "overview" => Ok(crate::discord_surface::session_overview(&rows)),
        "read" => {
            let Some(target) = target else {
                return Ok("Usage: sessions read <target>".into());
            };
            let messages: Vec<(String, String)> = sqlx::query_as(
                "SELECT role, content FROM messages WHERE session_key = ? ORDER BY created_at DESC LIMIT 5",
            )
            .bind(target)
            .fetch_all(pool)
            .await?;
            if messages.is_empty() {
                return Ok(format!("No messages for `{target}`."));
            }
            Ok(messages
                .into_iter()
                .rev()
                .map(|(role, content)| format!("*{role}:* {content}"))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "send" => {
            let Some(target) = target else {
                return Ok("Usage: sessions send <target>".into());
            };
            let text = values.get(1).copied().unwrap_or("");
            control
                .execute(
                    None,
                    crate::agent_control::AgentOp::Send {
                        session_id: target.to_owned(),
                        text: text.to_owned(),
                        request_id: "discord-sessions-send".into(),
                    },
                )
                .await?;
            Ok(format!("Sent: {text}"))
        }
        "stop" => {
            let Some(target) = target else {
                return Ok("Usage: sessions stop <target>".into());
            };
            control
                .execute(
                    None,
                    crate::agent_control::AgentOp::Stop {
                        session_id: target.to_owned(),
                        request_id: "discord-sessions-stop".into(),
                    },
                )
                .await?;
            Ok("Stopped that session.".into())
        }
        "answer" => {
            let Some(target) = target else {
                return Ok("Usage: sessions answer <target>".into());
            };
            let text = values.get(1).copied().unwrap_or("");
            let read = control
                .execute(
                    None,
                    crate::agent_control::AgentOp::Read {
                        session_id: target.to_owned(),
                    },
                )
                .await
                .ok();
            let prompt_id = read
                .as_ref()
                .and_then(|value| value.pointer("/session/pending/id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            if prompt_id.is_empty() {
                return Ok("No live prompt on that session.".into());
            }
            let revision = read
                .as_ref()
                .and_then(|value| value.pointer("/session/screen/revision"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            control
                .execute(
                    None,
                    crate::agent_control::AgentOp::Answer {
                        session_id: target.to_owned(),
                        prompt_id,
                        screen_revision: revision,
                        answer: Value::String(text.to_owned()),
                        request_id: "discord-sessions-answer".into(),
                    },
                )
                .await?;
            Ok(format!("Answered: {text}"))
        }
        "watch" => {
            let Some(target) = target else {
                return Ok("Usage: sessions watch <target>".into());
            };
            sqlx::query(
                "INSERT OR IGNORE INTO session_watches(target, added_at, reason) VALUES(?,?, 'discord')",
            )
            .bind(target)
            .bind(crate::storage::now_rfc3339())
            .execute(pool)
            .await?;
            Ok(format!("Watching `{target}`."))
        }
        "unwatch" => {
            let Some(target) = target else {
                return Ok("Usage: sessions unwatch <target>".into());
            };
            sqlx::query("DELETE FROM session_watches WHERE target = ?")
                .bind(target)
                .execute(pool)
                .await?;
            Ok(format!("Stopped watching `{target}`."))
        }
        other => Ok(crate::discord_surface::session_command_text(
            other, &rows, target,
        )),
    }
}

/// Posts `text` when `DISCORD_TOKEN` is set. No token reports that nothing was sent.
#[cfg(feature = "hub-discord")]
pub async fn post_if_configured(session: &super::session::SessionKey, text: &str) -> Result<bool> {
    let Some(token) = bot_token() else {
        return Ok(false);
    };
    let delay = message_rate()
        .lock()
        .await
        .reserve(&session.channel_id, std::time::Instant::now());
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let client = reqwest::Client::new();
    let (content, images) = crate::table_image::transform_markdown_tables(text);
    let response = if images.is_empty() {
        client
            .post(format!(
                "https://discord.com/api/v10/channels/{}/messages",
                session.channel_id
            ))
            .header("Authorization", format!("Bot {token}"))
            .json(&serde_json::json!({ "content": content }))
            .send()
            .await
            .map_err(|error| HubError::message(error.to_string()))?
    } else {
        let attachments: Vec<_> = images
            .iter()
            .enumerate()
            .map(|(index, image)| serde_json::json!({"id": index, "filename": image.filename}))
            .collect();
        let payload = serde_json::json!({"content": content, "attachments": attachments});
        let mut form = reqwest::multipart::Form::new().text("payload_json", payload.to_string());
        for (index, image) in images.into_iter().enumerate() {
            let part = reqwest::multipart::Part::bytes(image.image_bytes)
                .file_name(image.filename)
                .mime_str("image/png")
                .map_err(|error| HubError::message(error.to_string()))?;
            form = form.part(format!("files[{index}]"), part);
        }
        client
            .post(format!(
                "https://discord.com/api/v10/channels/{}/messages",
                session.channel_id
            ))
            .header("Authorization", format!("Bot {token}"))
            .multipart(form)
            .send()
            .await
            .map_err(|error| HubError::message(error.to_string()))?
    };
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(HubError::message(format!(
            "discord channel message failed: {status} {body}"
        )));
    }
    Ok(true)
}

/// Posts one cron or lane delivery into a Discord channel.
#[cfg(feature = "hub-discord")]
pub async fn deliver(channel: &str, text: &str) -> Result<bool> {
    let session =
        super::session::SessionKey::new("discord", Some(channel), channel, None::<String>, "");
    post_if_configured(&session, text).await
}

pub async fn publish(session: &super::session::SessionKey, text: &str) -> Result<bool> {
    #[cfg(feature = "hub-discord")]
    {
        return post_if_configured(session, text).await;
    }
    #[cfg(not(feature = "hub-discord"))]
    {
        let _ = (session, text);
        Ok(false)
    }
}

#[cfg(feature = "hub-discord")]
fn message_rate() -> &'static tokio::sync::Mutex<ChannelRate> {
    static RATE: std::sync::OnceLock<tokio::sync::Mutex<ChannelRate>> = std::sync::OnceLock::new();
    RATE.get_or_init(|| tokio::sync::Mutex::new(ChannelRate::discord_messages()))
}

#[cfg(feature = "hub-discord")]
pub async fn connect_gateway<F, Fut>(mut on_frame: F) -> Result<()>
where
    F: FnMut(Value) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    let token = bot_token().ok_or_else(|| HubError::config("DISCORD_TOKEN is not set"))?;
    let mut attempt = 0u32;
    loop {
        match run_gateway_session(&token, &mut on_frame).await {
            Ok(()) => attempt = 0,
            Err(error) => tracing::warn!(%error, attempt, "discord gateway session ended"),
        }
        tokio::time::sleep(reconnect_backoff(attempt)).await;
        attempt = attempt.saturating_add(1);
    }
}

#[cfg(feature = "hub-discord")]
async fn run_gateway_session<F, Fut>(token: &str, on_frame: &mut F) -> Result<()>
where
    F: FnMut(Value) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    let client = reqwest::Client::new();
    let gateway: Value = client
        .get("https://discord.com/api/v10/gateway/bot")
        .header("Authorization", format!("Bot {token}"))
        .send()
        .await
        .map_err(|error| HubError::message(error.to_string()))?
        .json()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    let url = gateway
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| HubError::message("discord gateway url missing"))?;
    let (socket, _) = tokio_tungstenite::connect_async(format!("{url}/?v=10&encoding=json"))
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    use futures_util::{SinkExt, StreamExt};
    let (mut write, mut read) = socket.split();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<String>(8);
    let _gateway = crate::discord_voice::attach(out_tx.clone());
    let writer = tokio::spawn(async move {
        while let Some(text) = out_rx.recv().await {
            if write
                .send(tokio_tungstenite::tungstenite::Message::Text(text.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    let seq = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(-1));
    let mut heartbeat: Option<tokio::task::JoinHandle<()>> = None;
    let mut identified = false;
    let watchdog = crate::discord_surface::receive_watchdog_threshold();
    loop {
        let next = read.next();
        tokio::pin!(next);
        let message = if watchdog == 0 {
            next.await
        } else {
            let limit = std::time::Duration::from_secs(watchdog.saturating_mul(3));
            tokio::select! {
                message = &mut next => message,
                () = tokio::time::sleep(limit) => {
                    return Err(HubError::message("discord receive watchdog restart"));
                }
            }
        };
        let Some(message) = message else {
            break;
        };
        let message = message.map_err(|error| HubError::message(error.to_string()))?;
        let tokio_tungstenite::tungstenite::Message::Text(text) = message else {
            continue;
        };
        let Ok(frame) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let op = frame.get("op").and_then(Value::as_i64);
        if op == Some(10) && !identified {
            let millis = heartbeat_millis(&frame).unwrap_or(41250);
            let beat_tx = out_tx.clone();
            let beat_seq = std::sync::Arc::clone(&seq);
            heartbeat = Some(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_millis(millis));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    let current = beat_seq.load(std::sync::atomic::Ordering::Relaxed);
                    let body = heartbeat_frame((current >= 0).then_some(current));
                    if beat_tx.send(body.to_string()).await.is_err() {
                        break;
                    }
                }
            }));
            let identify = identify_body(token, gateway_intents(voice_receive_from_env()));
            out_tx
                .send(identify.to_string())
                .await
                .map_err(|error| HubError::message(error.to_string()))?;
            let register_token = token.to_owned();
            tokio::spawn(async move {
                if let Err(error) = register_global_commands(&register_token).await {
                    tracing::warn!(%error, "discord slash registration failed");
                }
            });
            identified = true;
            continue;
        }
        if op == Some(0) {
            if let Some(number) = frame.get("s").and_then(Value::as_i64) {
                seq.store(number, std::sync::atomic::Ordering::Relaxed);
            }
            crate::discord_voice::observe(&frame);
            on_frame(frame).await;
        }
    }
    drop(out_tx);
    if let Some(task) = heartbeat {
        task.abort();
    }
    let _ = writer.await;
    Err(HubError::message("discord gateway closed"))
}

#[cfg(feature = "hub-discord")]
async fn register_global_commands(token: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let me: Value = client
        .get("https://discord.com/api/v10/oauth2/applications/@me")
        .header("Authorization", format!("Bot {token}"))
        .send()
        .await
        .map_err(|error| HubError::message(error.to_string()))?
        .json()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    let id = me
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| HubError::message("discord application id missing"))?;
    let response = client
        .put(format!(
            "https://discord.com/api/v10/applications/{id}/commands"
        ))
        .header("Authorization", format!("Bot {token}"))
        .json(&crate::discord_surface::application_commands())
        .send()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    if !response.status().is_success() {
        let status = response.status();
        return Err(HubError::message(format!(
            "discord slash registration failed: {status}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_message_and_an_approval_interaction_parse() {
        let message = json!({"op":0,"t":"MESSAGE_CREATE","d":{"channel_id":"9","content":"hi","author":{"id":"3","bot":false}}});
        match handle_dispatch(&message).unwrap() {
            DiscordDispatch::Inbound(inbound) => assert_eq!(inbound.text, "hi"),
            other => panic!("expected inbound, got {other:?}"),
        }
        let click = json!({
            "op": 0,
            "t": "INTERACTION_CREATE",
            "d": {
                "data": {"custom_id": "omon:approval:prompt-9:once"},
                "ferryx": {"sessionId": "pane-1", "screenRevision": "rev-1"}
            }
        });
        match handle_dispatch(&click).unwrap() {
            DiscordDispatch::Approval(click) => {
                assert_eq!(click.prompt_id, "prompt-9");
                assert_eq!(click.option_index, 0);
                assert_eq!(click.screen_revision, "rev-1");
            }
            other => panic!("expected approval, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn pairing_is_single_use_and_threads_are_named() {
        assert!(should_auto_thread(true, false));
        assert!(!should_auto_thread(true, true));
        assert_eq!(auto_thread_name("  hello   thread  "), "hello thread");
        let components = approval_components(&[("Yes", "ferryx:approval:1")]);
        assert_eq!(
            components["components"][0]["custom_id"],
            "ferryx:approval:1"
        );
        let pool = crate::storage::memory().await.unwrap();
        issue_pairing(&pool, "discord", "user-1", "123456")
            .await
            .unwrap();
        let forum = json!({"op":0,"t":"MESSAGE_CREATE","d":{
            "channel_id":"9","content":"hi","channel_type":15,
            "author":{"id":"3","bot":false},
            "message_snapshots":[{"content":"quoted","attachments":[{"filename":"shot.png"}]}]
        }});
        match handle_dispatch(&forum).unwrap() {
            DiscordDispatch::Inbound(inbound) => {
                assert!(inbound.text.contains("[Forwarded]"));
                assert!(inbound.text.contains("quoted"));
                assert!(inbound.text.starts_with('['));
            }
            other => panic!("expected inbound, got {other:?}"),
        }
        let command = json!({"op":0,"t":"INTERACTION_CREATE","d":{"type":2,"data":{"name":"sessions","options":[{"type":1,"name":"overview"}]}}});
        match handle_dispatch(&command).unwrap() {
            DiscordDispatch::Command { name, .. } => assert_eq!(name, "sessions"),
            other => panic!("expected command, got {other:?}"),
        }
        assert_eq!(
            consume_pairing(&pool, "123456").await.unwrap().as_deref(),
            Some("user-1")
        );
        assert!(consume_pairing(&pool, "123456").await.unwrap().is_none());
        issue_pairing(&pool, "discord", "user-2", "999999")
            .await
            .unwrap();
        for _ in 0..PAIRING_MAX_ATTEMPTS {
            note_pairing_failure(&pool, "999999").await.unwrap();
        }
        note_pairing_failure(&pool, "no-such-code").await.unwrap();
        assert!(consume_pairing(&pool, "999999").await.unwrap().is_none());
        let attempts: i64 =
            sqlx::query_scalar("SELECT attempts FROM pairing_codes WHERE code = '999999'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(attempts, PAIRING_MAX_ATTEMPTS);
        let locked = issue_pairing(&pool, "discord", "user-2", "888888")
            .await
            .unwrap_err()
            .to_string();
        assert!(locked.contains("pairing locked"));
    }

    #[test]
    fn intents_heartbeat_threads_and_the_channel_rate_match_discord() {
        assert_eq!(gateway_intents(false), 37377);
        assert_eq!(gateway_intents(true), 37377 + INTENT_GUILD_VOICE_STATES);
        let hello = json!({"op": 10, "d": {"heartbeat_interval": 41250}});
        assert_eq!(heartbeat_millis(&hello), Some(41250));
        assert_eq!(heartbeat_frame(Some(4))["op"], 1);
        assert_eq!(reconnect_backoff(0), std::time::Duration::from_secs(2));
        assert_eq!(reconnect_backoff(5), std::time::Duration::from_secs(60));
        assert!(should_auto_create_thread(true, true, true, false, false));
        assert!(!should_auto_create_thread(true, true, true, false, true));
        assert_eq!(
            derive_auto_thread_name("<@9> hello thread", "9"),
            "hello thread"
        );
        let profiles = super::super::router::ProfileRouter::from_json(
            r#"[{"name":"a","channel":"11","pane_session_id":"pane-a"},{"name":"b","channel":"22","pane_session_id":"pane-b"}]"#,
        );
        assert_eq!(
            profiles
                .match_route(None, 22, None)
                .unwrap()
                .pane_session_id
                .as_deref(),
            Some("pane-b")
        );
        let start = std::time::Instant::now();
        let mut rate = ChannelRate::discord_messages();
        for _ in 0..5 {
            assert!(rate.reserve("C", start).is_zero());
        }
        assert_eq!(rate.reserve("C", start), std::time::Duration::from_secs(5));
    }
}
