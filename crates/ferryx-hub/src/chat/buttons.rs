//! Slack `omon:btn:<namespace>:<action>:<arg>` clicks. The command comes from
//! `~/.ferryx/hub/button-actions.json` (override `OMON_BUTTON_ACTIONS_FILE`). The environment is cleared
//! so gateway credentials are not copied in, then the click's user, channel, and message are added.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::error::{HubError, Result};

pub const BUTTON_PREFIX: &str = "omon:btn:";
const NOT_AUTHORIZED: (&str, &str) = ("이 버튼을 누를 권한이 없습니다.", "You are not allowed to press this button.");
const DEFAULT_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonClick {
    pub namespace: String,
    pub action: String,
    pub arg: String,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ButtonActionConfig {
    pub command: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub fn parse_button_custom_id(custom_id: &str) -> Option<ButtonClick> {
    let mut parts = custom_id.strip_prefix(BUTTON_PREFIX)?.split(':');
    let (namespace, action, arg) = (parts.next()?, parts.next()?, parts.next()?);
    let token = parts.next();
    if parts.next().is_some()
        || !is_name(namespace)
        || !is_name(action)
        || !is_arg(arg)
        || token.is_some_and(|token| token.is_empty() || token.starts_with('-'))
    {
        return None;
    }
    Some(ButtonClick {
        namespace: namespace.to_owned(),
        action: action.to_owned(),
        arg: arg.to_owned(),
        token: token.map(str::to_owned),
    })
}

fn is_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn is_arg(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-')
}

pub fn button_value(action: &Value) -> Option<&str> {
    ["value", "action_id"]
        .into_iter()
        .filter_map(|key| action.get(key).and_then(Value::as_str))
        .find(|value| value.starts_with(BUTTON_PREFIX))
}

pub fn button_actions_path() -> PathBuf {
    if let Some(path) = crate::hub_env::var_os("OMON_BUTTON_ACTIONS_FILE") {
        return PathBuf::from(path);
    }
    crate::hub_env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".ferryx/hub/button-actions.json")
}

pub fn load_button_action(path: &Path, namespace: &str) -> Result<Option<ButtonActionConfig>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(HubError::message(format!(
                "cannot read {}: {error}",
                path.display()
            )))
        }
    };
    let mut actions: HashMap<String, ButtonActionConfig> = serde_json::from_str(&text)
        .map_err(|error| HubError::message(format!("invalid {}: {error}", path.display())))?;
    Ok(actions
        .remove(namespace)
        .filter(|config| !config.command.is_empty()))
}

pub async fn run_button_action(
    config: &ButtonActionConfig,
    click: &ButtonClick,
    extra_env: &[(String, String)],
) -> Result<ActionOutcome> {
    let Some((program, base_args)) = config.command.split_first() else {
        return Err(HubError::message("button action command is empty"));
    };
    let mut command = crate::child_env::command(program);
    command
        .args(base_args)
        .arg(&click.action)
        .arg(&click.arg)
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(token) = &click.token {
        command.arg(token);
    }
    command.env("PATH", crate::child_env::child_path());
    for key in ["HOME", "TMPDIR", "LANG", "TZ"] {
        if let Some(value) = crate::hub_env::var_os(key) {
            command.env(key, value);
        }
    }
    for (key, value) in extra_env {
        command.env(key, value);
    }
    if let Some(cwd) = &config.cwd {
        command.current_dir(cwd);
    }
    let timeout =
        std::time::Duration::from_secs(config.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS));
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| {
            HubError::message(format!(
                "button action timed out after {}s",
                timeout.as_secs()
            ))
        })?
        .map_err(|error| HubError::message(error.to_string()))?;
    Ok(ActionOutcome {
        success: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Runs every `omon:btn:` action. A click from anyone other than `owner_id` runs nothing.
pub async fn run_button_clicks(
    payload: &Value,
    owner_id: &str,
    actions_path: &Path,
) -> Result<Vec<String>> {
    if payload.get("type").and_then(Value::as_str) != Some("block_actions") {
        return Ok(Vec::new());
    }
    let user = payload
        .pointer("/user/id")
        .and_then(Value::as_str)
        .unwrap_or("");
    // omo-gateway: anyone in SLACK_ALLOWED_USERS (default: the owner) may press a button.
    if user.is_empty()
        || !crate::config::slack_allowed_users(owner_id)
            .iter()
            .any(|id| id == user)
    {
        return Err(HubError::message(crate::locale::text(NOT_AUTHORIZED.0, NOT_AUTHORIZED.1)));
    }
    let channel = payload
        .pointer("/channel/id")
        .or_else(|| payload.pointer("/container/channel_id"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let message_ts = payload
        .pointer("/container/message_ts")
        .or_else(|| payload.pointer("/message/ts"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut lines = Vec::new();
    let actions = payload
        .get("actions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for action in &actions {
        let Some(value) = button_value(action) else {
            continue;
        };
        let Some(click) = parse_button_custom_id(value) else {
            tracing::warn!(value, "ignoring malformed Slack button value");
            continue;
        };
        let Some(config) = load_button_action(actions_path, &click.namespace)? else {
            let namespace = &click.namespace;
            lines.push(if crate::locale::current().is_korean() {
                format!("`{namespace}` 버튼 동작이 설정되어 있지 않습니다.")
            } else {
                format!("No action is configured for the `{namespace}` button.")
            });
            continue;
        };
        let extra = vec![
            ("OMON_BUTTON_USER_ID".into(), user.to_owned()),
            ("OMON_BUTTON_CHANNEL_ID".into(), channel.to_owned()),
            ("OMON_BUTTON_MESSAGE_ID".into(), message_ts.to_owned()),
            ("OMON_BUTTON_PLATFORM".into(), "slack".into()),
        ];
        let outcome = run_button_action(&config, &click, &extra).await?;
        lines.push(outcome_text(&click.action, &outcome));
    }
    Ok(lines)
}

/// Longest reply shown for one click; Slack ephemeral text stays readable well under its own limit.
const MAX_REPLY_CHARS: usize = 1500;
/// A crash dump shows only its last lines (the error), never the whole trace.
const MAX_ERROR_LINES: usize = 3;

/// What the person who clicked sees. The command's stdout is its human result (also on failure, when the
/// command explains itself there); stderr is only a fallback, reduced to its last lines. Terminal colour
/// codes are removed and the text is capped, so a command can never paste a raw dump into Slack.
fn outcome_text(action: &str, outcome: &ActionOutcome) -> String {
    let stdout = clean(&outcome.stdout);
    if outcome.success {
        return if stdout.is_empty() {
            crate::locale::text("완료했습니다.", "Done.")
        } else {
            cap(&stdout)
        };
    }
    if !stdout.is_empty() {
        return cap(&stdout);
    }
    let stderr = clean(&outcome.stderr);
    let tail: Vec<&str> = stderr.lines().rev().take(MAX_ERROR_LINES).collect();
    let tail: Vec<&str> = tail.into_iter().rev().collect();
    let code = outcome
        .code
        .map_or_else(|| "signal".to_owned(), |code| code.to_string());
    let headline = if crate::locale::current().is_korean() {
        format!("⚠️ {action} 실패 (exit {code})")
    } else {
        format!("⚠️ {action} failed (exit {code})")
    };
    if tail.is_empty() {
        headline
    } else {
        cap(&format!("{headline}\n{}", tail.join("\n")))
    }
}

fn clean(text: &str) -> String {
    static ANSI: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let ansi = ANSI.get_or_init(|| {
        regex::Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").expect("valid ANSI pattern")
    });
    ansi.replace_all(text, "").trim().to_owned()
}

fn cap(text: &str) -> String {
    if text.chars().count() <= MAX_REPLY_CHARS {
        return text.to_owned();
    }
    let mut shown: String = text.chars().take(MAX_REPLY_CHARS - 1).collect();
    shown.push('…');
    shown
}

pub fn payload_has_button(payload: &Value) -> bool {
    payload
        .get("actions")
        .and_then(Value::as_array)
        .is_some_and(|actions| actions.iter().any(|action| button_value(action).is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_content_intel_button_runs_the_configured_command() {
        let path = std::env::temp_dir().join(format!("ferryx-buttons-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{"content-intel":{"command":["printf","ok%s\\n%s"],"timeout_secs":5}}"#,
        )
        .unwrap();
        let payload = serde_json::json!({
            "type": "block_actions",
            "user": {"id": "UOWNER"},
            "channel": {"id": "C0CA1C16N49"},
            "container": {"message_ts": "1.2"},
            "actions": [{
                "action_id": "omon:btn:content-intel:publish:86",
                "value": "omon:btn:content-intel:publish:86"
            }]
        });
        let lines = run_button_clicks(&payload, "UOWNER", &path).await.unwrap();
        assert_eq!(lines, vec!["okpublish\n86".to_owned()]);
        let refused = run_button_clicks(&payload, "UOTHER", &path)
            .await
            .unwrap_err()
            .to_string();
        assert!(refused.contains("권한이 없습니다"));
        // With no owner configured nobody may click: there is no built-in owner.
        let unconfigured = run_button_clicks(&payload, "", &path)
            .await
            .unwrap_err()
            .to_string();
        assert!(unconfigured.contains("권한이 없습니다"), "{unconfigured}");
        let _ = std::fs::remove_file(&path);
    }

    fn outcome(success: bool, stdout: &str, stderr: &str) -> ActionOutcome {
        ActionOutcome {
            success,
            code: Some(if success { 0 } else { 1 }),
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn a_failed_command_that_explains_itself_on_stdout_shows_that_text() {
        let text = outcome_text(
            "apply",
            &outcome(false, "⚠️ 위시켓 지원 실패\n• 로그아웃\n", "bun: noise\n"),
        );
        assert_eq!(text, "⚠️ 위시켓 지원 실패\n• 로그아웃");
    }

    #[test]
    fn a_crash_shows_only_the_last_stderr_lines_without_colour_codes() {
        let stderr = "line1\nline2\n    at a (x:1)\n    at b (x:2)\n\x1b[31merror: boom\x1b[0m\n";
        let text = outcome_text("bid", &outcome(false, "", stderr));
        assert_eq!(
            text,
            "⚠️ bid 실패 (exit 1)\n    at a (x:1)\n    at b (x:2)\nerror: boom"
        );
    }

    #[test]
    fn success_text_is_cleaned_and_capped() {
        assert_eq!(
            outcome_text("send", &outcome(true, "\x1b[2mok\x1b[0m\n", "")),
            "ok"
        );
        assert_eq!(
            outcome_text("send", &outcome(true, "  \n", "")),
            "완료했습니다."
        );
        crate::locale::with(crate::locale::OwnerLocale::english_utc(), || {
            assert_eq!(outcome_text("send", &outcome(true, "", "")), "Done.");
            assert_eq!(outcome_text("bid", &outcome(false, "", "")), "⚠️ bid failed (exit 1)");
        });
        let long = outcome_text("send", &outcome(true, &"가".repeat(2000), ""));
        assert_eq!(long.chars().count(), MAX_REPLY_CHARS);
        assert!(long.ends_with('…'));
    }
}
