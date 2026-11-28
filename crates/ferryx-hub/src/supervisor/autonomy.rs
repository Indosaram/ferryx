//! Production autonomy for worker prompts (plan D1, D3, G1): the one place that reads and
//! writes the owner's answer history, decides, answers on the owner's behalf, raises decision
//! cards, and keeps the action log and the panel's decision list current.
//!
//! Event driven: [`Autonomy::observe`] is called for each blocked-session observation, never
//! on a timer, and it does not recover sessions (the recovery scheduler is separate).
//!
//! - A prompt the history, a standing rule or the contract supports is answered through
//!   [`SessionControl`] and logged ("Answered ... / basis").
//! - Anything else, and every irreversible or external step (D1a), becomes an [`Escalation`]
//!   with a recommended option and a reason, published to the owner's panel.
//! - An answer the owner gives (card, phone, desktop) is recorded in [`AnswerHistory`], logged
//!   and resolves its card. An answer the supervisor itself gave is never recorded as the
//!   owner's, so learning cannot feed on itself.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use sqlx::SqlitePool;

use super::activity::SqliteActionLog;
use super::answer_history::{
    irreversible_or_external, AnswerContext, AnswerHistory, AnswerRecord,
};
use super::panel_contract::{
    ActionEntry, ActionKind, ActionLog, AnswerDecision, Escalation, EvidenceRef, FleetSession,
    PendingPrompt, RuleStore, SessionControl, SessionRef, StandingRule, UndoHandle,
};
use super::prompt_judge::{judge_prompt, BoundGoal, PromptJudge, MIN_CONFIDENCE};
use super::recovery::RecoveryEscalations;
use super::rules::SqliteRuleStore;
use super::undo::{ActionUndo, UndoError};
use crate::error::Result;
use crate::fleet::EscalationSource;
use crate::meta::mhc_supervisor::AnswerGate;
use crate::security::{auto_answer_decided, AutoDecision, Reading};

/// Wall clock in epoch milliseconds.
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Undo-handle kind of an autonomous answer: the undo restores the decision card.
pub const RESTORE_DECISION: &str = "restoreDecision";

/// Where the panel's owner-scoped frames go. The daemon implements it over its owner event
/// channel; every real mutation below calls it exactly once.
pub trait PanelPublisher: Send + Sync {
    /// An escalation appeared (`Some`) or was resolved (`None`) for `session_ref`.
    fn decision(&self, session_ref: &SessionRef, decision: Option<&Escalation>);
    /// An action-log entry was created or updated in place.
    fn action(&self, action: &ActionEntry);
    /// The standing rules changed; `rules` is the whole new list.
    fn rules(&self, rules: &[StandingRule]);
}

/// An [`ActionLog`] that announces every stored entry.
pub struct PublishingActionLog {
    inner: Arc<dyn ActionLog>,
    publisher: Arc<dyn PanelPublisher>,
}

impl PublishingActionLog {
    pub fn new(inner: Arc<dyn ActionLog>, publisher: Arc<dyn PanelPublisher>) -> Self {
        Self { inner, publisher }
    }
}

#[async_trait]
impl ActionLog for PublishingActionLog {
    async fn upsert_incident(&self, entry: ActionEntry) -> Result<ActionEntry> {
        let stored = self.inner.upsert_incident(entry).await?;
        self.publisher.action(&stored);
        Ok(stored)
    }

    async fn list_today(&self, since_ms: i64) -> Result<Vec<ActionEntry>> {
        self.inner.list_today(since_ms).await
    }

    async fn mark_undone(&self, id: i64, now_ms: i64) -> Result<()> {
        self.inner.mark_undone(id, now_ms).await?;
        if let Some(entry) = self.inner.list_today(0).await?.into_iter().find(|entry| entry.id == id) {
            self.publisher.action(&entry);
        }
        Ok(())
    }
}

/// A [`RuleStore`] that announces the whole list after every change.
pub struct PublishingRuleStore {
    inner: Arc<dyn RuleStore>,
    publisher: Arc<dyn PanelPublisher>,
}

impl PublishingRuleStore {
    pub fn new(inner: Arc<dyn RuleStore>, publisher: Arc<dyn PanelPublisher>) -> Self {
        Self { inner, publisher }
    }
}

#[async_trait]
impl RuleStore for PublishingRuleStore {
    async fn list(&self) -> Result<Vec<StandingRule>> {
        self.inner.list().await
    }

    async fn upsert(&self, rule: StandingRule) -> Result<StandingRule> {
        let stored = self.inner.upsert(rule).await?;
        self.publisher.rules(&self.inner.list().await?);
        Ok(stored)
    }
}

/// The decisions open for the owner, one per blocked session. After a restart the registry is
/// empty and refilled from the live blocked prompts, which are the source of truth.
#[derive(Default)]
pub struct EscalationRegistry {
    open: Mutex<HashMap<SessionRef, Escalation>>,
    /// Recovery escalations (a session the supervisor could not recover). Kept apart from
    /// `open` so a prompt observation clearing its card never withdraws them.
    recovery: Mutex<HashMap<SessionRef, Escalation>>,
}

impl EscalationRegistry {
    fn guard(&self) -> std::sync::MutexGuard<'_, HashMap<SessionRef, Escalation>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn put(&self, escalation: Escalation) {
        self.guard().insert(escalation.session_ref.clone(), escalation);
    }

    fn remove(&self, session_ref: &SessionRef) -> Option<Escalation> {
        self.guard().remove(session_ref)
    }

    fn recovery_guard(&self) -> std::sync::MutexGuard<'_, HashMap<SessionRef, Escalation>> {
        self.recovery.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn put_recovery(&self, escalation: Escalation) {
        self.recovery_guard()
            .insert(escalation.session_ref.clone(), escalation);
    }

    fn remove_recovery(&self, session_ref: &SessionRef) -> Option<Escalation> {
        self.recovery_guard().remove(session_ref)
    }

    fn contains(&self, session_ref: &SessionRef) -> bool {
        self.guard().contains_key(session_ref)
    }
}

#[async_trait]
impl EscalationSource for EscalationRegistry {
    async fn pending(&self) -> Result<Vec<Escalation>> {
        let mut open: Vec<Escalation> = self.guard().values().cloned().collect();
        open.extend(self.recovery_guard().values().cloned());
        open.sort_by(|a, b| {
            a.created_at_ms
                .cmp(&b.created_at_ms)
                .then_with(|| a.session_ref.as_str().cmp(b.session_ref.as_str()))
        });
        Ok(open)
    }
}

/// A git remote URL in a form two checkouts of one repository share.
fn normalize_remote(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    url.strip_suffix(".git").unwrap_or(url).to_ascii_lowercase()
}

/// The repository key of a working directory: its normalized `origin` URL, so every worktree
/// of one repository shares its history; the directory itself when there is no origin.
#[derive(Default)]
pub struct RepoResolver {
    cache: Mutex<HashMap<String, String>>,
}

impl RepoResolver {
    pub async fn key(&self, cwd: &str) -> String {
        if let Some(known) = self
            .cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(cwd)
            .cloned()
        {
            return known;
        }
        let key = origin_key(cwd).await.unwrap_or_else(|| cwd.to_owned());
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(cwd.to_owned(), key.clone());
        key
    }
}

async fn origin_key(cwd: &str) -> Option<String> {
    if cwd.is_empty() {
        return None;
    }
    let output = crate::child_env::command("git")
        .arg("-C")
        .arg(cwd)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!url.is_empty()).then(|| normalize_remote(&url))
}

/// The answer decision for one prompt, and the [`AnswerGate`] every supervisor-initiated
/// answer passes (engine tool or autonomous): the D1a classifier first, then the decision must
/// answer, and with the same option. Owner taps never come here.
pub struct DecisionGate {
    pool: SqlitePool,
    history: AnswerHistory,
    rules: Arc<dyn RuleStore>,
    repos: Arc<RepoResolver>,
    /// The engine's judgment, when one is installed. Only [`Self::decide`] asks it: the gate
    /// every supervisor-initiated answer passes ([`Self::allow`]) stays rules, contract and
    /// history, so an answer the engine proposed never costs a second model call.
    judge: Option<Arc<dyn PromptJudge>>,
}

impl DecisionGate {
    pub fn new(
        pool: SqlitePool,
        rules: Arc<dyn RuleStore>,
        repos: Arc<RepoResolver>,
        judge: Option<Arc<dyn PromptJudge>>,
    ) -> Self {
        Self {
            history: AnswerHistory::new(pool.clone()),
            pool,
            rules,
            repos,
            judge,
        }
    }

    /// The decision for `prompt` of `session`, from rules and the goal contract, then history,
    /// then the engine's judgment. No screen: only [`Self::decide_on_screen`] gives the engine
    /// the lines around the prompt.
    pub async fn decide(
        &self,
        session: &FleetSession,
        prompt: &PendingPrompt,
    ) -> Result<AnswerDecision> {
        self.decide_on_screen(session, prompt, "").await
    }

    /// [`Self::decide`] with the pane's screen, whose last lines the engine sees.
    pub async fn decide_on_screen(
        &self,
        session: &FleetSession,
        prompt: &PendingPrompt,
        screen: &str,
    ) -> Result<AnswerDecision> {
        let learned = self.decide_learned(session, prompt).await?;
        let AnswerDecision::Escalate {
            recommended_index,
            reason,
        } = learned
        else {
            return Ok(learned);
        };
        // An irreversible or external step never reaches the engine: the owner decides it,
        // whatever a model would say. Told apart by the classifier itself, never by the reason
        // text.
        let worktree = (!session.cwd.is_empty()).then_some(session.cwd.as_str());
        if irreversible_or_external(prompt, worktree).is_some() {
            return Ok(AnswerDecision::Escalate {
                recommended_index,
                reason,
            });
        }
        // Neither does an approval read off the screen: the reading may have the command
        // wrong, so the screen tier is always the owner's. Asking the engine for an answer
        // that can never be typed only spends a model call.
        if session.state_source == "screen"
            && matches!(prompt.kind.as_str(), "permission" | "approval")
        {
            return Ok(AnswerDecision::Escalate {
                recommended_index,
                reason,
            });
        }
        Ok(self
            .judged(session, prompt, screen, recommended_index, &reason)
            .await
            .unwrap_or(AnswerDecision::Escalate {
                recommended_index,
                reason,
            }))
    }

    /// Rules, the bound goal's contract, and the owner's past answers: no engine call. This is
    /// what [`AnswerGate::allow`] checks a supervisor-initiated answer against.
    async fn decide_learned(
        &self,
        session: &FleetSession,
        prompt: &PendingPrompt,
    ) -> Result<AnswerDecision> {
        let repo = self.repos.key(&session.cwd).await;
        let rules = self.rules.list().await?;
        let worktree = (!session.cwd.is_empty()).then_some(session.cwd.as_str());
        let context = AnswerContext {
            prompt,
            repo: &repo,
            agent: &session.agent,
            host_id: &session.host_id,
            worktree,
        };
        let decisions = self.contract_decisions(session).await?;
        self.history
            .decide(&context, &rules, decisions.as_ref().map(|(revision, decisions)| {
                super::answer_history::ContractDecisions {
                    goal_revision: *revision,
                    decisions,
                }
            }))
            .await
    }

    /// The goal contract's decisions for the goal `session` is bound to, with the goal revision
    /// they were decided at; `Ok(None)` for a session no goal is bound to. A read that fails is
    /// an error, never "no contract": the caller escalates rather than answer a prompt the
    /// owner already decided, so a database fault can only cost the owner a tap, not a wrong
    /// answer (the `observe` error arm says the earlier answers could not be read).
    async fn contract_decisions(
        &self,
        session: &FleetSession,
    ) -> Result<Option<(i64, Vec<super::answers::PromptDecision>)>> {
        let Some(goal) = self.bound_goal(session).await? else {
            return Ok(None);
        };
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT prompt_id, option_label, source FROM supervisor_goal_decisions
              WHERE goal_id = ? AND goal_revision = ? ORDER BY id",
        )
        .bind(&goal.goal_id)
        .bind(goal.revision)
        .fetch_all(&self.pool)
        .await?;
        let decisions: Vec<super::answers::PromptDecision> = rows
            .into_iter()
            .filter_map(|(prompt_id, option_label, source)| {
                let source = match source.as_str() {
                    "owner" => super::answers::DecisionSource::Owner,
                    "contract" => super::answers::DecisionSource::Contract,
                    _ => return None,
                };
                Some(super::answers::PromptDecision {
                    goal_revision: goal.revision,
                    prompt_id,
                    option_label,
                    source,
                })
            })
            .collect();
        Ok((!decisions.is_empty()).then_some((goal.revision, decisions)))
    }

    /// The goal `session` is bound to through its execution binding on the session's own host:
    /// its revision, the owner's admitted request and the contract's requirements. A provider
    /// session id is unique on one host only, so the host is part of the lookup: the same id on
    /// another host is another worker, bound to another goal (or none), and reading it as this
    /// one would bind this session to a goal it never ran.
    pub async fn bound_goal(&self, session: &FleetSession) -> Result<Option<BoundGoal>> {
        let backend_session_id = session.session_ref.backend_session_id();
        if backend_session_id.is_empty() {
            return Ok(None);
        }
        let row: Option<(String, i64, String, Option<String>)> =
            sqlx::query_as::<_, (String, i64, String, Option<String>)>(
                "SELECT g.goal_id, g.revision, g.lane_key, r.body
                   FROM supervisor_execution_bindings b
                   JOIN supervisor_goals g ON g.goal_id = b.goal_id
                   LEFT JOIN supervisor_requests r ON r.goal_id = g.goal_id
                  WHERE b.host_id = ? AND b.provider_session_id = ?
                  ORDER BY r.id LIMIT 1",
            )
            .bind(&session.host_id)
            .bind(backend_session_id)
            .fetch_optional(&self.pool)
            .await?;
        let Some((goal_id, revision, lane_key, title)) = row else {
            return Ok(None);
        };
        let requirements: Vec<String> = sqlx::query_scalar(
            "SELECT text FROM lane_goal_requirements WHERE lane_key = ? ORDER BY rowid",
        )
        .bind(&lane_key)
        .fetch_all(&self.pool)
        .await?;
        Ok(Some(BoundGoal {
            goal_id,
            revision,
            title: title.unwrap_or_default(),
            requirements,
        }))
    }

    /// Asks the engine what the owner would pick, from the owner's memory, the project's memory,
    /// the reference hits for the prompt, the bound goal and the screen. `None` when no engine
    /// is installed, the call fails, or the verdict cannot be used: the caller then keeps the
    /// escalation it already had. A confident valid verdict becomes an answer; anything else an
    /// escalation that recommends the engine's option and carries its reason.
    async fn judged(
        &self,
        session: &FleetSession,
        prompt: &PendingPrompt,
        screen: &str,
        recommended_index: usize,
        reason: &str,
    ) -> Option<AnswerDecision> {
        let judge = self.judge.as_ref()?;
        let goal = match self.bound_goal(session).await {
            Ok(goal) => goal,
            Err(error) => {
                tracing::warn!(%error, "the bound goal could not be read; judging without it");
                None
            }
        };
        let repo = self.repos.key(&session.cwd).await;
        let asked = judge_prompt(&self.pool, session, prompt, goal.as_ref(), &repo).await;
        let asked = super::prompt_judge::with_screen(asked, screen);
        let verdict = match judge.judge(&asked).await {
            Ok(verdict) => verdict,
            Err(error) => {
                tracing::warn!(%error, "the prompt judge failed; the owner decides");
                return None;
            }
        };
        let index = verdict.option_index;
        if index < prompt.options.len() && verdict.confidence >= MIN_CONFIDENCE {
            return Some(AnswerDecision::Answer {
                option_index: index,
                basis: format!("Judged from your memory: {}", verdict.reason),
            });
        }
        // An index that is not an option is no recommendation at all; the learned one stands.
        let recommended_index = if index < prompt.options.len() { index } else { recommended_index };
        Some(AnswerDecision::Escalate {
            recommended_index,
            reason: if verdict.reason.is_empty() {
                reason.to_owned()
            } else {
                format!("Judged, but not certainly enough: {}", verdict.reason)
            },
        })
    }

    /// Whether the owner paused the supervisor. A pause flag that cannot be read counts as set.
    pub async fn paused(&self) -> bool {
        crate::control::is_paused(&self.pool).await.unwrap_or(true)
    }

    pub async fn session_paused(&self, session: &SessionRef) -> bool {
        self.paused().await || super::hold::session_held(&self.pool, session).await.unwrap_or(true)
    }

    /// The hub database the goal decisions and answer history live in.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[async_trait]
impl AnswerGate for DecisionGate {
    async fn allow(
        &self,
        session: &FleetSession,
        prompt: &PendingPrompt,
        option_index: usize,
    ) -> std::result::Result<(), String> {
        if self.session_paused(&session.session_ref).await {
            return Err("the supervisor is paused; the owner decides".to_owned());
        }
        let worktree = (!session.cwd.is_empty()).then_some(session.cwd.as_str());
        if let Some(category) = irreversible_or_external(prompt, worktree) {
            return Err(format!(
                "this is an irreversible or external step ({category}); the owner decides"
            ));
        }
        // Rules, contract and history only: an answer the engine proposed is not re-judged.
        match self
            .decide_learned(session, prompt)
            .await
            .map_err(|error| error.to_string())?
        {
            AnswerDecision::Answer {
                option_index: chosen,
                ..
            } if chosen == option_index => Ok(()),
            AnswerDecision::Answer { .. } => Err(
                "that option is not the one the owner's earlier answers support; the owner decides"
                    .to_owned(),
            ),
            AnswerDecision::Escalate { reason, .. } => Err(reason),
        }
    }
}

/// A prompt the service is tracking for a session.
struct Tracked {
    session: FleetSession,
    prompt: PendingPrompt,
    repo: String,
    /// Settled: the supervisor answered it, or the owner did.
    settled: bool,
}

/// What one answered-prompt action-log row says, apart from the session and prompt it is about.
struct LogLine {
    summary: String,
    basis: String,
    undo: Option<UndoHandle>,
    /// Request id of the delivery receipt agent control returned, when there was one.
    receipt: Option<String>,
}

/// The newest prompt the supervisor answered on its own in a session: what Undo restores.
/// Dropped once a different prompt or screen revision shows up in that session.
struct AutonomousAnswer {
    session: FleetSession,
    prompt: PendingPrompt,
    repo: String,
    option_index: usize,
    basis: String,
}

/// Whether the supervisor may judge at all (todo 2's `supervisorHealthy`). While it is not,
/// [`Autonomy::observe`] escalates every prompt, including the ones a rule or the owner's own
/// past answers would settle: a failing engine judged whose answers cannot be trusted.
#[async_trait]
pub trait SupervisorHealth: Send + Sync {
    async fn healthy(&self) -> bool;
}

/// The engine's recorded calls as its health: a hub that never recorded one is unknown, not
/// failing, and stays healthy.
pub struct RecordedEngineHealth {
    pool: SqlitePool,
}

impl RecordedEngineHealth {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SupervisorHealth for RecordedEngineHealth {
    async fn healthy(&self) -> bool {
        !super::setup::supervisor_degraded(&self.pool).await
    }
}

/// The production autonomy service. Build it with [`Autonomy::open`].
pub struct Autonomy {
    history: AnswerHistory,
    gate: Arc<DecisionGate>,
    rules: Arc<dyn RuleStore>,
    log: Arc<dyn ActionLog>,
    escalations: Arc<EscalationRegistry>,
    control: Arc<dyn SessionControl>,
    publisher: Arc<dyn PanelPublisher>,
    repos: Arc<RepoResolver>,
    health: Arc<dyn SupervisorHealth>,
    tracked: Mutex<HashMap<SessionRef, Tracked>>,
    answered: Mutex<HashMap<SessionRef, AutonomousAnswer>>,
}

/// What [`Autonomy::build`] needs, so the constructor takes one argument rather than eight.
/// `open_with` and `open_with_health` fill in the same stores; only the judge and the health
/// probe differ.
pub struct AutonomyParts {
    pub pool: SqlitePool,
    pub control: Arc<dyn SessionControl>,
    pub publisher: Arc<dyn PanelPublisher>,
    pub rules: Arc<dyn RuleStore>,
    pub log: Arc<dyn ActionLog>,
    pub repos: Arc<RepoResolver>,
    pub judge: Option<Arc<dyn PromptJudge>>,
    pub health: Arc<dyn SupervisorHealth>,
}

impl Autonomy {
    /// The hub database this service's stores live in.
    pub fn pool(&self) -> &SqlitePool {
        self.gate.pool()
    }

    /// Opens the stores on the hub database (seeding the default rules once) and wires the
    /// publisher into every store mutation. The engine judge is wired by
    /// [`Self::open_with`]; this one judges nothing and trusts the engine's recorded calls.
    pub async fn open(
        pool: SqlitePool,
        control: Arc<dyn SessionControl>,
        publisher: Arc<dyn PanelPublisher>,
    ) -> Result<Arc<Self>> {
        Self::open_with(pool, control, publisher, None).await
    }

    /// [`Self::open`] with the engine's judge (and its recorded-call health).
    pub async fn open_with(
        pool: SqlitePool,
        control: Arc<dyn SessionControl>,
        publisher: Arc<dyn PanelPublisher>,
        judge: Option<Arc<dyn PromptJudge>>,
    ) -> Result<Arc<Self>> {
        let rules: Arc<dyn RuleStore> = Arc::new(PublishingRuleStore::new(
            Arc::new(SqliteRuleStore::open(pool.clone(), now_ms()).await?),
            publisher.clone(),
        ));
        let log: Arc<dyn ActionLog> = Arc::new(PublishingActionLog::new(
            Arc::new(SqliteActionLog::new(pool.clone())),
            publisher.clone(),
        ));
        let repos = Arc::new(RepoResolver::default());
        let health: Arc<dyn SupervisorHealth> = Arc::new(RecordedEngineHealth::new(pool.clone()));
        Self::build(AutonomyParts {
            pool,
            control,
            publisher,
            rules,
            log,
            repos,
            judge,
            health,
        })
        .await
    }

    /// [`Self::open_with`] with the health probe replaced: a test (or a caller that knows
    /// better than the recorded calls) decides whether the supervisor is degraded.
    pub async fn open_with_health(
        pool: SqlitePool,
        control: Arc<dyn SessionControl>,
        publisher: Arc<dyn PanelPublisher>,
        judge: Option<Arc<dyn PromptJudge>>,
        health: Arc<dyn SupervisorHealth>,
    ) -> Result<Arc<Self>> {
        let rules: Arc<dyn RuleStore> = Arc::new(PublishingRuleStore::new(
            Arc::new(SqliteRuleStore::open(pool.clone(), now_ms()).await?),
            publisher.clone(),
        ));
        let log: Arc<dyn ActionLog> = Arc::new(PublishingActionLog::new(
            Arc::new(SqliteActionLog::new(pool.clone())),
            publisher.clone(),
        ));
        let repos = Arc::new(RepoResolver::default());
        Self::build(AutonomyParts {
            pool,
            control,
            publisher,
            rules,
            log,
            repos,
            judge,
            health,
        })
        .await
    }

    async fn build(parts: AutonomyParts) -> Result<Arc<Self>> {
        let AutonomyParts {
            pool,
            control,
            publisher,
            rules,
            log,
            repos,
            judge,
            health,
        } = parts;
        Ok(Arc::new(Self {
            history: AnswerHistory::new(pool.clone()),
            gate: Arc::new(DecisionGate::new(pool, rules.clone(), repos.clone(), judge)),
            rules,
            log,
            escalations: Arc::new(EscalationRegistry::default()),
            control,
            publisher,
            repos,
            health,
            tracked: Mutex::new(HashMap::new()),
            answered: Mutex::new(HashMap::new()),
        }))
    }

    pub fn gate(&self) -> Arc<DecisionGate> {
        self.gate.clone()
    }

    /// The standing rules; a change announces itself.
    pub fn rule_store(&self) -> Arc<dyn RuleStore> {
        self.rules.clone()
    }

    /// The action log; an entry announces itself.
    pub fn action_log(&self) -> Arc<dyn ActionLog> {
        self.log.clone()
    }

    /// The open decisions the panel shows.
    pub fn escalation_source(&self) -> Arc<dyn EscalationSource> {
        self.escalations.clone()
    }

    /// The persisted owner answers, to read back what the supervisor learned.
    pub fn answer_history(&self) -> AnswerHistory {
        self.history.clone()
    }

    fn tracked(&self) -> std::sync::MutexGuard<'_, HashMap<SessionRef, Tracked>> {
        self.tracked.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn answered(&self) -> std::sync::MutexGuard<'_, HashMap<SessionRef, AutonomousAnswer>> {
        self.answered.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The session has no pending prompt any more (answered, in the terminal or elsewhere, or
    /// gone): its card disappears.
    pub fn clear(&self, session_ref: &SessionRef) {
        self.tracked().remove(session_ref);
        if self.escalations.remove(session_ref).is_some() {
            self.publisher.decision(session_ref, None);
        }
    }

    /// One observation of a session. A new pending prompt is answered from what the owner has
    /// taught the supervisor, or escalated; a session without one clears its card.
    pub async fn observe(&self, session: &FleetSession) {
        let Some(prompt) = session.pending_prompt.clone() else {
            self.clear(&session.session_ref);
            return;
        };
        if session.stale {
            return;
        }
        let repo = self.repos.key(&session.cwd).await;
        // A different prompt or screen revision supersedes the last autonomous answer: its
        // Undo may not resurrect a card over it.
        self.answered().retain(|key, answered| {
            *key != session.session_ref
                || (answered.prompt.prompt_id == prompt.prompt_id
                    && answered.prompt.screen_revision == prompt.screen_revision)
        });
        let replaced = {
            let mut tracked = self.tracked();
            match tracked.get(&session.session_ref) {
                Some(known) if known.prompt.prompt_id == prompt.prompt_id => return,
                other => {
                    let replaced = other.is_some();
                    tracked.insert(
                        session.session_ref.clone(),
                        Tracked {
                            session: session.clone(),
                            prompt: prompt.clone(),
                            repo,
                            settled: false,
                        },
                    );
                    replaced
                }
            }
        };
        if replaced && self.escalations.remove(&session.session_ref).is_some() {
            self.publisher.decision(&session.session_ref, None);
        }
        let paused = self.gate.session_paused(&session.session_ref).await;
        // While the supervisor is degraded, or the owner has paused it, every prompt goes to the
        // owner, including the ones a rule or the owner's own past answers would settle: no
        // engine call is spent and nothing is typed.
        let healthy = self.health.healthy().await;
        if paused || !healthy {
            let (recommended_index, basis) = match self.gate.decide_learned(session, &prompt).await {
                Ok(AnswerDecision::Answer { option_index, basis }) => (option_index, basis),
                Ok(AnswerDecision::Escalate {
                    recommended_index,
                    reason,
                }) => (recommended_index, reason),
                Err(error) => (0, format!("Your earlier answers could not be read ({error}).")),
            };
            let why = if healthy {
                format!("The supervisor is paused. {basis}")
            } else {
                format!("supervisor degraded: {basis}")
            };
            self.escalate(session, &prompt, recommended_index, why);
            return;
        }
        // The last lines of the pane are what the engine judges around: read once, and an
        // unreadable screen is not a reason to answer without them.
        let screen = self
            .control
            .read_tail(&session.session_ref, super::prompt_judge::SCREEN_LINES)
            .await
            .unwrap_or_default();
        let decision = self.gate.decide_on_screen(session, &prompt, &screen).await;
        // The same reading the screen policy sees: who reported the state, what kind of prompt
        // it is, and whether the daemon could read the screen at all.
        let reading = Reading {
            state_source: session.state_source.clone(),
            kind: Some(prompt.kind.clone()),
            screen_readable: session.screen_readable,
        };
        let worktree = (!session.cwd.is_empty()).then_some(session.cwd.as_str());
        match decision {
            Ok(AnswerDecision::Answer { option_index, basis }) => {
                let decided = AnswerDecision::Answer {
                    option_index,
                    basis: basis.clone(),
                };
                match auto_answer_decided(&reading, &prompt, worktree, &decided) {
                    AutoDecision::Answer { .. } => {
                        self.answer(session, &prompt, option_index, &basis).await;
                    }
                    // A held or owner-only prompt stays with the owner: nothing is typed.
                    AutoDecision::EscalateOwner => self.escalate(
                        session,
                        &prompt,
                        option_index,
                        format!("This one is yours to decide. Basis: {basis}"),
                    ),
                    AutoDecision::Hold => self.escalate(
                        session,
                        &prompt,
                        option_index,
                        format!("I could not read this pane's screen. Basis: {basis}"),
                    ),
                }
            }
            Ok(AnswerDecision::Escalate {
                recommended_index,
                reason,
            }) => self.escalate(session, &prompt, recommended_index, reason),
            Err(error) => self.escalate(
                session,
                &prompt,
                0,
                format!("Your earlier answers could not be read ({error})."),
            ),
        }
    }

    async fn answer(
        &self,
        session: &FleetSession,
        prompt: &PendingPrompt,
        option_index: usize,
        basis: &str,
    ) {
        let sent = self
            .control
            .answer(
                &session.session_ref,
                &prompt.prompt_id,
                prompt.screen_revision.as_deref(),
                option_index,
            )
            .await;
        let request_id = match sent {
            Ok(request_id) => request_id,
            Err(error) => {
                self.escalate(
                    session,
                    prompt,
                    option_index,
                    format!("I could not answer it ({error})."),
                );
                return;
            }
        };
        if let Some(known) = self.tracked().get_mut(&session.session_ref) {
            known.settled = true;
        }
        let repo = self.repos.key(&session.cwd).await;
        self.answered().insert(
            session.session_ref.clone(),
            AutonomousAnswer {
                session: session.clone(),
                prompt: prompt.clone(),
                repo,
                option_index,
                basis: basis.to_owned(),
            },
        );
        let label = prompt.options.get(option_index).cloned().unwrap_or_default();
        let undo = UndoHandle {
            kind: RESTORE_DECISION.to_owned(),
            payload: serde_json::json!({
                "sessionRef": session.session_ref.as_str(),
                "promptId": prompt.prompt_id,
                "screenRevision": prompt.screen_revision,
                "optionIndex": option_index,
            }),
        };
        self.write_log(
            session,
            prompt,
            "answer",
            LogLine {
                summary: format!("Answered \"{label}\" in {}", session.title),
                basis: basis.to_owned(),
                undo: Some(undo),
                receipt: request_id,
            },
        )
        .await;
    }

    fn escalate(
        &self,
        session: &FleetSession,
        prompt: &PendingPrompt,
        recommended_index: usize,
        reason: String,
    ) {
        let last = prompt.options.len().saturating_sub(1);
        let escalation = Escalation {
            session_ref: session.session_ref.clone(),
            prompt: prompt.clone(),
            recommended_index: recommended_index.min(last),
            reason,
            created_at_ms: now_ms(),
        };
        self.escalations.put(escalation.clone());
        self.publisher
            .decision(&escalation.session_ref, Some(&escalation));
    }

    async fn write_log(
        &self,
        session: &FleetSession,
        prompt: &PendingPrompt,
        who: &str,
        line: LogLine,
    ) {
        let LogLine { summary, basis, undo, receipt } = line;
        let now = now_ms();
        let entry = ActionEntry {
            id: 0,
            incident_key: format!("{who}:{}:{}", session.session_ref, prompt.prompt_id),
            session_ref: Some(session.session_ref.clone()),
            kind: ActionKind::Answered,
            summary,
            basis,
            // D9: only the request id of the receipt agent control returned proves delivery.
            // An answer accepted without one, and an owner answer observed in passing, carry no
            // evidence and stay Unverified.
            evidence: receipt
                .into_iter()
                .map(|id| EvidenceRef {
                    kind: "receipt".to_owned(),
                    id,
                })
                .collect(),
            undo,
            live: false,
            created_at_ms: now,
            updated_at_ms: now,
        };
        if let Err(error) = self.log.upsert_incident(entry).await {
            tracing::warn!(%error, "an answered prompt was not logged");
        }
    }

    /// Claims the owner's answer to prompt `prompt_id` of the session whose backend id is
    /// `backend_session_id`, synchronously and before the pane's next observation can clear the
    /// tracked prompt. `None` for a prompt the supervisor never saw or that is already settled
    /// (answered by the supervisor, or by the owner a moment ago), so nothing is recorded twice
    /// and the supervisor's own answers are never learned as the owner's.
    pub fn take_owner_answer(&self, backend_session_id: &str, prompt_id: &str) -> Option<OwnerAnswer> {
        let mut tracked = self.tracked();
        let (session_ref, known) = tracked.iter_mut().find(|(session_ref, known)| {
            session_ref.backend_session_id() == backend_session_id
                && known.prompt.prompt_id == prompt_id
        })?;
        if known.settled {
            return None;
        }
        known.settled = true;
        Some(OwnerAnswer {
            session_ref: session_ref.clone(),
            session: known.session.clone(),
            prompt: known.prompt.clone(),
            repo: known.repo.clone(),
        })
    }

    /// The owner's answer to a tracked, unsettled prompt, without settling it. Taken before the
    /// answer is typed, so the pane's next observation cannot clear the prompt first; the caller
    /// records it with [`Self::record_owner_answer`] once the answer went through.
    ///
    /// `host_matches` says which computers the answer was typed on: backend and prompt ids are
    /// only unique per computer, so a pane on one host never claims another host's prompt.
    pub fn peek_owner_answer(
        &self,
        host_matches: impl Fn(&str) -> bool,
        backend_session_id: &str,
        prompt_id: &str,
    ) -> Option<OwnerAnswer> {
        let (claimed, ()) = self.peek_owner_answer_where(host_matches, backend_session_id, |prompt| {
            (prompt.prompt_id == prompt_id).then_some(())
        })?;
        Some(claimed)
    }

    /// The owner's answer to the first prompt of `backend_session_id` on a host `host_matches`
    /// accepts that `select` maps to a value: the answer with that value. Read under one lock
    /// over the tracked prompts, so the prompt a caller decided on is the one it claimed; typed
    /// input knows only its session, and `select` says what it picked on the prompt it found.
    pub fn peek_owner_answer_where<T>(
        &self,
        host_matches: impl Fn(&str) -> bool,
        backend_session_id: &str,
        select: impl Fn(&PendingPrompt) -> Option<T>,
    ) -> Option<(OwnerAnswer, T)> {
        let tracked = self.tracked();
        let (session_ref, known, selected) = tracked.iter().find_map(|(session_ref, known)| {
            if !host_matches(session_ref.host_id())
                || session_ref.backend_session_id() != backend_session_id
            {
                return None;
            }
            let selected = select(&known.prompt)?;
            Some((session_ref, known, selected))
        })?;
        if known.settled {
            return None;
        }
        let answer = OwnerAnswer {
            session_ref: session_ref.clone(),
            session: known.session.clone(),
            prompt: known.prompt.clone(),
            repo: known.repo.clone(),
        };
        Some((answer, selected))
    }

    /// Records a claimed owner answer: the choice joins the answer history (a duplicate or an
    /// unlearnable shape is not stored), the log gets "You chose ...", and the card resolves.
    pub async fn record_owner_answer(&self, answer: OwnerAnswer, option_index: usize) {
        let OwnerAnswer {
            session_ref,
            session,
            prompt,
            repo,
        } = answer;
        if let Some(known) = self.tracked().get_mut(&session_ref) {
            known.settled = true;
        }
        if let Some(label) = prompt.options.get(option_index) {
            let record = AnswerRecord {
                prompt: &prompt,
                repo: &repo,
                agent: &session.agent,
                option_label: label,
                at_ms: now_ms(),
            };
            // `Ok(false)`: not learned (no safe shape, or this answer was already recorded).
            let learned = match self.history.record(&record).await {
                Ok(stored) => stored,
                Err(error) => {
                    tracing::warn!(%error, "an owner answer was not recorded");
                    false
                }
            };
            // A prompt of a bound goal is also decided for that goal and revision, so the next
            // pane of the same goal asking the same prompt is answered from the goal's own
            // terms even before any history exists.
            self.decide_for_goal(&session, &prompt, label).await;
            let basis = if learned {
                "Your answer, remembered for next time"
            } else {
                "Your answer"
            };
            self.write_log(
                &session,
                &prompt,
                "owner",
                LogLine {
                    summary: format!("You chose {label}"),
                    basis: basis.to_owned(),
                    undo: None,
                    receipt: None,
                },
            )
            .await;
        }
        if self.escalations.remove(&session_ref).is_some() {
            self.publisher.decision(&session_ref, None);
        }
    }

    /// Records the owner's own answer to a prompt of a goal `session` is bound to, against the
    /// goal's current revision. A session no goal is bound to writes nothing; a prompt that was
    /// already decided for that revision keeps its first label (a later answer to the same
    /// prompt is the same decision seen twice).
    async fn decide_for_goal(&self, session: &FleetSession, prompt: &PendingPrompt, label: &str) {
        let goal = match self.gate.bound_goal(session).await {
            Ok(goal) => goal,
            Err(error) => {
                tracing::warn!(%error, "the bound goal could not be read; the answer was not recorded");
                return;
            }
        };
        let Some(goal) = goal else {
            return;
        };
        let written = sqlx::query(
            "INSERT INTO supervisor_goal_decisions(goal_id, goal_revision, prompt_id, option_label,
                source, created_at)
             SELECT ?, ?, ?, ?, 'owner', ?
              WHERE NOT EXISTS(SELECT 1 FROM supervisor_goal_decisions
                                WHERE goal_id = ? AND goal_revision = ? AND prompt_id = ?)",
        )
        .bind(&goal.goal_id)
        .bind(goal.revision)
        .bind(&prompt.prompt_id)
        .bind(label)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&goal.goal_id)
        .bind(goal.revision)
        .bind(&prompt.prompt_id)
        .execute(self.gate.pool())
        .await;
        if let Err(error) = written {
            tracing::warn!(%error, "an owner answer was not recorded for its goal");
        }
    }

    /// The owner chose option `option_index` of prompt `prompt_id` (a card tap, the phone, the
    /// desktop, Slack). See [`Self::take_owner_answer`] and [`Self::record_owner_answer`].
    pub async fn owner_answered(&self, backend_session_id: &str, prompt_id: &str, option_index: usize) {
        if let Some(answer) = self.take_owner_answer(backend_session_id, prompt_id) {
            self.record_owner_answer(answer, option_index).await;
        }
    }
}

impl RecoveryEscalations for Autonomy {
    fn open(&self, escalation: Escalation) {
        self.escalations.put_recovery(escalation.clone());
        self.publisher
            .decision(&escalation.session_ref, Some(&escalation));
    }

    fn resolve(&self, session: &SessionRef) {
        if self.escalations.remove_recovery(session).is_some() {
            self.publisher.decision(session, None);
        }
    }
}

/// An owner answer claimed by [`Autonomy::take_owner_answer`].
pub struct OwnerAnswer {
    session_ref: SessionRef,
    session: FleetSession,
    prompt: PendingPrompt,
    repo: String,
}

/// Undo of an autonomous answer: the item is pending again as a decision card recommending the
/// option that was sent, with the basis as its reason. Nothing is typed into the pane and
/// nothing is learned. Refusals are typed: no handle (never answerable, or already undone) is
/// `Nothing`; a newer prompt or screen revision, an open card, or a restart that lost the
/// prompt is `Stale`, and the handle stays.
#[async_trait]
impl ActionUndo for Autonomy {
    async fn undo(&self, id: i64) -> std::result::Result<(), UndoError> {
        let entry = self
            .log
            .list_today(0)
            .await
            .map_err(|error| UndoError::Failed(error.to_string()))?
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or(UndoError::Nothing(id))?;
        let handle = entry.undo.ok_or(UndoError::Nothing(id))?;
        if handle.kind != RESTORE_DECISION {
            return Err(UndoError::NotSupported);
        }
        let session_ref = entry.session_ref.ok_or(UndoError::Nothing(id))?;
        let prompt_id = handle.payload.get("promptId").and_then(|v| v.as_str());
        let revision = handle.payload.get("screenRevision").and_then(|v| v.as_str());
        let record = {
            let mut answered = self.answered();
            let current = answered.get(&session_ref).is_some_and(|known| {
                Some(known.prompt.prompt_id.as_str()) == prompt_id
                    && known.prompt.screen_revision.as_deref() == revision
            });
            let superseded = self.tracked().get(&session_ref).is_some_and(|known| {
                Some(known.prompt.prompt_id.as_str()) != prompt_id
                    || known.prompt.screen_revision.as_deref() != revision
            });
            if !current || superseded || self.escalations.contains(&session_ref) {
                return Err(UndoError::Stale(id));
            }
            answered.remove(&session_ref).ok_or(UndoError::Stale(id))?
        };
        if let Err(error) = self.log.mark_undone(id, now_ms()).await {
            self.answered().insert(session_ref, record);
            return Err(UndoError::Failed(error.to_string()));
        }
        self.tracked().insert(
            session_ref,
            Tracked {
                session: record.session.clone(),
                prompt: record.prompt.clone(),
                repo: record.repo,
                settled: false,
            },
        );
        self.escalate(
            &record.session,
            &record.prompt,
            record.option_index,
            format!("You undid my answer. Basis: {}", record.basis),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;
