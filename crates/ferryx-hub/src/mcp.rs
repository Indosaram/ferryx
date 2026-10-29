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
use crate::supervisor::admission;
use crate::supervisor::request;
use crate::supervisor::ChannelProvider;
use crate::tracker::Tracker;

pub struct McpContext {
    pub control: Arc<dyn AgentControl>,
    pub launcher: Arc<dyn TaskLauncher>,
    pub pool: SqlitePool,
    pub meta_session_id: String,
    pub tracker_token: String,
    /// Forwards every `supervisor.request` that writes (submit, adopt, a message with work,
    /// goal control, memory) to the daemon, whose peer check refuses it from inside a Ferryx
    /// pane. `None` serves them here (in-process callers and tests); goal control and memory
    /// are then refused outright when this process runs in a pane (`FERRYX_SESSION_ID` set).
    pub owner_route: Option<Arc<dyn OwnerRoute>>,
}

/// Where `supervisor.request` writes go when the MCP process is not the authority itself:
/// the daemon's `SupervisorRequestV1`, under its pane peer check.
#[async_trait::async_trait]
pub trait OwnerRoute: Send + Sync {
    async fn forward(&self, envelope: Value)
        -> std::result::Result<Value, request::RequestFailure>;
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
            "Set a lane's goal contract (owner meta-agent channel only). The first call creates the contract. On a lane that already has a contract, the call proposes a pending criteria revision replacing the requirements and needs instruction_quote: an exact quote (at least 8 chars) of the owner's own words (lane title, session goal, or owner reply). Nothing is applied until the revision is accepted.",
            schema(
                json!({"key": {"type": "string"},
                "requirements": {"type": "array", "items": {"type": "object",
                    "properties": {"id": {"type": "string"}, "text": {"type": "string"}, "verifier": {"type": "string"}},
                    "required": ["id", "text", "verifier"]}},
                "mode": {"type": "string", "enum": ["off", "supervised", "full"]},
                "turnLimit": {"type": "integer", "minimum": 1},
                "instruction_quote": {"type": "string", "description": "required when the lane already has a contract: exact owner-authored quote justifying the proposed requirement change"}}),
                &["key", "requirements"],
            ),
        ),
        (
            "lane_goal_approve",
            "Approve and send the lane's proposal awaiting approval, once (owner meta-agent channel only).",
            schema(json!({"key": {"type": "string"}}), &["key"]),
        ),
        (
            "supervisor.request",
            "Send owner work and owner decisions to the unified supervisor (owner meta-agent channel only). action=submit with payload {request: the owner's exact words, work: {title, instruction, requirements, projectId?, hostId?, kind?, cwd?, operations?, turnLimit?}} admits one goal in a single transaction and returns the persisted receipt; the hub daemon then starts the worker. Only an outcome of admitted or awaiting_owner means work is registered. action=message with payload {text, work?} is conversation unless work is attached. action=adopt with payload {laneKey, projectId?, hostId?, requirements?} adopts one legacy lane explicitly, keeping its mode and remaining budget. operations lists what the work needs (edit, test, commit, push, publish, spend, destructive): only declared operations are granted, so omitting it makes the request read-only, and anything beyond edit, test and commit holds the goal for the owner. Goal actions need goalId and expectedRevision (a stale revision is REVISION_CONFLICT): correct {text, requirements?}, pause {reason?}, resume {}, cancel {reason?}, approve {} (releases an awaiting_owner hold under the existing grant, never widens it), resolve_delivery {outcome: delivered|not_delivered} (the owner's answer to a delivery_uncertain goal: delivered grades the run without resending, not_delivered queues the same instruction as a new dispatch; the uncertain one is never resent), depend {childGoalId} or {eventKey}. teach {text} and forget {memoryId} reach supervisor memory (MEMORY_* when unavailable). Goal and memory actions are refused from inside a Ferryx pane.",
            schema(
                json!({"requestId": request_id, "projectId": {"type": "string"},
                "goalId": {"type": "string"}, "expectedRevision": {"type": "integer"},
                "action": {"type": "string", "enum": ["submit", "message", "adopt", "correct", "pause", "resume", "cancel", "approve", "resolve_delivery", "teach", "forget", "depend"]},
                "payload": {"type": "object"}}),
                &["requestId", "action", "payload"],
            ),
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
                .filter(|answer| answer.is_object())
                .cloned()
                .ok_or_else(|| "missing answer".to_owned())?;
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
            // A supervisor lane, or an adopted one, is paused through the supervisor.
            refuse_supervisor_owned(context, &key, "pause it through the supervisor").await?;
            let changed = crate::lanes::pause::pause(&context.pool, &key, "session:lead")
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({"key": key, "paused": true, "changed": changed}))
        }
        "lane.resume" => {
            let key = required(args, "key")?;
            refuse_supervisor_owned(context, &key, "resume it through the supervisor").await?;
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
            // A supervisor lane's status, and an adopted lane's worktree (where its supervisor
            // worker may run), change only through the supervisor.
            refuse_supervisor_owned(context, &key, "change its state through the supervisor").await?;
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
            // An admitted goal's lane is driven only by the hub's supervisor dispatcher; a
            // legacy drive would launch a second worker.
            refuse_supervisor_owned(context, &key, "the hub dispatcher drives it").await?;
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
        // The hub MCP server is the owner's meta-agent control plane (`ferryx-cli mcp`), never a
        // worker tool server, so goal set/approve here act with the owner's authority. A lane's
        // existing contract is never overwritten here: changes go through the revision guard.
        "lane_goal_set" => {
            let key = required(args, "key")?;
            // A supervisor goal's requirements, mode and budget change only through the
            // supervisor, which bumps the goal revision.
            refuse_supervisor_owned(context, &key, "change its goal through the supervisor").await?;
            let requirements: Vec<crate::goals::contract::RequirementSpec> =
                serde_json::from_value(args.get("requirements").cloned().unwrap_or(Value::Null))
                    .map_err(|error| format!("requirements: {error}"))?;
            let existing = crate::goals::contract::contract_of(&context.pool, &key)
                .await
                .map_err(|error| error.to_string())?;
            if existing.is_some() {
                let quote = args
                    .get("instruction_quote")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        format!(
                            "CONTRACT_EXISTS: lane {key} already has a goal contract; pass instruction_quote with the owner's exact words to propose a criteria revision"
                        )
                    })?;
                let revision = crate::goals::revision::propose(
                    &context.pool,
                    &key,
                    &crate::goals::revision::RevisionChange::Replace { requirements },
                    quote,
                )
                .await
                .map_err(|error| error.to_string())?;
                return Ok(json!({"key": key, "proposedRevision": revision}));
            }
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
                Some(limit) => {
                    u32::try_from(limit).map_err(|_| "turnLimit is too large".to_string())?
                }
                None => existing
                    .as_ref()
                    .map(|contract| contract.turn_limit)
                    .unwrap_or(GOAL_DEFAULT_TURN_LIMIT),
            };
            crate::goals::contract::set_contract(
                &context.pool,
                &key,
                &requirements,
                mode,
                turn_limit,
            )
            .await
            .map_err(|error| error.to_string())?;
            let view = crate::lanes::supervised::goal_view(&context.pool, &key)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({"key": key, "goalContract": view}))
        }
        "lane_goal_approve" => {
            let key = required(args, "key")?;
            refuse_supervisor_owned(context, &key, "the hub dispatcher sends its turns").await?;
            let outcome = crate::goals::r#loop::approve_proposal(
                &context.pool,
                context.control.as_ref(),
                &key,
            )
            .await
            .map_err(|error| error.to_string())?;
            Ok(json!(outcome))
        }
        "supervisor.request" => supervisor_request(context, args).await,
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

/// Refuses a legacy lane API aimed at a supervisor goal's lane (`SUPERVISOR_OWNED`).
async fn refuse_supervisor_owned(
    context: &McpContext,
    key: &str,
    why: &str,
) -> std::result::Result<(), String> {
    let owned = crate::supervisor::supervisor_owns_lane(&context.pool, key)
        .await
        .map_err(|error| error.to_string())?;
    if owned {
        return Err(format!(
            "{}: lane {key} belongs to a supervisor goal; {why}",
            crate::supervisor::SUPERVISOR_OWNED
        ));
    }
    Ok(())
}

/// Serves `supervisor.request` through the shared supervisor request service. Every action
/// that writes (submit, adopt, a message with work, goal control, memory) goes through
/// [`McpContext::owner_route`] to the daemon, whose pane peer check refuses it when this MCP
/// process runs inside a Ferryx pane: a worker agent may not admit owner work or decide for
/// the owner through MCP. No pane is exempt; no Ferryx launch path registers `ferryx-cli mcp`
/// with a pane's agent (the owner runs it from their own shell or client). Only a message
/// without work (conversation, which writes nothing) is answered here. This process has no
/// turn observer and transmits nothing.
async fn supervisor_request(
    context: &McpContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let fail = |failure: request::RequestFailure| format!("{}: {}", failure.code, failure.message);
    // A malformed envelope is refused before the owner is even looked up.
    let (_, action) = request::parse_envelope(args.clone()).map_err(fail)?;
    let workless_message = action == request::Action::Message
        && args.pointer("/payload/work").is_none_or(Value::is_null);
    if !workless_message {
        if let Some(route) = &context.owner_route {
            return route.forward(args.clone()).await.map_err(fail);
        }
    }
    // Without a route (in-process callers only; `ferryx-cli mcp` always has one) goal control
    // and memory are still refused in a pane.
    if action.needs_owner_authority()
        && std::env::var_os("FERRYX_SESSION_ID").is_some_and(|session| !session.is_empty())
    {
        return Err(fail(request::RequestFailure::new(
            "FORBIDDEN",
            "goal control and memory actions are the owner's and are refused inside a Ferryx pane (FERRYX_SESSION_ID is set)",
        )));
    }
    let owner = admission::installation_owner(&context.pool)
        .await
        .map_err(|error| format!("{}: {error}", error.code()))?;
    let request_context = request::RequestContext {
        principal: owner,
        adapter: ChannelProvider::Mcp,
        channel_binding_id: None,
        control: None,
        memory: None,
    };
    request::handle(&context.pool, &request_context, args.clone())
        .await
        .map_err(fail)
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
            owner_route: None,
        };
        let send = |session: &str| {
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"agents.send",
                "arguments":{"sessionId": session, "text":"go","requestId":"r1"}}})
        };
        let refused = dispatch(&context, &send("pane-1")).await;
        assert!(refused.to_string().contains(LANE_PAUSED), "{refused}");
        assert!(
            control.ops.lock().unwrap().is_empty(),
            "nothing reached the pane"
        );
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
            owner_route: None,
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
            assert!(
                names.iter().any(|known| known == name),
                "{name} missing from {names:?}"
            );
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
            owner_route: None,
        };
        let call = |name: &str, arguments: Value| json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name": name, "arguments": arguments}});
        let set = dispatch(
            &context,
            &call(
                "lane_goal_set",
                json!({"key": "ledger", "mode": "full", "turnLimit": 3,
                "requirements": [{"id": "r1", "text": "tests pass", "verifier": "cargo test"}]}),
            ),
        )
        .await;
        assert!(set.get("error").is_none(), "{set}");
        let shown = dispatch(&context, &call("lane_goal_show", json!({"key": "ledger"}))).await;
        let text = shown["result"]["content"][0]["text"].as_str().unwrap();
        let body: Value = serde_json::from_str(text).unwrap();
        assert_eq!(body["goalContract"]["mode"], "full");
        assert_eq!(body["goalContract"]["turnLimit"], 3);
        assert_eq!(body["goalContract"]["requirements"][0]["id"], "r1");
        let approve = dispatch(
            &context,
            &call("lane_goal_approve", json!({"key": "ledger"})),
        )
        .await;
        assert!(approve.get("error").is_some(), "{approve}");
    }

    #[tokio::test]
    async fn lane_goal_set_on_an_existing_contract_requires_an_owner_quote_and_creates_a_pending_revision(
    ) {
        let pool = crate::storage::memory().await.unwrap();
        crate::lanes::state::insert_lane(
            &pool,
            "ledger",
            "Ledger must reconcile nightly",
            "lead",
            "/repo",
            "file marker",
        )
        .await
        .unwrap();
        let context = McpContext {
            control: Arc::new(RecordingControl::default()),
            launcher: Arc::new(FixedLauncher::new("pane")),
            pool: pool.clone(),
            meta_session_id: "meta".into(),
            tracker_token: "t".into(),
            owner_route: None,
        };
        let call = |arguments: Value| json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name": "lane_goal_set", "arguments": arguments}});
        let first = dispatch(
            &context,
            &call(json!({"key": "ledger",
                "requirements": [{"id": "r1", "text": "tests pass", "verifier": "cargo test"}]})),
        )
        .await;
        assert!(first.get("error").is_none(), "{first}");
        let replacement =
            json!([{"id": "r2", "text": "reconcile runs", "verifier": "cargo test reconcile"}]);
        let unquoted = dispatch(
            &context,
            &call(json!({"key": "ledger", "requirements": replacement.clone()})),
        )
        .await;
        assert!(unquoted.get("error").is_some(), "{unquoted}");
        let misquoted = dispatch(
            &context,
            &call(json!({"key": "ledger", "requirements": replacement.clone(),
                "instruction_quote": "words the owner never wrote"})),
        )
        .await;
        assert!(misquoted.get("error").is_some(), "{misquoted}");
        let quoted = dispatch(
            &context,
            &call(json!({"key": "ledger", "requirements": replacement,
                "instruction_quote": "reconcile nightly"})),
        )
        .await;
        assert!(quoted.get("error").is_none(), "{quoted}");
        let text = quoted["result"]["content"][0]["text"].as_str().unwrap();
        let body: Value = serde_json::from_str(text).unwrap();
        assert_eq!(body["proposedRevision"]["state"], "proposed");
        assert_eq!(
            body["proposedRevision"]["instruction_quote"],
            "reconcile nightly"
        );
        let contract = crate::goals::contract::contract_of(&pool, "ledger")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(contract.requirements.len(), 1);
        assert_eq!(
            contract.requirements[0].id, "r1",
            "active requirements untouched"
        );
    }

    #[tokio::test]
    async fn supervisor_request_submit_only_admits_and_never_transmits() {
        let pool = crate::storage::memory().await.unwrap();
        let identity = crate::supervisor::ensure_identity(&pool, "slack:T1:U-owner")
            .await
            .unwrap();
        let project = crate::supervisor::create_project(
            &pool,
            &identity.supervisor_id,
            "ferryx",
            "github.com/example/ferryx",
        )
        .await
        .unwrap();
        crate::supervisor::bind_project_host(&pool, &project.project_id, "local", "/repo/.git")
            .await
            .unwrap();
        let control = Arc::new(RecordingControl::default());
        let launcher = Arc::new(FixedLauncher::new("pane"));
        let context = McpContext {
            control: control.clone(),
            launcher: launcher.clone(),
            pool: pool.clone(),
            meta_session_id: "meta".into(),
            tracker_token: "t".into(),
            owner_route: None,
        };
        let call = |name: &str, arguments: Value| json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name": name, "arguments": arguments}});
        let text = |response: &Value| -> Value {
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap()
        };
        let work = json!({"title": "Write result.txt", "instruction": "Create result.txt containing READY.",
            "requirements": [{"id": "r1", "text": "result.txt contains READY", "verifier": "verify.sh exits 0"}]});
        let submit = json!({"requestId": "mcp-req-1", "action": "submit", "payload": {
            "request": "Create result.txt containing READY. Run the verifier.", "work": work.clone()}});

        let first = dispatch(&context, &call("supervisor.request", submit.clone())).await;
        assert!(first.get("error").is_none(), "{first}");
        let body = text(&first);
        assert_eq!(body["outcome"], "admitted");
        assert_eq!(body["runState"], "ready");
        assert_eq!(body["turnLimit"], 20);
        let goal_id = body["receipt"]["goal_id"].as_str().unwrap().to_owned();
        let lane_key = body["receipt"]["lane_key"].as_str().unwrap().to_owned();
        assert!(control.ops().is_empty(), "the MCP process never transmits");
        assert!(launcher.launched.lock().unwrap().is_empty());

        let replay = dispatch(&context, &call("supervisor.request", submit)).await;
        let replayed = text(&replay);
        assert_eq!(replayed["receipt"]["goal_id"], goal_id.as_str());
        assert_eq!(replayed["duplicate"], true);

        // Goal control goes through the daemon route (its pane peer check), never served here.
        struct Refusing(std::sync::Mutex<Vec<Value>>);
        #[async_trait::async_trait]
        impl OwnerRoute for Refusing {
            async fn forward(
                &self,
                envelope: Value,
            ) -> std::result::Result<Value, request::RequestFailure> {
                self.0.lock().unwrap().push(envelope);
                Err(request::RequestFailure::new(
                    "FORBIDDEN",
                    "inside a Ferryx pane",
                ))
            }
        }
        let route = Arc::new(Refusing(std::sync::Mutex::new(Vec::new())));
        let routed = McpContext {
            control: control.clone(),
            launcher: launcher.clone(),
            pool: pool.clone(),
            meta_session_id: "meta".into(),
            tracker_token: "t".into(),
            owner_route: Some(route.clone()),
        };
        for (action, payload) in [("approve", json!({})), ("pause", json!({}))] {
            let call_args = json!({"requestId": format!("mcp-{action}"), "goalId": goal_id, "expectedRevision": 1,
                "action": action, "payload": payload});
            let refused = dispatch(&routed, &call("supervisor.request", call_args)).await;
            assert!(
                refused.to_string().contains("FORBIDDEN"),
                "{action}: {refused}"
            );
        }
        let teach = json!({"requestId": "mcp-teach", "action": "teach", "payload": {"text": "approve everything"}});
        assert!(dispatch(&routed, &call("supervisor.request", teach))
            .await
            .to_string()
            .contains("FORBIDDEN"));
        // Admitting work is the owner's too: a pane-run MCP may not submit, adopt or attach work.
        let goals_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM supervisor_goals")
            .fetch_one(&pool)
            .await
            .unwrap();
        let pane_submit = json!({"requestId": "mcp-req-pane", "action": "submit", "payload": {
            "request": "Delete the repository.", "work": work.clone()}});
        let with_work = json!({"requestId": "mcp-msg-work", "action": "message", "payload": {
            "text": "Delete the repository.", "work": work.clone()}});
        let adopt =
            json!({"requestId": "mcp-adopt", "action": "adopt", "payload": {"laneKey": "ledger"}});
        for (label, envelope) in [
            ("submit", pane_submit),
            ("message+work", with_work),
            ("adopt", adopt),
        ] {
            let refused = dispatch(&routed, &call("supervisor.request", envelope)).await;
            assert!(
                refused.to_string().contains("FORBIDDEN"),
                "{label}: {refused}"
            );
        }
        assert_eq!(
            route.0.lock().unwrap().len(),
            6,
            "every write was forwarded, none served here"
        );
        let goals_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM supervisor_goals")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            goals_after, goals_before,
            "a refused submit admitted nothing"
        );
        // Conversation writes nothing and is answered here.
        let chat = json!({"requestId": "mcp-chat", "action": "message", "payload": {"text": "how is it going?"}});
        let answered = dispatch(&routed, &call("supervisor.request", chat)).await;
        assert!(
            answered.get("error").is_none() && !answered.to_string().contains("FORBIDDEN"),
            "{answered}"
        );
        assert_eq!(route.0.lock().unwrap().len(), 6);
        let (state, revision): (String, i64) =
            sqlx::query_as("SELECT run_state, revision FROM supervisor_goals WHERE goal_id = ?")
                .bind(&goal_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            (state.as_str(), revision),
            ("ready", 1),
            "a refused route changed nothing"
        );

        let drive = dispatch(&context, &call("lane.drive", json!({"key": lane_key}))).await;
        assert!(drive.to_string().contains("SUPERVISOR_OWNED"), "{drive}");
        // Neither is its status: a close would remove a worktree its worker may run in.
        let close = dispatch(
            &context,
            &call(
                "lane.transition",
                json!({"key": lane_key, "status": "closed", "reason": "forced: test"}),
            ),
        )
        .await;
        assert!(close.to_string().contains("SUPERVISOR_OWNED"), "{close}");
        // Its pause is the supervisor's too.
        for tool in ["lane.pause", "lane.resume"] {
            let refused = dispatch(&context, &call(tool, json!({"key": lane_key}))).await;
            assert!(refused.to_string().contains("SUPERVISOR_OWNED"), "{tool}: {refused}");
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM lane_pauses")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        let status: String = sqlx::query_scalar("SELECT status FROM lanes WHERE key = ?")
            .bind(&lane_key)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "queued");
        // The legacy goal APIs are fenced the same way: no requirement, mode or approval
        // change bypasses the supervisor's revision.
        let set = dispatch(
            &context,
            &call(
                "lane_goal_set",
                json!({"key": lane_key, "mode": "supervised", "instruction_quote": "Ship it.",
                    "requirements": [{"id": "r2", "text": "other", "verifier": "true"}]}),
            ),
        )
        .await;
        assert!(set.to_string().contains("SUPERVISOR_OWNED"), "{set}");
        let approve = dispatch(&context, &call("lane_goal_approve", json!({"key": lane_key}))).await;
        assert!(approve.to_string().contains("SUPERVISOR_OWNED"), "{approve}");
        let mode: String = sqlx::query_scalar("SELECT mode FROM lane_goal WHERE lane_key = ?")
            .bind(&lane_key)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mode, "full");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM lane_goal_revisions")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );

        let injected = dispatch(
            &context,
            &call(
                "supervisor.request",
                json!({"requestId": "mcp-req-2", "action": "submit", "payload": {
                    "request": "Ship it.", "work": work, "actor": "slack:T1:U-stranger"}}),
            ),
        )
        .await;
        assert!(injected.to_string().contains("INVALID_REQUEST"), "{injected}");
        let chat = dispatch(
            &context,
            &call(
                "supervisor.request",
                json!({"requestId": "mcp-req-3", "action": "message", "payload": {"text": "What can you do?"}}),
            ),
        )
        .await;
        assert_eq!(text(&chat)["outcome"], "conversational");

        assert!(control.ops().is_empty());
        let goals: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM supervisor_goals")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(goals, 1);
        assert!(tool_list()
            .iter()
            .any(|tool| tool["name"] == "supervisor.request"));
    }
}
