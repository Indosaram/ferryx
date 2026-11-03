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
    PendingPrompt, RuleStore, SessionControl, SessionRef, StandingRule,
};
use super::recovery::RecoveryEscalations;
use super::rules::SqliteRuleStore;
use crate::error::Result;
use crate::fleet::EscalationSource;
use crate::meta::mhc_supervisor::AnswerGate;

/// Wall clock in epoch milliseconds.
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

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
}

impl DecisionGate {
    pub fn new(pool: SqlitePool, rules: Arc<dyn RuleStore>, repos: Arc<RepoResolver>) -> Self {
        Self {
            history: AnswerHistory::new(pool.clone()),
            pool,
            rules,
            repos,
        }
    }

    /// The decision for `prompt` of `session`, from rules, then history.
    pub async fn decide(
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
        self.history.decide(&context, &rules, None).await
    }

    /// Whether the owner paused the supervisor. A pause flag that cannot be read counts as set.
    pub async fn paused(&self) -> bool {
        crate::control::is_paused(&self.pool).await.unwrap_or(true)
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
        if self.paused().await {
            return Err("the supervisor is paused; the owner decides".to_owned());
        }
        let worktree = (!session.cwd.is_empty()).then_some(session.cwd.as_str());
        if let Some(category) = irreversible_or_external(prompt, worktree) {
            return Err(format!(
                "this is an irreversible or external step ({category}); the owner decides"
            ));
        }
        match self
            .decide(session, prompt)
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
    tracked: Mutex<HashMap<SessionRef, Tracked>>,
}

impl Autonomy {
    /// Opens the stores on the hub database (seeding the default rules once) and wires the
    /// publisher into every store mutation.
    pub async fn open(
        pool: SqlitePool,
        control: Arc<dyn SessionControl>,
        publisher: Arc<dyn PanelPublisher>,
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
        Ok(Arc::new(Self {
            history: AnswerHistory::new(pool.clone()),
            gate: Arc::new(DecisionGate::new(pool, rules.clone(), repos.clone())),
            rules,
            log,
            escalations: Arc::new(EscalationRegistry::default()),
            control,
            publisher,
            repos,
            tracked: Mutex::new(HashMap::new()),
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

    fn tracked(&self) -> std::sync::MutexGuard<'_, HashMap<SessionRef, Tracked>> {
        self.tracked.lock().unwrap_or_else(PoisonError::into_inner)
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
        let paused = self.gate.paused().await;
        match self.gate.decide(session, &prompt).await {
            Ok(AnswerDecision::Answer { option_index, basis }) if !paused => {
                self.answer(session, &prompt, option_index, &basis).await;
            }
            Ok(AnswerDecision::Answer { option_index, basis }) => {
                self.escalate(
                    session,
                    &prompt,
                    option_index,
                    format!("The supervisor is paused. {basis}"),
                );
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
        if let Err(error) = sent {
            self.escalate(
                session,
                prompt,
                option_index,
                format!("I could not answer it ({error})."),
            );
            return;
        }
        if let Some(known) = self.tracked().get_mut(&session.session_ref) {
            known.settled = true;
        }
        let label = prompt.options.get(option_index).cloned().unwrap_or_default();
        self.write_log(
            session,
            prompt,
            "answer",
            format!("Answered \"{label}\" in {}", session.title),
            basis.to_owned(),
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
        summary: String,
        basis: String,
    ) {
        let now = now_ms();
        let entry = ActionEntry {
            id: 0,
            incident_key: format!("{who}:{}:{}", session.session_ref, prompt.prompt_id),
            session_ref: Some(session.session_ref.clone()),
            kind: ActionKind::Answered,
            summary,
            basis,
            evidence: vec![EvidenceRef {
                kind: "prompt".to_owned(),
                id: prompt.prompt_id.clone(),
            }],
            undo: None,
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
    pub fn peek_owner_answer(&self, backend_session_id: &str, prompt_id: &str) -> Option<OwnerAnswer> {
        let tracked = self.tracked();
        let (session_ref, known) = tracked.iter().find(|(session_ref, known)| {
            session_ref.backend_session_id() == backend_session_id
                && known.prompt.prompt_id == prompt_id
        })?;
        (!known.settled).then(|| OwnerAnswer {
            session_ref: session_ref.clone(),
            session: known.session.clone(),
            prompt: known.prompt.clone(),
            repo: known.repo.clone(),
        })
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
            let basis = if learned {
                "Your answer, remembered for next time"
            } else {
                "Your answer"
            };
            self.write_log(
                &session,
                &prompt,
                "owner",
                format!("You chose {label}"),
                basis.to_owned(),
            )
            .await;
        }
        if self.escalations.remove(&session_ref).is_some() {
            self.publisher.decision(&session_ref, None);
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

#[cfg(test)]
mod tests;
