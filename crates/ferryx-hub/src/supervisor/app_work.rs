//! Typed work tools for the owner's app conversation (plan todo 6, finding H2).
//!
//! The app's supervisor conversation (channel `"app"`) has no Slack actor, so the Slack work
//! handler refuses every call from it. [`AppWorkTools`] serves that one scope without
//! widening authorization:
//!
//! - The actor is never taken from the model or the call. It is the installation's owner,
//!   resolved on every call ([`admission::installation_owner`]); no identity, or several, and
//!   the call is refused `UNAUTHORIZED`. The app conversation itself is reachable only after
//!   the hub authenticated the owner (the panel composer's owner gate).
//! - `submit` needs the owner message in flight: the caller registers it with
//!   [`AppTurnGuard::begin`] and passes [`AppTurnGuard::token`] as the turn of the
//!   conversation turn ([`converse`] does both). A call without it, or with the token of a
//!   turn that ended, is refused `SUPERVISOR_WORK_SCOPE`: there is no source text to admit and
//!   nothing runs.
//! - The session tools (list, read, answer, send, stop) need no message text; they are
//!   owner-only like the rest and refuse the token of a turn that ended.
//! - A scope other than the app's goes to the optional fallback handler (the Slack handler)
//!   and is refused without one. The app scope never reaches the fallback.
//!
//! This module does not depend on `hub-slack`, so the app path works with Slack compiled out.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use async_trait::async_trait;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::admission::{
    self, ActorContext, AdmissionError, OperationClass, OwnerChoices, SubmitOutcome, SubmitWork,
};
use super::panel_contract::SessionRef;
use super::{goal_of, identity_of, ChannelProvider, ExecutionKind};
use crate::goals::contract::RequirementSpec;
use crate::meta::mhc_fault::SupervisorFault;
use crate::meta::mhc_profile::ConversationScope;
use crate::meta::mhc_supervisor::{
    MhcSupervisor, SessionTools, WorkToolCall, WorkToolHandler, WorkToolRejection, WorkToolRequest,
    WorkerPreference,
};

/// The conversation channel of the app.
pub const APP_CHANNEL: &str = "app";

/// The conversation scope of every app turn: no project, channel `"app"`.
pub fn app_scope() -> ConversationScope {
    ConversationScope {
        project_id: None,
        channel_key: APP_CHANNEL.to_owned(),
    }
}

/// The model's `submit` call as an untrusted [`SubmitWork`] proposal. It declares only the
/// baseline operations a natural-language execution request carries; anything wider needs the
/// owner's explicit choice.
pub fn submit_from_call(
    title: &str,
    requirements: &[String],
    worker: WorkerPreference,
    source_body: &str,
) -> SubmitWork {
    let requirements: Vec<RequirementSpec> = requirements
        .iter()
        .enumerate()
        .map(|(index, text)| RequirementSpec {
            id: format!("r{}", index + 1),
            text: text.trim().to_owned(),
            verifier: format!("independent verification that: {}", text.trim()),
        })
        .collect();
    SubmitWork {
        project_id: None,
        host_id: None,
        kind: match worker {
            WorkerPreference::Mhc => Some(ExecutionKind::Mhc),
            WorkerPreference::Pty => Some(ExecutionKind::Pty),
            WorkerPreference::Any => None,
        },
        title: title.trim().to_owned(),
        instruction: format!("{}\n\n{}", title.trim(), source_body.trim()),
        requirements,
        cwd: None,
        operations: OperationClass::BASELINE.to_vec(),
        turn_limit: None,
    }
}

/// The project a chat-delegated goal belongs to, and the host it runs on when that project has
/// exactly one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectResolution {
    pub project_id: String,
    pub host_id: Option<String>,
}

/// Resolves the project of an app submit from the owner's own turn, before admission:
///
/// 1. the project whose checkout contains the cwd of the first session the owner addressed with
///    `@`;
/// 2. else, when the owner's previous app submit ended in a project `needs_choice`
///    (`asked_for_project`), the single project the owner's message alone names: the answer to
///    the supervisor's own question. The proposal title gets no say in it, so a title the model
///    derived from an earlier turn cannot turn a one-name answer into a refusal;
/// 3. else the single project whose name or repository identity basename the owner message and
///    the proposal title name as a whole word, case-insensitively.
///
/// Several matches are refused (`None`), never guessed. `host_id` is the project's host when it
/// has exactly one. `None` leaves the project to admission, which then asks the owner and lists
/// the project names.
pub async fn resolve_project(
    pool: &SqlitePool,
    supervisor_id: &str,
    owner_text: &str,
    title: &str,
    mentions: &[SessionRef],
    sessions: Option<&SessionTools>,
    asked_for_project: bool,
) -> Result<Option<ProjectResolution>, AdmissionError> {
    let answering = if asked_for_project {
        project_of_names(pool, supervisor_id, owner_text, "").await?
    } else {
        None
    };
    let mentioned = project_of_mentions(pool, supervisor_id, mentions, sessions).await;
    let project_id = match (mentioned, answering) {
        (Some(project_id), _) => project_id,
        (None, Some(project_id)) => project_id,
        (None, None) => match project_of_names(pool, supervisor_id, owner_text, title).await? {
            Some(project_id) => project_id,
            None => return Ok(None),
        },
    };
    Ok(Some(ProjectResolution {
        host_id: single_host(pool, &project_id).await?,
        project_id,
    }))
}

/// The project of the first session the owner addressed with @: the one whose checkout on that
/// session's host contains the session's cwd. `None` when that session is unknown to the fleet,
/// the fleet cannot be listed, or the cwd belongs to no project or to several.
async fn project_of_mentions(
    pool: &SqlitePool,
    supervisor_id: &str,
    mentions: &[SessionRef],
    sessions: Option<&SessionTools>,
) -> Option<String> {
    let sessions = sessions?;
    let first = mentions.first()?;
    let fleet = match sessions.fleet().await {
        Ok(fleet) => fleet,
        Err(error) => {
            tracing::warn!(
                %error,
                session = %first,
                "the fleet was not listed; a mentioned session names no project"
            );
            return None;
        }
    };
    let session = fleet.iter().find(|session| session.session_ref == *first)?;
    let hosts: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT h.project_id, h.host_id, h.git_common_dir
           FROM supervisor_project_hosts h
           JOIN supervisor_projects p ON p.project_id = h.project_id
          WHERE p.supervisor_id = ?",
    )
    .bind(supervisor_id)
    .fetch_all(pool)
    .await
    .ok()?;
    let mut matched: Vec<String> = hosts
        .into_iter()
        .filter(|(_, host_id, git_common_dir)| {
            host_id == &session.host_id
                && admission::within(
                    &admission::checkout_root(git_common_dir),
                    session.cwd.trim_end_matches(['/', '\\']),
                )
        })
        .map(|(project_id, _, _)| project_id)
        .collect();
    matched.sort();
    matched.dedup();
    match matched.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    }
}

/// The single project the owner's message and the proposal title name: its name or its
/// repository identity basename, matched case-insensitively as a whole word. Several matches
/// are refused.
async fn project_of_names(
    pool: &SqlitePool,
    supervisor_id: &str,
    owner_text: &str,
    title: &str,
) -> Result<Option<String>, AdmissionError> {
    let projects: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT project_id, name, repo_identity FROM supervisor_projects
          WHERE supervisor_id = ? ORDER BY project_id",
    )
    .bind(supervisor_id)
    .fetch_all(pool)
    .await?;
    let named = format!("{owner_text}\n{title}");
    let mut matched: Vec<String> = projects
        .into_iter()
        .filter(|(_, name, repo_identity)| {
            names_word(name, &named) || names_word(repo_basename(repo_identity), &named)
        })
        .map(|(project_id, _, _)| project_id)
        .collect();
    matched.sort();
    matched.dedup();
    Ok(match matched.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    })
}

/// The host of a project when it has exactly one; `None` for none or several.
async fn single_host(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<Option<String>, AdmissionError> {
    let hosts: Vec<String> = sqlx::query_scalar(
        "SELECT host_id FROM supervisor_project_hosts WHERE project_id = ? ORDER BY host_id LIMIT 2",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    Ok(match hosts.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    })
}

/// The last path segment of a repository identity, without a `.git` suffix:
/// `github.com/example/ferryx` is `ferryx`.
fn repo_basename(repo_identity: &str) -> &str {
    let trimmed = repo_identity.trim_end_matches(['/', '\\']);
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed)
}

/// A character of a name's own word: ASCII letters, digits and `_`. Anything else (a space, a
/// Korean particle, a hyphen) ends a word, so `ferryx에서` names `ferryx`.
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether `needle` occurs in `haystack` as a whole word, case-insensitively.
fn names_word(needle: &str, haystack: &str) -> bool {
    let needle: Vec<char> = needle.to_lowercase().chars().collect();
    if needle.is_empty() {
        return false;
    }
    let haystack: Vec<char> = haystack.to_lowercase().chars().collect();
    if needle.len() > haystack.len() {
        return false;
    }
    for start in 0..=haystack.len() - needle.len() {
        if haystack[start..start + needle.len()] != needle[..] {
            continue;
        }
        let before = start.checked_sub(1).map(|at| haystack[at]);
        let after = haystack.get(start + needle.len()).copied();
        if !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char) {
            return true;
        }
    }
    false
}

/// An app turn in flight: the owner's message, the sessions it addressed and what its submits
/// did.
struct AppTurn {
    request_id: String,
    text: String,
    mentions: Vec<SessionRef>,
    outcomes: Vec<SubmitOutcome>,
}

#[derive(Default)]
struct Registry {
    next: u64,
    live: HashMap<u64, AppTurn>,
}

fn registry() -> MutexGuard<'static, Registry> {
    static TURNS: OnceLock<Mutex<Registry>> = OnceLock::new();
    TURNS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Registers one owner message of the app conversation for the turn that answers it; dropped
/// with the turn. Turns are kept apart by token, so a turn queued behind another never
/// displaces it.
pub struct AppTurnGuard {
    generation: u64,
}

impl AppTurnGuard {
    /// `request_id` identifies the owner's message (it is the admission source id) and
    /// `owner_text` is what the owner wrote, verbatim.
    pub fn begin(request_id: &str, owner_text: &str) -> Self {
        Self::begin_with_mentions(request_id, owner_text, Vec::new())
    }

    /// Like [`Self::begin`], with the sessions the owner addressed with @ in this message.
    pub fn begin_with_mentions(
        request_id: &str,
        owner_text: &str,
        mentions: Vec<SessionRef>,
    ) -> Self {
        let mut registry = registry();
        registry.next += 1;
        let generation = registry.next;
        registry.live.insert(
            generation,
            AppTurn {
                request_id: request_id.to_owned(),
                text: owner_text.to_owned(),
                mentions,
                outcomes: Vec::new(),
            },
        );
        Self { generation }
    }

    /// The turn token to pass to `MhcSupervisor::converse_in_turn`.
    pub fn token(&self) -> u64 {
        self.generation
    }

    /// Outcomes of the submits admission answered during this turn.
    pub fn outcomes(&self) -> Vec<SubmitOutcome> {
        registry()
            .live
            .get(&self.generation)
            .map(|turn| turn.outcomes.clone())
            .unwrap_or_default()
    }
}

impl Drop for AppTurnGuard {
    fn drop(&mut self) {
        registry().live.remove(&self.generation);
    }
}

/// One turn of the app conversation with its owner message registered, so the typed work
/// tools the turn calls can act for the owner. `request_id` is the app request's id and
/// `owner_text` the owner's own words; `message` is what the engine receives.
pub async fn converse(
    supervisor: &MhcSupervisor,
    request_id: &str,
    owner_text: &str,
    message: &str,
) -> Result<String, SupervisorFault> {
    let turn = AppTurnGuard::begin_with_mentions(
        request_id,
        owner_text,
        super::transcript::addressed_sessions(message),
    );
    supervisor
        .converse_in_turn(&app_scope(), Some(turn.token()), message)
        .await
}

/// Whether `turn` may act: no token (an untracked caller) or the token of a live app turn.
fn turn_current(turn: Option<u64>) -> bool {
    match turn {
        None => true,
        Some(generation) => registry().live.contains_key(&generation),
    }
}

fn no_turn() -> WorkToolRejection {
    WorkToolRejection::new(
        "SUPERVISOR_WORK_SCOPE",
        "no authenticated owner message is in flight for this conversation turn",
    )
}

fn refused(error: &admission::AdmissionError) -> WorkToolRejection {
    WorkToolRejection::new(error.code(), &error.to_string())
}

/// The hub-meta key prefix of the marker that says the owner's last app submit ended in a project
/// `needs_choice`: the state rule 3 reads so that one project name answers the supervisor's own
/// question. The value is the admission source id of the turn that asked, so the answer stays
/// traceable to the question.
const PROJECT_CHOICE_PREFIX: &str = "supervisor_app_project_choice:";

/// Records that the turn `request_id` ended in a project `needs_choice`, or (`None`) that it did
/// not, which clears an earlier question. Every submit writes it, so the next submit of the
/// conversation reads only its own predecessor's question. The key is per owner, so one owner's
/// question never decides another's answer.
async fn record_project_choice(
    pool: &SqlitePool,
    owner: &str,
    request_id: Option<&str>,
) -> Result<(), AdmissionError> {
    let key = format!("{PROJECT_CHOICE_PREFIX}{owner}");
    match request_id {
        Some(request_id) => {
            sqlx::query(
                "INSERT INTO hub_meta(key, value) VALUES(?, ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            )
            .bind(&key)
            .bind(request_id)
            .execute(pool)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM hub_meta WHERE key = ?")
                .bind(&key)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// Whether the owner's last app submit ended in a project `needs_choice`; the admission source id
/// of that turn when it did. A missing row (no such submit, or one that named a project) answers
/// `None`. The marker only lets rule 3 apply, which matches the owner's own message more strictly
/// than rule 2, so it never widens a guess.
async fn project_question(
    pool: &SqlitePool,
    owner: &str,
) -> Result<Option<String>, AdmissionError> {
    let asked: Option<String> = sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?")
        .bind(format!("{PROJECT_CHOICE_PREFIX}{owner}"))
        .fetch_optional(pool)
        .await?;
    Ok(asked)
}

/// The submit with the project and host the owner's turn resolves to, unless the proposal or the
/// owner's choices already named them. A turn whose owner has no supervisor identity resolves
/// nothing and leaves the scope to admission.
async fn resolve_for_turn(
    pool: &SqlitePool,
    owner: &str,
    actor: &ActorContext,
    work: &SubmitWork,
    turn: Option<u64>,
    sessions: Option<&SessionTools>,
) -> Result<SubmitWork, AdmissionError> {
    let mut work = work.clone();
    let chosen_project = work
        .project_id
        .clone()
        .or_else(|| actor.choices.project_id.clone());
    if work.host_id.is_some() && chosen_project.is_some() {
        return Ok(work);
    }
    let Some(identity) = identity_of(pool, owner).await? else {
        return Ok(work);
    };
    // A project the request or the owner already named keeps its own host: a name match could
    // otherwise attribute it to a different project.
    let resolved = match chosen_project {
        Some(project_id) => ProjectResolution {
            host_id: single_host(pool, &project_id).await?,
            project_id,
        },
        None => {
            let mentions: Vec<SessionRef> = match turn {
                None => Vec::new(),
                Some(generation) => registry()
                    .live
                    .get(&generation)
                    .map(|live| live.mentions.clone())
                    .unwrap_or_default(),
            };
            let asked = project_question(pool, owner).await?.is_some();
            let Some(resolved) = resolve_project(
                pool,
                &identity.supervisor_id,
                &actor.source_body,
                &work.title,
                &mentions,
                sessions,
                asked,
            )
            .await?
            else {
                return Ok(work);
            };
            resolved
        }
    };
    work.project_id.get_or_insert(resolved.project_id);
    if let Some(host_id) = resolved.host_id {
        work.host_id.get_or_insert(host_id);
    }
    Ok(work)
}

/// The work tools of the app conversation, with an optional handler for every other scope.
pub struct AppWorkTools {
    pool: SqlitePool,
    sessions: Option<Arc<SessionTools>>,
    fallback: Option<Arc<dyn WorkToolHandler>>,
}

impl AppWorkTools {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            sessions: None,
            fallback: None,
        }
    }

    /// Serves the session tools (list, read, answer, send, stop) too.
    pub fn with_sessions(mut self, sessions: Arc<SessionTools>) -> Self {
        self.sessions = Some(sessions);
        self
    }

    /// Answers requests of every scope but the app's (the Slack handler).
    pub fn with_fallback(mut self, fallback: Arc<dyn WorkToolHandler>) -> Self {
        self.fallback = Some(fallback);
        self
    }
}

#[async_trait]
impl WorkToolHandler for AppWorkTools {
    async fn handle(&self, request: WorkToolRequest) -> Result<Value, WorkToolRejection> {
        if request.scope != app_scope() {
            return match &self.fallback {
                Some(other) => other.handle(request).await,
                None => Err(no_turn()),
            };
        }
        let owner = admission::installation_owner(&self.pool)
            .await
            .map_err(|error| refused(&error))?;
        let turn = request.turn;
        match request.call {
            WorkToolCall::Submit {
                title,
                requirements,
                worker,
            } => {
                let actor = turn
                    .and_then(|generation| {
                        registry().live.get(&generation).map(|live| ActorContext {
                            principal: owner.clone(),
                            source_adapter: ChannelProvider::Desktop,
                            source_message_id: format!("app:{}", live.request_id),
                            source_body: live.text.clone(),
                            channel_binding_id: None,
                            choices: OwnerChoices::default(),
                        })
                    })
                    .ok_or_else(no_turn)?;
                let work = submit_from_call(&title, &requirements, worker, &actor.source_body);
                let work = match resolve_for_turn(
                    &self.pool,
                    &owner,
                    &actor,
                    &work,
                    turn,
                    self.sessions.as_deref(),
                )
                .await
                {
                    Ok(work) => work,
                    Err(error) => return Err(refused(&error)),
                };
                let outcome = admission::handle_message(&self.pool, &actor, Some(&work)).await;
                // The question the next app turn answers. A refused submit is not a question, so
                // it clears any earlier one instead. The outcome itself is already persisted, so
                // a failure here is logged rather than turned into a failed submit.
                let asked = match &outcome {
                    Ok(SubmitOutcome::NeedsChoice { missing, .. }) if missing == "project" => {
                        Some(actor.source_message_id.as_str())
                    }
                    _ => None,
                };
                if let Err(error) = record_project_choice(&self.pool, &owner, asked).await {
                    tracing::warn!(
                        %error,
                        owner = %owner,
                        "the project question was not recorded"
                    );
                }
                let outcome = outcome.map_err(|error| refused(&error))?;
                if let Some(generation) = turn {
                    if let Some(live) = registry().live.get_mut(&generation) {
                        live.outcomes.push(outcome.clone());
                    }
                }
                serde_json::to_value(&outcome).map_err(|error| {
                    WorkToolRejection::new("SUPERVISOR_WORK_INVALID", &error.to_string())
                })
            }
            WorkToolCall::Status { goal_id: None } => {
                // This turn's own outcomes: not a statement about earlier work.
                let outcomes: Vec<SubmitOutcome> = match turn {
                    None => Vec::new(),
                    Some(generation) => registry()
                        .live
                        .get(&generation)
                        .map(|live| live.outcomes.clone())
                        .ok_or_else(no_turn)?,
                };
                Ok(json!({ "outcomes": outcomes }))
            }
            WorkToolCall::Status {
                goal_id: Some(goal_id),
            } => {
                if !turn_current(turn) {
                    return Err(no_turn());
                }
                let goal = goal_of(&self.pool, &goal_id)
                    .await
                    .map_err(|error| WorkToolRejection::new(error.code(), &error.to_string()))?;
                match goal {
                    Some(goal) if goal.owner_principal == owner => serde_json::to_value(&goal)
                        .map_err(|error| {
                            WorkToolRejection::new("SUPERVISOR_WORK_INVALID", &error.to_string())
                        }),
                    _ => Err(WorkToolRejection::new(
                        "NOT_FOUND",
                        "no such goal for this owner",
                    )),
                }
            }
            call => {
                if !turn_current(turn) {
                    return Err(no_turn());
                }
                match &self.sessions {
                    Some(sessions) => sessions.execute(call).await,
                    None => Err(WorkToolRejection::new(
                        "SUPERVISOR_SESSIONS_UNAVAILABLE",
                        "the session tools are not installed",
                    )),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::error::Result as HubResult;
    use crate::meta::mhc_supervisor::FleetSource;
    use crate::supervisor::panel_contract::{
        FleetSession, PendingPrompt, SessionControl, SessionRef,
    };
    use crate::supervisor::{bind_project_host, create_project, ensure_identity};

    const OWNER: &str = "desktop:owner";
    const WORK: &str = "Create result.txt containing READY. Run the verifier.";

    /// A test gate that allows exactly the intended answer (option 0 of the fixture prompt).
    struct AllowFirstOption;

    #[async_trait]
    impl crate::meta::mhc_supervisor::AnswerGate for AllowFirstOption {
        async fn allow(
            &self,
            _session: &crate::supervisor::panel_contract::FleetSession,
            _prompt: &crate::supervisor::panel_contract::PendingPrompt,
            option_index: usize,
        ) -> std::result::Result<(), String> {
            (option_index == 0)
                .then_some(())
                .ok_or_else(|| "only option 0 is allowed here".to_owned())
        }
    }

    async fn owner_pool() -> SqlitePool {
        let pool = crate::storage::memory().await.unwrap();
        let identity = ensure_identity(&pool, OWNER).await.unwrap();
        let project = create_project(
            &pool,
            &identity.supervisor_id,
            "ferryx",
            "github.com/example/ferryx",
        )
        .await
        .unwrap();
        bind_project_host(
            &pool,
            &project.project_id,
            "host-a",
            "/home/owner/code/ferryx/.git",
        )
        .await
        .unwrap();
        pool
    }

    async fn goals(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM supervisor_goals")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[derive(Default)]
    struct FakeControl {
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl SessionControl for FakeControl {
        async fn stop(&self, session: &SessionRef) -> HubResult<()> {
            self.calls.lock().unwrap().push(format!("stop {session}"));
            Ok(())
        }

        async fn send(&self, session: &SessionRef, text: &str) -> HubResult<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("send {session} {text}"));
            Ok(())
        }

        async fn answer(
            &self,
            session: &SessionRef,
            prompt_id: &str,
            screen_revision: Option<&str>,
            option_index: usize,
        ) -> HubResult<Option<String>> {
            self.calls.lock().unwrap().push(format!(
                "answer {session} {prompt_id} {screen_revision:?} {option_index}"
            ));
            Ok(None)
        }

        async fn read_tail(&self, session: &SessionRef, lines: usize) -> HubResult<String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("read {session} {lines}"));
            Ok("tail".to_owned())
        }
    }

    struct FakeFleet(Vec<FleetSession>);

    #[async_trait]
    impl FleetSource for FakeFleet {
        async fn list(&self) -> HubResult<Vec<FleetSession>> {
            Ok(self.0.clone())
        }
    }

    fn fleet_session() -> FleetSession {
        FleetSession {
            session_ref: SessionRef::new("mac", "s1"),
            host_id: "mac".into(),
            host_label: "This Mac".into(),
            title: "task".into(),
            cwd: "/work".into(),
            agent: "claude".into(),
            state: "blocked".into(),
            state_source: "provider".into(),
            state_since_ms: None,
            screen_changed_at_ms: None,
            pending_prompt: Some(PendingPrompt {
                prompt_id: "p-s1".into(),
                kind: "approval".into(),
                question: "Run the unit tests?".into(),
                command: None,
                options: vec!["Allow".into(), "Deny".into()],
                screen_revision: Some("r1".into()),
            }),
            last_assistant_line: None,
            updated_at_ms: 1,
            stale: false,
            health: None,
            attention: None,
            reason: None,
            branch: None,
        }
    }

    fn with_sessions(pool: &SqlitePool) -> (Arc<FakeControl>, AppWorkTools, Vec<FleetSession>) {
        let fleet = vec![fleet_session()];
        let control = Arc::new(FakeControl::default());
        let sessions = SessionTools::new(control.clone(), Arc::new(FakeFleet(fleet.clone())))
            .with_gate(Arc::new(AllowFirstOption));
        (
            control,
            AppWorkTools::new(pool.clone()).with_sessions(Arc::new(sessions)),
            fleet,
        )
    }

    fn request(scope: ConversationScope, turn: Option<u64>, call: WorkToolCall) -> WorkToolRequest {
        WorkToolRequest {
            request_id: "req".into(),
            scope,
            turn,
            call,
        }
    }

    fn app(turn: Option<u64>, call: WorkToolCall) -> WorkToolRequest {
        request(app_scope(), turn, call)
    }

    fn submit() -> WorkToolCall {
        WorkToolCall::Submit {
            title: "Write result.txt".into(),
            requirements: vec!["result.txt contains READY".into()],
            worker: WorkerPreference::Any,
        }
    }

    fn session_calls() -> Vec<WorkToolCall> {
        let session_ref = SessionRef::new("mac", "s1");
        vec![
            WorkToolCall::ListSessions,
            WorkToolCall::ReadSession {
                session_ref: session_ref.clone(),
                lines: 5,
            },
            WorkToolCall::AnswerPrompt {
                session_ref: session_ref.clone(),
                prompt_id: "p-s1".into(),
                screen_revision: Some("r1".into()),
                option_index: 0,
            },
            WorkToolCall::SendToSession {
                session_ref: session_ref.clone(),
                text: "continue".into(),
            },
            WorkToolCall::StopSession { session_ref },
        ]
    }

    #[tokio::test]
    async fn an_app_submit_and_status_reach_admission_for_the_installation_owner() {
        let pool = owner_pool().await;
        let tools = AppWorkTools::new(pool.clone());
        let turn = AppTurnGuard::begin("req-1", WORK);

        let status = tools
            .handle(app(
                Some(turn.token()),
                WorkToolCall::Status { goal_id: None },
            ))
            .await
            .unwrap();
        assert_eq!(status, json!({ "outcomes": [] }));

        let submitted = tools
            .handle(app(Some(turn.token()), submit()))
            .await
            .unwrap();
        let goal_id = submitted["receipt"]["goal_id"].as_str().unwrap().to_owned();
        assert_eq!(goals(&pool).await, 1);
        assert_eq!(turn.outcomes().len(), 1);
        let owned: String =
            sqlx::query_scalar("SELECT owner_principal FROM supervisor_goals WHERE goal_id = ?")
                .bind(&goal_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(owned, OWNER);

        let status = tools
            .handle(app(
                Some(turn.token()),
                WorkToolCall::Status { goal_id: None },
            ))
            .await
            .unwrap();
        assert_eq!(status["outcomes"].as_array().unwrap().len(), 1);
        let goal = tools
            .handle(app(
                Some(turn.token()),
                WorkToolCall::Status {
                    goal_id: Some(goal_id),
                },
            ))
            .await
            .unwrap();
        assert_eq!(goal["owner_principal"], OWNER);
        let unknown = tools
            .handle(app(
                Some(turn.token()),
                WorkToolCall::Status {
                    goal_id: Some("00000000-0000-4000-8000-000000000000".into()),
                },
            ))
            .await
            .unwrap_err();
        assert_eq!(unknown.code, "NOT_FOUND");
    }

    #[tokio::test]
    async fn the_five_session_tools_reach_session_control_from_an_app_turn() {
        let pool = owner_pool().await;
        let (control, tools, fleet) = with_sessions(&pool);
        let turn = AppTurnGuard::begin("req-2", "check the sessions");

        for token in [Some(turn.token()), None] {
            let mut calls = session_calls().into_iter();
            let listed = tools
                .handle(app(token, calls.next().unwrap()))
                .await
                .unwrap();
            assert_eq!(listed, json!({ "sessions": fleet }));
            for call in calls {
                tools.handle(app(token, call)).await.unwrap();
            }
        }
        let once = vec![
            "read mac:s1 5".to_owned(),
            r#"answer mac:s1 p-s1 Some("r1") 0"#.to_owned(),
            "send mac:s1 continue".to_owned(),
            "stop mac:s1".to_owned(),
        ];
        assert_eq!(
            *control.calls.lock().unwrap(),
            [once.clone(), once].concat(),
            "list, read, answer, send and stop all reached session control"
        );
    }

    #[tokio::test]
    async fn an_app_call_without_an_owner_message_or_a_live_turn_is_refused() {
        let pool = owner_pool().await;
        let (control, tools, _) = with_sessions(&pool);
        let ended = AppTurnGuard::begin("req-3", WORK).token();
        let live = AppTurnGuard::begin("req-4", WORK);

        // Submit needs the owner message in flight: no token, an ended turn's token and an
        // unknown token all find nothing to admit.
        for token in [None, Some(ended), Some(live.token() + 1_000_000)] {
            let refused = tools.handle(app(token, submit())).await.unwrap_err();
            assert_eq!(refused.code, "SUPERVISOR_WORK_SCOPE");
        }
        assert_eq!(goals(&pool).await, 0);
        // A session call from a turn that ended never reaches session control.
        for call in session_calls() {
            let refused = tools.handle(app(Some(ended), call)).await.unwrap_err();
            assert_eq!(refused.code, "SUPERVISOR_WORK_SCOPE");
        }
        let refused = tools
            .handle(app(Some(ended), WorkToolCall::Status { goal_id: None }))
            .await
            .unwrap_err();
        assert_eq!(refused.code, "SUPERVISOR_WORK_SCOPE");
        assert!(control.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_scope_that_is_not_the_apps_is_refused_without_a_fallback() {
        let pool = owner_pool().await;
        let (control, tools, _) = with_sessions(&pool);
        let turn = AppTurnGuard::begin("req-5", WORK);
        for scope in [
            ConversationScope {
                project_id: None,
                channel_key: "slack:T1:UBOT:D1:".into(),
            },
            ConversationScope {
                project_id: Some("p-1".into()),
                channel_key: APP_CHANNEL.into(),
            },
        ] {
            for call in session_calls().into_iter().chain([submit()]) {
                let refused = tools
                    .handle(request(scope.clone(), Some(turn.token()), call))
                    .await
                    .unwrap_err();
                assert_eq!(refused.code, "SUPERVISOR_WORK_SCOPE");
            }
        }
        assert!(control.calls.lock().unwrap().is_empty());
        assert_eq!(goals(&pool).await, 0);
    }

    #[tokio::test]
    async fn a_non_owner_installation_is_refused_for_every_app_call() {
        // No supervisor identity: nobody is the owner.
        let pool = crate::storage::memory().await.unwrap();
        let (control, tools, _) = with_sessions(&pool);
        let turn = AppTurnGuard::begin("req-6", WORK);
        for call in session_calls().into_iter().chain([submit()]) {
            let refused = tools
                .handle(app(Some(turn.token()), call))
                .await
                .unwrap_err();
            assert_eq!(refused.code, "UNAUTHORIZED");
        }
        // Several identities: the caller cannot be told apart, so nobody is.
        ensure_identity(&pool, "desktop:one").await.unwrap();
        ensure_identity(&pool, "desktop:two").await.unwrap();
        for call in session_calls().into_iter().chain([submit()]) {
            let refused = tools
                .handle(app(Some(turn.token()), call))
                .await
                .unwrap_err();
            assert_eq!(refused.code, "UNAUTHORIZED");
        }
        assert!(control.calls.lock().unwrap().is_empty());
        assert_eq!(goals(&pool).await, 0);
    }

    #[tokio::test]
    async fn without_session_tools_the_app_gets_a_typed_refusal() {
        let pool = owner_pool().await;
        let tools = AppWorkTools::new(pool);
        let turn = AppTurnGuard::begin("req-7", WORK);
        let refused = tools
            .handle(app(Some(turn.token()), WorkToolCall::ListSessions))
            .await
            .unwrap_err();
        assert_eq!(refused.code, "SUPERVISOR_SESSIONS_UNAVAILABLE");
    }

    /// The owner text plus the trailer `converse` registers mentions from round-trips into the
    /// addressed sessions; text without the trailer addresses none.
    #[test]
    fn addressed_sessions_read_back_the_trailer_turn_message_adds() {
        let refs = vec![SessionRef::new("host-a", "pty-1")];
        let message = crate::supervisor::transcript::turn_message("do it", &refs);
        assert_eq!(
            crate::supervisor::transcript::addressed_sessions(&message),
            refs
        );
        assert!(crate::supervisor::transcript::addressed_sessions("do it").is_empty());
    }

    #[test]
    fn a_name_matches_only_as_a_whole_word_and_a_repo_basename_drops_its_git_suffix() {
        assert!(names_word("ferryx", "ferryx에서 README 오타 고쳐줘"));
        assert!(names_word("ferryx", "FERRYX: fix the typo"));
        assert!(names_word("ferryx", "fix ferryx, please"));
        assert!(!names_word("ferryx", "fix ferryxhub"));
        assert!(!names_word("app", "the happy path"));
        assert!(!names_word("", "anything"));
        assert_eq!(repo_basename("github.com/example/ferryx"), "ferryx");
        assert_eq!(repo_basename("github.com/example/ferryx.git"), "ferryx");
        assert_eq!(repo_basename("github.com/example/ferryx/"), "ferryx");
        assert_eq!(repo_basename("ferryx"), "ferryx");
    }

    /// Three projects on two hosts, so a rule that guesses shows up as a wrong goal.
    async fn many_project_pool() -> SqlitePool {
        let pool = crate::storage::memory().await.unwrap();
        let identity = ensure_identity(&pool, OWNER).await.unwrap();
        for (name, repo) in [
            ("ferryx", "github.com/example/ferryx"),
            ("other", "github.com/example/other"),
            ("pane", "github.com/example/pane"),
        ] {
            let project = create_project(&pool, &identity.supervisor_id, name, repo)
                .await
                .unwrap();
            bind_project_host(
                &pool,
                &project.project_id,
                "host-a",
                &format!("/home/owner/code/{name}/.git"),
            )
            .await
            .unwrap();
        }
        pool
    }

    fn project_id_of<'a>(projects: &'a [(String, String)], name: &str) -> &'a str {
        &projects
            .iter()
            .find(|(_, project_name)| project_name == name)
            .unwrap()
            .0
    }

    async fn projects(pool: &SqlitePool) -> Vec<(String, String)> {
        sqlx::query_as("SELECT project_id, name FROM supervisor_projects ORDER BY name")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    async fn goal_target(pool: &SqlitePool) -> (String, String) {
        sqlx::query_as(
            "SELECT g.project_id, b.host_id FROM supervisor_goals g
               JOIN supervisor_execution_bindings b ON b.goal_id = g.goal_id",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    fn fleet_with(session_ref: SessionRef, host_id: &str, cwd: &str) -> Vec<FleetSession> {
        let mut session = fleet_session();
        session.session_ref = session_ref;
        session.host_id = host_id.to_owned();
        session.cwd = cwd.to_owned();
        vec![session]
    }

    fn tools_with_fleet(pool: &SqlitePool, fleet: Vec<FleetSession>) -> AppWorkTools {
        let sessions =
            SessionTools::new(Arc::new(FakeControl::default()), Arc::new(FakeFleet(fleet)));
        AppWorkTools::new(pool.clone()).with_sessions(Arc::new(sessions))
    }

    /// Rule 1: the project of the first @mentioned session's cwd on that session's host, and its
    /// single host — even when the owner's message names no project at all.
    #[tokio::test]
    async fn a_mentioned_sessions_cwd_names_the_project_and_its_host() {
        let pool = many_project_pool().await;
        let projects = projects(&pool).await;
        let mention = SessionRef::new("host-a", "pty-1");
        let tools = tools_with_fleet(
            &pool,
            fleet_with(mention.clone(), "host-a", "/home/owner/code/other/src/deep"),
        );
        let turn = AppTurnGuard::begin_with_mentions("req-m1", WORK, vec![mention]);

        tools
            .handle(app(Some(turn.token()), submit()))
            .await
            .unwrap();

        let (project_id, host_id) = goal_target(&pool).await;
        assert_eq!(project_id, project_id_of(&projects, "other"));
        assert_eq!(host_id, "host-a");
    }

    /// Rule 1 wins over rule 2: the mentioned session's cwd decides even when the owner's text
    /// names another project.
    #[tokio::test]
    async fn a_mentioned_session_outranks_a_project_name_in_the_message() {
        let pool = many_project_pool().await;
        let projects = projects(&pool).await;
        let mention = SessionRef::new("host-a", "pty-1");
        let tools = tools_with_fleet(
            &pool,
            fleet_with(mention.clone(), "host-a", "/home/owner/code/pane"),
        );
        let turn = AppTurnGuard::begin_with_mentions("req-m2", WORK, vec![mention]);

        tools
            .handle(app(Some(turn.token()), submit()))
            .await
            .unwrap();

        let (project_id, _) = goal_target(&pool).await;
        assert_eq!(project_id, project_id_of(&projects, "pane"));
    }

    /// Rule 2: a unique project name in the owner's message, whole-word and case-insensitive,
    /// as in the plan's Korean example.
    #[tokio::test]
    async fn a_unique_project_name_in_the_owner_message_decides_the_project() {
        let pool = many_project_pool().await;
        let projects = projects(&pool).await;
        let tools = AppWorkTools::new(pool.clone());
        let turn = AppTurnGuard::begin("req-n1", "FERRYX에서 README 오타 고쳐줘");

        tools
            .handle(app(Some(turn.token()), submit()))
            .await
            .unwrap();

        let (project_id, host_id) = goal_target(&pool).await;
        assert_eq!(project_id, project_id_of(&projects, "ferryx"));
        assert_eq!(host_id, "host-a");
    }

    /// The proposal title participates in the match, not only the owner's message.
    #[tokio::test]
    async fn the_proposal_title_also_names_the_project() {
        let pool = many_project_pool().await;
        let projects = projects(&pool).await;
        let tools = AppWorkTools::new(pool.clone());
        let turn = AppTurnGuard::begin("req-n2", WORK);
        let titled = WorkToolCall::Submit {
            title: "Fix the typo in pane's README".into(),
            requirements: vec!["the typo is fixed".into()],
            worker: WorkerPreference::Any,
        };

        tools.handle(app(Some(turn.token()), titled)).await.unwrap();

        let (project_id, _) = goal_target(&pool).await;
        assert_eq!(project_id, project_id_of(&projects, "pane"));
    }

    /// Two projects named in one message are refused, never guessed: nothing is persisted and
    /// admission holds the goal for the owner to pick.
    #[tokio::test]
    async fn two_matching_projects_are_refused_and_the_owner_picks() {
        let pool = many_project_pool().await;
        let tools = AppWorkTools::new(pool.clone());
        let turn = AppTurnGuard::begin("req-n3", "fix ferryx and pane");

        let outcome = tools
            .handle(app(Some(turn.token()), submit()))
            .await
            .unwrap();

        assert_eq!(outcome["outcome"], "needs_choice");
        assert_eq!(outcome["missing"], "project");
        assert_eq!(goals(&pool).await, 0);
    }

    /// A project name that only occurs inside a longer word names nothing: with several
    /// projects and no other hint, the owner still has to pick.
    #[tokio::test]
    async fn a_name_inside_another_word_names_no_project() {
        let pool = many_project_pool().await;
        let tools = AppWorkTools::new(pool.clone());
        let turn = AppTurnGuard::begin("req-n4", "fix the ferryxhub typo");

        let outcome = tools
            .handle(app(Some(turn.token()), submit()))
            .await
            .unwrap();

        assert_eq!(outcome["outcome"], "needs_choice");
        assert_eq!(outcome["missing"], "project");
        assert_eq!(goals(&pool).await, 0);
    }

    /// A project on two hosts has no single host, so the host stays unresolved and admission
    /// asks the owner instead of picking one.
    #[tokio::test]
    async fn a_project_on_two_hosts_names_no_host() {
        let pool = many_project_pool().await;
        let projects = projects(&pool).await;
        let ferryx = project_id_of(&projects, "ferryx").to_owned();
        bind_project_host(&pool, &ferryx, "host-b", "/srv/ferryx/.git")
            .await
            .unwrap();
        let tools = AppWorkTools::new(pool.clone());
        let turn = AppTurnGuard::begin("req-h1", "fix the ferryx README");

        let outcome = tools
            .handle(app(Some(turn.token()), submit()))
            .await
            .unwrap();

        assert_eq!(outcome["outcome"], "needs_choice");
        assert_eq!(outcome["missing"], "host");
        assert_eq!(goals(&pool).await, 0);
    }

    /// Rule 4: a `needs_choice` for the project lists the project names, so the owner can answer
    /// with one.
    #[tokio::test]
    async fn a_project_choice_lists_the_project_names() {
        let pool = many_project_pool().await;
        let tools = AppWorkTools::new(pool.clone());
        let turn = AppTurnGuard::begin("req-c1", "고쳐줘");

        let outcome = tools
            .handle(app(Some(turn.token()), submit()))
            .await
            .unwrap();

        assert_eq!(outcome["outcome"], "needs_choice");
        assert_eq!(outcome["missing"], "project");
        let detail = outcome["detail"].as_str().unwrap();
        for name in ["ferryx", "other", "pane"] {
            assert!(detail.contains(name), "{detail} does not list {name}");
        }
        assert_eq!(goals(&pool).await, 0);
    }

    /// Rules 3 and 4 together, the plan's failure-then-success scenario: no name is asked for by
    /// listing the names, and the next message naming exactly one project creates the goal.
    #[tokio::test]
    async fn the_project_named_in_answer_to_the_question_is_admitted() {
        let pool = many_project_pool().await;
        let projects = projects(&pool).await;
        let tools = AppWorkTools::new(pool.clone());

        let asked = AppTurnGuard::begin("req-c2", "고쳐줘");
        let outcome = tools
            .handle(app(Some(asked.token()), submit()))
            .await
            .unwrap();
        assert_eq!(outcome["missing"], "project");
        assert_eq!(goals(&pool).await, 0);

        let answer = AppTurnGuard::begin("req-c3", "ferryx");
        let admitted = tools
            .handle(app(Some(answer.token()), submit()))
            .await
            .unwrap();
        assert_eq!(admitted["outcome"], "admitted");

        let (project_id, host_id) = goal_target(&pool).await;
        assert_eq!(project_id, project_id_of(&projects, "ferryx"));
        assert_eq!(host_id, "host-a");
        assert_eq!(goals(&pool).await, 1);
    }

    /// Rule 3 answers the supervisor's own question: the owner's message alone decides, so a
    /// proposal title naming a second project cannot turn a one-name answer into a refusal.
    #[tokio::test]
    async fn an_answer_to_the_question_ignores_the_proposal_title() {
        let pool = many_project_pool().await;
        let projects = projects(&pool).await;
        let tools = AppWorkTools::new(pool.clone());
        let asked = AppTurnGuard::begin("req-c4", "고쳐줘");
        tools
            .handle(app(Some(asked.token()), submit()))
            .await
            .unwrap();

        let answer = AppTurnGuard::begin("req-c5", "ferryx");
        let titled = WorkToolCall::Submit {
            title: "Fix the typo in pane's README".into(),
            requirements: vec!["the typo is fixed".into()],
            worker: WorkerPreference::Any,
        };

        tools.handle(app(Some(answer.token()), titled)).await.unwrap();

        let (project_id, _) = goal_target(&pool).await;
        assert_eq!(project_id, project_id_of(&projects, "ferryx"));
    }

    /// Rule 3 is conditional: without the earlier question, the same message and title name two
    /// projects and are refused, never guessed.
    #[tokio::test]
    async fn without_a_question_the_same_title_still_names_two_projects() {
        let pool = many_project_pool().await;
        let tools = AppWorkTools::new(pool.clone());
        let turn = AppTurnGuard::begin("req-c6", "ferryx");
        let titled = WorkToolCall::Submit {
            title: "Fix the typo in pane's README".into(),
            requirements: vec!["the typo is fixed".into()],
            worker: WorkerPreference::Any,
        };

        let outcome = tools.handle(app(Some(turn.token()), titled)).await.unwrap();

        assert_eq!(outcome["outcome"], "needs_choice");
        assert_eq!(outcome["missing"], "project");
        assert_eq!(goals(&pool).await, 0);
    }

    /// The project question the owner currently owes, if any.
    async fn asked_project(pool: &SqlitePool) -> Option<String> {
        super::project_question(pool, OWNER).await.unwrap()
    }

    /// The question stands only while the owner owes a project: the next turn records its own
    /// answer, so an admitted goal clears it, and a question about the host never sets it.
    #[tokio::test]
    async fn only_a_project_question_arms_the_next_answer() {
        let pool = many_project_pool().await;
        let projects = projects(&pool).await;
        let ferryx = project_id_of(&projects, "ferryx").to_owned();
        let tools = AppWorkTools::new(pool.clone());
        assert_eq!(asked_project(&pool).await, None);

        // A host question is not a project question.
        bind_project_host(&pool, &ferryx, "host-b", "/srv/ferryx/.git")
            .await
            .unwrap();
        let host = AppTurnGuard::begin("req-c7", "fix the ferryx README");
        let outcome = tools
            .handle(app(Some(host.token()), submit()))
            .await
            .unwrap();
        assert_eq!(outcome["missing"], "host");
        assert_eq!(asked_project(&pool).await, None);

        // A project question is, and it names the turn that asked.
        let asked = AppTurnGuard::begin("req-c8", "고쳐줘");
        tools
            .handle(app(Some(asked.token()), submit()))
            .await
            .unwrap();
        assert_eq!(asked_project(&pool).await.as_deref(), Some("app:req-c8"));

        // The turn that answers it records its own outcome instead.
        let answer = AppTurnGuard::begin("req-c9", "other");
        let admitted = tools
            .handle(app(Some(answer.token()), submit()))
            .await
            .unwrap();
        assert_eq!(admitted["outcome"], "admitted");
        assert_eq!(asked_project(&pool).await, None);
    }

    #[derive(Default)]
    struct Fallback {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl WorkToolHandler for Fallback {
        async fn handle(&self, _request: WorkToolRequest) -> Result<Value, WorkToolRejection> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({ "handledBy": "fallback" }))
        }
    }

    #[tokio::test]
    async fn other_scopes_go_to_the_fallback_and_the_app_scope_never_does() {
        let pool = owner_pool().await;
        let fallback = Arc::new(Fallback::default());
        let tools = AppWorkTools::new(pool).with_fallback(fallback.clone());
        let slack = ConversationScope {
            project_id: None,
            channel_key: "slack:T1:UBOT:D1:".into(),
        };
        let reply = tools
            .handle(request(slack, Some(1), WorkToolCall::ListSessions))
            .await
            .unwrap();
        assert_eq!(reply["handledBy"], "fallback");
        assert_eq!(fallback.calls.load(Ordering::SeqCst), 1);
        // An app call stays with the app handler, whatever the fallback would answer.
        let turn = AppTurnGuard::begin("req-8", WORK);
        let refused = tools
            .handle(app(Some(turn.token()), WorkToolCall::ListSessions))
            .await
            .unwrap_err();
        assert_eq!(refused.code, "SUPERVISOR_SESSIONS_UNAVAILABLE");
        assert_eq!(fallback.calls.load(Ordering::SeqCst), 1);
    }
}
