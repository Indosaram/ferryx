//! Frozen Herdr reference-chat contract types (task 1).
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
//! (MIT, see `docs/chat/HERDR_LICENSE`). The upstream sources that define these shapes
//! are `shared/protocol.ts` (`ConversationTurn`, `ConversationPart`,
//! `ConversationMetadata`, `InteractivePrompt`, `PromptAnswer`), `server/conversation.ts`
//! (`RecognizedConversation`, `ConversationPage`, cursor/`history_id` semantics) and
//! `src/lib/compose.ts` (submit byte shaping).
//!
//! This module is **types only**: it declares the shared wire shapes, the identity and
//! receipt helpers every lane consumes, and the public signatures the provider family
//! lanes implement. It opens no socket, spawns no process and reads no file.
//!
//! Frozen contract: `docs/chat/herdr-port-contract.md`. Do not add, rename or re-shape a
//! wire field without a new revision of that file.
//!
//! Identity, error, result and delivery-receipt types are **reused** from
//! [`crate::scoped_contracts`], never duplicated here.

use serde::{Deserialize, Serialize};

use crate::scoped_contracts::{AttachmentMediaType, AttachmentReceipt, DeliveryStage, TargetRef};

/// Route prefix for the reference-chat surface. Task 13 registers the routes.
pub const REFERENCE_CHAT_ROUTE_PREFIX: &str = "/api/v1/reference-chat";

/// The reference's own copy for a pane whose turns are same-pane output rather than a
/// native transcript (`ChatView.tsx` details summary). Kept byte-exact on purpose: it is
/// the disclosure the user reads, and it must not be re-worded into a promise.
pub const REFERENCE_SCROLLBACK_DISCLOSURE: &str =
    "Conversation unavailable — show terminal output";

/// The reference's composer cap (`compose.ts`: `MAX_COMPOSER_CHARS`), still subject to the
/// existing transport byte limit.
pub const REFERENCE_SUBMIT_MAX_CHARS: usize = 20_000;

/// How many provider-native history families the reference recognises. Six readers plus
/// `Unavailable`; the 24-row action matrix lives in the contract document, not here.
pub const REFERENCE_NATIVE_HISTORY_KINDS: usize = 6;

/// A reference-chat target: the owning host + backend session + daemon incarnation, plus
/// the provider/native session identity **only where the reader identified one**.
///
/// Flattened so the wire object is the scoped `TargetRef` with one optional extra field:
/// `{ hostId, ownerId, epoch, backendSessionId, providerSessionId? }`.
///
/// A visual leaf id is never a target. `providerSessionId: None` means *unknown* — it is
/// never permission to read another session's file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceTargetRef {
    #[serde(flatten)]
    pub target: TargetRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
}

impl ReferenceTargetRef {
    /// A target whose provider session identity is not known.
    pub fn without_provider_session(target: TargetRef) -> Self {
        Self { target, provider_session_id: None }
    }

    /// A target bound to a reader-identified provider session.
    pub fn with_provider_session(target: TargetRef, provider_session_id: impl Into<String>) -> Self {
        Self { target, provider_session_id: Some(provider_session_id.into()) }
    }

    /// Does this target carry a reader-identified provider session?
    ///
    /// Callers use this to decide whether `providerRead` may ever be claimed: with no
    /// provider session identity there is nothing to match a native observation against.
    pub fn has_provider_session(&self) -> bool {
        self.provider_session_id.as_deref().is_some_and(|id| !id.trim().is_empty())
    }
}

/// Where a conversation page's turns came from.
///
/// Kebab-case on the wire, matching the upstream reader names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceHistorySource {
    ClaudeTranscript,
    CodexTranscript,
    OmpTranscript,
    OmoTranscript,
    GjcTranscript,
    PiTranscript,
    /// Same-pane output for a pane no native reader knows. A legitimate primary result,
    /// not a failure — and it must be disclosed as such.
    Scrollback,
}

impl ReferenceHistorySource {
    /// The native reader this source belongs to, or `Unavailable` for scrollback.
    pub fn native_kind(self) -> ReferenceNativeHistoryKind {
        match self {
            Self::ClaudeTranscript => ReferenceNativeHistoryKind::Claude,
            Self::CodexTranscript => ReferenceNativeHistoryKind::Codex,
            Self::OmpTranscript => ReferenceNativeHistoryKind::Omp,
            Self::OmoTranscript => ReferenceNativeHistoryKind::Omo,
            Self::GjcTranscript => ReferenceNativeHistoryKind::Gjc,
            Self::PiTranscript => ReferenceNativeHistoryKind::Pi,
            Self::Scrollback => ReferenceNativeHistoryKind::Unavailable,
        }
    }

    /// The wire name of the reader, for logs and capability matrices.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeTranscript => "claude-transcript",
            Self::CodexTranscript => "codex-transcript",
            Self::OmpTranscript => "omp-transcript",
            Self::OmoTranscript => "omo-transcript",
            Self::GjcTranscript => "gjc-transcript",
            Self::PiTranscript => "pi-transcript",
            Self::Scrollback => "scrollback",
        }
    }
}

/// How honestly the page's turns are sourced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceHistoryAvailability {
    /// An exact native file was resolved for this target.
    Native,
    /// Explicit same-pane output. Disclosure required.
    Scrollback,
    /// The pane's agent holds a session it has not written yet: a conversation with zero
    /// turns, **not** a missing one.
    NotStarted,
}

/// Which provider-native reader a lane implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceNativeHistoryKind {
    Claude,
    Codex,
    Omp,
    Omo,
    Gjc,
    Pi,
    /// No native reader for this registry entry today. A boundary, not a permanent denial.
    Unavailable,
}

impl ReferenceNativeHistoryKind {
    /// Map a **registry id** to its native reader.
    ///
    /// This is the only label→reader mapping the contract allows, and it is keyed on the
    /// registry id the inventory already publishes — never on a terminal title. An OmO pane
    /// whose label flips between `pi` and `claude` is routed by process/session evidence,
    /// which is why `omo` must be selected by registry id here and confirmed by the caller.
    pub fn from_registry_id(registry_id: &str) -> Self {
        match registry_id.trim().to_ascii_lowercase().as_str() {
            "claude" => Self::Claude,
            "codex" => Self::Codex,
            "omp" => Self::Omp,
            "omo" => Self::Omo,
            "gjc" => Self::Gjc,
            "pi" => Self::Pi,
            _ => Self::Unavailable,
        }
    }

    /// Does this registry entry have a native reader at all?
    pub fn is_native(self) -> bool {
        !matches!(self, Self::Unavailable)
    }

    /// The wire name of the reader family.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude-transcript",
            Self::Codex => "codex-transcript",
            Self::Omp => "omp-transcript",
            Self::Omo => "omo-transcript",
            Self::Gjc => "gjc-transcript",
            Self::Pi => "pi-transcript",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Which seat a turn's content came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceTurnRole {
    User,
    Assistant,
}

/// Whether a human typed the turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceTurnSource {
    /// Someone typed it.
    Typed,
    /// The agent's runtime put a message in the user's seat (a background-job result).
    /// It starts a turn; nobody typed it.
    Runtime,
}

/// The discriminant of [`ReferencePart`]; kept as its own enum so a part can be classified
/// without destructuring its payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferencePartKind {
    Text,
    Thinking,
    Skill,
    Tool,
    Image,
    Compact,
    Notice,
    TaskResult,
}

/// Evidence of skill activity, not a claim that the skill's workflow completed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceSkillActivity {
    pub name: String,
    pub evidence: ReferenceSkillEvidence,
    pub status: ReferenceSkillStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceSkillEvidence {
    Invocation,
    Instructions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceSkillStatus {
    Requested,
    Loaded,
    Failed,
}

/// One OmO background task that ended, as OmO reported it back to the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceTaskResult {
    pub id: String,
    /// The summary the `task` call gave it, else its name, else the agent it ran as, else its id.
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub status: ReferenceTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    /// The task's last answer, or why it failed.
    pub result: String,
    /// Set when `result` was cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_cut: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceTaskStatus {
    Completed,
    Failed,
    Cancelled,
}

/// One part of a turn. The reference's `ConversationPart` union, ported field-for-field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ReferencePart {
    /// Prose. `phase` distinguishes an interim commentary line from the turn's answer.
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<ReferenceTextPhase>,
    },
    /// The agent's reasoning block; the client folds it and shows it only on request.
    Thinking { text: String },
    /// A skill the transcript recorded on its own, beside the text it was invoked with.
    ///
    /// The pinned readers push it in three places: the shared omp/omo/gjc/pi record loop pushes
    /// one per skill a user prompt invoked (`transcript-records.ts:259`), and Codex pushes one for
    /// a completed read no tool part carries and for its explicitly selected skill
    /// (`codex.ts:241,275`). A skill the reader attached to a *tool call* stays on that call
    /// ([`ReferencePart::Tool::skill`]) and is never duplicated here.
    ///
    /// The turn's skill list draws it; the inline part list does not, because the reference
    /// filters `skill` out of the parts it renders in order (`ChatView.tsx:322`).
    Skill { skill: ReferenceSkillActivity },
    /// A tool call. `output_ref`/`output_size` are set when `output` was cut, so the whole
    /// output can be fetched on demand.
    Tool {
        name: String,
        summary: String,
        input: String,
        output: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skill: Option<ReferenceSkillActivity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_size: Option<u64>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ReferenceImageRef>,
    },
    /// A native user image, addressed by an opaque ref and fetched on demand.
    Image { media_type: String, r#ref: String },
    /// The summary a compaction left; the conversation before it is what it sums up.
    Compact { text: String },
    /// A message the agent's runtime put in the user's seat. `source` is the runtime's own
    /// name for it.
    Notice {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    },
    /// OmO's background tasks that ended.
    TaskResult { tasks: Vec<ReferenceTaskResult> },
}

impl ReferencePart {
    /// The part's discriminant.
    pub fn kind(&self) -> ReferencePartKind {
        match self {
            Self::Text { .. } => ReferencePartKind::Text,
            Self::Thinking { .. } => ReferencePartKind::Thinking,
            Self::Skill { .. } => ReferencePartKind::Skill,
            Self::Tool { .. } => ReferencePartKind::Tool,
            Self::Image { .. } => ReferencePartKind::Image,
            Self::Compact { .. } => ReferencePartKind::Compact,
            Self::Notice { .. } => ReferencePartKind::Notice,
            Self::TaskResult { .. } => ReferencePartKind::TaskResult,
        }
    }

    /// Does this part carry text the chat can render as prose without a fetch?
    pub fn is_inline_text(&self) -> bool {
        matches!(self, Self::Text { .. } | Self::Thinking { .. } | Self::Compact { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceTextPhase {
    Commentary,
    FinalAnswer,
}

/// An opaque image reference, fetched through the owning host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceImageRef {
    pub media_type: String,
    pub r#ref: String,
}

/// Turns a `/tree` walked away from. No page of the live conversation can reach them, so
/// they are disclosed rather than dropped in silence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceAbandonedBranch {
    pub count: u64,
    pub branches: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// One turn of a structured agent conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceTurn {
    pub role: ReferenceTurnRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// Last recorded assistant activity — never the next user's timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ReferenceTurnSource>,
    pub parts: Vec<ReferencePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abandoned: Option<ReferenceAbandonedBranch>,
}

/// A position in a transcript stream: which stream, and where inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceHistoryCursor {
    /// Identity of the transcript the cursor was minted against.
    pub stream_id: String,
    /// Byte offset inside that stream.
    pub offset: u64,
}

impl ReferenceHistoryCursor {
    /// A cursor only names a position while the stream it was minted against is still the
    /// live stream. A cursor from another stream is refused, never silently re-anchored.
    pub fn matches_stream(&self, stream_id: &str) -> bool {
        self.stream_id == stream_id
    }
}

/// One page of conversation, plus the identity the client fences it with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceHistoryPage {
    pub source: ReferenceHistorySource,
    pub availability: ReferenceHistoryAvailability,
    pub turns: Vec<ReferenceTurn>,
    /// The page before this one; `None` at the conversation's beginning or for scrollback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ReferenceHistoryCursor>,
    pub has_more: bool,
    /// Changes whenever the answer could. A response whose generation is not the requested
    /// one is discarded, never painted.
    pub generation: String,
    /// Required when `availability` is not `native`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

impl ReferenceHistoryPage {
    /// Are these turns a native transcript?
    pub fn is_native(&self) -> bool {
        self.availability == ReferenceHistoryAvailability::Native
    }

    /// The disclosure the UI must show, or `None` when nothing needs disclosing.
    ///
    /// Native pages disclose nothing. Everything else is labelled: a scrollback page is
    /// same-pane output, and a not-started session is a conversation with no turns yet.
    pub fn disclosure(&self) -> Option<&'static str> {
        match self.availability {
            ReferenceHistoryAvailability::Native => None,
            ReferenceHistoryAvailability::Scrollback => Some(REFERENCE_SCROLLBACK_DISCLOSURE),
            ReferenceHistoryAvailability::NotStarted => Some("This session has not written a turn yet."),
        }
    }

    /// May the delivery ladder ever reach `providerRead` for this page?
    ///
    /// Only a native page bound to a reader-identified provider session can be matched
    /// against a native observation. Generic sources stop at `accepted`.
    pub fn can_reach_provider_read(&self, target: &ReferenceTargetRef) -> bool {
        self.is_native() && target.has_provider_session()
    }
}

/// A bounded, VT-aware snapshot of the original pane.
///
/// Reading never emits input and never resizes: the mirror is fed history and asked for a
/// frame at the pane's own geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceScreenSnapshot {
    /// Changes whenever the visible screen changes; an answer names the revision it saw.
    pub revision: String,
    pub text: String,
    pub truncated: bool,
    /// Replay gap: the reader could not reconstruct the history it was asked for.
    pub gap: bool,
    pub cols: u16,
    pub rows: u16,
}

impl ReferenceScreenSnapshot {
    /// Is this snapshot usable as the basis for a prompt answer?
    ///
    /// A gapped or truncated snapshot cannot carry a screen revision an answer may be
    /// validated against.
    pub fn is_answerable(&self) -> bool {
        !self.gap && !self.truncated && !self.text.is_empty()
    }
}

/// Which surface the submit came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceSubmitOrigin {
    Chat,
    /// The explicit terminal's own raw input path. It is deliberately not refused while an
    /// agent waits for an answer: that is how the user answers in the terminal.
    Terminal,
}

/// `POST /submit` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceSubmitPayload {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
    pub origin: ReferenceSubmitOrigin,
}

/// What the caller observed about the pane's ability to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceStopCapability {
    /// The pane's program handles an interrupt key. The reference sends `Escape`.
    ProviderInterrupt,
    /// The explicit terminal's deliberate signal. Kept separate from chat Stop.
    ShellSignal,
    /// The capability is unknown for this target.
    Refused,
}

impl ReferenceStopCapability {
    /// A refusal is a typed answer, not a failure to try harder.
    pub fn is_refusal(self) -> bool {
        matches!(self, Self::Refused)
    }
}

/// `POST /stop` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceStopPayload {
    pub capability: ReferenceStopCapability,
}

/// The reference's `InteractivePrompt.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferencePromptKind {
    Question,
    Approval,
    Plan,
    Menu,
}

/// One selectable option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferencePromptOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One question of a form that asks several at once, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferencePromptStep {
    pub label: String,
    pub answered: bool,
    pub current: bool,
}

/// A prompt detected on the original pane's current screen.
///
/// `id` names what the prompt says and which asking of it this is: an answer names it, and
/// one whose prompt changed between the read and the click is refused instead of misfired.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferencePrompt {
    pub id: String,
    pub agent: String,
    pub kind: ReferencePromptKind,
    pub title: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub options: Vec<ReferencePromptOption>,
    pub multi_select: bool,
    /// Index of the "type your own answer" option, when the menu has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_option_index: Option<u32>,
    /// A question queued while the agent keeps working.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued: Option<ReferencePromptQueueState>,
    /// The questions of a form that asks several at once, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<ReferencePromptStep>,
    /// The last-resort card for a blocked pane no reader knows: answered with its own
    /// buttons only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<bool>,
}

impl ReferencePrompt {
    /// The option indices a typed number may pick, in the order the agent shows them.
    pub fn selectable_indices(&self) -> Vec<usize> {
        (0..self.options.len())
            .filter(|index| Some(*index as u32) != self.custom_option_index)
            .collect()
    }

    /// Does an answer to this prompt need an explicit confirm before it goes out?
    ///
    /// A typed message that picks an approval's, a plan's or a menu's option could act on a
    /// stray "yes" or "1". A tap on an option in the card is explicit already.
    pub fn needs_confirmation(&self, answer: &ReferencePromptAnswer) -> bool {
        matches!(
            self.kind,
            ReferencePromptKind::Approval | ReferencePromptKind::Plan | ReferencePromptKind::Menu
        ) && answer.option_index.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferencePromptQueueState {
    /// Collapsed: a message typed in the chat still goes to the agent.
    Collapsed,
    /// Open in the terminal: the queue holds the input, so the chat sends nothing until it
    /// is answered or closed.
    Open,
}

/// The user's answer. Exactly one of `option_index` / `option_indices` / `custom_text`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferencePromptAnswer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_indices: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_text: Option<String>,
}

impl ReferencePromptAnswer {
    /// How many of the three mutually exclusive shapes were set.
    pub fn variant_count(&self) -> usize {
        usize::from(self.option_index.is_some())
            + usize::from(self.option_indices.is_some())
            + usize::from(self.custom_text.is_some())
    }

    /// Is this exactly one answer shape?
    ///
    /// Zero shapes is an empty answer; two or more is ambiguous and must be refused rather
    /// than resolved by precedence.
    pub fn is_single_choice(&self) -> bool {
        self.variant_count() == 1
    }
}

/// `POST /answer` payload. Binds the answer to the exact card the user saw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferencePromptAnswerPayload {
    pub prompt_id: String,
    /// The screen revision the card was rendered from. The current screen is re-read
    /// immediately before the serialized answer keys; a changed screen is refused.
    pub screen_revision: String,
    pub answer: ReferencePromptAnswer,
}

/// One step of a key sequence that answers a prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceKeyStep {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl ReferenceKeyStep {
    /// A step that types literal text.
    pub fn typed(text: impl Into<String>) -> Self {
        Self { keys: Vec::new(), text: Some(text.into()) }
    }

    /// A step that presses named keys.
    pub fn keys<K: Into<String>>(keys: impl IntoIterator<Item = K>) -> Self {
        Self { keys: keys.into_iter().map(Into::into).collect(), text: None }
    }

    /// Does this step do anything at all?
    pub fn is_effective(&self) -> bool {
        !self.keys.is_empty() || self.text.as_deref().is_some_and(|text| !text.is_empty())
    }
}

/// `POST /files` payload: a bounded file staged on the **owning** host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceFileStagePayload {
    pub name: String,
    pub media_type: AttachmentMediaType,
    pub size_bytes: u64,
    pub content_base64: String,
}

/// A staged file, as the chat refers to it.
///
/// `receipt` is the existing owner-private [`AttachmentReceipt`] — the contract adds no
/// second receipt type. `mention_text` is the editable `@path ` text that reaches the
/// agent through the ordinary submit path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceFileReceipt {
    pub receipt: AttachmentReceipt,
    pub display_name: String,
    pub mention_text: String,
}

impl ReferenceFileReceipt {
    /// The `@path ` mention for a staged file: plain text the user can still edit before
    /// sending, never an opaque managed-turn attachment.
    pub fn mention_for(path: &str) -> String {
        format!("@{path} ")
    }
}

/// Where a delivery stage sits on the ladder: staged < accepted < providerRead.
pub fn reference_stage_rank(stage: DeliveryStage) -> u8 {
    match stage {
        DeliveryStage::Staged => 0,
        DeliveryStage::Accepted => 1,
        DeliveryStage::ProviderRead => 2,
    }
}

/// Has the delivery reached at least `floor`?
///
/// Callers ask this instead of comparing variants, so a later stage inserted into the ladder
/// cannot silently pass an equality check written against today's two variants.
pub fn reference_stage_at_least(stage: DeliveryStage, floor: DeliveryStage) -> bool {
    reference_stage_rank(stage) >= reference_stage_rank(floor)
}

/// The reference-chat route for one session, e.g. `/api/v1/reference-chat/abc/history`.
pub fn reference_chat_route(session_id: &str, suffix: &str) -> String {
    let suffix = suffix.trim_matches('/');
    if suffix.is_empty() {
        format!("{REFERENCE_CHAT_ROUTE_PREFIX}/{session_id}")
    } else {
        format!("{REFERENCE_CHAT_ROUTE_PREFIX}/{session_id}/{suffix}")
    }
}

/// The error code that means "the mutation may have happened, and we cannot tell".
///
/// The accept-then-unknown idiom: a writer deadline or a transport loss **after** the write
/// was dispatched is not a failure to retry automatically. The pending record is held until
/// the caller resolves it.
pub const REFERENCE_OUTCOME_UNKNOWN_CODE: &str = "OPERATION_OUTCOME_UNKNOWN";

/// Does this wire error code mean the outcome is unknown?
pub fn reference_is_outcome_unknown(code: &str) -> bool {
    code == REFERENCE_OUTCOME_UNKNOWN_CODE
}

/// The durable draft key for a target, mirroring `drafts.ts`'s `draftKey`.
///
/// Owner-scoped: two panes on the same host with the same session id but a different daemon
/// incarnation are different drafts.
pub fn reference_draft_key(target: &ReferenceTargetRef) -> String {
    format!(
        "{}|{}|{}|{}",
        target.target.host_id, target.target.owner_id, target.target.epoch.0, target.target.backend_session_id
    )
}

/// Parse one native history family's bytes into turns.
///
/// Implemented by the family lanes (tasks 17–21) and the shared dispatcher (task 2). A lane
/// handles exactly one [`ReferenceNativeHistoryKind`]; `Unavailable` is an error, never an
/// empty success, because an empty success would be read as "this session has no turns".
pub type ReferenceHistoryParser =
    fn(kind: ReferenceNativeHistoryKind, text: &str) -> Result<Vec<ReferenceTurn>, String>;

/// Detect a structured prompt on a screen.
///
/// Implemented by the detector families (tasks 22–26) and dispatched by task 9. `agent` is
/// the registry id; a detector must not claim a provider the reference does not name.
pub type ReferencePromptDetector = fn(agent: &str, screen: &str) -> Option<ReferencePrompt>;

/// Turn an answer into the keys that answer the prompt on the original pane.
pub type ReferenceAnswerPlanner =
    fn(prompt: &ReferencePrompt, answer: &ReferencePromptAnswer) -> Result<Vec<ReferenceKeyStep>, String>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoped_contracts::Epoch;
    use serde_json::json;

    fn target() -> TargetRef {
        TargetRef {
            host_id: "host-a".into(),
            owner_id: "owner-a".into(),
            epoch: Epoch(u64::MAX),
            backend_session_id: "sess-1".into(),
        }
    }

    fn roundtrip<T: serde::de::DeserializeOwned + serde::Serialize>(wire: serde_json::Value) {
        // Given expected machine JSON, when decoded/encoded, then its shape is exact.
        let dto: T = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(dto).unwrap(), wire);
    }

    #[test]
    fn reference_target_roundtrips_with_and_without_provider_session() {
        roundtrip::<ReferenceTargetRef>(json!({
            "hostId": "host-a", "ownerId": "owner-a",
            "epoch": u64::MAX.to_string(), "backendSessionId": "sess-1"
        }));
        roundtrip::<ReferenceTargetRef>(json!({
            "hostId": "host-a", "ownerId": "owner-a",
            "epoch": u64::MAX.to_string(), "backendSessionId": "sess-1",
            "providerSessionId": "provider-7"
        }));
    }

    #[test]
    fn target_without_provider_session_can_never_claim_provider_read() {
        let bare = ReferenceTargetRef::without_provider_session(target());
        let bound = ReferenceTargetRef::with_provider_session(target(), "provider-7");
        assert!(!bare.has_provider_session());
        assert!(bound.has_provider_session());
        assert!(!ReferenceTargetRef::with_provider_session(target(), "   ").has_provider_session());
    }

    #[test]
    fn history_page_discloses_every_non_native_source() {
        let native = ReferenceHistoryPage {
            source: ReferenceHistorySource::ClaudeTranscript,
            availability: ReferenceHistoryAvailability::Native,
            turns: vec![],
            cursor: None,
            has_more: false,
            generation: "g1".into(),
            unavailable_reason: None,
        };
        assert!(native.is_native());
        assert_eq!(native.disclosure(), None);
        assert!(native.can_reach_provider_read(&ReferenceTargetRef::with_provider_session(
            target(),
            "p"
        )));
        assert!(!native.can_reach_provider_read(&ReferenceTargetRef::without_provider_session(target())));

        let scrollback = ReferenceHistoryPage {
            source: ReferenceHistorySource::Scrollback,
            availability: ReferenceHistoryAvailability::Scrollback,
            turns: vec![],
            cursor: None,
            has_more: false,
            generation: "g1".into(),
            unavailable_reason: Some("no native reader".into()),
        };
        assert!(!scrollback.is_native());
        assert_eq!(scrollback.disclosure(), Some(REFERENCE_SCROLLBACK_DISCLOSURE));
        assert!(!scrollback.can_reach_provider_read(&ReferenceTargetRef::with_provider_session(
            target(),
            "p"
        )));

        let not_started = ReferenceHistoryPage {
            availability: ReferenceHistoryAvailability::NotStarted,
            source: ReferenceHistorySource::OmoTranscript,
            turns: vec![],
            cursor: None,
            has_more: false,
            generation: "g1".into(),
            unavailable_reason: None,
        };
        assert!(not_started.disclosure().is_some());
    }

    #[test]
    fn history_page_roundtrips_with_source_availability_and_cursor() {
        roundtrip::<ReferenceHistoryPage>(json!({
            "source": "pi-transcript",
            "availability": "native",
            "turns": [{"role": "assistant", "parts": [{"kind": "thinking", "text": "hmm"}]}],
            "cursor": {"streamId": "dev:ino", "offset": 4096},
            "hasMore": true,
            "generation": "gen-9"
        }));
        roundtrip::<ReferenceHistoryPage>(json!({
            "source": "scrollback",
            "availability": "scrollback",
            "turns": [],
            "hasMore": false,
            "generation": "gen-1",
            "unavailableReason": "no native reader for this pane"
        }));
    }

    #[test]
    fn cursor_from_another_stream_is_refused() {
        let cursor = ReferenceHistoryCursor { stream_id: "dev:ino".into(), offset: 4096 };
        assert!(cursor.matches_stream("dev:ino"));
        assert!(!cursor.matches_stream("dev:other"));
        roundtrip::<ReferenceHistoryCursor>(json!({"streamId": "dev:ino", "offset": 4096}));
    }

    #[test]
    fn every_part_variant_roundtrips_and_reports_its_kind() {
        let cases: Vec<(serde_json::Value, ReferencePartKind)> = vec![
            (json!({"kind": "text", "text": "hi"}), ReferencePartKind::Text),
            (
                json!({"kind": "text", "text": "hi", "phase": "finalAnswer"}),
                ReferencePartKind::Text,
            ),
            (json!({"kind": "thinking", "text": "hmm"}), ReferencePartKind::Thinking),
            (
                json!({"kind": "skill", "skill": {
                    "name": "frontend", "evidence": "instructions", "status": "loaded",
                    "path": "/Users/dev/.agents/skills/frontend/SKILL.md"}}),
                ReferencePartKind::Skill,
            ),
            (
                json!({"kind": "tool", "name": "read", "summary": "s", "input": "i", "output": "o"}),
                ReferencePartKind::Tool,
            ),
            (
                json!({"kind": "tool", "name": "read", "summary": "s", "input": "i", "output": "cut",
                       "error": true, "outputRef": "ref-1", "outputSize": 900}),
                ReferencePartKind::Tool,
            ),
            (
                json!({"kind": "image", "mediaType": "image/png", "ref": "img-1"}),
                ReferencePartKind::Image,
            ),
            (json!({"kind": "compact", "text": "summary"}), ReferencePartKind::Compact),
            (json!({"kind": "notice", "text": "job done", "source": "async-result"}), ReferencePartKind::Notice),
            (
                json!({"kind": "taskResult", "tasks": [{
                    "id": "t1", "title": "task one", "status": "completed", "result": "ok"}]}),
                ReferencePartKind::TaskResult,
            ),
        ];
        for (wire, kind) in cases {
            let part: ReferencePart = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(part.kind(), kind, "kind mismatch for {wire}");
            assert_eq!(serde_json::to_value(&part).unwrap(), wire);
        }
        assert!(serde_json::from_value::<ReferencePart>(json!({"kind": "video", "ref": "x"})).is_err());
    }

    #[test]
    fn a_standalone_skill_part_carries_the_pinned_activity_and_stays_out_of_inline_prose() {
        // The pinned `ConversationPart` has a `skill` variant beside the tool-attached one
        // (`shared/protocol.ts`); the turn's skill list draws it, the prose path does not.
        let document = json!({
            "kind": "skill",
            "skill": {
                "name": "lonely", "evidence": "instructions", "status": "loaded",
                "path": "/Users/dev/.claude/skills/lonely/SKILL.md"
            }
        });
        let part: ReferencePart = serde_json::from_value(document.clone()).expect("the pinned skill part decodes");
        assert_eq!(part.kind(), ReferencePartKind::Skill);
        assert!(!part.is_inline_text(), "a chip is not prose");
        let ReferencePart::Skill { skill } = &part else {
            panic!("the part is the skill variant, got {:?}", part.kind());
        };
        assert_eq!(skill.name, "lonely");
        assert_eq!(skill.evidence, ReferenceSkillEvidence::Instructions);
        assert_eq!(skill.status, ReferenceSkillStatus::Loaded);
        assert_eq!(skill.path.as_deref(), Some("/Users/dev/.claude/skills/lonely/SKILL.md"));
        assert_eq!(serde_json::to_value(&part).unwrap(), document);

        // An invocation chip names a skill with no document: a real shape, not a placeholder.
        let invocation = json!({
            "kind": "skill",
            "skill": {"name": "debugging", "evidence": "invocation", "status": "requested"}
        });
        roundtrip::<ReferencePart>(invocation);

        // A skill part with no activity at all is not a shape the contract allows.
        assert!(serde_json::from_value::<ReferencePart>(json!({"kind": "skill"})).is_err());
    }

    #[test]
    fn turn_carries_abandoned_branches_and_runtime_source() {
        roundtrip::<ReferenceTurn>(json!({
            "role": "user", "source": "runtime", "parts": [{"kind": "text", "text": "job"}],
            "abandoned": {"count": 3, "branches": 1, "summary": "walked away"}
        }));
        roundtrip::<ReferenceTurn>(json!({"role": "assistant", "parts": []}));
    }

    #[test]
    fn screen_snapshot_is_answerable_only_when_complete() {
        let good = ReferenceScreenSnapshot {
            revision: "r1".into(),
            text: "Do you want to proceed?".into(),
            truncated: false,
            gap: false,
            cols: 80,
            rows: 24,
        };
        assert!(good.is_answerable());
        for broken in [
            ReferenceScreenSnapshot { gap: true, ..good.clone() },
            ReferenceScreenSnapshot { truncated: true, ..good.clone() },
            ReferenceScreenSnapshot { text: String::new(), ..good.clone() },
        ] {
            assert!(!broken.is_answerable());
        }
    }

    #[test]
    fn submit_and_stop_payloads_roundtrip() {
        roundtrip::<ReferenceSubmitPayload>(json!({
            "text": "hello", "attachmentIds": ["a1"], "origin": "chat"}));
        roundtrip::<ReferenceSubmitPayload>(json!({"text": "hello", "origin": "terminal"}));
        roundtrip::<ReferenceStopPayload>(json!({"capability": "providerInterrupt"}));
        roundtrip::<ReferenceStopPayload>(json!({"capability": "refused"}));
        assert!(ReferenceStopCapability::Refused.is_refusal());
        assert!(!ReferenceStopCapability::ProviderInterrupt.is_refusal());
        assert!(!ReferenceStopCapability::ShellSignal.is_refusal());
    }

    #[test]
    fn prompt_answer_requires_exactly_one_shape() {
        roundtrip::<ReferencePromptAnswerPayload>(json!({
            "promptId": "p1", "screenRevision": "r1", "answer": {"optionIndex": 0}}));
        assert!(ReferencePromptAnswer { option_index: Some(0), ..Default::default() }.is_single_choice());
        assert!(ReferencePromptAnswer { option_indices: Some(vec![0, 2]), ..Default::default() }.is_single_choice());
        assert!(ReferencePromptAnswer { custom_text: Some("mine".into()), ..Default::default() }.is_single_choice());
        assert!(!ReferencePromptAnswer::default().is_single_choice());
        assert_eq!(ReferencePromptAnswer::default().variant_count(), 0);
        let ambiguous = ReferencePromptAnswer {
            option_index: Some(0),
            custom_text: Some("mine".into()),
            ..Default::default()
        };
        assert_eq!(ambiguous.variant_count(), 2);
        assert!(!ambiguous.is_single_choice());
    }

    #[test]
    fn prompt_confirmation_applies_only_to_option_picks_on_risky_kinds() {
        let mut prompt = ReferencePrompt {
            id: "p1".into(),
            agent: "claude".into(),
            kind: ReferencePromptKind::Approval,
            title: "Ready?".into(),
            question: "Ready?".into(),
            body: None,
            options: vec![
                ReferencePromptOption { label: "Yes".into(), description: None },
                ReferencePromptOption { label: "No".into(), description: None },
                ReferencePromptOption { label: "Other".into(), description: None },
            ],
            multi_select: false,
            custom_option_index: Some(2),
            queued: None,
            steps: vec![],
            fallback: None,
        };
        let pick = ReferencePromptAnswer { option_index: Some(0), ..Default::default() };
        let typed = ReferencePromptAnswer { custom_text: Some("sure".into()), ..Default::default() };
        assert!(prompt.needs_confirmation(&pick));
        assert!(!prompt.needs_confirmation(&typed));
        prompt.kind = ReferencePromptKind::Question;
        assert!(!prompt.needs_confirmation(&pick));
        prompt.kind = ReferencePromptKind::Menu;
        assert!(prompt.needs_confirmation(&pick));
        assert_eq!(prompt.selectable_indices(), vec![0, 1]);
    }

    #[test]
    fn key_steps_reject_empty_steps() {
        assert!(ReferenceKeyStep::keys(["Enter"]).is_effective());
        assert!(ReferenceKeyStep::typed("2").is_effective());
        assert!(!ReferenceKeyStep::typed("").is_effective());
        assert!(!ReferenceKeyStep { keys: vec![], text: None }.is_effective());
        roundtrip::<ReferenceKeyStep>(json!({"keys": ["Enter"]}));
        roundtrip::<ReferenceKeyStep>(json!({"text": "2"}));
    }

    #[test]
    fn file_receipt_reuses_the_scoped_attachment_receipt() {
        let wire = json!({
            "receipt": {"hostId": "h", "attachmentId": "opaque-1", "sha256": "hash",
                        "sizeBytes": 12, "mediaType": "image/png"},
            "displayName": "shot.png", "mentionText": "@/tmp/opaque-1 "
        });
        roundtrip::<ReferenceFileReceipt>(wire);
        roundtrip::<ReferenceFileStagePayload>(json!({
            "name": "shot.png", "mediaType": "image/png", "sizeBytes": 12, "contentBase64": "AAA="}));
        assert_eq!(ReferenceFileReceipt::mention_for("/tmp/opaque-1"), "@/tmp/opaque-1 ");
    }

    #[test]
    fn delivery_ladder_ranks_stages_in_order() {
        assert!(reference_stage_at_least(DeliveryStage::Accepted, DeliveryStage::Staged));
        assert!(reference_stage_at_least(DeliveryStage::ProviderRead, DeliveryStage::Accepted));
        assert!(reference_stage_at_least(DeliveryStage::Staged, DeliveryStage::Staged));
        assert!(!reference_stage_at_least(DeliveryStage::Staged, DeliveryStage::Accepted));
        assert!(!reference_stage_at_least(DeliveryStage::Accepted, DeliveryStage::ProviderRead));
        assert_eq!(reference_stage_rank(DeliveryStage::Staged), 0);
        assert_eq!(reference_stage_rank(DeliveryStage::ProviderRead), 2);
    }

    #[test]
    fn registry_ids_map_to_native_readers_without_guessing() {
        assert_eq!(ReferenceNativeHistoryKind::from_registry_id("claude"), ReferenceNativeHistoryKind::Claude);
        assert_eq!(ReferenceNativeHistoryKind::from_registry_id("  Codex "), ReferenceNativeHistoryKind::Codex);
        assert_eq!(ReferenceNativeHistoryKind::from_registry_id("omp"), ReferenceNativeHistoryKind::Omp);
        assert_eq!(ReferenceNativeHistoryKind::from_registry_id("omo"), ReferenceNativeHistoryKind::Omo);
        assert_eq!(ReferenceNativeHistoryKind::from_registry_id("gjc"), ReferenceNativeHistoryKind::Gjc);
        assert_eq!(ReferenceNativeHistoryKind::from_registry_id("pi"), ReferenceNativeHistoryKind::Pi);
        for unknown in ["mimo-code", "cursor-agent", "prime-agent", "openclaw", ""] {
            assert_eq!(
                ReferenceNativeHistoryKind::from_registry_id(unknown),
                ReferenceNativeHistoryKind::Unavailable,
                "{unknown} must not claim a native reader"
            );
        }
        assert!(!ReferenceNativeHistoryKind::Unavailable.is_native());
        assert!(ReferenceNativeHistoryKind::Pi.is_native());
        assert_eq!(ReferenceHistorySource::PiTranscript.native_kind(), ReferenceNativeHistoryKind::Pi);
        assert_eq!(ReferenceHistorySource::Scrollback.native_kind(), ReferenceNativeHistoryKind::Unavailable);
        assert_eq!(ReferenceHistorySource::GjcTranscript.as_str(), "gjc-transcript");
        assert_eq!(REFERENCE_NATIVE_HISTORY_KINDS, 6);
    }

    #[test]
    fn history_source_wire_names_match_the_upstream_readers() {
        let cases = [
            (ReferenceHistorySource::ClaudeTranscript, "claude-transcript"),
            (ReferenceHistorySource::CodexTranscript, "codex-transcript"),
            (ReferenceHistorySource::OmpTranscript, "omp-transcript"),
            (ReferenceHistorySource::OmoTranscript, "omo-transcript"),
            (ReferenceHistorySource::GjcTranscript, "gjc-transcript"),
            (ReferenceHistorySource::PiTranscript, "pi-transcript"),
            (ReferenceHistorySource::Scrollback, "scrollback"),
        ];
        for (source, name) in cases {
            assert_eq!(serde_json::to_value(source).unwrap(), json!(name));
            assert_eq!(source.as_str(), name);
            roundtrip::<ReferenceHistorySource>(json!(name));
            let expected_kind = if source == ReferenceHistorySource::Scrollback {
                "unavailable"
            } else {
                name
            };
            assert_eq!(source.native_kind().as_str(), expected_kind);
        }
        roundtrip::<ReferenceNativeHistoryKind>(json!("unavailable"));
        assert_eq!(
            serde_json::to_value(ReferenceHistoryAvailability::NotStarted).unwrap(),
            json!("notStarted")
        );
    }

    #[test]
    fn routes_and_unknown_outcome_helpers() {
        assert_eq!(reference_chat_route("s1", "history"), "/api/v1/reference-chat/s1/history");
        assert_eq!(reference_chat_route("s1", "/files/f1"), "/api/v1/reference-chat/s1/files/f1");
        assert_eq!(reference_chat_route("s1", ""), "/api/v1/reference-chat/s1");
        assert!(reference_is_outcome_unknown(REFERENCE_OUTCOME_UNKNOWN_CODE));
        assert!(!reference_is_outcome_unknown("TIMEOUT"));
    }

    #[test]
    fn draft_key_is_owner_and_epoch_scoped() {
        let base = ReferenceTargetRef::without_provider_session(target());
        let other_epoch = ReferenceTargetRef::without_provider_session(TargetRef {
            epoch: Epoch(7),
            ..target()
        });
        let other_owner = ReferenceTargetRef::without_provider_session(TargetRef {
            owner_id: "owner-b".into(),
            ..target()
        });
        assert_ne!(reference_draft_key(&base), reference_draft_key(&other_epoch));
        assert_ne!(reference_draft_key(&base), reference_draft_key(&other_owner));
        let bound = ReferenceTargetRef::with_provider_session(target(), "p9");
        assert_eq!(reference_draft_key(&base), reference_draft_key(&bound));
    }
}
