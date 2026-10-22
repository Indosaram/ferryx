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
    let mut child = tokio::process::Command::new("bun")
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
        SlackCommand::Stop => {
            env.control
                .execute(
                    None,
                    AgentOp::Stop {
                        session_id: env.meta_session_id.to_owned(),
                        request_id: format!("omo-stop-{}", invocation.user_id),
                    },
                )
                .await?;
            remember(env.pool, &format!("suspended:{}", key.storage_key()), "1").await?;
            Ok(SlashReply::private(
                "🛑 Stopped active turn and marked session suspended.",
            ))
        }
        SlackCommand::Status => status(env).await,
        SlackCommand::Tools => Ok(SlashReply::public(format!(
            "Tools: {}",
            crate::context::TOOLS.join(", ")
        ))),
        SlackCommand::Skill { action, arg } => skill(env, &action, arg.as_deref()).await,
        SlackCommand::Memory { action, arg } => memory(env, &key, &action, arg.as_deref()).await,
        SlackCommand::Cron { action } => cron(env, &action).await,
        SlackCommand::Steer(text) => {
            let prompt = format!("[steer] {text}");
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
        SlackCommand::Undo => undo(env, &key).await,
        SlackCommand::Retry => retry(env, &key).await,
        SlackCommand::Compress => compress(env, &key).await,
        SlackCommand::Title(text) => {
            remember(env.pool, &format!("title:{}", key.storage_key()), &text).await?;
            Ok(SlashReply::public(format!("Thread title set to `{text}`.")))
        }
        SlackCommand::Thread { name, message } => Ok(SlashReply::public(match message {
            Some(message) => format!("Thread `{name}`: {message}"),
            None => format!("Thread `{name}`."),
        })),
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
}
