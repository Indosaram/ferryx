//! Gateway tools the agent and the action loop call.
//! Browser drives Camofox through the Chrome CDP port, the same client omo uses.
use std::path::Path;

use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::effect::{classify_command, requires_always_ask, Effect};
use crate::error::{HubError, Result};

pub const NAMES: &[&str] = &[
    "browser",
    "cron",
    "file",
    "mcp",
    "message_context",
    "skills",
    "terminal",
    "web",
];

/// Runs one tool. `approved` is the owner's ledger decision for a side effect.
pub async fn dispatch(
    pool: &SqlitePool,
    name: &str,
    args: &Value,
    approved: bool,
) -> Result<Value> {
    match name {
        "terminal" => {
            let command = required_str(args, "command")?;
            let output = run_allowed(command, approved).await?;
            Ok(json!({"output": output}))
        }
        "file" => file_op(args, approved).await,
        "web" => web_get(args).await,
        "memory" => memory_op(pool, args).await,
        "message_context" => message_context(args).await,
        "cron" => cron_list(pool).await,
        "skills" => Ok(json!({"skills": list_skills(skills_root(args))})),
        "mcp" => mcp_list(pool).await,
        "browser" => crate::browser::execute(args).await,
        other => Err(HubError::message(format!("unknown gateway tool: {other}"))),
    }
}

pub async fn run_allowed(command: &str, approved: bool) -> Result<String> {
    let class = classify_command(command, None);
    if !approved && (requires_always_ask(class.effect, false) || !effect_may_run(class.effect)) {
        return Err(HubError::message(format!(
            "effect {} needs owner approval before it runs",
            class.effect.as_str()
        )));
    }
    run_command(command).await
}

fn effect_may_run(effect: Effect) -> bool {
    matches!(effect, Effect::Read | Effect::Draft)
}

pub async fn run_command(command: &str) -> Result<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        crate::child_env::command("sh")
            .arg("-c")
            .arg(command)
            .output(),
    )
    .await
    .map_err(|_| HubError::message("tool command timed out"))?
    .map_err(|error| HubError::message(error.to_string()))?;
    if !output.status.success() {
        return Err(HubError::message(format!(
            "tool command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn file_op(args: &Value, approved: bool) -> Result<Value> {
    let path = required_str(args, "path")?;
    if args.get("content").is_some() {
        if !approved && !crate::effect::is_draft_path(path, None) {
            return Err(HubError::message(
                "writing outside a draft path needs owner approval",
            ));
        }
        let content = required_str(args, "content")?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|error| HubError::message(error.to_string()))?;
            }
        }
        tokio::fs::write(path, content)
            .await
            .map_err(|error| HubError::message(error.to_string()))?;
        return Ok(json!({"wrote": path}));
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    Ok(json!({"content": text}))
}

async fn web_get(args: &Value) -> Result<Value> {
    let url = required_str(args, "url")?;
    #[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
    {
        let response = reqwest::Client::new()
            .get(url)
            .send()
            .await
            .map_err(|error| HubError::message(error.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|error| HubError::message(error.to_string()))?;
        let clipped: String = body.chars().take(4000).collect();
        Ok(json!({"status": status, "body": clipped}))
    }
    #[cfg(not(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice")))]
    {
        let _ = url;
        Err(HubError::message(
            "web tool needs hub-slack, hub-discord, or hub-voice",
        ))
    }
}

async fn memory_op(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let store = crate::memory::MemoryStore::new(pool.clone());
    let session = args
        .get("session_key")
        .and_then(Value::as_str)
        .unwrap_or("meta");
    if let Some(content) = args.get("content").and_then(Value::as_str) {
        let memory = store
            .remember(
                session,
                content,
                args.get("metadata").cloned().unwrap_or(json!({})),
            )
            .await?;
        return Ok(json!({"id": memory.id}));
    }
    let query = required_str(args, "query")?;
    let found = store.search(session, query, 5).await?;
    Ok(
        json!({"memories": found.into_iter().map(|item| json!({"content": item.content, "score": item.score})).collect::<Vec<_>>()}),
    )
}

async fn cron_list(pool: &SqlitePool) -> Result<Value> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, expression FROM scheduled_jobs WHERE enabled = 1 ORDER BY id")
            .fetch_all(pool)
            .await?;
    Ok(
        json!({"jobs": rows.into_iter().map(|(id, expression)| json!({"id": id, "expression": expression})).collect::<Vec<_>>()}),
    )
}

async fn message_context(args: &Value) -> Result<Value> {
    if args.get("pages").is_some() {
        return Ok(json!({"messages": crate::context::walk_pages(args)}));
    }
    let channel = args.get("channel").and_then(Value::as_str);
    let ts = args
        .get("ts")
        .or_else(|| args.get("thread_ts"))
        .and_then(Value::as_str);
    if let (Some(channel), Some(ts)) = (channel, ts) {
        let token = args
            .get("token")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| crate::config::secret_from_env("SLACK_BOT_TOKEN"));
        let Some(token) = token.filter(|token| !token.is_empty()) else {
            return Err(HubError::message(
                "message_context needs SLACK_BOT_TOKEN to call conversations.replies",
            ));
        };
        let base = args
            .get("base")
            .and_then(Value::as_str)
            .unwrap_or("https://slack.com/api");
        let messages = crate::context::conversations_replies(base, &token, channel, ts).await?;
        return Ok(json!({"messages": messages}));
    }
    Ok(json!({"messages": crate::context::walk_pages(args)}))
}

async fn mcp_list(pool: &SqlitePool) -> Result<Value> {
    let _ = pool;
    let listed = crate::mcp::tool_list();
    let tools: Vec<&str> = listed
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect();
    Ok(json!({"tools": tools}))
}

pub(crate) fn skills_root(args: &Value) -> Option<std::path::PathBuf> {
    args.get("root")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .or_else(|| {
            crate::hub_env::var_os("HERMES_HOME")
                .map(|root| std::path::PathBuf::from(root).join("skills"))
        })
        .or_else(|| {
            crate::hub_env::var_os("HOME").map(|home| {
                std::path::PathBuf::from(home)
                    .join(".hermes")
                    .join("skills")
            })
        })
}

pub(crate) fn list_skills(root: Option<std::path::PathBuf>) -> Vec<String> {
    let Some(root) = root else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            if let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) {
                names.push(name.to_owned());
            }
        } else if path.join("SKILL.md").is_file() {
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                names.push(name.to_owned());
            }
        }
    }
    names.sort();
    names
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| HubError::message(format!("tool argument {key} is required")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_read_runs_and_a_spend_waits_for_approval() {
        let pool = crate::storage::memory().await.unwrap();
        let ran = dispatch(&pool, "terminal", &json!({"command": "printf hi"}), false)
            .await
            .unwrap();
        assert_eq!(ran["output"], "hi");
        let held = dispatch(
            &pool,
            "terminal",
            &json!({"command": "stripe charges create --amount 1000"}),
            false,
        )
        .await;
        assert!(held.is_err());
        assert!(NAMES.contains(&"message_context"));
    }

    #[tokio::test]
    async fn memory_and_cron_tools_read_the_hub_database() {
        let pool = crate::storage::memory().await.unwrap();
        dispatch(
            &pool,
            "memory",
            &json!({"session_key": "s", "content": "lane deploy note"}),
            true,
        )
        .await
        .unwrap();
        let found = dispatch(
            &pool,
            "memory",
            &json!({"session_key": "s", "query": "deploy"}),
            true,
        )
        .await
        .unwrap();
        assert_eq!(found["memories"][0]["content"], "lane deploy note");
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json) VALUES('daily','every 1d','{}')")
            .execute(&pool)
            .await
            .unwrap();
        let jobs = dispatch(&pool, "cron", &json!({}), true).await.unwrap();
        assert_eq!(jobs["jobs"][0]["id"], "daily");
        let mcp = dispatch(&pool, "mcp", &json!({}), true).await.unwrap();
        assert!(mcp["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "agents.list"));
    }
}
