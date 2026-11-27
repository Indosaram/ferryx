//! Agent Client Protocol (ACP) JSON-RPC 2.0 structured-chat provider for `opencode acp`
//! and `cline --acp`.
//!
//! Verified against live `opencode acp --help`, `cline --help`, and live `opencode acp`
//! `initialize` response:
//! * `opencode` -> `opencode acp --cwd <dir>`
//! * `cline` -> `cline --acp --cwd <dir>` (+ `-p`, `-y`, `--thinking`, `--compaction`, `-m`)
//! * JSON-RPC 2.0 handshake: `initialize` -> `session/new` (or `session/load` on resume)
//! * Turns: optional `session/set_mode` / `session/set_model`, then `session/prompt`
//! * Notifications: `session/update` (`agent_message_chunk`, `agent_thought_chunk`, `tool_call`,
//!   `tool_call_update`, `plan`, `available_commands_update`, `current_mode_update`)
//! * Server requests: `session/request_permission` -> `PartKind::ApprovalRequest` -> JSON-RPC reply
//! * Interrupt: `session/cancel` notification.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::{mpsc, oneshot, Mutex};

use super::types::{
    ApprovalDecision, Capabilities, FileChangeEntry, ModelInfo, PartKind, PermissionMode,
    SessionSettings, SlashCommandItem, StructuredChatError, ToolCallStatus,
};
use super::{PartDraft, ProviderEvent, StructuredProvider, TurnRef};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

pub fn acp_capabilities(provider_id: &str) -> Capabilities {
    match provider_id {
        "cline" => Capabilities {
            steer: false,
            rollback: false,
            resume: true,
            revert_files: false,
            history_paging: true,
            permission_modes: vec![PermissionMode::Supervised, PermissionMode::FullAccess],
            models: Vec::new(),
            slash_commands: vec!["/clear".into(), "/compact".into(), "/plan".into(), "/act".into()],
            execution_modes: vec!["act".into(), "plan".into()],
            thinking_levels: vec![
                "none".into(),
                "low".into(),
                "medium".into(),
                "high".into(),
                "xhigh".into(),
            ],
            ..Capabilities::default()
        },
        _ => Capabilities {
            steer: false,
            rollback: false,
            resume: true,
            revert_files: false,
            history_paging: true,
            permission_modes: vec![PermissionMode::Supervised, PermissionMode::FullAccess],
            models: Vec::new(),
            slash_commands: vec!["/clear".into(), "/compact".into(), "/help".into()],
            execution_modes: vec!["build".into(), "plan".into()],
            ..Capabilities::default()
        },
    }
}

pub fn build_acp_argv(
    provider_id: &str,
    cwd: &Path,
    settings: &SessionSettings,
) -> (String, Vec<String>) {
    let cwd_str = cwd.to_string_lossy().into_owned();
    if provider_id == "cline" {
        let mut args = vec!["--acp".to_owned(), "--cwd".to_owned(), cwd_str];
        if settings.execution_mode.as_deref() == Some("plan") {
            args.push("--plan".to_owned());
        }
        if settings.permission_mode == Some(PermissionMode::FullAccess) {
            args.push("--yolo".to_owned());
        }
        if let Some(thinking) = settings.effort.as_deref().filter(|s| !s.trim().is_empty()) {
            args.push("--thinking".to_owned());
            args.push(thinking.to_owned());
        }
        if let Some(comp) = settings
            .compaction_mode
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            args.push("--compaction".to_owned());
            args.push(comp.to_owned());
        }
        if let Some(model) = settings.model.as_deref().filter(|s| !s.trim().is_empty()) {
            args.push("-m".to_owned());
            args.push(model.to_owned());
        }
        ("cline".to_owned(), args)
    } else {
        (
            "opencode".to_owned(),
            vec!["acp".to_owned(), "--cwd".to_owned(), cwd_str],
        )
    }
}

#[derive(Debug, Clone)]
struct PendingPermission {
    rpc_id: Value,
    allow_once: String,
    allow_always: String,
    reject_once: String,
}

#[derive(Debug, Default)]
struct AcpState {
    rpc_responses: HashMap<i64, oneshot::Sender<Result<Value, String>>>,
    pending_permissions: HashMap<String, PendingPermission>,
    session_id: Option<String>,
    active_turn_id: Option<String>,
    current_mode: Option<String>,
    current_model: Option<String>,
    available_commands: Vec<SlashCommandItem>,
    supports_load_session: bool,
}

struct AcpTransport {
    writer: Mutex<Box<dyn AsyncWrite + Unpin + Send>>,
    child: Mutex<Option<Child>>,
    next_id: AtomicI64,
    state: Arc<Mutex<AcpState>>,
}

impl AcpTransport {
    fn from_io<R, W>(
        reader: R,
        writer: W,
        child: Option<Child>,
        events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> Arc<Self>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let state = Arc::new(Mutex::new(AcpState::default()));
        let t = Arc::new(Self {
            writer: Mutex::new(Box::new(writer)),
            child: Mutex::new(child),
            next_id: AtomicI64::new(1),
            state: Arc::clone(&state),
        });
        tokio::spawn(acp_read_loop(reader, state, events));
        t
    }

    async fn write_frame(&self, frame: &Value) -> Result<(), StructuredChatError> {
        let mut line = serde_json::to_string(frame)
            .map_err(|e| StructuredChatError::Provider(format!("acp serialize: {e}")))?;
        line.push('\n');
        let mut w = self.writer.lock().await;
        w.write_all(line.as_bytes())
            .await
            .map_err(|e| StructuredChatError::Provider(format!("acp write: {e}")))?;
        w.flush()
            .await
            .map_err(|e| StructuredChatError::Provider(format!("acp flush: {e}")))?;
        Ok(())
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, StructuredChatError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.state.lock().await.rpc_responses.insert(id, tx);
        let frame = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        if let Err(err) = self.write_frame(&frame).await {
            self.state.lock().await.rpc_responses.remove(&id);
            return Err(err);
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(Ok(val))) => Ok(val),
            Ok(Ok(Err(msg))) => Err(StructuredChatError::Provider(msg)),
            Ok(Err(_)) => Err(StructuredChatError::Provider(
                "acp process closed before answering request".into(),
            )),
            Err(_) => {
                self.state.lock().await.rpc_responses.remove(&id);
                Err(StructuredChatError::Provider(format!(
                    "acp {method} timed out after {}s",
                    REQUEST_TIMEOUT.as_secs()
                )))
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), StructuredChatError> {
        self.write_frame(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .await
    }

    async fn shutdown(&self) {
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill().await;
        }
    }
}

async fn acp_read_loop<R: AsyncRead + Unpin + Send + 'static>(
    reader: R,
    state: Arc<Mutex<AcpState>>,
    events: mpsc::UnboundedSender<ProviderEvent>,
) {
    let mut buf = BufReader::new(reader);
    let mut raw = Vec::new();
    let end_reason = loop {
        raw.clear();
        match buf.read_until(b'\n', &mut raw).await {
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
                handle_acp_frame(value, &state, &events).await;
            }
            Err(err) => break format!("read error: {err}"),
        }
    };

    let mut st = state.lock().await;
    for (_, tx) in st.rpc_responses.drain() {
        let _ = tx.send(Err(format!("acp ended ({end_reason})")));
    }
    st.pending_permissions.clear();
    let active_turn = st.active_turn_id.take();
    drop(st);
    if let Some(turn_id) = active_turn {
        let _ = events.send(ProviderEvent::TurnCompleted {
            turn_id,
            error: Some(format!("acp process exited mid-turn ({end_reason})")),
        });
    }
    let _ = events.send(ProviderEvent::Ended { reason: end_reason });
}

async fn handle_acp_frame(
    value: Value,
    state: &Arc<Mutex<AcpState>>,
    events: &mpsc::UnboundedSender<ProviderEvent>,
) {
    let method = value.get("method").and_then(Value::as_str);
    let id = value.get("id").cloned().filter(|v| !v.is_null());

    match (method, id) {
        (None, Some(id_val)) => {
            let Some(num_id) = id_val.as_i64() else {
                return;
            };
            let res = if let Some(err) = value.get("error") {
                Err(err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("acp rpc error")
                    .to_owned())
            } else {
                Ok(value.get("result").cloned().unwrap_or(Value::Null))
            };
            if let Some(tx) = state.lock().await.rpc_responses.remove(&num_id) {
                let _ = tx.send(res);
            }
        }
        (Some("session/request_permission"), Some(rpc_id)) => {
            let params = value.get("params").cloned().unwrap_or(Value::Null);
            let request_id = format!("acp-perm-{}", rpc_id);
            let options = params
                .get("options")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut allow_once = "allow_once".to_owned();
            let mut allow_always = "allow_always".to_owned();
            let mut reject_once = "reject_once".to_owned();
            for opt in &options {
                let Some(opt_id) = opt.get("optionId").and_then(Value::as_str) else {
                    continue;
                };
                match opt.get("kind").and_then(Value::as_str) {
                    Some("allow_once") => allow_once = opt_id.to_owned(),
                    Some("allow_always") => allow_always = opt_id.to_owned(),
                    Some("reject_once") | Some("reject_always") => reject_once = opt_id.to_owned(),
                    _ => {}
                }
            }
            let tool_call = params.get("toolCall");
            let summary = tool_call
                .and_then(|tc| tc.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("Permission required")
                .to_owned();
            let kind = tool_call
                .and_then(|tc| tc.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("command")
                .to_owned();
            let turn_id = {
                let mut st = state.lock().await;
                st.pending_permissions.insert(
                    request_id.clone(),
                    PendingPermission {
                        rpc_id,
                        allow_once,
                        allow_always,
                        reject_once,
                    },
                );
                st.active_turn_id.clone()
            };
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::ApprovalRequest {
                    request_id,
                    kind,
                    summary,
                    reason: None,
                    answered: false,
                },
            }));
        }
        (Some("session/update"), None) => {
            let params = value.get("params").unwrap_or(&Value::Null);
            let update = params.get("update").unwrap_or(params);
            handle_session_update(update, state, events).await;
        }
        _ => {}
    }
}

async fn handle_session_update(
    update: &Value,
    state: &Arc<Mutex<AcpState>>,
    events: &mpsc::UnboundedSender<ProviderEvent>,
) {
    let Some(kind) = update.get("sessionUpdate").and_then(Value::as_str) else {
        return;
    };
    let turn_id = state.lock().await.active_turn_id.clone();
    match kind {
        "agent_message_chunk" => {
            if let Some(text) = update
                .get("content")
                .and_then(|c| c.get("text"))
                .and_then(Value::as_str)
            {
                if !text.is_empty() {
                    let _ = events.send(ProviderEvent::Part(PartDraft {
                        turn_id,
                        payload: PartKind::AssistantMessage {
                            text: text.to_owned(),
                        },
                    }));
                }
            }
        }
        "agent_thought_chunk" => {
            if let Some(text) = update
                .get("content")
                .and_then(|c| c.get("text"))
                .and_then(Value::as_str)
            {
                if !text.is_empty() {
                    let _ = events.send(ProviderEvent::Part(PartDraft {
                        turn_id,
                        payload: PartKind::Reasoning {
                            text: text.to_owned(),
                        },
                    }));
                }
            }
        }
        "tool_call" | "tool_call_update" => {
            let id = update
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_owned();
            let name = update
                .get("title")
                .or_else(|| update.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_owned();
            let status = match update.get("status").and_then(Value::as_str) {
                Some("completed") => ToolCallStatus::Completed,
                Some("failed") => ToolCallStatus::Failed,
                Some("cancelled") => ToolCallStatus::Cancelled,
                _ => ToolCallStatus::InProgress,
            };
            let mut output_parts = Vec::new();
            let mut diffs = Vec::new();
            if let Some(contents) = update.get("content").and_then(Value::as_array) {
                for item in contents {
                    match item.get("type").and_then(Value::as_str) {
                        Some("diff") => {
                            let path = item
                                .get("path")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_owned();
                            let new_text = item
                                .get("newText")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_owned();
                            diffs.push(FileChangeEntry {
                                path,
                                kind: "edit".into(),
                                diff: new_text,
                            });
                        }
                        _ => {
                            if let Some(t) = item
                                .get("content")
                                .and_then(|c| c.get("text"))
                                .and_then(Value::as_str)
                                .or_else(|| item.get("text").and_then(Value::as_str))
                            {
                                output_parts.push(t.to_owned());
                            }
                        }
                    }
                }
            }
            let output = if output_parts.is_empty() {
                None
            } else {
                Some(output_parts.join("\n"))
            };
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id: turn_id.clone(),
                payload: PartKind::ToolCall {
                    id: id.clone(),
                    name,
                    status: status.clone(),
                    output,
                    exit_code: None,
                    duration_ms: None,
                },
            }));
            if !diffs.is_empty() {
                let _ = events.send(ProviderEvent::Part(PartDraft {
                    turn_id,
                    payload: PartKind::FileChange {
                        id: format!("{id}-diff"),
                        status,
                        changes: diffs,
                    },
                }));
            }
        }
        "plan" => {
            let text = update
                .get("entries")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|e| {
                            let content = e.get("content").and_then(Value::as_str)?;
                            let status = e.get("status").and_then(Value::as_str).unwrap_or("pending");
                            Some(format!("[{status}] {content}"))
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            if !text.is_empty() {
                let _ = events.send(ProviderEvent::Part(PartDraft {
                    turn_id,
                    payload: PartKind::Plan { text },
                }));
            }
        }
        "available_commands_update" => {
            if let Some(arr) = update.get("availableCommands").and_then(Value::as_array) {
                let parsed: Vec<SlashCommandItem> = arr
                    .iter()
                    .filter_map(|item| {
                        let name = item.get("name").and_then(Value::as_str)?.trim();
                        if name.is_empty() {
                            return None;
                        }
                        Some(SlashCommandItem {
                            name: name.trim_start_matches('/').to_owned(),
                            description: item
                                .get("description")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                            source: "acp".into(),
                            syntax: "slash".into(),
                        })
                    })
                    .collect();
                state.lock().await.available_commands = parsed;
            }
        }
        "current_mode_update" => {
            if let Some(mode) = update.get("currentModeId").and_then(Value::as_str) {
                state.lock().await.current_mode = Some(mode.to_owned());
            }
        }
        _ => {}
    }
}

pub struct AcpProvider {
    provider_id: &'static str,
    cwd: PathBuf,
    events: mpsc::UnboundedSender<ProviderEvent>,
    initial_settings: SessionSettings,
    transport: Mutex<Option<Arc<AcpTransport>>>,
}

impl AcpProvider {
    pub fn spawn(
        provider_id: &str,
        cwd: PathBuf,
        events: mpsc::UnboundedSender<ProviderEvent>,
        settings: &SessionSettings,
    ) -> Result<Self, StructuredChatError> {
        let static_id: &'static str = if provider_id == "cline" { "cline" } else { "opencode" };
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
        let transport = AcpTransport::from_io(reader, writer, None, events.clone());
        Self {
            provider_id,
            cwd,
            events,
            initial_settings: settings,
            transport: Mutex::new(Some(transport)),
        }
    }

    async fn ensure_transport(&self) -> Result<Arc<AcpTransport>, StructuredChatError> {
        let mut guard = self.transport.lock().await;
        if let Some(existing) = guard.as_ref() {
            return Ok(Arc::clone(existing));
        }
        let (program, args) = build_acp_argv(self.provider_id, &self.cwd, &self.initial_settings);
        let mut cmd = tokio::process::Command::new(&program);
        cmd.args(&args)
            .current_dir(&self.cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut child = cmd.spawn().map_err(|err| {
            StructuredChatError::Provider(format!("failed to spawn {program} ACP: {err}"))
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            StructuredChatError::Provider("failed to capture acp stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            StructuredChatError::Provider("failed to capture acp stdout".into())
        })?;
        let transport = AcpTransport::from_io(stdout, stdin, Some(child), self.events.clone());
        *guard = Some(Arc::clone(&transport));
        Ok(transport)
    }

    async fn require_transport(&self) -> Result<Arc<AcpTransport>, StructuredChatError> {
        self.transport
            .lock()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                StructuredChatError::Conflict("SESSION_ENDED: acp transport is not started".into())
            })
    }
}

#[async_trait]
impl StructuredProvider for AcpProvider {
    fn provider_id(&self) -> &'static str {
        self.provider_id
    }

    fn capabilities(&self) -> Capabilities {
        acp_capabilities(self.provider_id)
    }

    async fn start(&self, resume_thread: Option<String>) -> Result<String, StructuredChatError> {
        let transport = self.ensure_transport().await?;
        let init_res = transport
            .request(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": {
                        "fs": { "readTextFile": false, "writeTextFile": false },
                        "terminal": false,
                    }
                }),
            )
            .await?;
        let can_load = init_res
            .get("agentCapabilities")
            .and_then(|c| c.get("loadSession"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        transport.state.lock().await.supports_load_session = can_load;

        let cwd_str = self.cwd.to_string_lossy().into_owned();
        let session_id = if let (Some(resume_id), true) = (resume_thread.as_deref(), can_load) {
            let _ = transport
                .request(
                    "session/load",
                    json!({
                        "sessionId": resume_id,
                        "cwd": cwd_str,
                        "mcpServers": [],
                    }),
                )
                .await?;
            resume_id.to_owned()
        } else {
            let created = transport
                .request(
                    "session/new",
                    json!({
                        "cwd": cwd_str,
                        "mcpServers": [],
                    }),
                )
                .await?;
            created
                .get("sessionId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{}-{}", self.provider_id, uuid::Uuid::new_v4()))
        };
        transport.state.lock().await.session_id = Some(session_id.clone());
        Ok(session_id)
    }

    async fn turn(
        &self,
        text: String,
        attachments: Vec<String>,
        settings: &SessionSettings,
    ) -> Result<TurnRef, StructuredChatError> {
        let transport = self.require_transport().await?;
        let (session_id, cur_mode, cur_model) = {
            let st = transport.state.lock().await;
            let sid = st
                .session_id
                .clone()
                .ok_or_else(|| StructuredChatError::Conflict("ACP session not started".into()))?;
            (sid, st.current_mode.clone(), st.current_model.clone())
        };

        if let Some(mode) = settings.execution_mode.as_deref().filter(|m| !m.is_empty()) {
            if cur_mode.as_deref() != Some(mode) {
                let _ = transport
                    .request(
                        "session/set_mode",
                        json!({ "sessionId": session_id, "modeId": mode }),
                    )
                    .await;
                transport.state.lock().await.current_mode = Some(mode.to_owned());
            }
        }
        if let Some(model) = settings.model.as_deref().filter(|m| !m.is_empty()) {
            if cur_model.as_deref() != Some(model) {
                let _ = transport
                    .request(
                        "session/set_model",
                        json!({ "sessionId": session_id, "modelId": model }),
                    )
                    .await;
                transport.state.lock().await.current_model = Some(model.to_owned());
            }
        }

        let turn_id = format!("{}-turn-{}", self.provider_id, uuid::Uuid::new_v4());
        transport.state.lock().await.active_turn_id = Some(turn_id.clone());
        let _ = self.events.send(ProviderEvent::TurnStarted {
            turn_id: turn_id.clone(),
        });

        let mut prompt_blocks = vec![json!({ "type": "text", "text": text })];
        for path in attachments {
            prompt_blocks.push(json!({
                "type": "resource_link",
                "uri": format!("file://{path}"),
                "name": path,
            }));
        }

        let t_bg = Arc::clone(&transport);
        let events_bg = self.events.clone();
        let tid_bg = turn_id.clone();
        tokio::spawn(async move {
            let res = t_bg
                .request(
                    "session/prompt",
                    json!({
                        "sessionId": session_id,
                        "prompt": prompt_blocks,
                    }),
                )
                .await;
            let mut st = t_bg.state.lock().await;
            st.pending_permissions.clear();
            let active = st.active_turn_id.take();
            drop(st);
            if let Some(tid) = active {
                let error = res.err().map(|e| e.to_string());
                let _ = events_bg.send(ProviderEvent::TurnCompleted {
                    turn_id: tid,
                    error,
                });
            } else {
                let _ = tid_bg;
            }
        });

        Ok(TurnRef { turn_id })
    }

    async fn steer(&self, _expected_turn: &str, _text: String) -> Result<(), StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "STEER_UNSUPPORTED: ACP does not support mid-turn steer".into(),
        ))
    }

    async fn interrupt(&self, _turn_id: &str) -> Result<(), StructuredChatError> {
        let transport = self.require_transport().await?;
        let Some(session_id) = transport.state.lock().await.session_id.clone() else {
            return Ok(());
        };
        transport
            .notify("session/cancel", json!({ "sessionId": session_id }))
            .await
    }

    async fn rollback(&self, _before_turn_id: &str) -> Result<String, StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "UNSUPPORTED_METHOD: ACP does not support turn rollback".into(),
        ))
    }

    async fn answer_approval(
        &self,
        request_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), StructuredChatError> {
        let transport = self.require_transport().await?;
        let perm = transport
            .state
            .lock()
            .await
            .pending_permissions
            .remove(request_id)
            .ok_or(StructuredChatError::NotFound)?;
        let option_id = match decision {
            ApprovalDecision::Accept => perm.allow_once,
            ApprovalDecision::AcceptForSession => perm.allow_always,
            ApprovalDecision::Decline => perm.reject_once,
        };
        transport
            .write_frame(&json!({
                "jsonrpc": "2.0",
                "id": perm.rpc_id,
                "result": {
                    "outcome": {
                        "outcome": "selected",
                        "optionId": option_id,
                    }
                }
            }))
            .await
    }

    async fn clear(&self) -> Result<String, StructuredChatError> {
        let transport = self.require_transport().await?;
        let created = transport
            .request(
                "session/new",
                json!({
                    "cwd": self.cwd.to_string_lossy(),
                    "mcpServers": [],
                }),
            )
            .await?;
        let new_id = created
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{}-{}", self.provider_id, uuid::Uuid::new_v4()));
        transport.state.lock().await.session_id = Some(new_id.clone());
        Ok(new_id)
    }

    async fn compact(&self) -> Result<(), StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "COMPACT_UNSUPPORTED: ACP has no compact verb".into(),
        ))
    }

    async fn commands(&self) -> Result<Vec<SlashCommandItem>, StructuredChatError> {
        let transport = self.require_transport().await?;
        let cmds = transport.state.lock().await.available_commands.clone();
        if !cmds.is_empty() {
            return Ok(cmds);
        }
        Ok(self
            .capabilities()
            .slash_commands
            .into_iter()
            .map(|name| SlashCommandItem {
                name: name.trim_start_matches('/').to_owned(),
                description: None,
                source: "builtin".into(),
                syntax: "slash".into(),
            })
            .collect())
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
    let fallback_cmds = acp_capabilities(provider_id).slash_commands;
    if provider_id != "opencode" {
        return Ok((Vec::new(), fallback_cmds));
    }
    let mut cmd = tokio::process::Command::new("opencode");
    cmd.args(["models"])
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let out = match tokio::time::timeout(Duration::from_secs(8), cmd.output()).await {
        Ok(Ok(o)) if o.status.success() => o,
        _ => return Ok((Vec::new(), fallback_cmds)),
    };
    let models = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && l.contains('/'))
        .map(|id| ModelInfo {
            id: id.to_owned(),
            label: id.to_owned(),
            efforts: Vec::new(),
        })
        .collect();
    Ok((models, fallback_cmds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    struct AcpServer {
        server_reader: BufReader<tokio::io::DuplexStream>,
        server_writer: tokio::io::DuplexStream,
    }

    impl AcpServer {
        async fn recv(&mut self) -> Value {
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(5), self.server_reader.read_line(&mut line))
                .await
                .expect("acp server recv timed out")
                .unwrap();
            serde_json::from_str(line.trim()).unwrap()
        }

        async fn send(&mut self, val: &Value) {
            let mut s = serde_json::to_string(val).unwrap();
            s.push('\n');
            self.server_writer.write_all(s.as_bytes()).await.unwrap();
            self.server_writer.flush().await.unwrap();
        }
    }

    struct AcpHarness {
        provider: AcpProvider,
        events_rx: mpsc::UnboundedReceiver<ProviderEvent>,
        srv: AcpServer,
    }

    impl AcpHarness {
        fn new(provider_id: &'static str) -> Self {
            let (cr, sw) = tokio::io::duplex(65536);
            let (sr, cw) = tokio::io::duplex(65536);
            let (tx, rx) = mpsc::unbounded_channel();
            let provider = AcpProvider::from_test_io(
                provider_id,
                PathBuf::from("/tmp"),
                tx,
                SessionSettings::default(),
                cr,
                cw,
            );
            Self {
                provider,
                events_rx: rx,
                srv: AcpServer {
                    server_reader: BufReader::new(sr),
                    server_writer: sw,
                },
            }
        }
    }

    #[test]
    fn argv_matches_opencode_and_cline_measured_flags() {
        let (p1, a1) = build_acp_argv("opencode", Path::new("/ws"), &SessionSettings::default());
        assert_eq!(p1, "opencode");
        assert_eq!(a1, vec!["acp", "--cwd", "/ws"]);

        let settings = SessionSettings {
            model: Some("claude-sonnet".into()),
            effort: Some("high".into()),
            permission_mode: Some(PermissionMode::FullAccess),
            execution_mode: Some("plan".into()),
            compaction_mode: Some("agentic".into()),
            ..SessionSettings::default()
        };
        let (p2, a2) = build_acp_argv("cline", Path::new("/ws"), &settings);
        assert_eq!(p2, "cline");
        assert!(a2.iter().any(|x| x == "--acp"));
        assert!(a2.windows(2).any(|w| w == ["--cwd", "/ws"]));
        assert!(a2.iter().any(|x| x == "--plan"));
        assert!(a2.iter().any(|x| x == "--yolo"));
        assert!(a2.windows(2).any(|w| w == ["--thinking", "high"]));
        assert!(a2.windows(2).any(|w| w == ["--compaction", "agentic"]));
        assert!(a2.windows(2).any(|w| w == ["-m", "claude-sonnet"]));
    }

    #[test]
    fn capabilities_differentiate_cline_and_opencode() {
        let cline = acp_capabilities("cline");
        assert_eq!(cline.execution_modes, vec!["act", "plan"]);
        assert_eq!(cline.thinking_levels.len(), 5);
        let oc = acp_capabilities("opencode");
        assert_eq!(oc.execution_modes, vec!["build", "plan"]);
    }

    #[tokio::test]
    async fn start_runs_initialize_then_session_new() {
        let mut h = AcpHarness::new("opencode");
        let (res, ()) = tokio::join!(h.provider.start(None), async {
            let r1 = h.srv.recv().await;
            assert_eq!(r1["method"], "initialize");
            h.srv.send(&json!({
                "jsonrpc": "2.0",
                "id": r1["id"],
                "result": { "protocolVersion": 1, "agentCapabilities": { "loadSession": true } }
            })).await;
            let r2 = h.srv.recv().await;
            assert_eq!(r2["method"], "session/new");
            h.srv.send(&json!({
                "jsonrpc": "2.0",
                "id": r2["id"],
                "result": { "sessionId": "acp-s-1" }
            })).await;
        });
        assert_eq!(res.unwrap(), "acp-s-1");
    }

    #[tokio::test]
    async fn start_uses_session_load_when_resuming_and_supported() {
        let mut h = AcpHarness::new("opencode");
        let (res, ()) = tokio::join!(h.provider.start(Some("existing-s".into())), async {
            let r1 = h.srv.recv().await;
            h.srv.send(&json!({
                "jsonrpc": "2.0",
                "id": r1["id"],
                "result": { "protocolVersion": 1, "agentCapabilities": { "loadSession": true } }
            })).await;
            let r2 = h.srv.recv().await;
            assert_eq!(r2["method"], "session/load");
            assert_eq!(r2["params"]["sessionId"], "existing-s");
            h.srv.send(&json!({ "jsonrpc": "2.0", "id": r2["id"], "result": {} })).await;
        });
        assert_eq!(res.unwrap(), "existing-s");
    }

    #[tokio::test]
    async fn turn_applies_mode_model_and_streams_all_session_updates() {
        let mut h = AcpHarness::new("opencode");
        let (sres, ()) = tokio::join!(h.provider.start(None), async {
            let r1 = h.srv.recv().await;
            h.srv.send(&json!({ "jsonrpc": "2.0", "id": r1["id"], "result": { "protocolVersion": 1 } })).await;
            let r2 = h.srv.recv().await;
            h.srv.send(&json!({ "jsonrpc": "2.0", "id": r2["id"], "result": { "sessionId": "s1" } })).await;
        });
        sres.unwrap();

        let settings = SessionSettings {
            execution_mode: Some("plan".into()),
            model: Some("openai/gpt-5".into()),
            ..SessionSettings::default()
        };
        let (tref_res, ()) = tokio::join!(
            h.provider.turn("fix bug".into(), vec!["src/a.rs".into()], &settings),
            async {
                let rm = h.srv.recv().await;
                assert_eq!(rm["method"], "session/set_mode");
                assert_eq!(rm["params"]["modeId"], "plan");
                h.srv.send(&json!({ "jsonrpc": "2.0", "id": rm["id"], "result": {} })).await;

                let rmod = h.srv.recv().await;
                assert_eq!(rmod["method"], "session/set_model");
                assert_eq!(rmod["params"]["modelId"], "openai/gpt-5");
                h.srv.send(&json!({ "jsonrpc": "2.0", "id": rmod["id"], "result": {} })).await;
            }
        );
        let tref = tref_res.unwrap();
        let rp = h.srv.recv().await;
        assert_eq!(rp["method"], "session/prompt");
        assert_eq!(rp["params"]["prompt"][1]["type"], "resource_link");

        for upd in [
            json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "thinking" } }),
            json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "answer" } }),
            json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "tc1",
                "title": "edit",
                "status": "completed",
                "content": [{ "type": "diff", "path": "src/a.rs", "newText": "+fixed" }]
            }),
            json!({
                "sessionUpdate": "plan",
                "entries": [{ "content": "Run tests", "status": "in_progress" }]
            }),
            json!({
                "sessionUpdate": "available_commands_update",
                "availableCommands": [{ "name": "review", "description": "Review diff" }]
            }),
            json!({ "sessionUpdate": "current_mode_update", "currentModeId": "build" }),
        ] {
            h.srv.send(&json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "update": upd } })).await;
        }
        h.srv.send(&json!({ "jsonrpc": "2.0", "id": rp["id"], "result": { "stopReason": "end_turn" } })).await;

        let mut seen = Vec::new();
        while let Some(ev) = h.events_rx.recv().await {
            match ev {
                ProviderEvent::TurnStarted { turn_id } => {
                    assert_eq!(turn_id, tref.turn_id);
                    seen.push("start");
                }
                ProviderEvent::Part(p) => match p.payload {
                    PartKind::Reasoning { .. } => seen.push("thought"),
                    PartKind::AssistantMessage { .. } => seen.push("msg"),
                    PartKind::ToolCall { .. } => seen.push("tool"),
                    PartKind::FileChange { .. } => seen.push("diff"),
                    PartKind::Plan { .. } => seen.push("plan"),
                    _ => {}
                },
                ProviderEvent::TurnCompleted { error, .. } => {
                    assert!(error.is_none());
                    seen.push("end");
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(seen, vec!["start", "thought", "msg", "tool", "diff", "plan", "end"]);
        let cmds = h.provider.commands().await.unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name, "review");
    }

    #[tokio::test]
    async fn permission_request_maps_accept_session_and_decline() {
        let mut h = AcpHarness::new("cline");
        h.srv.send(&json!({
            "jsonrpc": "2.0",
            "id": 77,
            "method": "session/request_permission",
            "params": {
                "toolCall": { "title": "run rm", "kind": "command" },
                "options": [
                    { "optionId": "opt-once", "kind": "allow_once" },
                    { "optionId": "opt-always", "kind": "allow_always" },
                    { "optionId": "opt-deny", "kind": "reject_once" }
                ]
            }
        })).await;
        let ev = h.events_rx.recv().await.unwrap();
        let req_id = match ev {
            ProviderEvent::Part(PartDraft { payload: PartKind::ApprovalRequest { request_id, .. }, .. }) => request_id,
            other => panic!("unexpected: {other:?}"),
        };
        h.provider.answer_approval(&req_id, ApprovalDecision::AcceptForSession).await.unwrap();
        let reply = h.srv.recv().await;
        assert_eq!(reply["id"], 77);
        assert_eq!(reply["result"]["outcome"]["optionId"], "opt-always");
    }

    #[tokio::test]
    async fn interrupt_sends_session_cancel_notification() {
        let mut h = AcpHarness::new("opencode");
        let (sres, ()) = tokio::join!(h.provider.start(None), async {
            let r1 = h.srv.recv().await;
            h.srv.send(&json!({ "jsonrpc": "2.0", "id": r1["id"], "result": { "protocolVersion": 1 } })).await;
            let r2 = h.srv.recv().await;
            h.srv.send(&json!({ "jsonrpc": "2.0", "id": r2["id"], "result": { "sessionId": "s-cancel" } })).await;
        });
        sres.unwrap();

        h.provider.interrupt("t1").await.unwrap();
        let n = h.srv.recv().await;
        assert_eq!(n["method"], "session/cancel");
        assert_eq!(n["params"]["sessionId"], "s-cancel");
        assert!(n.get("id").is_none());
    }

    #[tokio::test]
    async fn clear_starts_new_acp_session() {
        let mut h = AcpHarness::new("opencode");
        let (res, ()) = tokio::join!(h.provider.clear(), async {
            let req = h.srv.recv().await;
            assert_eq!(req["method"], "session/new");
            h.srv.send(&json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "sessionId": "fresh-acp" } })).await;
        });
        assert_eq!(res.unwrap(), "fresh-acp");
    }

    #[tokio::test]
    async fn invalid_utf8_and_malformed_json_survive() {
        let mut h = AcpHarness::new("opencode");
        h.srv.server_writer.write_all(b"\xff\xfe\nbad json\n").await.unwrap();
        h.srv.server_writer.flush().await.unwrap();
        let (res, ()) = tokio::join!(h.provider.clear(), async {
            let req = h.srv.recv().await;
            h.srv.send(&json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "sessionId": "ok" } })).await;
        });
        assert_eq!(res.unwrap(), "ok");
    }

    #[tokio::test]
    async fn child_eof_mid_turn_fails_turn_before_ended() {
        let mut h = AcpHarness::new("opencode");
        let (sres, ()) = tokio::join!(h.provider.start(None), async {
            let r1 = h.srv.recv().await;
            h.srv.send(&json!({ "jsonrpc": "2.0", "id": r1["id"], "result": { "protocolVersion": 1 } })).await;
            let r2 = h.srv.recv().await;
            h.srv.send(&json!({ "jsonrpc": "2.0", "id": r2["id"], "result": { "sessionId": "s-eof" } })).await;
        });
        sres.unwrap();

        let tref = h.provider.turn("hi".into(), vec![], &SessionSettings::default()).await.unwrap();
        let _ = h.srv.recv().await;
        let _ = h.events_rx.recv().await.unwrap();
        drop(h.srv.server_writer);

        let mut got_err = false;
        let mut got_ended = false;
        while let Some(ev) = h.events_rx.recv().await {
            match ev {
                ProviderEvent::TurnCompleted { turn_id, error: Some(_) } if turn_id == tref.turn_id => {
                    got_err = true;
                }
                ProviderEvent::Ended { .. } => {
                    got_ended = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(got_err && got_ended);
    }

    #[tokio::test]
    async fn unsupported_verbs_return_typed_errors() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let p = AcpProvider::spawn("opencode", PathBuf::from("/tmp"), tx, &SessionSettings::default()).unwrap();
        assert_eq!(p.steer("t", "x".into()).await.unwrap_err().code(), "STEER_UNSUPPORTED");
        assert_eq!(p.compact().await.unwrap_err().code(), "COMPACT_UNSUPPORTED");
        assert_eq!(p.rollback("t").await.unwrap_err().code(), "UNSUPPORTED_METHOD");
    }

    #[tokio::test]
    async fn permission_decline_sends_reject_option_id() {
        let mut h = AcpHarness::new("opencode");
        h.srv.send(&json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "session/request_permission",
            "params": {
                "toolCall": { "title": "delete", "kind": "command" },
                "options": [
                    { "optionId": "deny-custom", "kind": "reject_once" }
                ]
            }
        })).await;
        let ev = h.events_rx.recv().await.unwrap();
        let req_id = match ev {
            ProviderEvent::Part(PartDraft { payload: PartKind::ApprovalRequest { request_id, .. }, .. }) => request_id,
            other => panic!("unexpected: {other:?}"),
        };
        h.provider.answer_approval(&req_id, ApprovalDecision::Decline).await.unwrap();
        let reply = h.srv.recv().await;
        assert_eq!(reply["result"]["outcome"]["optionId"], "deny-custom");
    }
}
