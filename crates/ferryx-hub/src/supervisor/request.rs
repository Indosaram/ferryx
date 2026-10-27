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
//! can reach agent control. `approve` releases an `awaiting_owner` hold under the grant the
//! goal already has. It never widens that grant: more operations need a new submission with
//! explicit owner choices.
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::admission::{self, ActorContext, AdmissionError, AdoptWork, OwnerChoices, SubmitWork};
use super::dispatcher::{GRANT_REVOKED, TURN_LIMIT};
use super::wakes::{self, DependencyError};
use super::{
    enqueue_dispatch, execution_binding_of, goal_of, identity_of, revise_goal, transition_goal,
    waits_of, ChannelProvider, RunState, SupervisorError, SupervisorGoal, WaitState, WaitTarget,
};
use crate::agent_control::{AgentControl, AgentOp, LOCAL_HOST_ID};
use crate::goals::contract::{self, RequirementSpec};
use crate::goals::types::GoalMode;
use crate::meta::mhc_supervisor::{IngestOutcome, MhcSupervisor};

/// No memory engine is reachable from this process (the MCP server and CLI never run one).
pub const MEMORY_UNAVAILABLE: &str = "MEMORY_UNAVAILABLE";
/// The hub has no native memory-forget/correct RPC yet (todo 15 completes it).
pub const MEMORY_FORGET_UNAVAILABLE: &str = "MEMORY_FORGET_UNAVAILABLE";
/// The hub has no native memory-receipt listing yet (todo 15 completes it).
pub const MEMORY_RECEIPTS_UNAVAILABLE: &str = "MEMORY_RECEIPTS_UNAVAILABLE";
/// `state_detail` prefix of an owner pause.
pub const OWNER_PAUSED: &str = "OWNER_PAUSED";
/// `state_detail` prefix of an owner cancel.
pub const OWNER_CANCELLED: &str = "OWNER_CANCELLED";
/// `state_detail` prefix of an owner approval releasing a hold.
pub const OWNER_APPROVED: &str = "OWNER_APPROVED";

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

/// HTTP status of a supervisor wire code.
pub fn status_for_code(code: &str) -> u16 {
    match code {
        "INVALID_REQUEST" | "WAIT_CYCLE" | "CROSS_OWNER" => 400,
        "UNAUTHORIZED" | "FORBIDDEN" => 403,
        "NOT_FOUND" => 404,
        "STORAGE" | "INTERNAL" => 500,
        "SUPERVISOR_UNAVAILABLE" => 503,
        code if code.starts_with("MHC_") || code.starts_with("MEMORY_") => 503,
        _ => 409,
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
            GoalError::Sql(_) => "STORAGE",
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
    /// The retained mhc supervisor, when this process runs one.
    pub memory: Option<MhcSupervisor>,
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

    /// Actions that name one existing goal.
    pub fn targets_goal(self) -> bool {
        matches!(
            self,
            Self::Correct | Self::Pause | Self::Resume | Self::Cancel | Self::Approve | Self::Depend
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TeachPayload {
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ForgetPayload {
    memory_id: String,
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
        Action::Message | Action::Submit | Action::Adopt => admit(pool, context, &envelope, action).await,
        Action::Teach => teach(pool, context, &envelope).await,
        Action::Forget => {
            let forget: ForgetPayload = payload(&envelope.payload)?;
            require_text("memoryId", &forget.memory_id)?;
            owner_identity(pool, context).await?;
            Err(RequestFailure::new(
                MEMORY_FORGET_UNAVAILABLE,
                "the hub has no native memory forget/correct operation yet; nothing was forgotten",
            ))
        }
        Action::Correct
        | Action::Pause
        | Action::Resume
        | Action::Cancel
        | Action::Approve
        | Action::Depend => goal_action(pool, context, &envelope, action).await,
    }
}

/// Fills a work proposal's project from the envelope; a contradiction is refused.
fn with_project(proposed: &mut Option<String>, envelope: Option<&str>) -> Result<(), RequestFailure> {
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
        _ => {
            let mut adopt: AdoptWork = payload(&envelope.payload)?;
            with_project(&mut adopt.project_id, project)?;
            let body = format!("adopt {}", adopt.lane_key);
            admission::adopt_work(pool, &actor(body), &adopt).await?
        }
    };
    serde_json::to_value(&outcome).map_err(|error| RequestFailure::new("INTERNAL", error.to_string()))
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
    if goal.supervisor_id != identity.supervisor_id || goal.owner_principal != identity.owner_principal {
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
            transition_goal(pool, goal_id, expected, goal.run_state, RunState::Paused, Some(&detail))
                .await?;
        }
        GoalPayload::Reason(reason) => {
            let detail = match reason {
                Some(reason) => format!("{OWNER_CANCELLED}: {reason} (by {by})"),
                None => format!("{OWNER_CANCELLED}: by {by}"),
            };
            let live = live_run(pool, goal_id).await?;
            let cancelled = transition_goal(
                pool,
                goal_id,
                expected,
                goal.run_state,
                RunState::Cancelled,
                Some(&detail),
            )
            .await?;
            extra.insert(
                "workerStop".into(),
                stop_worker(context, live, goal_id, cancelled.revision).await,
            );
        }
        GoalPayload::Empty if action == Action::Resume => {
            if goal.run_state != RunState::Paused {
                return Err(SupervisorError::StateConflict {
                    goal_id: goal_id.to_owned(),
                    expected: RunState::Paused,
                    actual: goal.run_state,
                }
                .into());
            }
            requeue_unsent(pool, &goal).await?;
            resume_to_live(pool, goal_id, expected, RunState::Paused, &format!("resumed by {by}"))
                .await?;
        }
        GoalPayload::Empty => {
            approve(pool, &goal, expected, by).await?;
            let grant = admission::goal_grant(pool, goal_id).await?;
            extra.insert("grant".into(), json!(grant));
        }
        GoalPayload::Correct(correct) => {
            if goal.run_state.is_terminal() {
                return Err(SupervisorError::GoalClosed {
                    goal_id: goal_id.to_owned(),
                    state: goal.run_state,
                }
                .into());
            }
            if let Some(requirements) = &correct.requirements {
                let current = contract::contract_of(pool, &goal.lane_key)
                    .await?
                    .ok_or_else(|| RequestFailure::new("NOT_FOUND", "the goal has no contract"))?;
                contract::set_contract(
                    pool,
                    &goal.lane_key,
                    requirements,
                    current.mode,
                    current.turn_limit,
                )
                .await?;
            }
            let revised = revise_goal(pool, goal_id, expected).await?;
            let instruction = format!(
                "Owner correction at goal revision {} (verbatim):\n{}\n\nApply it to the current work; it overrides earlier instructions where they conflict.",
                revised.revision,
                correct.text.trim()
            );
            let dispatch = enqueue_dispatch(pool, goal_id, revised.revision, &instruction).await?;
            super::dispatcher::poke();
            extra.insert("dispatchRequestId".into(), json!(dispatch.request_id));
        }
        GoalPayload::Depend(target) => {
            let wait = wakes::add_dependency(pool, goal_id, &target).await?;
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

/// Releases an `awaiting_owner` hold under the goal's existing grant, in `full` mode.
async fn approve(
    pool: &SqlitePool,
    goal: &SupervisorGoal,
    expected: i64,
    by: &str,
) -> Result<(), RequestFailure> {
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
    contract::set_mode(pool, &goal.lane_key, GoalMode::Full).await?;
    requeue_unsent(pool, goal).await?;
    transition_goal(
        pool,
        &goal.goal_id,
        expected,
        RunState::AwaitingOwner,
        RunState::Ready,
        Some(&format!("{OWNER_APPROVED}: by {by}")),
    )
    .await?;
    super::dispatcher::poke();
    Ok(())
}

/// An owner hold superseded the goal's unsent intent. When the goal's latest dispatch is that
/// superseded intent, its instruction is queued again at the current revision, so the goal is
/// never `ready` without the work it held.
async fn requeue_unsent(pool: &SqlitePool, goal: &SupervisorGoal) -> Result<(), RequestFailure> {
    let latest: Option<(String, String)> = sqlx::query_as(
        "SELECT state, instruction FROM supervisor_dispatches WHERE goal_id = ?
         ORDER BY id DESC LIMIT 1",
    )
    .bind(&goal.goal_id)
    .fetch_optional(pool)
    .await?;
    if let Some((state, instruction)) = latest {
        if state == "superseded" {
            enqueue_dispatch(pool, &goal.goal_id, goal.revision, &instruction).await?;
        }
    }
    Ok(())
}

/// Resumes to `running` when the goal still has a live run, otherwise to `ready`.
async fn resume_to_live(
    pool: &SqlitePool,
    goal_id: &str,
    expected: i64,
    from: RunState,
    detail: &str,
) -> Result<(), RequestFailure> {
    match transition_goal(pool, goal_id, expected, from, RunState::Running, Some(detail)).await {
        Ok(_) => Ok(()),
        Err(SupervisorError::NoLiveRun { .. }) => {
            transition_goal(pool, goal_id, expected, from, RunState::Ready, Some(detail)).await?;
            super::dispatcher::poke();
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
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

async fn teach(
    pool: &SqlitePool,
    context: &RequestContext,
    envelope: &Envelope,
) -> Result<Value, RequestFailure> {
    let teach: TeachPayload = payload(&envelope.payload)?;
    require_text("text", &teach.text)?;
    owner_identity(pool, context).await?;
    let Some(memory) = &context.memory else {
        return Err(RequestFailure::new(
            MEMORY_UNAVAILABLE,
            "no native supervisor memory engine runs in this process; nothing was learned",
        ));
    };
    let source = format!("{}:{}", context.adapter, envelope.request_id);
    match memory.ingest_owner(&source, &teach.text).await {
        // Accepted by the owner-ingest session; persisted/applied receipts are todo 15.
        Ok(IngestOutcome::Learned { .. }) => Ok(json!({"action": "teach",
            "memory": {"state": "accepted", "sourceMessageId": source, "duplicate": false}})),
        Ok(IngestOutcome::Duplicate) => Ok(json!({"action": "teach",
            "memory": {"state": "accepted", "sourceMessageId": source, "duplicate": true}})),
        Err(fault) => Err(RequestFailure::new(fault.code(), fault.to_string())),
    }
}

/// The memory receipt listing the remote `GET /memory` serves: unavailable until todo 15.
pub fn memory_receipts_unavailable() -> RequestFailure {
    RequestFailure::new(
        MEMORY_RECEIPTS_UNAVAILABLE,
        "the hub has no native memory receipt listing yet",
    )
}

/// One goal as every adapter reports it: identity, authoritative revision, run state, wait
/// reason, pending waits and the next persisted trigger.
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
    Ok(json!({
        "goalId": goal.goal_id,
        "supervisorId": goal.supervisor_id,
        "projectId": goal.project_id,
        "laneKey": goal.lane_key,
        "runState": goal.run_state,
        "stateDetail": goal.state_detail,
        "revision": goal.revision,
        "pendingWaits": waits,
        "nextTrigger": next_trigger,
        "updatedAt": goal.updated_at,
    }))
}

/// Supervisor goals for the snapshot: every open goal and the latest ended ones.
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
    let identity = match identities.as_slice() {
        [(supervisor_id, owner)] => json!({"supervisorId": supervisor_id, "ownerPrincipal": owner}),
        _ => Value::Null,
    };
    Ok(json!({"identity": identity, "goals": goals}))
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
        None => return Err(RequestFailure::new("NOT_FOUND", format!("project {project_id} not found"))),
        Some(owner) if owner != identity.supervisor_id => {
            return Err(RequestFailure::new("FORBIDDEN", format!("project {project_id} belongs to another owner")))
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
