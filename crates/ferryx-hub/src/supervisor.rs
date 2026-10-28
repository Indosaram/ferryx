//! Durable state of the unified supervisor (migration 0020): the owner's one identity, the
//! projects and channels it answers for, admitted requests, typed execution bindings, wait
//! dependencies, dispatch receipts and pending wakes.
//!
//! Only storage lives here. Admission policy, dispatch and wake handling build on these
//! functions. Every multi-statement write runs in one `BEGIN IMMEDIATE` transaction
//! ([`WriteTx`]) that never spans a model or network call: callers commit first, then act.
//!
//! A goal's [`RunState`] is separate from the lane lifecycle: `lanes.status` is projected from
//! it, never scheduled from. A lane without a `supervisor_goals` row stays historical until an
//! explicit adoption. Requirements stay in `lane_goal_requirements`; a supervisor goal
//! references its `lane_goal` rather than copying them. `supervisor_goals.revision` is the
//! authoritative revision of this path: every goal mutation names the revision it was decided
//! against (run-state moves also name the state), and a stale one is refused with
//! `REVISION_CONFLICT`. Owner corrections and owner holds (pause, await-owner, cancel) bump the
//! revision and supersede unsent intents of older revisions, so nothing decided before them
//! can dispatch.
//!
//! Delivery rests on one invariant: per goal and execution generation at most one dispatch is
//! *in flight* — `claimed`, `accepted`, `started`, or `uncertain` and not yet resolved by the
//! owner. A claim, a move into `ready` (which would allow a claim) and a released claim's
//! `dispatching -> ready` reset are refused or skipped while another dispatch is in flight
//! (`DISPATCH_IN_FLIGHT`); a partial unique index enforces the same invariant in the schema.
//! Only [`resolve_uncertain_delivery`] releases an uncertain dispatch for a resend; its row and
//! evidence stay. A claim given back before transmission is `released`: never sent, never in
//! flight.
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{SqliteConnection, SqlitePool};
use uuid::Uuid;

use crate::error::HubError;
use crate::goals::contract::{self, RequirementSpec};
use crate::goals::types::{GoalError, GoalMode, ProposalState};
use crate::storage::{now_rfc3339, WriteTx};

/// Native mhc memory name of the supervisor. It is never a machine or chat-thread id.
pub const MEMORY_NAME: &str = "ferryx-supervisor";

/// Continuation-turn budget of a newly admitted goal until the owner changes it.
pub const DEFAULT_TURN_LIMIT: u32 = 20;

/// Lane-key prefix of a lane created by admission.
pub const LANE_KEY_PREFIX: &str = "sv-";

/// Source message id prefix of an explicit legacy-lane adoption: `adopt:<legacy lane key>`.
/// Reserved for adoption ([`AdmitTerms::adopts_lane`]), so the admitted request row also marks
/// the legacy lane as owned by the supervisor ([`supervisor_owns_lane`]).
pub const ADOPT_MESSAGE_PREFIX: &str = "adopt:";

/// Code of [`SupervisorError::SetupConflict`].
pub const SETUP_CONFLICT: &str = "SETUP_CONFLICT";

/// Code of [`SupervisorError::SessionAlreadyBound`]: a 409-class refusal.
pub const SESSION_ALREADY_BOUND: &str = "SESSION_ALREADY_BOUND";

/// `lanes.done_signals` of an admitted lane: completion is the goal contract, not a marker.
const LANE_DONE_SIGNALS: &str = "goal contract";

/// Longest source message id, event id, run id or event key.
const MAX_ID_CHARS: usize = 512;

/// Longest request body or worker instruction.
const MAX_BODY_CHARS: usize = 64_000;

/// Typed failure of supervisor storage. [`SupervisorError::code`] is the stable wire code.
#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("goal {goal_id} revision changed (expected {expected}, found {actual})")]
    RevisionConflict {
        goal_id: String,
        expected: i64,
        actual: i64,
    },
    #[error("goal {goal_id} is {actual}, not {expected}")]
    StateConflict {
        goal_id: String,
        expected: RunState,
        actual: RunState,
    },
    #[error("goal {goal_id} has no live run on its current execution generation")]
    NoLiveRun { goal_id: String },
    #[error("goal {goal_id} has dispatch {request_id} in flight; it takes no new dispatch")]
    DispatchInFlight { goal_id: String, request_id: String },
    #[error("dispatch {request_id} was released before transmission: {why}")]
    ClaimReleased { request_id: String, why: String },
    #[error(
        "{source_adapter} message {source_message_id} was already admitted as goal {goal_id} with a different body"
    )]
    RequestBodyChanged {
        source_adapter: ChannelProvider,
        source_message_id: String,
        goal_id: String,
        revision: i64,
    },
    #[error("goal {goal_id} cannot move from {from} to {to}")]
    IllegalTransition {
        goal_id: String,
        from: RunState,
        to: RunState,
    },
    #[error("goal {goal_id} is {state}")]
    GoalClosed { goal_id: String, state: RunState },
    #[error("execution generation changed (expected {expected}, found {actual})")]
    StaleGeneration { expected: i64, actual: i64 },
    #[error("goal {goal_id} already has an unsent dispatch")]
    DispatchPending { goal_id: String },
    #[error("dispatch {request_id} cannot be claimed: {why}")]
    NotClaimable { request_id: String, why: String },
    #[error("dispatch {request_id} cannot move from {from} to {to}")]
    ReceiptOutOfOrder {
        request_id: String,
        from: DispatchState,
        to: DispatchState,
    },
    #[error("dispatch {request_id} belongs to run {expected:?}, not {actual}")]
    RunMismatch {
        request_id: String,
        expected: Option<String>,
        actual: String,
    },
    #[error("dispatch {request_id} receipt names session {actual}, the binding has {expected:?}")]
    SessionMismatch {
        request_id: String,
        expected: Option<String>,
        actual: String,
    },
    #[error("{kind} {id} not found")]
    NotFound { kind: &'static str, id: String },
    #[error("{0} belongs to a different owner")]
    CrossOwner(String),
    #[error("goal {0} cannot wait on itself")]
    SelfWait(String),
    #[error("goal {goal} waiting on {child} would close a dependency cycle")]
    WaitCycle { goal: String, child: String },
    #[error("invalid supervisor request: {0}")]
    Invalid(String),
    #[error("unknown {kind}: {value}")]
    Unknown { kind: &'static str, value: String },
    /// A legacy lane API (goal set, mode, approve, discard, `lane.drive`) aimed at a lane the
    /// supervisor owns.
    #[error(
        "SUPERVISOR_OWNED: lane {lane_key} belongs to a supervisor goal; change it through the supervisor"
    )]
    SupervisorOwned { lane_key: String },
    /// An adoption whose legacy lane the legacy path still drives at commit: a worker attached
    /// to a lane that has not ended, a legacy goal turn pending or in flight, or a lane close
    /// removing its worktree.
    #[error("legacy lane {lane_key} cannot be adopted now: {why}")]
    LegacyLaneLive { lane_key: String, why: String },
    /// An adoption whose legacy goal became stricter (off, or supervised under a `full`
    /// adoption) or changed its adopted terms after the adoption read it. Adopting again takes
    /// the current terms.
    #[error("legacy lane {lane_key} changed while its adoption was prepared: {why}")]
    LegacyTermsChanged { lane_key: String, why: String },
    /// Set-up would change what the installation already holds: give it a second owner,
    /// rename a project or move a project's host mapping. Nothing was written.
    #[error("SETUP_CONFLICT: {0}")]
    SetupConflict(String),
    /// The owner chose a pane for a new goal that another open goal is already bound to on
    /// that host. Nothing was written (409-class).
    #[error("SESSION_ALREADY_BOUND: session {session_id} on host {host_id} is bound to open goal {goal_id}")]
    SessionAlreadyBound {
        host_id: String,
        session_id: String,
        goal_id: String,
    },
    #[error(transparent)]
    Goal(#[from] GoalError),
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
}

impl SupervisorError {
    /// Stable code shown to adapters; `REVISION_CONFLICT` maps to HTTP 409.
    pub fn code(&self) -> &'static str {
        match self {
            SupervisorError::RevisionConflict { .. } => "REVISION_CONFLICT",
            SupervisorError::StateConflict { .. } => "REVISION_CONFLICT",
            SupervisorError::NoLiveRun { .. } => "NO_LIVE_RUN",
            SupervisorError::DispatchInFlight { .. } => "DISPATCH_IN_FLIGHT",
            SupervisorError::ClaimReleased { .. } => "SUPERSEDED",
            SupervisorError::RequestBodyChanged { .. } => "REQUEST_CONFLICT",
            SupervisorError::IllegalTransition { .. } => "ILLEGAL_TRANSITION",
            SupervisorError::GoalClosed { .. } => "GOAL_CLOSED",
            SupervisorError::StaleGeneration { .. } => "STALE_GENERATION",
            SupervisorError::DispatchPending { .. } => "DISPATCH_PENDING",
            SupervisorError::NotClaimable { .. } => "NOT_CLAIMABLE",
            SupervisorError::ReceiptOutOfOrder { .. } => "RECEIPT_OUT_OF_ORDER",
            SupervisorError::RunMismatch { .. } => "RUN_MISMATCH",
            SupervisorError::SessionMismatch { .. } => "SESSION_MISMATCH",
            SupervisorError::NotFound { .. } => "NOT_FOUND",
            SupervisorError::CrossOwner(_) => "CROSS_OWNER",
            SupervisorError::SelfWait(_) | SupervisorError::WaitCycle { .. } => "WAIT_CYCLE",
            SupervisorError::SupervisorOwned { .. } => SUPERVISOR_OWNED,
            SupervisorError::LegacyLaneLive { .. } => admission::LEGACY_LANE_LIVE,
            SupervisorError::LegacyTermsChanged { .. } => admission::LEGACY_TERMS_CHANGED,
            SupervisorError::SetupConflict(_) => SETUP_CONFLICT,
            SupervisorError::SessionAlreadyBound { .. } => SESSION_ALREADY_BOUND,
            SupervisorError::Invalid(_)
            | SupervisorError::Unknown { .. }
            | SupervisorError::Goal(_) => "INVALID_REQUEST",
            SupervisorError::Sql(_) => "STORAGE",
        }
    }
}

impl From<SupervisorError> for HubError {
    fn from(error: SupervisorError) -> Self {
        match error {
            SupervisorError::Sql(error) => HubError::Sql(error),
            other => HubError::message(format!("{}: {other}", other.code())),
        }
    }
}

/// A snake_case enum with `ALL`, `as_str`, `Display` and a `FromStr` that refuses any other
/// string with [`SupervisorError::Unknown`]. Serde uses the same strings.
macro_rules! supervisor_enum {
    ($(#[$meta:meta])* $name:ident, $kind:literal, { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const ALL: &[$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = SupervisorError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                match text {
                    $($text => Ok($name::$variant),)+
                    other => Err(SupervisorError::Unknown { kind: $kind, value: other.to_owned() }),
                }
            }
        }
    };
}

supervisor_enum!(
    /// Persisted run state of a supervisor goal, separate from the lane lifecycle.
    RunState, "run state", {
        Ready => "ready",
        Dispatching => "dispatching",
        Running => "running",
        WaitingChildren => "waiting_children",
        WaitingEvent => "waiting_event",
        AwaitingOwner => "awaiting_owner",
        Verifying => "verifying",
        Completed => "completed",
        Paused => "paused",
        Degraded => "degraded",
        DeliveryUncertain => "delivery_uncertain",
        Cancelled => "cancelled",
    }
);

impl RunState {
    /// `completed` and `cancelled` never move again.
    pub fn is_terminal(self) -> bool {
        matches!(self, RunState::Completed | RunState::Cancelled)
    }

    /// Owner holds. Entering one bumps the goal revision and supersedes older unsent intents.
    pub fn is_owner_hold(self) -> bool {
        matches!(
            self,
            RunState::Paused | RunState::AwaitingOwner | RunState::Cancelled
        )
    }

    /// The transition table. `submit -> ready -> dispatching -> running`; only verification
    /// reaches `completed`; owner holds (`awaiting_owner`, `paused`), `degraded` and `cancelled`
    /// are reachable from every live state. Staying in the same state is not a transition.
    ///
    /// `running` is listed from `paused`, `degraded`, `delivery_uncertain` and `ready` because
    /// a pause never stops a worker and a restart or a late receipt may find one still running;
    /// [`transition_goal`] additionally requires a live run (a started, unsettled dispatch on
    /// the binding's current generation, run and session) for every move into `running`, and
    /// refuses every move into `ready` while any dispatch is in flight. Without a live run,
    /// resuming a paused goal goes to `ready`; with one, it resumes to `running`.
    pub fn can_transition_to(self, to: RunState) -> bool {
        use RunState::*;
        if self.is_terminal() || self == to {
            return false;
        }
        match to {
            Ready => matches!(
                self,
                Paused
                    | Degraded
                    | Verifying
                    | AwaitingOwner
                    | WaitingChildren
                    | WaitingEvent
                    | DeliveryUncertain
            ),
            Dispatching => self == Ready,
            Running => matches!(
                self,
                Dispatching | Paused | Degraded | DeliveryUncertain | Ready
            ),
            WaitingChildren | WaitingEvent => {
                matches!(self, Running | Verifying | WaitingChildren | WaitingEvent)
            }
            Verifying => matches!(self, Running | WaitingChildren | WaitingEvent),
            Completed => self == Verifying,
            DeliveryUncertain => matches!(self, Dispatching | Running),
            AwaitingOwner | Paused | Degraded | Cancelled => true,
        }
    }
}

supervisor_enum!(
    /// Channel provider of a binding, and the source adapter of an admitted request.
    ChannelProvider, "channel provider", {
        Slack => "slack",
        Discord => "discord",
        Desktop => "desktop",
        Phone => "phone",
        Cli => "cli",
        Mcp => "mcp",
    }
);

supervisor_enum!(
    /// What executes a goal: a terminal agent pane or a native mhc session.
    ExecutionKind, "execution kind", {
        Pty => "pty",
        Mhc => "mhc",
    }
);

supervisor_enum!(
    /// Life of one dispatch intent. Each receipt is a separate fact. `released` is a claim
    /// given back before transmission; `superseded` an intent replaced before it was claimed.
    DispatchState, "dispatch state", {
        Ready => "ready",
        Claimed => "claimed",
        Accepted => "accepted",
        Started => "started",
        Settled => "settled",
        Uncertain => "uncertain",
        Superseded => "superseded",
        Released => "released",
    }
);

supervisor_enum!(
    WaitKind, "wait kind", {
        ChildGoal => "child_goal",
        ExternalEvent => "external_event",
    }
);

supervisor_enum!(
    WaitState, "wait state", {
        Pending => "pending",
        Satisfied => "satisfied",
        Failed => "failed",
        Cancelled => "cancelled",
    }
);

supervisor_enum!(
    /// What one event delivered to a wait says. A failure resolves the wait; it is not success.
    WaitOutcome, "wait outcome", {
        Progress => "progress",
        Satisfied => "satisfied",
        Failed => "failed",
    }
);

supervisor_enum!(
    WakeCause, "wake cause", {
        DependencyReady => "dependency_ready",
        DependencyFailed => "dependency_failed",
        OwnerCorrection => "owner_correction",
        OwnerControl => "owner_control",
        TurnSettled => "turn_settled",
        Deadline => "deadline",
        Report => "report",
    }
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorIdentity {
    pub supervisor_id: String,
    pub owner_principal: String,
    pub memory_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub project_id: String,
    pub supervisor_id: String,
    pub name: String,
    pub repo_identity: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectHost {
    pub project_id: String,
    pub host_id: String,
    pub git_common_dir: String,
    pub updated_at: String,
}

/// Where a conversation happens and who authenticated in it. Empty strings mean "none".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelAddress {
    pub provider: ChannelProvider,
    pub workspace_id: String,
    pub account_id: String,
    pub channel_id: String,
    pub thread_id: String,
    pub owner_principal: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelBinding {
    pub binding_id: String,
    pub supervisor_id: String,
    pub address: ChannelAddress,
    pub project_id: Option<String>,
    pub goal_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorGoal {
    pub goal_id: String,
    pub supervisor_id: String,
    pub project_id: String,
    pub lane_key: String,
    pub owner_principal: String,
    pub run_state: RunState,
    pub state_detail: Option<String>,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// The worker a goal is bound to. Never stored in `lanes.omo_thread_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionBinding {
    pub goal_id: String,
    pub kind: ExecutionKind,
    pub host_id: String,
    pub provider_session_id: Option<String>,
    pub run_id: Option<String>,
    pub generation: i64,
    pub updated_at: String,
}

/// One dispatch intent and its receipts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dispatch {
    pub id: i64,
    pub goal_id: String,
    pub goal_revision: i64,
    pub ordinal: i64,
    /// Stable transport request id, transmitted at most once.
    pub request_id: String,
    pub generation: i64,
    pub instruction: String,
    pub state: DispatchState,
    pub run_id: Option<String>,
    /// Provider session that observed the start or settle.
    pub provider_session_id: Option<String>,
    pub uncertain_code: Option<String>,
    pub claimed_at: Option<String>,
    pub accepted_at: Option<String>,
    pub started_at: Option<String>,
    pub settled_at: Option<String>,
    pub uncertain_at: Option<String>,
    /// When the owner resolved this uncertain delivery, allowing a resend.
    pub owner_resolved_at: Option<String>,
    pub created_at: String,
}

impl Dispatch {
    /// Claimed, accepted, started, or uncertain and not yet resolved by the owner: the
    /// worker may be busy with it. At most one per goal and generation.
    pub fn is_in_flight(&self) -> bool {
        match self.state {
            DispatchState::Claimed | DispatchState::Accepted | DispatchState::Started => true,
            DispatchState::Uncertain => self.owner_resolved_at.is_none(),
            DispatchState::Ready
            | DispatchState::Settled
            | DispatchState::Superseded
            | DispatchState::Released => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wait {
    pub wait_id: String,
    pub supervisor_id: String,
    pub goal_id: String,
    pub kind: WaitKind,
    pub child_goal_id: Option<String>,
    pub event_key: Option<String>,
    pub state: WaitState,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingWake {
    pub id: i64,
    pub goal_id: String,
    pub cause: WakeCause,
    pub event_id: String,
    pub goal_revision: i64,
    pub generation: i64,
    pub due_at: String,
    pub consumed_at: Option<String>,
    pub created_at: String,
}

/// The host and kind of worker an admitted goal targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionTarget {
    pub kind: ExecutionKind,
    pub host_id: String,
}

/// An existing pane the owner chose for a new goal: its provider session on the goal's host.
/// Bound in the admission transaction ([`admit_request_bound`]), so the dispatcher never sees
/// the goal unbound and never launches a pane for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundSession {
    pub host_id: String,
    pub session_id: String,
}

/// One owner request to admit. Actor fields are server-derived from the authenticated
/// adapter, never taken from model arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmitRequest {
    pub supervisor_id: String,
    pub owner_principal: String,
    pub source_adapter: ChannelProvider,
    pub source_message_id: String,
    /// The original request text exactly as authored; its hash is the replay check.
    pub body: String,
    pub project_id: String,
    pub channel_binding_id: Option<String>,
    pub target: ExecutionTarget,
    /// Title and working directory of the lane on the target host.
    pub title: String,
    pub cwd: String,
    /// First worker instruction, carried by the initial ready dispatch intent.
    pub instruction: String,
    pub requirements: Vec<RequirementSpec>,
    pub mode: GoalMode,
    pub turn_limit: u32,
    /// The operations the request was granted, persisted with the goal
    /// (`supervisor_goals.operation_grant`). Empty grants no write operation.
    pub operation_grant: Vec<admission::OperationClass>,
}

impl AdmitRequest {
    fn validate(&self) -> Result<(), SupervisorError> {
        require_uuid("supervisor id", &self.supervisor_id)?;
        require_uuid("project id", &self.project_id)?;
        if let Some(binding_id) = &self.channel_binding_id {
            require_uuid("channel binding id", binding_id)?;
        }
        require_id("owner principal", &self.owner_principal, 256)?;
        require_id("source message id", &self.source_message_id, MAX_ID_CHARS)?;
        require_id("host id", &self.target.host_id, 256)?;
        require_text("request body", &self.body, MAX_BODY_CHARS)?;
        require_text("lane title", &self.title, 256)?;
        require_text("working directory", &self.cwd, 4096)?;
        require_text("instruction", &self.instruction, MAX_BODY_CHARS)?;
        contract::validate(&self.requirements, self.turn_limit)?;
        Ok(())
    }
}

/// Conditions of one admission that are checked and applied inside its write transaction, so
/// no other writer, dispatcher or restart ever sees the goal before they hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdmitTerms {
    /// Commit the goal directly on owner hold: `awaiting_owner` with this `state_detail` and
    /// its initial intent superseded (revision 2, as a hold after admission would leave it).
    /// The goal is never `ready`, so nothing can claim it and the reason is never replaced.
    pub owner_hold: Option<String>,
    /// The legacy lane this admission adopts. The request's source message id must be
    /// `adopt:<lane>` ([`ADOPT_MESSAGE_PREFIX`]). A lane is adopted once: an earlier
    /// `adopt:<lane>` admission from any adapter answers as the replay (or refuses a different
    /// owner or body), checked under the write lock. Refused with `LEGACY_LANE_LIVE` when,
    /// inside the transaction, the legacy lane still has a worker attached (and has not ended)
    /// or a legacy goal turn pending or in flight, or a lane close is removing its worktree,
    /// and with `LEGACY_TERMS_CHANGED` when its legacy goal is now stricter than the request's
    /// mode. Requires [`AdmitTerms::legacy_goal_read`].
    pub adopts_lane: Option<String>,
    /// The legacy goal as the adoption read it ([`LegacyGoalRead`]); required with
    /// `adopts_lane` and refused without it. An adoption whose legacy goal changed since
    /// (requirements or turn limit replaced, a turn counted, a goal created or removed) is
    /// refused with `LEGACY_TERMS_CHANGED` under the write lock, so the adopted terms are
    /// never stale.
    pub legacy_goal_read: Option<LegacyGoalRead>,
}

/// `(revision, turns_used)` of a legacy lane's `lane_goal`, or `None` when it has none: what
/// an adoption copies (requirements and turn limit bump the revision; the budget left is the
/// turn limit minus the turns used). The mode is re-checked separately.
pub type LegacyGoalRead = Option<(i64, u32)>;

/// What admission persisted. A replay of the same source message returns the same receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionReceipt {
    pub supervisor_id: String,
    pub goal_id: String,
    pub lane_key: String,
    pub source_adapter: ChannelProvider,
    pub source_message_id: String,
    /// Goal revision the request was admitted at.
    pub revision: i64,
    /// Request id of the initial dispatch intent.
    pub dispatch_request_id: String,
    pub admitted_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    pub receipt: AdmissionReceipt,
    /// True when the source message was admitted before and nothing new was written.
    pub duplicate: bool,
}

/// A delivery receipt for a claimed dispatch. Started and settled name the run and the
/// provider session that observed it, so a receipt identifies its target, not just a
/// generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receipt {
    Accepted,
    Started {
        run_id: String,
        provider_session_id: String,
    },
    Settled {
        run_id: String,
        provider_session_id: String,
    },
    Uncertain { code: String },
}

impl Receipt {
    fn state(&self) -> DispatchState {
        match self {
            Receipt::Accepted => DispatchState::Accepted,
            Receipt::Started { .. } => DispatchState::Started,
            Receipt::Settled { .. } => DispatchState::Settled,
            Receipt::Uncertain { .. } => DispatchState::Uncertain,
        }
    }

    fn allowed_from(&self) -> &'static [DispatchState] {
        use DispatchState::*;
        match self {
            Receipt::Accepted => &[Claimed],
            // An uncertain dispatch may still be shown to have run (restart reconciliation).
            Receipt::Started { .. } => &[Claimed, Accepted, Uncertain],
            // A settle may arrive without its start having been observed.
            Receipt::Settled { .. } => &[Accepted, Started, Uncertain],
            Receipt::Uncertain { .. } => &[Claimed, Accepted],
        }
    }

    /// The run and provider session a start or settle observed.
    fn run(&self) -> Option<(&str, &str)> {
        match self {
            Receipt::Started {
                run_id,
                provider_session_id,
            }
            | Receipt::Settled {
                run_id,
                provider_session_id,
            } => Some((run_id.as_str(), provider_session_id.as_str())),
            Receipt::Accepted | Receipt::Uncertain { .. } => None,
        }
    }
}

/// What a goal waits on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitTarget {
    ChildGoal(String),
    ExternalEvent(String),
}

/// What recording one wait event changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitEventReceipt {
    /// False when this (wait, event) pair was already recorded; nothing changed.
    pub inserted: bool,
    pub wait: Wait,
    /// The wake persisted by this event, if it made the goal eligible to resume.
    pub wake: Option<PendingWake>,
}

/// Lane key of an admitted goal: deterministic from the goal id.
pub fn lane_key_for(goal_id: &Uuid) -> String {
    let simple = goal_id.simple().to_string();
    format!("{LANE_KEY_PREFIX}{}", &simple[..20])
}

/// Stable transport request id of one dispatch intent.
pub fn dispatch_request_id(goal_id: &str, revision: i64, ordinal: i64) -> String {
    format!("{LANE_KEY_PREFIX}{goal_id}-r{revision}-d{ordinal}")
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn require_text(label: &str, value: &str, max_chars: usize) -> Result<(), SupervisorError> {
    if value.trim().is_empty() {
        return Err(SupervisorError::Invalid(format!("{label} is required")));
    }
    if value.chars().count() > max_chars {
        return Err(SupervisorError::Invalid(format!(
            "{label} exceeds {max_chars} characters"
        )));
    }
    Ok(())
}

/// An identifier: non-empty, bounded, no surrounding whitespace and no control characters.
fn require_id(label: &str, value: &str, max_chars: usize) -> Result<(), SupervisorError> {
    require_text(label, value, max_chars)?;
    if value.trim() != value || value.chars().any(char::is_control) {
        return Err(SupervisorError::Invalid(format!(
            "{label} has surrounding whitespace or control characters"
        )));
    }
    Ok(())
}

/// A canonical lowercase hyphenated UUID, the only form stored.
fn require_uuid(label: &str, value: &str) -> Result<(), SupervisorError> {
    match Uuid::parse_str(value) {
        Ok(parsed) if parsed.hyphenated().to_string() == value => Ok(()),
        _ => Err(SupervisorError::Invalid(format!(
            "{label} must be a lowercase hyphenated UUID"
        ))),
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.is_unique_violation())
}

fn check_revision(goal: &SupervisorGoal, expected: i64) -> Result<(), SupervisorError> {
    if goal.revision == expected {
        Ok(())
    } else {
        Err(SupervisorError::RevisionConflict {
            goal_id: goal.goal_id.clone(),
            expected,
            actual: goal.revision,
        })
    }
}

fn check_generation(actual: i64, expected: i64) -> Result<(), SupervisorError> {
    if actual == expected {
        Ok(())
    } else {
        Err(SupervisorError::StaleGeneration { expected, actual })
    }
}

fn check_open(goal: &SupervisorGoal) -> Result<(), SupervisorError> {
    if goal.run_state.is_terminal() {
        Err(SupervisorError::GoalClosed {
            goal_id: goal.goal_id.clone(),
            state: goal.run_state,
        })
    } else {
        Ok(())
    }
}

const GOAL_COLUMNS: &str = "goal_id, supervisor_id, project_id, lane_key, owner_principal, \
     run_state, state_detail, revision, created_at, updated_at";

type GoalRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    i64,
    String,
    String,
);

fn goal_from_row(row: GoalRow) -> Result<SupervisorGoal, SupervisorError> {
    let (
        goal_id,
        supervisor_id,
        project_id,
        lane_key,
        owner_principal,
        run_state,
        state_detail,
        revision,
        created_at,
        updated_at,
    ) = row;
    Ok(SupervisorGoal {
        goal_id,
        supervisor_id,
        project_id,
        lane_key,
        owner_principal,
        run_state: run_state.parse()?,
        state_detail,
        revision,
        created_at,
        updated_at,
    })
}

async fn load_goal(
    conn: &mut SqliteConnection,
    goal_id: &str,
) -> Result<Option<SupervisorGoal>, SupervisorError> {
    let sql = format!("SELECT {GOAL_COLUMNS} FROM supervisor_goals WHERE goal_id = ?");
    let row: Option<GoalRow> = sqlx::query_as(&sql)
        .bind(goal_id)
        .fetch_optional(&mut *conn)
        .await?;
    row.map(goal_from_row).transpose()
}

async fn require_goal(
    conn: &mut SqliteConnection,
    goal_id: &str,
) -> Result<SupervisorGoal, SupervisorError> {
    load_goal(conn, goal_id)
        .await?
        .ok_or_else(|| SupervisorError::NotFound {
            kind: "goal",
            id: goal_id.to_owned(),
        })
}

type BindingRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    String,
);

fn binding_from_row(row: BindingRow) -> Result<ExecutionBinding, SupervisorError> {
    let (goal_id, kind, host_id, provider_session_id, run_id, generation, updated_at) = row;
    Ok(ExecutionBinding {
        goal_id,
        kind: kind.parse()?,
        host_id,
        provider_session_id,
        run_id,
        generation,
        updated_at,
    })
}

async fn load_binding(
    conn: &mut SqliteConnection,
    goal_id: &str,
) -> Result<Option<ExecutionBinding>, SupervisorError> {
    let row: Option<BindingRow> = sqlx::query_as(
        "SELECT goal_id, kind, host_id, provider_session_id, run_id, generation, updated_at
         FROM supervisor_execution_bindings WHERE goal_id = ?",
    )
    .bind(goal_id)
    .fetch_optional(&mut *conn)
    .await?;
    row.map(binding_from_row).transpose()
}

async fn require_binding(
    conn: &mut SqliteConnection,
    goal_id: &str,
) -> Result<ExecutionBinding, SupervisorError> {
    load_binding(conn, goal_id)
        .await?
        .ok_or_else(|| SupervisorError::NotFound {
            kind: "execution binding",
            id: goal_id.to_owned(),
        })
}

const DISPATCH_COLUMNS: &str = "id, goal_id, goal_revision, ordinal, request_id, generation, \
     instruction, state, run_id, provider_session_id, uncertain_code, claimed_at, accepted_at, \
     started_at, settled_at, uncertain_at, owner_resolved_at, created_at";

/// The in-flight predicate over a dispatch aliased `d`; the schema's
/// `uq_supervisor_dispatches_one_in_flight` index uses the same condition.
const IN_FLIGHT: &str = "(d.state IN ('claimed','accepted','started') \
     OR (d.state = 'uncertain' AND d.owner_resolved_at IS NULL))";

#[derive(sqlx::FromRow)]
struct DispatchRow {
    id: i64,
    goal_id: String,
    goal_revision: i64,
    ordinal: i64,
    request_id: String,
    generation: i64,
    instruction: String,
    state: String,
    run_id: Option<String>,
    provider_session_id: Option<String>,
    uncertain_code: Option<String>,
    claimed_at: Option<String>,
    accepted_at: Option<String>,
    started_at: Option<String>,
    settled_at: Option<String>,
    uncertain_at: Option<String>,
    owner_resolved_at: Option<String>,
    created_at: String,
}

fn dispatch_from_row(row: DispatchRow) -> Result<Dispatch, SupervisorError> {
    Ok(Dispatch {
        id: row.id,
        goal_id: row.goal_id,
        goal_revision: row.goal_revision,
        ordinal: row.ordinal,
        request_id: row.request_id,
        generation: row.generation,
        instruction: row.instruction,
        state: row.state.parse()?,
        run_id: row.run_id,
        provider_session_id: row.provider_session_id,
        uncertain_code: row.uncertain_code,
        claimed_at: row.claimed_at,
        accepted_at: row.accepted_at,
        started_at: row.started_at,
        settled_at: row.settled_at,
        uncertain_at: row.uncertain_at,
        owner_resolved_at: row.owner_resolved_at,
        created_at: row.created_at,
    })
}

async fn require_dispatch(
    conn: &mut SqliteConnection,
    request_id: &str,
) -> Result<Dispatch, SupervisorError> {
    let sql = format!("SELECT {DISPATCH_COLUMNS} FROM supervisor_dispatches WHERE request_id = ?");
    let row: Option<DispatchRow> = sqlx::query_as(&sql)
        .bind(request_id)
        .fetch_optional(&mut *conn)
        .await?;
    row.map(dispatch_from_row)
        .transpose()?
        .ok_or_else(|| SupervisorError::NotFound {
            kind: "dispatch",
            id: request_id.to_owned(),
        })
}

/// Writes the next ready intent of `goal_id` at `revision`.
async fn insert_dispatch(
    conn: &mut SqliteConnection,
    goal_id: &str,
    revision: i64,
    generation: i64,
    instruction: &str,
    now: &str,
) -> Result<Dispatch, SupervisorError> {
    let ordinal: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(ordinal), 0) + 1 FROM supervisor_dispatches
         WHERE goal_id = ? AND goal_revision = ?",
    )
    .bind(goal_id)
    .bind(revision)
    .fetch_one(&mut *conn)
    .await?;
    let request_id = dispatch_request_id(goal_id, revision, ordinal);
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO supervisor_dispatches(goal_id, goal_revision, ordinal, request_id, generation,
                instruction, state, created_at, updated_at)
         VALUES(?,?,?,?,?,?,'ready',?,?) RETURNING id",
    )
    .bind(goal_id)
    .bind(revision)
    .bind(ordinal)
    .bind(&request_id)
    .bind(generation)
    .bind(instruction)
    .bind(now)
    .bind(now)
    .fetch_one(&mut *conn)
    .await?;
    Ok(Dispatch {
        id,
        goal_id: goal_id.to_owned(),
        goal_revision: revision,
        ordinal,
        request_id,
        generation,
        instruction: instruction.to_owned(),
        state: DispatchState::Ready,
        run_id: None,
        provider_session_id: None,
        uncertain_code: None,
        claimed_at: None,
        accepted_at: None,
        started_at: None,
        settled_at: None,
        uncertain_at: None,
        owner_resolved_at: None,
        created_at: now.to_owned(),
    })
}

type WaitRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
);

const WAIT_COLUMNS: &str =
    "wait_id, supervisor_id, goal_id, kind, child_goal_id, event_key, state, created_at, resolved_at";

fn wait_from_row(row: WaitRow) -> Result<Wait, SupervisorError> {
    let (
        wait_id,
        supervisor_id,
        goal_id,
        kind,
        child_goal_id,
        event_key,
        state,
        created_at,
        resolved_at,
    ) = row;
    Ok(Wait {
        wait_id,
        supervisor_id,
        goal_id,
        kind: kind.parse()?,
        child_goal_id,
        event_key,
        state: state.parse()?,
        created_at,
        resolved_at,
    })
}

type WakeRow = (
    i64,
    String,
    String,
    String,
    i64,
    i64,
    String,
    Option<String>,
    String,
);

const WAKE_COLUMNS: &str =
    "id, goal_id, cause, event_id, goal_revision, generation, due_at, consumed_at, created_at";

fn wake_from_row(row: WakeRow) -> Result<PendingWake, SupervisorError> {
    let (id, goal_id, cause, event_id, goal_revision, generation, due_at, consumed_at, created_at) =
        row;
    Ok(PendingWake {
        id,
        goal_id,
        cause: cause.parse()?,
        event_id,
        goal_revision,
        generation,
        due_at,
        consumed_at,
        created_at,
    })
}

/// Persists one wake for `goal` at its current revision and `generation`. A replay of the
/// same (goal, cause, event) returns the row already written.
async fn insert_wake(
    conn: &mut SqliteConnection,
    goal: &SupervisorGoal,
    generation: i64,
    cause: WakeCause,
    event_id: &str,
    due_at: &str,
) -> Result<PendingWake, SupervisorError> {
    sqlx::query(
        "INSERT INTO supervisor_pending_wakes(goal_id, cause, event_id, goal_revision, generation,
                due_at, created_at)
         VALUES(?,?,?,?,?,?,?) ON CONFLICT(goal_id, cause, event_id) DO NOTHING",
    )
    .bind(&goal.goal_id)
    .bind(cause.as_str())
    .bind(event_id)
    .bind(goal.revision)
    .bind(generation)
    .bind(due_at)
    .bind(now_rfc3339())
    .execute(&mut *conn)
    .await?;
    let sql = format!(
        "SELECT {WAKE_COLUMNS} FROM supervisor_pending_wakes
         WHERE goal_id = ? AND cause = ? AND event_id = ?"
    );
    let row: WakeRow = sqlx::query_as(&sql)
        .bind(&goal.goal_id)
        .bind(cause.as_str())
        .bind(event_id)
        .fetch_one(&mut *conn)
        .await?;
    wake_from_row(row)
}

async fn owner_of_supervisor(
    conn: &mut SqliteConnection,
    supervisor_id: &str,
) -> Result<String, SupervisorError> {
    let owner: Option<String> = sqlx::query_scalar(
        "SELECT owner_principal FROM supervisor_identity WHERE supervisor_id = ?",
    )
    .bind(supervisor_id)
    .fetch_optional(&mut *conn)
    .await?;
    owner.ok_or_else(|| SupervisorError::NotFound {
        kind: "supervisor",
        id: supervisor_id.to_owned(),
    })
}

/// Fails unless `project_id` exists and belongs to `supervisor_id`.
async fn require_project_of(
    conn: &mut SqliteConnection,
    supervisor_id: &str,
    project_id: &str,
) -> Result<(), SupervisorError> {
    let owner: Option<String> =
        sqlx::query_scalar("SELECT supervisor_id FROM supervisor_projects WHERE project_id = ?")
            .bind(project_id)
            .fetch_optional(&mut *conn)
            .await?;
    match owner {
        None => Err(SupervisorError::NotFound {
            kind: "project",
            id: project_id.to_owned(),
        }),
        Some(owner) if owner != supervisor_id => {
            Err(SupervisorError::CrossOwner(format!("project {project_id}")))
        }
        Some(_) => Ok(()),
    }
}

/// The owner's supervisor identity, created on first use. One per owner installation:
/// repeated calls return the same UUID.
///
/// Test-only: it does not refuse a second owner, so [`provision_owner`] is the one production
/// writer of identities and keeps one identity per installation.
#[cfg(test)]
pub(crate) async fn ensure_identity(
    pool: &SqlitePool,
    owner_principal: &str,
) -> Result<SupervisorIdentity, SupervisorError> {
    require_id("owner principal", owner_principal, 256)?;
    sqlx::query(
        "INSERT INTO supervisor_identity(supervisor_id, owner_principal, memory_name, created_at)
         VALUES(?,?,?,?) ON CONFLICT(owner_principal) DO NOTHING",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(owner_principal)
    .bind(MEMORY_NAME)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    identity_of(pool, owner_principal)
        .await?
        .ok_or_else(|| SupervisorError::NotFound {
            kind: "supervisor identity",
            id: owner_principal.to_owned(),
        })
}

pub async fn identity_of(
    pool: &SqlitePool,
    owner_principal: &str,
) -> Result<Option<SupervisorIdentity>, SupervisorError> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT supervisor_id, owner_principal, memory_name, created_at
         FROM supervisor_identity WHERE owner_principal = ?",
    )
    .bind(owner_principal)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(supervisor_id, owner_principal, memory_name, created_at)| SupervisorIdentity {
            supervisor_id,
            owner_principal,
            memory_name,
            created_at,
        },
    ))
}

/// Every supervisor identity of this installation, oldest first. Read-only: the engine status
/// view reports it so an operator can see the identity is unchanged across engine restarts.
pub async fn identities(pool: &SqlitePool) -> Result<Vec<SupervisorIdentity>, SupervisorError> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT supervisor_id, owner_principal, memory_name, created_at
         FROM supervisor_identity ORDER BY created_at, supervisor_id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(supervisor_id, owner_principal, memory_name, created_at)| SupervisorIdentity {
                supervisor_id,
                owner_principal,
                memory_name,
                created_at,
            },
        )
        .collect())
}

type ProjectRow = (String, String, String, String, String);

fn project_from_row(row: ProjectRow) -> Project {
    let (project_id, supervisor_id, name, repo_identity, created_at) = row;
    Project {
        project_id,
        supervisor_id,
        name,
        repo_identity,
        created_at,
    }
}

/// The supervisor's project for a workspace/repository identity, created on first use with a
/// new persisted UUID. A raw local path is never the identity; see [`bind_project_host`].
pub async fn create_project(
    pool: &SqlitePool,
    supervisor_id: &str,
    name: &str,
    repo_identity: &str,
) -> Result<Project, SupervisorError> {
    require_uuid("supervisor id", supervisor_id)?;
    require_text("project name", name, 256)?;
    require_id("repository identity", repo_identity, 1024)?;
    let mut tx = pool.begin_write().await?;
    owner_of_supervisor(&mut tx, supervisor_id).await?;
    sqlx::query(
        "INSERT INTO supervisor_projects(project_id, supervisor_id, name, repo_identity, created_at)
         VALUES(?,?,?,?,?) ON CONFLICT(supervisor_id, repo_identity) DO NOTHING",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(supervisor_id)
    .bind(name)
    .bind(repo_identity)
    .bind(now_rfc3339())
    .execute(&mut *tx)
    .await?;
    let row: ProjectRow = sqlx::query_as(
        "SELECT project_id, supervisor_id, name, repo_identity, created_at
         FROM supervisor_projects WHERE supervisor_id = ? AND repo_identity = ?",
    )
    .bind(supervisor_id)
    .bind(repo_identity)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(project_from_row(row))
}

/// Records where a project lives on one host: that host's canonical git common dir. The
/// caller canonicalizes on the host; one directory maps to at most one project.
pub async fn bind_project_host(
    pool: &SqlitePool,
    project_id: &str,
    host_id: &str,
    git_common_dir: &str,
) -> Result<ProjectHost, SupervisorError> {
    require_uuid("project id", project_id)?;
    require_id("host id", host_id, 256)?;
    require_id("git common dir", git_common_dir, 4096)?;
    let mut tx = pool.begin_write().await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM supervisor_projects WHERE project_id = ?)")
            .bind(project_id)
            .fetch_one(&mut *tx)
            .await?;
    if !exists {
        return Err(SupervisorError::NotFound {
            kind: "project",
            id: project_id.to_owned(),
        });
    }
    let now = now_rfc3339();
    let written = sqlx::query(
        "INSERT INTO supervisor_project_hosts(project_id, host_id, git_common_dir, updated_at)
         VALUES(?,?,?,?)
         ON CONFLICT(project_id, host_id) DO UPDATE SET
            git_common_dir = excluded.git_common_dir,
            updated_at = excluded.updated_at",
    )
    .bind(project_id)
    .bind(host_id)
    .bind(git_common_dir)
    .bind(&now)
    .execute(&mut *tx)
    .await;
    match written {
        Ok(_) => {}
        Err(error) if is_unique_violation(&error) => {
            return Err(SupervisorError::Invalid(format!(
                "{git_common_dir} on {host_id} is already bound to another project"
            )));
        }
        Err(error) => return Err(error.into()),
    }
    tx.commit().await?;
    Ok(ProjectHost {
        project_id: project_id.to_owned(),
        host_id: host_id.to_owned(),
        git_common_dir: git_common_dir.to_owned(),
        updated_at: now,
    })
}

/// Sets up this installation's supervisor in one write transaction: its one owner identity,
/// the project for `repo_identity` and the project's mapping on `host_id`. Re-running it with
/// the same arguments changes nothing and returns the same ids.
///
/// It never changes what the installation already holds; each of these is
/// [`SupervisorError::SetupConflict`] and writes nothing:
/// - another owner already has this installation's identity (one identity per installation);
/// - the owner's project for `repo_identity` already has another name;
/// - the project is already mapped on `host_id` to another git common dir;
/// - `git_common_dir` on `host_id` already belongs to another project.
pub async fn provision_owner(
    pool: &SqlitePool,
    owner_principal: &str,
    project_name: &str,
    repo_identity: &str,
    host_id: &str,
    git_common_dir: &str,
) -> Result<(SupervisorIdentity, Project, ProjectHost), SupervisorError> {
    require_id("owner principal", owner_principal, 256)?;
    require_text("project name", project_name, 256)?;
    require_id("repository identity", repo_identity, 1024)?;
    require_id("host id", host_id, 256)?;
    require_id("git common dir", git_common_dir, 4096)?;
    let mut tx = pool.begin_write().await?;

    let owners: Vec<(String, String)> = sqlx::query_as(
        "SELECT supervisor_id, owner_principal FROM supervisor_identity ORDER BY created_at",
    )
    .fetch_all(&mut *tx)
    .await?;
    if !owners.is_empty() && !owners.iter().any(|(_, owner)| owner == owner_principal) {
        let held: Vec<&str> = owners.iter().map(|(_, owner)| owner.as_str()).collect();
        return Err(SupervisorError::SetupConflict(format!(
            "this installation's supervisor already belongs to {}; it is not given a second owner",
            held.join(", ")
        )));
    }
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO supervisor_identity(supervisor_id, owner_principal, memory_name, created_at)
         VALUES(?,?,?,?) ON CONFLICT(owner_principal) DO NOTHING",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(owner_principal)
    .bind(MEMORY_NAME)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let (supervisor_id, owner, memory_name, identity_created): (String, String, String, String) =
        sqlx::query_as(
            "SELECT supervisor_id, owner_principal, memory_name, created_at
             FROM supervisor_identity WHERE owner_principal = ?",
        )
        .bind(owner_principal)
        .fetch_one(&mut *tx)
        .await?;
    let identity = SupervisorIdentity {
        supervisor_id,
        owner_principal: owner,
        memory_name,
        created_at: identity_created,
    };

    let existing: Option<ProjectRow> = sqlx::query_as(
        "SELECT project_id, supervisor_id, name, repo_identity, created_at
         FROM supervisor_projects WHERE supervisor_id = ? AND repo_identity = ?",
    )
    .bind(&identity.supervisor_id)
    .bind(repo_identity)
    .fetch_optional(&mut *tx)
    .await?;
    let project = match existing {
        Some(row) => {
            let project = project_from_row(row);
            if project.name != project_name {
                return Err(SupervisorError::SetupConflict(format!(
                    "project {} for {repo_identity} is named {:?}, not {project_name:?}",
                    project.project_id, project.name
                )));
            }
            project
        }
        None => {
            let project = Project {
                project_id: Uuid::new_v4().to_string(),
                supervisor_id: identity.supervisor_id.clone(),
                name: project_name.to_owned(),
                repo_identity: repo_identity.to_owned(),
                created_at: now.clone(),
            };
            sqlx::query(
                "INSERT INTO supervisor_projects(project_id, supervisor_id, name, repo_identity, created_at)
                 VALUES(?,?,?,?,?)",
            )
            .bind(&project.project_id)
            .bind(&project.supervisor_id)
            .bind(&project.name)
            .bind(&project.repo_identity)
            .bind(&project.created_at)
            .execute(&mut *tx)
            .await?;
            project
        }
    };

    let mapped: Option<(String, String)> = sqlx::query_as(
        "SELECT git_common_dir, updated_at FROM supervisor_project_hosts
         WHERE project_id = ? AND host_id = ?",
    )
    .bind(&project.project_id)
    .bind(host_id)
    .fetch_optional(&mut *tx)
    .await?;
    let claimed_by: Option<String> = sqlx::query_scalar(
        "SELECT project_id FROM supervisor_project_hosts WHERE host_id = ? AND git_common_dir = ?",
    )
    .bind(host_id)
    .bind(git_common_dir)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(other) = claimed_by.filter(|other| *other != project.project_id) {
        return Err(SupervisorError::SetupConflict(format!(
            "{git_common_dir} on {host_id} is already mapped to project {other}"
        )));
    }
    let host = match mapped {
        Some((dir, _)) if dir != git_common_dir => {
            return Err(SupervisorError::SetupConflict(format!(
                "project {} is already mapped on {host_id} to {dir}; it is not moved",
                project.project_id
            )))
        }
        Some((dir, updated_at)) => ProjectHost {
            project_id: project.project_id.clone(),
            host_id: host_id.to_owned(),
            git_common_dir: dir,
            updated_at,
        },
        None => {
            sqlx::query(
                "INSERT INTO supervisor_project_hosts(project_id, host_id, git_common_dir, updated_at)
                 VALUES(?,?,?,?)",
            )
            .bind(&project.project_id)
            .bind(host_id)
            .bind(git_common_dir)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            ProjectHost {
                project_id: project.project_id.clone(),
                host_id: host_id.to_owned(),
                git_common_dir: git_common_dir.to_owned(),
                updated_at: now,
            }
        }
    };
    tx.commit().await?;
    Ok((identity, project, host))
}

/// The project whose canonical git common dir on `host_id` is `git_common_dir`.
pub async fn project_for_host(
    pool: &SqlitePool,
    host_id: &str,
    git_common_dir: &str,
) -> Result<Option<Project>, SupervisorError> {
    let row: Option<ProjectRow> = sqlx::query_as(
        "SELECT p.project_id, p.supervisor_id, p.name, p.repo_identity, p.created_at
         FROM supervisor_projects p
         JOIN supervisor_project_hosts h ON h.project_id = p.project_id
         WHERE h.host_id = ? AND h.git_common_dir = ?",
    )
    .bind(host_id)
    .bind(git_common_dir)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(project_from_row))
}

/// Binds a channel address to the supervisor, optionally scoped to a project and goal. The
/// authenticated principal must be the supervisor's owner: sharing a supervisor never shares
/// it with another principal. Rebinding the same address updates its scope.
pub async fn bind_channel(
    pool: &SqlitePool,
    supervisor_id: &str,
    address: &ChannelAddress,
    project_id: Option<&str>,
    goal_id: Option<&str>,
) -> Result<ChannelBinding, SupervisorError> {
    require_uuid("supervisor id", supervisor_id)?;
    require_id("channel id", &address.channel_id, 256)?;
    require_id("owner principal", &address.owner_principal, 256)?;
    for (label, value) in [
        ("workspace id", &address.workspace_id),
        ("account id", &address.account_id),
        ("thread id", &address.thread_id),
    ] {
        if !value.is_empty() {
            require_id(label, value, 256)?;
        }
    }
    let mut tx = pool.begin_write().await?;
    let owner = owner_of_supervisor(&mut tx, supervisor_id).await?;
    if owner != address.owner_principal {
        return Err(SupervisorError::CrossOwner(format!(
            "{} channel {}",
            address.provider, address.channel_id
        )));
    }
    if let Some(project_id) = project_id {
        require_project_of(&mut tx, supervisor_id, project_id).await?;
    }
    if let Some(goal_id) = goal_id {
        let goal = require_goal(&mut tx, goal_id).await?;
        if goal.supervisor_id != supervisor_id {
            return Err(SupervisorError::CrossOwner(format!("goal {goal_id}")));
        }
        if project_id.is_some_and(|project_id| project_id != goal.project_id) {
            return Err(SupervisorError::Invalid(format!(
                "goal {goal_id} is not in the bound project"
            )));
        }
    }
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO supervisor_channel_bindings(binding_id, supervisor_id, provider, workspace_id,
                account_id, channel_id, thread_id, owner_principal, project_id, goal_id,
                created_at, updated_at)
         VALUES(?,?,?,?,?,?,?,?,?,?,?,?)
         ON CONFLICT(provider, workspace_id, account_id, channel_id, thread_id, owner_principal)
         DO UPDATE SET project_id = excluded.project_id, goal_id = excluded.goal_id,
            updated_at = excluded.updated_at",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(supervisor_id)
    .bind(address.provider.as_str())
    .bind(&address.workspace_id)
    .bind(&address.account_id)
    .bind(&address.channel_id)
    .bind(&address.thread_id)
    .bind(&address.owner_principal)
    .bind(project_id)
    .bind(goal_id)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let (binding_id, created_at, updated_at): (String, String, String) = sqlx::query_as(
        "SELECT binding_id, created_at, updated_at FROM supervisor_channel_bindings
         WHERE provider = ? AND workspace_id = ? AND account_id = ? AND channel_id = ?
           AND thread_id = ? AND owner_principal = ?",
    )
    .bind(address.provider.as_str())
    .bind(&address.workspace_id)
    .bind(&address.account_id)
    .bind(&address.channel_id)
    .bind(&address.thread_id)
    .bind(&address.owner_principal)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ChannelBinding {
        binding_id,
        supervisor_id: supervisor_id.to_owned(),
        address: address.clone(),
        project_id: project_id.map(str::to_owned),
        goal_id: goal_id.map(str::to_owned),
        created_at,
        updated_at,
    })
}

/// An earlier admission of the same source message.
struct Admitted {
    supervisor_id: String,
    owner_principal: String,
    body_sha256: String,
    current_revision: i64,
    receipt: AdmissionReceipt,
}

async fn admitted_request(
    conn: &mut SqliteConnection,
    source_adapter: ChannelProvider,
    source_message_id: &str,
) -> Result<Option<Admitted>, SupervisorError> {
    type AdmittedRow = (String, String, String, String, String, i64, i64, String);
    let row: Option<AdmittedRow> = sqlx::query_as(
        "SELECT r.supervisor_id, r.owner_principal, r.body_sha256, r.goal_id, g.lane_key,
                r.admitted_revision, g.revision, r.created_at
         FROM supervisor_requests r JOIN supervisor_goals g ON g.goal_id = r.goal_id
         WHERE r.source_adapter = ? AND r.source_message_id = ?",
    )
    .bind(source_adapter.as_str())
    .bind(source_message_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(
        |(
            supervisor_id,
            owner_principal,
            body_sha256,
            goal_id,
            lane_key,
            admitted_revision,
            current_revision,
            admitted_at,
        )| Admitted {
            receipt: AdmissionReceipt {
                supervisor_id: supervisor_id.clone(),
                dispatch_request_id: dispatch_request_id(&goal_id, admitted_revision, 1),
                goal_id,
                lane_key,
                source_adapter,
                source_message_id: source_message_id.to_owned(),
                revision: admitted_revision,
                admitted_at,
            },
            supervisor_id,
            owner_principal,
            body_sha256,
            current_revision,
        },
    ))
}

/// The adapter of an earlier admission of an `adopt:<lane>` message id, from any adapter.
async fn adoption_adapter(
    conn: &mut SqliteConnection,
    source_message_id: &str,
) -> Result<Option<ChannelProvider>, SupervisorError> {
    let adapter: Option<String> = sqlx::query_scalar(
        "SELECT source_adapter FROM supervisor_requests WHERE source_message_id = ? LIMIT 1",
    )
    .bind(source_message_id)
    .fetch_optional(&mut *conn)
    .await?;
    adapter.map(|adapter| adapter.parse::<ChannelProvider>()).transpose()
}

/// The receipt of an earlier admission, if it is a replay of the same request: the same
/// supervisor and owner (`CROSS_OWNER` otherwise) and the same body (`REQUEST_CONFLICT`
/// otherwise).
fn replayed_receipt(
    admitted: Admitted,
    supervisor_id: &str,
    owner_principal: &str,
    source_adapter: ChannelProvider,
    source_message_id: &str,
    body_sha256: &str,
) -> Result<AdmissionReceipt, SupervisorError> {
    if admitted.supervisor_id != supervisor_id || admitted.owner_principal != owner_principal {
        return Err(SupervisorError::CrossOwner(format!(
            "{source_adapter} message {source_message_id}"
        )));
    }
    if admitted.body_sha256 != body_sha256 {
        return Err(SupervisorError::RequestBodyChanged {
            source_adapter,
            source_message_id: source_message_id.to_owned(),
            goal_id: admitted.receipt.goal_id,
            revision: admitted.current_revision,
        });
    }
    Ok(admitted.receipt)
}

/// The receipt of an earlier admission of this source message, checked exactly like
/// [`admit_request`]'s replay (owner, then body hash), or `None` when the message was never
/// admitted. Admission handlers call it before resolving a scope, so a same-body replay always
/// returns the existing goal whatever the current projects and hosts are.
pub async fn replayed_admission(
    pool: &SqlitePool,
    supervisor_id: &str,
    owner_principal: &str,
    source_adapter: ChannelProvider,
    source_message_id: &str,
    body: &str,
) -> Result<Option<AdmissionReceipt>, SupervisorError> {
    let mut conn = pool.acquire().await?;
    let Some(admitted) = admitted_request(&mut conn, source_adapter, source_message_id).await?
    else {
        return Ok(None);
    };
    replayed_receipt(
        admitted,
        supervisor_id,
        owner_principal,
        source_adapter,
        source_message_id,
        &sha256_hex(body),
    )
    .map(Some)
}

/// Status, attached pane, legacy proposal state and legacy goal mode of a legacy lane.
type LegacyLaneRow = (String, Option<String>, Option<String>, Option<String>);

/// Refuses a legacy lane that cannot be adopted at `admitted_mode` now. `LEGACY_LANE_LIVE`: a
/// worker attached to a lane that has not ended, a legacy goal turn `pending` (committed by a
/// loop pass or an approval before its send) or `sent`, or a lane close removing its worktree.
/// `LEGACY_TERMS_CHANGED`: a legacy goal that became stricter after the adoption read it (now
/// `off`, or `supervised` under a `full` adoption). `NOT_FOUND` for an unknown lane.
///
/// Runs under the admission's write lock. The legacy goal loop re-checks ownership after it
/// commits a proposal `pending` and before it sends, and a lane close claims each worktree
/// `removing` in a statement that refuses an adopted lane, so of a racing adoption and either
/// one exactly one wins.
///
/// The adopted requirements and budget are re-checked by the caller against
/// [`AdmitTerms::legacy_goal_read`].
async fn legacy_lane_live(
    conn: &mut SqliteConnection,
    lane_key: &str,
    admitted_mode: GoalMode,
) -> Result<(), SupervisorError> {
    let row: Option<LegacyLaneRow> = sqlx::query_as(
        "SELECT l.status, l.omo_thread_id, g.proposal_state, g.mode
         FROM lanes l LEFT JOIN lane_goal g ON g.lane_key = l.key WHERE l.key = ?",
    )
    .bind(lane_key)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((status, pane, proposal_state, legacy_mode)) = row else {
        return Err(SupervisorError::NotFound {
            kind: "lane",
            id: lane_key.to_owned(),
        });
    };
    let stricter = match legacy_mode.as_deref() {
        Some(mode) if mode == GoalMode::Off.as_str() => true,
        Some(mode) if mode == GoalMode::Supervised.as_str() => admitted_mode == GoalMode::Full,
        _ => false,
    };
    if stricter {
        let mode = legacy_mode.unwrap_or_default();
        return Err(SupervisorError::LegacyTermsChanged {
            lane_key: lane_key.to_owned(),
            why: format!(
                "its legacy goal changed to {mode}; adopt it again to take its current terms"
            ),
        });
    }
    let ended = matches!(status.as_str(), "verified_done" | "closed" | "failed");
    let attached = !ended && pane.as_deref().is_some_and(|pane| !pane.is_empty());
    let turn_in_flight = [ProposalState::Pending, ProposalState::Sent]
        .iter()
        .any(|state| proposal_state.as_deref() == Some(state.as_str()));
    let closing: Option<String> = sqlx::query_scalar(
        "SELECT path FROM lane_owned_worktrees WHERE lane_key = ? AND state = 'removing' LIMIT 1",
    )
    .bind(lane_key)
    .fetch_optional(&mut *conn)
    .await?;
    if !attached && !turn_in_flight && closing.is_none() {
        return Ok(());
    }
    let mut why = status;
    if attached {
        why.push_str(", worker attached");
    }
    if turn_in_flight {
        why.push_str(", legacy turn in flight");
    }
    if let Some(path) = closing {
        why.push_str(&format!(", worktree {path} being removed by a lane close"));
    }
    why.push_str(
        "; end it, let its legacy goal loop stop and let a close finish or fail before adopting it",
    );
    Err(SupervisorError::LegacyLaneLive {
        lane_key: lane_key.to_owned(),
        why,
    })
}

/// Admits one owner request in a single write transaction: the lane, its goal contract, the
/// supervisor goal (`ready`), the typed execution binding, the request dedupe row and the
/// initial ready dispatch intent. Nothing is visible until commit; a failure leaves none of
/// them. Acknowledge the owner only from the returned receipt.
///
/// A replay of the same source message with the same body returns the original receipt and
/// writes nothing. A changed body is [`SupervisorError::RequestBodyChanged`], never a second
/// launch.
pub async fn admit_request(
    pool: &SqlitePool,
    request: &AdmitRequest,
) -> Result<Admission, SupervisorError> {
    admit_request_with(pool, request, &AdmitTerms::default()).await
}

/// [`admit_request`] with [`AdmitTerms`] checked and applied in the same transaction: an
/// owner hold commits the goal as `awaiting_owner`, and an adoption re-checks its legacy lane
/// before anything is written. A replay returns the original receipt before either applies;
/// for an adoption, an `adopt:<lane>` admission from any adapter is that replay.
pub async fn admit_request_with(
    pool: &SqlitePool,
    request: &AdmitRequest,
    terms: &AdmitTerms,
) -> Result<Admission, SupervisorError> {
    admit_request_bound(pool, request, terms, None).await
}

/// [`admit_request_with`] that also binds an existing pane the owner chose (`session`) to
/// the new goal's first generation, in the same transaction: `provider_session_id` is set
/// before any dispatcher can read the binding, so the goal is delivered to that pane (on a
/// paired host, through the paired path) and no pane is launched for it.
///
/// The session's host must be the goal's target host and the worker a pane (checked first,
/// before anything is read). A session another open goal (not completed or cancelled) is
/// bound to on that host is refused with [`SupervisorError::SessionAlreadyBound`]; that check
/// runs after the replay lookup, so a replay of an admitted request returns its original
/// receipt (and binding) even when its pane is now held.
pub async fn admit_request_bound(
    pool: &SqlitePool,
    request: &AdmitRequest,
    terms: &AdmitTerms,
    session: Option<&BoundSession>,
) -> Result<Admission, SupervisorError> {
    request.validate()?;
    if let Some(session) = session {
        require_id("provider session id", &session.session_id, MAX_ID_CHARS)?;
        if session.host_id != request.target.host_id {
            return Err(SupervisorError::Invalid(format!(
                "session {} is on host {}, but the goal targets host {}",
                session.session_id, session.host_id, request.target.host_id
            )));
        }
        if request.target.kind != ExecutionKind::Pty {
            return Err(SupervisorError::Invalid(
                "only a terminal (pty) worker can be bound to an existing pane".into(),
            ));
        }
    }
    match &terms.adopts_lane {
        Some(lane) if request.source_message_id != format!("{ADOPT_MESSAGE_PREFIX}{lane}") => {
            return Err(SupervisorError::Invalid(format!(
                "an adoption of lane {lane} must use source message id {ADOPT_MESSAGE_PREFIX}{lane}"
            )))
        }
        None if request.source_message_id.starts_with(ADOPT_MESSAGE_PREFIX) => {
            return Err(SupervisorError::Invalid(format!(
                "source message ids starting with {ADOPT_MESSAGE_PREFIX} are reserved for adoption"
            )))
        }
        Some(lane) if terms.legacy_goal_read.is_none() => {
            return Err(SupervisorError::Invalid(format!(
                "an adoption of lane {lane} must carry the legacy goal it read"
            )))
        }
        _ => {}
    }
    if let Some(detail) = &terms.owner_hold {
        require_text("owner hold detail", detail, MAX_BODY_CHARS)?;
    }
    let body_sha256 = sha256_hex(&request.body);
    let operation_grant = serde_json::to_string(&request.operation_grant)
        .map_err(|error| SupervisorError::Invalid(format!("operation grant: {error}")))?;
    let mut tx = pool.begin_write().await?;
    // A lane is adopted once, whichever adapter asks: the dedupe key is (adapter, message id),
    // so under the write lock an adoption first looks for `adopt:<lane>` from any adapter and
    // replays that admission (same owner and body) or refuses, never admitting a second goal.
    let replay_adapter = match &terms.adopts_lane {
        Some(_) => adoption_adapter(&mut tx, &request.source_message_id)
            .await?
            .unwrap_or(request.source_adapter),
        None => request.source_adapter,
    };
    if let Some(admitted) =
        admitted_request(&mut tx, replay_adapter, &request.source_message_id).await?
    {
        return Ok(Admission {
            receipt: replayed_receipt(
                admitted,
                &request.supervisor_id,
                &request.owner_principal,
                replay_adapter,
                &request.source_message_id,
                &body_sha256,
            )?,
            duplicate: true,
        });
    }

    let owner = owner_of_supervisor(&mut tx, &request.supervisor_id).await?;
    if owner != request.owner_principal {
        return Err(SupervisorError::CrossOwner(format!(
            "supervisor {}",
            request.supervisor_id
        )));
    }
    require_project_of(&mut tx, &request.supervisor_id, &request.project_id).await?;
    let mapped: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM supervisor_project_hosts WHERE project_id = ? AND host_id = ?)",
    )
    .bind(&request.project_id)
    .bind(&request.target.host_id)
    .fetch_one(&mut *tx)
    .await?;
    if !mapped {
        return Err(SupervisorError::NotFound {
            kind: "project host mapping",
            id: format!("{}@{}", request.project_id, request.target.host_id),
        });
    }
    if let Some(binding_id) = &request.channel_binding_id {
        let binding: Option<(String, String)> = sqlx::query_as(
            "SELECT supervisor_id, owner_principal FROM supervisor_channel_bindings
             WHERE binding_id = ?",
        )
        .bind(binding_id)
        .fetch_optional(&mut *tx)
        .await?;
        match binding {
            None => {
                return Err(SupervisorError::NotFound {
                    kind: "channel binding",
                    id: binding_id.clone(),
                })
            }
            Some((supervisor_id, owner_principal))
                if supervisor_id != request.supervisor_id
                    || owner_principal != request.owner_principal =>
            {
                return Err(SupervisorError::CrossOwner(format!(
                    "channel binding {binding_id}"
                )))
            }
            Some(_) => {}
        }
    }
    if let Some(session) = session {
        // Under the write lock: of two admissions choosing the same pane exactly one commits.
        let holder: Option<String> = sqlx::query_scalar(
            "SELECT b.goal_id FROM supervisor_execution_bindings b
               JOIN supervisor_goals g ON g.goal_id = b.goal_id
              WHERE b.kind = 'pty' AND b.host_id = ? AND b.provider_session_id = ?
                AND g.run_state NOT IN ('completed','cancelled')
              LIMIT 1",
        )
        .bind(&session.host_id)
        .bind(&session.session_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(goal_id) = holder {
            return Err(SupervisorError::SessionAlreadyBound {
                host_id: session.host_id.clone(),
                session_id: session.session_id.clone(),
                goal_id,
            });
        }
    }
    // Checked under the write lock. A legacy launch attaches its pane only through
    // [`attach_legacy_pane`], which refuses a lane this admission adopted, so SQLite's write
    // serialization lets exactly one of the two commit.
    if let Some(legacy) = &terms.adopts_lane {
        legacy_lane_live(&mut tx, legacy, request.mode).await?;
        if let Some(read) = terms.legacy_goal_read {
            let now: Option<(i64, i64)> =
                sqlx::query_as("SELECT revision, turns_used FROM lane_goal WHERE lane_key = ?")
                    .bind(legacy)
                    .fetch_optional(&mut *tx)
                    .await?;
            if now != read.map(|(revision, used)| (revision, i64::from(used))) {
                return Err(SupervisorError::LegacyTermsChanged {
                    lane_key: legacy.clone(),
                    why: "its legacy goal's requirements, turn limit or turns used changed; adopt it again to take its current terms".into(),
                });
            }
        }
    }

    let goal_uuid = Uuid::new_v4();
    let goal_id = goal_uuid.to_string();
    let lane_key = lane_key_for(&goal_uuid);
    let now = now_rfc3339();
    let lead = format!("supervisor:{}", request.supervisor_id);
    sqlx::query(
        "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, status, done_signals,
                last_activity_at)
         VALUES(?,?,?,?,?,'queued',?,?)",
    )
    .bind(&lane_key)
    .bind(&request.title)
    .bind(&lead)
    .bind(&request.cwd)
    .bind(&lead)
    .bind(LANE_DONE_SIGNALS)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO lane_goal(lane_key, mode, turn_limit, updated_at) VALUES(?,?,?,?)")
        .bind(&lane_key)
        .bind(request.mode.as_str())
        .bind(i64::from(request.turn_limit))
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    for requirement in &request.requirements {
        sqlx::query(
            "INSERT INTO lane_goal_requirements(lane_key, req_id, text, verifier, status,
                    evidence_json, updated_at)
             VALUES(?,?,?,?,'pending','[]',?)",
        )
        .bind(&lane_key)
        .bind(requirement.id.trim())
        .bind(&requirement.text)
        .bind(&requirement.verifier)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "INSERT INTO supervisor_goals(goal_id, supervisor_id, project_id, lane_key,
                owner_principal, run_state, revision, operation_grant, created_at, updated_at)
         VALUES(?,?,?,?,?,?,1,?,?,?)",
    )
    .bind(&goal_id)
    .bind(&request.supervisor_id)
    .bind(&request.project_id)
    .bind(&lane_key)
    .bind(&request.owner_principal)
    .bind(RunState::Ready.as_str())
    .bind(&operation_grant)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO supervisor_execution_bindings(goal_id, kind, host_id, provider_session_id,
                generation, updated_at)
         VALUES(?,?,?,?,1,?)",
    )
    .bind(&goal_id)
    .bind(request.target.kind.as_str())
    .bind(&request.target.host_id)
    .bind(session.map(|session| session.session_id.as_str()))
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO supervisor_requests(source_adapter, source_message_id, body, body_sha256,
                supervisor_id, goal_id, owner_principal, channel_binding_id, admitted_revision,
                created_at)
         VALUES(?,?,?,?,?,?,?,?,1,?)",
    )
    .bind(request.source_adapter.as_str())
    .bind(&request.source_message_id)
    .bind(&request.body)
    .bind(&body_sha256)
    .bind(&request.supervisor_id)
    .bind(&goal_id)
    .bind(&request.owner_principal)
    .bind(request.channel_binding_id.as_deref())
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let dispatch = insert_dispatch(&mut tx, &goal_id, 1, 1, &request.instruction, &now).await?;
    if let Some(detail) = &terms.owner_hold {
        // The same end state as an owner hold right after admission, without the window.
        sqlx::query(
            "UPDATE supervisor_goals SET run_state = ?, state_detail = ?, revision = 2, updated_at = ?
             WHERE goal_id = ?",
        )
        .bind(RunState::AwaitingOwner.as_str())
        .bind(detail)
        .bind(&now)
        .bind(&goal_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE supervisor_dispatches SET state = 'superseded', updated_at = ?
             WHERE goal_id = ? AND state = 'ready'",
        )
        .bind(&now)
        .bind(&goal_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Admission {
        receipt: AdmissionReceipt {
            supervisor_id: request.supervisor_id.clone(),
            goal_id,
            lane_key,
            source_adapter: request.source_adapter,
            source_message_id: request.source_message_id.clone(),
            revision: 1,
            dispatch_request_id: dispatch.request_id,
            admitted_at: now,
        },
        duplicate: false,
    })
}

pub async fn goal_of(
    pool: &SqlitePool,
    goal_id: &str,
) -> Result<Option<SupervisorGoal>, SupervisorError> {
    let mut conn = pool.acquire().await?;
    load_goal(&mut conn, goal_id).await
}

pub async fn execution_binding_of(
    pool: &SqlitePool,
    goal_id: &str,
) -> Result<Option<ExecutionBinding>, SupervisorError> {
    let mut conn = pool.acquire().await?;
    load_binding(&mut conn, goal_id).await
}

/// Every dispatch of `goal_id`, oldest first, with its receipts. Read-only.
pub async fn dispatches_of(
    pool: &SqlitePool,
    goal_id: &str,
) -> Result<Vec<Dispatch>, SupervisorError> {
    let sql = format!("SELECT {DISPATCH_COLUMNS} FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id");
    let rows: Vec<DispatchRow> = sqlx::query_as(&sql).bind(goal_id).fetch_all(pool).await?;
    rows.into_iter().map(dispatch_from_row).collect()
}

/// True when the goal has a started, unsettled dispatch on its binding's current generation,
/// run and provider session.
async fn has_live_run(conn: &mut SqliteConnection, goal_id: &str) -> Result<bool, SupervisorError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM supervisor_dispatches d
            JOIN supervisor_execution_bindings b ON b.goal_id = d.goal_id
            WHERE d.goal_id = ? AND d.state = 'started' AND d.generation = b.generation
              AND d.run_id = b.run_id AND d.provider_session_id = b.provider_session_id)",
    )
    .bind(goal_id)
    .fetch_one(&mut *conn)
    .await?)
}

/// The request id of the goal's dispatch in flight on the binding's current generation, if
/// any. See [`Dispatch::is_in_flight`].
async fn in_flight_dispatch(
    conn: &mut SqliteConnection,
    goal_id: &str,
) -> Result<Option<String>, SupervisorError> {
    let sql = format!(
        "SELECT d.request_id FROM supervisor_dispatches d
         JOIN supervisor_execution_bindings b
           ON b.goal_id = d.goal_id AND b.generation = d.generation
         WHERE d.goal_id = ? AND {IN_FLIGHT}
         ORDER BY d.id LIMIT 1"
    );
    Ok(sqlx::query_scalar(&sql)
        .bind(goal_id)
        .fetch_optional(&mut *conn)
        .await?)
}

/// Refuses with `DISPATCH_IN_FLIGHT` while the goal has a dispatch in flight.
async fn require_nothing_in_flight(
    conn: &mut SqliteConnection,
    goal_id: &str,
) -> Result<(), SupervisorError> {
    match in_flight_dispatch(conn, goal_id).await? {
        Some(request_id) => Err(SupervisorError::DispatchInFlight {
            goal_id: goal_id.to_owned(),
            request_id,
        }),
        None => Ok(()),
    }
}

/// Compare-and-set of a goal's run state: moves it only when the goal is still at
/// `expected_revision` and in `expected_state` (otherwise `REVISION_CONFLICT`) and the table
/// allows the move. `dispatching` is entered only by [`claim_dispatch`]; `running` only with a
/// live run; `ready` only while no dispatch is in flight (`DISPATCH_IN_FLIGHT`), so a busy
/// worker never gets a second instruction; an unresolved uncertain delivery leaves
/// `delivery_uncertain` only through [`resolve_uncertain_delivery`] or a live run. Entering an
/// owner hold (`paused`, `awaiting_owner`, `cancelled`) bumps the
/// revision and supersedes every unsent intent of an older revision, so a decision made before
/// the hold can never dispatch; cancelling also cancels pending waits. All in one transaction.
/// A dispatch already claimed when a hold lands is not superseded here, because it may be in
/// flight; the dispatcher releases it with [`confirm_claim_before_transmit`].
/// After commit it pokes this process's wake scheduler ([`wakes::poke`]), so an owner pause,
/// cancel or resume is delivered without waiting for the scheduler's rescan.
pub async fn transition_goal(
    pool: &SqlitePool,
    goal_id: &str,
    expected_revision: i64,
    expected_state: RunState,
    to: RunState,
    detail: Option<&str>,
) -> Result<SupervisorGoal, SupervisorError> {
    let mut tx = pool.begin_write().await?;
    let goal =
        transition_goal_in(&mut tx, goal_id, expected_revision, expected_state, to, detail).await?;
    tx.commit().await?;
    wakes::poke();
    Ok(goal)
}

/// Holds a goal `degraded` with `detail` only while `run_id` is still the live run of the
/// started dispatch `request_id` and the goal is still `running`, checked and moved in one
/// write transaction so a settle or owner action committed meanwhile is never overwritten.
/// Returns whether the goal was held; nothing is written otherwise.
pub async fn hold_live_run(
    pool: &SqlitePool,
    goal_id: &str,
    request_id: &str,
    run_id: &str,
    detail: &str,
) -> Result<bool, SupervisorError> {
    let mut tx = pool.begin_write().await?;
    let live: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT state, run_id FROM supervisor_dispatches WHERE request_id = ?")
            .bind(request_id)
            .fetch_optional(&mut *tx)
            .await?;
    let is_live = matches!(
        &live,
        Some((state, live_run)) if state == "started" && live_run.as_deref() == Some(run_id)
    );
    if !is_live {
        return Ok(false);
    }
    let goal = require_goal(&mut tx, goal_id).await?;
    if goal.run_state != RunState::Running {
        return Ok(false);
    }
    transition_goal_in(
        &mut tx,
        goal_id,
        goal.revision,
        RunState::Running,
        RunState::Degraded,
        Some(detail),
    )
    .await?;
    tx.commit().await?;
    wakes::poke();
    Ok(true)
}

/// [`transition_goal`]'s checks and writes, inside the caller's transaction. Nothing is
/// written when it refuses.
async fn transition_goal_in(
    conn: &mut SqliteConnection,
    goal_id: &str,
    expected_revision: i64,
    expected_state: RunState,
    to: RunState,
    detail: Option<&str>,
) -> Result<SupervisorGoal, SupervisorError> {
    if to == RunState::Dispatching {
        return Err(SupervisorError::Invalid(
            "a goal enters dispatching only by claiming its ready dispatch".into(),
        ));
    }
    let goal = require_goal(conn, goal_id).await?;
    check_revision(&goal, expected_revision)?;
    if goal.run_state != expected_state {
        return Err(SupervisorError::StateConflict {
            goal_id: goal_id.to_owned(),
            expected: expected_state,
            actual: goal.run_state,
        });
    }
    if !goal.run_state.can_transition_to(to) {
        return Err(SupervisorError::IllegalTransition {
            goal_id: goal_id.to_owned(),
            from: goal.run_state,
            to,
        });
    }
    if to == RunState::Running && !has_live_run(conn, goal_id).await? {
        return Err(SupervisorError::NoLiveRun {
            goal_id: goal_id.to_owned(),
        });
    }
    if to == RunState::Ready {
        require_nothing_in_flight(conn, goal_id).await?;
    }
    let revision = if to.is_owner_hold() {
        goal.revision + 1
    } else {
        goal.revision
    };
    let now = now_rfc3339();
    let moved = sqlx::query(
        "UPDATE supervisor_goals SET run_state = ?, state_detail = ?, revision = ?, updated_at = ?
         WHERE goal_id = ? AND revision = ? AND run_state = ?",
    )
    .bind(to.as_str())
    .bind(detail)
    .bind(revision)
    .bind(&now)
    .bind(goal_id)
    .bind(expected_revision)
    .bind(expected_state.as_str())
    .execute(&mut *conn)
    .await?;
    if moved.rows_affected() != 1 {
        return Err(SupervisorError::RevisionConflict {
            goal_id: goal_id.to_owned(),
            expected: expected_revision,
            actual: goal.revision,
        });
    }
    if to.is_owner_hold() {
        sqlx::query(
            "UPDATE supervisor_dispatches SET state = 'superseded', updated_at = ?
             WHERE goal_id = ? AND state = 'ready' AND goal_revision < ?",
        )
        .bind(&now)
        .bind(goal_id)
        .bind(revision)
        .execute(&mut *conn)
        .await?;
        revoke_claimed(conn, goal_id).await?;
    }
    if to == RunState::Cancelled {
        sqlx::query(
            "UPDATE supervisor_waits SET state = 'cancelled', resolved_at = ?
             WHERE goal_id = ? AND state = 'pending'",
        )
        .bind(&now)
        .bind(goal_id)
        .execute(&mut *conn)
        .await?;
    }
    Ok(SupervisorGoal {
        run_state: to,
        state_detail: detail.map(str::to_owned),
        revision,
        updated_at: now,
        ..goal
    })
}

/// Records an owner correction: bumps the goal revision and supersedes every unsent intent
/// of an older revision, so a stale instruction is never sent. The caller enqueues the
/// corrected instruction with [`enqueue_dispatch`] at the new revision.
/// After commit it pokes this process's wake scheduler ([`wakes::poke`]).
pub async fn revise_goal(
    pool: &SqlitePool,
    goal_id: &str,
    expected_revision: i64,
) -> Result<SupervisorGoal, SupervisorError> {
    let mut tx = pool.begin_write().await?;
    let goal = revise_goal_in(&mut tx, goal_id, expected_revision).await?;
    tx.commit().await?;
    wakes::poke();
    Ok(goal)
}

/// [`revise_goal`]'s checks and writes inside the caller's write transaction, so an owner
/// correction commits its contract, revision bump and corrected intent together. Every
/// owner correction goes through here, so this is the one place a claimed send of the goal
/// is revoked in the typing fence ([`revoke_claimed`]); the claimed row itself is left to the
/// dispatcher, whose pre-transmit check sees the moved revision and releases it untransmitted.
pub(crate) async fn revise_goal_in(
    conn: &mut SqliteConnection,
    goal_id: &str,
    expected_revision: i64,
) -> Result<SupervisorGoal, SupervisorError> {
    let goal = require_goal(conn, goal_id).await?;
    check_revision(&goal, expected_revision)?;
    check_open(&goal)?;
    let revision = goal.revision + 1;
    let now = now_rfc3339();
    let moved = sqlx::query(
        "UPDATE supervisor_goals SET revision = ?, updated_at = ? WHERE goal_id = ? AND revision = ?",
    )
    .bind(revision)
    .bind(&now)
    .bind(goal_id)
    .bind(expected_revision)
    .execute(&mut *conn)
    .await?;
    if moved.rows_affected() != 1 {
        return Err(SupervisorError::RevisionConflict {
            goal_id: goal_id.to_owned(),
            expected: expected_revision,
            actual: goal.revision,
        });
    }
    sqlx::query(
        "UPDATE supervisor_dispatches SET state = 'superseded', updated_at = ?
         WHERE goal_id = ? AND state = 'ready' AND goal_revision < ?",
    )
    .bind(&now)
    .bind(goal_id)
    .bind(revision)
    .execute(&mut *conn)
    .await?;
    revoke_claimed(conn, goal_id).await?;
    Ok(SupervisorGoal {
        revision,
        updated_at: now,
        ..goal
    })
}

/// An owner hold or correction landed while a dispatch of `goal_id` is claimed: that claim
/// may already be past its confirm call and waiting for the pane, so it is revoked in the
/// in-process typing fence ([`delivery::check_before_typing`]) and nothing of it is typed.
async fn revoke_claimed(conn: &mut SqliteConnection, goal_id: &str) -> Result<(), SupervisorError> {
    let claimed: Vec<String> = sqlx::query_scalar(
        "SELECT request_id FROM supervisor_dispatches WHERE goal_id = ? AND state = 'claimed'",
    )
    .bind(goal_id)
    .fetch_all(&mut *conn)
    .await?;
    delivery::revoke(claimed);
    Ok(())
}

/// Writes the goal's next ready dispatch intent at `expected_revision` and the binding's
/// current generation. A goal holds at most one unsent intent.
pub async fn enqueue_dispatch(
    pool: &SqlitePool,
    goal_id: &str,
    expected_revision: i64,
    instruction: &str,
) -> Result<Dispatch, SupervisorError> {
    require_text("instruction", instruction, MAX_BODY_CHARS)?;
    let mut tx = pool.begin_write().await?;
    let goal = require_goal(&mut tx, goal_id).await?;
    check_revision(&goal, expected_revision)?;
    check_open(&goal)?;
    let generation = require_binding(&mut tx, goal_id).await?.generation;
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM supervisor_dispatches WHERE goal_id = ? AND state = 'ready')",
    )
    .bind(goal_id)
    .fetch_one(&mut *tx)
    .await?;
    if pending {
        return Err(SupervisorError::DispatchPending {
            goal_id: goal_id.to_owned(),
        });
    }
    let dispatch = insert_dispatch(
        &mut tx,
        goal_id,
        goal.revision,
        generation,
        instruction,
        &now_rfc3339(),
    )
    .await?;
    tx.commit().await?;
    Ok(dispatch)
}

/// Unsent intents whose goal is `ready` at the intent's revision and has no dispatch in flight,
/// oldest first: exactly the intents [`claim_dispatch`] would accept.
pub async fn ready_dispatches(
    pool: &SqlitePool,
    limit: u32,
) -> Result<Vec<Dispatch>, SupervisorError> {
    let sql = format!(
        "SELECT {DISPATCH_COLUMNS} FROM supervisor_dispatches
         WHERE state = 'ready' AND EXISTS(
            SELECT 1 FROM supervisor_goals g
            WHERE g.goal_id = supervisor_dispatches.goal_id AND g.run_state = 'ready'
              AND g.revision = supervisor_dispatches.goal_revision)
           AND NOT EXISTS(
            SELECT 1 FROM supervisor_dispatches d
            JOIN supervisor_execution_bindings b
              ON b.goal_id = d.goal_id AND b.generation = d.generation
            WHERE d.goal_id = supervisor_dispatches.goal_id AND {IN_FLIGHT})
         ORDER BY created_at, id LIMIT ?"
    );
    let rows: Vec<DispatchRow> = sqlx::query_as(&sql)
        .bind(i64::from(limit))
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(dispatch_from_row).collect()
}

/// Claims a ready intent for transmission: the intent moves to `claimed` and its goal to
/// `dispatching` together. Refused unless the goal is `ready` at `expected_revision`, the
/// intent was written for that revision, and `generation` is the binding's and the intent's
/// current generation, and while another dispatch of the goal is in flight
/// (`DISPATCH_IN_FLIGHT`). At most one claimer wins; the request id is transmitted once,
/// right after [`confirm_claim_before_transmit`].
pub async fn claim_dispatch(
    pool: &SqlitePool,
    request_id: &str,
    expected_revision: i64,
    generation: i64,
) -> Result<Dispatch, SupervisorError> {
    let mut tx = pool.begin_write().await?;
    let dispatch = require_dispatch(&mut tx, request_id).await?;
    let goal = require_goal(&mut tx, &dispatch.goal_id).await?;
    check_revision(&goal, expected_revision)?;
    if dispatch.state != DispatchState::Ready {
        return Err(SupervisorError::NotClaimable {
            request_id: request_id.to_owned(),
            why: format!("it is {}", dispatch.state),
        });
    }
    if dispatch.goal_revision != goal.revision {
        return Err(SupervisorError::NotClaimable {
            request_id: request_id.to_owned(),
            why: format!(
                "it was written for revision {}, the goal is at {}",
                dispatch.goal_revision, goal.revision
            ),
        });
    }
    let binding = require_binding(&mut tx, &goal.goal_id).await?;
    check_generation(binding.generation, generation)?;
    check_generation(dispatch.generation, generation)?;
    if goal.run_state != RunState::Ready {
        return Err(SupervisorError::NotClaimable {
            request_id: request_id.to_owned(),
            why: format!("the goal is {}", goal.run_state),
        });
    }
    require_nothing_in_flight(&mut tx, &goal.goal_id).await?;
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE supervisor_dispatches SET state = 'claimed', claimed_at = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&now)
    .bind(&now)
    .bind(dispatch.id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE supervisor_goals SET run_state = 'dispatching', state_detail = NULL,
                updated_at = ?
         WHERE goal_id = ?",
    )
    .bind(&now)
    .bind(&goal.goal_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Dispatch {
        state: DispatchState::Claimed,
        claimed_at: Some(now),
        ..dispatch
    })
}

/// The dispatcher's last check, made right before it transmits a claimed dispatch and after
/// any work it did since claiming. Returns the dispatch when it is still `claimed`, the goal is
/// still `dispatching` at the dispatch's revision and the binding is still at its generation:
/// transmit then, immediately, with no other await in between. `Ok` persists nothing, so a
/// `claimed` row cannot tell whether its bytes left; restart reconciliation therefore treats
/// every `claimed` dispatch as uncertain ([`reconcile_claims_after_restart`]).
///
/// Otherwise an owner hold, correction or generation change landed after the claim; because
/// the caller has not transmitted, the dispatch is `released` (never sent, not in flight) in
/// the same transaction and `SUPERSEDED` is returned: do not transmit. A goal still
/// `dispatching` returns to `ready` only when no other dispatch is in flight. Bytes already
/// handed to the transport cannot be recalled; their receipts arrive through
/// [`record_receipt`].
pub async fn confirm_claim_before_transmit(
    pool: &SqlitePool,
    request_id: &str,
) -> Result<Dispatch, SupervisorError> {
    let mut tx = pool.begin_write().await?;
    let dispatch = require_dispatch(&mut tx, request_id).await?;
    if dispatch.state != DispatchState::Claimed {
        return Err(SupervisorError::NotClaimable {
            request_id: request_id.to_owned(),
            why: format!("it is {}, not claimed", dispatch.state),
        });
    }
    let goal = require_goal(&mut tx, &dispatch.goal_id).await?;
    let binding = require_binding(&mut tx, &dispatch.goal_id).await?;
    let why = if goal.revision != dispatch.goal_revision {
        Some(format!(
            "goal revision moved from {} to {}",
            dispatch.goal_revision, goal.revision
        ))
    } else if goal.run_state != RunState::Dispatching {
        Some(format!("goal is {}, not dispatching", goal.run_state))
    } else if binding.generation != dispatch.generation {
        Some(format!(
            "execution generation moved from {} to {}",
            dispatch.generation, binding.generation
        ))
    } else {
        None
    };
    let Some(why) = why else {
        return Ok(dispatch);
    };
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE supervisor_dispatches SET state = 'released', updated_at = ?
         WHERE id = ? AND state = 'claimed'",
    )
    .bind(&now)
    .bind(dispatch.id)
    .execute(&mut *tx)
    .await?;
    if goal.run_state == RunState::Dispatching
        && in_flight_dispatch(&mut tx, &goal.goal_id).await?.is_none()
    {
        sqlx::query(
            "UPDATE supervisor_goals SET run_state = 'ready', state_detail = NULL, updated_at = ?
             WHERE goal_id = ? AND run_state = 'dispatching'",
        )
        .bind(&now)
        .bind(&goal.goal_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Err(SupervisorError::ClaimReleased {
        request_id: request_id.to_owned(),
        why,
    })
}

/// `uncertain_code` of a dispatch that restart reconciliation found still `claimed`.
pub const CLAIM_UNCONFIRMED_AT_RESTART: &str = "CLAIM_UNCONFIRMED_AT_RESTART";

/// `uncertain_code` of a dispatch that restart reconciliation found `accepted`: the target
/// took the bytes, but no start was recorded before the restart.
pub const START_UNCONFIRMED_AT_RESTART: &str = "START_UNCONFIRMED_AT_RESTART";

/// The owner's resolution of an uncertain delivery: every unresolved `uncertain` dispatch of
/// the goal is marked resolved (row and evidence kept), so it no longer counts as in flight
/// and a resend may be claimed. From `delivery_uncertain` the goal moves to `ready`; from an
/// owner hold (`paused`, `awaiting_owner`) it stays held. Compare-and-set on
/// `expected_revision` and `expected_state`. Refused with `DISPATCH_IN_FLIGHT` while a
/// dispatch is claimed, accepted or started: those are not uncertain. A real receipt that later
/// arrives for a resolved dispatch is still recorded ([`record_receipt`]).
pub async fn resolve_uncertain_delivery(
    pool: &SqlitePool,
    goal_id: &str,
    expected_revision: i64,
    expected_state: RunState,
    detail: Option<&str>,
) -> Result<SupervisorGoal, SupervisorError> {
    if !matches!(
        expected_state,
        RunState::DeliveryUncertain | RunState::Paused | RunState::AwaitingOwner
    ) {
        return Err(SupervisorError::Invalid(format!(
            "an uncertain delivery is resolved from delivery_uncertain or an owner hold, not \
             {expected_state}"
        )));
    }
    let mut tx = pool.begin_write().await?;
    let goal = require_goal(&mut tx, goal_id).await?;
    check_revision(&goal, expected_revision)?;
    if goal.run_state != expected_state {
        return Err(SupervisorError::StateConflict {
            goal_id: goal_id.to_owned(),
            expected: expected_state,
            actual: goal.run_state,
        });
    }
    let busy: Option<String> = sqlx::query_scalar(
        "SELECT d.request_id FROM supervisor_dispatches d
         JOIN supervisor_execution_bindings b
           ON b.goal_id = d.goal_id AND b.generation = d.generation
         WHERE d.goal_id = ? AND d.state IN ('claimed','accepted','started')
         ORDER BY d.id LIMIT 1",
    )
    .bind(goal_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(request_id) = busy {
        return Err(SupervisorError::DispatchInFlight {
            goal_id: goal_id.to_owned(),
            request_id,
        });
    }
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE supervisor_dispatches SET owner_resolved_at = ?, updated_at = ?
         WHERE goal_id = ? AND state = 'uncertain' AND owner_resolved_at IS NULL",
    )
    .bind(&now)
    .bind(&now)
    .bind(goal_id)
    .execute(&mut *tx)
    .await?;
    let to = if expected_state == RunState::DeliveryUncertain {
        RunState::Ready
    } else {
        expected_state
    };
    let moved = sqlx::query(
        "UPDATE supervisor_goals SET run_state = ?, state_detail = ?, updated_at = ?
         WHERE goal_id = ? AND revision = ? AND run_state = ?",
    )
    .bind(to.as_str())
    .bind(detail)
    .bind(&now)
    .bind(goal_id)
    .bind(expected_revision)
    .bind(expected_state.as_str())
    .execute(&mut *tx)
    .await?;
    if moved.rows_affected() != 1 {
        return Err(SupervisorError::RevisionConflict {
            goal_id: goal_id.to_owned(),
            expected: expected_revision,
            actual: goal.revision,
        });
    }
    tx.commit().await?;
    wakes::poke();
    Ok(SupervisorGoal {
        run_state: to,
        state_detail: detail.map(str::to_owned),
        updated_at: now,
        ..goal
    })
}

/// Restart reconciliation, run once before any dispatcher starts: a `claimed` row cannot tell
/// whether its bytes left before the restart, so every one becomes `uncertain` with
/// [`CLAIM_UNCONFIRMED_AT_RESTART`] (still in flight until the owner resolves it or a real
/// receipt arrives), and its goal moves from `dispatching` to `delivery_uncertain`. An
/// `accepted` row (the target took the bytes, no start was recorded) becomes `uncertain` with
/// [`START_UNCONFIRMED_AT_RESTART`] the same way; neither is ever resent. A goal still
/// `dispatching` after that (the crash fell between a receipt and the goal's move) is moved
/// on from what its dispatches prove: `running` with a started one, `delivery_uncertain` with
/// an unresolved uncertain one, and `ready` when nothing is in flight, since then nothing can
/// have been transmitted. Returns the dispatches it changed.
pub async fn reconcile_claims_after_restart(
    pool: &SqlitePool,
) -> Result<Vec<Dispatch>, SupervisorError> {
    let mut tx = pool.begin_write().await?;
    let sql = format!(
        "SELECT {DISPATCH_COLUMNS} FROM supervisor_dispatches
         WHERE state IN ('claimed','accepted') ORDER BY id"
    );
    let rows: Vec<DispatchRow> = sqlx::query_as(&sql).fetch_all(&mut *tx).await?;
    let now = now_rfc3339();
    let mut changed = Vec::with_capacity(rows.len());
    for row in rows {
        let mut dispatch = dispatch_from_row(row)?;
        let code = if dispatch.state == DispatchState::Claimed {
            CLAIM_UNCONFIRMED_AT_RESTART
        } else {
            START_UNCONFIRMED_AT_RESTART
        };
        sqlx::query(
            "UPDATE supervisor_dispatches SET state = 'uncertain', uncertain_code = ?,
                    uncertain_at = ?, updated_at = ?
             WHERE id = ? AND state IN ('claimed','accepted')",
        )
        .bind(code)
        .bind(&now)
        .bind(&now)
        .bind(dispatch.id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE supervisor_goals SET run_state = 'delivery_uncertain', state_detail = ?,
                    updated_at = ?
             WHERE goal_id = ? AND run_state = 'dispatching'",
        )
        .bind(code)
        .bind(&now)
        .bind(&dispatch.goal_id)
        .execute(&mut *tx)
        .await?;
        dispatch.state = DispatchState::Uncertain;
        dispatch.uncertain_code = Some(code.to_owned());
        dispatch.uncertain_at = Some(now.clone());
        changed.push(dispatch);
    }
    let stranded: Vec<String> = sqlx::query_scalar(
        "SELECT goal_id FROM supervisor_goals WHERE run_state = 'dispatching' ORDER BY goal_id",
    )
    .fetch_all(&mut *tx)
    .await?;
    for goal_id in stranded {
        let flight: Option<(String, Option<String>)> = sqlx::query_as(&format!(
            "SELECT d.state, d.uncertain_code FROM supervisor_dispatches d
             JOIN supervisor_execution_bindings b
               ON b.goal_id = d.goal_id AND b.generation = d.generation
             WHERE d.goal_id = ? AND {IN_FLIGHT}
             ORDER BY d.id LIMIT 1"
        ))
        .bind(&goal_id)
        .fetch_optional(&mut *tx)
        .await?;
        let (to, detail) = match flight {
            Some((state, _)) if state == "started" => ("running", None),
            Some((_, code)) => (
                "delivery_uncertain",
                Some(code.unwrap_or_else(|| CLAIM_UNCONFIRMED_AT_RESTART.to_owned())),
            ),
            None => ("ready", None),
        };
        sqlx::query(
            "UPDATE supervisor_goals SET run_state = ?, state_detail = ?, updated_at = ?
             WHERE goal_id = ? AND run_state = 'dispatching'",
        )
        .bind(to)
        .bind(detail)
        .bind(&now)
        .bind(&goal_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(changed)
}

/// Records one delivery receipt for a claimed intent. Accepted, started, settled and
/// uncertain are separate facts: acceptance never implies a start, and a receipt from a stale
/// generation is refused. A start or settle must name the binding's current provider session
/// and, once a run is recorded, that run. An `uncertain` dispatch can still be resolved to
/// started or settled by such a receipt, so restart reconciliation records a run that really
/// happened; its uncertain code and time stay as history. Repeating the receipt already
/// recorded is a no-op. Goal run-state moves are the caller's, through [`transition_goal`].
///
/// Only the newest possibly-transmitted dispatch of the goal's generation owns the binding's
/// run: a start marks it live, a settle ends it (`run_id` back to NULL). A dispatch counts as
/// possibly transmitted once claimed, unless it was `released` before transmission. A start or
/// settle for an older dispatch while a newer one was possibly transmitted is
/// `RECEIPT_OUT_OF_ORDER`: the older dispatch keeps its state, and only its evidence (run,
/// session, first observed start/settle time) is committed, never the binding, so a late
/// receipt can never take the live-run marker from the newer run or put a second dispatch in
/// flight. With no such newer dispatch, a late start for an owner-resolved uncertain dispatch
/// makes it the live run. A start or settle naming a run other than the binding's live one is
/// `RUN_MISMATCH`.
pub async fn record_receipt(
    pool: &SqlitePool,
    request_id: &str,
    generation: i64,
    receipt: &Receipt,
) -> Result<Dispatch, SupervisorError> {
    validate_receipt(receipt)?;
    let mut tx = pool.begin_write().await?;
    match apply_receipt(&mut tx, request_id, generation, receipt).await? {
        AppliedReceipt::Recorded(dispatch) => {
            tx.commit().await?;
            wakes::poke();
            Ok(*dispatch)
        }
        // The older dispatch's evidence is kept; the receipt itself is refused.
        AppliedReceipt::EvidenceOnly(error) => {
            tx.commit().await?;
            Err(error)
        }
    }
}

fn validate_receipt(receipt: &Receipt) -> Result<(), SupervisorError> {
    if let Some((run_id, session)) = receipt.run() {
        require_id("run id", run_id, MAX_ID_CHARS)?;
        require_id("provider session id", session, MAX_ID_CHARS)?;
    }
    if let Receipt::Uncertain { code } = receipt {
        require_id("uncertain code", code, 128)?;
    }
    Ok(())
}

/// What [`apply_receipt`] wrote inside the caller's transaction.
enum AppliedReceipt {
    /// The receipt is recorded, or was already.
    Recorded(Box<Dispatch>),
    /// A late start or settle of an older dispatch: only its evidence was written, and the
    /// receipt itself is refused with this error.
    EvidenceOnly(SupervisorError),
}

/// [`record_receipt`]'s checks and writes, inside the caller's transaction. An `Err` wrote
/// nothing.
async fn apply_receipt(
    conn: &mut SqliteConnection,
    request_id: &str,
    generation: i64,
    receipt: &Receipt,
) -> Result<AppliedReceipt, SupervisorError> {
    let dispatch = require_dispatch(conn, request_id).await?;
    let binding = require_binding(conn, &dispatch.goal_id).await?;
    check_generation(dispatch.generation, generation)?;
    check_generation(binding.generation, generation)?;
    let to = receipt.state();
    if dispatch.state == to {
        let same = match receipt {
            Receipt::Started {
                run_id,
                provider_session_id,
            }
            | Receipt::Settled {
                run_id,
                provider_session_id,
            } => {
                dispatch.run_id.as_ref() == Some(run_id)
                    && dispatch.provider_session_id.as_ref() == Some(provider_session_id)
            }
            Receipt::Uncertain { code } => dispatch.uncertain_code.as_ref() == Some(code),
            Receipt::Accepted => true,
        };
        if same {
            return Ok(AppliedReceipt::Recorded(Box::new(dispatch)));
        }
    }
    if !receipt.allowed_from().contains(&dispatch.state) {
        return Err(SupervisorError::ReceiptOutOfOrder {
            request_id: request_id.to_owned(),
            from: dispatch.state,
            to,
        });
    }
    if let Some((run_id, session)) = receipt.run() {
        if binding.provider_session_id.as_deref() != Some(session) {
            return Err(SupervisorError::SessionMismatch {
                request_id: request_id.to_owned(),
                expected: binding.provider_session_id,
                actual: session.to_owned(),
            });
        }
        if let Some(recorded) = dispatch.run_id.as_deref() {
            if recorded != run_id {
                return Err(SupervisorError::RunMismatch {
                    request_id: request_id.to_owned(),
                    expected: dispatch.run_id.clone(),
                    actual: run_id.to_owned(),
                });
            }
        }
        let newer_transmitted: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM supervisor_dispatches
              WHERE goal_id = ? AND generation = ? AND id > ? AND claimed_at IS NOT NULL
                AND state <> 'released')",
        )
        .bind(&dispatch.goal_id)
        .bind(generation)
        .bind(dispatch.id)
        .fetch_one(&mut *conn)
        .await?;
        if newer_transmitted {
            let observed_at = if to == DispatchState::Started {
                "started_at"
            } else {
                "settled_at"
            };
            let sql = format!(
                "UPDATE supervisor_dispatches SET run_id = ?, provider_session_id = ?,
                        {observed_at} = COALESCE({observed_at}, ?), updated_at = ?
                 WHERE id = ?"
            );
            let now = now_rfc3339();
            sqlx::query(&sql)
                .bind(run_id)
                .bind(session)
                .bind(&now)
                .bind(&now)
                .bind(dispatch.id)
                .execute(&mut *conn)
                .await?;
            return Ok(AppliedReceipt::EvidenceOnly(SupervisorError::ReceiptOutOfOrder {
                request_id: request_id.to_owned(),
                from: dispatch.state,
                to,
            }));
        }
        if let Some(bound) = binding.run_id.as_deref() {
            if bound != run_id {
                return Err(SupervisorError::RunMismatch {
                    request_id: request_id.to_owned(),
                    expected: binding.run_id.clone(),
                    actual: run_id.to_owned(),
                });
            }
        }
    }
    let now = now_rfc3339();
    let mut updated = dispatch;
    updated.state = to;
    match receipt {
        Receipt::Started {
            run_id,
            provider_session_id,
        }
        | Receipt::Settled {
            run_id,
            provider_session_id,
        } => {
            let observed_at = if to == DispatchState::Started {
                "started_at"
            } else {
                "settled_at"
            };
            let sql = format!(
                "UPDATE supervisor_dispatches SET state = ?, run_id = ?, provider_session_id = ?,
                        {observed_at} = ?, updated_at = ?
                 WHERE id = ?"
            );
            sqlx::query(&sql)
                .bind(to.as_str())
                .bind(run_id)
                .bind(provider_session_id)
                .bind(&now)
                .bind(&now)
                .bind(updated.id)
                .execute(&mut *conn)
                .await?;
            // This dispatch is the newest claimed one and the binding holds no other run.
            let bound_run = (to == DispatchState::Started).then_some(run_id);
            sqlx::query(
                "UPDATE supervisor_execution_bindings SET run_id = ?, updated_at = ?
                 WHERE goal_id = ? AND generation = ? AND (run_id IS NULL OR run_id = ?)",
            )
            .bind(bound_run)
            .bind(&now)
            .bind(&updated.goal_id)
            .bind(generation)
            .bind(run_id)
            .execute(&mut *conn)
            .await?;
            updated.run_id = Some(run_id.clone());
            updated.provider_session_id = Some(provider_session_id.clone());
            if to == DispatchState::Started {
                updated.started_at = Some(now);
            } else {
                updated.settled_at = Some(now);
            }
        }
        Receipt::Accepted => {
            sqlx::query(
                "UPDATE supervisor_dispatches SET state = 'accepted', accepted_at = ?,
                        updated_at = ?
                 WHERE id = ?",
            )
            .bind(&now)
            .bind(&now)
            .bind(updated.id)
            .execute(&mut *conn)
            .await?;
            updated.accepted_at = Some(now);
        }
        Receipt::Uncertain { code } => {
            sqlx::query(
                "UPDATE supervisor_dispatches SET state = 'uncertain', uncertain_code = ?,
                        uncertain_at = ?, updated_at = ?
                 WHERE id = ?",
            )
            .bind(code)
            .bind(&now)
            .bind(&now)
            .bind(updated.id)
            .execute(&mut *conn)
            .await?;
            updated.uncertain_code = Some(code.clone());
            updated.uncertain_at = Some(now);
        }
    }
    Ok(AppliedReceipt::Recorded(Box::new(updated)))
}

/// Records a causal start in one transaction: accepted (unless already recorded), started
/// with the run and provider session the target named, and the goal's move from
/// `dispatching` to `running` at its current revision. A goal that entered an owner hold
/// after the claim keeps the hold, since the worker runs anyway and the owner decides; the
/// start is still recorded. Any refusal (a stale generation, a session or run mismatch, a goal
/// in another state) or store error writes nothing, so a started dispatch never exists
/// without its goal running or owner-held, and no settle can land between the two. The one
/// exception is a late start of a dispatch a newer one superseded (`RECEIPT_OUT_OF_ORDER`):
/// as in [`record_receipt`], its evidence (and its accepted stage) is committed and the start
/// itself is refused, so no goal moves.
pub async fn record_start(
    pool: &SqlitePool,
    request_id: &str,
    generation: i64,
    run_id: &str,
    provider_session_id: &str,
) -> Result<Dispatch, SupervisorError> {
    let started = Receipt::Started {
        run_id: run_id.to_owned(),
        provider_session_id: provider_session_id.to_owned(),
    };
    validate_receipt(&started)?;
    let mut tx = pool.begin_write().await?;
    let claimed = require_dispatch(&mut tx, request_id).await?;
    if claimed.state == DispatchState::Claimed {
        if let AppliedReceipt::EvidenceOnly(error) =
            apply_receipt(&mut tx, request_id, generation, &Receipt::Accepted).await?
        {
            return Err(error);
        }
    }
    let dispatch = match apply_receipt(&mut tx, request_id, generation, &started).await? {
        AppliedReceipt::Recorded(dispatch) => *dispatch,
        AppliedReceipt::EvidenceOnly(error) => {
            tx.commit().await?;
            return Err(error);
        }
    };
    let goal = require_goal(&mut tx, &dispatch.goal_id).await?;
    if !goal.run_state.is_owner_hold() {
        transition_goal_in(
            &mut tx,
            &dispatch.goal_id,
            goal.revision,
            RunState::Dispatching,
            RunState::Running,
            None,
        )
        .await?;
    }
    tx.commit().await?;
    wakes::poke();
    Ok(dispatch)
}

/// Records the provider session of the binding's current generation (first launch). A
/// different session for the same generation is refused: rebinding needs
/// [`advance_generation`].
pub async fn attach_session(
    pool: &SqlitePool,
    goal_id: &str,
    generation: i64,
    provider_session_id: &str,
) -> Result<ExecutionBinding, SupervisorError> {
    require_id("provider session id", provider_session_id, MAX_ID_CHARS)?;
    let mut tx = pool.begin_write().await?;
    let binding = require_binding(&mut tx, goal_id).await?;
    check_generation(binding.generation, generation)?;
    match binding.provider_session_id.as_deref() {
        Some(existing) if existing == provider_session_id => return Ok(binding),
        Some(existing) => {
            return Err(SupervisorError::Invalid(format!(
                "generation {generation} of goal {goal_id} is bound to session {existing}"
            )))
        }
        None => {}
    }
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE supervisor_execution_bindings SET provider_session_id = ?, updated_at = ?
         WHERE goal_id = ?",
    )
    .bind(provider_session_id)
    .bind(&now)
    .bind(goal_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ExecutionBinding {
        provider_session_id: Some(provider_session_id.to_owned()),
        updated_at: now,
        ..binding
    })
}

/// Fences the binding's current generation and starts the next one, optionally on a new
/// provider session. Receipts of the old generation are refused from now on; unsent intents
/// move to the new generation because they were never transmitted.
pub async fn advance_generation(
    pool: &SqlitePool,
    goal_id: &str,
    expected_generation: i64,
    provider_session_id: Option<&str>,
) -> Result<ExecutionBinding, SupervisorError> {
    if let Some(session) = provider_session_id {
        require_id("provider session id", session, MAX_ID_CHARS)?;
    }
    let mut tx = pool.begin_write().await?;
    let binding = require_binding(&mut tx, goal_id).await?;
    check_generation(binding.generation, expected_generation)?;
    let generation = binding.generation + 1;
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE supervisor_execution_bindings SET generation = ?, provider_session_id = ?,
                run_id = NULL, updated_at = ?
         WHERE goal_id = ?",
    )
    .bind(generation)
    .bind(provider_session_id)
    .bind(&now)
    .bind(goal_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE supervisor_dispatches SET generation = ?, updated_at = ?
         WHERE goal_id = ? AND state = 'ready'",
    )
    .bind(generation)
    .bind(&now)
    .bind(goal_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ExecutionBinding {
        provider_session_id: provider_session_id.map(str::to_owned),
        run_id: None,
        generation,
        updated_at: now,
        ..binding
    })
}

/// True when `child_id` already reaches `goal_id` through pending child waits.
const WAIT_CYCLE_SQL: &str = "WITH RECURSIVE reach(goal_id) AS (
        SELECT child_goal_id FROM supervisor_waits
         WHERE goal_id = ? AND state = 'pending' AND child_goal_id IS NOT NULL
        UNION
        SELECT w.child_goal_id FROM supervisor_waits w JOIN reach r ON w.goal_id = r.goal_id
         WHERE w.state = 'pending' AND w.child_goal_id IS NOT NULL
    )
    SELECT EXISTS(SELECT 1 FROM reach WHERE goal_id = ?)";

/// Makes `goal_id` wait on a child goal or a named external event. Self-waits, links to
/// another owner's goal and dependency cycles are refused before commit.
pub async fn add_wait(
    pool: &SqlitePool,
    goal_id: &str,
    target: &WaitTarget,
) -> Result<Wait, SupervisorError> {
    if let WaitTarget::ExternalEvent(key) = target {
        require_id("event key", key, MAX_ID_CHARS)?;
    }
    if matches!(target, WaitTarget::ChildGoal(child_id) if child_id == goal_id) {
        return Err(SupervisorError::SelfWait(goal_id.to_owned()));
    }
    let mut tx = pool.begin_write().await?;
    let goal = require_goal(&mut tx, goal_id).await?;
    check_open(&goal)?;
    let (kind, child_goal_id, event_key) = match target {
        WaitTarget::ChildGoal(child_id) => {
            let child = require_goal(&mut tx, child_id).await?;
            if child.supervisor_id != goal.supervisor_id
                || child.owner_principal != goal.owner_principal
            {
                return Err(SupervisorError::CrossOwner(format!("goal {child_id}")));
            }
            let cycle: bool = sqlx::query_scalar(WAIT_CYCLE_SQL)
                .bind(child_id)
                .bind(goal_id)
                .fetch_one(&mut *tx)
                .await?;
            if cycle {
                return Err(SupervisorError::WaitCycle {
                    goal: goal_id.to_owned(),
                    child: child_id.clone(),
                });
            }
            (WaitKind::ChildGoal, Some(child_id.clone()), None)
        }
        WaitTarget::ExternalEvent(key) => (WaitKind::ExternalEvent, None, Some(key.clone())),
    };
    let wait_id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO supervisor_waits(wait_id, supervisor_id, goal_id, kind, child_goal_id,
                event_key, state, created_at)
         VALUES(?,?,?,?,?,?,'pending',?)",
    )
    .bind(&wait_id)
    .bind(&goal.supervisor_id)
    .bind(goal_id)
    .bind(kind.as_str())
    .bind(child_goal_id.as_deref())
    .bind(event_key.as_deref())
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Wait {
        wait_id,
        supervisor_id: goal.supervisor_id,
        goal_id: goal_id.to_owned(),
        kind,
        child_goal_id,
        event_key,
        state: WaitState::Pending,
        created_at: now,
        resolved_at: None,
    })
}

/// Every wait of a goal, oldest first.
pub async fn waits_of(pool: &SqlitePool, goal_id: &str) -> Result<Vec<Wait>, SupervisorError> {
    let sql =
        format!("SELECT {WAIT_COLUMNS} FROM supervisor_waits WHERE goal_id = ? ORDER BY rowid");
    let rows: Vec<WaitRow> = sqlx::query_as(&sql).bind(goal_id).fetch_all(pool).await?;
    rows.into_iter().map(wait_from_row).collect()
}

/// Records one event for a wait, once per (wait, event). A satisfying or failing event
/// resolves a pending wait and, in the same transaction, persists the wake: a failure always
/// wakes the goal (it is not success), a satisfaction only once no other wait is pending.
pub async fn record_wait_event(
    pool: &SqlitePool,
    wait_id: &str,
    event_id: &str,
    outcome: WaitOutcome,
    evidence: &[String],
) -> Result<WaitEventReceipt, SupervisorError> {
    require_id("event id", event_id, MAX_ID_CHARS)?;
    let evidence_json = serde_json::to_string(evidence)
        .map_err(|error| SupervisorError::Invalid(error.to_string()))?;
    let mut tx = pool.begin_write().await?;
    let sql = format!("SELECT {WAIT_COLUMNS} FROM supervisor_waits WHERE wait_id = ?");
    let row: Option<WaitRow> = sqlx::query_as(&sql)
        .bind(wait_id)
        .fetch_optional(&mut *tx)
        .await?;
    let mut wait =
        row.map(wait_from_row)
            .transpose()?
            .ok_or_else(|| SupervisorError::NotFound {
                kind: "wait",
                id: wait_id.to_owned(),
            })?;
    let now = now_rfc3339();
    let inserted = sqlx::query(
        "INSERT INTO supervisor_wait_events(wait_id, event_id, outcome, evidence_json, received_at)
         VALUES(?,?,?,?,?) ON CONFLICT(wait_id, event_id) DO NOTHING",
    )
    .bind(wait_id)
    .bind(event_id)
    .bind(outcome.as_str())
    .bind(&evidence_json)
    .bind(&now)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    let resolution = match outcome {
        WaitOutcome::Progress => None,
        WaitOutcome::Satisfied => Some((WaitState::Satisfied, WakeCause::DependencyReady)),
        WaitOutcome::Failed => Some((WaitState::Failed, WakeCause::DependencyFailed)),
    };
    let Some((state, cause)) = resolution.filter(|_| inserted && wait.state == WaitState::Pending)
    else {
        tx.commit().await?;
        return Ok(WaitEventReceipt {
            inserted,
            wait,
            wake: None,
        });
    };
    sqlx::query("UPDATE supervisor_waits SET state = ?, resolved_at = ? WHERE wait_id = ?")
        .bind(state.as_str())
        .bind(&now)
        .bind(wait_id)
        .execute(&mut *tx)
        .await?;
    wait.state = state;
    wait.resolved_at = Some(now.clone());
    let others_pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM supervisor_waits WHERE goal_id = ? AND state = 'pending')",
    )
    .bind(&wait.goal_id)
    .fetch_one(&mut *tx)
    .await?;
    let wake = if cause == WakeCause::DependencyFailed || !others_pending {
        let goal = require_goal(&mut tx, &wait.goal_id).await?;
        let generation = require_binding(&mut tx, &wait.goal_id).await?.generation;
        let wake_event = format!("{wait_id}/{event_id}");
        Some(insert_wake(&mut tx, &goal, generation, cause, &wake_event, &now).await?)
    } else {
        None
    };
    tx.commit().await?;
    if wake.is_some() {
        wakes::poke();
    }
    Ok(WaitEventReceipt {
        inserted,
        wait,
        wake,
    })
}

/// Persists a wake for `goal_id` at its current revision and generation, due at `due_at`.
/// Replays of the same (goal, cause, event) return the row already written.
pub async fn enqueue_wake(
    pool: &SqlitePool,
    goal_id: &str,
    cause: WakeCause,
    event_id: &str,
    due_at: DateTime<Utc>,
) -> Result<PendingWake, SupervisorError> {
    require_id("event id", event_id, MAX_ID_CHARS)?;
    let mut tx = pool.begin_write().await?;
    let goal = require_goal(&mut tx, goal_id).await?;
    let generation = require_binding(&mut tx, goal_id).await?.generation;
    let wake = insert_wake(
        &mut tx,
        &goal,
        generation,
        cause,
        event_id,
        &due_at.to_rfc3339(),
    )
    .await?;
    tx.commit().await?;
    Ok(wake)
}

/// Unconsumed wakes due by `due_by`, oldest first.
pub async fn pending_wakes(
    pool: &SqlitePool,
    due_by: DateTime<Utc>,
    limit: u32,
) -> Result<Vec<PendingWake>, SupervisorError> {
    let sql = format!(
        "SELECT {WAKE_COLUMNS} FROM supervisor_pending_wakes
         WHERE consumed_at IS NULL AND due_at <= ? ORDER BY due_at, id LIMIT ?"
    );
    let rows: Vec<WakeRow> = sqlx::query_as(&sql)
        .bind(due_by.to_rfc3339())
        .bind(i64::from(limit))
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(wake_from_row).collect()
}

/// Marks a wake consumed. Returns false when it was already consumed (another consumer won).
pub async fn consume_wake(pool: &SqlitePool, wake_id: i64) -> Result<bool, SupervisorError> {
    let updated = sqlx::query(
        "UPDATE supervisor_pending_wakes SET consumed_at = ? WHERE id = ? AND consumed_at IS NULL",
    )
    .bind(now_rfc3339())
    .bind(wake_id)
    .execute(pool)
    .await?;
    Ok(updated.rows_affected() == 1)
}

/// Refusal code of a legacy lane API (`lane.drive`, `lane.transition`, `lane.pause`,
/// `lane.resume`, goal set, mode, approve, discard, revision accept and reject, a tracker
/// move or lane action, a lane close's worktree removal, and the legacy runner's pane attach)
/// aimed at a lane that belongs to a supervisor goal. An owner reply in such a lane's thread
/// is not refused but left to normal routing.
pub const SUPERVISOR_OWNED: &str = "SUPERVISOR_OWNED";

/// True when the supervisor owns the lane: a supervisor goal's own lane, whose worker is
/// started only by the hub's supervisor dispatcher and whose requirements, mode and budget
/// change only through the supervisor (which bumps the goal revision), or a legacy lane an
/// admitted `adopt:<lane>` request took over, whose work continues in the adopting goal.
/// Neither is driven or changed through the legacy lane APIs.
pub async fn supervisor_owns_lane(pool: &SqlitePool, lane_key: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM supervisor_goals WHERE lane_key = ?)
             OR EXISTS(SELECT 1 FROM supervisor_requests WHERE source_message_id = ?)",
    )
    .bind(lane_key)
    .bind(format!("{ADOPT_MESSAGE_PREFIX}{lane_key}"))
    .fetch_one(pool)
    .await
}

/// The legacy lane-goal fence of the desktop and daemon path (`supervisor_goal`): a legacy
/// action that changes a lane the supervisor owns ([`supervisor_owns_lane`]) is refused with
/// `SUPERVISOR_OWNED`. `mutates` is false only for a read (show), which is always allowed.
pub async fn refuse_legacy_goal_action(
    pool: &SqlitePool,
    lane_key: &str,
    mutates: bool,
) -> Result<(), SupervisorError> {
    if mutates && supervisor_owns_lane(pool, lane_key).await? {
        return Err(SupervisorError::SupervisorOwned {
            lane_key: lane_key.to_owned(),
        });
    }
    Ok(())
}

/// Attaches a pane the legacy lane runner launched, in one statement that refuses a lane the
/// supervisor owns ([`supervisor_owns_lane`]): false when it was refused and nothing changed.
/// An adoption checks the lane's pane under its write lock, and this checks the adoption row
/// in the same write, so exactly one of a racing adoption and legacy launch commits.
pub async fn attach_legacy_pane(
    pool: &SqlitePool,
    lane_key: &str,
    session_id: &str,
) -> Result<bool, sqlx::Error> {
    let attached = sqlx::query(
        "UPDATE lanes SET omo_thread_id = ?, updated_at = ? WHERE key = ?
           AND NOT EXISTS(SELECT 1 FROM supervisor_goals WHERE lane_key = ?)
           AND NOT EXISTS(SELECT 1 FROM supervisor_requests WHERE source_message_id = ?)",
    )
    .bind(session_id)
    .bind(now_rfc3339())
    .bind(lane_key)
    .bind(lane_key)
    .bind(format!("{ADOPT_MESSAGE_PREFIX}{lane_key}"))
    .execute(pool)
    .await?;
    Ok(attached.rows_affected() == 1)
}

#[cfg(test)]
mod tests;

pub mod admission;
pub mod answers;
pub mod completion;
pub mod delivery;
pub mod dispatcher;
pub mod receipts;
pub mod wakes;
