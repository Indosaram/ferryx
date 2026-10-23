//! Runs a parsed `/omo-*` command against the hub database and agent control.
use serde_json::Value;
use sqlx::SqlitePool;

use super::commands::{
    help_text, parse_command, session_key, HuddleAction, SessionsCommand, SlackCommand,
    SlashInvocation,
};
use super::session::SessionKey;
use crate::agent_control::{AgentControl, AgentOp};
use crate::error::Result;
use crate::lanes::kibitzer::ModelTurn;
use crate::storage::now_rfc3339;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashReply {
    pub text: String,
    pub ephemeral: bool,
}

impl SlashReply {
    fn public(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ephemeral: false,
        }
    }

    fn private(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ephemeral: true,
        }
    }
}

const UNAUTHORIZED: &str = "You are not authorized to use this command.";
const OWNER_ONLY: &str = "Only the owner can run that command.";
const HUDDLE_OFF: &str =
    "Slack huddles are off. Set OMON_SLACK_HUDDLE_ENABLED=true and restart the hub.";

fn process_started() -> chrono::DateTime<chrono::Utc> {
    static START: std::sync::OnceLock<chrono::DateTime<chrono::Utc>> = std::sync::OnceLock::new();
    *START.get_or_init(chrono::Utc::now)
}

pub(crate) fn uptime_secs() -> i64 {
    (chrono::Utc::now() - process_started())
        .num_seconds()
        .max(0)
}

/// One stdin line for the huddle sidecar. The sidecar process is `bun` plus the
/// bundled `index.mjs`. A missing bun or a timeout is returned to the caller.
pub async fn run_huddle_sidecar(line: &str) -> Result<String> {
    let sidecar = crate::voice::sidecar_path();
    let mut child = crate::child_env::command("bun")
        .arg(&sidecar)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| crate::error::HubError::message(error.to_string()))?;
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(line.as_bytes()).await;
        let _ = stdin.write_all(b"\n").await;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(8), child.wait_with_output()).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if stdout.is_empty() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                if stderr.is_empty() {
                    Ok(String::new())
                } else {
                    Err(crate::error::HubError::message(stderr))
                }
            } else {
                Ok(stdout)
            }
        }
        Ok(Err(error)) => Err(crate::error::HubError::message(error.to_string())),
        Err(_) => Err(crate::error::HubError::message("huddle sidecar timed out")),
    }
}

/// One stdin line for the huddle sidecar.
pub fn huddle_line(action: &HuddleAction) -> String {
    let body = match action {
        HuddleAction::Join(target) => serde_json::json!({"id": "1", "op": "join", "url": target}),
        HuddleAction::Leave => serde_json::json!({"id": "1", "op": "leave"}),
        HuddleAction::Status => serde_json::json!({"id": "1", "op": "status"}),
        HuddleAction::Speak(text) => serde_json::json!({"id": "1", "op": "speak", "text": text}),
        HuddleAction::Transcript => serde_json::json!({"id": "1", "op": "transcript"}),
    };
    body.to_string()
}

pub struct SlashEnv<'a> {
    pub pool: &'a SqlitePool,
    pub owner_id: &'a str,
    pub bot_user_id: &'a str,
    pub control: &'a dyn AgentControl,
    pub meta_session_id: &'a str,
    pub huddle_enabled: bool,
    /// When set, `/omo-compress` summarizes history through the omo model backend.
    pub model: Option<&'a dyn ModelTurn>,
}

pub async fn execute(env: &SlashEnv<'_>, invocation: &SlashInvocation) -> Result<SlashReply> {
    if let Some(reply) = admission(env, invocation).await? {
        return Ok(reply);
    }
    let command = match parse_command(&invocation.command, &invocation.text) {
        Ok(command) => command,
        Err(usage) => return Ok(SlashReply::private(usage)),
    };
    let key = session_key(invocation, env.bot_user_id);
    match command {
        SlackCommand::Help => Ok(SlashReply::private(help_text())),
        SlackCommand::Model(name) => {
            remember(env.pool, &model_key(&key), &name).await?;
            Ok(SlashReply::public(format!("Model switched to `{name}`.")))
        }
        SlackCommand::Reset { confirm } => {
            if confirm != Some(true) {
                return Ok(SlashReply::public(
                    "⚠️ This will permanently clear conversation context and memories for this session. Re-run `/omo-reset confirm` to proceed.",
                ));
            }
            sqlx::query("DELETE FROM messages WHERE session_key = ?")
                .bind(key.storage_key())
                .execute(env.pool)
                .await?;
            sqlx::query("DELETE FROM memories WHERE session_key = ?")
                .bind(key.storage_key())
                .execute(env.pool)
                .await?;
            Ok(SlashReply::public(
                "Session context and memory cleared.".to_owned(),
            ))
        }
        SlackCommand::Stop => stop(env, invocation, &key).await,
        SlackCommand::Status => status(env).await,
        SlackCommand::Tools => Ok(SlashReply::public(format!(
            "Tools: {}",
            crate::context::TOOLS.join(", ")
        ))),
        SlackCommand::Skill { action, arg } => skill(env, &action, arg.as_deref()).await,
        SlackCommand::Memory { action, arg } => memory(env, &key, &action, arg.as_deref()).await,
        SlackCommand::Cron { action } => cron(env, &action).await,
        SlackCommand::Steer(text) => steer(env, invocation, &key, &text).await,
        SlackCommand::Undo => undo(env, &key).await,
        SlackCommand::Retry => retry(env, &key).await,
        SlackCommand::Compress => compress(env, &key).await,
        SlackCommand::Title(text) => title(env, invocation, &text).await,
        SlackCommand::Thread { name, message } => {
            thread(env, invocation, &key, &name, message).await
        }
        SlackCommand::Deny(reason) => deny(env, &key, reason.as_deref()).await,
        SlackCommand::Yolo(mode) => yolo(env, &key, mode.as_deref()).await,
        SlackCommand::Pair(code) => pair(env, invocation, &code).await,
        SlackCommand::Sessions(command) => {
            if invocation.user_id != env.owner_id {
                return Ok(SlashReply::private(OWNER_ONLY));
            }
            sessions(env, command).await
        }
        SlackCommand::Huddle(action) => {
            if !env.huddle_enabled {
                return Ok(SlashReply::private(HUDDLE_OFF));
            }
            let line = huddle_line(&action);
            remember(env.pool, "huddle:last", &line).await?;
            match run_huddle_sidecar(&line).await {
                Ok(stdout) if !stdout.trim().is_empty() => Ok(SlashReply::private(stdout)),
                Ok(_) => Ok(SlashReply::private(line)),
                Err(error) => Ok(SlashReply::private(format!(
                    "{line}\nhuddle sidecar: {error}"
                ))),
            }
        }
    }
}

#[cfg_attr(not(feature = "hub-slack"), allow(dead_code))]
const THREAD_ONLY: &str =
    "이 명령은 대화 스레드 안에서 실행해 주세요. 채널 최상위에서는 새 채널 세션을 만들지 않습니다.";

/// Which session a stop or steer means. Inside a thread or DM it is that session. At a
/// channel's top level Slack sends no thread, so it is the one thread with a live turn there.
fn live_turn_target(
    invocation: &SlashInvocation,
    key: &SessionKey,
) -> std::result::Result<Option<String>, &'static str> {
    let storage = key.storage_key();
    if super::turns::is_active(&storage) {
        return Ok(Some(storage));
    }
    if invocation.is_dm() || invocation.thread_ts.is_some() {
        return Ok(None);
    }
    let mut candidates = super::turns::active_in_channel(&invocation.channel_id);
    match candidates.len() {
        0 => Err("이 채널에 진행 중인 대화가 없습니다."),
        1 => Ok(candidates.pop()),
        _ => Err(
            "진행 중인 대화가 여러 개입니다. 멈추거나 거절할 대화의 스레드 안에서 실행해 주세요.",
        ),
    }
}

/// `/omo-stop`: interrupts the session's running omo turn (`turn/interrupt`) and marks the
/// session suspended. Without the omo app-server path, the meta pane is stopped instead.
async fn stop(
    env: &SlashEnv<'_>,
    invocation: &SlashInvocation,
    key: &SessionKey,
) -> Result<SlashReply> {
    let target = match live_turn_target(invocation, key) {
        Ok(target) => target,
        Err(message) if omo_turns_enabled() => return Ok(SlashReply::private(message)),
        Err(_) => None,
    };
    let session = target.clone().unwrap_or_else(|| key.storage_key());
    let interrupted = target.as_deref().is_some_and(super::turns::stop);
    remember(env.pool, &format!("suspended:{session}"), "1").await?;
    if interrupted {
        return Ok(SlashReply::private(
            "🛑 Stopped active turn and marked session suspended.",
        ));
    }
    if omo_turns_enabled() {
        return Ok(SlashReply::private(
            "🛑 No active turn running; session marked suspended.",
        ));
    }
    env.control
        .execute(
            None,
            AgentOp::Stop {
                session_id: env.meta_session_id.to_owned(),
                request_id: format!("omo-stop-{}", invocation.user_id),
            },
        )
        .await?;
    Ok(SlashReply::private(
        "🛑 Stopped active turn and marked session suspended.",
    ))
}

fn omo_turns_enabled() -> bool {
    #[cfg(feature = "hub-slack")]
    {
        super::slack_turn::installed().is_some()
    }
    #[cfg(not(feature = "hub-slack"))]
    {
        false
    }
}

/// `/omo-steer`: guidance goes into the running turn (`turn/steer`); with no turn running, or
/// one the engine will not steer, it starts the session's next turn instead (omo-gateway).
async fn steer(
    env: &SlashEnv<'_>,
    invocation: &SlashInvocation,
    key: &SessionKey,
    text: &str,
) -> Result<SlashReply> {
    let prompt = format!("[Steering] {}", text.trim());
    #[cfg(feature = "hub-slack")]
    if let Some(config) = super::slack_turn::installed() {
        let target = match live_turn_target(invocation, key) {
            Ok(Some(target)) => target,
            Ok(None) if invocation.is_dm() || invocation.thread_ts.is_some() => key.storage_key(),
            _ => return Ok(SlashReply::private(THREAD_ONLY)),
        };
        if let Some((thread_id, turn_id)) = super::turns::ids(&target) {
            let mut omo = config.omo.clone();
            if let Some(path) = &config.token_path {
                omo.token = crate::meta::omo_client::token_from_file(path);
            }
            match crate::meta::omo_client::steer(&omo, &thread_id, &turn_id, &prompt).await {
                Ok(true) => {
                    return Ok(SlashReply::private(format!(
                        "🎯 Steering guidance applied to the running turn: `{text}`"
                    )))
                }
                Ok(false) => {}
                Err(error) => tracing::warn!(%error, "turn/steer failed; queuing a new turn"),
            }
        }
        let session = SessionKey::from_storage_key(&target)
            .map_err(|error| crate::error::HubError::message(error.to_string()))?;
        let admitted = super::slack_turn::Admitted {
            channel: session.channel_id.clone(),
            reply_thread: session.thread_id.clone(),
            message_ts: format!("steer-{}", uuid::Uuid::new_v4()),
            user: invocation.user_id.clone(),
            text: prompt,
            key: session,
            files: Vec::new(),
            event: serde_json::Value::Null,
            dm: false,
            bot_user: String::new(),
        };
        let pool = env.pool.clone();
        super::slack::spawn_carrying_test_api(async move {
            super::slack_turn::handle(config, &pool, admitted).await;
        });
        return Ok(SlashReply::private(format!(
            "🎯 Steering guidance queued: `{text}`"
        )));
    }
    let _ = key;
    env.control
        .execute(
            None,
            AgentOp::Send {
                session_id: env.meta_session_id.to_owned(),
                text: prompt,
                request_id: format!("omo-steer-{}", invocation.user_id),
            },
        )
        .await?;
    Ok(SlashReply::private(format!(
        "🎯 Steering guidance applied to the running turn: `{text}`"
    )))
}

const THREAD_ROOT_PREFIX: &str = "🧵 *";
/// Slack slash payloads carry no `thread_ts`, so `/omo-title` must be told its thread explicitly.
const TITLE_NO_THREAD: &str = "❌ `/omo-title` cannot tell which thread you mean: Slack sends no `thread_ts` with a slash command, even inside a thread. Run `/omo-title <thread link or timestamp> <title>`.";

#[cfg_attr(not(feature = "hub-slack"), allow(dead_code))]
fn thread_root_text(name: &str) -> String {
    format!("{THREAD_ROOT_PREFIX}{name}*")
}

#[cfg_attr(not(feature = "hub-slack"), allow(dead_code))]
fn escape_mrkdwn(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Reads the leading `<thread link or timestamp>` of `/omo-title` text: a bare Slack ts
/// (`1700000000.000100`) or a permalink to the same channel
/// (`https://<workspace>.slack.com/archives/<channel>/p1700000000000100`).
fn explicit_thread(text: &str, channel: &str) -> Option<(String, String)> {
    let (target, title) = text.trim().split_once(char::is_whitespace)?;
    let target = target.trim_start_matches('<').trim_end_matches('>');
    let target = target.split('|').next()?;
    let timestamp = if target.starts_with("https://") {
        let (_, path) = target.split_once("://")?.1.split_once('/')?;
        let path = path.split(['?', '#']).next()?;
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() != 3 || parts[0] != "archives" || parts[1] != channel {
            return None;
        }
        let digits = parts[2].strip_prefix('p')?;
        if digits.len() <= 6 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let (seconds, micros) = digits.split_at(digits.len() - 6);
        format!("{seconds}.{micros}")
    } else {
        let (seconds, fraction) = target.split_once('.')?;
        if seconds.is_empty()
            || fraction.is_empty()
            || !seconds
                .bytes()
                .chain(fraction.bytes())
                .all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        target.to_owned()
    };
    Some((timestamp, title.trim().to_owned()))
}

/// A stored thread session makes ingress treat unmentioned replies in it as the bot's.
#[cfg_attr(not(feature = "hub-slack"), allow(dead_code))]
async fn ensure_session(pool: &SqlitePool, key: &SessionKey) -> Result<()> {
    sqlx::query(
        "INSERT INTO sessions (session_key, platform, guild_id, channel_id, thread_id, user_id, state_json)
         VALUES (?, ?, ?, ?, ?, ?, '{}') ON CONFLICT(session_key) DO NOTHING",
    )
    .bind(key.storage_key())
    .bind(&key.platform)
    .bind(&key.guild_id)
    .bind(&key.channel_id)
    .bind(&key.thread_id)
    .bind(&key.user_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// `/omo-thread <name> [message]`: Slack has no thread objects, so the thread is a root
/// message whose `ts` keys the session. A starter message runs as the thread's first turn.
async fn thread(
    env: &SlashEnv<'_>,
    invocation: &SlashInvocation,
    key: &SessionKey,
    name: &str,
    message: Option<String>,
) -> Result<SlashReply> {
    if invocation.is_dm() {
        return Ok(SlashReply::private(
            "❌ Threads can only be created in channels, not in DMs.",
        ));
    }
    if invocation.thread_ts.is_some() {
        return Ok(SlashReply::private(
            "❌ Cannot create a thread inside an existing thread.",
        ));
    }
    #[cfg(feature = "hub-slack")]
    {
        let root_ts =
            super::slack_web::post_message(&invocation.channel_id, &thread_root_text(name)).await?;
        let thread_key = SessionKey {
            thread_id: Some(root_ts.clone()),
            ..key.clone()
        };
        ensure_session(env.pool, &thread_key).await?;
        let link = match super::slack_web::permalink(&invocation.channel_id, &root_ts).await {
            Ok(url) => format!("<{url}|{}>", escape_mrkdwn(name)),
            Err(error) => {
                tracing::warn!(%error, channel = %invocation.channel_id, "slack thread permalink lookup failed");
                format!("*{name}*")
            }
        };
        if let Some(starter) = message.filter(|text| !text.trim().is_empty()) {
            let Some(config) = super::slack_turn::installed() else {
                return Ok(SlashReply::public(format!(
                    "❌ Created thread {link} but could not start a session: the omo app-server is not configured"
                )));
            };
            let admitted = super::slack_turn::Admitted {
                key: thread_key,
                channel: invocation.channel_id.clone(),
                reply_thread: Some(root_ts.clone()),
                message_ts: root_ts,
                user: invocation.user_id.clone(),
                text: starter,
                files: Vec::new(),
                event: serde_json::Value::Null,
                dm: false,
                bot_user: String::new(),
            };
            let pool = env.pool.clone();
            super::slack::spawn_carrying_test_api(async move {
                super::slack_turn::handle(config, &pool, admitted).await;
            });
        }
        Ok(SlashReply::public(format!("🧵 Created thread {link}.")))
    }
    #[cfg(not(feature = "hub-slack"))]
    {
        let _ = (env, key, message);
        Ok(SlashReply::private(format!(
            "❌ Cannot create `{name}`: this hub was built without Slack."
        )))
    }
}

/// `/omo-title`: renames a thread made by `/omo-thread` by editing its bot-authored root.
/// Slack slash payloads carry no `thread_ts` (even when typed inside a thread), so the
/// thread may be named explicitly: `/omo-title <thread link or ts> <text>`.
async fn title(env: &SlashEnv<'_>, invocation: &SlashInvocation, text: &str) -> Result<SlashReply> {
    let explicit =
        explicit_thread(text, &invocation.channel_id).filter(|(_, title)| !title.trim().is_empty());
    let (thread_ts, text): (String, &str) = match (&invocation.thread_ts, &explicit) {
        (Some(thread_ts), _) => (thread_ts.clone(), text),
        (None, Some((thread_ts, title))) => (thread_ts.clone(), title.as_str()),
        (None, None) => return Ok(SlashReply::private(TITLE_NO_THREAD)),
    };
    #[cfg(feature = "hub-slack")]
    {
        let root = super::slack_web::thread_root(&invocation.channel_id, &thread_ts).await?;
        let created_by_thread = root.as_ref().is_some_and(|message| {
            message["ts"].as_str() == Some(thread_ts.as_str())
                && message["user"].as_str() == Some(env.bot_user_id)
                && message["text"]
                    .as_str()
                    .is_some_and(|text| text.starts_with(THREAD_ROOT_PREFIX))
        });
        if !created_by_thread {
            return Ok(SlashReply::private(
                "❌ Only threads created by `/omo-thread` can be renamed.",
            ));
        }
        let title = text.trim();
        super::slack_web::update_message(
            &invocation.channel_id,
            &thread_ts,
            &thread_root_text(title),
        )
        .await?;
        Ok(SlashReply::public(format!(
            "🏷️ Thread renamed to `{title}`."
        )))
    }
    #[cfg(not(feature = "hub-slack"))]
    {
        let _ = (env, thread_ts);
        Ok(SlashReply::private(format!(
            "❌ Cannot rename to `{}`: this hub was built without Slack.",
            text.trim()
        )))
    }
}

async fn admission(env: &SlashEnv<'_>, invocation: &SlashInvocation) -> Result<Option<SlashReply>> {
    if invocation.user_id == env.owner_id {
        return Ok(None);
    }
    let paired: Option<String> = sqlx::query_scalar(
        "SELECT user_id FROM pairing_codes WHERE user_id = ? AND consumed_at IS NOT NULL",
    )
    .bind(&invocation.user_id)
    .fetch_optional(env.pool)
    .await?;
    if paired.is_some() {
        return Ok(None);
    }
    Ok(Some(SlashReply::private(UNAUTHORIZED)))
}

async fn remember(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query("INSERT INTO hub_meta(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
    Ok(())
}

fn model_key(key: &SessionKey) -> String {
    format!("model:{}", key.storage_key())
}

async fn status(env: &SlashEnv<'_>) -> Result<SlashReply> {
    let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(env.pool)
        .await?;
    let memories: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories")
        .fetch_one(env.pool)
        .await?;
    let ledger: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM action_proposals")
        .fetch_one(env.pool)
        .await?;
    Ok(SlashReply::public(format!(
        "Active sessions: {sessions}\nMemory entries: {memories}\nUptime: {}s\nLedger entries: {ledger}",
        uptime_secs()
    )))
}

async fn cron(env: &SlashEnv<'_>, action: &str) -> Result<SlashReply> {
    if action != "list" {
        return Ok(SlashReply::public("Usage: `/omo-cron [list]`"));
    }
    let lines = crate::schedule::health_lines(env.pool).await?;
    Ok(SlashReply::public(format!(
        "⏰ *Active OMO Cron Jobs* ({} total):\n{}",
        lines.len(),
        lines.join("\n")
    )))
}

async fn skill(env: &SlashEnv<'_>, action: &str, arg: Option<&str>) -> Result<SlashReply> {
    match action {
        "list" | "search" => {
            let names = crate::tools::list_skills(crate::tools::skills_root(&serde_json::json!({})));
            let shown: Vec<_> = names
                .into_iter()
                .filter(|name| arg.is_none_or(|query| name.contains(query)))
                .collect();
            let total = shown.len();
            if shown.is_empty() {
                return Ok(SlashReply::public(format!(
                    "📚 *Available OMO Skills* ({total} total):\n_(none)_"
                )));
            }
            Ok(SlashReply::public(format!(
                "📚 *Available OMO Skills* ({total} total):\n`{}`\n_(Use `/omo-skill read <skill_name>` to inspect)_",
                shown.join("`, `")
            )))
        }
        "read" => {
            let Some(name) = arg else {
                return Ok(SlashReply::private("Usage: `/omo-skill read <name>`"));
            };
            let root = crate::tools::skills_root(&serde_json::json!({}));
            let text = root.as_ref().and_then(|root| {
                crate::cron_exec::find_skill_file(root, name)
                    .and_then(|path| std::fs::read_to_string(path).ok())
            });
            Ok(SlashReply::public(
                text.unwrap_or_else(|| format!("No skill named `{name}`.")),
            ))
        }
        "run" => {
            let Some(name) = arg else {
                return Ok(SlashReply::private("Usage: `/omo-skill run <name>`"));
            };
            let root = crate::tools::skills_root(&serde_json::json!({}));
            let from_file = root.as_ref().and_then(|root| {
                crate::cron_exec::find_skill_file(root, name)
                    .and_then(|path| std::fs::read_to_string(path).ok())
            });
            let prompt: Option<String> = if from_file.is_some() {
                from_file
            } else {
                sqlx::query_scalar(
                    "SELECT system_prompt FROM bot_profiles WHERE name = ? OR bot_id = ?",
                )
                .bind(name)
                .bind(name)
                .fetch_optional(env.pool)
                .await?
            };
            let Some(prompt) = prompt.filter(|text| !text.trim().is_empty()) else {
                return Ok(SlashReply::public(format!("No skill named `{name}`.")));
            };
            env.control
                .execute(
                    None,
                    AgentOp::Send {
                        session_id: env.meta_session_id.to_owned(),
                        text: prompt,
                        request_id: format!("omo-skill-{name}"),
                    },
                )
                .await?;
            Ok(SlashReply::public(format!("Running skill `{name}`.")))
        }
        "approve" | "reject" => {
            let Some(id) = arg else {
                return Ok(SlashReply::private(format!(
                    "Usage: `/omo-skill {action} <name>`"
                )));
            };
            let decision = if action == "approve" { "allow" } else { "deny" };
            sqlx::query(
                "INSERT INTO action_precedents(signature, effect, decision, scope, origin, created_at, updated_at) VALUES(?,?,?,?,?,?,?)
                 ON CONFLICT(signature, scope) DO UPDATE SET decision = excluded.decision, updated_at = excluded.updated_at",
            )
            .bind(id)
            .bind("skill")
            .bind(decision)
            .bind("exact")
            .bind("slash")
            .bind(now_rfc3339())
            .bind(now_rfc3339())
            .execute(env.pool)
            .await?;
            Ok(SlashReply::public(format!(
                "Skill `{id}` marked {decision}."
            )))
        }
        "pending" | "review" => {
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT signature, decision FROM action_precedents WHERE effect = 'skill' ORDER BY updated_at DESC LIMIT 20",
            )
            .fetch_all(env.pool)
            .await?;
            if rows.is_empty() {
                return Ok(SlashReply::public("No skill reviews."));
            }
            let lines: Vec<_> = rows
                .into_iter()
                .map(|(id, decision)| format!("• `{id}` {decision}"))
                .collect();
            Ok(SlashReply::public(lines.join("\n")))
        }
        other => Ok(SlashReply::private(format!(
            "Usage: `/omo-skill <list|search|read|run|pending|review|approve|reject>`. `{other}` is not a skill action."
        ))),
    }
}

async fn memory(
    env: &SlashEnv<'_>,
    key: &SessionKey,
    action: &str,
    arg: Option<&str>,
) -> Result<SlashReply> {
    match action {
        "list" => {
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT id, content FROM memories WHERE session_key = ? ORDER BY updated_at DESC LIMIT 10",
            )
            .bind(key.storage_key())
            .fetch_all(env.pool)
            .await?;
            if rows.is_empty() {
                return Ok(SlashReply::public("No memories stored for this session."));
            }
            let lines: Vec<_> = rows
                .into_iter()
                .map(|(id, content)| format!("• `{id}`: {content}"))
                .collect();
            Ok(SlashReply::public(lines.join("\n")))
        }
        "approve" | "reject" => {
            let Some(id) = arg else {
                return Ok(SlashReply::private("Usage: `/omo-memory approve <id>`"));
            };
            let decision = if action == "approve" { "allow" } else { "deny" };
            sqlx::query(
                "INSERT INTO action_precedents(signature, effect, decision, scope, origin, created_at, updated_at) VALUES(?,?,?,?,?,?,?)",
            )
            .bind(id)
            .bind("memory")
            .bind(decision)
            .bind("exact")
            .bind("slash")
            .bind(now_rfc3339())
            .bind(now_rfc3339())
            .execute(env.pool)
            .await?;
            Ok(SlashReply::public(format!(
                "Memory `{id}` marked {decision}."
            )))
        }
        _ => Ok(SlashReply::private(
            "Usage: `/omo-memory [list|approve|reject]`",
        )),
    }
}

async fn undo(env: &SlashEnv<'_>, key: &SessionKey) -> Result<SlashReply> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, content FROM messages WHERE session_key = ? AND role = 'user' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(key.storage_key())
    .fetch_optional(env.pool)
    .await?;
    let Some((id, content)) = row else {
        return Ok(SlashReply::public("No conversation history to undo."));
    };
    sqlx::query("DELETE FROM messages WHERE id = ?")
        .bind(&id)
        .execute(env.pool)
        .await?;
    Ok(SlashReply::public(format!(
        "↩️ *Undid last message*:\n• *User:* `{content}`"
    )))
}

async fn retry(env: &SlashEnv<'_>, key: &SessionKey) -> Result<SlashReply> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT content FROM messages WHERE session_key = ? AND role = 'user' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(key.storage_key())
    .fetch_optional(env.pool)
    .await?;
    let Some((content,)) = row else {
        return Ok(SlashReply::private(
            "No previous user message found to retry.",
        ));
    };
    env.control
        .execute(
            None,
            AgentOp::Send {
                session_id: env.meta_session_id.to_owned(),
                text: content.clone(),
                request_id: format!("omo-retry-{}", key.storage_key()),
            },
        )
        .await?;
    Ok(SlashReply::private(format!(
        "🔄 Retrying last message: `{content}`"
    )))
}

async fn deny(env: &SlashEnv<'_>, key: &SessionKey, reason: Option<&str>) -> Result<SlashReply> {
    let id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM action_proposals WHERE session_key = ? AND state = 'pending' ORDER BY id DESC LIMIT 1",
    )
    .bind(key.storage_key())
    .fetch_optional(env.pool)
    .await?;
    let Some(id) = id else {
        return Ok(SlashReply::private("No pending approval found to deny."));
    };
    crate::ledger::resolve(env.pool, id, false, reason).await?;
    Ok(SlashReply::public(
        match reason.map(str::trim).filter(|text| !text.is_empty()) {
            Some(reason) => format!("❌ Denied pending command approval: `{reason}`"),
            None => "❌ Denied pending command approval.".into(),
        },
    ))
}

async fn yolo(env: &SlashEnv<'_>, key: &SessionKey, mode: Option<&str>) -> Result<SlashReply> {
    let on = match mode.map(|value| value.to_ascii_lowercase()) {
        None => !crate::ledger::yolo(env.pool, &key.storage_key()).await?,
        Some(value) if matches!(value.as_str(), "on" | "true" | "1") => true,
        Some(value) if matches!(value.as_str(), "off" | "false" | "0") => false,
        Some(_) => return Ok(SlashReply::private("Usage: `/omo-yolo [on|off]`")),
    };
    crate::ledger::set_yolo(env.pool, &key.storage_key(), on).await?;
    Ok(SlashReply::private(if on {
        "YOLO is on for this session. Tainted actions still wait."
    } else {
        "YOLO is off for this session."
    }))
}

async fn pair(env: &SlashEnv<'_>, invocation: &SlashInvocation, code: &str) -> Result<SlashReply> {
    if invocation.user_id != env.owner_id {
        return Ok(SlashReply::private(
            "❌ You are not authorized to approve pairing codes.",
        ));
    }
    let user = crate::chat::discord::consume_pairing(env.pool, code).await?;
    Ok(match user {
        Some(user_id) => SlashReply::public(format!(
            "✅ Successfully paired and authorized user <@{user_id}>!"
        )),
        None => {
            crate::chat::discord::note_pairing_failure(env.pool, code).await?;
            SlashReply::private("❌ Invalid pairing code.")
        }
    })
}

async fn sessions(env: &SlashEnv<'_>, command: SessionsCommand) -> Result<SlashReply> {
    match command {
        SessionsCommand::Overview => {
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT session_key, channel_id FROM sessions ORDER BY updated_at DESC LIMIT 20",
            )
            .fetch_all(env.pool)
            .await?;
            if rows.is_empty() {
                return Ok(SlashReply::public("No sessions."));
            }
            let lines: Vec<_> = rows
                .into_iter()
                .map(|(key, channel)| format!("• `{channel}` {key}"))
                .collect();
            Ok(SlashReply::public(lines.join("\n")))
        }
        SessionsCommand::Read { target, limit } => {
            let limit = limit.unwrap_or(5) as i64;
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT role, content FROM messages WHERE session_key = ? ORDER BY created_at DESC LIMIT ?",
            )
            .bind(&target)
            .bind(limit)
            .fetch_all(env.pool)
            .await?;
            if rows.is_empty() {
                return Ok(SlashReply::public(format!("No messages for `{target}`.")));
            }
            let lines: Vec<_> = rows
                .into_iter()
                .rev()
                .map(|(role, content)| format!("*{role}:* {content}"))
                .collect();
            Ok(SlashReply::public(lines.join("\n")))
        }
        SessionsCommand::Send { target, text, mode } => {
            let body = if mode == "steer" {
                format!("[steer] {text}")
            } else {
                text.clone()
            };
            env.control
                .execute(
                    None,
                    AgentOp::Send {
                        session_id: target,
                        text: body,
                        request_id: format!("omo-sessions-{mode}"),
                    },
                )
                .await?;
            Ok(SlashReply::private(format!("Sent ({mode}): {text}")))
        }
        SessionsCommand::Stop { target } => {
            env.control
                .execute(
                    None,
                    AgentOp::Stop {
                        session_id: target,
                        request_id: "omo-sessions-stop".into(),
                    },
                )
                .await?;
            Ok(SlashReply::private("Stopped that session."))
        }
        SessionsCommand::Answer {
            target,
            text,
            request_id,
        } => answer_session(env, target, text, request_id).await,
        SessionsCommand::Watch { target } => {
            sqlx::query(
                "INSERT OR IGNORE INTO session_watches(target, added_at, reason) VALUES(?,?, 'slash')",
            )
            .bind(&target)
            .bind(now_rfc3339())
            .execute(env.pool)
            .await?;
            Ok(SlashReply::public(format!("Watching `{target}`.")))
        }
        SessionsCommand::Unwatch { target } => {
            sqlx::query("DELETE FROM session_watches WHERE target = ?")
                .bind(&target)
                .execute(env.pool)
                .await?;
            Ok(SlashReply::public(format!("Stopped watching `{target}`.")))
        }
    }
}

async fn compress(env: &SlashEnv<'_>, key: &SessionKey) -> Result<SlashReply> {
    let Some(model) = env.model else {
        return Ok(SlashReply::public(
            "❌ Compression requires a configured LLM client; history left untouched.",
        ));
    };
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM messages WHERE session_key = ? ORDER BY created_at, id",
    )
    .bind(key.storage_key())
    .fetch_all(env.pool)
    .await?;
    if rows.is_empty() {
        return Ok(SlashReply::public("No history to compress."));
    }
    let transcript = rows
        .iter()
        .map(|(role, content)| format!("{role}: {content}"))
        .collect::<Vec<_>>()
        .join("\n");
    let summary = model
        .complete(&format!(
            "Summarize this session in one paragraph:\n{transcript}"
        ))
        .await?;
    sqlx::query("DELETE FROM messages WHERE session_key = ?")
        .bind(key.storage_key())
        .execute(env.pool)
        .await?;
    sqlx::query(
        "INSERT INTO messages(id, session_key, role, content, created_at) VALUES(?,?,?,?,?)",
    )
    .bind(format!("compress-{}", uuid::Uuid::new_v4()))
    .bind(key.storage_key())
    .bind("system")
    .bind(&summary)
    .bind(now_rfc3339())
    .execute(env.pool)
    .await?;
    Ok(SlashReply::public(format!("Compressed history: {summary}")))
}

async fn answer_session(
    env: &SlashEnv<'_>,
    target: String,
    text: String,
    request_id: Option<String>,
) -> Result<SlashReply> {
    let read = env
        .control
        .execute(
            None,
            AgentOp::Read {
                session_id: target.clone(),
            },
        )
        .await
        .ok();
    let prompt_id = request_id.or_else(|| {
        read.as_ref()
            .and_then(|value| value.pointer("/session/pending/id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let Some(prompt_id) = prompt_id else {
        return Ok(SlashReply::private("No live prompt on that session."));
    };
    let revision = read
        .as_ref()
        .and_then(|value| value.pointer("/session/screenRevision"))
        .and_then(Value::as_str)
        .unwrap_or("slash")
        .to_owned();
    env.control
        .execute(
            None,
            AgentOp::Answer {
                session_id: target,
                prompt_id,
                screen_revision: revision,
                answer: serde_json::json!({ "customText": text }),
                request_id: "omo-sessions-answer".into(),
            },
        )
        .await?;
    Ok(SlashReply::private(format!(
        "Answered the live prompt: {text}"
    )))
}

pub fn invocation_from_payload(payload: &Value) -> SlashInvocation {
    let field = |name: &str| {
        payload
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let optional = |name: &str| {
        payload
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    SlashInvocation {
        command: field("command"),
        text: field("text"),
        user_id: field("user_id"),
        channel_id: field("channel_id"),
        team_id: field("team_id"),
        thread_ts: optional("thread_ts"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;

    fn invocation(command: &str, text: &str) -> SlashInvocation {
        SlashInvocation {
            command: command.into(),
            text: text.into(),
            user_id: "UOWNER".into(),
            channel_id: "C1".into(),
            team_id: "T1".into(),
            thread_ts: None,
        }
    }

    #[tokio::test]
    async fn yolo_cron_stop_and_huddle_follow_the_gateway() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, next_run_at) VALUES('daily','0 9 * * *','{}','2026-10-23T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        let control = RecordingControl::default();
        let env = SlashEnv {
            pool: &pool,
            owner_id: "UOWNER",
            bot_user_id: "B1",
            control: &control,
            meta_session_id: "meta",
            huddle_enabled: false,
            model: None,
        };
        let yolo = execute(&env, &invocation("/omo-yolo", "on")).await.unwrap();
        assert!(yolo.ephemeral);
        assert!(yolo.text.contains("YOLO is on"));
        let listed = execute(&env, &invocation("/omo-cron", "list"))
            .await
            .unwrap();
        assert!(listed.text.contains("daily"));
        let stopped = execute(&env, &invocation("/omo-stop", "")).await.unwrap();
        assert!(stopped.text.contains("Stopped"));
        assert!(matches!(
            control.ops.lock().unwrap().last(),
            Some((_, AgentOp::Stop { session_id, .. })) if session_id == "meta"
        ));
        let huddle = execute(&env, &invocation("/omo-huddle", "status"))
            .await
            .unwrap();
        assert!(huddle.text.contains("huddles are off"));
        let stranger = SlashInvocation {
            user_id: "USTRANGER".into(),
            ..invocation("/omo-status", "")
        };
        let refused = execute(&env, &stranger).await.unwrap();
        assert!(refused.text.contains("not authorized"));
        let line = huddle_line(&HuddleAction::Join("https://slack.example/huddle".into()));
        assert!(line.contains("\"op\":\"join\""));
    }

    #[test]
    fn explicit_thread_accepts_ts_and_permalinks() {
        assert_eq!(
            explicit_thread("1700000000.000100 Renamed", "C1"),
            Some(("1700000000.000100".into(), "Renamed".into()))
        );
        assert_eq!(
            explicit_thread(
                "<https://acme.slack.com/archives/C1/p1700000000000100> Renamed",
                "C1"
            ),
            Some(("1700000000.000100".into(), "Renamed".into()))
        );
        assert_eq!(
            explicit_thread(
                "https://acme.slack.com/archives/C1/p1700000000000100?thread_ts=1 Renamed",
                "C1"
            ),
            Some(("1700000000.000100".into(), "Renamed".into()))
        );
        assert_eq!(explicit_thread("Renamed", "C1"), None);
        assert_eq!(
            explicit_thread(
                "https://acme.slack.com/archives/C2/p1700000000000100 X",
                "C1"
            ),
            None
        );
        assert_eq!(explicit_thread("1700.1 ", "C1"), None);
    }

    #[tokio::test]
    async fn thread_and_title_refuse_dms_nested_threads_and_unnamed_threads() {
        let pool = crate::storage::memory().await.unwrap();
        let control = RecordingControl::default();
        let env = SlashEnv {
            pool: &pool,
            owner_id: "UOWNER",
            bot_user_id: "B1",
            control: &control,
            meta_session_id: "meta",
            huddle_enabled: false,
            model: None,
        };
        let dm = SlashInvocation {
            channel_id: "D1".into(),
            ..invocation("/omo-thread", "plan")
        };
        assert!(execute(&env, &dm)
            .await
            .unwrap()
            .text
            .contains("not in DMs"));
        let nested = SlashInvocation {
            thread_ts: Some("1.1".into()),
            ..invocation("/omo-thread", "plan")
        };
        assert!(execute(&env, &nested)
            .await
            .unwrap()
            .text
            .contains("inside an existing thread"));
        let unnamed = execute(&env, &invocation("/omo-title", "New"))
            .await
            .unwrap();
        assert!(unnamed.ephemeral);
        assert_eq!(unnamed.text, TITLE_NO_THREAD);
    }

    /// e2e against a mock Slack Web API: `/omo-thread` posts the root, stores the thread
    /// session and links it; `/omo-title` renames only a root the bot made with `/omo-thread`.
    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn e2e_omo_thread_creates_a_thread_and_omo_title_renames_it() {
        let (base, calls) = crate::chat::slack_web::mock::serve("B1").await;
        let pool = crate::storage::memory().await.unwrap();
        let control = RecordingControl::default();
        let env = SlashEnv {
            pool: &pool,
            owner_id: "UOWNER",
            bot_user_id: "B1",
            control: &control,
            meta_session_id: "meta",
            huddle_enabled: false,
            model: None,
        };
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            let created = execute(&env, &invocation("/omo-thread", "Launch <plan>"))
                .await
                .unwrap();
            assert!(!created.ephemeral);
            assert!(created.text.starts_with("🧵 Created thread <https://acme.slack.com/archives/C1/p"), "{}", created.text);
            assert!(created.text.contains("|Launch &lt;plan&gt;>"), "{}", created.text);
            let (method, body) = calls.lock().unwrap()[0].clone();
            assert_eq!(method, "chat.postMessage");
            assert_eq!(body["channel"], "C1");
            assert_eq!(body["text"], "🧵 *Launch <plan>*");
            let root_ts = "1700000000.000001";
            let stored: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sessions WHERE platform = 'slack' AND channel_id = 'C1' AND thread_id = ?",
            )
            .bind(root_ts)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(stored, 1, "replies in the new thread reach the bot without a mention");

            let renamed = execute(&env, &invocation("/omo-title", &format!("{root_ts} Shipped")))
                .await
                .unwrap();
            assert_eq!(renamed.text, "🏷️ Thread renamed to `Shipped`.");
            let update = calls
                .lock()
                .unwrap()
                .iter()
                .find(|(method, _)| method == "chat.update")
                .cloned()
                .expect("chat.update");
            assert_eq!(update.1["ts"], root_ts);
            assert_eq!(update.1["text"], "🧵 *Shipped*");

            let foreign = execute(&env, &invocation("/omo-title", "1699999999.000001 Nope"))
                .await
                .unwrap();
            assert!(foreign.text.contains("Only threads created by `/omo-thread`"));
        })
        .await;
    }

    #[test]
    fn a_top_level_stop_picks_the_one_live_thread_in_the_channel() {
        let top = SlashInvocation {
            channel_id: "C-live".into(),
            ..invocation("/omo-stop", "")
        };
        let key = session_key(&top, "B1");
        assert_eq!(
            live_turn_target(&top, &key),
            Err("이 채널에 진행 중인 대화가 없습니다.")
        );
        let thread_key = SessionKey {
            thread_id: Some("1.1".into()),
            ..key.clone()
        };
        let guard = super::super::turns::begin(&thread_key.storage_key(), "C-live");
        assert_eq!(
            live_turn_target(&top, &key),
            Ok(Some(thread_key.storage_key()))
        );
        let other = super::super::turns::begin("other-live-thread", "C-live");
        assert!(
            live_turn_target(&top, &key).is_err(),
            "two live threads are ambiguous"
        );
        drop((guard, other));
        let in_thread = SlashInvocation {
            channel_id: "C-live".into(),
            thread_ts: Some("1.1".into()),
            ..invocation("/omo-stop", "")
        };
        assert_eq!(
            live_turn_target(&in_thread, &session_key(&in_thread, "B1")),
            Ok(None)
        );
    }
}
