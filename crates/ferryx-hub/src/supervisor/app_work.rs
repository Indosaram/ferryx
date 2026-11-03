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
    self, ActorContext, OperationClass, OwnerChoices, SubmitOutcome, SubmitWork,
};
use super::{goal_of, ChannelProvider, ExecutionKind};
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

/// An app turn in flight: the owner's message and what its submits did.
struct AppTurn {
    request_id: String,
    text: String,
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
        let mut registry = registry();
        registry.next += 1;
        let generation = registry.next;
        registry.live.insert(
            generation,
            AppTurn {
                request_id: request_id.to_owned(),
                text: owner_text.to_owned(),
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
    let turn = AppTurnGuard::begin(request_id, owner_text);
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
                let outcome = admission::handle_message(&self.pool, &actor, Some(&work))
                    .await
                    .map_err(|error| refused(&error))?;
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
        ) -> HubResult<()> {
            self.calls.lock().unwrap().push(format!(
                "answer {session} {prompt_id} {screen_revision:?} {option_index}"
            ));
            Ok(())
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
        }
    }

    fn with_sessions(pool: &SqlitePool) -> (Arc<FakeControl>, AppWorkTools, Vec<FleetSession>) {
        let fleet = vec![fleet_session()];
        let control = Arc::new(FakeControl::default());
        let sessions = SessionTools::new(control.clone(), Arc::new(FakeFleet(fleet.clone())));
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
