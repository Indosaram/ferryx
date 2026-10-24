//! stdio MCP server. One JSON-RPC message per line.
//! Tools: agents.list/read/wait/send/answer/stop, lane.*, tracker.*, cron.*, chat.reply.
use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::agent_control::{AgentControl, AgentOp, TaskLauncher};
use crate::chat::reply::chat_reply;
use crate::chat::session::SessionKey;
use crate::cron::{self, CronJob, CronTarget};
use crate::error::Result;
use crate::lanes::runner::LaneRunner;
use crate::lanes::state::{self, transition, LaneStatus};
use crate::tracker::Tracker;

pub struct McpContext {
    pub control: Arc<dyn AgentControl>,
    pub launcher: Arc<dyn TaskLauncher>,
    pub pool: SqlitePool,
    pub meta_session_id: String,
    pub tracker_token: String,
}

pub async fn serve(context: Arc<McpContext>) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            // A JSON-RPC notification (no id) is never answered.
            Ok(request) if request.get("id").is_none() => continue,
            Ok(request) => dispatch(&context, &request).await,
            Err(error) => {
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":error.to_string()}})
            }
        };
        stdout.write_all(response.to_string().as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
    Ok(())
}

pub async fn dispatch(context: &McpContext, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": {"name": "ferryx-hub", "version": env!("CARGO_PKG_VERSION")},
            "capabilities": {"tools": {}}
        })),
        "tools/list" => Ok(json!({"tools": tool_list()})),
        "tools/call" => {
            let name = request
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let args = request
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            call_tool(context, name, &args)
                .await
                .map(|value| json!({"content":[{"type":"text","text": value.to_string()}]}))
        }
        _ => Err(format!("unknown method {method}")),
    };
    match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(message) => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":message}}),
    }
}

pub(crate) fn tool_list() -> Vec<Value> {
    let session = json!({"type": "string", "description": "Pane session id from agents.list"});
    let host = json!({"type": "string", "description": "Paired host id; omit for this machine"});
    let request_id = json!({"type": "string", "description": "Idempotency key; reuse it when retrying the same input"});
    let schema = |properties: Value, required: &[&str]| json!({"type": "object", "properties": properties, "required": required});
    let tools: Vec<(&str, &str, Value)> = vec![
        (
            "agents.list",
            "List agent panes and their state. Pass hosts to list a fleet.",
            schema(
                json!({"host": host, "hosts": {"type": "array", "items": {"type": "string"}}}),
                &[],
            ),
        ),
        (
            "agents.read",
            "Read one pane: state, pending prompt, and screen text.",
            schema(json!({"sessionId": session, "host": host}), &["sessionId"]),
        ),
        (
            "agents.wait",
            "Block until the pane reaches a state after a cursor, without polling.",
            schema(
                json!({"sessionId": session, "host": host,
                "until": {"type": "string", "enum": ["working", "blocked", "idle", "prompt", "any"]},
                "after": {"type": "integer", "description": "Sequence cursor from a previous result"},
                "revision": {"type": "string", "description": "Wait for a screen revision other than this"},
                "timeoutMs": {"type": "integer"}}),
                &["sessionId"],
            ),
        ),
        (
            "agents.send",
            "Type a line into the pane and press Enter. Refused if a person just typed.",
            schema(
                json!({"sessionId": session, "host": host, "text": {"type": "string"}, "requestId": request_id}),
                &["sessionId", "text", "requestId"],
            ),
        ),
        (
            "agents.answer",
            "Answer the pending prompt from agents.read, checked against its screen revision.",
            schema(
                json!({"sessionId": session, "host": host, "promptId": {"type": "string"},
                "screenRevision": {"type": "string"},
                "answer": {"type": "object", "description": "Exactly one of optionIndex, optionIndices, customText"},
                "requestId": request_id}),
                &[
                    "sessionId",
                    "promptId",
                    "screenRevision",
                    "answer",
                    "requestId",
                ],
            ),
        ),
        (
            "agents.stop",
            "Interrupt the agent's turn with its interrupt key. Never kills the process.",
            schema(
                json!({"sessionId": session, "host": host, "requestId": request_id}),
                &["sessionId", "requestId"],
            ),
        ),
        (
            "lane.list",
            "List work lanes and their states.",
            schema(json!({}), &[]),
        ),
        (
            "lane.transition",
            "Move a lane to another status.",
            schema(
                json!({"key": {"type": "string"}, "status": {"type": "string"}, "reason": {"type": "string"}}),
                &["key", "status"],
            ),
        ),
        (
            "lane.pause",
            "Pause a live lane: it keeps its status and is not driven until resumed.",
            schema(json!({"key": {"type": "string"}}), &["key"]),
        ),
        (
            "lane.resume",
            "Resume a paused lane.",
            schema(json!({"key": {"type": "string"}}), &["key"]),
        ),
        (
            "lane.drive",
            "Drive a lane one step: launch or nudge its pane.",
            schema(json!({"key": {"type": "string"}}), &["key"]),
        ),
        (
            "tracker.issues",
            "List tracker issues, or create one when title is given.",
            schema(
                json!({"title": {"type": "string"}, "body": {"type": "string"}}),
                &[],
            ),
        ),
        (
            "lane_goal_show",
            "Show a lane's goal contract: requirements, mode, turns, stop reason, proposal and pending revisions.",
            schema(json!({"key": {"type": "string"}}), &["key"]),
        ),
        (
            "lane_goal_set",
            "Set or replace a lane's goal contract. Refused while a supervisor turn is in flight.",
            schema(
                json!({"key": {"type": "string"},
                "requirements": {"type": "array", "items": {"type": "object",
                    "properties": {"id": {"type": "string"}, "text": {"type": "string"}, "verifier": {"type": "string"}},
                    "required": ["id", "text", "verifier"]}},
                "mode": {"type": "string", "enum": ["off", "supervised", "full"]},
                "turnLimit": {"type": "integer", "minimum": 1}}),
                &["key", "requirements"],
            ),
        ),
        (
            "lane_goal_approve",
            "Approve and send the lane's proposal awaiting approval, once.",
            schema(json!({"key": {"type": "string"}}), &["key"]),
        ),
        ("cron.list", "List scheduled jobs.", schema(json!({}), &[])),
        (
            "cron.run",
            "Run a stored job now. With script, store the job first.",
            schema(
                json!({"id": {"type": "string"}, "name": {"type": "string"}, "script": {"type": "string"},
                "target": {"type": "string", "description": "shell (default) or another cron target"}}),
                &["id"],
            ),
        ),
        (
            "chat.reply",
            "Post a reply to a chat channel. The text is yours; it is never copied from a screen.",
            schema(
                json!({"platform": {"type": "string", "enum": ["slack", "discord"]}, "channelId": {"type": "string"},
                "userId": {"type": "string"}, "text": {"type": "string"}}),
                &["channelId", "text"],
            ),
        ),
    ];
    tools
        .into_iter()
        .map(|(name, description, input_schema)| {
            json!({"name": name, "description": description, "inputSchema": input_schema})
        })
        .collect()
}

/// Turn limit of a new goal contract when the caller names none (the storage default).
pub const GOAL_DEFAULT_TURN_LIMIT: u32 = 20;

/// The code a supervisor input tool returns when a person paused the lane that owns the pane.
pub const LANE_PAUSED: &str = "LANE_PAUSED";

/// Pause is the person's decision: the supervisor still reads the pane but types nothing into it.
/// Only this machine's panes belong to lanes, so a paired host's pane is never refused here.
async fn refuse_paused_pane(
    context: &McpContext,
    host: Option<&str>,
    args: &Value,
) -> std::result::Result<(), String> {
    if host.is_some_and(|host| host != "local") {
        return Ok(());
    }
    let Some(session_id) = args.get("sessionId").and_then(Value::as_str) else {
        return Ok(());
    };
    let paused = crate::lanes::supervised::session_paused(&context.pool, session_id)
        .await
        .map_err(|error| error.to_string())?;
    if paused {
        return Err(format!(
            "{LANE_PAUSED}: the lane driving {session_id} is paused; resume it before typing here"
        ));
    }
    Ok(())
}

async fn call_tool(
    context: &McpContext,
    name: &str,
    args: &Value,
) -> std::result::Result<Value, String> {
    let host = args.get("host").and_then(Value::as_str);
    if matches!(name, "agents.send" | "agents.answer" | "agents.stop") {
        refuse_paused_pane(context, host, args).await?;
    }
    match name {
        "agents.list" => {
            if let Some(hosts) = args.get("hosts").and_then(Value::as_array) {
                let hosts: Vec<String> = hosts
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                let entries = crate::fleet::list_persisted(
                    context.control.as_ref(),
                    &context.pool,
                    &hosts,
                    std::time::Duration::from_secs(5),
                )
                .await;
                return Ok(json!(entries
                    .into_iter()
                    .map(|entry| json!({
                        "hostId": entry.host_id,
                        "sessions": entry.sessions,
                        "error": entry.error,
                        "stale": entry.stale,
                    }))
                    .collect::<Vec<_>>()));
            }
            context
                .control
                .execute(host, AgentOp::List)
                .await
                .map_err(|error| error.to_string())
        }
        "agents.read" => {
            let session_id = required(args, "sessionId")?;
            context
                .control
                .execute(host, AgentOp::Read { session_id })
                .await
                .map_err(|error| error.to_string())
        }
        "agents.wait" => {
            let session_id = required(args, "sessionId")?;
            context
                .control
                .execute(
                    host,
                    AgentOp::Wait {
                        session_id,
                        until: args
                            .get("until")
                            .and_then(Value::as_str)
                            .unwrap_or("any")
                            .to_owned(),
                        after_revision: args
                            .get("revision")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        after: args.get("after").and_then(Value::as_u64).unwrap_or(0),
                        timeout_ms: args
                            .get("timeoutMs")
                            .and_then(Value::as_u64)
                            .unwrap_or(30_000),
                    },
                )
                .await
                .map_err(|error| error.to_string())
        }
        "agents.send" => {
            let session_id = required(args, "sessionId")?;
            let text = required(args, "text")?;
            let request_id = required(args, "requestId")?;
            context
                .control
                .execute(
                    host,
                    AgentOp::Send {
                        session_id,
                        text,
                        request_id,
                    },
                )
                .await
                .map_err(|error| error.to_string())
        }
        "agents.answer" => {
            let session_id = required(args, "sessionId")?;
            let prompt_id = required(args, "promptId")?;
            let screen_revision = required(args, "screenRevision")?;
            let request_id = required(args, "requestId")?;
            let answer = args
                .get("answer")
                .cloned()
                .unwrap_or_else(|| json!({"optionIndex": 0}));
            context
                .control
                .execute(
                    host,
                    AgentOp::Answer {
                        session_id,
                        prompt_id,
                        screen_revision,
                        answer,
                        request_id,
                    },
                )
                .await
                .map_err(|error| error.to_string())
        }
        "agents.stop" => {
            let session_id = required(args, "sessionId")?;
            let request_id = required(args, "requestId")?;
            context
                .control
                .execute(
                    host,
                    AgentOp::Stop {
                        session_id,
                        request_id,
                    },
                )
                .await
                .map_err(|error| error.to_string())
        }
        "lane.list" => {
            let rows: Vec<(String, String, Option<String>)> =
                sqlx::query_as("SELECT key, status, omo_thread_id FROM lanes ORDER BY key")
                    .fetch_all(&context.pool)
                    .await
                    .map_err(|error| error.to_string())?;
            let paused = crate::lanes::pause::paused_keys(&context.pool)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!(rows
                .into_iter()
                .map(|(key, status, pane)| {
                    let is_paused = paused.contains(&key);
                    json!({"key": key, "status": status, "pane": pane, "paused": is_paused})
                })
                .collect::<Vec<_>>()))
        }
        "lane.pause" => {
            let key = required(args, "key")?;
            let changed = crate::lanes::pause::pause(&context.pool, &key, "session:lead")
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({"key": key, "paused": true, "changed": changed}))
        }
        "lane.resume" => {
            let key = required(args, "key")?;
            if !crate::lanes::pause::resume(&context.pool, &key)
                .await
                .map_err(|error| error.to_string())?
            {
                return Err(format!("NOT_PAUSED: lane {key} is not paused"));
            }
            Ok(json!({"key": key, "paused": false}))
        }
        "lane.transition" => {
            let key = required(args, "key")?;
            let to = required(args, "status")?;
            let status: LaneStatus = to
                .parse()
                .map_err(|error: state::UnknownLaneStatus| error.to_string())?;
            let reason = args.get("reason").and_then(Value::as_str);
            if status == LaneStatus::Closed {
                crate::lanes::worktree::close(&context.pool, &key, reason)
                    .await
                    .map_err(|error| error.to_string())?;
            } else {
                transition(&context.pool, &key, status, reason)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Ok(json!({"key": key, "status": status.as_str()}))
        }
        "lane.drive" => {
            let key = required(args, "key")?;
            let runner = LaneRunner {
                pool: context.pool.clone(),
                control: Arc::clone(&context.control),
                launcher: Arc::clone(&context.launcher),
                judge: crate::judge::Judge::from_config(crate::judge::typesafe_from_env()),
                load: crate::lanes::guard::HostLoad::sample(0),
                thresholds: crate::lanes::guard::GuardThresholds::default(),
            };
            let report = runner
                .drive(&key)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({"sessionId": report.session_id, "status": report.status.as_str()}))
        }
        "lane_goal_show" => {
            let key = required(args, "key")?;
            let view = crate::lanes::supervised::goal_view(&context.pool, &key)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({"key": key, "goalContract": view}))
        }
        "lane_goal_set" => {
            let key = required(args, "key")?;
            let requirements: Vec<crate::goals::contract::RequirementSpec> = serde_json::from_value(
                args.get("requirements").cloned().unwrap_or(Value::Null),
            )
            .map_err(|error| format!("requirements: {error}"))?;
            let existing = crate::goals::contract::contract_of(&context.pool, &key)
                .await
                .map_err(|error| error.to_string())?;
            let mode = match args.get("mode").and_then(Value::as_str) {
                Some(mode) => mode
                    .parse::<crate::goals::GoalMode>()
                    .map_err(|error| error.to_string())?,
                None => existing
                    .as_ref()
                    .map(|contract| contract.mode)
                    .unwrap_or(crate::goals::GoalMode::Supervised),
            };
            let turn_limit = match args.get("turnLimit").and_then(Value::as_u64) {
                Some(limit) => u32::try_from(limit).map_err(|_| "turnLimit is too large".to_string())?,
                None => existing
                    .as_ref()
                    .map(|contract| contract.turn_limit)
                    .unwrap_or(GOAL_DEFAULT_TURN_LIMIT),
            };
            crate::goals::contract::set_contract(&context.pool, &key, &requirements, mode, turn_limit)
                .await
                .map_err(|error| error.to_string())?;
            let view = crate::lanes::supervised::goal_view(&context.pool, &key)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({"key": key, "goalContract": view}))
        }
        "lane_goal_approve" => {
            let key = required(args, "key")?;
            let outcome =
                crate::goals::r#loop::approve_proposal(&context.pool, context.control.as_ref(), &key)
                    .await
                    .map_err(|error| error.to_string())?;
            Ok(json!(outcome))
        }
        "tracker.issues" => {
            let tracker = Tracker::new(context.pool.clone(), context.tracker_token.clone());
            if let Some(title) = args.get("title").and_then(Value::as_str) {
                let id = tracker
                    .create(
                        title,
                        args.get("body").and_then(Value::as_str).unwrap_or(""),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(json!({"id": id}))
            } else {
                tracker
                    .list()
                    .await
                    .map(|issues| json!({"issues": issues}))
                    .map_err(|error| error.to_string())
            }
        }
        "cron.list" => {
            let jobs = cron::list_jobs(&context.pool)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!(jobs
                .into_iter()
                .map(|job| json!({"id": job.id, "target": job.target.as_str(), "name": job.name}))
                .collect::<Vec<_>>()))
        }
        "cron.run" => {
            let id = required(args, "id")?;
            if args.get("script").is_some() {
                let target = CronTarget::parse(
                    args.get("target")
                        .and_then(Value::as_str)
                        .unwrap_or("shell"),
                )
                .map_err(|error| error.to_string())?;
                cron::insert_job(
                    &context.pool,
                    &CronJob {
                        id: id.clone(),
                        name: args
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or(&id)
                            .to_owned(),
                        target,
                        script: args
                            .get("script")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        schedule: String::new(),
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            }
            let output = cron::execute(
                &context.pool,
                &id,
                context.control.as_ref(),
                context.launcher.as_ref(),
                &context.meta_session_id,
            )
            .await
            .map_err(|error| error.to_string())?;
            Ok(json!({"output": output}))
        }
        "chat.reply" => {
            let platform = args
                .get("platform")
                .and_then(Value::as_str)
                .unwrap_or("slack");
            let channel = required(args, "channelId")?;
            let user = args.get("userId").and_then(Value::as_str).unwrap_or("");
            let text = required(args, "text")?;
            let session = SessionKey::new(platform, None::<String>, channel, None::<String>, user);
            let id = chat_reply(&context.pool, &session, &text, None)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({"id": id, "sessionKey": session.storage_key()}))
        }
        other => Err(format!("unknown tool {other}")),
    }
}

fn required(args: &Value, key: &str) -> std::result::Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing {key}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::{FixedLauncher, RecordingControl};
    use serde_json::json;

    #[tokio::test]
    async fn a_paused_lane_refuses_supervisor_input_into_its_pane() {
        use crate::lanes::state::{insert_lane, set_pane, transition, LaneStatus};
        let control = Arc::new(RecordingControl::default());
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "ledger", "Ledger", "lead", "/repo", "file marker")
            .await
            .unwrap();
        transition(&pool, "ledger", LaneStatus::Working, Some("launched"))
            .await
            .unwrap();
        set_pane(&pool, "ledger", "pane-1").await.unwrap();
        crate::lanes::pause::pause(&pool, "ledger", "desktop")
            .await
            .unwrap();
        let context = McpContext {
            control: control.clone(),
            launcher: Arc::new(FixedLauncher::new("pane")),
            pool,
            meta_session_id: "meta".into(),
            tracker_token: "t".into(),
        };
        let send = |session: &str| {
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"agents.send",
                "arguments":{"sessionId": session, "text":"go","requestId":"r1"}}})
        };
        let refused = dispatch(&context, &send("pane-1")).await;
        assert!(refused.to_string().contains(LANE_PAUSED), "{refused}");
        assert!(control.ops.lock().unwrap().is_empty(), "nothing reached the pane");
        let sent = dispatch(&context, &send("pane-2")).await;
        assert!(!sent.to_string().contains(LANE_PAUSED), "{sent}");
        assert_eq!(control.ops.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn tools_call_agents_list_and_chat_reply() {
        let control = Arc::new(RecordingControl::default());
        *control.result.lock().unwrap() = json!({"sessions": []});
        let context = McpContext {
            control,
            launcher: Arc::new(FixedLauncher::new("pane")),
            pool: crate::storage::memory().await.unwrap(),
            meta_session_id: "meta".into(),
            tracker_token: "t".into(),
        };
        let listed = dispatch(
            &context,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"agents.list","arguments":{}}}),
        )
        .await;
        assert!(listed["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("sessions"));
        let reply = dispatch(
            &context,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"chat.reply","arguments":{"channelId":"C1","text":"ok"}}}),
        )
        .await;
        assert!(reply.get("error").is_none(), "{reply}");
        let names = dispatch(
            &context,
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}),
        )
        .await;
        let tools = names["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|tool| tool["name"] == "lane.drive"));
        assert!(tools.iter().any(|tool| tool["name"] == "agents.stop"));
    }

    #[test]
    fn tool_list_names_the_lane_goal_tools() {
        let names: Vec<String> = tool_list()
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
            .collect();
        for name in ["lane_goal_show", "lane_goal_set", "lane_goal_approve"] {
            assert!(names.iter().any(|known| known == name), "{name} missing from {names:?}");
        }
    }

    #[tokio::test]
    async fn lane_goal_tools_set_show_and_refuse_an_approval_with_nothing_awaiting() {
        let pool = crate::storage::memory().await.unwrap();
        crate::lanes::state::insert_lane(&pool, "ledger", "Ledger", "lead", "/repo", "file marker")
            .await
            .unwrap();
        let context = McpContext {
            control: Arc::new(RecordingControl::default()),
            launcher: Arc::new(FixedLauncher::new("pane")),
            pool,
            meta_session_id: "meta".into(),
            tracker_token: "t".into(),
        };
        let call = |name: &str, arguments: Value| {
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name": name, "arguments": arguments}})
        };
        let set = dispatch(
            &context,
            &call("lane_goal_set", json!({"key": "ledger", "mode": "full", "turnLimit": 3,
                "requirements": [{"id": "r1", "text": "tests pass", "verifier": "cargo test"}]})),
        )
        .await;
        assert!(set.get("error").is_none(), "{set}");
        let shown = dispatch(&context, &call("lane_goal_show", json!({"key": "ledger"}))).await;
        let text = shown["result"]["content"][0]["text"].as_str().unwrap();
        let body: Value = serde_json::from_str(text).unwrap();
        assert_eq!(body["goalContract"]["mode"], "full");
        assert_eq!(body["goalContract"]["turnLimit"], 3);
        assert_eq!(body["goalContract"]["requirements"][0]["id"], "r1");
        let approve = dispatch(&context, &call("lane_goal_approve", json!({"key": "ledger"}))).await;
        assert!(approve.get("error").is_some(), "{approve}");
    }
}
