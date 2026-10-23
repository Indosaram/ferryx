//! Slack `omon:btn:<namespace>:<action>:<arg>` clicks. The command comes from
//! `~/.omon/button-actions.json` (override `OMON_BUTTON_ACTIONS_FILE`). The environment is cleared
//! so gateway credentials are not copied in, then the click's user, channel, and message are added.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::error::{HubError, Result};

pub const BUTTON_PREFIX: &str = "omon:btn:";
const NOT_AUTHORIZED: &str = "이 버튼을 누를 권한이 없습니다.";
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
        .join(".omon")
        .join("button-actions.json")
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
        return Err(HubError::message(NOT_AUTHORIZED));
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
            lines.push(format!(
                "`{}` 버튼 동작이 설정되어 있지 않습니다.",
                click.namespace
            ));
            continue;
        };
        let extra = vec![
            ("OMON_BUTTON_USER_ID".into(), user.to_owned()),
            ("OMON_BUTTON_CHANNEL_ID".into(), channel.to_owned()),
            ("OMON_BUTTON_MESSAGE_ID".into(), message_ts.to_owned()),
            ("OMON_BUTTON_PLATFORM".into(), "slack".into()),
        ];
        let outcome = run_button_action(&config, &click, &extra).await?;
        let text = if outcome.success {
            let stdout = outcome.stdout.trim();
            if stdout.is_empty() {
                "완료했습니다.".to_owned()
            } else {
                stdout.to_owned()
            }
        } else {
            format!("button {} failed: {}", click.action, outcome.stderr.trim())
        };
        lines.push(text);
    }
    Ok(lines)
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
            r#"{"content-intel":{"command":["printf","ok%s\n%s"],"timeout_secs":5}}"#,
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
        // 2026-10-23 05:05Z: with no owner configured, omo-gateway's owner still may click.
        let mut owner_click = payload.clone();
        owner_click["user"]["id"] = serde_json::json!("U04M1JNJM8T");
        assert_eq!(
            run_button_clicks(&owner_click, "", &path).await.unwrap(),
            vec!["okpublish\n86".to_owned()]
        );
        let _ = std::fs::remove_file(&path);
    }
}
