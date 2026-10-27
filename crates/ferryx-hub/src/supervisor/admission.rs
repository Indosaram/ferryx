//! Work admission: the `submit`, `adopt` and `message` handlers behind the supervisor's typed
//! work tools.
//!
//! The actor ([`ActorContext`]) comes from the authenticated adapter and never from model
//! arguments. The model's [`SubmitWork`] proposal is untrusted. It may narrow the work or pick
//! among the owner's own projects and hosts, but it can never widen the grant. Operations
//! beyond [`OperationClass::BASELINE`] need an explicit owner choice, and asking for more holds
//! the goal in `awaiting_owner` instead of inventing permission.
//!
//! Admission is the store's single transaction ([`admit_request`]). The acknowledgment is
//! built only after commit, from the persisted goal, so a model can never claim that work was
//! registered. Nothing here transmits to a worker: the daemon's [`super::dispatcher`] starts
//! committed ready intents. An admission made inside the daemon wakes it with
//! [`super::dispatcher::poke`]. One made in another process (`ferryx-cli mcp`) is picked up
//! from the durable ready intent by the dispatcher's rescan.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::{
    admit_request, goal_of, identity_of, transition_goal, AdmissionReceipt, AdmitRequest,
    ChannelProvider, ExecutionKind, ExecutionTarget, RunState, SupervisorError,
    SupervisorIdentity, DEFAULT_TURN_LIMIT, LANE_KEY_PREFIX,
};
use crate::goals::contract::{contract_of, RequirementSpec};
use crate::goals::types::{GoalError, GoalMode};

/// `state_detail` prefix of a goal admitted on hold because its scope needs the owner.
pub const SCOPE_NEEDS_OWNER: &str = "SCOPE_NEEDS_OWNER";

/// What a worker may do under a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationClass {
    Edit,
    Test,
    /// A local commit. Pushing is [`OperationClass::Push`].
    Commit,
    Push,
    Publish,
    Spend,
    Destructive,
}

impl OperationClass {
    pub const ALL: &[OperationClass] = &[
        OperationClass::Edit,
        OperationClass::Test,
        OperationClass::Commit,
        OperationClass::Push,
        OperationClass::Publish,
        OperationClass::Spend,
        OperationClass::Destructive,
    ];

    /// What an owner's natural-language execution request grants by itself. Push, publication,
    /// spending and destructive changes always need an explicit owner choice.
    pub const BASELINE: &[OperationClass] = &[
        OperationClass::Edit,
        OperationClass::Test,
        OperationClass::Commit,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            OperationClass::Edit => "edit",
            OperationClass::Test => "test",
            OperationClass::Commit => "commit",
            OperationClass::Push => "push",
            OperationClass::Publish => "publish",
            OperationClass::Spend => "spend",
            OperationClass::Destructive => "destructive",
        }
    }
}

/// Choices the owner made explicitly in the adapter (a picker, a button, a flag) for this one
/// request. They are the only way to widen a grant beyond the baseline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnerChoices {
    pub project_id: Option<String>,
    pub host_id: Option<String>,
    pub kind: Option<ExecutionKind>,
    pub cwd: Option<String>,
    pub turn_limit: Option<u32>,
    pub operations: Vec<OperationClass>,
}

/// Who sent a request and what they wrote, as the authenticated adapter established it. Never
/// built from model arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorContext {
    /// The verified sender, e.g. `slack:T1:U123`.
    pub principal: String,
    pub source_adapter: ChannelProvider,
    pub source_message_id: String,
    /// The original message exactly as authored.
    pub source_body: String,
    pub channel_binding_id: Option<String>,
    pub choices: OwnerChoices,
}

/// The model's `submit_work` arguments. Untrusted: they can narrow the work, never widen it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SubmitWork {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub host_id: Option<String>,
    #[serde(default)]
    pub kind: Option<ExecutionKind>,
    pub title: String,
    /// The first worker instruction. The owner's verbatim request and the grant are appended.
    pub instruction: String,
    pub requirements: Vec<RequirementSpec>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Operations the work needs. Anything outside the grant holds the goal for the owner.
    #[serde(default)]
    pub operations: Vec<OperationClass>,
    /// May lower the continuation budget, never raise it.
    #[serde(default)]
    pub turn_limit: Option<u32>,
}

/// An explicit adoption of one legacy lane. Nothing is revived without it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AdoptWork {
    pub lane_key: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub host_id: Option<String>,
    /// Used only when the legacy lane has no goal contract of its own.
    #[serde(default)]
    pub requirements: Vec<RequirementSpec>,
}

/// A persisted goal as admission reports it, read back after commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmittedWork {
    pub receipt: AdmissionReceipt,
    /// True when this source message was admitted before; nothing new was written.
    pub duplicate: bool,
    pub run_state: RunState,
    pub state_detail: Option<String>,
    /// Current goal revision (the receipt carries the admitted one).
    pub revision: i64,
    pub mode: GoalMode,
    pub turn_limit: u32,
    pub turns_remaining: u32,
    /// The operations granted by this admission. Unknown (`None`) on a replay.
    pub grant: Option<Vec<OperationClass>>,
    /// Generated from the persisted rows, never from model text.
    pub acknowledgment: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SubmitOutcome {
    /// Persisted and `ready`: the daemon dispatcher starts it with full continuation.
    Admitted(Box<AdmittedWork>),
    /// Persisted on hold: its scope needs the owner, and nothing is sent to a worker.
    AwaitingOwner(Box<AdmittedWork>),
    /// Not work. Nothing was persisted.
    Conversational { reason: String },
    /// The owner has to pick a project, host or worker kind first. Nothing was persisted.
    NeedsChoice { missing: String, detail: String },
}

#[derive(Debug, thiserror::Error)]
pub enum AdmissionError {
    /// The sender is not the supervisor's owner (HTTP 403).
    #[error("{0}")]
    Unauthorized(String),
    #[error(transparent)]
    Store(#[from] SupervisorError),
}

impl AdmissionError {
    pub fn code(&self) -> &'static str {
        match self {
            AdmissionError::Unauthorized(_) => "UNAUTHORIZED",
            AdmissionError::Store(error) => error.code(),
        }
    }
}

impl From<sqlx::Error> for AdmissionError {
    fn from(error: sqlx::Error) -> Self {
        AdmissionError::Store(SupervisorError::Sql(error))
    }
}

impl From<GoalError> for AdmissionError {
    fn from(error: GoalError) -> Self {
        AdmissionError::Store(SupervisorError::Goal(error))
    }
}

/// The source of one admission: adapter, dedupe id and the authentic body.
struct Source<'a> {
    adapter: ChannelProvider,
    message_id: String,
    body: &'a str,
}

/// A request scope resolved against the authentic source and the owner's choices.
struct Scope {
    project_id: String,
    host_id: String,
    kind: ExecutionKind,
    cwd: String,
    turn_limit: u32,
    grant: Vec<OperationClass>,
    /// Why the owner must decide before anything dispatches. Empty means full continuation.
    holds: Vec<String>,
}

enum Resolved {
    Scope(Scope),
    NeedsChoice { missing: &'static str, detail: String },
}

fn needs_choice(missing: &'static str, detail: String) -> Result<Resolved, AdmissionError> {
    Ok(Resolved::NeedsChoice { missing, detail })
}

/// The owner of this installation, for an adapter whose caller is the local owner by
/// construction (`ferryx-cli mcp` runs as the owner's own process). Refused when no identity
/// exists, or several do and the caller cannot be told apart.
pub async fn installation_owner(pool: &SqlitePool) -> Result<String, AdmissionError> {
    let owners: Vec<String> = sqlx::query_scalar(
        "SELECT owner_principal FROM supervisor_identity ORDER BY created_at LIMIT 2",
    )
    .fetch_all(pool)
    .await?;
    match owners.as_slice() {
        [owner] => Ok(owner.clone()),
        [] => Err(AdmissionError::Unauthorized(
            "this installation has no supervisor identity".into(),
        )),
        _ => Err(AdmissionError::Unauthorized(
            "this installation has several supervisor identities; the caller is ambiguous".into(),
        )),
    }
}

/// The sender's supervisor identity. A sender without one is not an owner.
async fn authenticate(
    pool: &SqlitePool,
    actor: &ActorContext,
) -> Result<SupervisorIdentity, AdmissionError> {
    let principal = actor.principal.as_str();
    if principal.trim().is_empty() || principal.trim() != principal {
        return Err(AdmissionError::Unauthorized(
            "the request has no authenticated principal".into(),
        ));
    }
    identity_of(pool, principal).await?.ok_or_else(|| {
        AdmissionError::Unauthorized(format!("{principal} is not the supervisor's owner"))
    })
}

/// True for a single-sentence question such as "What can you do?". A polite request phrased
/// as a question ("Can you fix the build?") is not one, and neither is a question followed
/// by an instruction.
pub fn is_ordinary_question(body: &str) -> bool {
    let text = body.trim();
    let Some(question) = text.strip_suffix('?') else {
        return false;
    };
    if question.contains(['.', '!', '?', '\n']) {
        return false;
    }
    let lower = question.to_lowercase();
    const REQUESTS: [&str; 7] = [
        "can you",
        "could you",
        "would you",
        "will you",
        "can we",
        "could we",
        "please",
    ];
    if REQUESTS.iter().any(|request| lower.starts_with(request)) {
        return false;
    }
    const INTERROGATIVES: [&str; 22] = [
        "what", "what's", "whats", "how", "how's", "why", "who", "whose", "whom", "when",
        "where", "which", "is", "are", "was", "were", "do", "does", "did", "should", "has",
        "have",
    ];
    let first = lower.split_whitespace().next().unwrap_or("");
    INTERROGATIVES.contains(&first)
}

/// A conversational message never launches anything. With a `submit_work` call attached, the
/// message is admitted exactly like [`submit_work`].
pub async fn handle_message(
    pool: &SqlitePool,
    actor: &ActorContext,
    work: Option<&SubmitWork>,
) -> Result<SubmitOutcome, AdmissionError> {
    match work {
        None => Ok(SubmitOutcome::Conversational {
            reason: "a message without a work submission is conversation".into(),
        }),
        Some(work) => submit_work(pool, actor, work).await,
    }
}

/// Admits one owner execution request. A natural-language request within the baseline grant
/// becomes a `ready` goal with full continuation (turn budget [`DEFAULT_TURN_LIMIT`] unless the
/// owner chose another). A request whose scope exceeds the grant is persisted on hold in
/// `awaiting_owner`. An ordinary question, or a scope the owner must pick first, persists
/// nothing. A replay of the same source message returns the same goal, and a changed body is
/// `REQUEST_CONFLICT`, never a second launch.
pub async fn submit_work(
    pool: &SqlitePool,
    actor: &ActorContext,
    work: &SubmitWork,
) -> Result<SubmitOutcome, AdmissionError> {
    let identity = authenticate(pool, actor).await?;
    if is_ordinary_question(&actor.source_body) {
        return Ok(SubmitOutcome::Conversational {
            reason: "the owner asked a question; a question is answered, not launched".into(),
        });
    }
    let source = Source {
        adapter: actor.source_adapter,
        message_id: actor.source_message_id.clone(),
        body: &actor.source_body,
    };
    admit_scoped(pool, &identity, actor, &source, work).await
}

/// Explicitly adopts one legacy lane as a new supervisor goal. The legacy lane itself is left
/// as history. A lane is adopted once, whichever adapter asks again.
pub async fn adopt_work(
    pool: &SqlitePool,
    actor: &ActorContext,
    adopt: &AdoptWork,
) -> Result<SubmitOutcome, AdmissionError> {
    let identity = authenticate(pool, actor).await?;
    let lane_key = adopt.lane_key.as_str();
    if lane_key.trim().is_empty() || lane_key.trim() != lane_key {
        return Err(SupervisorError::Invalid("lane key is required".into()).into());
    }
    let supervised: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM supervisor_goals WHERE lane_key = ?)")
            .bind(lane_key)
            .fetch_one(pool)
            .await?;
    if supervised || lane_key.starts_with(LANE_KEY_PREFIX) {
        return Err(SupervisorError::Invalid(format!(
            "lane {lane_key} already belongs to the supervisor"
        ))
        .into());
    }
    let lane: Option<(String, String)> =
        sqlx::query_as("SELECT title, cwd FROM lanes WHERE key = ?")
            .bind(lane_key)
            .fetch_optional(pool)
            .await?;
    let Some((title, cwd)) = lane else {
        return Err(SupervisorError::NotFound {
            kind: "lane",
            id: lane_key.to_owned(),
        }
        .into());
    };
    let message_id = format!("adopt:{lane_key}");
    let earlier: Option<String> = sqlx::query_scalar(
        "SELECT source_adapter FROM supervisor_requests WHERE source_message_id = ? LIMIT 1",
    )
    .bind(&message_id)
    .fetch_optional(pool)
    .await?;
    let adapter = match earlier {
        Some(adapter) => adapter.parse::<ChannelProvider>()?,
        None => actor.source_adapter,
    };
    let requirements = match contract_of(pool, lane_key).await? {
        Some(contract) if !contract.requirements.is_empty() => contract
            .requirements
            .into_iter()
            .map(|requirement| RequirementSpec {
                id: requirement.id,
                text: requirement.text,
                verifier: requirement.verifier,
            })
            .collect(),
        _ => adopt.requirements.clone(),
    };
    let body = format!("Adopt legacy lane {lane_key}: {title}");
    let work = SubmitWork {
        project_id: adopt.project_id.clone(),
        host_id: adopt.host_id.clone(),
        kind: None,
        instruction: format!(
            "Continue the work of legacy lane {lane_key} ({title}). Earlier progress may already exist in the working directory; check it first."
        ),
        title,
        requirements,
        cwd: Some(cwd),
        operations: Vec::new(),
        turn_limit: None,
    };
    let source = Source {
        adapter,
        message_id,
        body: &body,
    };
    admit_scoped(pool, &identity, actor, &source, &work).await
}

async fn admit_scoped(
    pool: &SqlitePool,
    identity: &SupervisorIdentity,
    actor: &ActorContext,
    source: &Source<'_>,
    work: &SubmitWork,
) -> Result<SubmitOutcome, AdmissionError> {
    let scope = match resolve_scope(pool, identity, actor, work).await? {
        Resolved::Scope(scope) => scope,
        Resolved::NeedsChoice { missing, detail } => {
            return Ok(SubmitOutcome::NeedsChoice {
                missing: missing.to_owned(),
                detail,
            })
        }
    };
    let mode = if scope.holds.is_empty() {
        GoalMode::Full
    } else {
        GoalMode::Supervised
    };
    let request = AdmitRequest {
        supervisor_id: identity.supervisor_id.clone(),
        owner_principal: identity.owner_principal.clone(),
        source_adapter: source.adapter,
        source_message_id: source.message_id.clone(),
        body: source.body.to_owned(),
        project_id: scope.project_id.clone(),
        channel_binding_id: actor.channel_binding_id.clone(),
        target: ExecutionTarget {
            kind: scope.kind,
            host_id: scope.host_id.clone(),
        },
        title: work.title.trim().to_owned(),
        cwd: scope.cwd.clone(),
        instruction: worker_instruction(work, source.body, &scope),
        requirements: work.requirements.clone(),
        mode,
        turn_limit: scope.turn_limit,
    };
    let admission = admit_request(pool, &request).await?;
    if admission.duplicate {
        return admitted(pool, admission.receipt, true, None).await;
    }
    if scope.holds.is_empty() {
        super::dispatcher::poke();
    } else {
        let detail = format!("{SCOPE_NEEDS_OWNER}: {}", scope.holds.join("; "));
        match transition_goal(
            pool,
            &admission.receipt.goal_id,
            admission.receipt.revision,
            RunState::Ready,
            RunState::AwaitingOwner,
            Some(&detail),
        )
        .await
        {
            Ok(_) => {}
            // The dispatcher refuses a supervised goal the same way and may have held it first.
            Err(SupervisorError::RevisionConflict { .. } | SupervisorError::StateConflict { .. }) => {}
            Err(error) => return Err(error.into()),
        }
    }
    admitted(pool, admission.receipt, false, Some(scope.grant)).await
}

/// Reads the committed goal back and builds the outcome and its acknowledgment from it.
async fn admitted(
    pool: &SqlitePool,
    receipt: AdmissionReceipt,
    duplicate: bool,
    grant: Option<Vec<OperationClass>>,
) -> Result<SubmitOutcome, AdmissionError> {
    let goal = goal_of(pool, &receipt.goal_id)
        .await?
        .ok_or_else(|| SupervisorError::NotFound {
            kind: "goal",
            id: receipt.goal_id.clone(),
        })?;
    let (mode, turn_limit, turns_used): (String, i64, i64) =
        sqlx::query_as("SELECT mode, turn_limit, turns_used FROM lane_goal WHERE lane_key = ?")
            .bind(&goal.lane_key)
            .fetch_one(pool)
            .await?;
    let mode = mode.parse::<GoalMode>()?;
    let turn_limit = u32::try_from(turn_limit).unwrap_or(0);
    let turns_remaining =
        turn_limit.saturating_sub(u32::try_from(turns_used).unwrap_or(u32::MAX));
    let acknowledgment = match goal.run_state {
        RunState::AwaitingOwner => format!(
            "Goal {} is registered (lane {}, revision {}) and waits for your decision: {}. Nothing has been sent to a worker.",
            goal.goal_id,
            goal.lane_key,
            goal.revision,
            goal.state_detail.as_deref().unwrap_or("its scope needs confirmation"),
        ),
        RunState::Ready => format!(
            "Goal {} is registered (lane {}, revision {}). Its first instruction {} is queued for the hub dispatcher, and it continues on its own for up to {} of {} turns.",
            goal.goal_id,
            goal.lane_key,
            goal.revision,
            receipt.dispatch_request_id,
            turns_remaining,
            turn_limit,
        ),
        state => format!(
            "Goal {} is registered (lane {}, revision {}) and is {state}; {} of {} turns remain.",
            goal.goal_id, goal.lane_key, goal.revision, turns_remaining, turn_limit,
        ),
    };
    let work = Box::new(AdmittedWork {
        receipt,
        duplicate,
        run_state: goal.run_state,
        state_detail: goal.state_detail,
        revision: goal.revision,
        mode,
        turn_limit,
        turns_remaining,
        grant,
        acknowledgment,
    });
    Ok(if work.run_state == RunState::AwaitingOwner {
        SubmitOutcome::AwaitingOwner(work)
    } else {
        SubmitOutcome::Admitted(work)
    })
}

/// Resolves project, host, worker kind, working directory, budget and grant. The owner's
/// choices and the channel binding are authentic; the model's proposal only selects among the
/// owner's own projects and hosts, and any widening becomes a hold reason.
async fn resolve_scope(
    pool: &SqlitePool,
    identity: &SupervisorIdentity,
    actor: &ActorContext,
    work: &SubmitWork,
) -> Result<Resolved, AdmissionError> {
    let choices = &actor.choices;
    let bound_project = match &actor.channel_binding_id {
        None => None,
        Some(binding_id) => {
            let row: Option<(String, String, Option<String>)> = sqlx::query_as(
                "SELECT supervisor_id, owner_principal, project_id
                 FROM supervisor_channel_bindings WHERE binding_id = ?",
            )
            .bind(binding_id)
            .fetch_optional(pool)
            .await?;
            match row {
                None => {
                    return Err(SupervisorError::NotFound {
                        kind: "channel binding",
                        id: binding_id.clone(),
                    }
                    .into())
                }
                Some((supervisor_id, owner, _))
                    if supervisor_id != identity.supervisor_id
                        || owner != identity.owner_principal =>
                {
                    return Err(AdmissionError::Unauthorized(format!(
                        "channel binding {binding_id} belongs to another owner"
                    )))
                }
                Some((_, _, project_id)) => project_id,
            }
        }
    };
    let authentic_project = choices.project_id.clone().or(bound_project);
    let project_id = match (authentic_project, work.project_id.as_deref()) {
        (Some(chosen), Some(proposed)) if proposed != chosen => {
            return needs_choice(
                "project",
                format!("the request names project {proposed}, but the owner's channel or choice is {chosen}"),
            )
        }
        (Some(chosen), _) => chosen,
        (None, Some(proposed)) => proposed.to_owned(),
        (None, None) => {
            let projects: Vec<String> = sqlx::query_scalar(
                "SELECT project_id FROM supervisor_projects WHERE supervisor_id = ?
                 ORDER BY created_at LIMIT 2",
            )
            .bind(&identity.supervisor_id)
            .fetch_all(pool)
            .await?;
            match projects.as_slice() {
                [only] => only.clone(),
                [] => return needs_choice("project", "the supervisor has no project yet".into()),
                _ => {
                    return needs_choice(
                        "project",
                        "several projects could be meant; pick one".into(),
                    )
                }
            }
        }
    };
    let project_owner: Option<String> =
        sqlx::query_scalar("SELECT supervisor_id FROM supervisor_projects WHERE project_id = ?")
            .bind(&project_id)
            .fetch_optional(pool)
            .await?;
    match project_owner {
        None => return needs_choice("project", format!("project {project_id} is unknown")),
        Some(owner) if owner != identity.supervisor_id => {
            return Err(AdmissionError::Unauthorized(format!(
                "project {project_id} belongs to another owner"
            )))
        }
        Some(_) => {}
    }

    let hosts: Vec<(String, String)> = sqlx::query_as(
        "SELECT host_id, git_common_dir FROM supervisor_project_hosts
         WHERE project_id = ? ORDER BY host_id",
    )
    .bind(&project_id)
    .fetch_all(pool)
    .await?;
    let host_id = match (choices.host_id.as_deref(), work.host_id.as_deref()) {
        (Some(chosen), Some(proposed)) if chosen != proposed => {
            return needs_choice(
                "host",
                format!("the request names host {proposed}, but the owner chose {chosen}"),
            )
        }
        (Some(host), _) | (None, Some(host)) => host.to_owned(),
        (None, None) => match hosts.as_slice() {
            [(only, _)] => only.clone(),
            [] => return needs_choice("host", format!("project {project_id} has no host")),
            _ => {
                return needs_choice(
                    "host",
                    format!("project {project_id} lives on {} hosts; pick one", hosts.len()),
                )
            }
        },
    };
    let Some((_, git_common_dir)) = hosts.iter().find(|(host, _)| *host == host_id) else {
        return needs_choice(
            "host",
            format!("project {project_id} is not mapped on host {host_id}"),
        );
    };
    let kind = match (choices.kind, work.kind) {
        (Some(chosen), Some(proposed)) if chosen != proposed => {
            return needs_choice(
                "worker kind",
                format!("the request asks for a {proposed} worker, but the owner chose {chosen}"),
            )
        }
        (Some(kind), _) | (None, Some(kind)) => kind,
        (None, None) => ExecutionKind::Pty,
    };

    let mut holds = Vec::new();
    let root = checkout_root(git_common_dir);
    let cwd = match (choices.cwd.as_deref(), work.cwd.as_deref()) {
        (Some(chosen), _) => chosen.to_owned(),
        (None, Some(proposed)) => {
            if !within(&root, proposed) {
                holds.push(format!(
                    "working directory {proposed} is outside the project checkout {root}"
                ));
            }
            proposed.to_owned()
        }
        (None, None) => root,
    };
    if cwd.split(['/', '\\']).any(|part| part == "..") {
        return Err(
            SupervisorError::Invalid("working directory must not contain '..'".into()).into(),
        );
    }

    let mut grant: BTreeSet<OperationClass> = OperationClass::BASELINE.iter().copied().collect();
    grant.extend(choices.operations.iter().copied());
    let beyond: BTreeSet<OperationClass> = work
        .operations
        .iter()
        .copied()
        .filter(|operation| !grant.contains(operation))
        .collect();
    for operation in beyond {
        holds.push(format!(
            "{} needs the owner's explicit approval",
            operation.as_str()
        ));
    }

    let granted_limit = choices.turn_limit.unwrap_or(DEFAULT_TURN_LIMIT);
    let turn_limit = work
        .turn_limit
        .map_or(granted_limit, |asked| asked.min(granted_limit));

    Ok(Resolved::Scope(Scope {
        project_id,
        host_id,
        kind,
        cwd,
        turn_limit,
        grant: grant.into_iter().collect(),
        holds,
    }))
}

/// The checkout a git common dir belongs to: `/x/repo/.git` is `/x/repo`.
fn checkout_root(git_common_dir: &str) -> String {
    let trimmed = git_common_dir.trim_end_matches(['/', '\\']);
    for suffix in ["/.git", "\\.git"] {
        if let Some(root) = trimmed.strip_suffix(suffix) {
            if !root.is_empty() {
                return root.to_owned();
            }
        }
    }
    trimmed.to_owned()
}

fn within(root: &str, path: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/') || rest.starts_with('\\'))
}

/// The first worker instruction: the proposal, the owner's verbatim request, the grant and the
/// contract. The grant line names what needs the owner, so a worker never reads permission
/// into the model's wording.
fn worker_instruction(work: &SubmitWork, source_body: &str, scope: &Scope) -> String {
    let allowed: Vec<&str> = scope.grant.iter().map(|operation| operation.as_str()).collect();
    let withheld: Vec<&str> = OperationClass::ALL
        .iter()
        .filter(|operation| !scope.grant.contains(operation))
        .map(|operation| operation.as_str())
        .collect();
    let mut text = format!(
        "{}\n\nOwner request (verbatim):\n{}\n\nScope: work in {} on host {}. Allowed operations: {}.",
        work.instruction.trim(),
        source_body.trim(),
        scope.cwd,
        scope.host_id,
        allowed.join(", "),
    );
    if !withheld.is_empty() {
        text.push_str(&format!(
            " Ask the owner before any of: {}.",
            withheld.join(", ")
        ));
    }
    text.push_str("\n\nDone when:");
    for requirement in &work.requirements {
        text.push_str(&format!(
            "\n- {}: {} (verified by: {})",
            requirement.id.trim(),
            requirement.text.trim(),
            requirement.verifier.trim()
        ));
    }
    text
}
