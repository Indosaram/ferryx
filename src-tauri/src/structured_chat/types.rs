//! Wire types for the structured agent chat mode (lane G1, chat-t3-parity plan).
//!
//! Field names follow the frozen lane contract and the shapes Ferryx already puts on the wire
//! (`reference_chat` history parts) — nothing here invents a name the reference chat does not
//! use. All DTOs serialize `camelCase` like every other IPC/gateway DTO.

use serde::{Deserialize, Serialize};

/// How much of the next action the agent may take without asking (t3code `RuntimeMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    Supervised,
    AutoAcceptEdits,
    Auto,
    FullAccess,
}

impl PermissionMode {
    /// Every mode the Codex provider supports, in ascending grant order (t3code order).
    pub fn all() -> [PermissionMode; 4] {
        [
            PermissionMode::Supervised,
            PermissionMode::AutoAcceptEdits,
            PermissionMode::Auto,
            PermissionMode::FullAccess,
        ]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PermissionMode::Supervised => "supervised",
            PermissionMode::AutoAcceptEdits => "auto-accept-edits",
            PermissionMode::Auto => "auto",
            PermissionMode::FullAccess => "full-access",
        }
    }
}

/// A client's answer to one approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalDecision {
    Accept,
    AcceptForSession,
    Decline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueueDeliveryMode {
    All,
    OneAtATime,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionSettings {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: Option<PermissionMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fast_mode: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub additional_dirs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steering_mode: Option<QueueDeliveryMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_up_mode: Option<QueueDeliveryMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_compaction: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_retry: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction_mode: Option<String>,
}

/// The body of `POST /sessions`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionBody {
    pub provider: String,
    pub cwd: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default)]
    pub execution_mode: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub fast_mode: Option<bool>,
    #[serde(default)]
    pub additional_dirs: Vec<String>,
}

/// One client-supplied file attached to a turn.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRef {
    pub path: String,
}

/// The body of `POST /sessions/{id}/turns`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnBody {
    pub text: String,
    #[serde(default)]
    pub attachments: Option<Vec<AttachmentRef>>,
}

/// The body of `POST /sessions/{id}/steer`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteerBody {
    pub text: String,
}

/// The body of `POST /sessions/{id}/rollback`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackBody {
    /// How many whole trailing turns to drop from the thread history.
    pub turns: u32,
    /// Also restore the workspace files those turns changed (t3code composer.md:191-194, "Revert
    /// files too"). Only a provider that advertises `Capabilities::revert_files` can honour it; the
    /// store refuses it as a typed `REVERT_FILES_UNSUPPORTED` otherwise, so a client never believes
    /// files were restored when only the conversation was.
    #[serde(default)]
    pub revert_files: bool,
}

/// The body of `POST /sessions/{id}/approvals/{requestId}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalBody {
    pub decision: ApprovalDecision,
}

/// One model the provider listed, as the model-discovery probe reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    /// Reasoning effort ids the model accepts, empty when none are listed.
    pub efforts: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub steer: bool,
    pub rollback: bool,
    pub resume: bool,
    pub revert_files: bool,
    pub history_paging: bool,
    pub permission_modes: Vec<PermissionMode>,
    pub models: Vec<ModelInfo>,
    pub slash_commands: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub execution_modes: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub thinking_levels: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fast_mode: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub additional_dirs: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub queue_modes: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub compact_custom_instructions: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub tree_navigation: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fork: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub clone_session: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub edit_user_message: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub edit_assistant_message: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub session_rename: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub session_stats: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub session_switch: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reload: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub media: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub extension_ui: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub goals: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub favorite_models: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub scoped_models: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommandItem {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source: String,
    pub syntax: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactBody {
    #[serde(default)]
    pub custom_instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionUiResponseBody {
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub confirmed: Option<bool>,
    #[serde(default)]
    pub cancelled: Option<bool>,
    #[serde(default)]
    pub answers: Option<serde_json::Value>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTreeNode {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub role: String,
    pub preview: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTree {
    pub current_leaf_id: Option<String>,
    pub nodes: Vec<SessionTreeNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigateTreeBody {
    pub target_id: String,
    #[serde(default)]
    pub summarize: bool,
    #[serde(default)]
    pub custom_instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkBody {
    pub entry_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkResult {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditMessageBody {
    pub entry_id: String,
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameSessionBody {
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStats {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub turns: u64,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaBlob {
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPreferencesBody {
    #[serde(default)]
    pub favorite_models: Option<Vec<String>>,
    #[serde(default)]
    pub scoped_models: Option<Vec<String>>,
}

/// One change inside a [`PartKind::FileChange`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeEntry {
    pub path: String,
    pub kind: String,
    pub diff: String,
}

/// A tool call's lifecycle status, as the provider reported it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCallStatus {
    InProgress,
    Completed,
    Failed,
    Declined,
    Cancelled,
}

/// The conversation part kinds the history and event routes return. Every part is append-only
/// and carries the monotonic `seq` of its [`StructuredPart`] envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PartKind {
    UserMessage {
        text: String,
    },
    /// One assistant message: the provider's deltas merged into the final text.
    AssistantMessage {
        text: String,
    },
    Reasoning {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        status: ToolCallStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<i64>,
    },
    FileChange {
        id: String,
        status: ToolCallStatus,
        changes: Vec<FileChangeEntry>,
    },
    ApprovalRequest {
        request_id: String,
        /// `command` or `fileChange`, as the provider classified the request.
        kind: String,
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        answered: bool,
    },
    Plan {
        text: String,
    },
    Error {
        message: String,
    },
    Usage {
        total_tokens: u64,
        input_tokens: u64,
        cached_input_tokens: u64,
        output_tokens: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        context_window: Option<u64>,
    },
    /// Emitted by `/clear`: everything after this divider belongs to a new thread.
    NewSession,
    /// Emitted by `rollback`: the conversation above this divider was taken back. The marker is
    /// appended (never re-used), so `seq` stays strictly increasing across a rollback.
    Rollback {
        turns: u32,
    },
    ExtensionUiRequest {
        request_id: String,
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        prefill: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        options: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<serde_json::Value>,
        answered: bool,
    },
    ExtensionStatus {
        status_key: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        status_text: Option<String>,
    },
    ExtensionWidget {
        widget_key: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        lines: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        placement: Option<String>,
    },
    SubagentProgress {
        task_id: String,
        name: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    GoalUpdate {
        goal_id: String,
        title: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        progress: Option<String>,
    },
    MediaRef {
        tool_call_id: String,
        content_index: u32,
        mime_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
    },
}

/// One append-only history entry: a payload plus the sequence bookkeeping the routes add.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredPart {
    pub seq: u64,
    /// Provider turn this part belongs to, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub ts_ms: u64,
    #[serde(flatten)]
    pub payload: PartKind,
}

/// The lifecycle state of one session as `GET /sessions` and history readers see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    /// A provider process owns the thread in this gateway process.
    Running,
    /// The process is gone (gateway restarted or session closed); a new turn resumes the thread.
    Ended,
}

/// A session row as the routes address it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub session_id: String,
    pub provider: String,
    pub cwd: String,
    pub state: SessionState,
    pub settings: SessionSettings,
    /// The provider turn id in flight, when one is. The composer reads this to enable Stop
    /// instead of guessing from history; absent (not `null`) when no turn is running, so a
    /// session row keeps its five-field shape while it is idle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_turn_id: Option<String>,
}

/// Typed structured-chat failures the routes map to HTTP statuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuredChatError {
    /// Malformed body, unknown provider, non-existent cwd. HTTP 400.
    InvalidRequest(String),
    /// No such session. HTTP 404.
    NotFound,
    /// The operation cannot run now: no active turn, a turn already active. HTTP 409.
    Conflict(String),
    /// The provider cannot do this at all (typed `STEER_UNSUPPORTED` / `COMPACT_UNSUPPORTED`).
    Unsupported(String),
    /// The provider process failed or the child died. HTTP 502.
    Provider(String),
}

impl StructuredChatError {
    /// The machine code the routes carry beside the status. Typed unsupported codes name the
    /// frozen wire strings `STEER_UNSUPPORTED` / `COMPACT_UNSUPPORTED`.
    pub fn code(&self) -> &'static str {
        match self {
            StructuredChatError::InvalidRequest(_) => "INVALID_REQUEST",
            StructuredChatError::NotFound => "NOT_FOUND",
            StructuredChatError::Conflict(_) => "CONFLICT",
            StructuredChatError::Unsupported(message) => {
                for code in [
                    "COMPACT_UNSUPPORTED",
                    "STEER_UNSUPPORTED",
                    "REVERT_FILES_UNSUPPORTED",
                    "RESUME_UNSUPPORTED",
                    "EXTENSION_UI_UNSUPPORTED",
                    "TREE_UNSUPPORTED",
                    "FORK_UNSUPPORTED",
                    "CLONE_UNSUPPORTED",
                    "EDIT_MESSAGE_UNSUPPORTED",
                    "RENAME_UNSUPPORTED",
                    "STATS_UNSUPPORTED",
                    "RELOAD_UNSUPPORTED",
                    "MEDIA_UNSUPPORTED",
                    "CLEAR_QUEUE_UNSUPPORTED",
                    "MODEL_PREFERENCES_UNSUPPORTED",
                ] {
                    if message.starts_with(code) {
                        return code;
                    }
                }
                "UNSUPPORTED_METHOD"
            }
            StructuredChatError::Provider(_) => "PROVIDER_FAILED",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            StructuredChatError::InvalidRequest(m)
            | StructuredChatError::Conflict(m)
            | StructuredChatError::Unsupported(m)
            | StructuredChatError::Provider(m) => m,
            StructuredChatError::NotFound => "no such session",
        }
    }
}



impl std::fmt::Display for StructuredChatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for StructuredChatError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_modes_round_trip_the_wire_spelling() {
        for mode in PermissionMode::all() {
            let json = serde_json::to_value(mode).unwrap();
            assert_eq!(json, serde_json::json!(mode.as_str()));
            assert_eq!(serde_json::from_value::<PermissionMode>(json).unwrap(), mode);
        }
        assert_eq!(
            serde_json::from_str::<PermissionMode>("\"auto-accept-edits\"").unwrap(),
            PermissionMode::AutoAcceptEdits
        );
    }

    #[test]
    fn parts_flatten_their_kind_and_skip_absent_fields() {
        let part = StructuredPart {
            seq: 7,
            turn_id: Some("t".into()),
            ts_ms: 1,
            payload: PartKind::ToolCall {
                id: "i".into(),
                name: "shell".into(),
                status: ToolCallStatus::Completed,
                output: None,
                exit_code: Some(0),
                duration_ms: Some(12),
            },
        };
        let json = serde_json::to_value(&part).unwrap();
        assert_eq!(json["type"], "toolCall");
        assert_eq!(json["seq"], 7);
        assert_eq!(json["exitCode"], 0);
        assert!(json.get("output").is_none());
        let back: StructuredPart = serde_json::from_value(json).unwrap();
        assert_eq!(back, part);
    }

    #[test]
    fn error_codes_match_the_frozen_wire_names() {
        assert_eq!(
            StructuredChatError::Unsupported("COMPACT_UNSUPPORTED: no compact method".into()).code(),
            "COMPACT_UNSUPPORTED"
        );
        assert_eq!(
            StructuredChatError::Unsupported("STEER_UNSUPPORTED: no turn/steer".into()).code(),
            "STEER_UNSUPPORTED"
        );
        assert_eq!(
            StructuredChatError::Unsupported("UNSUPPORTED_METHOD".into()).code(),
            "UNSUPPORTED_METHOD"
        );
        assert_eq!(StructuredChatError::NotFound.code(), "NOT_FOUND");
    }
}
