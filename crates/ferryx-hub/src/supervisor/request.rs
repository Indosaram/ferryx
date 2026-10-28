//! The one supervisor request service every adapter calls: MCP `supervisor.request`, the
//! daemon's `SupervisorRequestV1` (desktop IPC and `ferryx-cli supervisor request`) and the
//! remote `/api/v1/supervisor/*` routes.
//!
//! The envelope is `{requestId, projectId?, goalId?, expectedRevision?, action, payload}`.
//! Typed payloads deny unknown fields. Actor, host grant and channel binding come from the
//! adapter ([`RequestContext`]), never from the body. Every refusal is a [`RequestFailure`]
//! with a stable code and an HTTP status: 400 invalid, 403 unauthorized, 404 unknown scope,
//! 409 revision/ownership conflict, 503 capability/engine unavailable. A malformed envelope
//! or payload is refused before anything is read or written.
//!
//! Goal actions (`correct`, `pause`, `resume`, `cancel`, `approve`, `depend`) need `goalId`
//! and `expectedRevision`, and the caller has to own the goal. Pause and cancel are
//! separate owner holds: a pause gates unsent work and keeps the worker; a cancel ends the
//! goal, cancels its own waits and fails the waits of parents on it (the wake scheduler
//! turns that into `DEPENDENCY_FAILED`). It also stops the worker's live run when the adapter
//! can reach agent control. A pause records the state it interrupted; a resume restores it or
//! the state its facts moved to (see [`resume`]). `approve` releases an `awaiting_owner` hold
//! under the grant the goal already has. It never widens that grant: more operations need a
//! new submission with explicit owner choices. It switches a supervised goal to `full` for
//! its remaining turns (reported as `modeChanged`). Pause, resume, approve and correct each
//! commit in one write transaction under the revision compare-and-set (a 409 writes nothing);
//! `depend` checks the revision inside the wait's transaction.
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::admission::{
    self, ActorContext, AdmissionError, AdoptSession, AdoptWork, OwnerChoices, SubmitWork,
};
use super::dispatcher::{GRANT_REVOKED, TURN_LIMIT};
use super::memory::{ForgetPayload, TeachPayload};
use super::wakes::{self, DependencyError};
use super::{
    execution_binding_of, goal_of, identity_of, waits_of, ChannelProvider, RunState,
    SupervisorError, SupervisorGoal, WaitState, WaitTarget,
};
use crate::agent_control::{AgentControl, AgentOp, LOCAL_HOST_ID};
use crate::goals::contract::{self, RequirementSpec};
use crate::goals::types::GoalMode;
use crate::meta::mhc_supervisor::OwnerMemory;
use crate::storage::WriteTx;

pub use super::memory::MEMORY_UNAVAILABLE;
/// `state_detail` prefix of an owner pause.
pub const OWNER_PAUSED: &str = "OWNER_PAUSED";
/// `state_detail` prefix of an owner cancel.
pub const OWNER_CANCELLED: &str = "OWNER_CANCELLED";
/// `state_detail` prefix of an owner approval releasing a hold.
pub const OWNER_APPROVED: &str = "OWNER_APPROVED";
/// `state_detail` prefix of a goal a resume held because nothing could follow without the
/// owner (no unsent instruction, no settled run to verify).
pub const OWNER_DECISION_NEEDED: &str = "OWNER_DECISION_NEEDED";
/// An approve that would release a goal with nothing to do next (409; nothing changes).
pub const NOTHING_TO_RELEASE: &str = "NOTHING_TO_RELEASE";

/// Longest correction or teach text.
const MAX_TEXT_CHARS: usize = 64_000;

/// A typed refusal with its stable wire code and HTTP status.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct RequestFailure {
    pub code: String,
    pub status: u16,
    pub message: String,
}

impl RequestFailure {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            status: status_for_code(code),
            message: message.into(),
        }
    }

    fn invalid(message: impl std::fmt::Display) -> Self {
        Self::new("INVALID_REQUEST", message.to_string())
    }

    /// The machine envelope fields every adapter reports.
    pub fn to_json(&self) -> Value {
        json!({"code": self.code, "status": self.status, "message": self.message})
    }
}

/// HTTP status of a supervisor wire code. Every code is listed: a code nobody mapped is a
/// 500, never a silent conflict.
pub fn status_for_code(code: &str) -> u16 {
    match code {
        "INVALID_REQUEST" | "WAIT_CYCLE" | "CROSS_OWNER" => 400,
        "UNAUTHORIZED" | "FORBIDDEN" => 403,
        "NOT_FOUND" => 404,
        "REVISION_CONFLICT"
        | "NO_LIVE_RUN"
        | "DISPATCH_IN_FLIGHT"
        | "SUPERSEDED"
        | "REQUEST_CONFLICT"
        | "ILLEGAL_TRANSITION"
        | "GOAL_CLOSED"
        | "STALE_GENERATION"
        | "DISPATCH_PENDING"
        | "NOT_CLAIMABLE"
        | "RECEIPT_OUT_OF_ORDER"
        | "RUN_MISMATCH"
        | "SESSION_MISMATCH"
        | "TURN_IN_FLIGHT"
        | NOTHING_TO_RELEASE
        | super::memory::MEMORY_CONTENT_RETAINED
        | super::memory::MEMORY_REFUSED
        | super::SUPERVISOR_OWNED
        | super::SETUP_CONFLICT
        | admission::SCOPE_NEEDS_OWNER
        | admission::LEGACY_LANE_LIVE
        | admission::LEGACY_TERMS_CHANGED
        | admission::LEGACY_GOAL_OFF
        | TURN_LIMIT => 409,
        "STORAGE" | "INTERNAL" => 500,
        "SUPERVISOR_UNAVAILABLE" | "JUDGE_UNAVAILABLE" | super::cutover::SUPERVISOR_INTAKE_CLOSED => 503,
        code if code.starts_with("MHC_") || code.starts_with("MEMORY_") => 503,
        _ => 500,
    }
}

impl From<SupervisorError> for RequestFailure {
    fn from(error: SupervisorError) -> Self {
        RequestFailure::new(error.code(), error.to_string())
    }
}

impl From<AdmissionError> for RequestFailure {
    fn from(error: AdmissionError) -> Self {
        RequestFailure::new(error.code(), error.to_string())
    }
}

impl From<DependencyError> for RequestFailure {
    fn from(error: DependencyError) -> Self {
        let mut failure = RequestFailure::new(error.code(), error.to_string());
        failure.status = error.http_status();
        failure
    }
}

impl From<sqlx::Error> for RequestFailure {
    fn from(error: sqlx::Error) -> Self {
        SupervisorError::Sql(error).into()
    }
}

impl From<crate::goals::types::GoalError> for RequestFailure {
    fn from(error: crate::goals::types::GoalError) -> Self {
        use crate::goals::types::GoalError;
        let code = match &error {
            GoalError::TurnInFlight(_) | GoalError::RevisionChanged { .. } => "REVISION_CONFLICT",
            GoalError::NoContract(_) => "NOT_FOUND",
            GoalError::TurnLimit { .. } => TURN_LIMIT,
            GoalError::JudgeUnavailable => "JUDGE_UNAVAILABLE",
            GoalError::SupervisorFault(fault) => fault.code(),
            GoalError::Sql(_) | GoalError::Hub(_) => "STORAGE",
            _ => "INVALID_REQUEST",
        };
        RequestFailure::new(code, error.to_string())
    }
}

/// Who asks and through which channel, as the authenticated adapter established it.
#[derive(Clone)]
pub struct RequestContext {
    /// The verified owner principal of the caller.
    pub principal: String,
    pub adapter: ChannelProvider,
    pub channel_binding_id: Option<String>,
    /// Agent control for stopping a cancelled goal's live run. `None` in processes that never
    /// transmit (the MCP server).
    pub control: Option<Arc<dyn AgentControl>>,
    /// The retained mhc supervisor's native owner memory, when this process runs one.
    pub memory: Option<Arc<dyn OwnerMemory>>,
}

/// The request envelope. Unknown fields (an `actor`, say) are refused.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Envelope {
    pub request_id: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub goal_id: Option<String>,
    #[serde(default)]
    pub expected_revision: Option<i64>,
    pub action: String,
    pub payload: Value,
}

/// The supported actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Message,
    Submit,
    Adopt,
    Correct,
    Pause,
    Resume,
    Cancel,
    Approve,
    Teach,
    Forget,
    Depend,
}

impl Action {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "message" => Self::Message,
            "submit" => Self::Submit,
            "adopt" => Self::Adopt,
            "correct" => Self::Correct,
            "pause" => Self::Pause,
            "resume" => Self::Resume,
            "cancel" => Self::Cancel,
            "approve" => Self::Approve,
            "teach" => Self::Teach,
            "forget" => Self::Forget,
            "depend" => Self::Depend,
            _ => return None,
        })
    }

    /// Goal control and memory: the owner's own decisions, never a worker's. Adapters that
    /// can be reached from inside a Ferryx pane route these through the daemon's peer check.
    pub fn needs_owner_authority(self) -> bool {
        self.targets_goal() || matches!(self, Self::Teach | Self::Forget)
    }

    /// Actions that name one existing goal.
    pub fn targets_goal(self) -> bool {
        matches!(
            self,
            Self::Correct
                | Self::Pause
                | Self::Resume
                | Self::Cancel
                | Self::Approve
                | Self::Depend
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitPayload {
    request: String,
    work: SubmitWork,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessagePayload {
    text: String,
    #[serde(default)]
    work: Option<SubmitWork>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReasonPayload {
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyPayload {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorrectPayload {
    /// The owner's correction, verbatim.
    text: String,
    /// Replaces the goal's requirements when given.
    #[serde(default)]
    requirements: Option<Vec<RequirementSpec>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DependPayload {
    #[serde(default)]
    child_goal_id: Option<String>,
    #[serde(default)]
    event_key: Option<String>,
}

fn payload<T: for<'de> Deserialize<'de>>(value: &Value) -> Result<T, RequestFailure> {
    serde_json::from_value(value.clone()).map_err(RequestFailure::invalid)
}

fn require_text(label: &str, text: &str) -> Result<(), RequestFailure> {
    if text.trim().is_empty() {
        return Err(RequestFailure::invalid(format!("{label} is required")));
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(RequestFailure::invalid(format!(
            "{label} exceeds {MAX_TEXT_CHARS} characters"
        )));
    }
    Ok(())
}

/// Parses an envelope from raw JSON. Nothing is read or written.
pub fn parse_envelope(value: Value) -> Result<(Envelope, Action), RequestFailure> {
    let envelope: Envelope = serde_json::from_value(value).map_err(RequestFailure::invalid)?;
    let id = envelope.request_id.as_str();
    if id.trim().is_empty()
        || id.trim() != id
        || id.chars().count() > 256
        || id.chars().any(char::is_control)
    {
        return Err(RequestFailure::invalid(
            "requestId must be 1-256 printable characters",
        ));
    }
    let action = Action::parse(&envelope.action)
        .ok_or_else(|| RequestFailure::invalid(format!("unknown action {}", envelope.action)))?;
    if action.targets_goal() {
        if envelope.goal_id.is_none() || envelope.expected_revision.is_none() {
            return Err(RequestFailure::invalid(format!(
                "{} needs goalId and expectedRevision",
                envelope.action
            )));
        }
    } else if envelope.goal_id.is_some() || envelope.expected_revision.is_some() {
        return Err(RequestFailure::invalid(format!(
            "{} does not take goalId or expectedRevision",
            envelope.action
        )));
    }
    Ok((envelope, action))
}

/// Serves one request envelope. Every payload is validated before any read or write.
pub async fn handle(
    pool: &SqlitePool,
    context: &RequestContext,
    value: Value,
) -> Result<Value, RequestFailure> {
    let (envelope, action) = parse_envelope(value)?;
    match action {
        Action::Message | Action::Submit | Action::Adopt => {
            let mut answer = admit(pool, context, &envelope, action).await?;
            learn(pool, context, &envelope, action, &mut answer);
            Ok(answer)
        }
        Action::Teach => {
            let teach: TeachPayload = payload(&envelope.payload)?;
            require_text("text", &teach.text)?;
            if let Some(receipt) = &teach.replaces {
                require_text("replaces", receipt)?;
            }
            super::memory::teach(
                pool,
                context,
                &envelope.request_id,
                envelope.project_id.as_deref(),
                &teach,
            )
            .await
        }
        Action::Forget => {
            let forget: ForgetPayload = payload(&envelope.payload)?;
            require_text("memoryId", &forget.memory_id)?;
            if envelope.project_id.is_some() {
                return Err(RequestFailure::invalid(
                    "forget names a receipt, not a project",
                ));
            }
            super::memory::forget(pool, context, &envelope.request_id, &forget).await
        }
        Action::Correct
        | Action::Pause
        | Action::Resume
        | Action::Cancel
        | Action::Approve
        | Action::Depend => {
            let mut answer = goal_action(pool, context, &envelope, action).await?;
            learn(pool, context, &envelope, action, &mut answer);
            Ok(answer)
        }
    }
}

/// The owner-authored text of an accepted request, when it is one the owner writes in words:
/// a message, a submitted request or a correction. An adoption, a control action or a
/// dependency carries none.
fn owner_words(envelope: &Envelope, action: Action) -> Option<String> {
    let field = match action {
        Action::Message | Action::Correct => "text",
        Action::Submit => "request",
        _ => return None,
    };
    envelope
        .payload
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Owner learning of an accepted request's words, reported on its answer as `learning`.
fn learn(
    pool: &SqlitePool,
    context: &RequestContext,
    envelope: &Envelope,
    action: Action,
    answer: &mut Value,
) {
    let Some(text) = owner_words(envelope, action) else {
        return;
    };
    // A replayed request is the same source: the engine and the provenance row dedupe it.
    let learning = super::memory::learn_in_background(
        pool,
        context,
        &envelope.request_id,
        envelope.project_id.as_deref(),
        &text,
    );
    if let Some(fields) = answer.as_object_mut() {
        fields.insert("learning".into(), learning);
    }
}

/// Fills a work proposal's project from the envelope; a contradiction is refused.
fn with_project(
    proposed: &mut Option<String>,
    envelope: Option<&str>,
) -> Result<(), RequestFailure> {
    match (proposed.as_deref(), envelope) {
        (Some(work), Some(envelope)) if work != envelope => Err(RequestFailure::invalid(format!(
            "projectId {envelope} contradicts the work's projectId {work}"
        ))),
        (None, Some(envelope)) => {
            *proposed = Some(envelope.to_owned());
            Ok(())
        }
        _ => Ok(()),
    }
}

async fn admit(
    pool: &SqlitePool,
    context: &RequestContext,
    envelope: &Envelope,
    action: Action,
) -> Result<Value, RequestFailure> {
    let actor = |source_body: String| ActorContext {
        principal: context.principal.clone(),
        source_adapter: context.adapter,
        source_message_id: envelope.request_id.clone(),
        source_body,
        channel_binding_id: context.channel_binding_id.clone(),
        choices: OwnerChoices::default(),
    };
    let project = envelope.project_id.as_deref();
    let outcome = match action {
        Action::Submit => {
            let mut submit: SubmitPayload = payload(&envelope.payload)?;
            with_project(&mut submit.work.project_id, project)?;
            admission::submit_work(pool, &actor(submit.request), &submit.work).await?
        }
        Action::Message => {
            let mut message: MessagePayload = payload(&envelope.payload)?;
            if let Some(work) = message.work.as_mut() {
                with_project(&mut work.project_id, project)?;
            }
            admission::handle_message(pool, &actor(message.text), message.work.as_ref()).await?
        }
        // One owner action adopts one old channel session (`sessionKey`) or one old lane
        // (`laneKey`) as one new goal contract.
        _ if envelope.payload.get("sessionKey").is_some() => {
            let mut adopt: AdoptSession = payload(&envelope.payload)?;
            with_project(&mut adopt.project_id, project)?;
            let body = format!("adopt session {}", adopt.session_key);
            admission::adopt_session(pool, &actor(body), &adopt).await?
        }
        _ => {
            let mut adopt: AdoptWork = payload(&envelope.payload)?;
            with_project(&mut adopt.project_id, project)?;
            let body = format!("adopt {}", adopt.lane_key);
            admission::adopt_work(pool, &actor(body), &adopt).await?
        }
    };
    serde_json::to_value(&outcome)
        .map_err(|error| RequestFailure::new("INTERNAL", error.to_string()))
}

async fn owner_identity(
    pool: &SqlitePool,
    context: &RequestContext,
) -> Result<super::SupervisorIdentity, RequestFailure> {
    identity_of(pool, &context.principal).await?.ok_or_else(|| {
        RequestFailure::new(
            "UNAUTHORIZED",
            format!("{} is not the supervisor's owner", context.principal),
        )
    })
}

/// The goal, when the caller owns it.
async fn owned_goal(
    pool: &SqlitePool,
    context: &RequestContext,
    goal_id: &str,
    project_id: Option<&str>,
) -> Result<SupervisorGoal, RequestFailure> {
    let identity = owner_identity(pool, context).await?;
    let goal = goal_of(pool, goal_id)
        .await?
        .ok_or_else(|| RequestFailure::new("NOT_FOUND", format!("goal {goal_id} not found")))?;
    if goal.supervisor_id != identity.supervisor_id
        || goal.owner_principal != identity.owner_principal
    {
        return Err(RequestFailure::new(
            "FORBIDDEN",
            format!("goal {goal_id} belongs to another owner"),
        ));
    }
    if project_id.is_some_and(|project| project != goal.project_id) {
        return Err(RequestFailure::invalid(format!(
            "goal {goal_id} is not in project {}",
            project_id.unwrap_or_default()
        )));
    }
    Ok(goal)
}

fn check_revision(goal: &SupervisorGoal, expected: i64) -> Result<(), RequestFailure> {
    if goal.revision == expected {
        return Ok(());
    }
    Err(SupervisorError::RevisionConflict {
        goal_id: goal.goal_id.clone(),
        expected,
        actual: goal.revision,
    }
    .into())
}

/// Validated payload of a goal action, parsed before anything is read.
enum GoalPayload {
    Correct(CorrectPayload),
    Reason(Option<String>),
    Empty,
    Depend(WaitTarget),
}

fn goal_payload(action: Action, value: &Value) -> Result<GoalPayload, RequestFailure> {
    Ok(match action {
        Action::Correct => {
            let correct: CorrectPayload = payload(value)?;
            require_text("text", &correct.text)?;
            if let Some(requirements) = &correct.requirements {
                contract::validate(requirements, 1)?;
            }
            GoalPayload::Correct(correct)
        }
        Action::Pause | Action::Cancel => {
            let reason: ReasonPayload = payload(value)?;
            if let Some(text) = &reason.reason {
                require_text("reason", text)?;
            }
            GoalPayload::Reason(reason.reason)
        }
        Action::Resume | Action::Approve => {
            let _: EmptyPayload = payload(value)?;
            GoalPayload::Empty
        }
        Action::Depend => {
            let depend: DependPayload = payload(value)?;
            GoalPayload::Depend(match (depend.child_goal_id, depend.event_key) {
                (Some(child), None) => WaitTarget::ChildGoal(child),
                (None, Some(key)) => WaitTarget::ExternalEvent(key),
                _ => {
                    return Err(RequestFailure::invalid(
                        "depend needs exactly one of childGoalId or eventKey",
                    ))
                }
            })
        }
        _ => return Err(RequestFailure::invalid("not a goal action")),
    })
}

async fn goal_action(
    pool: &SqlitePool,
    context: &RequestContext,
    envelope: &Envelope,
    action: Action,
) -> Result<Value, RequestFailure> {
    let parsed = goal_payload(action, &envelope.payload)?;
    let goal_id = envelope.goal_id.as_deref().unwrap_or_default();
    let expected = envelope.expected_revision.unwrap_or_default();
    let goal = owned_goal(pool, context, goal_id, envelope.project_id.as_deref()).await?;
    check_revision(&goal, expected)?;
    let by = context.adapter.as_str();
    let mut extra = serde_json::Map::new();
    match parsed {
        GoalPayload::Reason(reason) if action == Action::Pause => {
            let detail = match reason {
                Some(reason) => format!("{OWNER_PAUSED}: {reason} (by {by})"),
                None => format!("{OWNER_PAUSED}: by {by}"),
            };
            pause(pool, &goal, expected, &detail).await?;
        }
        GoalPayload::Reason(reason) => {
            let detail = match reason {
                Some(reason) => format!("{OWNER_CANCELLED}: {reason} (by {by})"),
                None => format!("{OWNER_CANCELLED}: by {by}"),
            };
            let live = live_run(pool, goal_id).await?;
            let mut tx = pool.begin_write().await?;
            let cancelled = super::transition_goal_in(
                &mut tx,
                goal_id,
                expected,
                goal.run_state,
                RunState::Cancelled,
                Some(&detail),
            )
            .await?;
            sqlx::query("UPDATE supervisor_goals SET held_from = NULL, held_detail = NULL WHERE goal_id = ?")
                .bind(goal_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            wakes::poke();
            extra.insert(
                "workerStop".into(),
                stop_worker(context, live, goal_id, cancelled.revision).await,
            );
        }
        GoalPayload::Empty if action == Action::Resume => {
            let restored = resume(pool, &goal, expected, &format!("resumed by {by}")).await?;
            extra.insert("restored".into(), json!(restored));
        }
        GoalPayload::Empty => {
            let mode_changed = approve(pool, &goal, expected, by).await?;
            let grant = admission::goal_grant(pool, goal_id).await?;
            extra.insert("grant".into(), json!(grant));
            extra.insert("modeChanged".into(), json!(mode_changed));
        }
        GoalPayload::Correct(correct) => {
            let dispatch = correct_goal(pool, &goal, expected, &correct).await?;
            extra.insert("dispatchRequestId".into(), json!(dispatch.request_id));
            extra.insert("carriedOriginal".into(), json!(dispatch.carried));
        }
        GoalPayload::Depend(target) => {
            let wait = wakes::add_dependency_at(pool, goal_id, &target, Some(expected)).await?;
            extra.insert("waitId".into(), json!(wait.wait_id));
        }
    }
    let mut receipt = goal_receipt(pool, goal_id).await?;
    if let Some(fields) = receipt.as_object_mut() {
        fields.insert("action".into(), json!(envelope.action));
        fields.extend(extra);
    }
    Ok(receipt)
}

/// Owner pause, in one write transaction: the compare-and-set into `paused` (revision bump,
/// unsent intents superseded) and the state and detail it interrupted, which a resume
/// restores. A pause never stops the worker.
async fn pause(
    pool: &SqlitePool,
    goal: &SupervisorGoal,
    expected: i64,
    detail: &str,
) -> Result<(), RequestFailure> {
    let mut tx = pool.begin_write().await?;
    super::transition_goal_in(
        &mut tx,
        &goal.goal_id,
        expected,
        goal.run_state,
        RunState::Paused,
        Some(detail),
    )
    .await?;
    sqlx::query("UPDATE supervisor_goals SET held_from = ?, held_detail = ? WHERE goal_id = ?")
        .bind(goal.run_state.as_str())
        .bind(goal.state_detail.as_deref())
        .bind(&goal.goal_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    wakes::poke();
    Ok(())
}

/// Owner resume of a paused goal, in one write transaction under the revision
/// compare-and-set: the target is [`release_target`] with the pre-pause state as a tie
/// breaker (an interrupted `awaiting_owner` hold is restored, a `verifying` detail kept). A
/// legacy lane pause row left on the goal's lane (written before lane pauses of supervisor
/// lanes became goal pauses) is cleared in the same transaction, or the dispatcher would hold
/// the goal `LANE_PAUSED` again on its next claim.
async fn resume(
    pool: &SqlitePool,
    goal: &SupervisorGoal,
    expected: i64,
    detail: &str,
) -> Result<&'static str, RequestFailure> {
    if goal.run_state != RunState::Paused {
        return Err(SupervisorError::StateConflict {
            goal_id: goal.goal_id.clone(),
            expected: RunState::Paused,
            actual: goal.run_state,
        }
        .into());
    }
    let goal_id = goal.goal_id.as_str();
    let mut tx = pool.begin_write().await?;
    let (held_from, held_detail): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT held_from, held_detail FROM supervisor_goals WHERE goal_id = ?")
            .bind(goal_id)
            .fetch_one(&mut *tx)
            .await?;
    let held_from = held_from
        .as_deref()
        .map(str::parse::<RunState>)
        .transpose()?;
    let restore_hold = (held_from == Some(RunState::AwaitingOwner))
        .then(|| held_detail.clone().unwrap_or_else(|| detail.to_owned()));
    let held_verifying = (held_from == Some(RunState::Verifying))
        .then(|| held_detail.clone())
        .flatten();
    let (to, to_detail, requeue) =
        match release_target(&mut tx, goal_id, restore_hold, held_verifying, detail).await? {
            Release::To {
                to,
                detail,
                requeue,
            } => (to, detail, requeue),
            // Nothing the goal could do next without the owner: held for an explicit decision.
            Release::Hold(why) => (RunState::AwaitingOwner, why, false),
        };
    let moved = super::transition_goal_in(
        &mut tx,
        goal_id,
        expected,
        RunState::Paused,
        to,
        Some(&to_detail),
    )
    .await?;
    if requeue {
        requeue_unsent(&mut tx, goal_id, moved.revision).await?;
    }
    sqlx::query(
        "UPDATE supervisor_goals SET held_from = NULL, held_detail = NULL WHERE goal_id = ?",
    )
    .bind(goal_id)
    .execute(&mut *tx)
    .await?;
    clear_lane_pause(&mut tx, &goal.lane_key).await?;
    tx.commit().await?;
    wakes::poke();
    super::dispatcher::poke();
    Ok(to.as_str())
}

/// Where an owner release (resume, approve) takes a held goal.
enum Release {
    /// Move to `to` with `detail`; `requeue` queues the never-transmitted instruction again.
    To {
        to: RunState,
        detail: String,
        requeue: bool,
    },
    /// Nothing can follow without an owner decision; why.
    Hold(String),
}

/// The state a held goal is released to, decided from what is true now, in this order:
/// 1. a live run goes on: `running`;
/// 2. an unresolved uncertain delivery stays the owner's: `delivery_uncertain`;
/// 3. another dispatch still in flight (a claim not yet released, an accepted send) is
///    `DISPATCH_IN_FLIGHT` (409), nothing changes, retry;
/// 4. `restore_hold` (resume of a pause that interrupted `awaiting_owner`): that hold;
/// 5. the latest instruction was never transmitted (ready, superseded by a hold, released
///    by the dispatcher): `ready`, with it queued again;
/// 6. pending waits: `waiting_children` / `waiting_event`;
/// 7. the latest run settled: `verifying` (`TURN_SETTLED`, or `held_verifying`'s detail),
///    exactly where the settle wake would have put it;
/// 8. otherwise (no dispatch at all, or a delivery that never settled on the current
///    generation, or an uncertain delivery the owner resolved): [`Release::Hold`]. A `ready`
///    goal is never left without an intent; resending such an instruction is the owner's
///    explicit `correct`.
async fn release_target(
    conn: &mut sqlx::SqliteConnection,
    goal_id: &str,
    restore_hold: Option<String>,
    held_verifying: Option<String>,
    detail: &str,
) -> Result<Release, RequestFailure> {
    let uncertain: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM supervisor_dispatches d
            JOIN supervisor_execution_bindings b ON b.goal_id = d.goal_id AND b.generation = d.generation
            WHERE d.goal_id = ? AND d.state = 'uncertain' AND d.owner_resolved_at IS NULL)",
    )
    .bind(goal_id)
    .fetch_one(&mut *conn)
    .await?;
    let pending: Vec<String> = sqlx::query_scalar(
        "SELECT kind FROM supervisor_waits WHERE goal_id = ? AND state = 'pending'",
    )
    .bind(goal_id)
    .fetch_all(&mut *conn)
    .await?;
    let latest = latest_dispatch_state(&mut *conn, goal_id).await?;
    let to = |to: RunState, detail: String, requeue: bool| Release::To {
        to,
        detail,
        requeue,
    };
    Ok(if super::has_live_run(&mut *conn, goal_id).await? {
        to(RunState::Running, detail.to_owned(), false)
    } else if uncertain {
        to(RunState::DeliveryUncertain, detail.to_owned(), false)
    } else if let Some(request_id) = super::in_flight_dispatch(&mut *conn, goal_id).await? {
        return Err(SupervisorError::DispatchInFlight {
            goal_id: goal_id.to_owned(),
            request_id,
        }
        .into());
    } else if let Some(hold) = restore_hold {
        Release::Hold(hold)
    } else if matches!(latest.as_deref(), Some("ready" | "superseded" | "released")) {
        to(RunState::Ready, detail.to_owned(), true)
    } else if !pending.is_empty() {
        let state = if pending.iter().any(|kind| kind == "child_goal") {
            RunState::WaitingChildren
        } else {
            RunState::WaitingEvent
        };
        to(
            state,
            format!(
                "{}: {} pending dependencies ({detail})",
                wakes::WAITING_ON,
                pending.len()
            ),
            false,
        )
    } else if latest.as_deref() == Some("settled") {
        to(
            RunState::Verifying,
            held_verifying.unwrap_or_else(|| wakes::TURN_SETTLED.to_owned()),
            false,
        )
    } else {
        Release::Hold(format!(
            "{OWNER_DECISION_NEEDED}: the goal has no unsent instruction and no settled run to verify (latest dispatch: {}); correct, then approve",
            latest.as_deref().unwrap_or("none")
        ))
    })
}

/// Clears a legacy lane pause row on a supervisor goal's lane, with its history line, inside
/// the caller's transaction. False (nothing written) when the lane is not paused.
async fn clear_lane_pause(
    conn: &mut sqlx::SqliteConnection,
    lane_key: &str,
) -> Result<bool, RequestFailure> {
    let now = crate::storage::now_rfc3339();
    let cleared = sqlx::query(
        "UPDATE lane_pauses SET resumed_at = ? WHERE lane_key = ? AND resumed_at IS NULL",
    )
    .bind(&now)
    .bind(lane_key)
    .execute(&mut *conn)
    .await?;
    if cleared.rows_affected() == 0 {
        return Ok(false);
    }
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM lanes WHERE key = ?")
        .bind(lane_key)
        .fetch_optional(&mut *conn)
        .await?;
    if let Some(status) = status {
        sqlx::query(
            "INSERT INTO lane_history(lane_key, at, from_status, to_status, reason) VALUES (?, ?, ?, ?, 'resumed')",
        )
        .bind(lane_key)
        .bind(&now)
        .bind(&status)
        .bind(&status)
        .execute(&mut *conn)
        .await?;
    }
    Ok(true)
}

/// State of the goal's latest dispatch row, if any.
async fn latest_dispatch_state(
    conn: &mut sqlx::SqliteConnection,
    goal_id: &str,
) -> Result<Option<String>, RequestFailure> {
    Ok(sqlx::query_scalar(
        "SELECT state FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(goal_id)
    .fetch_optional(&mut *conn)
    .await?)
}

/// Releases an `awaiting_owner` hold under the goal's existing grant, in one write
/// transaction: the compare-and-set to [`release_target`]'s state (`ready` with the
/// never-transmitted intent queued again, or `verifying` when the hold came after a settled
/// run, e.g. a verify-time `OWNER_APPROVAL_REQUIRED` or a model stop), and the lane mode
/// switch to `full` (the dispatcher sends a non-full goal's turns only with the owner, so the
/// approval is the owner's standing approval for this goal's remaining turns; reported as
/// `modeChanged`). When nothing could follow the approval (no unsent instruction, no settled
/// run to verify) it is refused `NOTHING_TO_RELEASE` (409): the owner sends a `correct`
/// instead. A 409 changes nothing. Returns whether the mode changed.
async fn approve(
    pool: &SqlitePool,
    goal: &SupervisorGoal,
    expected: i64,
    by: &str,
) -> Result<bool, RequestFailure> {
    if goal.run_state != RunState::AwaitingOwner {
        return Err(SupervisorError::StateConflict {
            goal_id: goal.goal_id.clone(),
            expected: RunState::AwaitingOwner,
            actual: goal.run_state,
        }
        .into());
    }
    let detail = goal.state_detail.as_deref().unwrap_or("");
    if detail.starts_with(GRANT_REVOKED) {
        return Err(RequestFailure::new(
            "FORBIDDEN",
            format!("an approval cannot restore a revoked grant: {detail}"),
        ));
    }
    if detail.starts_with(TURN_LIMIT) {
        return Err(RequestFailure::new(
            TURN_LIMIT,
            format!("the goal's turn budget is spent; an approval does not raise it: {detail}"),
        ));
    }
    let mut tx = pool.begin_write().await?;
    let approved = format!("{OWNER_APPROVED}: by {by}");
    let (to, to_detail, requeue) =
        match release_target(&mut tx, &goal.goal_id, None, None, &approved).await? {
            Release::To {
                to,
                detail,
                requeue,
            } => (to, detail, requeue),
            Release::Hold(why) => {
                return Err(RequestFailure::new(
                    NOTHING_TO_RELEASE,
                    format!("approving would leave the goal with nothing to do: {why}"),
                ))
            }
        };
    let moved = super::transition_goal_in(
        &mut tx,
        &goal.goal_id,
        expected,
        RunState::AwaitingOwner,
        to,
        Some(&to_detail),
    )
    .await?;
    let mode: Option<String> = sqlx::query_scalar("SELECT mode FROM lane_goal WHERE lane_key = ?")
        .bind(&goal.lane_key)
        .fetch_optional(&mut *tx)
        .await?;
    let mode_changed = mode.as_deref() != Some(GoalMode::Full.as_str());
    if mode_changed {
        contract::set_mode_in(&mut tx, &goal.lane_key, GoalMode::Full).await?;
    }
    if requeue {
        requeue_unsent(&mut tx, &goal.goal_id, moved.revision).await?;
    }
    tx.commit().await?;
    wakes::poke();
    super::dispatcher::poke();
    Ok(mode_changed)
}

/// Queues the goal's latest intent again at `revision` when it was never transmitted: an
/// owner hold `superseded` it, or the dispatcher `released` its claim before transmitting
/// (a pause during `dispatching`, a `SEND_REFUSED` hold). Nothing when an intent is already
/// ready or the latest dispatch was transmitted. Inside the caller's transaction.
async fn requeue_unsent(
    conn: &mut sqlx::SqliteConnection,
    goal_id: &str,
    revision: i64,
) -> Result<(), RequestFailure> {
    let latest: Option<(String, String)> = sqlx::query_as(
        "SELECT state, instruction FROM supervisor_dispatches WHERE goal_id = ?
         ORDER BY id DESC LIMIT 1",
    )
    .bind(goal_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((state, instruction)) = latest else {
        return Ok(());
    };
    if state != "superseded" && state != "released" {
        return Ok(());
    }
    let generation = super::require_binding(&mut *conn, goal_id)
        .await?
        .generation;
    super::insert_dispatch(
        &mut *conn,
        goal_id,
        revision,
        generation,
        &instruction,
        &crate::storage::now_rfc3339(),
    )
    .await?;
    Ok(())
}

/// A correction's queued intent, and whether it carries the never-transmitted instruction it
/// replaced.
struct Corrected {
    request_id: String,
    carried: bool,
}

/// Owner correction, in one write transaction under the revision compare-and-set: the new
/// requirements (when given), the revision bump that supersedes every unsent intent, and the
/// corrected intent at the new revision. When the goal's latest intent was never transmitted
/// (ready, superseded by a hold, or released), the corrected instruction carries it, so the
/// worker gets the task and the correction together. An instruction over the body limit is
/// refused before any write.
async fn correct_goal(
    pool: &SqlitePool,
    goal: &SupervisorGoal,
    expected: i64,
    correct: &CorrectPayload,
) -> Result<Corrected, RequestFailure> {
    let goal_id = goal.goal_id.as_str();
    let mut tx = pool.begin_write().await?;
    let current = super::require_goal(&mut tx, goal_id).await?;
    super::check_revision(&current, expected)?;
    super::check_open(&current)?;
    let latest: Option<(String, String)> = sqlx::query_as(
        "SELECT state, instruction FROM supervisor_dispatches WHERE goal_id = ?
         ORDER BY id DESC LIMIT 1",
    )
    .bind(goal_id)
    .fetch_optional(&mut *tx)
    .await?;
    // A claimed send is not carried: it may already be typed, and revise_goal_in fences it.
    let original = latest
        .filter(|(state, _)| matches!(state.as_str(), "ready" | "superseded" | "released"))
        .map(|(_, instruction)| instruction);
    let revision = current.revision + 1;
    let instruction = corrected_instruction(original.as_deref(), revision, correct);
    if instruction.chars().count() > super::MAX_BODY_CHARS {
        return Err(RequestFailure::invalid(format!(
            "the corrected instruction would exceed {} characters; shorten the correction",
            super::MAX_BODY_CHARS
        )));
    }
    if let Some(requirements) = &correct.requirements {
        let (mode, turn_limit): (String, i64) =
            sqlx::query_as("SELECT mode, turn_limit FROM lane_goal WHERE lane_key = ?")
                .bind(&goal.lane_key)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| RequestFailure::new("NOT_FOUND", "the goal has no contract"))?;
        let mode: GoalMode = mode.parse()?;
        let turn_limit = u32::try_from(turn_limit).unwrap_or(u32::MAX);
        contract::set_contract_in(&mut tx, &goal.lane_key, requirements, mode, turn_limit).await?;
    }
    // The same revision path as every owner correction (supersede, claimed-send fence).
    let revised = super::revise_goal_in(&mut tx, goal_id, expected).await?;
    debug_assert_eq!(revised.revision, revision);
    let now = revised.updated_at.clone();
    let generation = super::require_binding(&mut tx, goal_id).await?.generation;
    let dispatch =
        super::insert_dispatch(&mut tx, goal_id, revision, generation, &instruction, &now).await?;
    tx.commit().await?;
    wakes::poke();
    super::dispatcher::poke();
    Ok(Corrected {
        request_id: dispatch.request_id,
        carried: original.is_some(),
    })
}

/// The corrected worker instruction: the never-transmitted instruction it replaces (when
/// there is one), the owner's correction verbatim, and the replacement requirements.
fn corrected_instruction(
    original: Option<&str>,
    revision: i64,
    correct: &CorrectPayload,
) -> String {
    let mut text = String::new();
    if let Some(original) = original {
        text.push_str(original.trim_end());
        text.push_str("\n\n");
    }
    text.push_str(&format!(
        "Owner correction at goal revision {revision} (verbatim):\n{}\n",
        correct.text.trim()
    ));
    if let Some(block) =
        super::memory::task_only_block(&super::memory::split_task_only(&correct.text).1)
    {
        text.push_str(&format!("\n{block}\n"));
    }
    if let Some(requirements) = &correct.requirements {
        text.push_str("\nThe requirements are now:\n");
        for requirement in requirements {
            text.push_str(&format!(
                "- {}: {} (verified by: {})\n",
                requirement.id.trim(),
                requirement.text.trim(),
                requirement.verifier.trim()
            ));
        }
    }
    text.push_str(if original.is_some() {
        "\nThe correction overrides the instruction above where they conflict."
    } else {
        "\nApply it to the current work; it overrides earlier instructions where they conflict."
    });
    text
}

/// `(host, session)` of the goal's started, unsettled run on its current generation.
async fn live_run(
    pool: &SqlitePool,
    goal_id: &str,
) -> Result<Option<(Option<String>, String)>, RequestFailure> {
    let Some(binding) = execution_binding_of(pool, goal_id).await? else {
        return Ok(None);
    };
    let Some(session) = binding.provider_session_id.clone() else {
        return Ok(None);
    };
    let started: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM supervisor_dispatches
         WHERE goal_id = ? AND state = 'started' AND generation = ? AND provider_session_id = ?)",
    )
    .bind(goal_id)
    .bind(binding.generation)
    .bind(&session)
    .fetch_one(pool)
    .await?;
    if !started {
        return Ok(None);
    }
    let host = (binding.host_id != LOCAL_HOST_ID).then_some(binding.host_id);
    Ok(Some((host, session)))
}

/// Stops a cancelled goal's live run once, after the cancel committed.
async fn stop_worker(
    context: &RequestContext,
    live: Option<(Option<String>, String)>,
    goal_id: &str,
    revision: i64,
) -> Value {
    let Some((host, session)) = live else {
        return json!({"state": "no_live_run"});
    };
    let Some(control) = &context.control else {
        return json!({"state": "not_available", "sessionId": session,
            "detail": "this process does not reach agent control; stop the worker explicitly"});
    };
    let op = AgentOp::Stop {
        session_id: session.clone(),
        request_id: format!("{goal_id}-cancel-r{revision}"),
    };
    match control.execute(host.as_deref(), op).await {
        Ok(_) => json!({"state": "sent", "sessionId": session}),
        Err(error) => json!({"state": "failed", "sessionId": session, "detail": error.to_string()}),
    }
}

/// The legacy lane pause (`SupervisorSetPaused`, remote `POST /supervisor/pause`) as an
/// adapter to this service. `Ok(None)`: the lane is not a supervisor goal's, the legacy lane
/// pause applies unchanged. A supervisor goal's lane is paused or resumed through the goal
/// (`pause`/`resume` at its current revision: revision bump, `paused` run state, the pre-pause
/// state restored on resume); `Ok(Some(changed))` keeps the legacy answer, false when the goal
/// already was so. A legacy lane an adoption took over is `SUPERVISOR_OWNED`: its work goes on
/// in the adopting goal, which is paused instead.
pub async fn lane_pause(
    pool: &SqlitePool,
    context: &RequestContext,
    lane_key: &str,
    paused: bool,
) -> Result<Option<bool>, RequestFailure> {
    let goal_id: Option<String> =
        sqlx::query_scalar("SELECT goal_id FROM supervisor_goals WHERE lane_key = ?")
            .bind(lane_key)
            .fetch_optional(pool)
            .await?;
    let Some(goal_id) = goal_id else {
        if super::supervisor_owns_lane(pool, lane_key).await? {
            return Err(RequestFailure::new(
                super::SUPERVISOR_OWNED,
                format!(
                    "lane {lane_key} was adopted by a supervisor goal; pause that goal instead"
                ),
            ));
        }
        return Ok(None);
    };
    let goal = owned_goal(pool, context, &goal_id, None).await?;
    let action = match (paused, goal.run_state == RunState::Paused) {
        (true, true) => return Ok(Some(false)),
        (false, false) => {
            // A legacy lane pause row stranded on the goal's lane (the dispatcher would hold
            // the goal `LANE_PAUSED` on its next claim) is cleared; nothing else changes.
            let mut tx = pool.begin_write().await?;
            let cleared = clear_lane_pause(&mut tx, lane_key).await?;
            tx.commit().await?;
            if cleared {
                super::dispatcher::poke();
            }
            return Ok(Some(cleared));
        }
        (true, false) => "pause",
        (false, true) => "resume",
    };
    let envelope = json!({
        "requestId": format!("lane-{action}:{lane_key}:r{}", goal.revision),
        "goalId": goal_id,
        "expectedRevision": goal.revision,
        "action": action,
        "payload": {},
    });
    handle(pool, context, envelope).await?;
    Ok(Some(true))
}

/// The owner's native memory receipts with their provenance, as `GET /memory` serves them.
pub async fn memory_receipts(
    pool: &SqlitePool,
    context: &RequestContext,
) -> Result<Value, RequestFailure> {
    super::memory::receipts(pool, context, None).await
}

/// A goal's lane title and continuation budget (`mode`, `turn_limit`, `turns_used`), each
/// budget field null without a contract row.
type LaneTerms = (String, Option<String>, Option<i64>, Option<i64>);

/// One goal as every adapter reports it: identity, authoritative revision, run state, wait
/// reason, pending waits and the next persisted trigger, plus what the Supervisor views show
/// about it: the lane title, the continuation budget (`mode`, `turnLimit`, `turnsUsed`; null
/// without a contract row) and the persisted operation grant (null when it does not parse).
pub async fn goal_receipt(pool: &SqlitePool, goal_id: &str) -> Result<Value, RequestFailure> {
    let goal = goal_of(pool, goal_id)
        .await?
        .ok_or_else(|| RequestFailure::new("NOT_FOUND", format!("goal {goal_id} not found")))?;
    let waits: Vec<Value> = waits_of(pool, goal_id)
        .await?
        .into_iter()
        .filter(|wait| wait.state == WaitState::Pending)
        .map(|wait| {
            json!({"waitId": wait.wait_id, "kind": wait.kind, "childGoalId": wait.child_goal_id,
                "eventKey": wait.event_key})
        })
        .collect();
    let next_trigger: Option<String> = sqlx::query_scalar(
        "SELECT MIN(due_at) FROM supervisor_pending_wakes WHERE goal_id = ? AND consumed_at IS NULL",
    )
    .bind(goal_id)
    .fetch_one(pool)
    .await?;
    let lane: Option<LaneTerms> = sqlx::query_as(
        "SELECT l.title, g.mode, g.turn_limit, g.turns_used
         FROM lanes l LEFT JOIN lane_goal g ON g.lane_key = l.key WHERE l.key = ?",
    )
    .bind(&goal.lane_key)
    .fetch_optional(pool)
    .await?;
    let (title, mode, turn_limit, turns_used) = lane.unwrap_or((String::new(), None, None, None));
    let grant = admission::goal_grant(pool, goal_id).await.ok();
    Ok(json!({
        "goalId": goal.goal_id,
        "supervisorId": goal.supervisor_id,
        "projectId": goal.project_id,
        "laneKey": goal.lane_key,
        "title": title,
        "runState": goal.run_state,
        "stateDetail": goal.state_detail,
        "revision": goal.revision,
        "pendingWaits": waits,
        "nextTrigger": next_trigger,
        "updatedAt": goal.updated_at,
        "mode": mode,
        "turnLimit": turn_limit,
        "turnsUsed": turns_used,
        "grant": grant,
    }))
}

/// Supervisor goals for the snapshot: every open goal and the latest ended ones, and the
/// owner's projects (`{projectId, name}`, empty without a single identity) so a view can read
/// a project's conversation and submit into it.
pub async fn snapshot(pool: &SqlitePool) -> Result<Value, RequestFailure> {
    let identities: Vec<(String, String)> = sqlx::query_as(
        "SELECT supervisor_id, owner_principal FROM supervisor_identity ORDER BY created_at LIMIT 2",
    )
    .fetch_all(pool)
    .await?;
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT goal_id FROM supervisor_goals
         ORDER BY run_state IN ('completed','cancelled'), updated_at DESC LIMIT 100",
    )
    .fetch_all(pool)
    .await?;
    let mut goals = Vec::with_capacity(ids.len());
    for id in ids {
        goals.push(goal_receipt(pool, &id).await?);
    }
    let (identity, projects) = match identities.as_slice() {
        [(supervisor_id, owner)] => {
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT project_id, name FROM supervisor_projects WHERE supervisor_id = ?
                 ORDER BY created_at, project_id",
            )
            .bind(supervisor_id)
            .fetch_all(pool)
            .await?;
            let projects: Vec<Value> = rows
                .into_iter()
                .map(|(project_id, name)| json!({"projectId": project_id, "name": name}))
                .collect();
            (json!({"supervisorId": supervisor_id, "ownerPrincipal": owner}), projects)
        }
        _ => (Value::Null, Vec::new()),
    };
    Ok(json!({"identity": identity, "projects": projects, "goals": goals}))
}

/// The admitted owner messages of one project after a cursor, oldest first: the conversation
/// provenance the hub keeps. Engine replies are not persisted here.
pub async fn conversation(
    pool: &SqlitePool,
    context: &RequestContext,
    project_id: &str,
    after: i64,
) -> Result<Value, RequestFailure> {
    let identity = owner_identity(pool, context).await?;
    let owner: Option<String> =
        sqlx::query_scalar("SELECT supervisor_id FROM supervisor_projects WHERE project_id = ?")
            .bind(project_id)
            .fetch_optional(pool)
            .await?;
    match owner {
        None => {
            return Err(RequestFailure::new(
                "NOT_FOUND",
                format!("project {project_id} not found"),
            ))
        }
        Some(owner) if owner != identity.supervisor_id => {
            return Err(RequestFailure::new(
                "FORBIDDEN",
                format!("project {project_id} belongs to another owner"),
            ))
        }
        Some(_) => {}
    }
    type Row = (i64, String, String, String, String, i64, String);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT r.id, r.source_adapter, r.source_message_id, r.body, r.goal_id,
                r.admitted_revision, r.created_at
         FROM supervisor_requests r JOIN supervisor_goals g ON g.goal_id = r.goal_id
         WHERE g.project_id = ? AND r.id > ? ORDER BY r.id LIMIT 200",
    )
    .bind(project_id)
    .bind(after)
    .fetch_all(pool)
    .await?;
    let next = rows.last().map_or(after, |row| row.0);
    let messages: Vec<Value> = rows
        .into_iter()
        .map(|(id, adapter, message_id, body, goal_id, revision, at)| {
            json!({"id": id, "sourceAdapter": adapter, "sourceMessageId": message_id, "body": body,
                "goalId": goal_id, "admittedRevision": revision, "createdAt": at})
        })
        .collect();
    Ok(json!({"projectId": project_id, "messages": messages, "next": next}))
}
