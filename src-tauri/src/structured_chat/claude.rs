//! The Claude structured-chat provider: `claude -p` stream-json over stdio.
//!
//! Wire shapes were read from the Claude Agent SDK protocol (`sdk/package/sdk.d.ts` and
//! `sdk.mjs`, saved by the plan under
//! `.evidence/chat-t3-parity-20261125/lanes/chat-g2-claude-20261125/`), from t3code's Claude
//! adapter (`ClaudeAdapterV2.ts`, `ClaudeTurnTokenUsage.ts`), and confirmed against the installed
//! CLI (`claude --help`, 2.1.309):
//!
//! * Launch: `claude -p --input-format stream-json --output-format stream-json --verbose
//!   --include-partial-messages --permission-prompt-tool=stdio --permission-mode <mode>
//!   [--effort <level>] [--model <id>] (--session-id <uuid> | --resume <session>
//!   [--resume-session-at <uuid>])`. `--include-partial-messages` and
//!   `--permission-prompt-tool=stdio` are exactly what the SDK's `ProcessTransport` pushes for
//!   `includePartialMessages` and for a host-side `canUseTool` callback; without the second flag
//!   the CLI has no prompt surface and never sends `can_use_tool`. The permission-mode strings are
//!   the SDK's `PermissionMode` literals, which `claude --permission-mode` accepts.
//! * Turn input: one `{"type":"user","message":{…},"uuid":…}` line on stdin. The CLI reads
//!   further user lines while a turn runs, which is how steering works (`SDKUserMessage.priority`
//!   and the steering section of t3code's Claude provider docs).
//! * Control protocol (`SDKControlRequest` / `SDKControlResponse`): the CLI asks the host through
//!   `control_request` frames with `request.subtype == "can_use_tool"`; the host answers with a
//!   `control_response` frame carrying the same `request_id` and a `PermissionResult`. `interrupt`,
//!   `set_model` and `apply_flag_settings` are host→CLI control requests. A permission-mode change
//!   is not: the CLI refuses `bypassPermissions` on a process it was not launched for, so the
//!   provider reopens the session with the new launch flags instead.
//! * Parts come from `stream_event` deltas, `assistant` messages (text, thinking, tool_use),
//!   `user` messages (tool_result), `system` `init` (model, slash commands) and `result` (usage,
//!   turn end).
//!
//! The provider never builds a shell string: the CLI is spawned from an argv array, and the cwd
//! is validated by [`crate::structured_chat::store`] before this module ever sees it.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};

use super::types::{
    ApprovalDecision, Capabilities, FileChangeEntry, ModelInfo, PartKind, PermissionMode,
    SessionSettings, StructuredChatError, ToolCallStatus,
};
use super::{PartDraft, ProviderEvent, StructuredProvider, TurnRef};

/// Program name. The provider never accepts a client-supplied path.
pub const CLAUDE_PROGRAM: &str = "claude";

/// How long one control request may wait for its response before failing.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/// One inbound stream-json line, already classified.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InboundFrame {
    /// A CLI→host control request (`can_use_tool`, `request_user_dialog`, …).
    ControlRequest { request_id: String, request: Value },
    /// The answer to one host→CLI control request.
    ControlResponse {
        request_id: String,
        result: Result<Value, String>,
    },
    /// The CLI withdrew one of its own requests (`control_cancel_request`).
    ControlCancelled { request_id: String },
    /// A plain stream message: `assistant`, `user`, `system`, `result`, `stream_event`, …
    Message(Value),
    Malformed,
}

/// A line that is not a JSON object, or a control frame missing its ids, is malformed and
/// skipped: one bad line must not take the connection down.
pub(crate) fn classify_frame(line: &str) -> InboundFrame {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return InboundFrame::Malformed;
    };
    let Some(object) = value.as_object() else {
        return InboundFrame::Malformed;
    };
    match object.get("type").and_then(Value::as_str) {
        Some("control_request") => {
            let Some(request_id) = object.get("request_id").and_then(Value::as_str) else {
                return InboundFrame::Malformed;
            };
            InboundFrame::ControlRequest {
                request_id: request_id.to_owned(),
                request: object.get("request").cloned().unwrap_or(Value::Null),
            }
        }
        Some("control_response") => {
            let Some(response) = object.get("response").and_then(Value::as_object) else {
                return InboundFrame::Malformed;
            };
            let Some(request_id) = response.get("request_id").and_then(Value::as_str) else {
                return InboundFrame::Malformed;
            };
            let result = match response.get("subtype").and_then(Value::as_str) {
                Some("success") => Ok(response.get("response").cloned().unwrap_or(Value::Null)),
                Some("error") => Err(response
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown control error")
                    .to_owned()),
                _ => return InboundFrame::Malformed,
            };
            InboundFrame::ControlResponse {
                request_id: request_id.to_owned(),
                result,
            }
        }
        Some("control_cancel_request") => {
            let Some(request_id) = object.get("request_id").and_then(Value::as_str) else {
                return InboundFrame::Malformed;
            };
            InboundFrame::ControlCancelled {
                request_id: request_id.to_owned(),
            }
        }
        Some(_) => InboundFrame::Message(value),
        None => InboundFrame::Malformed,
    }
}

/// What the wire tells us about the turns this process ran: which one is in flight, and the last
/// assistant message of each, which is the rewind cursor a rollback needs.
///
/// `cursors` maps a turn id to the `uuid` of the last `assistant` frame it produced — the value
/// t3code passes as `resumeSessionAt` (`claudeTurnCursor`: a turn's `nativeTurnRef` is its SDK
/// assistant message uuid). `order` keeps the turn sequence, because a rollback target is named by
/// position and the map alone cannot tell which turn came first.
#[derive(Debug, Default)]
pub(crate) struct TurnTracker {
    active: Option<String>,
    order: Vec<String>,
    cursors: HashMap<String, String>,
    /// Steer uuids sent into the running turn that no result has echoed back yet.
    pending_steers: HashSet<String>,
    /// A completed segment result held while a steer is still pending.
    ///
    /// The CLI reports one `result` per steered segment, and Claude answers a steer it has not read
    /// yet as the next native turn. t3code therefore holds such a result instead of ending the turn
    /// there (`ClaudeAdapterV2.ts:12638-12649`): the run continues for the steer, and the next
    /// result settles the one turn.
    held_result: bool,
}

impl TurnTracker {
    fn begin(&mut self, turn_id: &str) {
        self.active = Some(turn_id.to_owned());
        self.order.push(turn_id.to_owned());
        self.pending_steers.clear();
        self.held_result = false;
    }

    /// Records one steer sent into the running turn, by the uuid its user line carries.
    fn steer(&mut self, uuid: &str) {
        self.pending_steers.insert(uuid.to_owned());
    }

    /// Drops every pending steer a result's prompt echo names: the CLI has read that steer.
    fn accept_steers(&mut self, echoed: &[String]) {
        for uuid in echoed {
            self.pending_steers.remove(uuid);
        }
    }

    /// Whether this completed result only ends the segment it answers while a steer is still
    /// pending. A result without a prompt echo proves nothing about a segment the steer will
    /// produce, so only an echoing result is held, and only the first one: the result after it
    /// settles the turn.
    fn hold_result(&mut self, echoed: &[String]) -> bool {
        if self.active.is_none() || self.held_result || self.pending_steers.is_empty() {
            return false;
        }
        if echoed.is_empty() {
            return false;
        }
        self.held_result = true;
        true
    }

    /// Records one assistant frame under the running turn: the newest uuid is the cursor.
    fn observe_assistant(&mut self, uuid: &str) {
        if let Some(turn_id) = self.active.clone() {
            self.cursors.insert(turn_id, uuid.to_owned());
        }
    }

    fn finish(&mut self) -> Option<String> {
        self.held_result = false;
        self.pending_steers.clear();
        self.active.take()
    }

    /// The bookkeeping to carry across a relaunch of the same session: the turn order and every
    /// rewind cursor, so a rollback issued after the relaunch still resolves.
    fn snapshot(&self) -> (Vec<String>, HashMap<String, String>) {
        (self.order.clone(), self.cursors.clone())
    }

    /// Puts a snapshot back, keeping only the turns the caller says survive — a rollback drops the
    /// tail, a plain reopen keeps all of them.
    fn restore(&mut self, kept: Vec<String>, cursors: HashMap<String, String>) {
        let survivors: HashSet<&String> = kept.iter().collect();
        self.cursors = cursors
            .into_iter()
            .filter(|(turn_id, _)| survivors.contains(turn_id))
            .collect();
        self.order = kept;
    }
}

/// The stream-json transport: one stdio connection to one child process.
///
/// Reading and writing take `&self` and lock only their own half, because the reader task holds
/// the read half across `read_line().await` for the whole session. A single connection-wide lock
/// would block every write behind that pending read.
#[async_trait::async_trait]
pub(crate) trait ClaudeTransport: Send + Sync {
    async fn send_line(&self, line: String) -> io::Result<()>;
    /// The next inbound line, or `None`/an error at EOF (the child closed stdout or exited).
    async fn read_line(&self) -> io::Result<Option<String>>;
}

/// The real transport: `claude` as an argv array, never a shell string.
pub(crate) struct ChildClaude {
    /// Held only so `kill_on_drop` can stop the process: this provider never waits on the child,
    /// the reader task notices the closed stdout instead.
    _child: std::sync::Mutex<tokio::process::Child>,
    stdin: Mutex<tokio::process::ChildStdin>,
    stdout: Mutex<BufReader<tokio::process::ChildStdout>>,
}

impl ChildClaude {
    /// Spawns the CLI from `argv` with `cwd` as its working directory.
    pub fn spawn(argv: &[String], cwd: &Path) -> io::Result<Self> {
        let mut command = tokio::process::Command::new(&argv[0]);
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
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "claude stdin closed"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "claude stdout closed"))?;
        Ok(Self {
            _child: std::sync::Mutex::new(child),
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
        })
    }
}

#[async_trait::async_trait]
impl ClaudeTransport for ChildClaude {
    async fn send_line(&self, line: String) -> io::Result<()> {
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await
    }

    async fn read_line(&self) -> io::Result<Option<String>> {
        let mut stdout = self.stdout.lock().await;
        let mut line = Vec::new();
        let read = stdout.read_until(b'\n', &mut line).await?;
        if read == 0 {
            return Ok(None);
        }
        // The SDK decodes each line with a `StringDecoder` and keeps going, so one byte the host
        // cannot decode must not end the session: read bytes and decode lossily, which leaves an
        // undecodable line to fail JSON parsing like any other malformed frame.
        Ok(Some(String::from_utf8_lossy(&line).into_owned()))
    }
}

// ---------------------------------------------------------------------------
// Launch arguments
// ---------------------------------------------------------------------------

/// The `--permission-mode` value for one Ferryx mode.
///
/// t3code's `permissionModeForClaudeRuntimePolicy` (`ClaudeAdapterV2.ts`) switches on
/// `runtimePolicy.runtimeMode` when no sandbox or approval policy narrows it:
/// `approval-required` → `default`, `auto-accept-edits` → `acceptEdits`, `auto` → `auto`,
/// `full-access` → `bypassPermissions`.
pub fn permission_mode_arg(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Supervised => "default",
        PermissionMode::AutoAcceptEdits => "acceptEdits",
        PermissionMode::Auto => "auto",
        PermissionMode::FullAccess => "bypassPermissions",
    }
}

/// Whether the CLI opens a new session or reopens a persisted one, optionally rewound to one
/// assistant message (the SDK's `resumeSessionAt`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchSession<'a> {
    New(&'a str),
    Resume(&'a str),
    ResumeAt { session_id: &'a str, at: &'a str },
}

/// The full argv that opens one session.
///
/// `--session-id` carries a client-chosen uuid, because the CLI reports its session id only on
/// `system/init`, which follows the first prompt; the store needs the thread id before that, so
/// the client picks it.
pub fn launch_argv(
    mode: PermissionMode,
    model: Option<&str>,
    effort: Option<&str>,
    session: LaunchSession<'_>,
) -> Vec<String> {
    let mut argv: Vec<String> = [
        CLAUDE_PROGRAM,
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        // The host answers `can_use_tool` over stdio; the SDK pushes exactly this for a
        // `canUseTool` callback.
        "--permission-prompt-tool=stdio",
        "--permission-mode",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    argv.push(permission_mode_arg(mode).to_owned());
    if mode == PermissionMode::FullAccess {
        // The SDK's `allowDangerouslySkipPermissions`, which `bypassPermissions` requires.
        argv.push("--allow-dangerously-skip-permissions".to_owned());
    }
    if let Some(effort) = effort {
        argv.push("--effort".to_owned());
        argv.push(effort.to_owned());
    }
    if let Some(model) = model {
        argv.push("--model".to_owned());
        argv.push(model.to_owned());
    }
    match session {
        LaunchSession::New(session_id) => {
            argv.push("--session-id".to_owned());
            argv.push(session_id.to_owned());
        }
        LaunchSession::Resume(session_id) => {
            argv.push("--resume".to_owned());
            argv.push(session_id.to_owned());
        }
        LaunchSession::ResumeAt { session_id, at } => {
            // The CLI refuses `--resume-session-at` without `--resume`.
            argv.push("--resume".to_owned());
            argv.push(session_id.to_owned());
            argv.push("--resume-session-at".to_owned());
            argv.push(at.to_owned());
        }
    }
    argv
}

/// Claude capability rows, before model discovery fills the catalog in.
///
/// `rollback` is true: the verb exists, but as a reopen with `--resume-session-at` rather than a
/// live control request (see [`StructuredProvider::rollback`]).
pub fn claude_capabilities() -> Capabilities {
    Capabilities {
        steer: true,
        rollback: true,
        // Claude has no native continuation verb in its stream-json protocol: `resume` reopens a
        // session, it does not continue a stopped run without a prompt.
        resume: false,
        // `--resume-session-at` rewinds the conversation only; the CLI has no file revert.
        revert_files: false,
        history_paging: true,
        permission_modes: PermissionMode::all().to_vec(),
        models: Vec::new(),
        slash_commands: Vec::new(),
        execution_modes: vec!["default".into(), "plan".into()],
        ..Capabilities::default()
    }
}

/// Lists the models a logged-in `claude` reports, through the shared reference-chat probe and
/// parser, plus the slash commands that probe's `system/init` record listed.
pub async fn discover_models_with_commands(
    cwd: &Path,
) -> Result<(Vec<ModelInfo>, Vec<String>), StructuredChatError> {
    use crate::remote::reference_chat::model_discovery::{
        claude_probe_stdin, parse_claude_list_models, DiscoveryError, CLAUDE_PROBE_ARGS,
        CLAUDE_PROBE_PROGRAM, DISCOVERY_PROBE_TIMEOUT, DISCOVERY_STDOUT_LIMIT,
    };
    let mut command = tokio::process::Command::new(CLAUDE_PROBE_PROGRAM);
    command
        .args(CLAUDE_PROBE_ARGS)
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    crate::util::configure_tokio_no_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| StructuredChatError::Provider(format!("claude probe failed: {error}")))?;
    if let Some(mut pipe) = child.stdin.take() {
        let stdin = claude_probe_stdin();
        let _ = pipe.write_all(stdin.as_bytes()).await;
        let _ = pipe.shutdown().await;
    }
    let read = async {
        use tokio::io::AsyncReadExt;
        let mut stdout = child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "model probe stdout closed")
        })?;
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let read = stdout.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if buffer.len() > DISCOVERY_STDOUT_LIMIT {
                break;
            }
        }
        Ok::<_, io::Error>(buffer)
    };
    let buffer = match tokio::time::timeout(DISCOVERY_PROBE_TIMEOUT, read).await {
        Ok(Ok(buffer)) => buffer,
        _ => return Err(StructuredChatError::Provider("model probe failed".into())),
    };
    let stdout = String::from_utf8_lossy(&buffer).to_string();
    let discovered = parse_claude_list_models(&stdout).map_err(|error: DiscoveryError| {
        StructuredChatError::Provider(format!("model discovery: {error:?}"))
    })?;
    let models = discovered
        .models
        .into_iter()
        .map(|model| ModelInfo {
            id: model.id,
            label: model.label,
            efforts: model.efforts,
        })
        .collect();
    // The same probe stdout carries the CLI's `system/init` slash-command catalog.
    let slash_commands = discovered
        .slash_commands
        .into_iter()
        .map(|command| command.name)
        .collect();
    Ok((models, slash_commands))
}

// ---------------------------------------------------------------------------
// Message mapping
// ---------------------------------------------------------------------------

/// Per-connection stream state: delta buffers and the ids needed to merge and finish parts.
#[derive(Debug, Default)]
pub(crate) struct StreamState {
    /// Assistant text deltas keyed by the streaming message id.
    text: HashMap<String, String>,
    /// Thinking deltas keyed by the streaming message id.
    thinking: HashMap<String, String>,
    /// The message id of the `message_start` whose deltas are arriving.
    current_message: Option<String>,
    /// Block identities already turned into parts.
    ///
    /// One assistant message arrives as several `assistant` frames that all carry the same
    /// `message.id` and a growing content list, so identity cannot be the message id: a later
    /// snapshot appends a `tool_use` the earlier ones did not carry. t3code identifies each block
    /// by its message id plus its position inside that message (`snapshotBlockIndex`,
    /// `ClaudeAdapterV2.ts:5405-5410`, ids `${message.id}:text:${index}`) and a tool call by its
    /// own `tool_use` id.
    seen_blocks: HashSet<String>,
    /// File changes by `tool_use` id, so the matching `tool_result` can close the same part.
    file_changes: HashMap<String, Vec<FileChangeEntry>>,
    /// Tool names by `tool_use` id, so a later `tool_result` can name the call it finishes.
    tool_names: HashMap<String, String>,
}

impl StreamState {
    /// Whether this content block is one no earlier snapshot of its message emitted, marking it
    /// emitted. Identity is the message id plus the block's ordinal among blocks of the same kind
    /// in that message — t3code's `snapshotBlockIndex` counts text and thinking separately and
    /// numbers them `${message.id}:${kind}:${index}` — and a tool call additionally by its own
    /// `tool_use` id, because the CLI may restate the call in a later frame.
    fn take_block(
        &mut self,
        message: Option<&str>,
        kind: &str,
        index: usize,
        call_id: Option<&str>,
    ) -> bool {
        let identity = match (message, call_id) {
            (Some(message), Some(call_id)) => format!("{message}:{kind}:{call_id}"),
            (Some(message), None) => format!("{message}:{kind}:{index}"),
            (None, Some(call_id)) => format!("{kind}:{call_id}"),
            // A frame with no message id cannot be deduplicated: emit it, as t3code does when its
            // snapshot set has no key to hold.
            (None, None) => return true,
        };
        self.seen_blocks.insert(identity)
    }
}

fn as_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn blocks(value: &Value) -> &[Value] {
    value
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Text of every `text` block in a content array.
fn joined_text(content: &[Value]) -> String {
    content
        .iter()
        .filter(|block| as_str(block, "type") == Some("text"))
        .filter_map(|block| as_str(block, "text"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The `+`/`-` diff text a file-editing tool asked for, from its own input.
fn tool_diff(tool: &str, input: &Value) -> String {
    let edit = |edit: &Value| {
        format!(
            "- {}\n+ {}",
            as_str(edit, "old_string").unwrap_or_default(),
            as_str(edit, "new_string").unwrap_or_default()
        )
    };
    match tool {
        "Edit" => edit(input),
        "Write" => as_str(input, "content")
            .unwrap_or_default()
            .lines()
            .map(|line| format!("+ {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        "MultiEdit" => input
            .get("edits")
            .and_then(Value::as_array)
            .map(|edits| edits.iter().map(edit).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// The file change one file-editing `tool_use` block makes, if it edits a file.
fn file_change_entry(tool: &str, input: &Value) -> Option<FileChangeEntry> {
    let path = as_str(input, "file_path")?.to_owned();
    Some(FileChangeEntry {
        path,
        kind: if tool == "Write" { "add" } else { "update" }.to_owned(),
        diff: tool_diff(tool, input),
    })
}

/// One `tool_result` block becomes a finished tool call, plus the terminal status of the file
/// change the same call started.
///
/// Parts are append-only, so a file change that stays `inProgress` at its `tool_use` stays that
/// way in the history forever unless the result repeats the part with its terminal status — what
/// t3code does by moving the `file_change` item to its final status when the result lands
/// (`ClaudeAdapterV2.ts:11455+`).
fn tool_result_parts(
    block: &Value,
    state: &mut StreamState,
    turn_id: Option<&str>,
) -> Vec<ProviderEvent> {
    let Some(id) = as_str(block, "tool_use_id") else {
        return Vec::new();
    };
    let failed = block
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let status = if failed {
        ToolCallStatus::Failed
    } else {
        ToolCallStatus::Completed
    };
    let output = match block.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(rows)) => joined_text(rows),
        _ => String::new(),
    };
    let name = state
        .tool_names
        .remove(id)
        .unwrap_or_else(|| "tool".to_owned());
    let mut events = vec![ProviderEvent::Part(PartDraft {
        turn_id: turn_id.map(str::to_owned),
        payload: PartKind::ToolCall {
            id: id.to_owned(),
            name,
            status: status.clone(),
            output: (!output.is_empty()).then_some(output),
            exit_code: None,
            duration_ms: None,
        },
    })];
    if let Some(changes) = state.file_changes.remove(id) {
        events.push(ProviderEvent::Part(PartDraft {
            turn_id: turn_id.map(str::to_owned),
            payload: PartKind::FileChange {
                id: id.to_owned(),
                status,
                changes,
            },
        }));
    }
    events
}

/// Maps one CLI stream message into zero or more provider events. Pure except for the buffers and
/// ids it owns, so the wire mapping is unit tested without a child process.
pub(crate) fn message_parts(
    frame: &Value,
    state: &mut StreamState,
    turn_id: Option<&str>,
) -> Vec<ProviderEvent> {
    match as_str(frame, "type") {
        Some("stream_event") => {
            let event = &frame["event"];
            match as_str(event, "type") {
                Some("message_start") => {
                    state.current_message = event
                        .pointer("/message/id")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
                Some("content_block_delta") => {
                    let delta = &event["delta"];
                    let Some(message) = state.current_message.clone() else {
                        return Vec::new();
                    };
                    match as_str(delta, "type") {
                        Some("text_delta") => {
                            if let Some(text) = as_str(delta, "text") {
                                state.text.entry(message).or_default().push_str(text);
                            }
                        }
                        Some("thinking_delta") => {
                            if let Some(text) = as_str(delta, "thinking") {
                                state.thinking.entry(message).or_default().push_str(text);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            Vec::new()
        }
        Some("assistant") => {
            // A subagent's frames belong to a sidechain the parent transcript does not show.
            if frame
                .get("parent_tool_use_id")
                .is_some_and(|value| !value.is_null())
            {
                return Vec::new();
            }
            let message = &frame["message"];
            let id = as_str(message, "id").map(str::to_owned);
            let mut events = Vec::new();
            // Per-frame ordinals: a snapshot restates the message's whole content list, so the
            // n-th text block of this frame is the same block as the n-th of the frame before.
            let mut text_index = 0usize;
            let mut thinking_index = 0usize;
            for block in blocks(message) {
                match as_str(block, "type") {
                    Some("text") => {
                        let index = text_index;
                        text_index += 1;
                        if !state.take_block(id.as_deref(), "text", index, None) {
                            continue;
                        }
                        let streamed = id
                            .as_ref()
                            .and_then(|id| state.text.remove(id))
                            .unwrap_or_default();
                        let text = as_str(block, "text")
                            .unwrap_or(streamed.as_str())
                            .to_owned();
                        if !text.is_empty() {
                            events.push(ProviderEvent::Part(PartDraft {
                                turn_id: turn_id.map(str::to_owned),
                                payload: PartKind::AssistantMessage { text },
                            }));
                        }
                    }
                    Some("thinking") => {
                        let index = thinking_index;
                        thinking_index += 1;
                        if !state.take_block(id.as_deref(), "thinking", index, None) {
                            continue;
                        }
                        let streamed = id
                            .as_ref()
                            .and_then(|id| state.thinking.remove(id))
                            .unwrap_or_default();
                        let text = as_str(block, "thinking")
                            .unwrap_or(streamed.as_str())
                            .to_owned();
                        if !text.is_empty() {
                            events.push(ProviderEvent::Part(PartDraft {
                                turn_id: turn_id.map(str::to_owned),
                                payload: PartKind::Reasoning { text },
                            }));
                        }
                    }
                    Some("tool_use") => {
                        let Some(call_id) = as_str(block, "id") else {
                            continue;
                        };
                        if !state.take_block(id.as_deref(), "tool_use", 0, Some(call_id)) {
                            continue;
                        }
                        let name = as_str(block, "name").unwrap_or("tool").to_owned();
                        let input = block.get("input").cloned().unwrap_or(Value::Null);
                        state.tool_names.insert(call_id.to_owned(), name.clone());
                        events.push(ProviderEvent::Part(PartDraft {
                            turn_id: turn_id.map(str::to_owned),
                            payload: PartKind::ToolCall {
                                id: call_id.to_owned(),
                                name: name.clone(),
                                status: ToolCallStatus::InProgress,
                                output: None,
                                exit_code: None,
                                duration_ms: None,
                            },
                        }));
                        if let Some(entry) = file_change_entry(&name, &input) {
                            state
                                .file_changes
                                .insert(call_id.to_owned(), vec![entry.clone()]);
                            events.push(ProviderEvent::Part(PartDraft {
                                turn_id: turn_id.map(str::to_owned),
                                payload: PartKind::FileChange {
                                    id: call_id.to_owned(),
                                    status: ToolCallStatus::InProgress,
                                    changes: vec![entry],
                                },
                            }));
                        }
                    }
                    _ => {}
                }
            }
            events
        }
        Some("user") => {
            let mut events = Vec::new();
            for block in blocks(&frame["message"]) {
                if as_str(block, "type") == Some("tool_result") {
                    events.extend(tool_result_parts(block, state, turn_id));
                }
            }
            events
        }
        Some("system") => match as_str(frame, "subtype") {
            Some("error") => vec![ProviderEvent::Part(PartDraft {
                turn_id: turn_id.map(str::to_owned),
                payload: PartKind::Error {
                    message: as_str(frame, "message")
                        .or_else(|| as_str(frame, "error"))
                        .unwrap_or("claude reported an error")
                        .to_owned(),
                },
            })],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// The usage part one `result` frame reports.
///
/// Field names and the arithmetic are `ClaudeTurnTokenUsage.ts`: input tokens are uncached input
/// plus cache reads plus cache creations, output is `output_tokens`, and the context window is
/// the one `modelUsage` records for the model.
pub(crate) fn usage_part(result: &Value) -> Option<PartKind> {
    let usage = result.get("usage").filter(|usage| usage.is_object())?;
    let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let uncached = count("input_tokens");
    let cached = count("cache_read_input_tokens");
    let created = count("cache_creation_input_tokens");
    let output = count("output_tokens");
    let input = uncached + cached + created;
    if input == 0 && output == 0 {
        return None;
    }
    let context_window = result
        .get("modelUsage")
        .and_then(Value::as_object)
        .and_then(|models| {
            models.values().find_map(|entry| {
                entry
                    .get("contextWindow")
                    .and_then(Value::as_u64)
                    .filter(|window| *window > 0)
            })
        });
    Some(PartKind::Usage {
        total_tokens: input + output,
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
        context_window,
    })
}

/// The failure text one `result` frame carries, when it reports a failed turn. A `success`
/// subtype can still carry `is_error: true`, which is how the CLI reports "Not logged in".
fn result_error(result: &Value) -> Option<String> {
    let subtype = as_str(result, "subtype").unwrap_or("success");
    let is_error = result
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if subtype == "success" && !is_error {
        return None;
    }
    Some(
        as_str(result, "result")
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                result
                    .get("errors")
                    .and_then(Value::as_array)
                    .and_then(|rows| rows.first())
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| format!("claude turn ended with {subtype}")),
    )
}

/// The tool calls a `result` frame's `permission_denials` rows name, as `(tool_use_id, tool_name)`.
///
/// t3code never reads this field: a refusal reaches the transcript through the ordinary
/// approval/`tool_result` path, so the call the user was shown is the call that ends declined. The
/// ids here are the ones the model actually used, which is what lets a host that sees no
/// `tool_result` close the same call rather than inventing a second one.
fn denied_calls(result: &Value) -> Vec<(String, String)> {
    result
        .get("permission_denials")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let id = as_str(row, "tool_use_id")?;
                    let name = as_str(row, "tool_name").unwrap_or("tool");
                    Some((id.to_owned(), name.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The prompt uuids a `result` frame echoes (`claudeEchoedPromptUuids`,
/// `ClaudeAdapterV2.ts:5026-5033`): `user_message_uuids`, or the single `user_message_uuid`.
fn echoed_prompt_uuids(result: &Value) -> Vec<String> {
    if let Some(rows) = result.get("user_message_uuids").and_then(Value::as_array) {
        return rows
            .iter()
            .filter_map(|uuid| uuid.as_str().map(str::to_owned))
            .collect();
    }
    as_str(result, "user_message_uuid")
        .map(|uuid| vec![uuid.to_owned()])
        .unwrap_or_default()
}

/// The `can_use_tool` answer the protocol expects (`PermissionResult` in `sdk.d.ts`).
///
/// `accept` allows this call and echoes the tool input; `acceptForSession` also writes a
/// session-scoped allow rule for the tool (`PermissionUpdate` `addRules`); `decline` refuses with
/// a message the model reads.
pub fn permission_response(decision: ApprovalDecision, tool_name: &str, input: &Value) -> Value {
    match decision {
        ApprovalDecision::Accept => json!({"behavior": "allow", "updatedInput": input}),
        ApprovalDecision::AcceptForSession => json!({
            "behavior": "allow",
            "updatedInput": input,
            "updatedPermissions": [{
                "type": "addRules",
                "rules": [{"toolName": tool_name}],
                "behavior": "allow",
                "destination": "session",
            }],
        }),
        ApprovalDecision::Decline => json!({
            "behavior": "deny",
            "message": "The user declined this tool call.",
        }),
    }
}

/// The approval part one `can_use_tool` request becomes, plus the tool facts an answer needs.
fn approval_part(
    request_id: &str,
    request: &Value,
    turn_id: Option<&str>,
) -> (PartDraft, String, Value) {
    let tool_name = as_str(request, "tool_name").unwrap_or("tool").to_owned();
    let input = request.get("input").cloned().unwrap_or(Value::Null);
    let kind = if matches!(
        tool_name.as_str(),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit"
    ) {
        "fileChange"
    } else {
        "command"
    };
    let path = as_str(&input, "file_path");
    let summary = match (path, tool_name.as_str()) {
        (Some(path), _) => format!("{tool_name} {path}"),
        (None, "Bash") => as_str(&input, "command")
            .map(str::to_owned)
            .unwrap_or_else(|| tool_name.clone()),
        _ => tool_name.clone(),
    };
    let draft = PartDraft {
        turn_id: turn_id.map(str::to_owned),
        payload: PartKind::ApprovalRequest {
            request_id: request_id.to_owned(),
            kind: kind.to_owned(),
            summary,
            reason: as_str(request, "decision_reason").map(str::to_owned),
            answered: false,
        },
    };
    (draft, tool_name, input)
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

/// One pending approval: what the answer needs to echo back.
struct PendingApproval {
    tool_name: String,
    input: Value,
}

/// Shared connection state: pending control responses and pending approvals.
struct ConnectionIo<T: ClaudeTransport> {
    transport: Arc<T>,
    next_id: Mutex<u64>,
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
    approvals: Mutex<HashMap<String, PendingApproval>>,
}

impl<T: ClaudeTransport + 'static> ConnectionIo<T> {
    fn new(transport: Arc<T>) -> Arc<Self> {
        Arc::new(Self {
            transport,
            next_id: Mutex::new(1),
            pending: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
        })
    }

    async fn next_request_id(&self) -> String {
        let mut next = self.next_id.lock().await;
        let id = format!("ferryx-{}", *next);
        *next += 1;
        id
    }

    async fn send_frame(&self, line: String) -> Result<(), StructuredChatError> {
        self.transport
            .send_line(line)
            .await
            .map_err(|error| StructuredChatError::Provider(format!("claude write failed: {error}")))
    }

    /// One host→CLI control request (`SDKControlRequest`) and its `control_response`.
    async fn control(&self, request: Value) -> Result<Value, StructuredChatError> {
        let request_id = self.next_request_id().await;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(request_id.clone(), tx);
        let frame = json!({
            "type": "control_request",
            "request_id": request_id,
            "request": request,
        });
        if let Err(error) = self.send_frame(frame.to_string()).await {
            self.pending.lock().await.remove(&request_id);
            return Err(error);
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(Ok(payload))) => Ok(payload),
            Ok(Ok(Err(message))) => Err(StructuredChatError::Provider(format!(
                "claude refused a control request: {message}"
            ))),
            Ok(Err(_)) | Err(_) => Err(StructuredChatError::Provider(
                "claude never answered a control request".into(),
            )),
        }
    }

    /// Answers one pending `can_use_tool` request with a `control_response` frame.
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
        let response = permission_response(decision, &pending.tool_name, &pending.input);
        self.send_frame(
            json!({
                "type": "control_response",
                "response": {
                    "subtype": "success",
                    "request_id": request_id,
                    "response": response,
                },
            })
            .to_string(),
        )
        .await
    }
}

/// The `stream-json` user line one turn or steer sends.
fn user_frame(text: &str, session_id: Option<&str>, uuid: &str, priority: Option<&str>) -> Value {
    let mut frame = json!({
        "type": "user",
        "message": {"role": "user", "content": [{"type": "text", "text": text}]},
        "parent_tool_use_id": Value::Null,
        "uuid": uuid,
    });
    if let Some(session_id) = session_id {
        frame["session_id"] = json!(session_id);
    }
    if let Some(priority) = priority {
        frame["priority"] = json!(priority);
    }
    frame
}

/// The turn text plus the attachment paths, which the CLI reads with its own file tools.
fn turn_text(text: &str, attachments: &[String]) -> String {
    if attachments.is_empty() {
        return text.to_owned();
    }
    let mut text = text.to_owned();
    text.push_str("\n\nAttached files:\n");
    for path in attachments {
        text.push_str("- ");
        text.push_str(path);
        text.push('\n');
    }
    text
}

/// Runs the reader loop: classify each line, resolve control responses, publish parts, hold
/// approval requests for the routes, and end the session when the child is gone.
async fn run_reader<T: ClaudeTransport + 'static>(
    io: Arc<ConnectionIo<T>>,
    turns: Arc<Mutex<TurnTracker>>,
    events: mpsc::UnboundedSender<ProviderEvent>,
    mut state: StreamState,
) {
    loop {
        let line = match io.transport.read_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match classify_frame(trimmed) {
            InboundFrame::Malformed => continue,
            InboundFrame::ControlResponse { request_id, result } => {
                if let Some(sender) = io.pending.lock().await.remove(&request_id) {
                    let _ = sender.send(result);
                }
            }
            InboundFrame::ControlCancelled { request_id } => {
                let withdrawn = io.approvals.lock().await.remove(&request_id);
                if let Some(approval) = withdrawn {
                    let _ = events.send(ProviderEvent::Part(PartDraft {
                        turn_id: None,
                        payload: PartKind::ApprovalRequest {
                            request_id,
                            kind: "command".into(),
                            summary: approval.tool_name,
                            reason: Some("the CLI withdrew this request".into()),
                            answered: true,
                        },
                    }));
                }
            }
            InboundFrame::ControlRequest { request_id, request } => {
                if as_str(&request, "subtype") != Some("can_use_tool") {
                    // A dialog this host cannot render would hang the CLI: refuse it explicitly.
                    let _ = io
                        .send_frame(
                            json!({
                                "type": "control_response",
                                "response": {
                                    "subtype": "error",
                                    "request_id": request_id,
                                    "error": "this host renders no such dialog",
                                },
                            })
                            .to_string(),
                        )
                        .await;
                    continue;
                }
                let turn_id = turns.lock().await.active.clone();
                let (draft, tool_name, input) =
                    approval_part(&request_id, &request, turn_id.as_deref());
                io.approvals
                    .lock()
                    .await
                    .insert(request_id, PendingApproval { tool_name, input });
                if events.send(ProviderEvent::Part(draft)).is_err() {
                    return;
                }
            }
            InboundFrame::Message(frame) => {
                let turn_id = {
                    let mut tracker = turns.lock().await;
                    let turn_id = tracker.active.clone();
                    let main_thread = frame
                        .get("parent_tool_use_id")
                        .is_none_or(Value::is_null);
                    // An assistant frame's uuid is the rewind cursor a later rollback needs.
                    if main_thread && as_str(&frame, "type") == Some("assistant") {
                        if let Some(uuid) = as_str(&frame, "uuid") {
                            tracker.observe_assistant(uuid);
                        }
                    }
                    turn_id
                };
                for event in message_parts(&frame, &mut state, turn_id.as_deref()) {
                    if events.send(event).is_err() {
                        return;
                    }
                }
                if as_str(&frame, "type") != Some("result") {
                    continue;
                }
                if let Some(payload) = usage_part(&frame) {
                    let _ = events.send(ProviderEvent::Part(PartDraft {
                        turn_id: turn_id.clone(),
                        payload,
                    }));
                }
                // A denial names the call the model made; t3code never reads
                // `permission_denials` and closes the call through the ordinary result path, so
                // the honest equivalent here is to end that very call rather than invent a
                // second one the model never issued.
                let denied = denied_calls(&frame);
                if !denied.is_empty() {
                    for (call_id, tool_name) in denied {
                        let name = state.tool_names.remove(&call_id).unwrap_or(tool_name);
                        state.file_changes.remove(&call_id);
                        let _ = events.send(ProviderEvent::Part(PartDraft {
                            turn_id: turn_id.clone(),
                            payload: PartKind::ToolCall {
                                id: call_id,
                                name,
                                status: ToolCallStatus::Declined,
                                output: None,
                                exit_code: None,
                                duration_ms: None,
                            },
                        }));
                    }
                }
                let error = result_error(&frame);
                let echoed = echoed_prompt_uuids(&frame);
                {
                    let mut tracker = turns.lock().await;
                    tracker.accept_steers(&echoed);
                    // A completed segment result that echoes a prompt, while a steer is still
                    // pending, ends only that segment: the steer's own result ends the turn
                    // (`ClaudeAdapterV2.ts:12638-12649`). Holding one result is enough — the next
                    // result settles the turn — so the turn finishes exactly once.
                    if error.is_none() && tracker.hold_result(&echoed) {
                        continue;
                    }
                }
                let finished = turns.lock().await.finish();
                let Some(finished) = finished else {
                    // A result with no turn in flight (a replay after a relaunch) completes
                    // nothing; reporting it would end a turn the store never saw begin.
                    continue;
                };
                let _ = events.send(ProviderEvent::TurnCompleted {
                    turn_id: finished,
                    error,
                });
            }
        }
    }
    // The child is gone: fail every in-flight control request now, forget approvals, and end the
    // session so the store stops accepting turns against a dead process. A turn that was running
    // fails with it — t3code maps a killed or non-zero-exiting provider process to a failed turn
    // (`ClaudeAdapterV2.ts:1956`), and a turn silently ending as if it had finished is a lie the
    // journal would keep.
    for (_, sender) in io.pending.lock().await.drain() {
        let _ = sender.send(Err("claude closed".into()));
    }
    io.approvals.lock().await.clear();
    let interrupted = turns.lock().await.finish();
    if let Some(turn_id) = interrupted {
        let _ = events.send(ProviderEvent::TurnCompleted {
            turn_id,
            error: Some("claude exited before the turn finished".into()),
        });
    }
    let _ = events.send(ProviderEvent::Ended {
        reason: "claude closed its stdout".into(),
    });
}

/// How a provider opens one transport. Production spawns [`ChildClaude`]; tests pass an
/// in-process duplex so the wire mapping runs without a child process.
pub(crate) type Launcher<T> = Arc<dyn Fn(&[String], &Path) -> io::Result<Arc<T>> + Send + Sync>;

/// The arguments and cwd one session opens with, kept so later turns can reopen it.
#[derive(Debug, Clone)]
struct LaunchPlan {
    cwd: PathBuf,
    mode: PermissionMode,
    model: Option<String>,
    effort: Option<String>,
}

impl LaunchPlan {
    /// argv for a fresh thread, a plain resume, or a resume rewound to one assistant message.
    fn argv(&self, resume: Option<(&str, Option<&str>)>) -> Vec<String> {
        let session = match resume {
            Some((session_id, Some(at))) => LaunchSession::ResumeAt { session_id, at },
            Some((session_id, None)) => LaunchSession::Resume(session_id),
            None => {
                // The id is chosen here because `system/init`, the only frame that reports it,
                // arrives after the first prompt, and the store needs it before then.
                let generated = uuid::Uuid::new_v4().to_string();
                return launch_argv(
                    self.mode,
                    self.model.as_deref(),
                    self.effort.as_deref(),
                    LaunchSession::New(&generated),
                );
            }
        };
        launch_argv(
            self.mode,
            self.model.as_deref(),
            self.effort.as_deref(),
            session,
        )
    }
}

/// The Claude implementation of [`StructuredProvider`]. One instance owns one CLI child.
///
/// Crate-visible only: the store names it in `spawn_provider`, and nothing outside the gateway
/// constructs a provider directly. The generic parameter is the transport seam tests script,
/// defaulting to the real child process.
pub(crate) struct ClaudeProvider<T: ClaudeTransport + 'static = ChildClaude> {
    plan: Mutex<LaunchPlan>,
    /// The open connection, once [`StructuredProvider::start`] has spawned the CLI.
    io: Mutex<Option<Arc<ConnectionIo<T>>>>,
    /// The session id this provider last opened with.
    session: Mutex<Option<String>>,
    /// Shared with the reader, which attributes every part to the turn in flight and records the
    /// assistant cursors a rollback rewinds to.
    turns: Arc<Mutex<TurnTracker>>,
    reader: Mutex<Option<tokio::task::JoinHandle<()>>>,
    events_tx: mpsc::UnboundedSender<ProviderEvent>,
    /// Test seam: how to open a transport. Production always spawns [`ChildClaude`].
    launch: Launcher<T>,
}

impl ClaudeProvider<ChildClaude> {
    /// Opens a provider for `cwd`; the CLI itself starts on [`StructuredProvider::start`],
    /// because the resume flag is part of its argv.
    pub fn spawn(
        cwd: PathBuf,
        events: mpsc::UnboundedSender<ProviderEvent>,
        settings: &SessionSettings,
    ) -> Self {
        Self::with_launcher(
            cwd,
            events,
            settings,
            Arc::new(|argv: &[String], cwd: &Path| ChildClaude::spawn(argv, cwd).map(Arc::new)),
        )
    }
}

impl<T: ClaudeTransport + 'static> ClaudeProvider<T> {
    /// Builds a provider from an explicit launcher; tests pass an in-process duplex here.
    pub fn with_launcher(
        cwd: PathBuf,
        events: mpsc::UnboundedSender<ProviderEvent>,
        settings: &SessionSettings,
        launch: Launcher<T>,
    ) -> Self {
        Self {
            plan: Mutex::new(LaunchPlan {
                cwd,
                mode: settings.permission_mode.unwrap_or(PermissionMode::FullAccess),
                model: settings.model.clone(),
                effort: settings.effort.clone(),
            }),
            io: Mutex::new(None),
            session: Mutex::new(None),
            turns: Arc::new(Mutex::new(TurnTracker::default())),
            reader: Mutex::new(None),
            events_tx: events,
            launch,
        }
    }

    async fn io(&self) -> Result<Arc<ConnectionIo<T>>, StructuredChatError> {
        self.io.lock().await.clone().ok_or_else(|| {
            StructuredChatError::Conflict("SESSION_NOT_STARTED: no claude process is open".into())
        })
    }

    /// Opens the CLI child and starts its reader. `resume` names the persisted session and,
    /// when it carries a second element, the assistant message to rewind to.
    async fn open(
        &self,
        resume: Option<(&str, Option<&str>)>,
    ) -> Result<String, StructuredChatError> {
        let argv = self.plan.lock().await.argv(resume);
        let cwd = self.plan.lock().await.cwd.clone();
        let transport = (self.launch)(&argv, &cwd).map_err(|error| {
            StructuredChatError::Provider(format!("claude spawn failed: {error}"))
        })?;
        let session_id = match resume {
            Some((session_id, _)) => session_id.to_owned(),
            // `LaunchPlan::argv` generated the uuid it passed to `--session-id`; read it back.
            None => argv
                .iter()
                .position(|arg| arg == "--session-id")
                .and_then(|index| argv.get(index + 1))
                .cloned()
                .unwrap_or_default(),
        };
        if session_id.is_empty() {
            return Err(StructuredChatError::Provider(
                "claude argv lost its session id".into(),
            ));
        }
        let io = ConnectionIo::new(transport);
        *self.reader.lock().await = Some(tokio::spawn(run_reader(
            Arc::clone(&io),
            Arc::clone(&self.turns),
            self.events_tx.clone(),
            StreamState::default(),
        )));
        *self.io.lock().await = Some(io);
        *self.session.lock().await = Some(session_id.clone());
        Ok(session_id)
    }

    /// Kills the running child and forgets the connection.
    async fn shutdown(&self) {
        if let Some(handle) = self.reader.lock().await.take() {
            handle.abort();
        }
        *self.io.lock().await = None;
        *self.turns.lock().await = TurnTracker::default();
        *self.session.lock().await = None;
    }

    /// Reopens the same persisted session, keeping the turn bookkeeping so a later rollback can
    /// still resolve a cursor. `shutdown` drops the tracker, hence the snapshot.
    async fn reopen_keeping_turns(&self, session_id: &str) -> Result<(), StructuredChatError> {
        let (order, cursors) = self.turns.lock().await.snapshot();
        self.shutdown().await;
        self.open(Some((session_id, None))).await?;
        self.turns.lock().await.restore(order, cursors);
        Ok(())
    }
}

#[async_trait::async_trait]
impl<T: ClaudeTransport + 'static> StructuredProvider for ClaudeProvider<T> {
    fn provider_id(&self) -> &'static str {
        "claude"
    }

    fn capabilities(&self) -> Capabilities {
        claude_capabilities()
    }

    /// Opens the CLI: `--resume <session>` for a persisted thread, `--session-id <uuid>` for a
    /// fresh one. The id is known before the CLI reports anything, because `system/init` only
    /// follows the first prompt.
    async fn start(&self, resume_thread: Option<String>) -> Result<String, StructuredChatError> {
        self.open(resume_thread.as_deref().map(|id| (id, None))).await
    }

    /// Sends one user line.
    ///
    /// Settings are applied the way t3code applies them: the model is a live control request
    /// (`set_model`) and effort goes through `apply_flag_settings` — except clearing it, which
    /// t3code reopens the process for, because only a relaunch drops back to the user's configured
    /// default (`canApplyClaudeSelectionLive` in `ClaudeAdapterV2.ts`). A permission-mode change
    /// is also a relaunch: the CLI refuses to enter `bypassPermissions` live on a process it was
    /// not launched for, and t3code's query policy key includes the mode.
    async fn turn(
        &self,
        text: String,
        attachments: Vec<String>,
        settings: &SessionSettings,
    ) -> Result<TurnRef, StructuredChatError> {
        if self.session.lock().await.is_none() {
            return Err(StructuredChatError::Conflict(
                "SESSION_NOT_STARTED: start the session before sending a turn".into(),
            ));
        }
        // A permission-mode change is handled first because it is the one setting applied by
        // reopening the process: anything sent live to the old child would be thrown away with it.
        // A changed model or effort rides the relaunch's argv too, so the live paths below then
        // see nothing to do.
        let mode = settings.permission_mode.unwrap_or(PermissionMode::FullAccess);
        let mode_changed = {
            let plan = self.plan.lock().await;
            plan.mode != mode
        };
        if mode_changed {
            // The CLI refuses `set_permission_mode {"mode":"bypassPermissions"}` on a process
            // launched without `--allow-dangerously-skip-permissions` (`bypass_not_launched`,
            // measured on 2.1.309), and t3code keys process reuse on the permission mode
            // (`claudeRuntimeQueryPolicyKey`, `ClaudeAdapterV2.ts:3918-3926`): a mode change is a
            // relaunch carrying the new launch flags, never a live switch. The thread is resumed,
            // so the conversation survives the new process.
            let session_id = self.session.lock().await.clone().unwrap_or_default();
            {
                let mut plan = self.plan.lock().await;
                plan.mode = mode;
                plan.model = settings.model.clone();
                plan.effort = settings.effort.clone();
            }
            self.reopen_keeping_turns(&session_id).await?;
        }
        let effort_changed = {
            let plan = self.plan.lock().await;
            plan.effort != settings.effort
        };
        if effort_changed {
            let session_id = self.session.lock().await.clone().unwrap_or_default();
            match settings.effort.clone() {
                Some(level) => {
                    let io = self.io().await?;
                    io.control(json!({
                        "subtype": "apply_flag_settings",
                        "settings": {"effortLevel": level},
                    }))
                    .await?;
                    self.plan.lock().await.effort = Some(level);
                }
                None => {
                    self.plan.lock().await.effort = None;
                    self.reopen_keeping_turns(&session_id).await?;
                }
            }
        }
        let io = self.io().await?;
        let model_changed = {
            let plan = self.plan.lock().await;
            plan.model != settings.model
        };
        if model_changed {
            // Omitting `model` resets to the session default (`SDKControlSetModelRequest`).
            let request = match &settings.model {
                Some(model) => json!({"subtype": "set_model", "model": model}),
                None => json!({"subtype": "set_model"}),
            };
            io.control(request).await?;
            self.plan.lock().await.model = settings.model.clone();
        }
        let turn_id = uuid::Uuid::new_v4().to_string();
        let session_id = self.session.lock().await.clone();
        let frame = user_frame(
            &turn_text(&text, &attachments),
            session_id.as_deref(),
            &turn_id,
            None,
        );
        // Claim the turn before the line leaves: the reader can report the first frame of this
        // turn as soon as the write lands, and a part attributed to the previous turn cannot be
        // corrected afterwards.
        self.turns.lock().await.begin(&turn_id);
        if let Err(error) = io.send_frame(frame.to_string()).await {
            self.turns.lock().await.finish();
            return Err(error);
        }
        Ok(TurnRef { turn_id })
    }

    /// Steers the running turn: a further `user` line at `priority: "next"`, which Claude folds
    /// into the turn in flight once the tool calls it is running finish — the behavior t3code's
    /// `steerTurn` relies on (`ClaudeAdapterV2.ts`: `// "now" makes Claude Code cancel the tool
    /// calls in flight and report them as a user refusal.`).
    async fn steer(&self, expected_turn: &str, text: String) -> Result<(), StructuredChatError> {
        let active = self.turns.lock().await.active.clone();
        if active.as_deref() != Some(expected_turn) {
            return Err(StructuredChatError::Conflict(
                "NO_ACTIVE_TURN: that turn is not running".into(),
            ));
        }
        let io = self.io().await?;
        let session_id = self.session.lock().await.clone();
        let uuid = uuid::Uuid::new_v4().to_string();
        // The uuid is the steer's identity: a result echoing it proves the CLI read this steer, and
        // until one does, a completed segment result is held rather than ending the turn.
        self.turns.lock().await.steer(&uuid);
        io.send_frame(user_frame(&text, session_id.as_deref(), &uuid, Some("next")).to_string())
            .await
    }

    /// Interrupts the running turn with the `interrupt` control request
    /// (`SDKControlInterruptRequest`).
    async fn interrupt(&self, _turn: &str) -> Result<(), StructuredChatError> {
        let io = self.io().await?;
        io.control(json!({"subtype": "interrupt"})).await?;
        Ok(())
    }

    /// Drops the turns from `before_turn` on by reopening the CLI with
    /// `--resume <session> --resume-session-at <cursor>`, the cursor being the uuid of the last
    /// assistant message of the turn before the target — t3code's `rollbackThread`
    /// (`ClaudeAdapterV2.ts`): it refuses while a turn runs, closes the live query, resolves
    /// `resumeSessionAt` from the kept turn's SDK assistant uuid, and reopens there.
    ///
    /// Rewinding past the first turn has no cursor. t3code allocates a fresh native thread id for
    /// that case (`ClaudeAdapterV2.ts:15403-15420`), and so does this: a new `--session-id`, whose
    /// id is returned so the store can persist it. When a turn does survive but the CLI recorded no
    /// cursor for it, t3code refuses rather than silently dropping the conversation
    /// (`resolveClaudeRollbackResumeSessionAt`), and so does this.
    ///
    /// A cursor the CLI refuses is a deterministic refusal of the persisted transcript: it is
    /// reported once as a provider failure and never retried.
    async fn rollback(&self, before_turn: &str) -> Result<String, StructuredChatError> {
        let (session_id, cursor, kept, cursors) = {
            let turns = self.turns.lock().await;
            if turns.active.is_some() {
                return Err(StructuredChatError::Conflict(
                    "TURN_ACTIVE: cannot roll back while a turn is running".into(),
                ));
            }
            let index = turns
                .order
                .iter()
                .position(|turn_id| turn_id == before_turn)
                .ok_or_else(|| {
                    StructuredChatError::Conflict(format!(
                        "ROLLBACK_UNKNOWN_TURN: {before_turn} is not a turn of this process"
                    ))
                })?;
            // Everything before the target survives; the target's predecessor is the cursor.
            let kept = turns.order[..index].to_vec();
            let cursor = kept
                .last()
                .and_then(|turn_id| turns.cursors.get(turn_id))
                .cloned();
            if cursor.is_none() && !kept.is_empty() {
                // A turn survives but the CLI never reported an assistant cursor for it, so there
                // is no point in its transcript to rewind to. Opening a fresh session here would
                // silently discard the turns the caller asked to keep.
                return Err(StructuredChatError::Conflict(
                    "ROLLBACK_NO_CURSOR: the kept turn has no assistant message to rewind to"
                        .into(),
                ));
            }
            let (_, cursors) = turns.snapshot();
            (self.session.lock().await.clone(), cursor, kept, cursors)
        };
        self.shutdown().await;
        let reopened = match (session_id, cursor) {
            (Some(session_id), Some(cursor)) => {
                self.open(Some((&session_id, Some(&cursor)))).await?
            }
            // No turn survives the rewind: t3code opens a fresh native thread, and its id is the
            // one the store must now persist.
            _ => self.open(None).await?,
        };
        self.turns.lock().await.restore(kept, cursors);
        Ok(reopened)
    }

    async fn answer_approval(
        &self,
        request_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), StructuredChatError> {
        self.io().await?.answer_approval(request_id, decision).await
    }

    /// `/clear`: a fresh CLI process with a new `--session-id`, so the next turn starts from an
    /// empty history. The store appends the `newSession` divider part.
    async fn clear(&self) -> Result<String, StructuredChatError> {
        self.shutdown().await;
        self.open(None).await
    }

    /// `/compact`: the CLI's own slash command, sent as a user line (t3code's
    /// `compactThread` does the same: `startTurn` with the text `"/compact"`).
    async fn compact(&self) -> Result<(), StructuredChatError> {
        let io = self.io().await?;
        let session_id = self.session.lock().await.clone();
        let uuid = uuid::Uuid::new_v4().to_string();
        io.send_frame(user_frame("/compact", session_id.as_deref(), &uuid, None).to_string())
            .await
    }

    async fn close(&self) {
        self.shutdown().await;
    }

    fn event_sink(&self) -> mpsc::UnboundedSender<ProviderEvent> {
        self.events_tx.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests. The fake transport is an in-process duplex: tests script the inbound
// lines and read back exactly what the provider wrote. Every control request
// the provider sends is answered by the fake, the way a cooperative CLI does.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    /// One scripted stdio child.
    ///
    /// The queue is a deque rather than a channel on purpose: a channel would leave a sender
    /// inside the fake, so the harness could never close it and the reader would never see EOF.
    struct FakeClaude {
        inbound: std::sync::Mutex<VecDeque<String>>,
        closed: AtomicBool,
        wake: Notify,
        written: mpsc::UnboundedSender<String>,
    }

    impl FakeClaude {
        fn close(&self) {
            self.closed.store(true, Ordering::SeqCst);
            self.wake.notify_one();
        }
    }

    #[async_trait::async_trait]
    impl ClaudeTransport for FakeClaude {
        async fn send_line(&self, line: String) -> io::Result<()> {
            // Answer our own control requests at once, so no test waits on a response.
            if let Ok(Value::Object(frame)) = serde_json::from_str::<Value>(&line) {
                if frame.get("type").and_then(Value::as_str) == Some("control_request") {
                    let reply = json!({
                        "type": "control_response",
                        "response": {
                            "subtype": "success",
                            "request_id": frame.get("request_id").cloned().unwrap_or(Value::Null),
                            "response": {},
                        },
                    });
                    self.inbound
                        .lock()
                        .unwrap()
                        .push_back(reply.to_string());
                    self.wake.notify_one();
                }
            }
            let _ = self.written.send(line);
            Ok(())
        }

        async fn read_line(&self) -> io::Result<Option<String>> {
            loop {
                // Subscribe before looking, so a line pushed between the look and the wait is not
                // lost: `Notify` keeps the permit for the next waiter.
                let wake = self.wake.notified();
                {
                    let mut inbound = self.inbound.lock().unwrap();
                    if let Some(line) = inbound.pop_front() {
                        return Ok(Some(line));
                    }
                    if self.closed.load(Ordering::SeqCst) {
                        return Ok(None);
                    }
                }
                wake.await;
            }
        }
    }

    /// One spawned child: how to script it, and what it was sent.
    struct Link {
        child: Arc<FakeClaude>,
        written: mpsc::UnboundedReceiver<String>,
    }

    struct Harness {
        provider: ClaudeProvider<FakeClaude>,
        events: mpsc::UnboundedReceiver<ProviderEvent>,
        links: Arc<std::sync::Mutex<Vec<Link>>>,
        argvs: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    }

    impl Harness {
        fn new(settings: SessionSettings) -> Self {
            let links: Arc<std::sync::Mutex<Vec<Link>>> =
                Arc::new(std::sync::Mutex::new(Vec::new()));
            let argvs: Arc<std::sync::Mutex<Vec<Vec<String>>>> =
                Arc::new(std::sync::Mutex::new(Vec::new()));
            let recorded_links = Arc::clone(&links);
            let recorded_argvs = Arc::clone(&argvs);
            let launch: Launcher<FakeClaude> = Arc::new(move |argv, _cwd| {
                let (written_tx, written_rx) = mpsc::unbounded_channel();
                recorded_argvs.lock().unwrap().push(argv.to_vec());
                let child = Arc::new(FakeClaude {
                    inbound: std::sync::Mutex::new(VecDeque::new()),
                    closed: AtomicBool::new(false),
                    wake: Notify::new(),
                    written: written_tx,
                });
                recorded_links.lock().unwrap().push(Link {
                    child: Arc::clone(&child),
                    written: written_rx,
                });
                Ok::<_, io::Error>(child)
            });
            let (events_tx, events_rx) = mpsc::unbounded_channel();
            let provider =
                ClaudeProvider::with_launcher(PathBuf::from("/tmp"), events_tx, &settings, launch);
            Self {
                provider,
                events: events_rx,
                links,
                argvs,
            }
        }

        /// Scripts the newest child with the given inbound lines.
        fn script(&self, lines: Vec<String>) {
            let links = self.links.lock().unwrap();
            let link = links.last().expect("a child was spawned");
            {
                let mut inbound = link.child.inbound.lock().unwrap();
                for line in lines {
                    inbound.push_back(line);
                }
            }
            link.child.wake.notify_one();
        }

        /// Closes the newest child's stdout, which is how a real child exiting appears.
        fn end_child(&self) {
            let link = self.links.lock().unwrap().pop().expect("a child was spawned");
            link.child.close();
        }

        /// Everything the newest child was sent, parsed as JSON.
        fn outbound(&self) -> Vec<Value> {
            let mut links = self.links.lock().unwrap();
            let link = links.last_mut().expect("a child was spawned");
            let mut frames = Vec::new();
            while let Ok(line) = link.written.try_recv() {
                frames.push(serde_json::from_str(&line).expect("outbound frame is JSON"));
            }
            frames
        }

        fn argvs(&self) -> Vec<Vec<String>> {
            self.argvs.lock().unwrap().clone()
        }

        fn last_argv(&self) -> Vec<String> {
            self.argvs().pop().expect("a child was spawned")
        }

        /// The next part event, skipping lifecycle events.
        async fn next_part(&mut self) -> PartDraft {
            loop {
                if let ProviderEvent::Part(draft) = self.next_event().await {
                    return draft;
                }
            }
        }

        /// The next turn-completed event.
        async fn next_completed(&mut self) -> (String, Option<String>) {
            loop {
                if let ProviderEvent::TurnCompleted { turn_id, error } = self.next_event().await {
                    return (turn_id, error);
                }
            }
        }

        async fn next_event(&mut self) -> ProviderEvent {
            tokio::time::timeout(Duration::from_secs(5), self.events.recv())
                .await
                .expect("the provider reports an event")
                .expect("the event channel stays open")
        }
    }

    fn supervised() -> SessionSettings {
        SessionSettings {
            model: Some("claude-sonnet-4-6".into()),
            effort: None,
            permission_mode: Some(PermissionMode::Supervised),
            ..SessionSettings::default()
        }
    }

    fn init_frame(session_id: &str) -> String {
        json!({
            "type": "system",
            "subtype": "init",
            "session_id": session_id,
            "model": "claude-sonnet-4-6",
            "slash_commands": ["compact", "clear"],
        })
        .to_string()
    }

    fn assistant_frame(message_id: &str, text: &str, uuid: &str) -> String {
        json!({
            "type": "assistant",
            "message": {"id": message_id, "content": [{"type": "text", "text": text}]},
            "uuid": uuid,
        })
        .to_string()
    }

    fn result_frame(text: &str) -> String {
        json!({
            "type": "result", "subtype": "success", "is_error": false, "result": text,
            "usage": {"input_tokens": 3, "output_tokens": 2},
        })
        .to_string()
    }

    #[test]
    fn permission_modes_map_to_the_cli_spellings() {
        assert_eq!(permission_mode_arg(PermissionMode::Supervised), "default");
        assert_eq!(
            permission_mode_arg(PermissionMode::AutoAcceptEdits),
            "acceptEdits"
        );
        assert_eq!(permission_mode_arg(PermissionMode::Auto), "auto");
        assert_eq!(
            permission_mode_arg(PermissionMode::FullAccess),
            "bypassPermissions"
        );
    }

    #[test]
    fn launch_argv_carries_the_stream_json_protocol_flags() {
        let argv = launch_argv(
            PermissionMode::FullAccess,
            Some("claude-opus-4-7"),
            Some("high"),
            LaunchSession::New("11111111-2222-3333-4444-555555555555"),
        );
        assert_eq!(argv[0], "claude");
        for flag in [
            "-p",
            "--input-format",
            "--output-format",
            "--verbose",
            "--include-partial-messages",
            "--permission-prompt-tool=stdio",
        ] {
            assert!(argv.iter().any(|arg| arg == flag), "{flag} missing");
        }
        let after = |flag: &str| argv[argv.iter().position(|arg| arg == flag).unwrap() + 1].clone();
        assert_eq!(after("--permission-mode"), "bypassPermissions");
        assert!(argv
            .iter()
            .any(|arg| arg == "--allow-dangerously-skip-permissions"));
        assert_eq!(after("--effort"), "high");
        assert_eq!(after("--model"), "claude-opus-4-7");
        assert_eq!(after("--session-id"), "11111111-2222-3333-4444-555555555555");
    }

    #[test]
    fn resume_argv_names_the_persisted_session() {
        let argv = launch_argv(
            PermissionMode::Supervised,
            None,
            None,
            LaunchSession::Resume("session-1"),
        );
        let after = |flag: &str| argv[argv.iter().position(|arg| arg == flag).unwrap() + 1].clone();
        assert_eq!(after("--resume"), "session-1");
        assert!(!argv.iter().any(|arg| arg == "--session-id"));
        assert!(!argv
            .iter()
            .any(|arg| arg == "--allow-dangerously-skip-permissions"));
        assert!(!argv.iter().any(|arg| arg == "--resume-session-at"));
    }

    #[test]
    fn resume_at_argv_rewinds_inside_the_persisted_session() {
        let argv = launch_argv(
            PermissionMode::Supervised,
            None,
            None,
            LaunchSession::ResumeAt {
                session_id: "session-1",
                at: "uuid-1",
            },
        );
        let after = |flag: &str| argv[argv.iter().position(|arg| arg == flag).unwrap() + 1].clone();
        assert_eq!(after("--resume"), "session-1");
        assert_eq!(after("--resume-session-at"), "uuid-1");
        assert!(!argv.iter().any(|arg| arg == "--session-id"));
    }

    #[tokio::test]
    async fn start_returns_the_uuid_it_launched_with() {
        let harness = Harness::new(supervised());
        let session_id = harness.provider.start(None).await.expect("start");
        assert_eq!(session_id.len(), 36, "a uuid");
        let argv = harness.last_argv();
        let named = argv[argv.iter().position(|arg| arg == "--session-id").unwrap() + 1].clone();
        assert_eq!(named, session_id);
    }

    #[tokio::test]
    async fn resume_launches_with_resume_and_keeps_the_thread_id() {
        let harness = Harness::new(supervised());
        let session_id = harness
            .provider
            .start(Some("persisted".into()))
            .await
            .expect("resume");
        assert_eq!(session_id, "persisted");
        let argv = harness.last_argv();
        assert!(argv.iter().any(|arg| arg == "--resume"));
        assert!(!argv.iter().any(|arg| arg == "--session-id"));
    }

    #[tokio::test]
    async fn streamed_deltas_merge_into_one_assistant_part() {
        let mut harness = Harness::new(supervised());
        let session_id = harness.provider.start(None).await.unwrap();
        harness
            .provider
            .turn("hi".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.script(vec![
            init_frame(&session_id),
            json!({"type": "stream_event", "event": {"type": "message_start",
                "message": {"id": "m1"}}})
            .to_string(),
            json!({"type": "stream_event", "event": {"type": "content_block_delta",
                "delta": {"type": "text_delta", "text": "Hel"}}})
            .to_string(),
            json!({"type": "stream_event", "event": {"type": "content_block_delta",
                "delta": {"type": "text_delta", "text": "lo"}}})
            .to_string(),
            assistant_frame("m1", "Hello", "u1"),
        ]);
        let part = harness.next_part().await;
        assert_eq!(
            part.payload,
            PartKind::AssistantMessage {
                text: "Hello".into()
            }
        );
        // The deltas were consumed by that one assistant message, not replayed as three parts.
        harness.script(vec![assistant_frame("m1", "Hello", "u1")]);
        harness
            .provider
            .turn("again".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.script(vec![result_frame("done")]);
        let part = harness.next_part().await;
        assert!(matches!(part.payload, PartKind::Usage { .. }));
    }

    #[tokio::test]
    async fn delta_buffer_is_the_fallback_when_the_block_carries_no_text() {
        let mut state = StreamState::default();
        for text in ["weigh", "ing"] {
            let events = message_parts(
                &json!({"type": "stream_event", "event": {"type": "message_start",
                    "message": {"id": "m1"}}}),
                &mut state,
                Some("t1"),
            );
            assert!(events.is_empty());
            message_parts(
                &json!({"type": "stream_event", "event": {"type": "content_block_delta",
                    "delta": {"type": "text_delta", "text": text}}}),
                &mut state,
                Some("t1"),
            );
        }
        let events = message_parts(
            &json!({"type": "assistant", "message": {"id": "m1", "content": [
                {"type": "text"}]}}),
            &mut state,
            Some("t1"),
        );
        assert_eq!(
            events,
            vec![ProviderEvent::Part(PartDraft {
                turn_id: Some("t1".into()),
                payload: PartKind::AssistantMessage {
                    text: "weighing".into()
                },
            })]
        );
    }

    #[test]
    fn thinking_becomes_a_reasoning_part() {
        let mut state = StreamState::default();
        let events = message_parts(
            &json!({"type": "assistant", "message": {"id": "m1", "content": [
                {"type": "thinking", "thinking": "weighing"}]}}),
            &mut state,
            Some("t1"),
        );
        assert!(matches!(
            &events[0],
            ProviderEvent::Part(PartDraft {
                payload: PartKind::Reasoning { text }, ..
            }) if text == "weighing"
        ));
    }

    #[tokio::test]
    async fn tool_use_and_tool_result_map_to_a_tool_call_and_a_file_change() {
        let mut state = StreamState::default();
        let events = message_parts(
            &json!({"type": "assistant", "message": {"id": "m1", "content": [
                {"type": "tool_use", "id": "tu1", "name": "Edit",
                 "input": {"file_path": "/tmp/a.rs", "old_string": "a", "new_string": "b"}}]}}),
            &mut state,
            Some("t1"),
        );
        assert!(matches!(
            &events[0],
            ProviderEvent::Part(PartDraft {
                payload: PartKind::ToolCall { name, status, .. }, ..
            }) if name == "Edit" && *status == ToolCallStatus::InProgress
        ));
        assert!(matches!(
            &events[1],
            ProviderEvent::Part(PartDraft {
                payload: PartKind::FileChange { changes, .. },
                ..
            }) if changes[0].path == "/tmp/a.rs"
                && changes[0].kind == "update"
                && changes[0].diff == "- a\n+ b"
        ));

        let events = message_parts(
            &json!({"type": "user", "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "tu1", "content": "edited"}]}}),
            &mut state,
            Some("t1"),
        );
        assert!(matches!(
            &events[0],
            ProviderEvent::Part(PartDraft {
                payload: PartKind::ToolCall { name, status, output, .. }, ..
            }) if name == "Edit" && *status == ToolCallStatus::Completed
                && output.as_deref() == Some("edited")
        ));
        // Parts are append-only, so the file change closes with a terminal part of its own; the
        // `InProgress` one emitted with the `tool_use` stays in the history.
        assert!(matches!(
            &events[1],
            ProviderEvent::Part(PartDraft {
                payload: PartKind::FileChange { id, status, changes }, ..
            }) if id == "tu1" && *status == ToolCallStatus::Completed
                && changes[0].path == "/tmp/a.rs"
        ));
    }

    #[tokio::test]
    async fn a_failed_tool_result_is_a_failed_tool_call() {
        let mut state = StreamState::default();
        message_parts(
            &json!({"type": "assistant", "message": {"id": "m1", "content": [
                {"type": "tool_use", "id": "tu1", "name": "Bash", "input": {"command": "false"}}]}}),
            &mut state,
            Some("t1"),
        );
        let events = message_parts(
            &json!({"type": "user", "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "tu1", "is_error": true,
                 "content": [{"type": "text", "text": "exit 1"}]}]}}),
            &mut state,
            Some("t1"),
        );
        assert!(matches!(
            &events[0],
            ProviderEvent::Part(PartDraft {
                payload: PartKind::ToolCall { status, output, .. }, ..
            }) if *status == ToolCallStatus::Failed && output.as_deref() == Some("exit 1")
        ));
    }

    #[tokio::test]
    async fn write_tool_reports_an_added_file() {
        let mut state = StreamState::default();
        let events = message_parts(
            &json!({"type": "assistant", "message": {"id": "m1", "content": [
                {"type": "tool_use", "id": "tu1", "name": "Write",
                 "input": {"file_path": "/tmp/new.rs", "content": "fn main() {}"}}]}}),
            &mut state,
            Some("t1"),
        );
        assert!(matches!(
            &events[1],
            ProviderEvent::Part(PartDraft {
                payload: PartKind::FileChange { changes, .. }, ..
            }) if changes[0].kind == "add" && changes[0].diff == "+ fn main() {}"
        ));
    }

    #[tokio::test]
    async fn multi_edit_reports_every_edit() {
        let mut state = StreamState::default();
        let events = message_parts(
            &json!({"type": "assistant", "message": {"id": "m1", "content": [
                {"type": "tool_use", "id": "tu1", "name": "MultiEdit", "input": {
                    "file_path": "/tmp/a.rs",
                    "edits": [
                        {"old_string": "a", "new_string": "b"},
                        {"old_string": "c", "new_string": "d"}]}}]}}),
            &mut state,
            Some("t1"),
        );
        assert!(matches!(
            &events[1],
            ProviderEvent::Part(PartDraft {
                payload: PartKind::FileChange { changes, .. }, ..
            }) if changes[0].diff == "- a\n+ b\n- c\n+ d"
        ));
    }

    #[tokio::test]
    async fn approval_round_trip_answers_with_the_permission_result() {
        let mut harness = Harness::new(supervised());
        let session_id = harness.provider.start(None).await.unwrap();
        harness
            .provider
            .turn("rm it".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.script(vec![
            init_frame(&session_id),
            json!({"type": "control_request", "request_id": "req-1", "request": {
                "subtype": "can_use_tool", "tool_name": "Bash",
                "input": {"command": "rm -rf build"},
                "decision_reason": "the command deletes files"}})
            .to_string(),
        ]);
        let request_id = loop {
            if let PartKind::ApprovalRequest {
                request_id,
                kind,
                summary,
                reason,
                answered,
            } = harness.next_part().await.payload
            {
                assert_eq!(kind, "command");
                assert_eq!(summary, "rm -rf build");
                assert_eq!(reason.as_deref(), Some("the command deletes files"));
                assert!(!answered);
                break request_id;
            }
        };
        assert_eq!(request_id, "req-1");

        harness
            .provider
            .answer_approval(&request_id, ApprovalDecision::AcceptForSession)
            .await
            .unwrap();
        let frames = harness.outbound();
        let answer = frames
            .iter()
            .find(|frame| frame["response"]["request_id"] == "req-1")
            .expect("approval answer frame");
        assert_eq!(answer["type"], "control_response");
        assert_eq!(answer["response"]["subtype"], "success");
        let result = &answer["response"]["response"];
        assert_eq!(result["behavior"], "allow");
        assert_eq!(result["updatedInput"]["command"], "rm -rf build");
        assert_eq!(result["updatedPermissions"][0]["type"], "addRules");
        assert_eq!(result["updatedPermissions"][0]["destination"], "session");

        // A second answer for the same request is a 404.
        let error = harness
            .provider
            .answer_approval(&request_id, ApprovalDecision::Decline)
            .await
            .unwrap_err();
        assert_eq!(error, StructuredChatError::NotFound);
    }

    #[tokio::test]
    async fn a_file_write_approval_is_classified_as_a_file_change() {
        let mut harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        harness
            .provider
            .turn("edit".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.script(vec![
            json!({"type": "control_request", "request_id": "req-2", "request": {
                "subtype": "can_use_tool", "tool_name": "Write",
                "input": {"file_path": "/tmp/a.rs", "content": "x"}}})
            .to_string(),
        ]);
        let part = harness.next_part().await;
        match &part.payload {
            PartKind::ApprovalRequest { kind, summary, .. } => {
                assert_eq!(kind, "fileChange");
                assert_eq!(summary, "Write /tmp/a.rs");
            }
            other => panic!("unexpected part {other:?}"),
        }
    }

    #[test]
    fn decline_and_accept_answers_carry_the_documented_shapes() {
        let input = json!({"command": "ls"});
        assert_eq!(
            permission_response(ApprovalDecision::Accept, "Bash", &input),
            json!({"behavior": "allow", "updatedInput": input})
        );
        assert_eq!(
            permission_response(ApprovalDecision::Decline, "Bash", &input)["behavior"],
            "deny"
        );
    }

    #[tokio::test]
    async fn interrupt_sends_the_interrupt_control_request() {
        let harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        harness.provider.interrupt("turn-1").await.unwrap();
        let frames = harness.outbound();
        let interrupt = frames
            .iter()
            .find(|frame| frame["request"]["subtype"] == "interrupt")
            .expect("interrupt control request");
        assert_eq!(interrupt["type"], "control_request");
        assert!(interrupt["request_id"].as_str().unwrap().starts_with("ferryx-"));
    }

    #[tokio::test]
    async fn steer_sends_a_priority_next_user_line() {
        let harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        let turn = harness
            .provider
            .turn("go".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.outbound();
        harness
            .provider
            .steer(&turn.turn_id, "left".into())
            .await
            .unwrap();
        let frames = harness.outbound();
        assert_eq!(frames[0]["type"], "user");
        assert_eq!(frames[0]["priority"], "next");
        assert_eq!(frames[0]["message"]["content"][0]["text"], "left");

        // Steering a turn that is not running is a conflict, not a silent send.
        let error = harness
            .provider
            .steer("someone-else", "hi".into())
            .await
            .unwrap_err();
        assert!(matches!(error, StructuredChatError::Conflict(_)));
    }

    #[tokio::test]
    async fn turn_sends_the_text_and_the_attachment_paths() {
        let harness = Harness::new(supervised());
        let session_id = harness.provider.start(None).await.unwrap();
        let turn = harness
            .provider
            .turn("hello".into(), vec!["/tmp/a.png".into()], &supervised())
            .await
            .unwrap();
        let frames = harness.outbound();
        assert_eq!(frames[0]["type"], "user");
        assert_eq!(frames[0]["session_id"], session_id);
        assert_eq!(frames[0]["uuid"], turn.turn_id);
        let text = frames[0]["message"]["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("hello"));
        assert!(text.contains("/tmp/a.png"));
        assert!(frames[0].get("priority").is_none());
    }

    /// REPORT item H: the mode is applied, but by relaunching with the new launch flags rather
    /// than by the live `set_permission_mode` this test used to require — the CLI refuses
    /// `bypassPermissions` on a process launched without the skip flag (`bypass_not_launched`),
    /// and t3code keys process reuse on the permission mode.
    #[tokio::test]
    async fn a_changed_permission_mode_is_applied_with_set_permission_mode() {
        let harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        assert!(!harness
            .last_argv()
            .iter()
            .any(|arg| arg == "--allow-dangerously-skip-permissions"));
        let auto = SessionSettings {
            permission_mode: Some(PermissionMode::AutoAcceptEdits),
            ..supervised()
        };
        harness
            .provider
            .turn("edit".into(), Vec::new(), &auto)
            .await
            .unwrap();
        // t3code keys process reuse on the permission mode, so a mode change is a relaunch; a
        // live `set_permission_mode` cannot enter a mode the process was not launched for.
        let argvs = harness.argvs();
        assert_eq!(argvs.len(), 2, "a mode change reopens the CLI: {argvs:?}");
        let reopened = &argvs[1];
        assert_eq!(
            reopened[reopened.iter().position(|arg| arg == "--permission-mode").unwrap() + 1],
            "acceptEdits"
        );
        assert!(reopened.iter().any(|arg| arg == "--resume"));
        assert!(!harness
            .outbound()
            .iter()
            .any(|frame| frame["request"]["subtype"] == "set_permission_mode"));
    }

    #[tokio::test]
    async fn a_changed_model_is_applied_with_set_model() {
        let harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        let other = SessionSettings {
            model: Some("claude-opus-4-7".into()),
            ..supervised()
        };
        harness
            .provider
            .turn("hi".into(), Vec::new(), &other)
            .await
            .unwrap();
        let frames = harness.outbound();
        let control = frames
            .iter()
            .find(|frame| frame["request"]["subtype"] == "set_model")
            .expect("set_model control request");
        assert_eq!(control["request"]["model"], "claude-opus-4-7");
    }

    #[tokio::test]
    async fn a_new_effort_is_applied_live_with_apply_flag_settings() {
        let harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        let heavy = SessionSettings {
            effort: Some("high".into()),
            ..supervised()
        };
        harness
            .provider
            .turn("think".into(), Vec::new(), &heavy)
            .await
            .unwrap();
        let frames = harness.outbound();
        let control = frames
            .iter()
            .find(|frame| frame["request"]["subtype"] == "apply_flag_settings")
            .expect("apply_flag_settings control request");
        assert_eq!(control["request"]["settings"]["effortLevel"], "high");
        // A live flag change keeps the process: only clearing effort relaunches.
        assert_eq!(harness.argvs().len(), 1);
    }

    #[tokio::test]
    async fn clearing_effort_reopens_the_session_with_resume() {
        let heavy = SessionSettings {
            effort: Some("high".into()),
            ..supervised()
        };
        let harness = Harness::new(heavy);
        let session_id = harness.provider.start(None).await.unwrap();
        let argv = harness.last_argv();
        assert_eq!(argv[argv.iter().position(|arg| arg == "--effort").unwrap() + 1], "high");
        harness
            .provider
            .turn("lighter".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        let argvs = harness.argvs();
        assert_eq!(argvs.len(), 2);
        assert!(!argvs[1].iter().any(|arg| arg == "--effort"));
        assert_eq!(
            argvs[1][argvs[1].iter().position(|arg| arg == "--resume").unwrap() + 1],
            session_id
        );
    }

    #[test]
    fn usage_comes_from_the_result_frame() {
        let part = usage_part(&json!({
            "type": "result", "subtype": "success", "is_error": false,
            "usage": {"input_tokens": 10, "cache_read_input_tokens": 5,
                "cache_creation_input_tokens": 2, "output_tokens": 7},
            "modelUsage": {"claude-sonnet-4-6": {"inputTokens": 17, "outputTokens": 7,
                "contextWindow": 200000}},
        }))
        .expect("a usage part");
        assert_eq!(
            part,
            PartKind::Usage {
                total_tokens: 24,
                input_tokens: 17,
                cached_input_tokens: 5,
                output_tokens: 7,
                context_window: Some(200000),
            }
        );
        // A zeroed result reports nothing rather than a misleading 0/0 row.
        assert!(usage_part(&json!({"type": "result", "usage": {
            "input_tokens": 0, "output_tokens": 0
        }}))
        .is_none());
    }

    #[tokio::test]
    async fn a_failed_result_ends_the_turn_with_its_error_text() {
        let mut harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        let turn = harness
            .provider
            .turn("hi".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.script(vec![json!({
            "type": "result", "subtype": "success", "is_error": true,
            "result": "Not logged in · Please run /login",
            "usage": {"input_tokens": 3, "output_tokens": 4},
        })
        .to_string()]);
        let (turn_id, error) = harness.next_completed().await;
        assert_eq!(turn_id, turn.turn_id);
        assert_eq!(error.as_deref(), Some("Not logged in · Please run /login"));
    }

    #[tokio::test]
    async fn a_result_of_a_different_subtype_reports_its_error_list() {
        let mut harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        harness
            .provider
            .turn("hi".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.script(vec![json!({
            "type": "result", "subtype": "error_during_execution", "is_error": true,
            "errors": ["No conversation found with session ID: x"],
            "usage": {},
        })
        .to_string()]);
        let (_, error) = harness.next_completed().await;
        assert_eq!(
            error.as_deref(),
            Some("No conversation found with session ID: x")
        );
    }

    /// REPORT item C: the denial ends the call the model made, instead of the invented
    /// `denied-{i}-{name}` call this test used to accept.
    #[tokio::test]
    async fn a_permission_denial_in_the_result_becomes_a_declined_tool_call() {
        let mut harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        harness
            .provider
            .turn("hi".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.script(vec![json!({
            "type": "result", "subtype": "success", "is_error": false,
            "permission_denials": [{"tool_name": "Bash", "tool_use_id": "tu1"}],
            "usage": {"input_tokens": 1, "output_tokens": 1},
        })
        .to_string()]);
        let mut declined = false;
        for _ in 0..3 {
            if let ProviderEvent::Part(PartDraft {
                payload: PartKind::ToolCall { id, name, status, .. },
                ..
            }) = harness.next_event().await
            {
                // The call the model made ends declined; no second, invented call appears.
                assert_eq!(id, "tu1", "the denial must not invent a call");
                if name == "Bash" && status == ToolCallStatus::Declined {
                    declined = true;
                    break;
                }
            }
        }
        assert!(declined, "the denial is reported as a declined tool call");
    }

    #[tokio::test]
    async fn malformed_lines_are_skipped_and_the_connection_survives() {
        let mut harness = Harness::new(supervised());
        let session_id = harness.provider.start(None).await.unwrap();
        harness
            .provider
            .turn("hi".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.script(vec![
            "{not json".to_owned(),
            "[]".to_owned(),
            json!({"type": "control_response"}).to_string(),
            init_frame(&session_id),
            assistant_frame("m1", "still here", "u1"),
        ]);
        let part = harness.next_part().await;
        assert_eq!(
            part.payload,
            PartKind::AssistantMessage {
                text: "still here".into()
            }
        );
    }

    #[tokio::test]
    async fn child_exit_mid_turn_emits_ended() {
        let mut harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        harness
            .provider
            .turn("hi".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.end_child();
        loop {
            if matches!(
                harness.next_event().await,
                ProviderEvent::Ended { .. }
            ) {
                break;
            }
        }
    }

    #[tokio::test]
    async fn compact_sends_the_slash_command() {
        let harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        harness.provider.compact().await.unwrap();
        let frames = harness.outbound();
        assert_eq!(frames[0]["message"]["content"][0]["text"], "/compact");
    }

    #[tokio::test]
    async fn clear_reopens_with_a_fresh_session_id() {
        let harness = Harness::new(supervised());
        let first = harness.provider.start(None).await.unwrap();
        let second = harness.provider.clear().await.unwrap();
        assert_ne!(first, second);
        let argvs = harness.argvs();
        assert_eq!(argvs.len(), 2);
        let named = &argvs[1][argvs[1].iter().position(|arg| arg == "--session-id").unwrap() + 1];
        assert_eq!(named, &second);
        assert!(!argvs[1].iter().any(|arg| arg == "--resume"));
    }

    #[tokio::test]
    async fn rollback_reopens_at_the_kept_turns_cursor() {
        let mut harness = Harness::new(supervised());
        let session_id = harness.provider.start(None).await.unwrap();

        let first = harness
            .provider
            .turn("one".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.script(vec![assistant_frame("m1", "first", "uuid-1"), result_frame("ok")]);
        assert_eq!(harness.next_completed().await.0, first.turn_id);

        let second = harness
            .provider
            .turn("two".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.script(vec![assistant_frame("m2", "second", "uuid-2"), result_frame("ok")]);
        assert_eq!(harness.next_completed().await.0, second.turn_id);

        let reopened = harness.provider.rollback(&second.turn_id).await.unwrap();
        assert_eq!(reopened, session_id, "a kept thread keeps its id");
        let argv = harness.last_argv();
        assert_eq!(
            argv[argv.iter().position(|arg| arg == "--resume").unwrap() + 1],
            session_id
        );
        assert_eq!(
            argv[argv.iter().position(|arg| arg == "--resume-session-at").unwrap() + 1],
            "uuid-1"
        );
        assert!(!argv.iter().any(|arg| arg == "--session-id"));
    }

    /// REPORT item G: the returned id is the thread the store must now persist.
    #[tokio::test]
    async fn rollback_of_the_first_turn_opens_a_fresh_thread() {
        let mut harness = Harness::new(supervised());
        let before = harness.provider.start(None).await.unwrap();
        let first = harness
            .provider
            .turn("one".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.script(vec![assistant_frame("m1", "first", "uuid-1"), result_frame("ok")]);
        harness.next_completed().await;

        let reopened = harness.provider.rollback(&first.turn_id).await.unwrap();
        let argv = harness.last_argv();
        assert!(!argv.iter().any(|arg| arg == "--resume"));
        let named = argv[argv.iter().position(|arg| arg == "--session-id").unwrap() + 1].clone();
        assert_ne!(named, before);
        // The store persists what this returns, so a restart resumes the new thread rather than
        // the conversation the rollback discarded.
        assert_eq!(reopened, named);
    }

    #[tokio::test]
    async fn rollback_without_a_cursor_refuses_instead_of_opening_an_empty_session() {
        let mut harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        harness
            .provider
            .turn("one".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        // A result that settles the turn without any assistant frame: the turn is in the order,
        // but the CLI recorded no cursor for it.
        harness.script(vec![result_frame("ok")]);
        harness.next_completed().await;
        let second = harness
            .provider
            .turn("two".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        harness.script(vec![result_frame("ok")]);
        harness.next_completed().await;

        let error = harness.provider.rollback(&second.turn_id).await.unwrap_err();
        assert!(
            matches!(&error, StructuredChatError::Conflict(message)
                if message.starts_with("ROLLBACK_NO_CURSOR")),
            "a kept turn with no cursor must refuse, not open an empty session: {error:?}"
        );
        assert_eq!(
            harness.argvs().len(),
            1,
            "a refused rollback must not spawn a fresh CLI"
        );
    }

    #[tokio::test]
    async fn rollback_refuses_while_a_turn_runs() {
        let harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        let turn = harness
            .provider
            .turn("one".into(), Vec::new(), &supervised())
            .await
            .unwrap();
        let error = harness.provider.rollback(&turn.turn_id).await.unwrap_err();
        assert!(matches!(error, StructuredChatError::Conflict(_)));
    }

    #[tokio::test]
    async fn rollback_of_an_unknown_turn_is_a_conflict() {
        let harness = Harness::new(supervised());
        harness.provider.start(None).await.unwrap();
        let error = harness.provider.rollback("no-such-turn").await.unwrap_err();
        assert!(matches!(&error, StructuredChatError::Conflict(message)
            if message.starts_with("ROLLBACK_UNKNOWN_TURN")));
    }

    #[tokio::test]
    async fn capabilities_support_every_mode_and_rollback() {
        let capabilities = claude_capabilities();
        assert!(capabilities.steer);
        assert!(capabilities.rollback);
        assert!(!capabilities.resume, "claude has no native continuation verb");
        assert!(!capabilities.revert_files, "rollback rewinds history only");
        assert_eq!(capabilities.permission_modes, PermissionMode::all().to_vec());
        assert!(capabilities.models.is_empty(), "discovery fills the catalog");
    }

    #[test]
    fn control_frames_classify_by_shape() {
        assert_eq!(
            classify_frame(
                r#"{"type":"control_response","response":{"subtype":"error",
                   "request_id":"r1","error":"nope"}}"#
            ),
            InboundFrame::ControlResponse {
                request_id: "r1".into(),
                result: Err("nope".into()),
            }
        );
        assert_eq!(classify_frame("not json"), InboundFrame::Malformed);
        assert_eq!(
            classify_frame(r#"{"type":"control_cancel_request","request_id":"r2"}"#),
            InboundFrame::ControlCancelled {
                request_id: "r2".into()
            }
        );
        assert!(matches!(
            classify_frame(r#"{"type":"assistant","message":{"id":"m"}}"#),
            InboundFrame::Message(_)
        ));
    }
}
