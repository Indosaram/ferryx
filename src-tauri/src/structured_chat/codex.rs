//! The Codex structured-chat provider: `codex app-server` JSON-RPC over stdio.
//!
//! Wire shapes were read from `openai/codex` `codex-rs/app-server-protocol` (v2) and from the
//! t3code recordings under `.evidence/chat-t3-parity-20261125/` (`apps/server/src/orchestration-v2/
//! testkit/fixtures/*/codex_transcript.ndjson`), not invented:
//!
//! * Handshake: `initialize` (id 1) → result, then the `initialized` notification. The framing is
//!   codex's own (no `"jsonrpc"` field), so frames here never send one.
//! * `thread/start {cwd, model?}`, `thread/resume {threadId, cwd, excludeTurns: true, ...}`,
//!   `turn/start {threadId, input, cwd, model?, effort?, summary:"auto", approvalPolicy,
//!   sandboxPolicy, approvalsReviewer}`, `turn/steer {threadId, input, expectedTurnId}`,
//!   `turn/interrupt {threadId, turnId}`, `thread/revert {threadId, beforeTurnId}`,
//!   `thread/compact/start {threadId}`.
//! * Approvals arrive as server→client requests `item/commandExecution/requestApproval` and
//!   `item/fileChange/requestApproval`; the answer is the JSON-RPC response
//!   `{"id": <server id>, "result": {"decision": "accept"|"acceptForSession"|"decline"}}`.
//! * Parts come from `item/started`, `item/completed`, the per-item delta notifications, and
//!   `thread/tokenUsage/updated`.
//!
//! The permission-mode mapping is t3code's `codexRuntimeModeTurnDefaults` byte for byte.
//!
//! The provider never builds a shell string: the app-server is spawned from an argv array, and
//! the cwd is validated by [`crate::structured_chat::store`] before this module ever sees it.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};

use super::types::{
    ApprovalDecision, PartKind, PermissionMode, SessionSettings, StructuredChatError,
};
use super::{PartDraft, ProviderEvent, StructuredProvider, TurnRef};

/// Program name. The probe never uses a client-supplied path.
pub const CODEX_PROGRAM: &str = "codex";

/// How long one request may wait for its response before failing the turn.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// One inbound app-server frame, already classified.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InboundFrame {
    Notification { method: String, params: Value },
    ServerRequest { id: Value, method: String, params: Value },
    Response { id: Value, result: Result<Value, WireError> },
    Malformed,
}

/// A JSON-RPC error body, with the numeric code kept: the code is the machine-readable part, and
/// a provider that answers "nothing to continue" says so with a code, not with prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WireError {
    pub code: Option<i64>,
    pub message: String,
}

/// JSON-RPC 2.0's standard codes, the only ones the app-server is pinned to use.
const JSONRPC_INVALID_REQUEST: i64 = -32600;
const JSONRPC_METHOD_NOT_FOUND: i64 = -32601;

/// A line that is not a JSON object, or a frame with neither method nor result, is malformed and
/// skipped: one bad line must not take the connection down.
pub(crate) fn classify_frame(line: &str) -> InboundFrame {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return InboundFrame::Malformed;
    };
    let Some(object) = value.as_object() else {
        return InboundFrame::Malformed;
    };
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let id = object.get("id").cloned().filter(|id| !id.is_null());
    match (method, id, object.get("result"), object.get("error")) {
        (Some(method), Some(id), _, _) => InboundFrame::ServerRequest { id, method, params: object.get("params").cloned().unwrap_or(Value::Null) },
        (Some(method), None, _, _) => InboundFrame::Notification { method, params: object.get("params").cloned().unwrap_or(Value::Null) },
        (None, Some(id), result, error) => {
            let result = match (result, error) {
                (Some(result), _) => Ok(result.clone()),
                (_, Some(error)) => Err(WireError {
                    code: error.get("code").and_then(Value::as_i64),
                    message: error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown app-server error")
                        .to_owned(),
                }),
                _ => return InboundFrame::Malformed,
            };
            InboundFrame::Response { id, result }
        }
        (None, None, _, _) => InboundFrame::Malformed,
    }
}

/// The id key the pending-approval map uses: integers dominate the wire, strings stay intact.
pub(crate) fn approval_key(id: &Value) -> String {
    match id {
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// The app-server transport: one stdio connection to one child process.
///
/// A connection is split into a reader and a writer half because the app-server answers only
/// after it has read the request: one mutex over the whole connection would deadlock on the
/// first call, with the reader holding it while blocked on a line the server will not send until
/// the write lands.
pub(crate) trait AppServerTransport: Send {
    fn split(self: Box<Self>) -> (Box<dyn AppServerReader>, Box<dyn AppServerWriter>);
    /// The child behind this connection, when it has one; a test duplex has none.
    fn child(&self) -> Option<ChildHandle> {
        None
    }
}

#[async_trait::async_trait]
pub(crate) trait AppServerWriter: Send {
    async fn send_line(&mut self, line: String) -> io::Result<()>;
}

#[async_trait::async_trait]
pub(crate) trait AppServerReader: Send {
    /// The next inbound line, or `None` at EOF (the child closed stdout or exited).
    async fn read_line(&mut self) -> io::Result<Option<String>>;
}

/// The real transport: `codex app-server` as an argv array, never a shell string.
pub(crate) struct ChildAppServer {
    /// Held for ownership: `kill_on_drop` reaps the child when the last handle goes.
    child: ChildHandle,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
}

impl ChildAppServer {
    /// Spawns the child with `cwd` as its working directory.
    pub fn spawn(argv: &[&str], cwd: &std::path::Path) -> io::Result<Self> {
        let mut command = tokio::process::Command::new(argv[0]);
        command
            .args(&argv[1..])
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            // Draining stderr would need another task; discarding it cannot block the child the
            // way a full pipe buffer would.
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        crate::util::configure_tokio_no_window(&mut command);
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "app-server stdin closed"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "app-server stdout closed"))?;
        Ok(Self {
            child: ChildHandle(Arc::new(std::sync::Mutex::new(child))),
            stdin,
            stdout: BufReader::new(stdout),
        })
    }
}

impl AppServerTransport for ChildAppServer {
    fn child(&self) -> Option<ChildHandle> {
        Some(self.child.clone())
    }

    fn split(self: Box<Self>) -> (Box<dyn AppServerReader>, Box<dyn AppServerWriter>) {
        let ChildAppServer {
            child,
            stdin,
            stdout,
        } = *self;
        (
            Box::new(ChildAppServerReader { stdout }),
            Box::new(ChildAppServerWriter {
                _child: child,
                stdin,
            }),
        )
    }
}

/// The child process handle, shared by both halves of a split connection and by the provider so
/// [`CodexProvider::close`] can reap the process instead of waiting for drop order.
#[derive(Clone)]
pub(crate) struct ChildHandle(Arc<std::sync::Mutex<tokio::process::Child>>);

impl ChildHandle {
    /// Kills the process if it is still running. Idempotent.
    pub fn kill(&self) {
        if let Ok(mut child) = self.0.lock() {
            let _ = child.start_kill();
        }
    }
}

struct ChildAppServerReader {
    stdout: BufReader<tokio::process::ChildStdout>,
}

struct ChildAppServerWriter {
    /// Keeps the process alive while the writer half exists.
    _child: ChildHandle,
    stdin: tokio::process::ChildStdin,
}

#[async_trait::async_trait]
impl AppServerReader for ChildAppServerReader {
    async fn read_line(&mut self) -> io::Result<Option<String>> {
        // Read raw bytes and decode lossily. `read_line` into a `String` fails the whole read with
        // `InvalidData` on one invalid UTF-8 byte, which the reader loop cannot tell from EOF: a
        // single bad byte would kill the session. The line survives, the byte becomes U+FFFD, and
        // the frame classifier drops the line if it is no longer JSON.
        let mut raw = Vec::new();
        let read = self.stdout.read_until(b'\n', &mut raw).await?;
        if read == 0 {
            Ok(None)
        } else {
            Ok(Some(String::from_utf8_lossy(&raw).into_owned()))
        }
    }
}

#[async_trait::async_trait]
impl AppServerWriter for ChildAppServerWriter {
    async fn send_line(&mut self, line: String) -> io::Result<()> {
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await
    }
}

/// The argv that launches the app-server for one session.
pub fn app_server_argv() -> Vec<&'static str> {
    vec![CODEX_PROGRAM, "app-server"]
}

/// The exact argv and stdin of the short-lived read-only model probe, reused from the
/// reference-chat discovery parsers so field names stay in one place.
fn model_probe(cwd: &std::path::Path) -> io::Result<(tokio::process::Child, String)> {
    use crate::remote::reference_chat::model_discovery::{
        codex_probe_stdin, CODEX_PROBE_ARGS, CODEX_PROBE_PROGRAM,
    };
    let mut command = tokio::process::Command::new(CODEX_PROBE_PROGRAM);
    command
        .args(CODEX_PROBE_ARGS)
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    crate::util::configure_tokio_no_window(&mut command);
    let child = command.spawn()?;
    Ok((child, codex_probe_stdin()))
}

/// Codex capability rows, before model discovery fills the catalog in.
pub fn codex_capabilities() -> super::types::Capabilities {
    super::types::Capabilities {
        steer: true,
        rollback: true,
        // `thread/queue/start` drains the queue a stop held, which is codex's native
        // continuation for a stopped run.
        resume: true,
        // `thread/revert` documents the opposite: "This only changes persisted conversation
        // history. It does not revert local file changes." Codex therefore has no file revert on
        // rollback, and the UI keeps its "only the conversation is rolled back" notice.
        revert_files: false,
        history_paging: true,
        permission_modes: PermissionMode::all().to_vec(),
        models: Vec::new(),
        slash_commands: Vec::new(),
        ..super::types::Capabilities::default()
    }
}

/// True once the accumulated probe stdout holds the settled `model/list` reply, so the reader can
/// stop without waiting for the child to exit.
fn probe_reply_complete(buffer: &[u8]) -> bool {
    let text = String::from_utf8_lossy(buffer);
    text.lines().any(|line| {
        let line = line.trim();
        if !line.starts_with('{') {
            return false;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        value.get("id").and_then(Value::as_u64) == Some(2)
            && (value.get("result").is_some() || value.get("error").is_some())
    })
}

/// Lists the models a logged-in codex reports, through the shared discovery parser, plus any
/// slash commands that record listed.
pub async fn discover_models_with_commands(
    cwd: &std::path::Path,
) -> Result<(Vec<super::types::ModelInfo>, Vec<String>), StructuredChatError> {
    use crate::remote::reference_chat::model_discovery::{
        parse_codex_model_list, DiscoveryError, DISCOVERY_PROBE_TIMEOUT, DISCOVERY_STDOUT_LIMIT,
    };
    let (mut child, stdin) =
        model_probe(cwd).map_err(|error| StructuredChatError::Provider(error.to_string()))?;
    let mut pipe = child.stdin.take();
    if let Some(pipe) = pipe.as_mut() {
        let _ = pipe.write_all(stdin.as_bytes()).await;
        let _ = pipe.flush().await;
    }
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| StructuredChatError::Provider("model probe stdout closed".into()))?;
    // The probe cannot wait for EOF: codex's app-server exits as soon as its stdin closes, and it
    // answers `model/list` only after processing the `initialized` notification, so closing stdin
    // right after the write races the answer away (measured on codex-cli 0.151.0). Read until the
    // `model/list` reply frame arrives, then close.
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let deadline = tokio::time::Instant::now() + DISCOVERY_PROBE_TIMEOUT;
    loop {
        use tokio::io::AsyncReadExt;
        match tokio::time::timeout_at(deadline, stdout.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(read)) => {
                buffer.extend_from_slice(&chunk[..read]);
                if buffer.len() > DISCOVERY_STDOUT_LIMIT || probe_reply_complete(&buffer) {
                    break;
                }
            }
            Ok(Err(error)) => {
                return Err(StructuredChatError::Provider(format!("model probe: {error}")));
            }
            Err(_) => return Err(StructuredChatError::Provider("model probe failed".into())),
        }
    }
    drop(pipe);
    let _ = child.kill().await;
    let stdout = String::from_utf8_lossy(&buffer).to_string();
    let discovered = parse_codex_model_list(&stdout).map_err(|error: DiscoveryError| {
        StructuredChatError::Provider(format!("model discovery: {error:?}"))
    })?;
    let models = discovered
        .models
        .into_iter()
        .map(|model| super::types::ModelInfo {
            id: model.id,
            label: model.label,
            efforts: model.efforts,
        })
        .collect();
    let slash_commands = discovered
        .slash_commands
        .into_iter()
        .map(|command| command.name)
        .collect();
    Ok((models, slash_commands))
}

/// Codex's exact permission-mode → approval/sandbox defaults, read from t3code
/// `apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts::codexRuntimeModeTurnDefaults`.
pub fn runtime_mode_turn_defaults(
    mode: PermissionMode,
) -> (Value, &'static str, Value) {
    match mode {
        PermissionMode::Supervised => (
            json!("untrusted"),
            "user",
            json!({"type": "readOnly"}),
        ),
        PermissionMode::AutoAcceptEdits => (
            json!("on-request"),
            "user",
            json!({"type": "workspaceWrite"}),
        ),
        PermissionMode::Auto => (
            json!("on-request"),
            "auto_review",
            json!({"type": "workspaceWrite"}),
        ),
        PermissionMode::FullAccess => (
            // `never` also rejects browser and app access prompts before the client sees them.
            json!({"granular": {
                "mcp_elicitations": true,
                "request_permissions": false,
                "rules": false,
                "sandbox_approval": false,
                "skill_approval": false,
            }}),
            "user",
            json!({"type": "dangerFullAccess"}),
        ),
    }
}

/// Shared connection state: outbound writes, pending responses, pending approval answers.
struct ConnectionIo {
    writer: Mutex<Box<dyn AppServerWriter>>,
    next_id: Mutex<i64>,
    pending: Mutex<HashMap<i64, oneshot::Sender<Result<Value, WireError>>>>,
    /// The raw server request id plus the route's answer channel, keyed by the canonical id.
    approvals: Mutex<HashMap<String, PendingApproval>>,
}

/// One approval request the client has not answered yet.
struct PendingApproval {
    /// The raw server request id, echoed back in the response frame.
    id: Value,
    /// The turn the request belongs to. The request dies with its turn: codex abandons its own
    /// pending requests when a turn ends, so answering one afterwards would write a response
    /// frame for an id the server has already resolved.
    turn_id: Option<String>,
    sender: oneshot::Sender<ApprovalDecision>,
}

/// Handshake params, mirroring the t3code recordings with Ferryx's own client identity.
fn initialize_params() -> Value {
    json!({
        "clientInfo": {"name": "ferryx", "title": "Ferryx", "version": env!("CARGO_PKG_VERSION")},
        "capabilities": {"experimentalApi": true},
    })
}

fn thread_start_params(cwd: &std::path::Path, model: Option<&str>, source: &str) -> Value {
    let mut params = json!({
        "cwd": cwd.to_string_lossy(),
        "sessionStartSource": source,
    });
    if let Some(model) = model {
        params["model"] = json!(model);
    }
    params
}

fn thread_resume_params(thread_id: &str, cwd: &std::path::Path, model: Option<&str>) -> Value {
    let mut params = json!({
        "threadId": thread_id,
        "cwd": cwd.to_string_lossy(),
        "excludeTurns": true,
    });
    if let Some(model) = model {
        params["model"] = json!(model);
    }
    params
}

fn turn_input(text: &str, attachments: &[String]) -> Value {
    // `openai/codex` `codex-rs/app-server-protocol/src/protocol/v2/turn.rs::UserInput`: local files
    // ride as `localImage {path}` when the path is an image and as `mention {name, path}`
    // otherwise. Text is one `text` row, first.
    let mut input: Vec<Value> = vec![json!({"type": "text", "text": text})];
    for path in attachments {
        if is_image_path(path) {
            input.push(json!({"type": "localImage", "path": path}));
        } else {
            input.push(json!({
                "type": "mention",
                "name": attachment_name(path),
                "path": path,
            }));
        }
    }
    Value::Array(input)
}

/// Whether a path names an image the model can read directly (codex `localImage`).
fn is_image_path(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
        .iter()
        .any(|extension| lowered.ends_with(extension))
}

/// The display name an attachment `mention` row carries.
fn attachment_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_owned())
}

fn turn_start_params(
    thread_id: &str,
    cwd: &std::path::Path,
    settings: &SessionSettings,
    text: &str,
    attachments: &[String],
) -> Value {
    let mode = settings.permission_mode.unwrap_or(PermissionMode::FullAccess);
    let (approval_policy, approvals_reviewer, sandbox_policy) = runtime_mode_turn_defaults(mode);
    let mut params = json!({
        "threadId": thread_id,
        "cwd": cwd.to_string_lossy(),
        "input": turn_input(text, attachments),
        "summary": "auto",
        "approvalPolicy": approval_policy,
        "approvalsReviewer": approvals_reviewer,
        "sandboxPolicy": sandbox_policy,
    });
    if let Some(model) = &settings.model {
        params["model"] = json!(model);
    }
    if let Some(effort) = &settings.effort {
        params["effort"] = json!(effort);
    }
    params
}

fn turn_steer_params(thread_id: &str, expected_turn_id: &str, text: &str) -> Value {
    json!({
        "threadId": thread_id,
        "input": turn_input(text, &[]),
        "expectedTurnId": expected_turn_id,
    })
}

/// Per-item delta buffers shared between the mapping pass and the completed pass.
pub(crate) type DeltaBuffers = HashMap<String, String>;

/// Maps one notification into zero or more provider events. Pure, so the wire mapping is unit
/// tested without a child process.
pub(crate) fn notification_events(
    method: &str,
    params: &Value,
    buffers: &mut DeltaBuffers,
) -> Vec<ProviderEvent> {
    let turn_id = params.get("turnId").and_then(Value::as_str).map(str::to_owned);
    let item = params.get("item");
    let item_id = || item.and_then(|item| item.get("id")).and_then(Value::as_str).map(str::to_owned);
    match method {
        "item/started" => {
            // The store stamps the user part itself when the turn starts (and on steer), so the
            // provider never emits `userMessage`: doing both would journal the same text twice.
            // Every other item is in flight, and t3code surfaces it: a command that is still
            // running (or that an interrupt kills before `item/completed`) must be visible in
            // history, not invisible until it finishes.
            let Some(item) = item else { return Vec::new() };
            let id = item_id().unwrap_or_default();
            let status = super::types::ToolCallStatus::InProgress;
            match item.get("type").and_then(Value::as_str) {
                Some("commandExecution") => vec![ProviderEvent::Part(PartDraft {
                    turn_id,
                    payload: PartKind::ToolCall {
                        id,
                        name: "shell".into(),
                        status,
                        output: item
                            .get("aggregatedOutput")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        exit_code: None,
                        duration_ms: None,
                    },
                })],
                Some("mcpToolCall") => {
                    let server = item.get("server").and_then(Value::as_str).unwrap_or("mcp");
                    let tool = item.get("tool").and_then(Value::as_str).unwrap_or("tool");
                    vec![ProviderEvent::Part(PartDraft {
                        turn_id,
                        payload: PartKind::ToolCall {
                            id,
                            name: format!("{server}/{tool}"),
                            status,
                            output: None,
                            exit_code: None,
                            duration_ms: None,
                        },
                    })]
                }
                Some("fileChange") => vec![ProviderEvent::Part(PartDraft {
                    turn_id,
                    payload: PartKind::FileChange {
                        id,
                        status,
                        changes: Vec::new(),
                    },
                })],
                _ => Vec::new(),
            }
        }
        "item/agentMessage/delta" => {
            append_delta(params, buffers);
            Vec::new()
        }
        "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
            append_delta(params, buffers);
            Vec::new()
        }
        "item/plan/delta" => {
            append_delta(params, buffers);
            Vec::new()
        }
        "item/commandExecution/outputDelta" => {
            append_delta(params, buffers);
            Vec::new()
        }
        "item/completed" => {
            let Some(item) = item else { return Vec::new() };
            let id = item_id();
            match item.get("type").and_then(Value::as_str) {
                Some("agentMessage") => {
                    let final_text = item
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| take_buffer(buffers, id.as_deref()));
                    vec![ProviderEvent::Part(PartDraft {
                        turn_id: turn_id.clone(),
                        payload: PartKind::AssistantMessage { text: final_text },
                    })]
                }
                Some("reasoning") => {
                    let summary = item
                        .get("summary")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default();
                    let text = if summary.is_empty() {
                        take_buffer(buffers, id.as_deref())
                    } else {
                        summary
                    };
                    vec![ProviderEvent::Part(PartDraft {
                        turn_id: turn_id.clone(),
                        payload: PartKind::Reasoning { text },
                    })]
                }
                Some("commandExecution") => {
                    let buffer = take_buffer(buffers, id.as_deref());
                    vec![ProviderEvent::Part(PartDraft {
                        turn_id: turn_id.clone(),
                        payload: PartKind::ToolCall {
                            id: id.unwrap_or_default(),
                            name: "shell".into(),
                            status: command_status(item),
                            output: item
                                .get("aggregatedOutput")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .or(Some(buffer)),
                            exit_code: item.get("exitCode").and_then(Value::as_i64).map(|v| v as i32),
                            duration_ms: item.get("durationMs").and_then(Value::as_i64),
                        },
                    })]
                }
                Some("mcpToolCall") => {
                    let server = item.get("server").and_then(Value::as_str).unwrap_or("mcp");
                    let tool = item.get("tool").and_then(Value::as_str).unwrap_or("tool");
                    vec![ProviderEvent::Part(PartDraft {
                        turn_id: turn_id.clone(),
                        payload: PartKind::ToolCall {
                            id: id.unwrap_or_default(),
                            name: format!("{server}/{tool}"),
                            status: named_status(
                                item.get("status").and_then(Value::as_str).unwrap_or("completed"),
                            ),
                            output: item
                                .pointer("/result/content/0/text")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .or_else(|| {
                                    item.get("error").map(|error| error.to_string())
                                }),
                            exit_code: None,
                            duration_ms: item.get("durationMs").and_then(Value::as_i64),
                        },
                    })]
                }
                Some("fileChange") => {
                    let changes = item
                        .get("changes")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .map(|row| super::types::FileChangeEntry {
                                    path: row
                                        .get("path")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_owned(),
                                    kind: patch_kind(row.get("kind")),
                                    diff: row
                                        .get("diff")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_owned(),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    vec![ProviderEvent::Part(PartDraft {
                        turn_id: turn_id.clone(),
                        payload: PartKind::FileChange {
                            id: id.unwrap_or_default(),
                            status: named_status(
                                item.get("status").and_then(Value::as_str).unwrap_or("completed"),
                            ),
                            changes,
                        },
                    })]
                }
                Some("plan") => {
                    let text = match item.get("text").and_then(Value::as_str) {
                        Some(text) => text.to_owned(),
                        // The protocol notes the completed plan is authoritative and may differ
                        // from the concatenated deltas; fall back to the deltas when it is absent.
                        None => take_buffer(buffers, id.as_deref()),
                    };
                    vec![ProviderEvent::Part(PartDraft {
                        turn_id: turn_id.clone(),
                        payload: PartKind::Plan { text },
                    })]
                }
                _ => Vec::new(),
            }
        }
        "thread/tokenUsage/updated" => {
            let usage = &params["tokenUsage"]["total"];
            vec![ProviderEvent::Part(PartDraft {
                turn_id: turn_id.clone(),
                payload: PartKind::Usage {
                    total_tokens: usage.get("totalTokens").and_then(Value::as_u64).unwrap_or(0),
                    input_tokens: usage.get("inputTokens").and_then(Value::as_u64).unwrap_or(0),
                    cached_input_tokens: usage
                        .get("cachedInputTokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    output_tokens: usage.get("outputTokens").and_then(Value::as_u64).unwrap_or(0),
                    context_window: params
                        .get("tokenUsage")
                        .and_then(|usage| usage.get("modelContextWindow"))
                        .and_then(Value::as_u64),
                },
            })]
        }
        "error" => {
            // t3code records a provider failure only when codex will not retry it.
            if params
                .get("willRetry")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Vec::new();
            }
            let message = params
                .pointer("/error/message")
                .and_then(Value::as_str)
                .or_else(|| params.get("message").and_then(Value::as_str))
                .unwrap_or("unknown app-server error")
                .to_owned();
            vec![ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::Error { message },
            })]
        }
        _ => Vec::new(),
    }
}

fn append_delta(params: &Value, buffers: &mut DeltaBuffers) {
    let Some(id) = params.get("itemId").and_then(Value::as_str) else {
        return;
    };
    let Some(delta) = params.get("delta").and_then(Value::as_str) else {
        return;
    };
    buffers.entry(id.to_owned()).or_default().push_str(delta);
}

fn take_buffer(buffers: &mut DeltaBuffers, id: Option<&str>) -> String {
    id.and_then(|id| buffers.remove(id)).unwrap_or_default()
}

fn named_status(raw: &str) -> super::types::ToolCallStatus {
    match raw {
        "inProgress" => super::types::ToolCallStatus::InProgress,
        "failed" => super::types::ToolCallStatus::Failed,
        "declined" => super::types::ToolCallStatus::Declined,
        "cancelled" => super::types::ToolCallStatus::Cancelled,
        _ => super::types::ToolCallStatus::Completed,
    }
}

fn command_status(item: &Value) -> super::types::ToolCallStatus {
    named_status(item.get("status").and_then(Value::as_str).unwrap_or("completed"))
}

fn patch_kind(kind: Option<&Value>) -> String {
    // `PatchChangeKind` is `#[serde(tag = "type")]`: `add`, `delete`, or `update {movePath}`.
    match kind {
        Some(Value::String(text)) => text.clone(),
        Some(value) => value
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| "update".into()),
        None => "update".into(),
    }
}

/// The approval-request methods codex asks a human through, with the part kind each maps to.
fn approval_kind(method: &str) -> Option<&'static str> {
    match method {
        "item/commandExecution/requestApproval" => Some("command"),
        "item/fileChange/requestApproval" => Some("fileChange"),
        _ => None,
    }
}

/// The summary line an approval part shows: the command, or the reason for a file change.
fn approval_summary(method: &str, params: &Value) -> String {
    let reason = params.get("reason").and_then(Value::as_str);
    match method {
        "item/commandExecution/requestApproval" => params
            .get("command")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| reason.map(str::to_owned))
            .unwrap_or_else(|| "command execution".into()),
        _ => reason.unwrap_or("file change").to_owned(),
    }
}

impl ConnectionIo {
    fn new(writer: Box<dyn AppServerWriter>) -> Arc<Self> {
        Arc::new(Self {
            writer: Mutex::new(writer),
            next_id: Mutex::new(1),
            pending: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
        })
    }

    async fn next_request_id(&self) -> i64 {
        let mut next = self.next_id.lock().await;
        let id = *next;
        *next += 1;
        id
    }

    async fn send_frame(&self, line: String) -> Result<(), StructuredChatError> {
        let mut writer = self.writer.lock().await;
        writer.send_line(line).await.map_err(|error| {
            StructuredChatError::Provider(format!("app-server write failed: {error}"))
        })
    }

    /// One request/response round trip with the shared timeout, keeping the wire error's code.
    async fn request_wire(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, WireError> {
        let id = self.next_request_id().await;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let frame = json!({"id": id, "method": method, "params": params});
        if let Err(error) = self.send_frame(frame.to_string()).await {
            self.pending.lock().await.remove(&id);
            return Err(WireError {
                code: None,
                message: error.message().to_owned(),
            });
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) | Err(_) => Err(WireError {
                code: None,
                message: format!("app-server never answered {method}"),
            }),
        }
    }

    /// One request/response round trip with the shared timeout.
    async fn request(&self, method: &str, params: Value) -> Result<Value, StructuredChatError> {
        self.request_wire(method, params)
            .await
            .map_err(|error| response_error(method, &error))
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), StructuredChatError> {
        let frame = json!({"method": method, "params": params});
        self.send_frame(frame.to_string()).await
    }

    /// Answers one pending server approval request by writing the JSON-RPC response frame the
    /// protocol expects: `{"id": <server id>, "result": {"decision": ...}}`.
    async fn answer_approval(
        &self,
        request_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), StructuredChatError> {
        let pending = self
            .approvals
            .lock()
            .await
            .remove(request_id)
            .ok_or(StructuredChatError::NotFound)?;
        let _ = pending.sender.send(decision);
        self.send_frame(
            json!({"id": pending.id, "result": {"decision": approval_decision_wire(decision)}})
                .to_string(),
        )
        .await
    }

    /// Drops every approval request the finished turn left open, so answering one afterwards is
    /// a `NotFound` instead of a response frame for an abandoned id.
    async fn purge_turn_approvals(&self, turn_id: &str) {
        self.approvals
            .lock()
            .await
            .retain(|_, pending| pending.turn_id.as_deref() != Some(turn_id));
    }

    /// Drops every pending approval request: the connection is gone, so none can be answered.
    async fn purge_all_approvals(&self) {
        self.approvals.lock().await.clear();
    }

    /// Drops the writer half. With the reader half already aborted, the last owner of the child
    /// handle is gone and `kill_on_drop` reaps `codex app-server` here, not at provider drop.
    async fn shutdown(&self) {
        let mut writer = self.writer.lock().await;
        *writer = Box::new(ClosedWriter);
    }
}

/// The writer a closed connection keeps, so `close` is idempotent and never writes again.
struct ClosedWriter;

#[async_trait::async_trait]
impl AppServerWriter for ClosedWriter {
    async fn send_line(&mut self, _line: String) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "app-server closed"))
    }
}

/// The camelCase decision string the protocol's `CommandExecutionApprovalDecision` spells.
fn approval_decision_wire(decision: ApprovalDecision) -> &'static str {
    match decision {
        ApprovalDecision::Accept => "accept",
        ApprovalDecision::AcceptForSession => "acceptForSession",
        ApprovalDecision::Decline => "decline",
    }
}

fn response_error(method: &str, error: &WireError) -> StructuredChatError {
    let message = error.message.as_str();
    let lowered = message.to_ascii_lowercase();
    if error.code == Some(JSONRPC_METHOD_NOT_FOUND)
        || lowered.contains("method not found")
        || lowered.contains("unknown method")
    {
        let name = if method.contains("compact") {
            "COMPACT_UNSUPPORTED: this codex has no thread/compact/start"
        } else if method.contains("steer") {
            "STEER_UNSUPPORTED: this codex has no turn/steer"
        } else if method.contains("queue/start") {
            "RESUME_UNSUPPORTED: this codex has no thread/queue/start"
        } else {
            "UNSUPPORTED_METHOD"
        };
        StructuredChatError::Unsupported(name.into())
    } else {
        StructuredChatError::Provider(format!("{method} failed: {message}"))
    }
}
/// Runs the reader loop: classify each inbound line, resolve pending responses, map
/// notifications into events, and hold approval requests open for the routes to answer.
async fn run_reader(
    io: Arc<ConnectionIo>,
    mut reader: Box<dyn AppServerReader>,
    thread_id: Arc<Mutex<Option<String>>>,
    events: mpsc::UnboundedSender<ProviderEvent>,
    mut buffers: DeltaBuffers,
) {
    // The turn the child is working on, tracked here so an EOF can fail it instead of leaving a
    // turn that simply stops. `turn/started` opens it, `turn/completed` closes it.
    let mut current_turn: Option<String> = None;
    let mut seen_agent_items: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_agent_text: Option<String> = None;
    loop {
        let line = match reader.read_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            // A decode failure is one unreadable line, not the end of the stream: `read_line`
            // reports invalid UTF-8 as `InvalidData`, and treating that as EOF would kill the
            // session over a single bad byte. Only a closed pipe is EOF.
            Err(error) if error.kind() == io::ErrorKind::InvalidData => continue,
            Err(_) => break,
        };
        match classify_frame(line.trim()) {
            InboundFrame::Malformed => continue,
            InboundFrame::Response { id, result } => {
                let Some(number) = id.as_i64() else { continue };
                if let Some(sender) = io.pending.lock().await.remove(&number) {
                    let _ = sender.send(result);
                }
            }
            InboundFrame::ServerRequest { id, method, params } => {
                let Some(kind) = approval_kind(&method) else {
                    // A request we cannot answer would hang the server: refuse it explicitly.
                    let _ = io
                        .send_frame(
                            json!({"id": id, "error": {"code": -32601, "message": "Method not found"}})
                                .to_string(),
                        )
                        .await;
                    continue;
                };
                let key = approval_key(&id);
                let (tx, _rx) = oneshot::channel();
                io.approvals.lock().await.insert(
                    key.clone(),
                    PendingApproval {
                        id: id.clone(),
                        turn_id: params
                            .get("turnId")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        sender: tx,
                    },
                );
                let event = ProviderEvent::Part(PartDraft {
                    turn_id: params.get("turnId").and_then(Value::as_str).map(str::to_owned),
                    payload: PartKind::ApprovalRequest {
                        request_id: key,
                        kind: kind.to_owned(),
                        summary: approval_summary(&method, &params),
                        reason: params.get("reason").and_then(Value::as_str).map(str::to_owned),
                        answered: false,
                    },
                });
                if events.send(event).is_err() {
                    continue;
                }
                // The reader stays live: the route answers through answer_approval, which writes
                // the response frame itself.
            }
            InboundFrame::Notification { method, params } => {
                match method.as_str() {
                    "thread/started" | "thread/status/changed" => {
                        if let Some(id) = params.pointer("/thread/id").and_then(Value::as_str) {
                            *thread_id.lock().await = Some(id.to_owned());
                        }
                    }
                    "turn/started" => {
                        if let Some(id) = params.pointer("/turn/id").and_then(Value::as_str) {
                            current_turn = Some(id.to_owned());
                            let _ = events.send(ProviderEvent::TurnStarted {
                                turn_id: id.to_owned(),
                            });
                        }
                    }
                    "turn/completed" => {
                        current_turn = None;
                        seen_agent_items.clear();
                        last_agent_text = None;
                        buffers.clear();
                        let turn = &params["turn"];
                        let turn_id = turn
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        // The turn's approval requests died with it: an answer that arrives now
                        // has no server request to resolve.
                        io.purge_turn_approvals(&turn_id).await;
                        let error = turn_error_message(turn);
                        let _ = events.send(ProviderEvent::TurnCompleted {
                            turn_id,
                            error,
                        });
                    }
                    "process/exited" => {
                        let _ = events.send(ProviderEvent::Ended {
                            reason: "app-server process exited".into(),
                        });
                    }
                    _ => {}
                }
                let completed_agent_id = (method == "item/completed"
                    && params.pointer("/item/type").and_then(Value::as_str) == Some("agentMessage"))
                    .then(|| {
                        params
                            .pointer("/item/id")
                            .and_then(Value::as_str)
                            .filter(|id| !id.is_empty())
                            .map(str::to_owned)
                    })
                    .flatten();
                for event in notification_events(&method, &params, &mut buffers) {
                    if !dedupe_agent_event(
                        &event,
                        completed_agent_id.as_deref(),
                        &mut seen_agent_items,
                        &mut last_agent_text,
                    ) {
                        continue;
                    }
                    if events.send(event).is_err() {
                        return;
                    }
                }
            }
        }
    }
    // The child is gone: fail every in-flight request now instead of after the timeout, and drop
    // the approval requests it can no longer answer.
    for (_, sender) in io.pending.lock().await.drain() {
        let _ = sender.send(Err(WireError {
            code: None,
            message: "app-server closed".into(),
        }));
    }
    io.purge_all_approvals().await;
    // A turn the child never completed is a failed turn, not a silent stop: report it before the
    // `Ended` the store turns into an `ended` session, so history records why it stopped.
    if let Some(turn_id) = current_turn.take() {
        let _ = events.send(ProviderEvent::TurnCompleted {
            turn_id,
            error: Some("the app-server exited before the turn completed".into()),
        });
    }
    let _ = events.send(ProviderEvent::Ended {
        reason: "app-server closed its stdout".into(),
    });
}

/// Drops duplicate `agentMessage` completions emitted when a retrying stream replays the same
/// item id or back-to-back identical reply within one turn, while keeping identical text that
/// legitimately recurs after a tool call or reasoning step.
fn dedupe_agent_event(
    event: &ProviderEvent,
    completed_item_id: Option<&str>,
    seen_agent_items: &mut std::collections::HashSet<String>,
    last_agent_text: &mut Option<String>,
) -> bool {
    let ProviderEvent::Part(draft) = event else {
        return true;
    };
    match &draft.payload {
        PartKind::AssistantMessage { text } => {
            if let Some(id) = completed_item_id {
                if !seen_agent_items.insert(id.to_owned()) {
                    return false;
                }
            }
            if last_agent_text.as_deref() == Some(text.as_str()) {
                return false;
            }
            *last_agent_text = Some(text.clone());
            true
        }
        PartKind::Error { .. } => true,
        _ => {
            *last_agent_text = None;
            true
        }
    }
}

/// The failure text of a `turn/completed` turn, or `None` when the turn succeeded.
///
/// `TurnError` is an object (`{message, codexErrorInfo, additionalDetails, misalignment}`), so a
/// failed turn's real reason lives under `/error/message`; reading the object as a string would
/// flatten every failure to a generic line.
pub(crate) fn turn_error_message(turn: &Value) -> Option<String> {
    let error = turn.get("error").filter(|error| !error.is_null())?;
    if let Some(message) = error.get("message").and_then(Value::as_str) {
        if !message.is_empty() {
            return Some(message.to_owned());
        }
    }
    if let Some(details) = error.get("additionalDetails").and_then(Value::as_str) {
        if !details.is_empty() {
            return Some(details.to_owned());
        }
    }
    Some("turn failed".into())
}

/// The Codex implementation of [`StructuredProvider`]. One instance owns one app-server child.
pub(crate) struct CodexProvider {
    thread_id: Arc<Mutex<Option<String>>>,
    io: Arc<ConnectionIo>,
    cwd: PathBuf,
    events_tx: mpsc::UnboundedSender<ProviderEvent>,
    reader: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// The child behind a real stdio connection; `None` for a test duplex.
    child: Option<ChildHandle>,
}

impl CodexProvider {
    /// Spawns a real `codex app-server` child for `cwd`. The child outlives no caller: it is
    /// killed on drop and by [`StructuredProvider::close`].
    pub fn spawn(
        cwd: PathBuf,
        events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> Result<Self, StructuredChatError> {
        let argv = app_server_argv();
        let transport = ChildAppServer::spawn(&argv, &cwd)
            .map_err(|error| StructuredChatError::Provider(format!("codex spawn failed: {error}")))?;
        Ok(Self::new(Box::new(transport), cwd, events))
    }

    pub fn new(
        transport: Box<dyn AppServerTransport>,
        cwd: PathBuf,
        events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> Self {
        let child = transport.child();
        let (reader, writer) = transport.split();
        let io = ConnectionIo::new(writer);
        let thread_id = Arc::new(Mutex::new(None));
        let reader = tokio::spawn(run_reader(
            Arc::clone(&io),
            reader,
            Arc::clone(&thread_id),
            events.clone(),
            DeltaBuffers::new(),
        ));
        Self {
            child,
            thread_id,
            io,
            cwd,
            events_tx: events,
            reader: Mutex::new(Some(reader)),
        }
    }

    async fn thread_id(&self) -> Option<String> {
        self.thread_id.lock().await.clone()
    }

    async fn require_thread_id(&self) -> Result<String, StructuredChatError> {
        self.thread_id().await.ok_or_else(|| {
            StructuredChatError::Conflict("SESSION_NOT_STARTED: no codex thread is open".into())
        })
    }

    async fn handshake(&self) -> Result<(), StructuredChatError> {
        self.io.request("initialize", initialize_params()).await?;
        self.io.notify("initialized", json!({})).await
    }

}

#[async_trait::async_trait]
impl StructuredProvider for CodexProvider {
    fn provider_id(&self) -> &'static str {
        "codex"
    }

    fn capabilities(&self) -> super::types::Capabilities {
        codex_capabilities()
    }

    async fn start(&self, resume_thread: Option<String>) -> Result<String, StructuredChatError> {
        self.handshake().await?;
        let model = None;
        let thread_id = match resume_thread {
            Some(thread_id) => {
                let result = self
                    .io
                    .request(
                        "thread/resume",
                        thread_resume_params(&thread_id, &self.cwd, model),
                    )
                    .await?;
                result
                    .pointer("/thread/id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or(thread_id)
            }
            None => {
                let result = self
                    .io
                    .request("thread/start", thread_start_params(&self.cwd, model, "startup"))
                    .await?;
                result
                    .pointer("/thread/id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        StructuredChatError::Provider("thread/start returned no thread id".into())
                    })?
            }
        };
        *self.thread_id.lock().await = Some(thread_id.clone());
        Ok(thread_id)
    }

    async fn turn(
        &self,
        text: String,
        attachments: Vec<String>,
        settings: &SessionSettings,
    ) -> Result<TurnRef, StructuredChatError> {
        let thread_id = self.require_thread_id().await?;
        let result = self
            .io
            .request(
                "turn/start",
                turn_start_params(&thread_id, &self.cwd, settings, &text, &attachments),
            )
            .await?;
        let turn_id = result
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                StructuredChatError::Provider("turn/start returned no turn id".into())
            })?;
        Ok(TurnRef { turn_id })
    }

    async fn steer(&self, expected_turn: &str, text: String) -> Result<(), StructuredChatError> {
        // Verified in codex-rs/app-server-protocol: `turn/steer {threadId, input, expectedTurnId}`.
        let thread_id = self.require_thread_id().await?;
        self.io
            .request(
                "turn/steer",
                turn_steer_params(&thread_id, expected_turn, &text),
            )
            .await?;
        Ok(())
    }

    async fn interrupt(&self, turn: &str) -> Result<(), StructuredChatError> {
        let thread_id = self.require_thread_id().await?;
        self.io
            .request(
                "turn/interrupt",
                json!({"threadId": thread_id, "turnId": turn}),
            )
            .await?;
        Ok(())
    }

    /// Drops every turn from `before_turn` on, via `thread/revert {threadId, beforeTurnId}` —
    /// the protocol's exact rollback method (verified in codex-rs/app-server-protocol
    /// `ThreadRevertParams`).
    async fn rollback(&self, before_turn: &str) -> Result<String, StructuredChatError> {
        let thread_id = self.require_thread_id().await?;
        self.io
            .request(
                "thread/revert",
                json!({"threadId": thread_id, "beforeTurnId": before_turn}),
            )
            .await?;
        // `thread/revert` rewinds the live thread in place; the id the caller must resume with is
        // the one it already had.
        Ok(thread_id)
    }

    async fn answer_approval(
        &self,
        request_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), StructuredChatError> {
        self.io.answer_approval(request_id, decision).await
    }

    /// Starts a fresh thread in this session (`/clear`), emitting nothing itself; the store
    /// appends the `newSession` divider part.
    ///
    /// No handshake here: the store calls this only on a running provider, whose connection is
    /// already initialized, and codex answers a second `initialize` with `Already initialized`.
    async fn clear(&self) -> Result<String, StructuredChatError> {
        let result = self
            .io
            .request("thread/start", thread_start_params(&self.cwd, None, "clear"))
            .await?;
        let thread_id = result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                StructuredChatError::Provider("thread/start returned no thread id".into())
            })?;
        *self.thread_id.lock().await = Some(thread_id.clone());
        Ok(thread_id)
    }

    /// `thread/compact/start {threadId}` — the protocol's compact method.
    async fn compact(&self) -> Result<(), StructuredChatError> {
        let thread_id = self.require_thread_id().await?;
        self.io
            .request("thread/compact/start", json!({"threadId": thread_id}))
            .await?;
        Ok(())
    }

    /// `thread/queue/start {threadId}` — codex's native continuation for a stopped run.
    ///
    /// A stop holds the queue instead of discarding it, and this is the call that resumes it; the
    /// response carries the turn it started, which the caller learns about through
    /// `turn/started` like any other. t3code's composer "Resume" is this behaviour (composer.md,
    /// "Press Resume in an empty composer to continue... this uses the provider's native
    /// continuation").
    /// `thread/queue/start {threadId}` — codex's native continuation for a stopped run.
    ///
    /// A stop holds the queue instead of discarding it, and this is the call that resumes it. The
    /// response is a `ThreadQueueStartResponse { turn }`; that turn is the resumed turn, so it is
    /// reported as `TurnStarted` here: without it the resumed turn never enters the store's
    /// `turn_ids`, and steer/interrupt/rollback cannot address it. t3code's composer "Resume" is
    /// this behaviour (composer.md, "Press Resume in an empty composer to continue... this uses the
    /// provider's native continuation").
    async fn resume(&self) -> Result<(), StructuredChatError> {
        let thread_id = self.require_thread_id().await?;
        let wire = self
            .io
            .request_wire("thread/queue/start", json!({"threadId": thread_id}))
            .await;
        match wire {
            Ok(result) => {
                if let Some(turn_id) = result.pointer("/turn/id").and_then(Value::as_str) {
                    let _ = self.events_tx.send(ProviderEvent::TurnStarted {
                        turn_id: turn_id.to_owned(),
                    });
                }
                Ok(())
            }
            // Nothing to continue is a state conflict, not a provider failure: the composer
            // answers it by sending a message. The wire says so with an invalid-request code
            // (there is no queued submission for this thread), so the decision reads the code,
            // never the prose, which no pinned codex source contains.
            Err(WireError {
                code: Some(JSONRPC_INVALID_REQUEST),
                ..
            }) => Err(StructuredChatError::Conflict(
                "RESUME_NOTHING_TO_CONTINUE: no stopped run is queued for this thread".into(),
            )),
            Err(error) => Err(response_error("thread/queue/start", &error)),
        }
    }

    /// Aborts the reader, closes the connection, and kills the child. Idempotent.
    async fn close(&self) {
        let mut reader = self.reader.lock().await;
        if let Some(handle) = reader.take() {
            handle.abort();
        }
        drop(reader);
        self.io.shutdown().await;
        if let Some(child) = &self.child {
            child.kill();
        }
    }

    fn event_sink(&self) -> mpsc::UnboundedSender<ProviderEvent> {
        self.events_tx.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests. The fake transport is an in-process duplex: tests script the inbound
// lines and read back exactly what the provider wrote.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structured_chat::types::ToolCallStatus;
    use tokio::sync::mpsc::UnboundedReceiver;

    /// One scripted thing the fake child does.
    ///
    /// `Emit` is unsolicited inbound traffic (a notification, a server request, a malformed
    /// line); `Reply` answers the next outbound *request*. A real app-server can only answer
    /// after reading the request, so the script is pumped on writes rather than preloaded: a
    /// preloaded answer would race the request that registers the pending entry.
    enum Step {
        Emit(String),
        Reply(String),
    }

    /// A scripted stdio child. `written` records everything the provider sent.
    struct FakeReader(mpsc::UnboundedReceiver<String>);

    struct FakeWriter {
        script: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Step>>>,
        inbound: InboundSink,
        written: mpsc::UnboundedSender<String>,
    }

    /// The child's stdout sender, shared with [`ExitHandle`] so a test can close the stream.
    /// Dropping the only sender is exactly what a real child exiting stdout does.
    #[derive(Clone)]
    struct InboundSink(std::sync::Arc<std::sync::Mutex<Option<mpsc::UnboundedSender<String>>>>);

    /// Closes the fake child's stdout, so the reader sees EOF and reports `Ended`.
    struct ExitHandle(InboundSink);

    impl ExitHandle {
        fn exit(&self) {
            let _ = self.0.0.lock().expect("sink lock").take();
        }
    }

    /// Builds the transport, the receiver of everything the provider writes, and an exit handle.
    fn fake_transport(
        script: Vec<Step>,
    ) -> (Box<dyn AppServerTransport>, UnboundedReceiver<String>, ExitHandle) {
        let (written, written_rx) = mpsc::unbounded_channel();
        let (inbound, inbound_rx) = mpsc::unbounded_channel();
        let sink = InboundSink(std::sync::Arc::new(std::sync::Mutex::new(Some(inbound))));
        (
            Box::new(FakeTransport {
                script: std::sync::Arc::new(std::sync::Mutex::new(script.into())),
                sink: sink.clone(),
                written,
                reader: std::sync::Arc::new(std::sync::Mutex::new(Some(inbound_rx))),
            }),
            written_rx,
            ExitHandle(sink),
        )
    }

    /// The in-process duplex behind [`fake_transport`].
    struct FakeTransport {
        script: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Step>>>,
        sink: InboundSink,
        written: mpsc::UnboundedSender<String>,
        reader: std::sync::Arc<std::sync::Mutex<Option<mpsc::UnboundedReceiver<String>>>>,
    }

    impl AppServerTransport for FakeTransport {
        fn split(self: Box<Self>) -> (Box<dyn AppServerReader>, Box<dyn AppServerWriter>) {
            let reader = self
                .reader
                .lock()
                .expect("reader lock")
                .take()
                .expect("reader taken once");
            (
                Box::new(FakeReader(reader)),
                Box::new(FakeWriter {
                    script: self.script,
                    inbound: self.sink,
                    written: self.written,
                }) as Box<dyn AppServerWriter>,
            )
        }
    }

    #[async_trait::async_trait]
    impl AppServerReader for FakeReader {
        async fn read_line(&mut self) -> io::Result<Option<String>> {
            self.0.recv().await.map(Some).ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "script exhausted")
            })
        }
    }

    #[async_trait::async_trait]
    impl AppServerWriter for FakeWriter {
        async fn send_line(&mut self, line: String) -> io::Result<()> {
            let _ = self.written.send(line.clone());
            // A frame with an `id` is a request and earns one scripted reply; a notification
            // earns none. Unsolicited steps in between always go out.
            let is_request = serde_json::from_str::<Value>(&line)
                .map(|frame| frame.get("id").is_some())
                .unwrap_or(false);
            let mut script = self.script.lock().expect("script lock");
            let mut wants_reply = is_request;
            loop {
                match script.front() {
                    Some(Step::Emit(_)) => {
                        let Some(Step::Emit(line)) = script.pop_front() else {
                            unreachable!()
                        };
                        self.deliver(line);
                    }
                    Some(Step::Reply(_)) if wants_reply => {
                        let Some(Step::Reply(line)) = script.pop_front() else {
                            unreachable!()
                        };
                        wants_reply = false;
                        self.deliver(line);
                    }
                    _ => break,
                }
            }
            Ok(())
        }
    }

    impl FakeWriter {
        fn deliver(&self, line: String) {
            if let Some(sink) = self.inbound.0.lock().expect("sink lock").as_ref() {
                let _ = sink.send(line);
            }
        }
    }

    struct Harness {
        provider: CodexProvider,
        written: mpsc::UnboundedReceiver<String>,
        events: UnboundedReceiver<ProviderEvent>,
    }

    impl Harness {
        fn new(script: Vec<Step>) -> Self {
            let (transport, written_rx, _exit) = fake_transport(script);
            let (events_tx, events_rx) = mpsc::unbounded_channel();
            let provider = CodexProvider::new(transport, PathBuf::from("/tmp"), events_tx);
            Self {
                provider,
                written: written_rx,
                events: events_rx,
            }
        }

        async fn outbound(&mut self) -> Vec<Value> {
            let mut frames = Vec::new();
            while let Ok(line) = self.written.try_recv() {
                frames.push(serde_json::from_str(&line).expect("outbound frame is JSON"));
            }
            frames
        }
    }

    /// A codex response frame: only `id` and `result` (the protocol never echoes the method).
    fn inbound_response(id: i64, method: &str, result: Value) -> Step {
        let _ = method;
        Step::Reply(json!({"id": id, "result": result}).to_string())
    }

    /// A server→client *request*: a method plus an `id` the client must answer.
    fn server_request(id: i64, method: &str, params: Value) -> Step {
        Step::Emit(json!({"id": id, "method": method, "params": params}).to_string())
    }

    async fn started(harness: &Harness) -> String {
        harness.provider.start(None).await.expect("start")
    }

    fn settings(mode: PermissionMode) -> SessionSettings {
        SessionSettings {
            model: Some("gpt-5.1-codex".into()),
            effort: Some("medium".into()),
            permission_mode: Some(mode),
            ..SessionSettings::default()
        }
    }


    #[tokio::test]
    async fn start_handshakes_and_starts_a_thread() {
        let mut harness = Harness::new(vec![
            inbound_response(1, "initialize", json!({"userAgent": "test"})),
            inbound_response(2, "thread/start", json!({"thread": {"id": "th-1"}})),
        ]);
        let thread = started(&harness).await;
        assert_eq!(thread, "th-1");
        let frames = harness.outbound().await;
        assert_eq!(frames[0]["method"], "initialize");
        assert!(frames[0]["params"]["clientInfo"]["name"] == "ferryx");
        assert_eq!(frames[1]["method"], "initialized");
        assert_eq!(frames[2]["method"], "thread/start");
        assert_eq!(frames[2]["params"]["cwd"], "/tmp");
        assert_eq!(frames[2]["params"]["sessionStartSource"], "startup");
    }

    /// Gap item 1: an empty queue means there is no stopped run to continue, which is a state
    /// conflict the composer answers by sending a message -- not a provider failure.
    #[tokio::test]
    async fn resume_on_an_empty_queue_is_a_typed_conflict() {
        let mut harness = Harness::new(vec![
            inbound_response(1, "initialize", json!({})),
            inbound_response(2, "thread/start", json!({"thread": {"id": "th-1"}})),
            Step::Reply(
                json!({"id": 3, "error": {"code": -32600, "message": "queue is empty"}})
                    .to_string(),
            ),
        ]);
        started(&harness).await;
        assert_eq!(
            harness.provider.resume().await.unwrap_err(),
            StructuredChatError::Conflict(
                "RESUME_NOTHING_TO_CONTINUE: no stopped run is queued for this thread".into()
            )
        );
        let frames = harness.outbound().await;
        assert_eq!(frames[3]["method"], "thread/queue/start");
        assert_eq!(frames[3]["params"]["threadId"], "th-1");
    }

    #[tokio::test]
    async fn resume_uses_thread_resume_with_the_named_thread() {
        let mut harness = Harness::new(vec![
            inbound_response(1, "initialize", json!({})),
            inbound_response(2, "thread/resume", json!({"thread": {"id": "th-resume"}})),
        ]);
        let thread = harness.provider.start(Some("th-resume".into())).await.unwrap();
        assert_eq!(thread, "th-resume");
        let frames = harness.outbound().await;
        assert_eq!(frames[2]["method"], "thread/resume");
        assert_eq!(frames[2]["params"]["threadId"], "th-resume");
        assert_eq!(frames[2]["params"]["excludeTurns"], true);
    }

    #[tokio::test]
    async fn clear_starts_a_thread_without_reinitializing() {
        let mut harness = Harness::new(vec![
            inbound_response(1, "initialize", json!({})),
            inbound_response(2, "thread/start", json!({"thread": {"id": "th-1"}})),
            inbound_response(3, "thread/start", json!({ "thread": { "id": "th-2" } })),
        ]);
        started(&harness).await;
        let thread = harness.provider.clear().await.unwrap();
        assert_eq!(thread, "th-2");
        let frames = harness.outbound().await;
        // A live connection is already initialized: codex answers a second `initialize` with
        // `Already initialized`, so `/clear` must go straight to `thread/start`.
        assert_eq!(frames.iter().filter(|f| f["method"] == "initialize").count(), 1);
        assert_eq!(frames[3]["method"], "thread/start");
        assert_eq!(frames[3]["params"]["sessionStartSource"], "clear");
    }

    #[test]
    fn probe_reply_complete_detects_the_model_list_answer() {
        assert!(!probe_reply_complete(b""));
        assert!(!probe_reply_complete(
            br#"{"id":1,"result":{"userAgent":"x"}}"#
        ));
        assert!(!probe_reply_complete(
            br#"{"method":"configWarning","params":{}}"#
        ));
        assert!(probe_reply_complete(
            b"{\"id\":1,\"result\":{}}\n{\"id\":2,\"result\":{\"data\":[]}}\n"
        ));
        assert!(probe_reply_complete(b"{\"id\":2,\"error\":{\"code\":-1}}"));
    }

    #[tokio::test]
    async fn turn_start_carries_mode_mapping_and_settings() {
        let mut harness = Harness::new(vec![
            inbound_response(1, "initialize", json!({})),
            inbound_response(2, "thread/start", json!({"thread": {"id": "th-1"}})),
            inbound_response(3, "turn/start", json!({"turn": {"id": "turn-1"}})),
        ]);
        started(&harness).await;
        let turn = harness
            .provider
            .turn("hello".into(), Vec::new(), &settings(PermissionMode::FullAccess))
            .await
            .unwrap();
        assert_eq!(turn.turn_id, "turn-1");
        let frames = harness.outbound().await;
        let params = &frames[3]["params"];
        assert_eq!(params["threadId"], "th-1");
        assert_eq!(params["summary"], "auto");
        assert_eq!(params["model"], "gpt-5.1-codex");
        assert_eq!(params["effort"], "medium");
        assert_eq!(params["approvalsReviewer"], "user");
        assert_eq!(params["approvalPolicy"]["granular"]["sandbox_approval"], false);
        assert_eq!(params["sandboxPolicy"]["type"], "dangerFullAccess");
        assert_eq!(params["input"][0]["type"], "text");
        assert_eq!(params["input"][0]["text"], "hello");
    }

    #[test]
    fn runtime_mode_mapping_matches_t3code_byte_for_byte() {
        let (policy, reviewer, sandbox) =
            runtime_mode_turn_defaults(PermissionMode::Supervised);
        assert_eq!(policy, json!("untrusted"));
        assert_eq!(reviewer, "user");
        assert_eq!(sandbox, json!({"type": "readOnly"}));

        let (policy, reviewer, sandbox) =
            runtime_mode_turn_defaults(PermissionMode::AutoAcceptEdits);
        assert_eq!(policy, json!("on-request"));
        assert_eq!(reviewer, "user");
        assert_eq!(sandbox, json!({"type": "workspaceWrite"}));

        let (policy, reviewer, sandbox) = runtime_mode_turn_defaults(PermissionMode::Auto);
        assert_eq!(policy, json!("on-request"));
        assert_eq!(reviewer, "auto_review");
        assert_eq!(sandbox, json!({"type": "workspaceWrite"}));

        let (policy, reviewer, sandbox) = runtime_mode_turn_defaults(PermissionMode::FullAccess);
        assert_eq!(policy["granular"]["mcp_elicitations"], true);
        assert_eq!(reviewer, "user");
        assert_eq!(sandbox, json!({"type": "dangerFullAccess"}));
    }

    // The store stamps the user turn's own `UserMessage` part; deltas below only carry the
    // assistant side.
    #[tokio::test]
    async fn streamed_deltas_merge_into_one_assistant_part() {
        let mut buffers = DeltaBuffers::new();
        // `item/started` never produces a part: the store owns the user message part.
        assert!(notification_events(
            "item/started",
            &json!({"turnId": "t1", "item": {"type": "userMessage", "id": "u1",
                "content": [{"type": "text", "text": "hi"}]}}),
            &mut buffers,
        )
        .is_empty());

        for delta in ["Hel", "lo ", "wor", "ld"] {
            let _ = notification_events(
                "item/agentMessage/delta",
                &json!({"turnId": "t1", "itemId": "m1", "delta": delta}),
                &mut buffers,
            );
        }
        let events = notification_events(
            "item/completed",
            &json!({"turnId": "t1", "item": {"type": "agentMessage", "id": "m1"}}),
            &mut buffers,
        );
        assert!(matches!(
            &events[0],
            ProviderEvent::Part(PartDraft { payload: PartKind::AssistantMessage { text }, .. }) if text == "Hello world"
        ));
    }

    #[test]
    fn reasoning_content_and_summary_deltas_both_merge() {
        let mut buffers = DeltaBuffers::new();
        for (method, delta) in [
            ("item/reasoning/summaryTextDelta", "summary "),
            ("item/reasoning/textDelta", "body "),
        ] {
            let _ = notification_events(
                method,
                &json!({"turnId": "t1", "itemId": "r1", "delta": delta}),
                &mut buffers,
            );
        }
        let events = notification_events(
            "item/completed",
            &json!({"turnId": "t1", "item": {"type": "reasoning", "id": "r1"}}),
            &mut buffers,
        );
        assert!(matches!(
            &events[0],
            ProviderEvent::Part(PartDraft { payload: PartKind::Reasoning { text }, .. })
                if text == "summary body "
        ));
    }

    #[test]
    fn plan_deltas_survive_when_the_completed_item_omits_text() {
        let mut buffers = DeltaBuffers::new();
        let _ = notification_events(
            "item/plan/delta",
            &json!({"turnId": "t1", "itemId": "p1", "delta": "step one"}),
            &mut buffers,
        );
        let events = notification_events(
            "item/completed",
            &json!({"turnId": "t1", "item": {"type": "plan", "id": "p1"}}),
            &mut buffers,
        );
        assert!(matches!(
            &events[0],
            ProviderEvent::Part(PartDraft { payload: PartKind::Plan { text }, .. })
                if text == "step one"
        ));
    }

    #[test]
    fn a_retryable_error_is_not_recorded_as_a_failure_part() {
        let mut buffers = DeltaBuffers::new();
        assert!(notification_events(
            "error",
            &json!({"turnId": "t1", "willRetry": true, "error": {"message": "flaky"}}),
            &mut buffers,
        )
        .is_empty());
        let events = notification_events(
            "error",
            &json!({"turnId": "t1", "error": {"message": "boom"}}),
            &mut buffers,
        );
        assert!(matches!(
            &events[0],
            ProviderEvent::Part(PartDraft { payload: PartKind::Error { message }, .. }) if message == "boom"
        ));
    }

    #[tokio::test]
    async fn tool_call_part_carries_status_output_and_exit_code() {
        let mut buffers = DeltaBuffers::new();
        let _ = notification_events(
            "item/commandExecution/outputDelta",
            &json!({"turnId": "t1", "itemId": "c1", "delta": "out"}),
            &mut buffers,
        );
        let events = notification_events(
            "item/completed",
            &json!({"turnId": "t1", "item": {"type": "commandExecution", "id": "c1",
                "status": "completed", "exitCode": 0, "durationMs": 42}}),
            &mut buffers,
        );
        assert!(matches!(
            &events[0],
            ProviderEvent::Part(PartDraft {
                payload: PartKind::ToolCall { name, status, output, exit_code, duration_ms, .. },
                ..
            }) if name == "shell" && *status == ToolCallStatus::Completed
                && output.as_deref() == Some("out") && *exit_code == Some(0) && *duration_ms == Some(42)
        ));
    }

    #[tokio::test]
    async fn steer_interrupt_and_rollback_send_the_protocol_methods() {
        let mut harness = Harness::new(vec![
            inbound_response(1, "initialize", json!({})),
            inbound_response(2, "thread/start", json!({"thread": {"id": "th-1"}})),
            inbound_response(3, "turn/steer", json!({"turnId": "turn-1"})),
            inbound_response(4, "turn/interrupt", json!({})),
            inbound_response(5, "thread/revert", json!({"thread": {"id": "th-1"}})),
        ]);
        started(&harness).await;
        harness.provider.steer("turn-1", "go left".into()).await.unwrap();
        harness.provider.interrupt("turn-1").await.unwrap();
        harness.provider.rollback("turn-1").await.unwrap();
        let frames = harness.outbound().await;
        assert_eq!(frames[3]["method"], "turn/steer");
        assert_eq!(frames[3]["params"]["expectedTurnId"], "turn-1");
        assert_eq!(frames[3]["params"]["input"][0]["text"], "go left");
        assert_eq!(frames[4]["method"], "turn/interrupt");
        assert_eq!(frames[4]["params"]["turnId"], "turn-1");
        assert_eq!(frames[5]["method"], "thread/revert");
        assert_eq!(frames[5]["params"]["beforeTurnId"], "turn-1");
    }

    #[tokio::test]
    async fn approval_round_trip_answers_with_the_decision() {
        let mut harness = Harness::new(vec![
            inbound_response(1, "initialize", json!({})),
            inbound_response(2, "thread/start", json!({"thread": {"id": "th-1"}})),
            server_request(
                900,
                "item/commandExecution/requestApproval",
                json!({"threadId": "th-1", "turnId": "t1", "itemId": "c1",
                    "command": "rm -rf /", "reason": "sandbox escape"}),
            ),
        ]);
        started(&harness).await;
        let mut request_id = String::new();
        while let Some(event) = harness.events.recv().await {
            if let ProviderEvent::Part(PartDraft { payload: PartKind::ApprovalRequest { request_id: id, kind, summary, reason, .. }, .. }) = event {
                assert_eq!(kind, "command");
                assert_eq!(summary, "rm -rf /");
                assert_eq!(reason.as_deref(), Some("sandbox escape"));
                request_id = id;
                break;
            }
        }
        assert_eq!(request_id, "900");
        harness
            .provider
            .answer_approval(&request_id, ApprovalDecision::AcceptForSession)
            .await
            .unwrap();
        let frames = harness.outbound().await;
        let answer = frames
            .iter()
            .find(|frame| frame["id"] == 900)
            .expect("approval answer frame");
        assert_eq!(answer["result"]["decision"], "acceptForSession");
        // A second answer for the same request is a 404.
        let error = harness
            .provider
            .answer_approval(&request_id, ApprovalDecision::Decline)
            .await
            .unwrap_err();
        assert_eq!(error, StructuredChatError::NotFound);
    }

    #[tokio::test]
    async fn malformed_lines_are_skipped_and_the_connection_survives() {
        let harness = Harness::new(vec![
            // Unsolicited junk the child emits before it is ever asked anything.
            Step::Emit("{not json".into()),
            Step::Emit("[]".into()),
            Step::Emit(json!({"id": 1, "nonsense": true}).to_string()),
            inbound_response(1, "initialize", json!({})),
            inbound_response(2, "thread/start", json!({"thread": {"id": "th-1"}})),
        ]);
        let thread = started(&harness).await;
        assert_eq!(thread, "th-1");
    }

    #[tokio::test]
    async fn child_exit_mid_turn_emits_ended() {
        // A child that exits closes its stdout: the reader must report `Ended` rather than hang
        // until the next request times out.
        let (transport, _written_rx, exit) = fake_transport(Vec::new());
        let (events_tx, mut events_rx) = mpsc::unbounded_channel();
        let _provider = CodexProvider::new(transport, PathBuf::from("/tmp"), events_tx);
        exit.exit();
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), events_rx.recv())
            .await
            .expect("ended event")
            .expect("channel open");
        assert!(matches!(event, ProviderEvent::Ended { .. }));
    }

    #[tokio::test]
    async fn method_not_found_maps_to_typed_unsupported() {
        let harness = Harness::new(vec![
            inbound_response(1, "initialize", json!({})),
            inbound_response(2, "thread/start", json!({"thread": {"id": "th-1"}})),
            Step::Reply(
                json!({"id": 3, "error": {"code": -32601, "message": "Method not found"}})
                    .to_string(),
            ),
        ]);
        started(&harness).await;
        let error = harness.provider.compact().await.unwrap_err();
        assert!(matches!(error, StructuredChatError::Unsupported(_)));
        assert_eq!(error.code(), "COMPACT_UNSUPPORTED");
    }

    #[test]
    fn frame_classification_separates_notifications_requests_and_responses() {
        assert!(matches!(
            classify_frame("{\"method\":\"item/started\",\"params\":{}}"),
            InboundFrame::Notification { .. }
        ));
        assert!(matches!(
            classify_frame("{\"id\":900,\"method\":\"item/commandExecution/requestApproval\",\"params\":{}}"),
            InboundFrame::ServerRequest { .. }
        ));
        assert!(matches!(
            classify_frame("{\"id\":3,\"result\":{\"ok\":true}}"),
            InboundFrame::Response { .. }
        ));
        assert!(matches!(
            classify_frame("{\"id\":4,\"error\":{\"message\":\"nope\"}}"),
            InboundFrame::Response { result: Err(_), .. }
        ));
        assert!(matches!(classify_frame("{not json"), InboundFrame::Malformed));
        assert!(matches!(classify_frame("[1,2]"), InboundFrame::Malformed));
    }

    #[test]
    fn usage_part_reads_token_usage_and_context_window() {
        let mut buffers = DeltaBuffers::new();
        let events = notification_events(
            "thread/tokenUsage/updated",
            &json!({"turnId": "t1", "tokenUsage": {
                "total": {"totalTokens": 21575, "inputTokens": 21310, "cachedInputTokens": 6912,
                    "outputTokens": 265},
                "modelContextWindow": 258400}}),
            &mut buffers,
        );
        assert!(matches!(
            &events[0],
            ProviderEvent::Part(PartDraft { payload: PartKind::Usage {
                total_tokens, input_tokens, cached_input_tokens, output_tokens, context_window,
            }, .. }) if *total_tokens == 21575 && *input_tokens == 21310
                && *cached_input_tokens == 6912 && *output_tokens == 265
                && *context_window == Some(258400)
        ));
    }

    #[test]
    fn file_change_part_keeps_paths_kinds_and_diffs() {
        let mut buffers = DeltaBuffers::new();
        let events = notification_events(
            "item/completed",
            &json!({"turnId": "t1", "item": {"type": "fileChange", "id": "f1", "status": "completed",
                "changes": [{"path": "a.rs", "kind": {"type": "add"}, "diff": "+fn a(){}"}]}}),
            &mut buffers,
        );
        assert!(matches!(
            &events[0],
            ProviderEvent::Part(PartDraft { payload: PartKind::FileChange { changes, status, .. }, .. })
                if changes.len() == 1 && changes[0].path == "a.rs"
                    && changes[0].kind == "add" && changes[0].diff == "+fn a(){}"
                    && *status == ToolCallStatus::Completed
        ));
    }

    #[test]
    fn error_notification_becomes_an_error_part() {
        let mut buffers = DeltaBuffers::new();
        let events = notification_events(
            "error",
            &json!({"message": "boom"}),
            &mut buffers,
        );
        assert!(matches!(
            &events[0],
            ProviderEvent::Part(PartDraft { payload: PartKind::Error { message }, .. }) if message == "boom"
        ));
    }

    #[test]
    fn duplicate_agent_message_retries_are_dropped_within_a_turn() {
        let mut seen = std::collections::HashSet::new();
        let mut last = None;
        let msg = ProviderEvent::Part(PartDraft {
            turn_id: Some("t1".into()),
            payload: PartKind::AssistantMessage { text: "ok".into() },
        });
        assert!(dedupe_agent_event(&msg, Some("m1"), &mut seen, &mut last));
        assert!(!dedupe_agent_event(&msg, Some("m1"), &mut seen, &mut last));
        assert!(!dedupe_agent_event(&msg, Some("m2"), &mut seen, &mut last));
        let tool = ProviderEvent::Part(PartDraft {
            turn_id: Some("t1".into()),
            payload: PartKind::ToolCall {
                id: "c1".into(),
                name: "shell".into(),
                status: ToolCallStatus::Completed,
                output: None,
                exit_code: Some(0),
                duration_ms: None,
            },
        });
        assert!(dedupe_agent_event(&tool, None, &mut seen, &mut last));
        assert!(dedupe_agent_event(&msg, Some("m3"), &mut seen, &mut last));
    }

    #[test]
    fn attachments_ride_as_local_image_and_mention_rows() {
        let input = turn_input("look", &["/tmp/a.png".into(), "/tmp/notes.md".into()]);
        let rows = input.as_array().expect("array");
        assert_eq!(rows[0]["type"], "text");
        assert_eq!(rows[0]["text"], "look");
        assert_eq!(rows[1]["type"], "localImage");
        assert_eq!(rows[1]["path"], "/tmp/a.png");
        assert_eq!(rows[2]["type"], "mention");
        assert_eq!(rows[2]["name"], "notes.md");
        assert_eq!(rows[2]["path"], "/tmp/notes.md");
    }
}
