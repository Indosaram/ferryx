//! Discord surfaces omo handles beside the gateway frame: forum titles, forwarded snapshots,
//! the receive watchdog, slash-command registration, and session commands.
use serde_json::{json, Value};

pub const RECEIVE_WATCHDOG_DEFAULT_SECS: u64 = 1200;
const ZOMBIE_CEILING: u64 = 3;

pub fn receive_watchdog_threshold_from(raw: Option<&str>) -> u64 {
    raw.and_then(|raw| raw.trim().parse().ok())
        .unwrap_or(RECEIVE_WATCHDOG_DEFAULT_SECS)
}

pub fn receive_watchdog_threshold() -> u64 {
    receive_watchdog_threshold_from(
        std::env::var("OMON_DISCORD_RECEIVE_WATCHDOG_SECS")
            .ok()
            .as_deref(),
    )
}

/// Silence past the threshold restarts a dead runner immediately. A runner that still
/// looks healthy restarts only after three thresholds, which is omo's zombie ceiling.
pub fn watchdog_should_restart(
    silence_secs: u64,
    threshold_secs: u64,
    runner_healthy: bool,
) -> bool {
    if threshold_secs == 0 || silence_secs < threshold_secs {
        return false;
    }
    !runner_healthy || silence_secs >= threshold_secs.saturating_mul(ZOMBIE_CEILING)
}

pub fn derive_forum_post_title(content: &str) -> String {
    let first_line = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("New Discussion");
    let no_mentions = mention_re().replace_all(first_line, "");
    let stripped = no_mentions
        .trim_start_matches(|ch: char| {
            matches!(ch, '#' | '>' | '*' | '_' | '`' | '~') || ch.is_whitespace()
        })
        .trim_end_matches(|ch: char| matches!(ch, '*' | '_' | '`' | '~') || ch.is_whitespace());
    let collapsed = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        "New Discussion".to_owned()
    } else if trimmed.chars().count() > 100 {
        let capped: String = trimmed.chars().take(97).collect();
        format!("{capped}...")
    } else if trimmed.chars().count() < 2 {
        format!("{trimmed} Discussion")
    } else {
        trimmed.to_owned()
    }
}

fn mention_re() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"<@!?[0-9]+>|<#[0-9]+>|<@&[0-9]+>").unwrap())
}

/// `(forwarded text, attachment filenames)` from Discord `message_snapshots`.
pub fn extract_forwarded_snapshots(snapshots: &Value) -> (String, Vec<String>) {
    let Some(items) = snapshots.as_array() else {
        return (String::new(), Vec::new());
    };
    let mut parts = Vec::new();
    let mut files = Vec::new();
    for snapshot in items {
        let mut part = snapshot
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        let names: Vec<&str> = snapshot
            .get("attachments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("filename").and_then(Value::as_str))
            .collect();
        if !names.is_empty() {
            let summary = if names.len() == 1 {
                format!("[Attachment: {}]", names[0])
            } else {
                format!("[Attachments: {}]", names.join(", "))
            };
            if part.is_empty() {
                part = summary;
            } else {
                part = format!("{part} {summary}");
            }
            files.extend(names.into_iter().map(str::to_owned));
        }
        if !part.is_empty() {
            parts.push(part);
        }
    }
    if parts.is_empty() {
        (String::new(), files)
    } else {
        (format!("[Forwarded]\n{}", parts.join("\n")), files)
    }
}

pub fn is_forum_message(data: &Value) -> bool {
    data.get("channel_type").and_then(Value::as_i64) == Some(15)
        || data.pointer("/parent_channel_type").and_then(Value::as_i64) == Some(15)
}

/// Global slash commands omo registers: skill through pair, plus `sessions` subcommands.
pub fn application_commands() -> Value {
    let string = |name, description| json!({"type": 3, "name": name, "description": description, "required": false});
    let sub = |name, description| json!({"type": 1, "name": name, "description": description});
    json!([
        {"name": "skill", "type": 1, "description": "Execute or inspect an OMO skill", "options": [string("action", "list, search, read, run, pending, review, approve, reject"), string("name_or_query", "Skill name, query, or pending ID")]},
        {"name": "skills", "type": 1, "description": "Execute, inspect, or manage capability skills", "options": [string("action", "Skill action"), string("name_or_query", "Skill name or query")]},
        {"name": "memory", "type": 1, "description": "Inspect or review persistent memories", "options": [string("action", "Memory action"), string("name_or_query", "Memory id or query")]},
        {"name": "cron", "type": 1, "description": "Inspect or manage background cron jobs", "options": [string("action", "Cron action")]},
        {"name": "model", "type": 1, "description": "Switch the model used by this Discord session", "options": [string("model", "Model id")]},
        {"name": "reset", "type": 1, "description": "Clear conversation context and persistent memory for this session"},
        {"name": "stop", "type": 1, "description": "Stop the active agent turn for this Discord session"},
        {"name": "status", "type": 1, "description": "Show gateway health and persistence statistics"},
        {"name": "tools", "type": 1, "description": "List configured tools and MCP endpoints"},
        {"name": "steer", "type": 1, "description": "Inject steering guidance into the session", "options": [string("text", "Guidance")]},
        {"name": "undo", "type": 1, "description": "Remove the most recent exchange"},
        {"name": "retry", "type": 1, "description": "Re-run the last user message"},
        {"name": "compress", "type": 1, "description": "Compress conversation history into a concise summary"},
        {"name": "title", "type": 1, "description": "Set or rename the current Discord thread title", "options": [string("name", "Title")]},
        {"name": "thread", "type": 1, "description": "Create a new thread and optionally start a session in it", "options": [string("name", "Thread name")]},
        {"name": "deny", "type": 1, "description": "Deny a pending dangerous command approval", "options": [string("reason", "Reason")]},
        {"name": "yolo", "type": 1, "description": "Toggle YOLO mode for this session", "options": [string("mode", "on or off")]},
        {"name": "pair", "type": 1, "description": "Authorize a new user via their one-time pairing code", "options": [string("code", "Pairing code")]},
        {"name": "sessions", "type": 1, "description": "Session control", "options": [
            sub("overview", "List sessions"),
            sub("read", "Read a session"),
            sub("send", "Send text to a session"),
            sub("stop", "Stop a session"),
            sub("answer", "Answer a session prompt"),
            sub("watch", "Watch a session"),
            sub("unwatch", "Stop watching a session")
        ]},
        {"name": "voice", "type": 1, "description": "Join or leave a Discord voice channel", "options": [
            {"type": 1, "name": "join", "description": "Join your voice channel, listen, and speak the agent reply.", "options": [
                {"type": 7, "name": "channel", "description": "Voice channel to join. Defaults to the channel you are in.", "channel_types": [2, 13], "required": false}
            ]},
            {"type": 1, "name": "leave", "description": "Leave the voice channel in this server."}
        ]}
    ])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceGatewayUpdate {
    Ready {
        user_id: u64,
    },
    State {
        guild_id: u64,
        user_id: u64,
        session_id: String,
        channel_id: Option<u64>,
    },
    Server {
        guild_id: u64,
        endpoint: String,
        token: String,
    },
}

pub fn voice_subcommand(data: &Value) -> Option<&str> {
    data.pointer("/data/options/0/name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
}

pub fn interaction_channel(data: &Value) -> Option<String> {
    data.get("channel_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

pub fn interaction_guild(data: &Value) -> Option<u64> {
    data.get("guild_id").and_then(snowflake)
}

pub fn interaction_user(data: &Value) -> Option<u64> {
    data.pointer("/member/user/id")
        .or_else(|| data.pointer("/user/id"))
        .and_then(snowflake)
}

pub fn voice_channel_option(data: &Value) -> Option<u64> {
    let options = data.pointer("/data/options/0/options")?.as_array()?;
    options.iter().find_map(|item| {
        if item.get("name").and_then(Value::as_str) == Some("channel") {
            item.get("value").and_then(snowflake)
        } else {
            None
        }
    })
}

/// Gateway opcode 4. `channel_id: null` leaves the voice channel.
pub fn voice_state_payload(guild_id: u64, channel_id: Option<u64>) -> Value {
    json!({
        "op": 4,
        "d": {
            "guild_id": guild_id.to_string(),
            "channel_id": channel_id.map(|id| id.to_string()),
            "self_mute": false,
            "self_deaf": false,
        }
    })
}

pub fn voice_gateway_update(frame: &Value) -> Option<VoiceGatewayUpdate> {
    if frame.get("op").and_then(Value::as_i64) != Some(0) {
        return None;
    }
    let kind = frame.get("t").and_then(Value::as_str)?;
    let data = frame.get("d")?;
    match kind {
        "READY" => Some(VoiceGatewayUpdate::Ready {
            user_id: snowflake(data.pointer("/user/id")?)?,
        }),
        "VOICE_STATE_UPDATE" => Some(VoiceGatewayUpdate::State {
            guild_id: snowflake(data.get("guild_id")?)?,
            user_id: snowflake(data.get("user_id")?)?,
            session_id: data
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            channel_id: data.get("channel_id").and_then(snowflake),
        }),
        "VOICE_SERVER_UPDATE" => {
            let endpoint = data
                .get("endpoint")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let token = data
                .get("token")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            if endpoint.is_empty() || token.is_empty() {
                return None;
            }
            Some(VoiceGatewayUpdate::Server {
                guild_id: snowflake(data.get("guild_id")?)?,
                endpoint,
                token,
            })
        }
        _ => None,
    }
}

fn snowflake(value: &Value) -> Option<u64> {
    match value {
        Value::String(text) => text.parse().ok().filter(|id| *id != 0),
        Value::Number(number) => number.as_u64().filter(|id| *id != 0),
        _ => None,
    }
}

pub fn command_name(data: &Value) -> Option<&str> {
    data.pointer("/data/name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
}

pub fn session_overview(rows: &[(String, String)]) -> String {
    if rows.is_empty() {
        return "No sessions.".into();
    }
    let mut lines = vec![format!("Sessions ({})", rows.len())];
    for (key, channel) in rows.iter().take(20) {
        lines.push(format!("• `{key}` {channel}"));
    }
    lines.join("\n")
}

pub fn session_command_text(
    subcommand: &str,
    rows: &[(String, String)],
    target: Option<&str>,
) -> String {
    match subcommand {
        "overview" => session_overview(rows),
        "read" => match target {
            Some(target) => rows
                .iter()
                .find(|(key, _)| key == target || key.ends_with(target))
                .map(|(key, channel)| format!("{key} channel {channel}"))
                .unwrap_or_else(|| format!("No session `{target}`.")),
            None => "Usage: sessions read <target>".into(),
        },
        "send" | "stop" | "answer" | "watch" | "unwatch" => match target {
            Some(target) if rows.iter().any(|(key, _)| key == target) => {
                format!("{subcommand} `{target}`")
            }
            Some(target) => format!("No session `{target}`."),
            None => format!("Usage: sessions {subcommand} <target>"),
        },
        other => format!("Unknown sessions command `{other}`."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchdog_forum_forwards_and_commands_match_omo() {
        assert!(!watchdog_should_restart(10, 1200, false));
        assert!(watchdog_should_restart(1200, 1200, false));
        assert!(!watchdog_should_restart(1200, 1200, true));
        assert!(watchdog_should_restart(3600, 1200, true));
        assert!(!watchdog_should_restart(9999, 0, false));
        assert_eq!(
            derive_forum_post_title("<@123> # Hello   forum"),
            "Hello forum"
        );
        assert_eq!(derive_forum_post_title("  "), "New Discussion");
        let (text, files) = extract_forwarded_snapshots(&json!([{
            "content": "quoted",
            "attachments": [{"filename": "shot.png"}]
        }]));
        assert!(text.starts_with("[Forwarded]\nquoted"));
        assert_eq!(files, vec!["shot.png".to_owned()]);
        let commands = application_commands();
        let names: Vec<&str> = commands
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item.get("name").and_then(Value::as_str))
            .collect();
        assert!(names.contains(&"sessions"));
        assert!(names.contains(&"pair"));
        assert!(names.contains(&"stop"));
        assert!(names.contains(&"voice"));
        let voice = commands
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == "voice")
            .unwrap();
        assert_eq!(voice["options"][0]["name"], "join");
        assert_eq!(voice["options"][1]["name"], "leave");
        let payload = voice_state_payload(9, Some(42));
        assert_eq!(payload["op"], 4);
        assert_eq!(payload["d"]["channel_id"], "42");
        assert!(voice_state_payload(9, None)["d"]["channel_id"].is_null());
        let ready = voice_gateway_update(&json!({"op":0,"t":"READY","d":{"user":{"id":"7"}}}));
        assert_eq!(ready, Some(VoiceGatewayUpdate::Ready { user_id: 7 }));
        let state = voice_gateway_update(&json!({
            "op": 0,
            "t": "VOICE_STATE_UPDATE",
            "d": {"guild_id":"9","user_id":"3","session_id":"sess","channel_id":"42"}
        }));
        assert_eq!(
            state,
            Some(VoiceGatewayUpdate::State {
                guild_id: 9,
                user_id: 3,
                session_id: "sess".into(),
                channel_id: Some(42),
            })
        );
        let server = voice_gateway_update(&json!({
            "op": 0,
            "t": "VOICE_SERVER_UPDATE",
            "d": {"guild_id":"9","token":"tok","endpoint":"example.discord.media:443"}
        }));
        assert_eq!(
            server,
            Some(VoiceGatewayUpdate::Server {
                guild_id: 9,
                endpoint: "example.discord.media:443".into(),
                token: "tok".into(),
            })
        );
        let subs = &commands
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == "sessions")
            .unwrap()["options"];
        assert_eq!(subs.as_array().unwrap().len(), 7);
        assert_eq!(
            session_command_text("overview", &[("host:1".into(), "C1".into())], None),
            "Sessions (1)\n• `host:1` C1"
        );
    }
}
