//! The production autonomy service on real SQLite stores (action log, rules, answer history,
//! pause flag). Only the edge to the agent (`SessionControl`) and the owner channel
//! (`PanelPublisher`) are recorded fakes; nothing here waits on time.
use std::sync::Mutex;

use super::*;
use crate::meta::mhc_supervisor::{
    parse_work_call, FleetSource, SessionTools, WorkToolCall,
};
use crate::supervisor::prompt_judge::{JudgeVerdict, PromptJudge};
use serde_json::json;

#[derive(Default)]
struct Control {
    calls: Mutex<Vec<String>>,
    fail: Mutex<bool>,
    /// Accepts answers without a delivery receipt, like a host that returns none.
    no_receipt: Mutex<bool>,
    /// What a screen read returns; empty for a pane with nothing on it.
    tail: Mutex<String>,
}

impl Control {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl SessionControl for Control {
    async fn stop(&self, session: &SessionRef) -> Result<()> {
        self.calls.lock().unwrap().push(format!("stop {session}"));
        Ok(())
    }

    async fn send(&self, session: &SessionRef, text: &str) -> Result<()> {
        self.calls.lock().unwrap().push(format!("send {session} {text}"));
        Ok(())
    }

    async fn answer(
        &self,
        session: &SessionRef,
        prompt_id: &str,
        _screen_revision: Option<&str>,
        option_index: usize,
    ) -> Result<Option<String>> {
        if *self.fail.lock().unwrap() {
            return Err(crate::error::HubError::message("the pane moved"));
        }
        self.calls
            .lock()
            .unwrap()
            .push(format!("answer {session} {prompt_id} {option_index}"));
        if *self.no_receipt.lock().unwrap() {
            return Ok(None);
        }
        Ok(Some(format!("req-{prompt_id}")))
    }

    async fn read_tail(&self, _session: &SessionRef, _lines: usize) -> Result<String> {
        Ok(self.tail.lock().unwrap().clone())
    }
}

#[derive(Default)]
struct Frames {
    events: Mutex<Vec<String>>,
}

impl Frames {
    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    fn count(&self, prefix: &str) -> usize {
        self.events().iter().filter(|event| event.starts_with(prefix)).count()
    }
}

impl PanelPublisher for Frames {
    fn decision(&self, session_ref: &SessionRef, decision: Option<&Escalation>) {
        let what = if decision.is_some() { "open" } else { "resolved" };
        self.events.lock().unwrap().push(format!("decision {session_ref} {what}"));
    }

    fn action(&self, action: &ActionEntry) {
        self.events.lock().unwrap().push(format!("action {}", action.summary));
    }

    fn rules(&self, rules: &[StandingRule]) {
        self.events.lock().unwrap().push(format!("rules {}", rules.len()));
    }
}

struct Fixture {
    pool: SqlitePool,
    autonomy: Arc<Autonomy>,
    control: Arc<Control>,
    frames: Arc<Frames>,
}

/// A judge that answers with whatever it was told to, and records what it was asked.
struct FakeJudge {
    reply: Mutex<std::result::Result<JudgeVerdict, String>>,
    asked: Mutex<Vec<String>>,
}

impl Default for FakeJudge {
    fn default() -> Self {
        Self {
            reply: Mutex::new(std::result::Result::Err(String::new())),
            asked: Mutex::new(Vec::new()),
        }
    }
}

impl FakeJudge {
    fn answers(index: usize, confidence: f64, reason: &str) -> Arc<Self> {
        Arc::new(Self::with(std::result::Result::Ok(JudgeVerdict {
            option_index: index,
            confidence,
            reason: reason.to_owned(),
        })))
    }

    fn fails(message: &str) -> Arc<Self> {
        Arc::new(Self::with(std::result::Result::Err(message.to_owned())))
    }

    fn with(reply: std::result::Result<JudgeVerdict, String>) -> Self {
        Self {
            reply: Mutex::new(reply),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }

    /// Changes what the engine answers from now on; what it was asked so far is kept.
    fn set(&self, next: Arc<Self>) {
        *self.reply.lock().unwrap() = next.reply.lock().unwrap().clone();
    }
}

#[async_trait]
impl PromptJudge for FakeJudge {
    async fn judge(&self, prompt: &str) -> Result<JudgeVerdict> {
        self.asked.lock().unwrap().push(prompt.to_owned());
        match &*self.reply.lock().unwrap() {
            Ok(verdict) => Ok(verdict.clone()),
            Err(message) => Err(crate::error::HubError::message(message.clone())),
        }
    }
}

/// The health probe the test controls: healthy unless told otherwise.
struct Health(std::sync::atomic::AtomicBool);

impl Health {
    fn healthy() -> Arc<Self> {
        Arc::new(Self(std::sync::atomic::AtomicBool::new(true)))
    }
}

#[async_trait]
impl SupervisorHealth for Health {
    async fn healthy(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Health {
    /// Marks the engine degraded (or healthy again) for observations from now on.
    fn set(&self, healthy: bool) {
        self.0.store(healthy, std::sync::atomic::Ordering::SeqCst);
    }
}

async fn fixture() -> Fixture {
    let pool = crate::storage::memory().await.unwrap();
    let control = Arc::new(Control::default());
    let frames = Arc::new(Frames::default());
    let autonomy = Autonomy::open(pool.clone(), control.clone(), frames.clone())
        .await
        .unwrap();
    Fixture {
        pool,
        autonomy,
        control,
        frames,
    }
}

/// A fixture whose gate has the fake engine behind it, at the health `health` says.
async fn judged(judge: Arc<FakeJudge>, health: Arc<Health>) -> Fixture {
    let pool = crate::storage::memory().await.unwrap();
    let control = Arc::new(Control::default());
    let frames = Arc::new(Frames::default());
    let autonomy = Autonomy::open_with_health(
        pool.clone(),
        control.clone(),
        frames.clone(),
        Some(judge.clone() as Arc<dyn PromptJudge>),
        health as Arc<dyn SupervisorHealth>,
    )
    .await
    .unwrap();
    Fixture {
        pool,
        autonomy,
        control,
        frames,
    }
}

fn question(id: &str, text: &str, command: Option<&str>) -> PendingPrompt {
    PendingPrompt {
        prompt_id: id.to_owned(),
        kind: "question".to_owned(),
        question: text.to_owned(),
        command: command.map(str::to_owned),
        options: vec!["npm".to_owned(), "pnpm".to_owned()],
        screen_revision: Some(format!("rev-{id}")),
    }
}

fn session(prompt: Option<PendingPrompt>) -> FleetSession {
    FleetSession {
        session_ref: SessionRef::new("mac", "s1"),
        host_id: "mac".to_owned(),
        host_label: "This computer".to_owned(),
        title: "ferryx".to_owned(),
        cwd: "/nonexistent/ferryx-autonomy-test".to_owned(),
        agent: "claude".to_owned(),
        state: "blocked".to_owned(),
        state_source: "provider".to_owned(),
        screen_readable: true,
        state_since_ms: None,
        screen_changed_at_ms: None,
        pending_prompt: prompt,
        last_assistant_line: None,
        updated_at_ms: 1,
        stale: false,
        health: None,
        attention: None,
        reason: None,
        branch: None,
    }
}

/// The owner answers `option` of a fresh prompt `n` of the same question three times over.
async fn teach(fixture: &Fixture, option: usize, times: usize) {
    for n in 0..times {
        let prompt = question(&format!("t{n}"), "Which package manager should I use?", None);
        fixture.autonomy.observe(&session(Some(prompt.clone()))).await;
        fixture.autonomy.owner_answered("s1", &prompt.prompt_id, option).await;
    }
}

#[tokio::test]
async fn an_unseen_question_becomes_a_published_decision_with_a_recommendation() {
    let fixture = fixture().await;
    let prompt = question("p1", "Which package manager should I use?", None);
    fixture.autonomy.observe(&session(Some(prompt))).await;

    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(open.len(), 1);
    assert!(open[0].recommended_index < 2);
    assert!(!open[0].reason.is_empty());
    assert_eq!(fixture.frames.count("decision mac:s1 open"), 1);
    assert!(fixture.control.calls().is_empty(), "nothing was answered");

    // The same observation again is not a second card.
    fixture
        .autonomy
        .observe(&session(Some(question("p1", "Which package manager should I use?", None))))
        .await;
    assert_eq!(fixture.frames.count("decision mac:s1 open"), 1);
}

#[tokio::test]
async fn three_owner_answers_teach_it_and_the_fourth_is_answered_with_a_log_line() {
    let fixture = fixture().await;
    teach(&fixture, 1, 3).await;

    // Each owner answer logged "You chose" and resolved its card.
    assert_eq!(fixture.frames.count("action You chose pnpm"), 3);
    assert_eq!(fixture.frames.count("decision mac:s1 resolved"), 3);
    assert!(fixture.autonomy.escalation_source().pending().await.unwrap().is_empty());
    let history = AnswerHistory::new(fixture.pool.clone());
    let probe = question("x", "Which package manager should I use?", None);
    assert_eq!(history.recent_labels(&probe, "/nonexistent/ferryx-autonomy-test", "claude").await.unwrap().len(), 3);

    let fourth = question("p4", "Which package manager should I use?", None);
    fixture.autonomy.observe(&session(Some(fourth))).await;

    assert_eq!(fixture.control.calls(), vec!["answer mac:s1 p4 1".to_owned()]);
    assert!(fixture.autonomy.escalation_source().pending().await.unwrap().is_empty());
    let log = fixture.autonomy.action_log().list_today(0).await.unwrap();
    let answered = log
        .iter()
        .find(|entry| entry.summary.starts_with("Answered"))
        .expect("the autonomous answer is logged");
    assert_eq!(answered.kind, ActionKind::Answered);
    assert!(answered.basis.contains("pnpm"), "{}", answered.basis);
    // The evidence is the request id agent control accepted the answer under, not the prompt id.
    assert_eq!(
        answered.evidence,
        vec![EvidenceRef { kind: "receipt".to_owned(), id: "req-p4".to_owned() }]
    );
    assert_eq!(fixture.frames.count("action Answered"), 1);

    // The supervisor's own answer is never learned as the owner's.
    fixture.autonomy.owner_answered("s1", "p4", 1).await;
    assert_eq!(history.recent_labels(&probe, "/nonexistent/ferryx-autonomy-test", "claude").await.unwrap().len(), 3);
}

#[tokio::test]
async fn an_answer_accepted_without_a_receipt_is_logged_with_no_evidence() {
    let fixture = fixture().await;
    teach(&fixture, 1, 3).await;
    *fixture.control.no_receipt.lock().unwrap() = true;
    let fourth = question("p4", "Which package manager should I use?", None);
    fixture.autonomy.observe(&session(Some(fourth))).await;

    assert_eq!(fixture.control.calls(), vec!["answer mac:s1 p4 1".to_owned()]);
    let log = fixture.autonomy.action_log().list_today(0).await.unwrap();
    let answered = log
        .iter()
        .find(|entry| entry.summary.starts_with("Answered"))
        .expect("the autonomous answer is logged");
    // No receipt came back, so nothing stands in for one: the panel shows it as Unverified.
    assert!(answered.evidence.is_empty(), "{:?}", answered.evidence);
}

#[tokio::test]
async fn an_owner_answer_recorded_twice_is_learned_once() {
    let fixture = fixture().await;
    let prompt = question("p1", "Which package manager should I use?", None);
    fixture.autonomy.observe(&session(Some(prompt.clone()))).await;
    fixture.autonomy.owner_answered("s1", "p1", 1).await;
    // The card route and the terminal observer both report the same answer.
    fixture.autonomy.observe(&session(Some(prompt.clone()))).await;
    fixture.autonomy.owner_answered("s1", "p1", 1).await;

    let history = AnswerHistory::new(fixture.pool.clone());
    let labels = history
        .recent_labels(&prompt, "/nonexistent/ferryx-autonomy-test", "claude")
        .await
        .unwrap();
    assert_eq!(labels, vec!["pnpm".to_owned()]);
}

#[tokio::test]
async fn an_irreversible_step_is_escalated_even_after_three_matching_answers() {
    let fixture = fixture().await;
    for n in 0..3 {
        let prompt = question(&format!("m{n}"), "Apply the migration?", Some("sqlx migrate run --database-url postgres://prod/db"));
        fixture.autonomy.observe(&session(Some(prompt.clone()))).await;
        fixture.autonomy.owner_answered("s1", &prompt.prompt_id, 0).await;
    }
    let again = question("m9", "Apply the migration?", Some("sqlx migrate run --database-url postgres://prod/db"));
    fixture.autonomy.observe(&session(Some(again))).await;

    assert!(fixture.control.calls().is_empty(), "D1a is never answered by the supervisor");
    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(open.len(), 1);
    assert!(open[0].reason.contains("irreversible"), "{}", open[0].reason);
}

#[tokio::test]
async fn pause_stops_autonomous_answers_but_the_card_still_appears() {
    let fixture = fixture().await;
    teach(&fixture, 1, 3).await;
    crate::control::set_paused(&fixture.pool, true).await.unwrap();

    let prompt = question("p9", "Which package manager should I use?", None);
    fixture.autonomy.observe(&session(Some(prompt.clone()))).await;

    assert!(fixture.control.calls().is_empty());
    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].recommended_index, 1);
    let gate = fixture.autonomy.gate();
    let refused = gate.allow(&session(Some(prompt)), &question("p9", "Which package manager should I use?", None), 1).await.unwrap_err();
    assert!(refused.contains("paused"), "{refused}");
}

#[tokio::test]
async fn a_failed_autonomous_answer_becomes_a_decision_not_a_lost_prompt() {
    let fixture = fixture().await;
    teach(&fixture, 1, 3).await;
    *fixture.control.fail.lock().unwrap() = true;

    fixture
        .autonomy
        .observe(&session(Some(question("p5", "Which package manager should I use?", None))))
        .await;

    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(open.len(), 1);
    assert!(open[0].reason.contains("could not answer"), "{}", open[0].reason);
    assert_eq!(fixture.frames.count("action Answered"), 0);
}

#[tokio::test]
async fn a_session_without_a_prompt_or_a_stale_one_changes_nothing_and_clears_its_card() {
    let fixture = fixture().await;
    let prompt = question("p1", "Which package manager should I use?", None);
    fixture.autonomy.observe(&session(Some(prompt.clone()))).await;
    assert_eq!(fixture.autonomy.escalation_source().pending().await.unwrap().len(), 1);

    fixture.autonomy.observe(&session(None)).await;
    assert!(fixture.autonomy.escalation_source().pending().await.unwrap().is_empty());
    assert_eq!(fixture.frames.count("decision mac:s1 resolved"), 1);

    let mut stale = session(Some(question("p2", "Which package manager should I use?", None)));
    stale.stale = true;
    fixture.autonomy.observe(&stale).await;
    assert!(fixture.autonomy.escalation_source().pending().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_rule_change_through_the_real_store_publishes_the_whole_list() {
    let fixture = fixture().await;
    let rules = fixture.autonomy.rule_store();
    let mut first = rules.list().await.unwrap().remove(0);
    first.params = json!({"backoffSecs": [30], "thenEverySecs": 30});
    rules.upsert(first).await.unwrap();

    assert_eq!(fixture.frames.events(), vec!["rules 4".to_owned()]);
    assert_eq!(rules.list().await.unwrap().len(), 4);
}

/// The fleet the session tools read: one blocked session.
struct OneSession(FleetSession);

#[async_trait]
impl FleetSource for OneSession {
    async fn list(&self) -> Result<Vec<FleetSession>> {
        Ok(vec![self.0.clone()])
    }
}

fn answer_call(prompt: &PendingPrompt, option: usize) -> WorkToolCall {
    parse_work_call(&json!({
        "action": "answer_prompt",
        "payload": {
            "sessionRef": "mac:s1",
            "promptId": prompt.prompt_id,
            "screenRevision": prompt.screen_revision,
            "optionIndex": option,
        }
    }))
    .unwrap()
}

#[tokio::test]
async fn the_engine_answer_tool_obeys_the_real_decision_gate() {
    let fixture = fixture().await;
    teach(&fixture, 1, 3).await;
    let prompt = question("e1", "Which package manager should I use?", None);
    let live = session(Some(prompt.clone()));
    let tools = SessionTools::new(fixture.control.clone(), Arc::new(OneSession(live)))
        .with_gate(fixture.autonomy.gate());

    // The learned option passes; another option on a routine prompt is refused.
    tools.execute(answer_call(&prompt, 1)).await.unwrap();
    let refused = tools.execute(answer_call(&prompt, 0)).await.unwrap_err();
    assert_eq!(refused.code, "SUPERVISOR_ANSWER_REFUSED");
    assert_eq!(fixture.control.calls(), vec!["answer mac:s1 e1 1".to_owned()]);

    // A prompt with no precedent is refused whatever option the engine picks.
    let unseen = question("e2", "Which database driver should I use?", None);
    let tools = SessionTools::new(fixture.control.clone(), Arc::new(OneSession(session(Some(unseen.clone())))))
        .with_gate(fixture.autonomy.gate());
    assert_eq!(tools.execute(answer_call(&unseen, 0)).await.unwrap_err().code, "SUPERVISOR_ANSWER_REFUSED");
    assert_eq!(fixture.control.calls().len(), 1);
}

#[tokio::test]
async fn the_gate_uses_the_irreversible_classifier_not_a_word_list() {
    let fixture = fixture().await;
    let gate = fixture.autonomy.gate();
    let risky = |command: &str| question("g1", "Run this?", Some(command));
    for command in [
        "git push --force origin main",
        "rm -rf /var/lib/app",
        "psql -c 'DROP TABLE users'",
        "chmod 777 /etc/passwd",
    ] {
        let prompt = risky(command);
        let refused = gate.allow(&session(Some(prompt.clone())), &prompt, 0).await.unwrap_err();
        assert!(refused.contains("irreversible"), "{command}: {refused}");
    }
    // Whatever the classifier does not know is still never answered without a precedent.
    let prompt = risky("alembic upgrade head");
    assert!(gate.allow(&session(Some(prompt.clone())), &prompt, 0).await.is_err());
    // A prompt with nothing risky in it still needs a precedent: the gate refuses it with the
    // decision's own reason, not the classifier's.
    let plain = question("g2", "Rename the helper module?", None);
    let reason = gate.allow(&session(Some(plain.clone())), &plain, 0).await.unwrap_err();
    assert!(!reason.contains("irreversible"), "{reason}");
}

const POOL_CWD: &str = "/nonexistent/ferryx-autonomy-test";

/// Teaches three owner answers, lets the supervisor answer `p4` on its own, and returns the
/// answered action entry.
async fn autonomous_answer(fixture: &Fixture) -> ActionEntry {
    teach(fixture, 1, 3).await;
    let fourth = question("p4", "Which package manager should I use?", None);
    fixture.autonomy.observe(&session(Some(fourth))).await;
    assert_eq!(fixture.control.calls(), vec!["answer mac:s1 p4 1".to_owned()]);
    fixture
        .autonomy
        .action_log()
        .list_today(0)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.summary.starts_with("Answered"))
        .expect("the autonomous answer is logged")
}

#[tokio::test]
async fn undoing_an_autonomous_answer_restores_the_decision_card_without_a_second_send() {
    let fixture = fixture().await;
    let answered = autonomous_answer(&fixture).await;
    let handle = answered.undo.clone().expect("an autonomous answer carries an undo handle");
    assert_eq!(handle.kind, RESTORE_DECISION);
    assert!(fixture.autonomy.escalation_source().pending().await.unwrap().is_empty());

    // Baselines after the autonomous answer: teaching opened one card per owner answer.
    let calls_before = fixture.control.calls();
    let opened_before = fixture.frames.count("decision mac:s1 open");

    fixture.autonomy.undo(answered.id).await.unwrap();

    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(open.len(), 1, "the item is pending again");
    assert_eq!(open[0].prompt.prompt_id, "p4");
    assert_eq!(open[0].prompt.screen_revision.as_deref(), Some("rev-p4"));
    assert_eq!(open[0].recommended_index, 1);
    assert!(open[0].reason.contains("You chose pnpm the last 3 times"), "{}", open[0].reason);
    assert_eq!(fixture.frames.count("decision mac:s1 open"), opened_before + 1);
    assert_eq!(fixture.control.calls(), calls_before, "undo never answers by itself");

    let entry = fixture
        .autonomy
        .action_log()
        .list_today(0)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.id == answered.id)
        .unwrap();
    assert_eq!(entry.undo, None, "the entry is marked undone");

    // Neither the autonomous answer nor its undo became an owner precedent.
    let history = AnswerHistory::new(fixture.pool.clone());
    let probe = question("x", "Which package manager should I use?", None);
    assert_eq!(history.recent_labels(&probe, POOL_CWD, "claude").await.unwrap().len(), 3);

    // Repeating the undo is a typed refusal and changes nothing.
    assert_eq!(fixture.autonomy.undo(answered.id).await, Err(UndoError::Nothing(answered.id)));
    assert_eq!(fixture.autonomy.escalation_source().pending().await.unwrap().len(), 1);
    assert_eq!(fixture.frames.count("decision mac:s1 open"), opened_before + 1);
    assert_eq!(fixture.control.calls(), calls_before);
}

#[tokio::test]
async fn a_newer_screen_revision_stops_undo_from_resurrecting_the_card() {
    let fixture = fixture().await;
    let answered = autonomous_answer(&fixture).await;

    // The pane moved on: a new prompt, escalated because this one is unlike the precedents.
    let newer = question("p5", "Rename the helper module?", None);
    fixture.autonomy.observe(&session(Some(newer))).await;
    let before = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(before.len(), 1);

    let refused = fixture.autonomy.undo(answered.id).await.unwrap_err();
    assert_eq!(refused, UndoError::Stale(answered.id));
    assert_eq!(refused.code(), "STALE");
    let after = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(after, before, "the newer card is untouched");
    assert_eq!(after[0].prompt.prompt_id, "p5");
    let entry = fixture
        .autonomy
        .action_log()
        .list_today(0)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.id == answered.id)
        .unwrap();
    assert!(entry.undo.is_some(), "a refused undo keeps its handle");
    assert_eq!(fixture.control.calls().len(), 1);
}

#[tokio::test]
async fn undo_of_an_unknown_or_handleless_action_is_not_found() {
    let fixture = fixture().await;
    teach(&fixture, 1, 1).await;
    assert_eq!(fixture.autonomy.undo(9_999).await, Err(UndoError::Nothing(9_999)));
    let owner_line = fixture.autonomy.action_log().list_today(0).await.unwrap().remove(0);
    assert_eq!(fixture.autonomy.undo(owner_line.id).await, Err(UndoError::Nothing(owner_line.id)));
}

#[tokio::test]
async fn an_owner_tap_is_not_gated_the_owner_may_answer_any_prompt() {
    let fixture = fixture().await;
    // The owner answers an irreversible prompt: no gate stands in the way, the choice is
    // logged, and the card resolves. The supervisor's gate would have refused the same prompt.
    let prompt = question("o1", "Deploy to production?", Some("kubectl apply -f prod.yaml"));
    let live = session(Some(prompt.clone()));
    fixture.autonomy.observe(&live).await;
    assert_eq!(fixture.autonomy.escalation_source().pending().await.unwrap().len(), 1);
    assert!(fixture.autonomy.gate().allow(&live, &prompt, 0).await.is_err());

    fixture.autonomy.owner_answered("s1", "o1", 0).await;

    assert!(fixture.autonomy.escalation_source().pending().await.unwrap().is_empty());
    assert_eq!(fixture.frames.count("action You chose npm"), 1);
    assert!(fixture.control.calls().is_empty(), "the owner's tap went to the pane, not through the supervisor");
}

fn recovery_escalation(session_ref: &SessionRef) -> Escalation {
    Escalation {
        session_ref: session_ref.clone(),
        prompt: PendingPrompt {
            prompt_id: "recovery:stall".to_owned(),
            kind: "recovery".to_owned(),
            question: "The session is still stalled after a restart. What now?".to_owned(),
            command: None,
            options: vec!["Restart it again".to_owned(), "Leave it".to_owned()],
            screen_revision: None,
        },
        recommended_index: 0,
        reason: "restarted once, stalled again".to_owned(),
        created_at_ms: 5,
    }
}

#[tokio::test]
async fn a_recovery_escalation_survives_clear_and_is_withdrawn_only_by_recovery() {
    let fixture = fixture().await;
    let session_ref = SessionRef::new("mac", "s1");
    RecoveryEscalations::open(&*fixture.autonomy, recovery_escalation(&session_ref));

    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].reason, "restarted once, stalled again");
    assert_eq!(fixture.frames.count("decision mac:s1 open"), 1);

    // A prompt observation clearing its own card never withdraws the recovery escalation.
    fixture.autonomy.clear(&session_ref);
    fixture.autonomy.observe(&session(None)).await;
    let still_open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(still_open.len(), 1);
    assert_eq!(fixture.frames.count("decision mac:s1 resolved"), 0);

    // Recovery withdraws it once the session is healthy; a second resolve publishes nothing.
    RecoveryEscalations::resolve(&*fixture.autonomy, &session_ref);
    RecoveryEscalations::resolve(&*fixture.autonomy, &session_ref);
    assert!(fixture.autonomy.escalation_source().pending().await.unwrap().is_empty());
    assert_eq!(fixture.frames.count("decision mac:s1 resolved"), 1);
}

/// Owner memory the fake engine must see: one taught line.
async fn taught(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO supervisor_memory_sources(source_id, supervisor_id, owner_principal,
            source_adapter, source_message_id, action, body, body_sha256, state,
            created_at, updated_at)
         SELECT 'src-1', supervisor_id, owner_principal, 'desktop', 'msg-1', 'teach',
                'Bun only in this repo, never npm.', ?, 'ok', '2026-11-15', '2026-11-15'
           FROM supervisor_identity LIMIT 1",
    )
    .bind(crate::usage::sha256_hex("Bun only in this repo, never npm."))
    .execute(pool)
    .await
    .unwrap();
}

/// An identity for the owner: memory rows and goal bindings need one.
async fn owner(pool: &SqlitePool) {
    crate::supervisor::provision_owner(pool, "qa-owner", "ferryx", "github.com/x/ferryx", "mac", "/work/ferryx/.git")
        .await
        .unwrap();
}

/// One admitted goal bound to the pane `s1` on host `mac`, at revision 1, with the owner
/// request `body`, so the gate reads a real binding.
async fn bound_goal(pool: &SqlitePool, body: &str) {
    let identity: (String, String) =
        sqlx::query_as("SELECT supervisor_id, owner_principal FROM supervisor_identity LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap();
    let project: String = sqlx::query_scalar("SELECT project_id FROM supervisor_projects LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();
    let request = crate::supervisor::AdmitRequest {
        supervisor_id: identity.0,
        owner_principal: identity.1,
        source_adapter: crate::supervisor::ChannelProvider::Desktop,
        source_message_id: "msg-goal-1".into(),
        body: body.into(),
        project_id: project,
        channel_binding_id: None,
        target: crate::supervisor::ExecutionTarget {
            kind: crate::supervisor::ExecutionKind::Pty,
            host_id: "mac".into(),
        },
        title: "Wire the fetch cache".into(),
        cwd: POOL_CWD.into(),
        instruction: "Wire the fetch cache.".into(),
        requirements: vec![crate::goals::contract::RequirementSpec {
            id: "r1".into(),
            text: "the fetch cache is wired".into(),
            verifier: "cargo test -p ferryx-hub passes".into(),
        }],
        mode: crate::goals::types::GoalMode::Supervised,
        turn_limit: 5,
        operation_grant: vec![crate::supervisor::admission::OperationClass::Edit],
    };
    crate::supervisor::admit_request_bound(
        pool,
        &request,
        &crate::supervisor::AdmitTerms::default(),
        Some(&crate::supervisor::BoundSession {
            host_id: "mac".into(),
            session_id: "s1".into(),
        }),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_confident_engine_verdict_answers_the_prompt_and_logs_the_basis() {
    let judge = FakeJudge::answers(1, 0.9, "Bun only in this repo");
    let fixture = judged(judge.clone(), Health::healthy()).await;
    owner(&fixture.pool).await;

    fixture
        .autonomy
        .observe(&session(Some(question("j1", "Which package manager should I use?", None))))
        .await;

    assert_eq!(fixture.control.calls(), vec!["answer mac:s1 j1 1".to_owned()]);
    assert_eq!(fixture.autonomy.escalation_source().pending().await.unwrap().len(), 0);
    let log = fixture.autonomy.action_log().list_today(0).await.unwrap();
    let answered = log
        .iter()
        .find(|entry| entry.summary.starts_with("Answered"))
        .expect("the judged answer is logged");
    assert_eq!(answered.basis, "Judged from your memory: Bun only in this repo");
    // The undo of an autonomous answer restores the card, exactly as a learned one does.
    assert_eq!(answered.undo.as_ref().map(|undo| undo.kind.as_str()), Some(RESTORE_DECISION));
}

#[tokio::test]
async fn a_low_confidence_verdict_escalates_with_the_engine_choice_as_recommendation() {
    let judge = FakeJudge::answers(1, 0.79, "probably bun");
    let fixture = judged(judge, Health::healthy()).await;
    owner(&fixture.pool).await;

    fixture
        .autonomy
        .observe(&session(Some(question("j2", "Which package manager should I use?", None))))
        .await;

    assert!(fixture.control.calls().is_empty(), "nothing is typed below the threshold");
    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].recommended_index, 1);
    assert!(open[0].reason.contains("probably bun"), "{}", open[0].reason);
}

#[tokio::test]
async fn a_failed_engine_call_escalates_and_answers_nothing() {
    let judge = FakeJudge::fails("the engine exited");
    let fixture = judged(judge, Health::healthy()).await;
    owner(&fixture.pool).await;

    fixture
        .autonomy
        .observe(&session(Some(question("j3", "Which package manager should I use?", None))))
        .await;

    assert!(fixture.control.calls().is_empty());
    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(fixture.frames.count("action Answered"), 0);
}

#[tokio::test]
async fn an_irreversible_prompt_never_reaches_the_engine() {
    let judge = FakeJudge::answers(0, 0.99, "yes, force push");
    let fixture = judged(judge.clone(), Health::healthy()).await;
    owner(&fixture.pool).await;

    let prompt = question("j4", "Run it?", Some("git push --force origin main"));
    fixture.autonomy.observe(&session(Some(prompt))).await;

    assert!(judge.asked().is_empty(), "the engine was never asked");
    assert!(fixture.control.calls().is_empty());
    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert!(open[0].reason.contains("irreversible"), "{}", open[0].reason);
}

#[tokio::test]
async fn a_screen_sourced_approval_never_reaches_the_engine_and_types_nothing() {
    let judge = FakeJudge::answers(0, 0.99, "approve it");
    let fixture = judged(judge.clone(), Health::healthy()).await;
    owner(&fixture.pool).await;

    let mut approval = question("j5", "Do you want to proceed?", Some("cargo test -p ferryx-hub"));
    approval.kind = "approval".into();
    let mut screen = session(Some(approval));
    screen.state_source = "screen".into();
    fixture.autonomy.observe(&screen).await;

    assert!(judge.asked().is_empty(), "a screen-sourced approval is the owner's");
    assert!(fixture.control.calls().is_empty(), "0 keystrokes");
    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(open.len(), 1);
}

#[tokio::test]
async fn an_unreadable_screen_is_held_and_types_nothing_even_with_a_confident_verdict() {
    let judge = FakeJudge::answers(1, 0.99, "bun");
    let fixture = judged(judge, Health::healthy()).await;
    owner(&fixture.pool).await;

    let mut blind = session(Some(question("j6", "Which package manager should I use?", None)));
    blind.screen_readable = false;
    fixture.autonomy.observe(&blind).await;

    assert!(fixture.control.calls().is_empty());
    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert!(open[0].reason.contains("could not read"), "{}", open[0].reason);
}

#[tokio::test]
async fn a_provider_sourced_approval_that_names_its_command_may_be_answered() {
    let judge = FakeJudge::answers(0, 0.9, "you always approve cargo test");
    let fixture = judged(judge, Health::healthy()).await;
    owner(&fixture.pool).await;

    let mut approval = question("j7", "Do you want to proceed?", Some("cargo test -p ferryx-hub"));
    approval.kind = "approval".into();
    fixture.autonomy.observe(&session(Some(approval))).await;

    assert_eq!(fixture.control.calls(), vec!["answer mac:s1 j7 0".to_owned()]);
}

#[tokio::test]
async fn a_malformed_verdict_escalates_instead_of_answering() {
    // The engine replied with prose: the strict parser refuses it, so the prompt escalates.
    let judge = FakeJudge::fails("the judge's verdict is not strict JSON");
    let fixture = judged(judge, Health::healthy()).await;
    owner(&fixture.pool).await;

    fixture
        .autonomy
        .observe(&session(Some(question("j8", "Which package manager should I use?", None))))
        .await;

    assert!(fixture.control.calls().is_empty());
    assert_eq!(fixture.autonomy.escalation_source().pending().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_verdict_naming_an_option_that_does_not_exist_escalates() {
    let judge = FakeJudge::answers(7, 0.99, "option seven");
    let fixture = judged(judge, Health::healthy()).await;
    owner(&fixture.pool).await;

    fixture
        .autonomy
        .observe(&session(Some(question("j9", "Which package manager should I use?", None))))
        .await;

    assert!(fixture.control.calls().is_empty());
    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    // No recommendation from an index that is not an option.
    assert!(open[0].recommended_index < 2, "{}", open[0].recommended_index);
}

#[tokio::test]
async fn a_degraded_engine_escalates_every_prompt_including_learned_ones() {
    // While the owner teaches, the engine is unsure, so the three answers are the owner's.
    let judge = FakeJudge::answers(1, 0.5, "not sure yet");
    let health = Health::healthy();
    let fixture = judged(judge.clone(), health.clone()).await;
    owner(&fixture.pool).await;
    teach(&fixture, 1, 3).await;
    judge.set(FakeJudge::answers(1, 0.99, "pnpm"));
    health.set(false);
    // Teaching ran while healthy, so the engine was asked then; only what it is asked from here
    // on tells whether a degraded engine is consulted.
    let asked_before = judge.asked().len();

    fixture
        .autonomy
        .observe(&session(Some(question("j10", "Which package manager should I use?", None))))
        .await;

    assert!(fixture.control.calls().is_empty(), "nothing was typed");
    assert_eq!(judge.asked().len(), asked_before, "a degraded engine is not asked");
    let open = fixture.autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(open.len(), 1);
    assert!(open[0].reason.starts_with("supervisor degraded:"), "{}", open[0].reason);
    // The learned recommendation still travels with the card.
    assert_eq!(open[0].recommended_index, 1);
}

#[tokio::test]
async fn the_prompt_shows_the_engine_the_owner_memory_the_goal_and_the_screen() {
    let judge = FakeJudge::answers(1, 0.9, "bun");
    let fixture = judged(judge.clone(), Health::healthy()).await;
    owner(&fixture.pool).await;
    taught(&fixture.pool).await;
    bound_goal(&fixture.pool, "Add the fetch cache behind the flag").await;

    *fixture.control.tail.lock().unwrap() =
        "? Which package manager should I use?\n  1. npm\n  2. pnpm".to_owned();
    fixture
        .autonomy
        .observe(&session(Some(question("j11", "Which package manager should I use?", None))))
        .await;

    let asked = judge.asked();
    assert_eq!(asked.len(), 1);
    let prompt = &asked[0];
    for needle in [
        "Bun only in this repo, never npm.",
        "Add the fetch cache behind the flag",
        "the fetch cache is wired",
        "question: Which package manager should I use?",
        "option 0: npm",
        "option 1: pnpm",
        "kind: question",
        "## Screen around the prompt",
        "  2. pnpm",
    ] {
        assert!(prompt.contains(needle), "missing {needle:?} in\n{prompt}");
    }
}

#[tokio::test]
async fn the_goal_contract_answers_before_the_engine_is_asked() {
    let judge = FakeJudge::answers(0, 0.99, "npm");
    let fixture = judged(judge.clone(), Health::healthy()).await;
    owner(&fixture.pool).await;
    bound_goal(&fixture.pool, "Use pnpm").await;
    let goal_id: String = sqlx::query_scalar("SELECT goal_id FROM supervisor_execution_bindings WHERE provider_session_id = 's1'")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO supervisor_goal_decisions(goal_id, goal_revision, prompt_id, option_label, source, created_at)
         VALUES(?, 1, 'd1', 'pnpm', 'owner', '2026-11-15')",
    )
    .bind(&goal_id)
    .execute(&fixture.pool)
    .await
    .unwrap();

    fixture
        .autonomy
        .observe(&session(Some(question("d1", "Which package manager should I use?", None))))
        .await;

    assert!(judge.asked().is_empty(), "the contract settled it; the engine was not asked");
    assert_eq!(fixture.control.calls(), vec!["answer mac:s1 d1 1".to_owned()]);
    let log = fixture.autonomy.action_log().list_today(0).await.unwrap();
    let answered = log.iter().find(|entry| entry.summary.starts_with("Answered")).unwrap();
    assert!(answered.basis.contains("goal contract"), "{}", answered.basis);
}

#[tokio::test]
async fn a_decision_of_an_older_goal_revision_does_not_answer() {
    let judge = FakeJudge::answers(1, 0.9, "pnpm");
    let fixture = judged(judge.clone(), Health::healthy()).await;
    owner(&fixture.pool).await;
    bound_goal(&fixture.pool, "Use pnpm").await;
    let goal_id: String = sqlx::query_scalar("SELECT goal_id FROM supervisor_execution_bindings WHERE provider_session_id = 's1'")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO supervisor_goal_decisions(goal_id, goal_revision, prompt_id, option_label, source, created_at)
         VALUES(?, 2, 'd2', 'npm', 'owner', '2026-11-15')",
    )
    .bind(&goal_id)
    .execute(&fixture.pool)
    .await
    .unwrap();

    fixture
        .autonomy
        .observe(&session(Some(question("d2", "Which package manager should I use?", None))))
        .await;

    // The stale revision is ignored: the engine judged, and its answer stands.
    assert_eq!(judge.asked().len(), 1);
    assert_eq!(fixture.control.calls(), vec!["answer mac:s1 d2 1".to_owned()]);
}

#[tokio::test]
async fn the_gate_never_asks_the_engine_for_a_supervisor_initiated_answer() {
    let judge = FakeJudge::answers(1, 0.99, "pnpm");
    let fixture = judged(judge.clone(), Health::healthy()).await;
    owner(&fixture.pool).await;

    let prompt = question("j12", "Which package manager should I use?", None);
    let refused = fixture
        .autonomy
        .gate()
        .allow(&session(Some(prompt.clone())), &prompt, 1)
        .await
        .unwrap_err();
    assert!(!refused.is_empty());
    assert!(judge.asked().is_empty(), "the gate stays rules, contract and history");
}

/// An engine that replies in prose, like a model that ignored the output contract.
struct ProseJudge;

#[async_trait]
impl PromptJudge for ProseJudge {
    async fn judge(&self, _prompt: &str) -> Result<JudgeVerdict> {
        crate::supervisor::prompt_judge::parse_verdict("I would pick option 2, Bun.")
    }
}

#[tokio::test]
async fn the_engine_prose_instead_of_json_of_a_verdict_escalates() {
    let fixture = judged(Arc::new(FakeJudge::with(Ok(JudgeVerdict {
        option_index: 1,
        confidence: 0.9,
        reason: "unused".into(),
    }))), Health::healthy()).await;
    // The real parser, on a real model reply, is what refuses it.
    let autonomy = Autonomy::open_with_health(
        fixture.pool.clone(),
        fixture.control.clone(),
        fixture.frames.clone(),
        Some(Arc::new(ProseJudge)),
        Health::healthy(),
    )
    .await
    .unwrap();
    owner(&fixture.pool).await;

    autonomy
        .observe(&session(Some(question("j13", "Which package manager should I use?", None))))
        .await;

    assert!(fixture.control.calls().is_empty(), "nothing is typed");
    let open = autonomy.escalation_source().pending().await.unwrap();
    assert_eq!(open.len(), 1);
}
