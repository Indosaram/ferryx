//! Maho (`mhc --mode rpc`) and OmO (`omo --mode rpc`) structured-chat provider.
//!
//! Wire contract matches `crates/maho-rpc/src/rpc_types.rs` and `rpc_command_surface.rs`
//! (verified against live `mhc --mode rpc` output):
//! * Process launch: `mhc --mode rpc` (or `omo --mode rpc`) with `Command::current_dir(&cwd)`
//!   (never `--cwd`, which `mhc` does not accept). Optional `--session <id>` on resume.
//! * Commands are NDJSON envelopes `{ "id": "<req>", "type": "<snake_case>", ...camelCaseFields }`.
//! * Responses are `{ "type": "response", "id": "<req>", "command": "<cmd>", "success": bool, "data"?, "error"? }`.
//! * Inbound `extension_ui_request` events:
//!   - Interactive (`select`, `confirm`, `input`, `editor`, `question`) are NEVER auto-cancelled;
//!     they are surfaced via `PartKind::ExtensionUiRequest` and answered via `extension_ui_response`.
//!   - Passive (`setStatus`, `setWidget`, `setHeader`, `setFooter`, `setTitle`, `notify`,
//!     `set_editor_text`) are emitted as `PartKind::ExtensionStatus` / `ExtensionWidget` /
//!     `ExtensionUiRequest` without blocking startup or turns.
//! * Session settings (`model`, `effort`/`thinking_level`, `fast_mode`, `steering_mode`,
//!   `follow_up_mode`, `auto_compaction`, `auto_retry`) are applied strictly over RPC:
//!   `start()` hydrates `applied_settings` from `get_state`, `set_thinking_level` uses
//!   `scope: "turn"`, and queue/auto toggles are sent only when explicitly changed from the
//!   server's current state (note: `mhc` persists queue/auto toggles in its own settings).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::{mpsc, oneshot, Mutex};

use super::types::{
    ApprovalDecision, Capabilities, EditMessageBody, ExtensionUiResponseBody, FileChangeEntry,
    MediaBlob, ModelInfo, ModelPreferencesBody, NavigateTreeBody, PartKind, PermissionMode,
    QueueDeliveryMode, SessionSettings, SessionStats, SessionTree, SessionTreeNode,
    SlashCommandItem, StructuredChatError, ToolCallStatus,
};
use super::{PartDraft, ProviderEvent, StructuredProvider, TurnRef};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

const BUILTIN_SLASH_COMMANDS: &[&str] = &[
    "/clear",
    "/compact",
    "/context",
    "/copy",
    "/diff",
    "/export",
    "/fork",
    "/goal",
    "/help",
    "/hotkeys",
    "/import",
    "/login",
    "/logout",
    "/model",
    "/name",
    "/new",
    "/plan",
    "/reload",
    "/resume",
    "/scoped-models",
    "/session",
    "/settings",
    "/share",
    "/stats",
    "/thinking",
    "/tree",
];

pub fn maho_capabilities() -> Capabilities {
    Capabilities {
        steer: true,
        rollback: true,
        resume: true,
        revert_files: false,
        history_paging: true,
        permission_modes: PermissionMode::all().to_vec(),
        models: Vec::new(),
        slash_commands: BUILTIN_SLASH_COMMANDS
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
        execution_modes: vec!["default".into()],
        agents: Vec::new(),
        thinking_levels: vec![
            "off".into(),
            "minimal".into(),
            "low".into(),
            "medium".into(),
            "high".into(),
            "xhigh".into(),
            "max".into(),
        ],
        fast_mode: true,
        additional_dirs: false,
        queue_modes: true,
        compact_custom_instructions: true,
        tree_navigation: true,
        fork: true,
        clone_session: true,
        edit_user_message: true,
        edit_assistant_message: true,
        session_rename: true,
        session_stats: true,
        session_switch: true,
        reload: true,
        media: true,
        extension_ui: true,
        goals: true,
        favorite_models: true,
        scoped_models: true,
    }
}

pub fn build_maho_argv(
    provider_id: &str,
    resume_thread: Option<&str>,
    settings: &SessionSettings,
) -> (String, Vec<String>) {
    let program = if provider_id == "omo" { "omo" } else { "mhc" }.to_owned();
    let mut args = vec!["--mode".to_owned(), "rpc".to_owned()];
    if let Some(session_id) = resume_thread.filter(|s| !s.trim().is_empty()) {
        args.push("--session".to_owned());
        args.push(session_id.to_owned());
    }
    if let Some(model) = settings.model.as_deref().filter(|s| !s.trim().is_empty()) {
        args.push("--model".to_owned());
        args.push(model.to_owned());
    }
    if let Some(thinking) = settings.effort.as_deref().filter(|s| !s.trim().is_empty()) {
        args.push("--thinking".to_owned());
        args.push(thinking.to_owned());
    }
    (program, args)
}

#[derive(Debug, Default)]
struct PendingState {
    rpc_responses: HashMap<String, oneshot::Sender<Result<Value, String>>>,
    extension_ui: HashMap<String, String>,
    active_turn_id: Option<String>,
    last_assistant_text: Option<String>,
    active_tool_calls: HashSet<String>,
    cached_commands: Vec<SlashCommandItem>,
    session_id: Option<String>,
    applied_settings: SessionSettings,
}

struct TransportHandle {
    writer: Mutex<Box<dyn AsyncWrite + Unpin + Send>>,
    child: Mutex<Option<Child>>,
    next_id: AtomicU64,
    pending: Arc<Mutex<PendingState>>,
}

impl TransportHandle {
    fn spawn_from_io<R, W>(
        reader: R,
        writer: W,
        child: Option<Child>,
        events: mpsc::UnboundedSender<ProviderEvent>,
        initial_settings: SessionSettings,
    ) -> Arc<Self>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let pending = Arc::new(Mutex::new(PendingState {
            applied_settings: initial_settings,
            ..PendingState::default()
        }));
        let handle = Arc::new(Self {
            writer: Mutex::new(Box::new(writer)),
            child: Mutex::new(child),
            next_id: AtomicU64::new(1),
            pending: Arc::clone(&pending),
        });
        tokio::spawn(read_loop(reader, pending, events));
        handle
    }

    async fn write_json(&self, value: &Value) -> Result<(), StructuredChatError> {
        let mut line = serde_json::to_string(value)
            .map_err(|e| StructuredChatError::Provider(format!("serialize rpc frame: {e}")))?;
        line.push('\n');
        let mut w = self.writer.lock().await;
        w.write_all(line.as_bytes())
            .await
            .map_err(|e| StructuredChatError::Provider(format!("write rpc frame: {e}")))?;
        w.flush()
            .await
            .map_err(|e| StructuredChatError::Provider(format!("flush rpc frame: {e}")))?;
        Ok(())
    }

    async fn request(&self, mut payload: Value) -> Result<Value, StructuredChatError> {
        let req_id = format!("fx-mhc-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("id".into(), Value::String(req_id.clone()));
        }
        let (tx, rx) = oneshot::channel();
        {
            self.pending
                .lock()
                .await
                .rpc_responses
                .insert(req_id.clone(), tx);
        }
        if let Err(err) = self.write_json(&payload).await {
            self.pending.lock().await.rpc_responses.remove(&req_id);
            return Err(err);
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(Ok(data))) => Ok(data),
            Ok(Ok(Err(msg))) => Err(StructuredChatError::Provider(msg)),
            Ok(Err(_)) => Err(StructuredChatError::Provider(
                "mhc rpc process closed before answering request".into(),
            )),
            Err(_) => {
                self.pending.lock().await.rpc_responses.remove(&req_id);
                Err(StructuredChatError::Provider(format!(
                    "mhc rpc request {req_id} timed out after {}s",
                    REQUEST_TIMEOUT.as_secs()
                )))
            }
        }
    }

    async fn shutdown(&self) {
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill().await;
        }
    }
}

async fn read_loop<R: AsyncRead + Unpin + Send + 'static>(
    reader: R,
    pending: Arc<Mutex<PendingState>>,
    events: mpsc::UnboundedSender<ProviderEvent>,
) {
    let mut buf_reader = BufReader::new(reader);
    let mut raw = Vec::new();
    let end_reason = loop {
        raw.clear();
        match buf_reader.read_until(b'\n', &mut raw).await {
            Ok(0) => break "eof".to_owned(),
            Ok(_) => {
                let line = String::from_utf8_lossy(&raw);
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
                    continue;
                };
                handle_inbound_frame(value, &pending, &events).await;
            }
            Err(err) => break format!("read error: {err}"),
        }
    };

    let mut state = pending.lock().await;
    for (_, tx) in state.rpc_responses.drain() {
        let _ = tx.send(Err(format!("mhc rpc ended ({end_reason})")));
    }
    state.extension_ui.clear();
    state.active_tool_calls.clear();
    let active_turn = state.active_turn_id.take();
    state.last_assistant_text = None;
    drop(state);

    if let Some(turn_id) = active_turn {
        let _ = events.send(ProviderEvent::TurnCompleted {
            turn_id,
            error: Some(format!("mhc process exited mid-turn ({end_reason})")),
        });
    }
    let _ = events.send(ProviderEvent::Ended { reason: end_reason });
}

async fn handle_inbound_frame(
    value: Value,
    pending: &Arc<Mutex<PendingState>>,
    events: &mpsc::UnboundedSender<ProviderEvent>,
) {
    let Some(frame_type) = value.get("type").and_then(Value::as_str) else {
        return;
    };
    match frame_type {
        "response" => {
            let Some(id) = value.get("id").and_then(Value::as_str) else {
                return;
            };
            let success = value
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let result = if success {
                Ok(value.get("data").cloned().unwrap_or(Value::Null))
            } else {
                let err_msg = value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("rpc command failed");
                let err_code = value.get("errorCode").and_then(Value::as_str);
                Err(match err_code {
                    Some(code) => format!("{code}: {err_msg}"),
                    None => err_msg.to_owned(),
                })
            };
            if let Some(tx) = pending.lock().await.rpc_responses.remove(id) {
                let _ = tx.send(result);
            }
        }
        "extension_ui_request" => {
            handle_extension_ui_request(&value, pending, events).await;
        }
        "commands_changed" => {
            if let Some(items) = value.get("commands").and_then(Value::as_array) {
                let parsed = parse_commands_array(items);
                pending.lock().await.cached_commands = parsed;
            }
        }
        "agent_start" | "turn_start" => {
            let mut st = pending.lock().await;
            if st.active_turn_id.is_none() {
                let turn_id = value
                    .get("turnId")
                    .or_else(|| value.get("turn_id"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("mhc-turn-{}", uuid::Uuid::new_v4()));
                st.active_turn_id = Some(turn_id.clone());
                st.last_assistant_text = None;
                drop(st);
                let _ = events.send(ProviderEvent::TurnStarted { turn_id });
            }
        }
        "agent_end" | "turn_end" => {
            let error = value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    value
                        .get("message")
                        .and_then(|m| m.get("errorMessage"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
            let mut st = pending.lock().await;
            st.extension_ui.clear();
            st.active_tool_calls.clear();
            st.last_assistant_text = None;
            let turn_id = st.active_turn_id.take();
            drop(st);
            if let Some(turn_id) = turn_id {
                let _ = events.send(ProviderEvent::TurnCompleted { turn_id, error });
            }
        }
        "message_update" => {
            let turn_id = pending.lock().await.active_turn_id.clone();
            if let Some(delta_ev) = value.get("assistantMessageEvent") {
                handle_assistant_delta_event(delta_ev, turn_id, pending, events).await;
            }
        }
        "message_end" => {
            let turn_id = pending.lock().await.active_turn_id.clone();
            if let Some(msg) = value.get("message") {
                handle_message_end(msg, turn_id, pending, events).await;
            }
        }
        "tool_execution_start" => {
            let turn_id = pending.lock().await.active_turn_id.clone();
            let id = value
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_owned();
            let name = value
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_owned();
            pending.lock().await.active_tool_calls.insert(id.clone());
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::ToolCall {
                    id,
                    name,
                    status: ToolCallStatus::InProgress,
                    output: value.get("args").map(|a| a.to_string()),
                    exit_code: None,
                    duration_ms: None,
                },
            }));
        }
        "tool_execution_update" => {
            let turn_id = pending.lock().await.active_turn_id.clone();
            let id = value
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_owned();
            let name = value
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_owned();
            let output = value
                .get("partialResult")
                .map(extract_tool_output_text)
                .filter(|s| !s.is_empty());
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::ToolCall {
                    id,
                    name,
                    status: ToolCallStatus::InProgress,
                    output,
                    exit_code: None,
                    duration_ms: None,
                },
            }));
        }
        "tool_execution_end" => {
            let turn_id = pending.lock().await.active_turn_id.clone();
            let id = value
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_owned();
            let name = value
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_owned();
            let is_error = value
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let result_val = value.get("result");
            let output = result_val
                .map(extract_tool_output_text)
                .filter(|s| !s.is_empty());
            let exit_code = result_val
                .and_then(|r| r.get("details"))
                .and_then(|d| d.get("exitCode"))
                .and_then(Value::as_i64)
                .map(|c| c as i32);
            let duration_ms = value.get("durationMs").and_then(Value::as_i64);
            pending.lock().await.active_tool_calls.remove(&id);

            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id: turn_id.clone(),
                payload: PartKind::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    status: if is_error {
                        ToolCallStatus::Failed
                    } else {
                        ToolCallStatus::Completed
                    },
                    output,
                    exit_code,
                    duration_ms,
                },
            }));

            if let Some(diff) = result_val
                .and_then(|r| r.get("details"))
                .and_then(|d| d.get("diff"))
                .and_then(Value::as_str)
            {
                let path = result_val
                    .and_then(|r| r.get("details"))
                    .and_then(|d| d.get("path"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                if !path.is_empty() || !diff.is_empty() {
                    let _ = events.send(ProviderEvent::Part(PartDraft {
                        turn_id: turn_id.clone(),
                        payload: PartKind::FileChange {
                            id: format!("{id}-diff"),
                            status: if is_error {
                                ToolCallStatus::Failed
                            } else {
                                ToolCallStatus::Completed
                            },
                            changes: vec![FileChangeEntry {
                                path,
                                kind: name,
                                diff: diff.to_owned(),
                            }],
                        },
                    }));
                }
            }

            if let Some(contents) = result_val
                .and_then(|r| r.get("content"))
                .and_then(Value::as_array)
            {
                for (idx, item) in contents.iter().enumerate() {
                    if item.get("type").and_then(Value::as_str) == Some("image") {
                        let mime_type = item
                            .get("mimeType")
                            .and_then(Value::as_str)
                            .unwrap_or("image/png")
                            .to_owned();
                        let alt = item
                            .get("alt")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                        let _ = events.send(ProviderEvent::Part(PartDraft {
                            turn_id: turn_id.clone(),
                            payload: PartKind::MediaRef {
                                tool_call_id: id.clone(),
                                content_index: idx as u32,
                                mime_type,
                                alt,
                            },
                        }));
                    }
                }
            }
        }
        "subagent_progress" | "subagent_update" | "task_progress" => {
            let turn_id = pending.lock().await.active_turn_id.clone();
            let task_id = value
                .get("taskId")
                .or_else(|| value.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("task")
                .to_owned();
            let name = value
                .get("name")
                .or_else(|| value.get("agent"))
                .and_then(Value::as_str)
                .unwrap_or("subagent")
                .to_owned();
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("running")
                .to_owned();
            let summary = value
                .get("summary")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::SubagentProgress {
                    task_id,
                    name,
                    status,
                    summary,
                },
            }));
        }
        "goal_update" | "goal_changed" | "ulw_goal_update" => {
            let turn_id = pending.lock().await.active_turn_id.clone();
            let goal_id = value
                .get("goalId")
                .or_else(|| value.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("goal")
                .to_owned();
            let title = value
                .get("title")
                .or_else(|| value.get("objective"))
                .and_then(Value::as_str)
                .unwrap_or("Goal")
                .to_owned();
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("active")
                .to_owned();
            let progress = value
                .get("progress")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::GoalUpdate {
                    goal_id,
                    title,
                    status,
                    progress,
                },
            }));
        }
        "usage" | "token_usage" => {
            let turn_id = pending.lock().await.active_turn_id.clone();
            emit_usage_from_value(&value, turn_id, events);
        }
        "error" | "extension_error" => {
            if let Some(message) = value.get("error").or_else(|| value.get("message")).and_then(Value::as_str) {
                let turn_id = pending.lock().await.active_turn_id.clone();
                let _ = events.send(ProviderEvent::Part(PartDraft {
                    turn_id,
                    payload: PartKind::Error {
                        message: message.to_owned(),
                    },
                }));
            }
        }
        _ => {}
    }
}

async fn handle_extension_ui_request(
    value: &Value,
    pending: &Arc<Mutex<PendingState>>,
    events: &mpsc::UnboundedSender<ProviderEvent>,
) {
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return;
    };
    let request_id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let turn_id = pending.lock().await.active_turn_id.clone();

    match method {
        "setStatus" => {
            let status_key = value
                .get("statusKey")
                .and_then(Value::as_str)
                .unwrap_or("status")
                .to_owned();
            let status_text = value
                .get("statusText")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::ExtensionStatus {
                    status_key,
                    status_text,
                },
            }));
        }
        "setWidget" | "setHeader" | "setFooter" => {
            let widget_key = value
                .get("widgetKey")
                .and_then(Value::as_str)
                .unwrap_or(method)
                .to_owned();
            let lines = value
                .get("widgetLines")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let placement = value
                .get("widgetPlacement")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| Some(method.to_owned()));
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::ExtensionWidget {
                    widget_key,
                    lines,
                    placement,
                },
            }));
        }
        "setTitle" => {
            let title = value
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::ExtensionStatus {
                    status_key: "title".into(),
                    status_text: title,
                },
            }));
        }
        "notify" => {
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let notify_type = value
                .get("notifyType")
                .and_then(Value::as_str)
                .unwrap_or("info");
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::ExtensionStatus {
                    status_key: format!("notify:{notify_type}"),
                    status_text: message,
                },
            }));
        }
        "set_editor_text" => {
            let text = value.get("text").and_then(Value::as_str).map(str::to_owned);
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::ExtensionUiRequest {
                    request_id,
                    method: "set_editor_text".into(),
                    title: None,
                    message: None,
                    placeholder: None,
                    prefill: text,
                    options: Vec::new(),
                    payload: None,
                    answered: true,
                },
            }));
        }
        "select" | "confirm" | "input" | "editor" | "question" => {
            if !request_id.is_empty() {
                pending
                    .lock()
                    .await
                    .extension_ui
                    .insert(request_id.clone(), method.to_owned());
            }
            let title = value.get("title").and_then(Value::as_str).map(str::to_owned);
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let placeholder = value
                .get("placeholder")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let prefill = value
                .get("prefill")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let options = value
                .get("options")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let payload = if method == "question" {
                Some(json!({
                    "requestId": value.get("requestId"),
                    "toolCallId": value.get("toolCallId"),
                    "waitForAnswer": value.get("waitForAnswer"),
                    "questions": value.get("questions"),
                    "timeout": value.get("timeout"),
                }))
            } else {
                None
            };
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::ExtensionUiRequest {
                    request_id,
                    method: method.to_owned(),
                    title,
                    message,
                    placeholder,
                    prefill,
                    options,
                    payload,
                    answered: false,
                },
            }));
        }
        _ => {}
    }
}

async fn handle_assistant_delta_event(
    delta_ev: &Value,
    turn_id: Option<String>,
    pending: &Arc<Mutex<PendingState>>,
    events: &mpsc::UnboundedSender<ProviderEvent>,
) {
    let Some(ev_type) = delta_ev.get("type").and_then(Value::as_str) else {
        return;
    };
    match ev_type {
        "text_delta" => {
            if let Some(delta) = delta_ev.get("delta").and_then(Value::as_str) {
                if !delta.is_empty() {
                    let _ = events.send(ProviderEvent::Part(PartDraft {
                        turn_id,
                        payload: PartKind::AssistantMessage {
                            text: delta.to_owned(),
                        },
                    }));
                }
            }
        }
        "thinking_delta" => {
            if let Some(delta) = delta_ev.get("delta").and_then(Value::as_str) {
                if !delta.is_empty() {
                    let _ = events.send(ProviderEvent::Part(PartDraft {
                        turn_id,
                        payload: PartKind::Reasoning {
                            text: delta.to_owned(),
                        },
                    }));
                }
            }
        }
        "text_end" => {
            if let Some(content) = delta_ev.get("content").and_then(Value::as_str) {
                if !content.is_empty() {
                    pending.lock().await.last_assistant_text = Some(content.to_owned());
                }
            }
        }
        _ => {}
    }
}

async fn handle_message_end(
    msg: &Value,
    turn_id: Option<String>,
    pending: &Arc<Mutex<PendingState>>,
    events: &mpsc::UnboundedSender<ProviderEvent>,
) {
    if msg.get("role").and_then(Value::as_str) != Some("assistant") {
        return;
    }
    if let Some(usage) = msg.get("usage") {
        emit_usage_from_value(usage, turn_id.clone(), events);
    }
    let mut full_text = String::new();
    if let Some(blocks) = msg.get("content").and_then(Value::as_array) {
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(t) = block.get("text").and_then(Value::as_str) {
                        full_text.push_str(t);
                    }
                }
                Some("thinking") => {
                    if let Some(t) = block.get("thinking").and_then(Value::as_str) {
                        if !t.is_empty() {
                            let _ = events.send(ProviderEvent::Part(PartDraft {
                                turn_id: turn_id.clone(),
                                payload: PartKind::Reasoning { text: t.to_owned() },
                            }));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if !full_text.is_empty() {
        let mut st = pending.lock().await;
        if st.last_assistant_text.as_deref() == Some(full_text.as_str()) {
            return;
        }
        st.last_assistant_text = Some(full_text.clone());
        drop(st);
        let _ = events.send(ProviderEvent::Part(PartDraft {
            turn_id,
            payload: PartKind::AssistantMessage { text: full_text },
        }));
    }
}

fn emit_usage_from_value(
    usage: &Value,
    turn_id: Option<String>,
    events: &mpsc::UnboundedSender<ProviderEvent>,
) {
    let input_tokens = usage
        .get("input")
        .or_else(|| usage.get("inputTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .get("output")
        .or_else(|| usage.get("outputTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cached_input_tokens = usage
        .get("cacheRead")
        .or_else(|| usage.get("cachedInputTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total_tokens = usage
        .get("totalTokens")
        .or_else(|| usage.get("total"))
        .and_then(Value::as_u64)
        .unwrap_or(input_tokens + output_tokens + cached_input_tokens);
    let context_window = usage.get("contextWindow").and_then(Value::as_u64);
    if total_tokens > 0 || input_tokens > 0 || output_tokens > 0 {
        let _ = events.send(ProviderEvent::Part(PartDraft {
            turn_id,
            payload: PartKind::Usage {
                total_tokens,
                input_tokens,
                cached_input_tokens,
                output_tokens,
                context_window,
            },
        }));
    }
}

fn extract_tool_output_text(val: &Value) -> String {
    if let Some(s) = val.as_str() {
        return s.to_owned();
    }
    if let Some(arr) = val.get("content").and_then(Value::as_array) {
        let parts: Vec<&str> = arr
            .iter()
            .filter_map(|item| {
                if item.get("type").and_then(Value::as_str) == Some("text") {
                    item.get("text").and_then(Value::as_str)
                } else {
                    None
                }
            })
            .collect();
        if !parts.is_empty() {
            return parts.join("\n");
        }
    }
    String::new()
}

fn parse_commands_array(items: &[Value]) -> Vec<SlashCommandItem> {
    items
        .iter()
        .filter_map(|item| {
            let name = item.get("name").and_then(Value::as_str)?.trim();
            if name.is_empty() {
                return None;
            }
            let source = item
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("builtin")
                .to_owned();
            let syntax = item
                .get("syntax")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    if source == "skill" || name.starts_with('$') {
                        "dollar".into()
                    } else {
                        "slash".into()
                    }
                });
            let description = item
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Some(SlashCommandItem {
                name: name.trim_start_matches(['/', '$']).to_owned(),
                description,
                source,
                syntax,
            })
        })
        .collect()
}

fn split_provider_model(model: &str) -> (Option<String>, String) {
    if let Some((provider, id)) = model.split_once('/') {
        if !provider.is_empty() && !id.is_empty() {
            return (Some(provider.to_owned()), id.to_owned());
        }
    }
    (None, model.to_owned())
}

fn queue_mode_wire(mode: QueueDeliveryMode) -> &'static str {
    match mode {
        QueueDeliveryMode::All => "all",
        QueueDeliveryMode::OneAtATime => "one-at-a-time",
    }
}

fn parse_queue_mode_wire(s: &str) -> Option<QueueDeliveryMode> {
    match s {
        "all" => Some(QueueDeliveryMode::All),
        "one-at-a-time" => Some(QueueDeliveryMode::OneAtATime),
        _ => None,
    }
}

pub struct MahoRpcProvider {
    provider_id: &'static str,
    cwd: PathBuf,
    events: mpsc::UnboundedSender<ProviderEvent>,
    initial_settings: SessionSettings,
    transport: Mutex<Option<Arc<TransportHandle>>>,
}

impl MahoRpcProvider {
    pub fn spawn(
        provider_id: &str,
        cwd: PathBuf,
        events: mpsc::UnboundedSender<ProviderEvent>,
        settings: &SessionSettings,
    ) -> Result<Self, StructuredChatError> {
        let static_id: &'static str = if provider_id == "omo" { "omo" } else { "maho" };
        Ok(Self {
            provider_id: static_id,
            cwd,
            events,
            initial_settings: settings.clone(),
            transport: Mutex::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn from_test_io<R, W>(
        provider_id: &'static str,
        cwd: PathBuf,
        events: mpsc::UnboundedSender<ProviderEvent>,
        settings: SessionSettings,
        reader: R,
        writer: W,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let handle = TransportHandle::spawn_from_io(reader, writer, None, events.clone(), settings.clone());
        Self {
            provider_id,
            cwd,
            events,
            initial_settings: settings,
            transport: Mutex::new(Some(handle)),
        }
    }

    async fn ensure_transport(
        &self,
        resume_thread: Option<&str>,
    ) -> Result<Arc<TransportHandle>, StructuredChatError> {
        let mut guard = self.transport.lock().await;
        if let Some(existing) = guard.as_ref() {
            return Ok(Arc::clone(existing));
        }
        let (program, args) = build_maho_argv(self.provider_id, resume_thread, &self.initial_settings);
        let mut cmd = tokio::process::Command::new(&program);
        cmd.args(&args)
            .current_dir(&self.cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut child = cmd.spawn().map_err(|err| {
            StructuredChatError::Provider(format!("failed to spawn {program} --mode rpc: {err}"))
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            StructuredChatError::Provider("failed to capture mhc rpc stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            StructuredChatError::Provider("failed to capture mhc rpc stdout".into())
        })?;
        let handle = TransportHandle::spawn_from_io(
            stdout,
            stdin,
            Some(child),
            self.events.clone(),
            self.initial_settings.clone(),
        );
        *guard = Some(Arc::clone(&handle));
        Ok(handle)
    }

    async fn require_transport(&self) -> Result<Arc<TransportHandle>, StructuredChatError> {
        self.transport
            .lock()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                StructuredChatError::Conflict("SESSION_ENDED: mhc rpc transport is not started".into())
            })
    }

    async fn apply_settings_delta(
        &self,
        transport: &TransportHandle,
        settings: &SessionSettings,
    ) -> Result<(), StructuredChatError> {
        let prev = transport.pending.lock().await.applied_settings.clone();

        if settings.model != prev.model {
            if let Some(model) = settings.model.as_deref().filter(|m| !m.is_empty()) {
                let (provider, model_id) = split_provider_model(model);
                let provider_name = provider.unwrap_or_else(|| "default".into());
                let _ = transport
                    .request(json!({
                        "type": "set_model",
                        "provider": provider_name,
                        "modelId": model_id,
                    }))
                    .await?;
            }
        }
        if settings.effort != prev.effort {
            if let Some(level) = settings.effort.as_deref().filter(|l| !l.is_empty()) {
                let _ = transport
                    .request(json!({
                        "type": "set_thinking_level",
                        "level": level,
                        "scope": "turn",
                    }))
                    .await?;
            }
        }
        if settings.fast_mode != prev.fast_mode {
            if let Some(enabled) = settings.fast_mode {
                let _ = transport
                    .request(json!({ "type": "set_fast_mode", "enabled": enabled }))
                    .await?;
            }
        }
        if settings.steering_mode != prev.steering_mode {
            if let Some(mode) = settings.steering_mode {
                let _ = transport
                    .request(json!({
                        "type": "set_steering_mode",
                        "mode": queue_mode_wire(mode),
                    }))
                    .await?;
            }
        }
        if settings.follow_up_mode != prev.follow_up_mode {
            if let Some(mode) = settings.follow_up_mode {
                let _ = transport
                    .request(json!({
                        "type": "set_follow_up_mode",
                        "mode": queue_mode_wire(mode),
                    }))
                    .await?;
            }
        }
        if settings.auto_compaction != prev.auto_compaction {
            if let Some(enabled) = settings.auto_compaction {
                let _ = transport
                    .request(json!({ "type": "set_auto_compaction", "enabled": enabled }))
                    .await?;
            }
        }
        if settings.auto_retry != prev.auto_retry {
            if let Some(enabled) = settings.auto_retry {
                let _ = transport
                    .request(json!({ "type": "set_auto_retry", "enabled": enabled }))
                    .await?;
            }
        }

        transport.pending.lock().await.applied_settings = settings.clone();
        Ok(())
    }
}

/// Maximum raw image attachment size inlined as base64 (larger files fall back to `@path`).
const MAX_INLINE_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

fn encode_attachments(text: &str, attachments: &[String]) -> (String, Option<Vec<Value>>) {
    let mut message = text.to_owned();
    let mut images = Vec::new();
    for path in attachments {
        let lower = path.to_ascii_lowercase();
        if let Some(mime) = image_mime_for_ext(&lower) {
            let within_limit = std::fs::metadata(path)
                .map(|meta| meta.is_file() && meta.len() <= MAX_INLINE_IMAGE_BYTES)
                .unwrap_or(false);
            if within_limit {
                if let Ok(bytes) = std::fs::read(path) {
                    use base64::Engine;
                    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                    images.push(json!({
                        "type": "image",
                        "data": data,
                        "mimeType": mime,
                    }));
                    continue;
                }
            }
        }
        if !message.is_empty() && !message.ends_with('\n') {
            message.push('\n');
        }
        message.push('@');
        message.push_str(path);
    }
    let images_opt = if images.is_empty() { None } else { Some(images) };
    (message, images_opt)
}

fn image_mime_for_ext(lower_path: &str) -> Option<&'static str> {
    if lower_path.ends_with(".png") {
        Some("image/png")
    } else if lower_path.ends_with(".jpg") || lower_path.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if lower_path.ends_with(".webp") {
        Some("image/webp")
    } else if lower_path.ends_with(".gif") {
        Some("image/gif")
    } else {
        None
    }
}

fn parse_tree_response(data: &Value) -> SessionTree {
    let current_leaf_id = data
        .get("currentLeafId")
        .or_else(|| data.get("leafId"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut nodes = Vec::new();
    if let Some(entries) = data.get("tree").and_then(Value::as_array) {
        flatten_tree_entries(entries, current_leaf_id.as_deref(), &mut nodes);
    } else if let Some(flat) = data.get("nodes").and_then(Value::as_array) {
        for item in flat {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                nodes.push(SessionTreeNode {
                    id: id.to_owned(),
                    parent_id: item
                        .get("parentId")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    role: item
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or("entry")
                        .to_owned(),
                    preview: item
                        .get("preview")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    label: item.get("label").and_then(Value::as_str).map(str::to_owned),
                    active: item
                        .get("active")
                        .and_then(Value::as_bool)
                        .unwrap_or_else(|| current_leaf_id.as_deref() == Some(id)),
                });
            }
        }
    }
    SessionTree {
        current_leaf_id,
        nodes,
    }
}

fn flatten_tree_entries(
    entries: &[Value],
    current_leaf_id: Option<&str>,
    out: &mut Vec<SessionTreeNode>,
) {
    for node in entries {
        let entry = node.get("entry").unwrap_or(node);
        if let Some(id) = entry.get("id").and_then(Value::as_str) {
            let parent_id = entry
                .get("parentId")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let role = entry
                .get("message")
                .and_then(|m| m.get("role"))
                .and_then(Value::as_str)
                .or_else(|| entry.get("type").and_then(Value::as_str))
                .unwrap_or("entry")
                .to_owned();
            let preview = entry
                .get("message")
                .map(extract_tool_output_text)
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    entry
                        .get("preview")
                        .or_else(|| entry.get("modelId"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            let label = node
                .get("label")
                .or_else(|| entry.get("label"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            out.push(SessionTreeNode {
                id: id.to_owned(),
                parent_id,
                role,
                preview: preview.chars().take(120).collect(),
                label,
                active: current_leaf_id == Some(id),
            });
        }
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            flatten_tree_entries(children, current_leaf_id, out);
        }
    }
}

#[async_trait]
impl StructuredProvider for MahoRpcProvider {
    fn provider_id(&self) -> &'static str {
        self.provider_id
    }

    fn capabilities(&self) -> Capabilities {
        maho_capabilities()
    }

    async fn start(&self, resume_thread: Option<String>) -> Result<String, StructuredChatError> {
        let transport = self.ensure_transport(resume_thread.as_deref()).await?;
        let state_data = transport.request(json!({ "type": "get_state" })).await?;
        let session_id = state_data
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or(resume_thread)
            .unwrap_or_else(|| format!("{}-{}", self.provider_id, uuid::Uuid::new_v4()));
        {
            let mut st = transport.pending.lock().await;
            st.session_id = Some(session_id.clone());
            if let Some(m) = state_data.get("steeringMode").and_then(Value::as_str) {
                st.applied_settings.steering_mode = parse_queue_mode_wire(m);
            }
            if let Some(m) = state_data.get("followUpMode").and_then(Value::as_str) {
                st.applied_settings.follow_up_mode = parse_queue_mode_wire(m);
            }
            if let Some(v) = state_data.get("autoCompactionEnabled").and_then(Value::as_bool) {
                st.applied_settings.auto_compaction = Some(v);
            }
            if let Some(v) = state_data.get("autoRetryEnabled").and_then(Value::as_bool) {
                st.applied_settings.auto_retry = Some(v);
            }
            if let Some(v) = state_data.get("fastMode").and_then(Value::as_bool) {
                st.applied_settings.fast_mode = Some(v);
            }
        }

        if let Ok(cmd_data) = transport.request(json!({ "type": "get_commands" })).await {
            if let Some(arr) = cmd_data.get("commands").and_then(Value::as_array) {
                transport.pending.lock().await.cached_commands = parse_commands_array(arr);
            }
        }
        Ok(session_id)
    }

    async fn turn(
        &self,
        text: String,
        attachments: Vec<String>,
        settings: &SessionSettings,
    ) -> Result<TurnRef, StructuredChatError> {
        let transport = self.require_transport().await?;
        self.apply_settings_delta(&transport, settings).await?;
        let (message, images) =
            crate::ipc::run_blocking(move || Ok(encode_attachments(&text, &attachments)))
                .await
                .map_err(|e| StructuredChatError::Provider(format!("attachment encode: {e}")))?;

        let turn_id = format!("mhc-turn-{}", uuid::Uuid::new_v4());
        {
            let mut st = transport.pending.lock().await;
            st.active_turn_id = Some(turn_id.clone());
            st.last_assistant_text = None;
        }
        let _ = self.events.send(ProviderEvent::TurnStarted {
            turn_id: turn_id.clone(),
        });

        let mut req = json!({
            "type": "prompt",
            "message": message,
        });
        if let Some(imgs) = images {
            req["images"] = Value::Array(imgs);
        }
        if let Some(thinking) = settings.effort.as_deref().filter(|s| !s.is_empty()) {
            req["thinkingLevel"] = Value::String(thinking.to_owned());
        }
        if let Err(err) = transport.request(req).await {
            let mut st = transport.pending.lock().await;
            let active = st.active_turn_id.take();
            drop(st);
            if let Some(tid) = active {
                let _ = self.events.send(ProviderEvent::TurnCompleted {
                    turn_id: tid,
                    error: Some(err.to_string()),
                });
            }
            return Err(err);
        }
        Ok(TurnRef { turn_id })
    }

    async fn steer(&self, _expected_turn: &str, text: String) -> Result<(), StructuredChatError> {
        let transport = self.require_transport().await?;
        transport
            .request(json!({
                "type": "steer",
                "message": text,
            }))
            .await?;
        Ok(())
    }

    async fn interrupt(&self, _turn_id: &str) -> Result<(), StructuredChatError> {
        let transport = self.require_transport().await?;
        transport.request(json!({ "type": "abort" })).await?;
        Ok(())
    }

    async fn rollback(&self, before_turn_id: &str) -> Result<String, StructuredChatError> {
        let transport = self.require_transport().await?;
        let _ = transport
            .request(json!({
                "type": "navigate_tree",
                "targetId": before_turn_id,
                "summarize": false,
            }))
            .await?;
        let sid = transport
            .pending
            .lock()
            .await
            .session_id
            .clone()
            .unwrap_or_else(|| before_turn_id.to_owned());
        Ok(sid)
    }

    async fn answer_approval(
        &self,
        request_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), StructuredChatError> {
        let confirmed = !matches!(decision, ApprovalDecision::Decline);
        self.answer_extension_ui(
            request_id,
            ExtensionUiResponseBody {
                value: None,
                confirmed: Some(confirmed),
                cancelled: Some(!confirmed),
                answers: None,
                comment: None,
            },
        )
        .await
    }

    async fn clear(&self) -> Result<String, StructuredChatError> {
        let transport = self.require_transport().await?;
        let data = transport.request(json!({ "type": "new_session" })).await?;
        if data.get("cancelled").and_then(Value::as_bool) == Some(true) {
            return Err(StructuredChatError::Conflict(
                "new_session was cancelled by provider".into(),
            ));
        }
        let state = transport.request(json!({ "type": "get_state" })).await?;
        let new_id = state
            .get("sessionId")
            .or_else(|| data.get("sessionId"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                StructuredChatError::Provider("mhc get_state omitted sessionId after new_session".into())
            })?;
        transport.pending.lock().await.session_id = Some(new_id.clone());
        Ok(new_id)
    }

    async fn compact(&self) -> Result<(), StructuredChatError> {
        self.compact_with_instructions(None).await
    }

    async fn compact_with_instructions(
        &self,
        instructions: Option<&str>,
    ) -> Result<(), StructuredChatError> {
        let transport = self.require_transport().await?;
        let mut req = json!({ "type": "compact" });
        if let Some(custom) = instructions.filter(|s| !s.trim().is_empty()) {
            req["customInstructions"] = Value::String(custom.to_owned());
        }
        transport.request(req).await?;
        Ok(())
    }

    async fn commands(&self) -> Result<Vec<SlashCommandItem>, StructuredChatError> {
        let transport = self.require_transport().await?;
        match transport.request(json!({ "type": "get_commands" })).await {
            Ok(data) => {
                let parsed = data
                    .get("commands")
                    .and_then(Value::as_array)
                    .map(|arr| parse_commands_array(arr))
                    .unwrap_or_default();
                transport.pending.lock().await.cached_commands = parsed.clone();
                Ok(parsed)
            }
            Err(_) => {
                let cached = transport.pending.lock().await.cached_commands.clone();
                if !cached.is_empty() {
                    Ok(cached)
                } else {
                    Ok(BUILTIN_SLASH_COMMANDS
                        .iter()
                        .map(|s| SlashCommandItem {
                            name: s.trim_start_matches('/').to_owned(),
                            description: None,
                            source: "builtin".into(),
                            syntax: "slash".into(),
                        })
                        .collect())
                }
            }
        }
    }

    async fn answer_extension_ui(
        &self,
        request_id: &str,
        response: ExtensionUiResponseBody,
    ) -> Result<(), StructuredChatError> {
        let transport = self.require_transport().await?;
        transport.pending.lock().await.extension_ui.remove(request_id);
        let mut frame = json!({
            "type": "extension_ui_response",
            "id": request_id,
        });
        if let Some(answers) = response.answers {
            frame["answers"] = answers;
            if let Some(comment) = response.comment {
                frame["comment"] = Value::String(comment);
            }
        } else if let Some(value) = response.value {
            frame["value"] = Value::String(value);
        } else if let Some(confirmed) = response.confirmed {
            frame["confirmed"] = Value::Bool(confirmed);
        } else {
            frame["cancelled"] = Value::Bool(response.cancelled.unwrap_or(true));
        }
        transport.write_json(&frame).await
    }

    async fn get_tree(&self) -> Result<SessionTree, StructuredChatError> {
        let transport = self.require_transport().await?;
        let data = transport.request(json!({ "type": "get_tree" })).await?;
        Ok(parse_tree_response(&data))
    }

    async fn navigate_tree(&self, body: &NavigateTreeBody) -> Result<(), StructuredChatError> {
        let transport = self.require_transport().await?;
        let mut req = json!({
            "type": "navigate_tree",
            "targetId": body.target_id,
            "summarize": body.summarize,
        });
        if let Some(custom) = body.custom_instructions.as_deref().filter(|s| !s.is_empty()) {
            req["customInstructions"] = Value::String(custom.to_owned());
        }
        transport.request(req).await?;
        Ok(())
    }

    async fn fork(&self, entry_id: &str) -> Result<String, StructuredChatError> {
        let transport = self.require_transport().await?;
        let data = transport
            .request(json!({
                "type": "fork",
                "entryId": entry_id,
            }))
            .await?;
        if data.get("cancelled").and_then(Value::as_bool) == Some(true) {
            return Err(StructuredChatError::Conflict(
                "fork was cancelled by provider".into(),
            ));
        }
        let state = transport.request(json!({ "type": "get_state" })).await?;
        let new_id = state
            .get("sessionId")
            .or_else(|| data.get("sessionId"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                StructuredChatError::Provider("mhc get_state omitted sessionId after fork".into())
            })?;
        transport.pending.lock().await.session_id = Some(new_id.clone());
        Ok(new_id)
    }

    async fn clone_session(&self) -> Result<String, StructuredChatError> {
        let transport = self.require_transport().await?;
        let data = transport.request(json!({ "type": "clone" })).await?;
        if data.get("cancelled").and_then(Value::as_bool) == Some(true) {
            return Err(StructuredChatError::Conflict(
                "clone was cancelled by provider".into(),
            ));
        }
        let state = transport.request(json!({ "type": "get_state" })).await?;
        let new_id = state
            .get("sessionId")
            .or_else(|| data.get("sessionId"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                StructuredChatError::Provider("mhc get_state omitted sessionId after clone".into())
            })?;
        transport.pending.lock().await.session_id = Some(new_id.clone());
        Ok(new_id)
    }

    async fn edit_message(&self, body: &EditMessageBody) -> Result<(), StructuredChatError> {
        let transport = self.require_transport().await?;
        let cmd_type = if body.role == "assistant" {
            "edit_assistant_message"
        } else {
            "edit_user_message"
        };
        transport
            .request(json!({
                "type": cmd_type,
                "entryId": body.entry_id,
                "text": body.text,
            }))
            .await?;
        Ok(())
    }

    async fn rename_session(&self, name: &str) -> Result<(), StructuredChatError> {
        let transport = self.require_transport().await?;
        transport
            .request(json!({
                "type": "set_session_name",
                "name": name,
            }))
            .await?;
        Ok(())
    }

    async fn session_stats(&self) -> Result<SessionStats, StructuredChatError> {
        let transport = self.require_transport().await?;
        let data = transport
            .request(json!({ "type": "get_session_stats" }))
            .await?;
        let tokens = data.get("tokens").unwrap_or(&data);
        let input_tokens = tokens
            .get("input")
            .or_else(|| data.get("inputTokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let output_tokens = tokens
            .get("output")
            .or_else(|| data.get("outputTokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let total_tokens = tokens
            .get("total")
            .or_else(|| data.get("totalTokens"))
            .and_then(Value::as_u64)
            .unwrap_or(input_tokens + output_tokens);
        let turns = data
            .get("userMessages")
            .or_else(|| data.get("turns"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let name = data
            .get("sessionName")
            .or_else(|| data.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let context_window = data.get("contextWindow").and_then(Value::as_u64);
        let cost_usd = data
            .get("cost")
            .and_then(|c| c.get("total").or(Some(c)))
            .and_then(Value::as_f64);
        Ok(SessionStats {
            name,
            turns,
            total_tokens,
            input_tokens,
            output_tokens,
            context_window,
            cost_usd,
        })
    }

    async fn reload(&self) -> Result<(), StructuredChatError> {
        let transport = self.require_transport().await?;
        transport.request(json!({ "type": "reload" })).await?;
        Ok(())
    }

    async fn get_media(
        &self,
        tool_call_id: &str,
        content_index: u32,
    ) -> Result<MediaBlob, StructuredChatError> {
        let transport = self.require_transport().await?;
        let data = transport
            .request(json!({
                "type": "get_media",
                "toolCallId": tool_call_id,
                "contentIndex": content_index,
            }))
            .await?;
        let mime_type = data
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream")
            .to_owned();
        let data_base64 = data
            .get("data")
            .or_else(|| data.get("dataBase64"))
            .and_then(Value::as_str)
            .ok_or(StructuredChatError::NotFound)?
            .to_owned();
        Ok(MediaBlob {
            mime_type,
            data_base64,
        })
    }

    async fn clear_queue(&self) -> Result<(), StructuredChatError> {
        let transport = self.require_transport().await?;
        transport.request(json!({ "type": "clear_queue" })).await?;
        Ok(())
    }

    async fn set_model_preferences(
        &self,
        body: &ModelPreferencesBody,
    ) -> Result<(), StructuredChatError> {
        let transport = self.require_transport().await?;
        if body.favorite_models.is_none() && body.scoped_models.is_none() {
            return Ok(());
        }
        let avail = transport
            .request(json!({ "type": "get_available_models" }))
            .await?;
        let catalog = avail
            .get("models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let resolve_wire_models = |ids: &[String]| -> Result<Vec<Value>, StructuredChatError> {
            let mut out = Vec::with_capacity(ids.len());
            for raw in ids {
                let (want_provider, want_id) = split_provider_model(raw);
                let found = catalog.iter().find(|entry| {
                    let entry_id = entry.get("id").and_then(Value::as_str).unwrap_or("");
                    let entry_provider = entry.get("provider").and_then(Value::as_str).unwrap_or("");
                    match &want_provider {
                        Some(p) => entry_provider == p && entry_id == want_id,
                        None => entry_id == want_id || format!("{entry_provider}/{entry_id}") == *raw,
                    }
                });
                let Some(model_obj) = found else {
                    return Err(StructuredChatError::InvalidRequest(format!(
                        "unknown model '{raw}' in mhc available models"
                    )));
                };
                out.push(json!({ "model": model_obj }));
            }
            Ok(out)
        };
        if let Some(favs) = &body.favorite_models {
            let models = resolve_wire_models(favs)?;
            transport
                .request(json!({
                    "type": "set_favorite_models",
                    "models": models,
                }))
                .await?;
        }
        if let Some(scoped) = &body.scoped_models {
            let models = resolve_wire_models(scoped)?;
            transport
                .request(json!({
                    "type": "set_scoped_models",
                    "models": models,
                }))
                .await?;
        }
        Ok(())
    }

    async fn close(&self) {
        if let Some(transport) = self.transport.lock().await.take() {
            transport.shutdown().await;
        }
    }

    fn event_sink(&self) -> mpsc::UnboundedSender<ProviderEvent> {
        self.events.clone()
    }
}

pub async fn discover_models_with_commands(
    provider_id: &str,
    cwd: &Path,
) -> Result<(Vec<ModelInfo>, Vec<String>), StructuredChatError> {
    let program = if provider_id == "omo" { "omo" } else { "mhc" };
    let fallback_commands = maho_capabilities().slash_commands;

    let mut cmd = tokio::process::Command::new(program);
    cmd.args(["--list-models"])
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    let output = match tokio::time::timeout(DISCOVERY_TIMEOUT, cmd.output()).await {
        Ok(Ok(out)) if out.status.success() => out,
        _ => return Ok((Vec::new(), fallback_commands)),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let models = parse_list_models_output(&stdout);
    Ok((models, fallback_commands))
}

pub(crate) fn parse_list_models_output(output: &str) -> Vec<ModelInfo> {
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    let mut saw_table_header = false;
    let thinking = vec![
        "off".into(),
        "minimal".into(),
        "low".into(),
        "medium".into(),
        "high".into(),
        "xhigh".into(),
        "max".into(),
    ];
    for raw_line in output.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("---") {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 2
            && cols[0].eq_ignore_ascii_case("provider")
            && cols[1].eq_ignore_ascii_case("model")
        {
            saw_table_header = true;
            continue;
        }
        if !saw_table_header {
            continue;
        }
        let id = if cols.len() >= 2 && !cols[0].contains('/') && !cols[1].starts_with('(') {
            format!("{}/{}", cols[0], cols[1])
        } else {
            cols[0].to_owned()
        };
        if seen.insert(id.clone()) {
            models.push(ModelInfo {
                label: id.clone(),
                id,
                efforts: thinking.clone(),
            });
        }
    }
    models
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    struct TestServer {
        server_reader: BufReader<tokio::io::DuplexStream>,
        server_writer: tokio::io::DuplexStream,
    }

    impl TestServer {
        async fn recv_req(&mut self) -> Value {
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(5), self.server_reader.read_line(&mut line))
                .await
                .expect("server recv timed out")
                .unwrap();
            serde_json::from_str(line.trim()).unwrap()
        }

        async fn send_line(&mut self, val: &Value) {
            let mut s = serde_json::to_string(val).unwrap();
            s.push('\n');
            self.server_writer.write_all(s.as_bytes()).await.unwrap();
            self.server_writer.flush().await.unwrap();
        }

        async fn reply_ok(&mut self, req: &Value, data: Value) {
            self.send_line(&json!({
                "id": req["id"],
                "type": "response",
                "command": req["type"],
                "success": true,
                "data": data,
            }))
            .await;
        }
    }

    struct TestHarness {
        provider: MahoRpcProvider,
        events_rx: mpsc::UnboundedReceiver<ProviderEvent>,
        srv: TestServer,
    }

    impl TestHarness {
        fn new(provider_id: &'static str) -> Self {
            let (client_read, server_write) = tokio::io::duplex(65536);
            let (server_read, client_write) = tokio::io::duplex(65536);
            let (events_tx, events_rx) = mpsc::unbounded_channel();
            let provider = MahoRpcProvider::from_test_io(
                provider_id,
                PathBuf::from("/tmp"),
                events_tx,
                SessionSettings::default(),
                client_read,
                client_write,
            );
            Self {
                provider,
                events_rx,
                srv: TestServer {
                    server_reader: BufReader::new(server_read),
                    server_writer: server_write,
                },
            }
        }
    }

    #[test]
    fn argv_never_passes_cwd_or_plan_and_maps_flags() {
        let settings = SessionSettings {
            model: Some("mahoquot/claude-opus-5-5".into()),
            effort: Some("high".into()),
            execution_mode: Some("plan".into()),
            ..SessionSettings::default()
        };
        let (prog, args) = build_maho_argv("maho", Some("sess-123"), &settings);
        assert_eq!(prog, "mhc");
        assert!(!args.iter().any(|a| a == "--cwd"));
        assert!(!args.iter().any(|a| a == "--plan"));
        assert!(args.windows(2).any(|w| w == ["--mode", "rpc"]));
        assert!(args.windows(2).any(|w| w == ["--session", "sess-123"]));
        assert!(args.windows(2).any(|w| w == ["--model", "mahoquot/claude-opus-5-5"]));
        assert!(args.windows(2).any(|w| w == ["--thinking", "high"]));

        let (omo_prog, _) = build_maho_argv("omo", None, &SessionSettings::default());
        assert_eq!(omo_prog, "omo");
    }

    #[test]
    fn capabilities_advertise_full_mhc_surface() {
        let caps = maho_capabilities();
        assert!(caps.steer && caps.rollback && caps.resume);
        assert!(caps.fast_mode && caps.queue_modes && caps.compact_custom_instructions);
        assert!(caps.tree_navigation && caps.fork && caps.clone_session);
        assert!(caps.edit_user_message && caps.edit_assistant_message);
        assert!(caps.session_rename && caps.session_stats && caps.session_switch);
        assert!(caps.reload && caps.media && caps.extension_ui && caps.goals);
        assert!(caps.favorite_models && caps.scoped_models);
        assert_eq!(caps.thinking_levels.len(), 7);
    }

    #[tokio::test]
    async fn start_handshake_ignores_passive_ui_and_caches_commands() {
        let mut h = TestHarness::new("maho");
        let (sid_res, ()) = tokio::join!(h.provider.start(None), async {
            h.srv.send_line(&json!({
                "type": "extension_ui_request",
                "id": "w1",
                "method": "setStatus",
                "statusKey": "senpi",
                "statusText": "ready"
            })).await;
            h.srv.send_line(&json!({
                "type": "extension_ui_request",
                "id": "w2",
                "method": "setWidget",
                "widgetKey": "goal",
                "widgetLines": ["Goal: active"],
                "widgetPlacement": "aboveEditor"
            })).await;
            let req1 = h.srv.recv_req().await;
            assert_eq!(req1["type"], "get_state");
            h.srv.reply_ok(&req1, json!({ "sessionId": "mhc-s-1" })).await;
            let req2 = h.srv.recv_req().await;
            assert_eq!(req2["type"], "get_commands");
            h.srv.reply_ok(&req2, json!({
                "commands": [
                    { "name": "tree", "description": "Navigate tree", "source": "builtin", "syntax": "slash" },
                    { "name": "debugging", "description": "Debug skill", "source": "skill", "syntax": "dollar" }
                ]
            })).await;
        });
        assert_eq!(sid_res.unwrap(), "mhc-s-1");
        let ev1 = h.events_rx.recv().await.unwrap();
        assert!(matches!(ev1, ProviderEvent::Part(PartDraft { payload: PartKind::ExtensionStatus { .. }, .. })));
        let ev2 = h.events_rx.recv().await.unwrap();
        assert!(matches!(ev2, ProviderEvent::Part(PartDraft { payload: PartKind::ExtensionWidget { .. }, .. })));
    }

    #[tokio::test]
    async fn turn_applies_session_scoped_settings_without_global_mutation() {
        let mut h = TestHarness::new("maho");
        let settings = SessionSettings {
            model: Some("openai/gpt-5".into()),
            effort: Some("xhigh".into()),
            fast_mode: Some(true),
            steering_mode: Some(QueueDeliveryMode::OneAtATime),
            follow_up_mode: Some(QueueDeliveryMode::All),
            auto_compaction: Some(true),
            auto_retry: Some(false),
            ..SessionSettings::default()
        };
        let (tref_res, ()) = tokio::join!(h.provider.turn("hello".into(), vec![], &settings), async {
            let r_model = h.srv.recv_req().await;
            assert_eq!(r_model["type"], "set_model");
            assert_eq!(r_model["provider"], "openai");
            assert_eq!(r_model["modelId"], "gpt-5");
            h.srv.reply_ok(&r_model, json!({})).await;

            let r_think = h.srv.recv_req().await;
            assert_eq!(r_think["type"], "set_thinking_level");
            assert_eq!(r_think["level"], "xhigh");
            assert_eq!(r_think["scope"], "turn");
            h.srv.reply_ok(&r_think, json!({})).await;

            let r_fast = h.srv.recv_req().await;
            assert_eq!(r_fast["type"], "set_fast_mode");
            assert_eq!(r_fast["enabled"], true);
            h.srv.reply_ok(&r_fast, json!({})).await;

            let r_steer = h.srv.recv_req().await;
            assert_eq!(r_steer["type"], "set_steering_mode");
            assert_eq!(r_steer["mode"], "one-at-a-time");
            h.srv.reply_ok(&r_steer, json!({})).await;

            let r_fu = h.srv.recv_req().await;
            assert_eq!(r_fu["type"], "set_follow_up_mode");
            assert_eq!(r_fu["mode"], "all");
            h.srv.reply_ok(&r_fu, json!({})).await;

            let r_ac = h.srv.recv_req().await;
            assert_eq!(r_ac["type"], "set_auto_compaction");
            h.srv.reply_ok(&r_ac, json!({})).await;

            let r_ar = h.srv.recv_req().await;
            assert_eq!(r_ar["type"], "set_auto_retry");
            assert_eq!(r_ar["enabled"], false);
            h.srv.reply_ok(&r_ar, json!({})).await;

            let r_prompt = h.srv.recv_req().await;
            assert_eq!(r_prompt["type"], "prompt");
            assert_eq!(r_prompt["message"], "hello");
            assert_eq!(r_prompt["thinkingLevel"], "xhigh");
            h.srv.reply_ok(&r_prompt, json!({})).await;
        });
        assert!(tref_res.unwrap().turn_id.starts_with("mhc-turn-"));
    }

    #[tokio::test]
    async fn streams_text_thinking_tool_diff_media_goal_subagent_and_usage() {
        let mut h = TestHarness::new("maho");
        h.srv.send_line(&json!({ "type": "turn_start", "turnId": "t-1" })).await;
        h.srv.send_line(&json!({ "type": "message_update", "assistantMessageEvent": { "type": "thinking_delta", "delta": "plan" } })).await;
        h.srv.send_line(&json!({ "type": "message_update", "assistantMessageEvent": { "type": "text_delta", "delta": "hi" } })).await;
        h.srv.send_line(&json!({ "type": "tool_execution_start", "toolCallId": "tc1", "toolName": "edit", "args": {"path": "a.rs"} })).await;
        h.srv.send_line(&json!({
            "type": "tool_execution_end",
            "toolCallId": "tc1",
            "toolName": "edit",
            "isError": false,
            "result": {
                "content": [
                    { "type": "text", "text": "edited" },
                    { "type": "image", "mimeType": "image/png", "alt": "shot" }
                ],
                "details": { "path": "a.rs", "diff": "+ok", "exitCode": 0 }
            }
        })).await;
        h.srv.send_line(&json!({ "type": "subagent_progress", "taskId": "st1", "name": "explore", "status": "running", "summary": "scanning" })).await;
        h.srv.send_line(&json!({ "type": "goal_update", "goalId": "g1", "title": "Parity", "status": "active", "progress": "2/5" })).await;
        h.srv.send_line(&json!({ "type": "usage", "input": 100, "output": 50, "cacheRead": 20, "contextWindow": 200000 })).await;
        h.srv.send_line(&json!({ "type": "turn_end" })).await;

        let mut kinds = Vec::new();
        while let Some(ev) = h.events_rx.recv().await {
            match ev {
                ProviderEvent::TurnStarted { .. } => kinds.push("start"),
                ProviderEvent::Part(p) => match p.payload {
                    PartKind::Reasoning { .. } => kinds.push("reasoning"),
                    PartKind::AssistantMessage { .. } => kinds.push("assistant"),
                    PartKind::ToolCall { status: ToolCallStatus::InProgress, .. } => kinds.push("tool_start"),
                    PartKind::ToolCall { status: ToolCallStatus::Completed, .. } => kinds.push("tool_end"),
                    PartKind::FileChange { .. } => kinds.push("file_change"),
                    PartKind::MediaRef { .. } => kinds.push("media_ref"),
                    PartKind::SubagentProgress { .. } => kinds.push("subagent"),
                    PartKind::GoalUpdate { .. } => kinds.push("goal"),
                    PartKind::Usage { .. } => kinds.push("usage"),
                    _ => {}
                },
                ProviderEvent::TurnCompleted { error, .. } => {
                    assert!(error.is_none());
                    kinds.push("end");
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(
            kinds,
            vec!["start", "reasoning", "assistant", "tool_start", "tool_end", "file_change", "media_ref", "subagent", "goal", "usage", "end"]
        );
    }

    #[tokio::test]
    async fn deduplicates_identical_back_to_back_assistant_messages() {
        let mut h = TestHarness::new("maho");
        h.srv.send_line(&json!({ "type": "turn_start", "turnId": "t-dup" })).await;
        let msg = json!({
            "type": "message_end",
            "message": { "role": "assistant", "content": [{ "type": "text", "text": "same reply" }] }
        });
        h.srv.send_line(&msg).await;
        h.srv.send_line(&msg).await;
        h.srv.send_line(&json!({ "type": "turn_end" })).await;

        let mut assistant_count = 0;
        while let Some(ev) = h.events_rx.recv().await {
            match ev {
                ProviderEvent::Part(PartDraft { payload: PartKind::AssistantMessage { .. }, .. }) => {
                    assistant_count += 1;
                }
                ProviderEvent::TurnCompleted { .. } => break,
                _ => {}
            }
        }
        assert_eq!(assistant_count, 1);
    }

    #[tokio::test]
    async fn interactive_extension_ui_requests_never_auto_cancel_and_roundtrip() {
        let mut h = TestHarness::new("maho");
        for (id, method, extra) in [
            ("u1", "select", json!({ "title": "Pick", "options": ["A", "B"] })),
            ("u2", "confirm", json!({ "title": "Confirm", "message": "Proceed?" })),
            ("u3", "input", json!({ "title": "Name", "placeholder": "enter" })),
            ("u4", "editor", json!({ "title": "Edit", "prefill": "abc" })),
            ("u5", "question", json!({ "requestId": "q1", "toolCallId": "tc", "waitForAnswer": true, "questions": [], "timeout": 30000 })),
        ] {
            let mut req = extra;
            req["type"] = json!("extension_ui_request");
            req["id"] = json!(id);
            req["method"] = json!(method);
            h.srv.send_line(&req).await;
            let ev = h.events_rx.recv().await.unwrap();
            match ev {
                ProviderEvent::Part(PartDraft {
                    payload: PartKind::ExtensionUiRequest { request_id, method: m, answered, .. },
                    ..
                }) => {
                    assert_eq!(request_id, id);
                    assert_eq!(m, method);
                    assert!(!answered);
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }

        h.provider.answer_extension_ui("u1", ExtensionUiResponseBody {
            value: Some("A".into()), confirmed: None, cancelled: None, answers: None, comment: None,
        }).await.unwrap();
        let sent = h.srv.recv_req().await;
        assert_eq!(sent["type"], "extension_ui_response");
        assert_eq!(sent["id"], "u1");
        assert_eq!(sent["value"], "A");

        h.provider.answer_extension_ui("u2", ExtensionUiResponseBody {
            value: None, confirmed: Some(true), cancelled: None, answers: None, comment: None,
        }).await.unwrap();
        let sent2 = h.srv.recv_req().await;
        assert_eq!(sent2["confirmed"], true);

        h.provider.answer_extension_ui("u5", ExtensionUiResponseBody {
            value: None, confirmed: None, cancelled: None,
            answers: Some(json!({ "q1": { "selected": ["Yes"] } })), comment: Some("ok".into()),
        }).await.unwrap();
        let sent5 = h.srv.recv_req().await;
        assert_eq!(sent5["answers"]["q1"]["selected"][0], "Yes");
        assert_eq!(sent5["comment"], "ok");
    }

    #[tokio::test]
    async fn invalid_utf8_and_malformed_json_do_not_kill_session() {
        let mut h = TestHarness::new("maho");
        h.srv.server_writer.write_all(b"\xff\xfe bad utf8\nnot json\n").await.unwrap();
        h.srv.server_writer.flush().await.unwrap();

        let (stats_res, ()) = tokio::join!(h.provider.session_stats(), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "get_session_stats");
            h.srv.reply_ok(&req, json!({ "userMessages": 3, "tokens": { "input": 10, "output": 20, "total": 30 } })).await;
        });
        let stats = stats_res.unwrap();
        assert_eq!(stats.turns, 3);
        assert_eq!(stats.total_tokens, 30);
    }

    #[tokio::test]
    async fn child_eof_mid_turn_emits_turn_completed_error_before_ended() {
        let mut h = TestHarness::new("maho");
        h.srv.send_line(&json!({ "type": "turn_start", "turnId": "t-crash" })).await;
        let _ = h.events_rx.recv().await.unwrap();
        drop(h.srv.server_writer);

        let ev1 = h.events_rx.recv().await.unwrap();
        assert!(matches!(ev1, ProviderEvent::TurnCompleted { turn_id, error: Some(_) } if turn_id == "t-crash"));
        let ev2 = h.events_rx.recv().await.unwrap();
        assert!(matches!(ev2, ProviderEvent::Ended { .. }));
    }

    #[tokio::test]
    async fn steer_abort_clear_queue_and_compact_with_instructions() {
        let mut h = TestHarness::new("maho");
        let (r1, ()) = tokio::join!(h.provider.steer("t1", "focus on tests".into()), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "steer");
            assert_eq!(req["message"], "focus on tests");
            h.srv.reply_ok(&req, json!({})).await;
        });
        r1.unwrap();

        let (r2, ()) = tokio::join!(h.provider.interrupt("t1"), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "abort");
            h.srv.reply_ok(&req, json!({})).await;
        });
        r2.unwrap();

        let (r3, ()) = tokio::join!(h.provider.clear_queue(), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "clear_queue");
            h.srv.reply_ok(&req, json!({})).await;
        });
        r3.unwrap();

        let (r4, ()) = tokio::join!(h.provider.compact_with_instructions(Some("keep diffs")), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "compact");
            assert_eq!(req["customInstructions"], "keep diffs");
            h.srv.reply_ok(&req, json!({})).await;
        });
        r4.unwrap();
    }

    #[tokio::test]
    async fn tree_navigate_fork_and_clone_verbs() {
        let mut h = TestHarness::new("maho");
        let (tree_res, ()) = tokio::join!(h.provider.get_tree(), async {
            let r_tree = h.srv.recv_req().await;
            assert_eq!(r_tree["type"], "get_tree");
            h.srv.reply_ok(&r_tree, json!({
                "currentLeafId": "e2",
                "tree": [{
                    "entry": { "id": "e1", "parentId": null, "type": "message", "message": { "role": "user", "content": [{ "type": "text", "text": "hi" }] } },
                    "children": [{
                        "entry": { "id": "e2", "parentId": "e1", "type": "message", "message": { "role": "assistant", "content": [{ "type": "text", "text": "hello" }] } }
                    }]
                }]
            })).await;
        });
        let tree = tree_res.unwrap();
        assert_eq!(tree.current_leaf_id.as_deref(), Some("e2"));
        assert_eq!(tree.nodes.len(), 2);
        assert!(tree.nodes[1].active);

        let nav_body = NavigateTreeBody {
            target_id: "e1".into(),
            summarize: true,
            custom_instructions: Some("brief".into()),
        };
        let (nav_res, ()) = tokio::join!(h.provider.navigate_tree(&nav_body), async {
            let r_nav = h.srv.recv_req().await;
            assert_eq!(r_nav["type"], "navigate_tree");
            assert_eq!(r_nav["targetId"], "e1");
            assert_eq!(r_nav["summarize"], true);
            h.srv.reply_ok(&r_nav, json!({})).await;
        });
        nav_res.unwrap();

        let (fork_res, ()) = tokio::join!(h.provider.fork("e1"), async {
            let r_fork = h.srv.recv_req().await;
            assert_eq!(r_fork["type"], "fork");
            assert_eq!(r_fork["entryId"], "e1");
            h.srv.reply_ok(&r_fork, json!({ "text": "", "cancelled": false })).await;
            let r_st = h.srv.recv_req().await;
            assert_eq!(r_st["type"], "get_state");
            h.srv.reply_ok(&r_st, json!({ "sessionId": "forked-1" })).await;
        });
        assert_eq!(fork_res.unwrap(), "forked-1");

        let (clone_res, ()) = tokio::join!(h.provider.clone_session(), async {
            let r_clone = h.srv.recv_req().await;
            assert_eq!(r_clone["type"], "clone");
            h.srv.reply_ok(&r_clone, json!({ "cancelled": false })).await;
            let r_st = h.srv.recv_req().await;
            assert_eq!(r_st["type"], "get_state");
            h.srv.reply_ok(&r_st, json!({ "sessionId": "cloned-1" })).await;
        });
        assert_eq!(clone_res.unwrap(), "cloned-1");
    }

    #[tokio::test]
    async fn edit_user_and_assistant_messages_dispatch_exact_rpc_commands() {
        let mut h = TestHarness::new("maho");
        let edit_u = EditMessageBody { entry_id: "e1".into(), role: "user".into(), text: "updated user".into() };
        let (ru, ()) = tokio::join!(h.provider.edit_message(&edit_u), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "edit_user_message");
            assert_eq!(req["entryId"], "e1");
            h.srv.reply_ok(&req, json!({})).await;
        });
        ru.unwrap();

        let edit_a = EditMessageBody { entry_id: "e2".into(), role: "assistant".into(), text: "updated assistant".into() };
        let (ra, ()) = tokio::join!(h.provider.edit_message(&edit_a), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "edit_assistant_message");
            assert_eq!(req["entryId"], "e2");
            h.srv.reply_ok(&req, json!({})).await;
        });
        ra.unwrap();
    }

    #[tokio::test]
    async fn rename_reload_get_media_clear_and_model_preferences() {
        let mut h = TestHarness::new("maho");

        let (rn, ()) = tokio::join!(h.provider.rename_session("sprint-1"), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "set_session_name");
            assert_eq!(req["name"], "sprint-1");
            h.srv.reply_ok(&req, json!({})).await;
        });
        rn.unwrap();

        let (rl, ()) = tokio::join!(h.provider.reload(), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "reload");
            h.srv.reply_ok(&req, json!({})).await;
        });
        rl.unwrap();

        let (med, ()) = tokio::join!(h.provider.get_media("tc-1", 0), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "get_media");
            assert_eq!(req["toolCallId"], "tc-1");
            assert_eq!(req["contentIndex"], 0);
            h.srv.reply_ok(&req, json!({ "mimeType": "image/png", "data": "iVBORw0=" })).await;
        });
        let blob = med.unwrap();
        assert_eq!(blob.mime_type, "image/png");
        assert_eq!(blob.data_base64, "iVBORw0=");

        let mp_body = ModelPreferencesBody {
            favorite_models: Some(vec!["mahoquot/claude-opus-5-5".into()]),
            scoped_models: Some(vec!["openai/gpt-5".into()]),
        };
        let (mp, ()) = tokio::join!(h.provider.set_model_preferences(&mp_body), async {
            let r_avail = h.srv.recv_req().await;
            assert_eq!(r_avail["type"], "get_available_models");
            h.srv.reply_ok(&r_avail, json!({
                "models": [
                    { "provider": "mahoquot", "id": "claude-opus-5-5", "name": "Opus 5.5" },
                    { "provider": "openai", "id": "gpt-5", "name": "GPT-5" }
                ]
            })).await;
            let r_fav = h.srv.recv_req().await;
            assert_eq!(r_fav["type"], "set_favorite_models");
            assert_eq!(r_fav["models"][0]["model"]["id"], "claude-opus-5-5");
            h.srv.reply_ok(&r_fav, json!({})).await;
            let r_sc = h.srv.recv_req().await;
            assert_eq!(r_sc["type"], "set_scoped_models");
            assert_eq!(r_sc["models"][0]["model"]["id"], "gpt-5");
            h.srv.reply_ok(&r_sc, json!({})).await;
        });
        mp.unwrap();

        let (cl, ()) = tokio::join!(h.provider.clear(), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "new_session");
            h.srv.reply_ok(&req, json!({ "cancelled": false })).await;
            let r_st = h.srv.recv_req().await;
            assert_eq!(r_st["type"], "get_state");
            h.srv.reply_ok(&r_st, json!({ "sessionId": "fresh-1" })).await;
        });
        assert_eq!(cl.unwrap(), "fresh-1");
    }

    #[tokio::test]
    async fn commands_rpc_preserves_slash_and_dollar_syntax() {
        let mut h = TestHarness::new("maho");
        let (cmds_res, ()) = tokio::join!(h.provider.commands(), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "get_commands");
            h.srv.reply_ok(&req, json!({
                "commands": [
                    { "name": "compact", "source": "builtin", "syntax": "slash" },
                    { "name": "visual-qa", "source": "skill", "syntax": "dollar" }
                ]
            })).await;
        });
        let cmds = cmds_res.unwrap();
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].syntax, "slash");
        assert_eq!(cmds[1].syntax, "dollar");
    }

    #[tokio::test]
    async fn answer_approval_maps_accept_and_decline_to_confirmed() {
        let mut h = TestHarness::new("maho");
        h.provider.answer_approval("ap1", ApprovalDecision::Accept).await.unwrap();
        let r1 = h.srv.recv_req().await;
        assert_eq!(r1["confirmed"], true);
        h.provider.answer_approval("ap2", ApprovalDecision::Decline).await.unwrap();
        let r2 = h.srv.recv_req().await;
        assert_eq!(r2["confirmed"], false);
    }

    #[tokio::test]
    async fn set_editor_text_emits_answered_extension_ui_prefill() {
        let mut h = TestHarness::new("maho");
        h.srv.send_line(&json!({
            "type": "extension_ui_request",
            "id": "ed1",
            "method": "set_editor_text",
            "text": "/compact keep diff"
        })).await;
        let ev = h.events_rx.recv().await.unwrap();
        match ev {
            ProviderEvent::Part(PartDraft {
                payload: PartKind::ExtensionUiRequest { method, prefill, answered, .. },
                ..
            }) => {
                assert_eq!(method, "set_editor_text");
                assert_eq!(prefill.as_deref(), Some("/compact keep diff"));
                assert!(answered);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_list_models_parses_provider_and_id_columns() {
        let sample = "provider   model\n---\nmahoquot   claude-opus-5-5\nopenai/gpt-5\n";
        let models = parse_list_models_output(sample);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "mahoquot/claude-opus-5-5");
        assert_eq!(models[1].id, "openai/gpt-5");
        assert!(parse_list_models_output("No models available. Run mhc auth login.\n").is_empty());
    }

    #[tokio::test]
    async fn rpc_error_response_surfaces_error_code_and_message() {
        let mut h = TestHarness::new("maho");
        let (res, ()) = tokio::join!(h.provider.reload(), async {
            let req = h.srv.recv_req().await;
            h.srv.send_line(&json!({
                "id": req["id"],
                "type": "response",
                "command": "reload",
                "success": false,
                "error": "streaming",
                "errorCode": "streaming"
            })).await;
        });
        assert!(res.unwrap_err().to_string().contains("streaming"));
    }

    #[tokio::test]
    async fn rollback_calls_navigate_tree_without_summarizing() {
        let mut h = TestHarness::new("maho");
        let (res, ()) = tokio::join!(h.provider.rollback("entry-9"), async {
            let req = h.srv.recv_req().await;
            assert_eq!(req["type"], "navigate_tree");
            assert_eq!(req["targetId"], "entry-9");
            assert_eq!(req["summarize"], false);
            h.srv.reply_ok(&req, json!({})).await;
        });
        let _ = res.unwrap();
    }

    #[test]
    fn encode_attachments_keeps_non_images_as_at_mentions() {
        let (msg, imgs) = encode_attachments("check this", &["src/lib.rs".into()]);
        assert_eq!(msg, "check this\n@src/lib.rs");
        assert!(imgs.is_none());
    }
}
