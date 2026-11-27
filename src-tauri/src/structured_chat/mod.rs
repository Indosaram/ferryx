//! Structured agent chat (lane G1, chat-t3-parity plan): agent sessions that talk to a provider
//! binary over its structured protocol, instead of reading a PTY transcript the way the
//! reference chat does.
//!
//! Layout:
//! * [`types`] — wire DTOs (parts, settings, bodies) shared by the HTTP routes and Tauri IPC.
//! * [`codex`] — the Codex `app-server` JSON-RPC provider (this lane).
//! * [`store`] — the session store: journal persistence, sequence numbers, long-poll events.
//!
//! `src-tauri/src/ipc/structured_chat.rs` mirrors the routes as Tauri commands, and
//! `src-tauri/src/remote/server.rs` mounts them under `/api/v1/structured-chat/`.
//!
//! ## The provider contract (wave 2: the Claude lane adds `claude.rs` against this trait)
//!
//! [`StructuredProvider`] is deliberately provider-neutral. An implementation owns exactly one
//! provider process (or connection) for one session working directory and:
//!
//! * reports a stable [`StructuredProvider::provider_id`] (`"codex"`, `"claude"`, …) the store
//!   dispatches session creation on;
//! * is constructed with the session's event channel: every provider→client fact flows out as a
//!   [`ProviderEvent`], never through a shared mutable side channel. Part payloads are
//!   provider-shaped ([`types::PartKind`]); the store stamps `seq`/`ts` and journals them, so a
//!   provider must not number parts itself;
//! * maps the store's provider-agnostic verbs (`start`/`turn`/`steer`/`interrupt`/`rollback`/
//!   `answer_approval`/`clear`/`compact`/`close`) onto its protocol, and reports which protocol
//!   method each verb maps to in its doc comments. A verb the protocol cannot express returns
//!   [`StructuredChatError::Unsupported`] with a `STEER_UNSUPPORTED`-style machine code, which
//!   the routes render as a typed 409;
//! * reports approvals as [`PartKind::ApprovalRequest`] parts whose `requestId` the routes can
//!   pass back to [`StructuredProvider::answer_approval`];
//! * survives a gateway restart: the store only needs [`StructuredProvider::start`] with the
//!   persisted thread id to resume a thread;
//! * never shells out through a string: providers spawn argv arrays, and the store validates the
//!   cwd before construction.
//!
//! Anything provider-specific stays inside the provider module, including how permission modes
//! map onto native approval/sandbox policies.

pub mod acp;
pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod maho_rpc;
pub mod revert_files;
pub mod store;
pub mod types;

#[cfg(test)]
#[path = "claude_falsify.rs"]
mod claude_falsify;

use async_trait::async_trait;
use tokio::sync::mpsc;

pub use types::{
    ApprovalBody, ApprovalDecision, AttachmentRef, Capabilities, CompactBody, CreateSessionBody,
    EditMessageBody, ExtensionUiResponseBody, FileChangeEntry, ForkBody, ForkResult, MediaBlob,
    ModelInfo, ModelPreferencesBody, NavigateTreeBody, PartKind, PermissionMode, QueueDeliveryMode,
    RenameSessionBody, RollbackBody, SessionSettings, SessionState, SessionStats, SessionSummary,
    SessionTree, SessionTreeNode, SlashCommandItem, SteerBody, StructuredChatError, StructuredPart,
    ToolCallStatus, TurnBody,
};

/// A provider-emitted part before the store stamps the envelope fields.
#[derive(Debug, Clone, PartialEq)]
pub struct PartDraft {
    /// Provider turn the part belongs to, when the provider knows it yet.
    pub turn_id: Option<String>,
    pub payload: PartKind,
}

/// Everything a provider reports back through its event channel.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    Part(PartDraft),
    /// A turn began on the provider side.
    TurnStarted { turn_id: String },
    /// A turn finished; `error` carries the failure text when it did not complete.
    TurnCompleted { turn_id: String, error: Option<String> },
    /// The provider process is gone (exit, EOF). The session becomes `ended`.
    Ended { reason: String },
}

/// A turn the provider started, as steer/interrupt bookkeeping addresses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRef {
    pub turn_id: String,
}

/// One structured-chat provider. See the module docs for the contract every implementation and
/// the wave-2 Claude lane must keep.
#[async_trait]
pub trait StructuredProvider: Send + Sync {
    /// Stable provider id session creation dispatches on.
    fn provider_id(&self) -> &'static str;

    /// What this provider supports, before any thread exists. `models` may stay empty when
    /// discovery needs a separate probe; the store fills it through the provider module.
    fn capabilities(&self) -> Capabilities;

    /// Opens (or resumes) the provider thread and returns its thread id. `resume_thread` is the
    /// id persisted by a previous gateway process.
    async fn start(&self, resume_thread: Option<String>) -> Result<String, StructuredChatError>;

    /// Starts one user turn and returns the provider turn id. Applies `settings` (model,
    /// effort, permission mode) from this turn on.
    async fn turn(
        &self,
        text: String,
        attachments: Vec<String>,
        settings: &SessionSettings,
    ) -> Result<TurnRef, StructuredChatError>;

    /// Steers the active turn mid-flight; `expected_turn` is the active turn's id.
    async fn steer(&self, expected_turn: &str, text: String) -> Result<(), StructuredChatError>;

    /// Interrupts one running turn.
    async fn interrupt(&self, turn_id: &str) -> Result<(), StructuredChatError>;

    /// Drops the thread history from `before_turn_id` (inclusive) on, and returns the id of the
    /// conversation the provider now speaks for.
    ///
    /// That id is usually the same one the caller passed, but a provider that can only express a
    /// rollback as a *reopen* (Claude relaunches with `--resume-session-at`, and a rollback of the
    /// first turn leaves nothing to resume) comes back with a different one. The store persists
    /// whatever comes back, so a later `start(Some(id))` resumes the conversation the user is
    /// actually looking at instead of the one they rolled away from.
    async fn rollback(&self, before_turn_id: &str) -> Result<String, StructuredChatError>;

    /// Answers one pending approval request the provider emitted.
    async fn answer_approval(
        &self,
        request_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), StructuredChatError>;

    /// Starts a fresh thread in the same session (`/clear`). The store appends the
    /// `newSession` divider part.
    async fn clear(&self) -> Result<String, StructuredChatError>;

    /// Compacts the thread, when the protocol has a compact method.
    async fn compact(&self) -> Result<(), StructuredChatError>;

    /// Continues a stopped run on the same thread, without sending a new prompt. t3code's
    /// composer "Resume" uses the provider's native continuation. Providers whose protocol has no
    /// continuation method return [`StructuredChatError::Unsupported`] and advertise
    /// `Capabilities::resume == false`, so the UI falls back to sending a message instead.
    async fn resume(&self) -> Result<(), StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "RESUME_UNSUPPORTED: this provider has no native continuation".into(),
        ))
    }

    async fn compact_with_instructions(
        &self,
        _instructions: Option<&str>,
    ) -> Result<(), StructuredChatError> {
        self.compact().await
    }

    async fn commands(&self) -> Result<Vec<SlashCommandItem>, StructuredChatError> {
        Ok(self
            .capabilities()
            .slash_commands
            .into_iter()
            .map(|name| SlashCommandItem {
                syntax: if name.starts_with('$') { "dollar".into() } else { "slash".into() },
                name: name.trim_start_matches(['/', '$']).to_owned(),
                description: None,
                source: "builtin".into(),
            })
            .collect())
    }

    async fn answer_extension_ui(
        &self,
        _request_id: &str,
        _response: ExtensionUiResponseBody,
    ) -> Result<(), StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "EXTENSION_UI_UNSUPPORTED: provider does not accept extension UI answers".into(),
        ))
    }

    async fn get_tree(&self) -> Result<SessionTree, StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "TREE_UNSUPPORTED: provider has no session tree".into(),
        ))
    }

    async fn navigate_tree(
        &self,
        _body: &NavigateTreeBody,
    ) -> Result<(), StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "TREE_UNSUPPORTED: provider has no session tree navigation".into(),
        ))
    }

    async fn fork(&self, _entry_id: &str) -> Result<String, StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "FORK_UNSUPPORTED: provider does not support forking".into(),
        ))
    }

    async fn clone_session(&self) -> Result<String, StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "CLONE_UNSUPPORTED: provider does not support cloning".into(),
        ))
    }

    async fn edit_message(&self, _body: &EditMessageBody) -> Result<(), StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "EDIT_MESSAGE_UNSUPPORTED: provider does not support message editing".into(),
        ))
    }

    async fn rename_session(&self, _name: &str) -> Result<(), StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "RENAME_UNSUPPORTED: provider does not support session rename".into(),
        ))
    }

    async fn session_stats(&self) -> Result<SessionStats, StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "STATS_UNSUPPORTED: provider does not expose session stats".into(),
        ))
    }

    async fn reload(&self) -> Result<(), StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "RELOAD_UNSUPPORTED: provider does not support reload".into(),
        ))
    }

    async fn get_media(
        &self,
        _tool_call_id: &str,
        _content_index: u32,
    ) -> Result<MediaBlob, StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "MEDIA_UNSUPPORTED: provider does not expose tool media".into(),
        ))
    }

    async fn clear_queue(&self) -> Result<(), StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "CLEAR_QUEUE_UNSUPPORTED: provider has no server-side queue".into(),
        ))
    }

    async fn set_model_preferences(
        &self,
        _body: &ModelPreferencesBody,
    ) -> Result<(), StructuredChatError> {
        Err(StructuredChatError::Unsupported(
            "MODEL_PREFERENCES_UNSUPPORTED: provider does not persist model lists".into(),
        ))
    }

    /// Stops the provider and kills its process. Idempotent.
    async fn close(&self);

    /// The channel provider events flow out of, as handed to the constructor.
    fn event_sink(&self) -> mpsc::UnboundedSender<ProviderEvent>;
}
