//! Antigravity (`agy`) bidirectional stream-json structured-chat provider.
//!
//! Verified against live `agy --help`:
//! * `--print --input-format stream-json --output-format stream-json`
//! * `--mode <accept-edits|auto|plan>`
//! * `--effort <low|medium|high|xhigh|max>`
//! * `--model <model>`
//! * `--agent <agent>`
//! * `--add-dir <dir>` (repeatable)
//! * `--dangerously-skip-permissions`
//! * `--conversation <id>` (resume by ID)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::{mpsc, Mutex};

use super::types::{
    ApprovalDecision, Capabilities, ModelInfo, PartKind, PermissionMode, SessionSettings,
    StructuredChatError, ToolCallStatus,
};
use super::{PartDraft, ProviderEvent, StructuredProvider, TurnRef};

pub const AGY_PROGRAM: &str = "agy";

pub fn antigravity_capabilities() -> Capabilities {
    Capabilities {
        steer: false,
        rollback: false,
        resume: true,
        revert_files: false,
        history_paging: true,
        permission_modes: vec![
            PermissionMode::Supervised,
            PermissionMode::AutoAcceptEdits,
            PermissionMode::Auto,
            PermissionMode::FullAccess,
        ],
        models: Vec::new(),
        slash_commands: vec!["/clear".into(), "/compact".into(), "/help".into()],
        execution_modes: vec!["accept-edits".into(), "auto".into(), "plan".into()],
        agents: Vec::new(),
        thinking_levels: vec![
            "low".into(),
            "medium".into(),
            "high".into(),
            "xhigh".into(),
            "max".into(),
        ],
        additional_dirs: true,
        ..Capabilities::default()
    }
}

pub fn build_agy_argv(settings: &SessionSettings, conversation_id: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "--print".to_owned(),
        "--input-format".to_owned(),
        "stream-json".to_owned(),
        "--output-format".to_owned(),
        "stream-json".to_owned(),
    ];
    if let Some(mode) = resolve_agy_mode(settings) {
        args.push("--mode".to_owned());
        args.push(mode);
    }
    if let Some(effort) = settings.effort.as_deref().filter(|s| !s.trim().is_empty()) {
        args.push("--effort".to_owned());
        args.push(effort.to_owned());
    }
    if let Some(model) = settings.model.as_deref().filter(|s| !s.trim().is_empty()) {
        args.push("--model".to_owned());
        args.push(model.to_owned());
    }
    if let Some(agent) = settings.agent_name.as_deref().filter(|s| !s.trim().is_empty()) {
        args.push("--agent".to_owned());
        args.push(agent.to_owned());
    }
    for dir in &settings.additional_dirs {
        if !dir.trim().is_empty() {
            args.push("--add-dir".to_owned());
            args.push(dir.clone());
        }
    }
    if settings.permission_mode == Some(PermissionMode::FullAccess) {
        args.push("--dangerously-skip-permissions".to_owned());
    }
    if let Some(conv) = conversation_id.filter(|s| !s.trim().is_empty()) {
        args.push("--conversation".to_owned());
        args.push(conv.to_owned());
    }
    args
}

fn resolve_agy_mode(settings: &SessionSettings) -> Option<String> {
    if let Some(mode) = settings
        .execution_mode
        .as_deref()
        .filter(|m| matches!(*m, "accept-edits" | "auto" | "plan"))
    {
        return Some(mode.to_owned());
    }
    match settings.permission_mode {
        Some(PermissionMode::AutoAcceptEdits) => Some("accept-edits".into()),
        Some(PermissionMode::Auto) => Some("auto".into()),
        _ => None,
    }
}

fn spawn_settings_changed(a: &SessionSettings, b: &SessionSettings) -> bool {
    a.model != b.model
        || a.effort != b.effort
        || a.permission_mode != b.permission_mode
        || a.execution_mode != b.execution_mode
        || a.agent_name != b.agent_name
        || a.additional_dirs != b.additional_dirs
}

#[derive(Debug, Default)]
struct AgyState {
    conversation_id: Option<String>,
    active_turn_id: Option<String>,
    pending_approvals: HashMap<String, String>,
    tool_names: HashMap<String, String>,
}

struct AgyProcess {
    writer: Mutex<Box<dyn AsyncWrite + Unpin + Send>>,
    child: Mutex<Option<Child>>,
    settings: SessionSettings,
    state: Arc<Mutex<AgyState>>,
}

impl AgyProcess {
    fn from_io<R, W>(
        reader: R,
        writer: W,
        child: Option<Child>,
        settings: SessionSettings,
        conversation_id: Option<String>,
        events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> Arc<Self>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let state = Arc::new(Mutex::new(AgyState {
            conversation_id,
            ..AgyState::default()
        }));
        let proc = Arc::new(Self {
            writer: Mutex::new(Box::new(writer)),
            child: Mutex::new(child),
            settings,
            state: Arc::clone(&state),
        });
        tokio::spawn(agy_read_loop(reader, state, events));
        proc
    }

    async fn write_frame(&self, frame: &Value) -> Result<(), StructuredChatError> {
        let mut line = serde_json::to_string(frame)
            .map_err(|e| StructuredChatError::Provider(format!("agy serialize: {e}")))?;
        line.push('\n');
        let mut w = self.writer.lock().await;
        w.write_all(line.as_bytes())
            .await
            .map_err(|e| StructuredChatError::Provider(format!("agy write: {e}")))?;
        w.flush()
            .await
            .map_err(|e| StructuredChatError::Provider(format!("agy flush: {e}")))?;
        Ok(())
    }

    async fn kill(&self) {
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill().await;
        }
    }
}

async fn agy_read_loop<R: AsyncRead + Unpin + Send + 'static>(
    reader: R,
    state: Arc<Mutex<AgyState>>,
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
                handle_agy_frame(&value, &state, &events).await;
            }
            Err(err) => break format!("read error: {err}"),
        }
    };

    let mut st = state.lock().await;
    st.pending_approvals.clear();
    let active_turn = st.active_turn_id.take();
    drop(st);
    if let Some(turn_id) = active_turn {
        let _ = events.send(ProviderEvent::TurnCompleted {
            turn_id,
            error: Some(format!("agy exited mid-turn ({end_reason})")),
        });
    }
    let _ = events.send(ProviderEvent::Ended { reason: end_reason });
}

async fn handle_agy_frame(
    value: &Value,
    state: &Arc<Mutex<AgyState>>,
    events: &mpsc::UnboundedSender<ProviderEvent>,
) {
    let Some(frame_type) = value.get("type").and_then(Value::as_str) else {
        return;
    };
    match frame_type {
        "system" => {
            if let Some(cid) = value
                .get("conversation_id")
                .or_else(|| value.get("session_id"))
                .and_then(Value::as_str)
            {
                state.lock().await.conversation_id = Some(cid.to_owned());
            }
        }
        "assistant" => {
            let turn_id = state.lock().await.active_turn_id.clone();
            let blocks = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(Value::as_array);
            if let Some(blocks) = blocks {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                if !text.is_empty() {
                                    let _ = events.send(ProviderEvent::Part(PartDraft {
                                        turn_id: turn_id.clone(),
                                        payload: PartKind::AssistantMessage {
                                            text: text.to_owned(),
                                        },
                                    }));
                                }
                            }
                        }
                        Some("thinking") => {
                            if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                                if !text.is_empty() {
                                    let _ = events.send(ProviderEvent::Part(PartDraft {
                                        turn_id: turn_id.clone(),
                                        payload: PartKind::Reasoning {
                                            text: text.to_owned(),
                                        },
                                    }));
                                }
                            }
                        }
                        Some("tool_use") => {
                            let id = block
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("tool")
                                .to_owned();
                            let name = block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("tool")
                                .to_owned();
                            state.lock().await.tool_names.insert(id.clone(), name.clone());
                            let _ = events.send(ProviderEvent::Part(PartDraft {
                                turn_id: turn_id.clone(),
                                payload: PartKind::ToolCall {
                                    id,
                                    name,
                                    status: ToolCallStatus::InProgress,
                                    output: block.get("input").map(|v| v.to_string()),
                                    exit_code: None,
                                    duration_ms: None,
                                },
                            }));
                        }
                        _ => {}
                    }
                }
            }
        }
        "user" => {
            let turn_id = state.lock().await.active_turn_id.clone();
            let blocks = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(Value::as_array);
            if let Some(blocks) = blocks {
                for block in blocks {
                    if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                        let id = block
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or("tool")
                            .to_owned();
                        let name = state
                            .lock()
                            .await
                            .tool_names
                            .remove(&id)
                            .unwrap_or_else(|| "tool".into());
                        let is_error = block
                            .get("is_error")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        let output = block
                            .get("content")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                            .or_else(|| block.get("content").map(|v| v.to_string()));
                        let _ = events.send(ProviderEvent::Part(PartDraft {
                            turn_id: turn_id.clone(),
                            payload: PartKind::ToolCall {
                                id,
                                name,
                                status: if is_error {
                                    ToolCallStatus::Failed
                                } else {
                                    ToolCallStatus::Completed
                                },
                                output,
                                exit_code: None,
                                duration_ms: None,
                            },
                        }));
                    }
                }
            }
        }
        "control_request" | "permission_request" => {
            let turn_id = state.lock().await.active_turn_id.clone();
            let request_id = value
                .get("request_id")
                .or_else(|| value.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("perm")
                .to_owned();
            let kind = value
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("command")
                .to_owned();
            let summary = value
                .get("summary")
                .or_else(|| value.get("tool_name"))
                .and_then(Value::as_str)
                .unwrap_or("Approval required")
                .to_owned();
            let reason = value
                .get("reason")
                .and_then(Value::as_str)
                .map(str::to_owned);
            state
                .lock()
                .await
                .pending_approvals
                .insert(request_id.clone(), kind.clone());
            let _ = events.send(ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::ApprovalRequest {
                    request_id,
                    kind,
                    summary,
                    reason,
                    answered: false,
                },
            }));
        }
        "result" => {
            if let Some(cid) = value
                .get("conversation_id")
                .or_else(|| value.get("session_id"))
                .and_then(Value::as_str)
            {
                state.lock().await.conversation_id = Some(cid.to_owned());
            }
            let turn_id = state.lock().await.active_turn_id.clone();
            if let Some(usage) = value.get("usage") {
                let input_tokens = usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
                let output_tokens = usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
                let cached = usage
                    .get("cache_read_input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let total_tokens = input_tokens + output_tokens + cached;
                if total_tokens > 0 {
                    let _ = events.send(ProviderEvent::Part(PartDraft {
                        turn_id: turn_id.clone(),
                        payload: PartKind::Usage {
                            total_tokens,
                            input_tokens,
                            cached_input_tokens: cached,
                            output_tokens,
                            context_window: None,
                        },
                    }));
                }
            }
            let is_error = value.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            let error = if is_error {
                Some(
                    value
                        .get("result")
                        .or_else(|| value.get("error"))
                        .and_then(Value::as_str)
                        .unwrap_or("agy turn failed")
                        .to_owned(),
                )
            } else {
                None
            };
            let mut st = state.lock().await;
            st.pending_approvals.clear();
            let completed = st.active_turn_id.take();
            drop(st);
            if let Some(tid) = completed {
                let _ = events.send(ProviderEvent::TurnCompleted { turn_id: tid, error });
            }
        }
        "error" => {
            if let Some(msg) = value.get("message").or_else(|| value.get("error")).and_then(Value::as_str) {
                let turn_id = state.lock().await.active_turn_id.clone();
                let _ = events.send(ProviderEvent::Part(PartDraft {
                    turn_id,
                    payload: PartKind::Error {
                        message: msg.to_owned(),
                    },
                }));
            }
        }
        _ => {}
    }
}

pub struct AntigravityProvider {
    cwd: PathBuf,
    events: mpsc::UnboundedSender<ProviderEvent>,
    initial_settings: SessionSettings,
    conversation_id: Mutex<Option<String>>,
    proc: Mutex<Option<Arc<AgyProcess>>>,
}

impl AntigravityProvider {
    pub fn spawn(
        cwd: PathBuf,
        events: mpsc::UnboundedSender<ProviderEvent>,
        settings: &SessionSettings,
    ) -> Result<Self, StructuredChatError> {
        Ok(Self {
            cwd,
            events,
            initial_settings: settings.clone(),
            conversation_id: Mutex::new(None),
            proc: Mutex::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn from_test_io<R, W>(
        cwd: PathBuf,
        events: mpsc::UnboundedSender<ProviderEvent>,
        settings: SessionSettings,
        conversation_id: Option<String>,
        reader: R,
        writer: W,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let proc = AgyProcess::from_io(
            reader,
            writer,
            None,
            settings.clone(),
            conversation_id.clone(),
            events.clone(),
        );
        Self {
            cwd,
            events,
            initial_settings: settings,
            conversation_id: Mutex::new(conversation_id),
            proc: Mutex::new(Some(proc)),
        }
    }

    async fn spawn_or_relaunch(
        &self,
        settings: &SessionSettings,
    ) -> Result<Arc<AgyProcess>, StructuredChatError> {
        let mut guard = self.proc.lock().await;
        if let Some(existing) = guard.as_ref() {
            if !spawn_settings_changed(&existing.settings, settings) {
                return Ok(Arc::clone(existing));
            }
            if let Some(cid) = existing.state.lock().await.conversation_id.clone() {
                *self.conversation_id.lock().await = Some(cid);
            }
            existing.kill().await;
            *guard = None;
        }
        let conv = self.conversation_id.lock().await.clone();
        let args = build_agy_argv(settings, conv.as_deref());
        let mut cmd = tokio::process::Command::new(AGY_PROGRAM);
        cmd.args(&args)
            .current_dir(&self.cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut child = cmd.spawn().map_err(|err| {
            StructuredChatError::Provider(format!("failed to spawn agy stream-json: {err}"))
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            StructuredChatError::Provider("failed to capture agy stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            StructuredChatError::Provider("failed to capture agy stdout".into())
        })?;
        let proc = AgyProcess::from_io(
            stdout,
            stdin,
            Some(child),
            settings.clone(),
            conv,
            self.events.clone(),
        );
        *guard = Some(Arc::clone(&proc));
        Ok(proc)
    }
}

#[async_trait]
impl StructuredProvider for AntigravityProvider {
    fn provider_id(&self) -> &'static str {
        "antigravity"
    }

    fn capabilities(&self) -> Capabilities {
        antigravity_capabilities()
    }

    async fn start(&self, resume_thread: Option<String>) -> Result<String, StructuredChatError> {
        let conv_id = resume_thread.unwrap_or_else(|| format!("agy-{}", uuid::Uuid::new_v4()));
        *self.conversation_id.lock().await = Some(conv_id.clone());
        Ok(conv_id)
    }

    async fn turn(
        &self,
        text: String,
        attachments: Vec<String>,
        settings: &SessionSettings,
    ) -> Result<TurnRef, StructuredChatError> {
        let proc = self.spawn_or_relaunch(settings).await?;
        let turn_id = format!("agy-turn-{}", uuid::Uuid::new_v4());
        proc.state.lock().await.active_turn_id = Some(turn_id.clone());
        let _ = self.events.send(ProviderEvent::TurnStarted {
            turn_id: turn_id.clone(),
        });

        let mut content = text;
        for path in attachments {
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push('@');
            content.push_str(&path);
        }
        proc.write_frame(&json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": content,
            }
        }))
        .await?;
        Ok(TurnRef { turn_id })
    }

    async fn steer(&self, _expected_turn: &str, _text: String) -> Result<(), StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "STEER_UNSUPPORTED: agy stream-json does not support mid-turn steer".into(),
        ))
    }

    async fn interrupt(&self, _turn_id: &str) -> Result<(), StructuredChatError> {
        if let Some(proc) = self.proc.lock().await.take() {
            if let Some(cid) = proc.state.lock().await.conversation_id.clone() {
                *self.conversation_id.lock().await = Some(cid);
            }
            proc.kill().await;
        }
        Ok(())
    }

    async fn rollback(&self, _before_turn_id: &str) -> Result<String, StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "UNSUPPORTED_METHOD: agy stream-json does not support rollback".into(),
        ))
    }

    async fn answer_approval(
        &self,
        request_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), StructuredChatError> {
        let proc = self
            .proc
            .lock()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| StructuredChatError::Conflict("SESSION_ENDED: agy not running".into()))?;
        proc.state.lock().await.pending_approvals.remove(request_id);
        let behavior = match decision {
            ApprovalDecision::Accept | ApprovalDecision::AcceptForSession => "allow",
            ApprovalDecision::Decline => "deny",
        };
        proc.write_frame(&json!({
            "type": "control_response",
            "request_id": request_id,
            "decision": behavior,
        }))
        .await
    }

    async fn clear(&self) -> Result<String, StructuredChatError> {
        if let Some(proc) = self.proc.lock().await.take() {
            proc.kill().await;
        }
        let fresh = format!("agy-{}", uuid::Uuid::new_v4());
        *self.conversation_id.lock().await = Some(fresh.clone());
        Ok(fresh)
    }

    async fn compact(&self) -> Result<(), StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "COMPACT_UNSUPPORTED: agy stream-json has no compact verb".into(),
        ))
    }

    async fn close(&self) {
        let _ = &self.initial_settings;
        if let Some(proc) = self.proc.lock().await.take() {
            proc.kill().await;
        }
    }

    fn event_sink(&self) -> mpsc::UnboundedSender<ProviderEvent> {
        self.events.clone()
    }
}

pub async fn discover_models_with_commands(
    _cwd: &Path,
) -> Result<(Vec<ModelInfo>, Vec<String>), StructuredChatError> {
    Ok((Vec::new(), antigravity_capabilities().slash_commands))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[test]
    fn argv_includes_all_measured_agy_flags() {
        let settings = SessionSettings {
            model: Some("gemini-3-pro".into()),
            effort: Some("xhigh".into()),
            permission_mode: Some(PermissionMode::FullAccess),
            execution_mode: Some("plan".into()),
            agent_name: Some("reviewer".into()),
            additional_dirs: vec!["/extra/a".into(), "/extra/b".into()],
            ..SessionSettings::default()
        };
        let args = build_agy_argv(&settings, Some("conv-42"));
        assert!(args.iter().any(|a| a == "--print"));
        assert!(args.windows(2).any(|w| w == ["--input-format", "stream-json"]));
        assert!(args.windows(2).any(|w| w == ["--output-format", "stream-json"]));
        assert!(args.windows(2).any(|w| w == ["--mode", "plan"]));
        assert!(args.windows(2).any(|w| w == ["--effort", "xhigh"]));
        assert!(args.windows(2).any(|w| w == ["--model", "gemini-3-pro"]));
        assert!(args.windows(2).any(|w| w == ["--agent", "reviewer"]));
        assert!(args.windows(2).any(|w| w == ["--add-dir", "/extra/a"]));
        assert!(args.windows(2).any(|w| w == ["--add-dir", "/extra/b"]));
        assert!(args.iter().any(|a| a == "--dangerously-skip-permissions"));
        assert!(args.windows(2).any(|w| w == ["--conversation", "conv-42"]));
    }

    #[test]
    fn argv_maps_permission_mode_when_execution_mode_unset() {
        let s1 = SessionSettings {
            permission_mode: Some(PermissionMode::AutoAcceptEdits),
            ..SessionSettings::default()
        };
        assert!(build_agy_argv(&s1, None).windows(2).any(|w| w == ["--mode", "accept-edits"]));
        let s2 = SessionSettings {
            permission_mode: Some(PermissionMode::Auto),
            ..SessionSettings::default()
        };
        assert!(build_agy_argv(&s2, None).windows(2).any(|w| w == ["--mode", "auto"]));
        let s3 = SessionSettings {
            permission_mode: Some(PermissionMode::Supervised),
            ..SessionSettings::default()
        };
        assert!(!build_agy_argv(&s3, None).iter().any(|a| a == "--dangerously-skip-permissions"));
    }

    #[test]
    fn spawn_settings_changed_detects_mode_agent_and_dirs() {
        let base = SessionSettings::default();
        let mut with_dir = base.clone();
        with_dir.additional_dirs.push("/tmp/extra".into());
        assert!(spawn_settings_changed(&base, &with_dir));

        let mut with_agent = base.clone();
        with_agent.agent_name = Some("ops".into());
        assert!(spawn_settings_changed(&base, &with_agent));

        let mut with_mode = base.clone();
        with_mode.execution_mode = Some("plan".into());
        assert!(spawn_settings_changed(&base, &with_mode));

        assert!(!spawn_settings_changed(&base, &base));
    }

    #[test]
    fn capabilities_advertise_agy_modes_efforts_and_add_dir() {
        let caps = antigravity_capabilities();
        assert_eq!(caps.execution_modes, vec!["accept-edits", "auto", "plan"]);
        assert_eq!(caps.thinking_levels, vec!["low", "medium", "high", "xhigh", "max"]);
        assert!(caps.additional_dirs);
        assert!(!caps.steer);
    }

    #[tokio::test]
    async fn turn_writes_ndjson_user_frame_and_parses_stream_events() {
        let (cr, mut sw) = tokio::io::duplex(32768);
        let (sr, cw) = tokio::io::duplex(32768);
        let mut sreader = BufReader::new(sr);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let settings = SessionSettings::default();
        let provider = AntigravityProvider::from_test_io(
            PathBuf::from("/tmp"),
            tx,
            settings.clone(),
            None,
            cr,
            cw,
        );

        let tref = provider.turn("hello".into(), vec!["a.rs".into()], &settings).await.unwrap();
        let mut line = String::new();
        sreader.read_line(&mut line).await.unwrap();
        let sent: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(sent["type"], "user");
        assert_eq!(sent["message"]["content"], "hello\n@a.rs");

        for frame in [
            json!({ "type": "system", "conversation_id": "conv-99" }),
            json!({
                "type": "assistant",
                "message": {
                    "content": [
                        { "type": "thinking", "thinking": "hmm" },
                        { "type": "text", "text": "done" },
                        { "type": "tool_use", "id": "tu1", "name": "bash", "input": { "cmd": "ls" } }
                    ]
                }
            }),
            json!({
                "type": "user",
                "message": {
                    "content": [
                        { "type": "tool_result", "tool_use_id": "tu1", "content": "ok", "is_error": false }
                    ]
                }
            }),
            json!({
                "type": "result",
                "conversation_id": "conv-99",
                "is_error": false,
                "usage": { "input_tokens": 50, "output_tokens": 25 }
            }),
        ] {
            let mut s = serde_json::to_string(&frame).unwrap();
            s.push('\n');
            sw.write_all(s.as_bytes()).await.unwrap();
        }
        sw.flush().await.unwrap();

        let mut seen = Vec::new();
        while let Some(ev) = rx.recv().await {
            match ev {
                ProviderEvent::TurnStarted { turn_id } => {
                    assert_eq!(turn_id, tref.turn_id);
                    seen.push("start");
                }
                ProviderEvent::Part(p) => match p.payload {
                    PartKind::Reasoning { .. } => seen.push("thinking"),
                    PartKind::AssistantMessage { .. } => seen.push("text"),
                    PartKind::ToolCall { status: ToolCallStatus::InProgress, .. } => seen.push("tool_use"),
                    PartKind::ToolCall { status: ToolCallStatus::Completed, name, .. } => {
                        assert_eq!(name, "bash");
                        seen.push("tool_result");
                    }
                    PartKind::Usage { total_tokens, .. } => {
                        assert_eq!(total_tokens, 75);
                        seen.push("usage");
                    }
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
        assert_eq!(seen, vec!["start", "thinking", "text", "tool_use", "tool_result", "usage", "end"]);
    }

    #[tokio::test]
    async fn approval_request_roundtrips_allow_and_deny() {
        let (cr, mut sw) = tokio::io::duplex(16384);
        let (sr, cw) = tokio::io::duplex(16384);
        let mut sreader = BufReader::new(sr);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let provider = AntigravityProvider::from_test_io(
            PathBuf::from("/tmp"),
            tx,
            SessionSettings::default(),
            None,
            cr,
            cw,
        );
        let req = json!({
            "type": "permission_request",
            "request_id": "p-1",
            "kind": "command",
            "summary": "run tests"
        });
        sw.write_all(format!("{req}\n").as_bytes()).await.unwrap();
        sw.flush().await.unwrap();
        let ev = rx.recv().await.unwrap();
        assert!(matches!(ev, ProviderEvent::Part(PartDraft { payload: PartKind::ApprovalRequest { request_id, .. }, .. }) if request_id == "p-1"));

        provider.answer_approval("p-1", ApprovalDecision::Accept).await.unwrap();
        let mut line = String::new();
        sreader.read_line(&mut line).await.unwrap();
        let resp: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(resp["type"], "control_response");
        assert_eq!(resp["request_id"], "p-1");
        assert_eq!(resp["decision"], "allow");

        provider.answer_approval("p-2", ApprovalDecision::Decline).await.unwrap();
        line.clear();
        sreader.read_line(&mut line).await.unwrap();
        let resp2: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(resp2["decision"], "deny");
    }

    #[tokio::test]
    async fn invalid_utf8_and_malformed_lines_are_skipped() {
        let (cr, mut sw) = tokio::io::duplex(16384);
        let (_sr, cw) = tokio::io::duplex(16384);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _provider = AntigravityProvider::from_test_io(
            PathBuf::from("/tmp"),
            tx,
            SessionSettings::default(),
            None,
            cr,
            cw,
        );
        sw.write_all(b"\xff\xfe bad\nnot-json\n").await.unwrap();
        sw.write_all(b"{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}\n").await.unwrap();
        sw.flush().await.unwrap();
        let ev = rx.recv().await.unwrap();
        assert!(matches!(ev, ProviderEvent::Part(PartDraft { payload: PartKind::AssistantMessage { text }, .. }) if text == "ok"));
    }

    #[tokio::test]
    async fn child_eof_mid_turn_fails_turn_before_ended() {
        let (cr, sw) = tokio::io::duplex(16384);
        let (_sr, cw) = tokio::io::duplex(16384);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let settings = SessionSettings::default();
        let provider = AntigravityProvider::from_test_io(
            PathBuf::from("/tmp"),
            tx,
            settings.clone(),
            None,
            cr,
            cw,
        );
        let tref = provider.turn("run".into(), vec![], &settings).await.unwrap();
        let _ = rx.recv().await.unwrap();
        drop(sw);
        let ev1 = rx.recv().await.unwrap();
        assert!(matches!(ev1, ProviderEvent::TurnCompleted { turn_id, error: Some(_) } if turn_id == tref.turn_id));
        let ev2 = rx.recv().await.unwrap();
        assert!(matches!(ev2, ProviderEvent::Ended { .. }));
    }

    #[tokio::test]
    async fn error_result_marks_turn_failed() {
        let (cr, mut sw) = tokio::io::duplex(16384);
        let (_sr, cw) = tokio::io::duplex(16384);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let settings = SessionSettings::default();
        let provider = AntigravityProvider::from_test_io(
            PathBuf::from("/tmp"),
            tx,
            settings.clone(),
            None,
            cr,
            cw,
        );
        let _ = provider.turn("boom".into(), vec![], &settings).await.unwrap();
        let _ = rx.recv().await.unwrap();
        sw.write_all(b"{\"type\":\"result\",\"is_error\":true,\"result\":\"quota exceeded\"}\n").await.unwrap();
        sw.flush().await.unwrap();
        let ev = rx.recv().await.unwrap();
        assert!(matches!(ev, ProviderEvent::TurnCompleted { error: Some(e), .. } if e == "quota exceeded"));
    }

    #[tokio::test]
    async fn unsupported_verbs_return_typed_errors() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let p = AntigravityProvider::spawn(PathBuf::from("/tmp"), tx, &SessionSettings::default()).unwrap();
        assert_eq!(p.steer("t", "x".into()).await.unwrap_err().code(), "STEER_UNSUPPORTED");
        assert_eq!(p.compact().await.unwrap_err().code(), "COMPACT_UNSUPPORTED");
        assert_eq!(p.rollback("t").await.unwrap_err().code(), "UNSUPPORTED_METHOD");
    }

    #[tokio::test]
    async fn clear_resets_conversation_id() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let p = AntigravityProvider::spawn(PathBuf::from("/tmp"), tx, &SessionSettings::default()).unwrap();
        let id1 = p.start(Some("old-conv".into())).await.unwrap();
        assert_eq!(id1, "old-conv");
        let id2 = p.clear().await.unwrap();
        assert_ne!(id1, id2);
        assert!(id2.starts_with("agy-"));
    }

    #[tokio::test]
    async fn tool_result_error_sets_failed_status() {
        let (cr, mut sw) = tokio::io::duplex(16384);
        let (_sr, cw) = tokio::io::duplex(16384);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _provider = AntigravityProvider::from_test_io(
            PathBuf::from("/tmp"),
            tx,
            SessionSettings::default(),
            None,
            cr,
            cw,
        );
        sw.write_all(b"{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"x\",\"content\":\"err\",\"is_error\":true}]}}\n").await.unwrap();
        sw.flush().await.unwrap();
        let ev = rx.recv().await.unwrap();
        assert!(matches!(ev, ProviderEvent::Part(PartDraft { payload: PartKind::ToolCall { status: ToolCallStatus::Failed, .. }, .. })));
    }
}
