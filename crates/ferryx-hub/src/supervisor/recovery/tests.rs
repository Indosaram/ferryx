//! Deterministic recovery tests: a fake clock the test moves, a fake `SessionControl` that
//! scripts terminal output, an in-memory action log. Nothing sleeps or waits on time.
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;

use super::*;
use crate::error::HubError;
use crate::supervisor::health::SessionHealthClassifier;
use crate::supervisor::panel_contract::{
    default_rules, ActionEntry, ActionKind, FleetSession, RuleKind, StandingRule,
};

const T0: i64 = 1_800_000_000_000;
const MIN: i64 = 60_000;

struct FakeClock(AtomicI64);
impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct Pause(AtomicBool);
#[async_trait]
impl PauseGate for Pause {
    async fn paused(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct Log {
    rows: Mutex<Vec<ActionEntry>>,
    writes: Mutex<usize>,
}
#[async_trait]
impl ActionLog for Log {
    async fn upsert_incident(&self, entry: ActionEntry) -> Result<ActionEntry> {
        *self.writes.lock().unwrap() += 1;
        let mut rows = self.rows.lock().unwrap();
        match rows.iter_mut().find(|r| r.incident_key == entry.incident_key) {
            Some(row) => *row = entry.clone(),
            None => rows.push(entry.clone()),
        }
        Ok(entry)
    }
    async fn list_today(&self, _since_ms: i64) -> Result<Vec<ActionEntry>> {
        Ok(self.rows.lock().unwrap().clone())
    }
    async fn mark_undone(&self, _id: i64, _now_ms: i64) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct Rules(Mutex<Vec<StandingRule>>);
#[async_trait]
impl RuleStore for Rules {
    async fn list(&self) -> Result<Vec<StandingRule>> {
        let mut rules = default_rules();
        for over in self.0.lock().unwrap().iter() {
            match rules.iter_mut().find(|r| r.kind == over.kind) {
                Some(rule) => rule.params = over.params.clone(),
                None => rules.push(over.clone()),
            }
        }
        Ok(rules)
    }
    async fn upsert(&self, _rule: StandingRule) -> Result<StandingRule> {
        Err(HubError::message("read-only"))
    }
}

#[derive(Default)]
struct Cards {
    opened: Mutex<Vec<Escalation>>,
    resolved: Mutex<Vec<SessionRef>>,
}
impl RecoveryEscalations for Cards {
    fn open(&self, escalation: Escalation) {
        self.opened.lock().unwrap().push(escalation);
    }
    fn resolve(&self, session: &SessionRef) {
        self.resolved.lock().unwrap().push(session.clone());
    }
}

/// Scripted terminal: while `up` is false a send adds another error line, else it adds a
/// working line (the old error stays in the scrollback).
#[derive(Default)]
struct World {
    tails: Mutex<HashMap<SessionRef, String>>,
    /// Sessions whose terminal cannot be read right now.
    unreadable: Mutex<HashSet<SessionRef>>,
    /// While set, `send` fails (nothing typed).
    fail_send: AtomicBool,
    up: AtomicBool,
    /// While set, a send produces no output at all.
    mute: AtomicBool,
    /// While set, a send only produces a spinner/status line (neither an error nor work).
    spinner: AtomicBool,
    ops: Mutex<Vec<String>>,
    error: Mutex<String>,
}
impl World {
    fn push(&self, session: &SessionRef, line: &str) {
        let mut tails = self.tails.lock().unwrap();
        tails.entry(session.clone()).or_default().push_str(&format!("{line}\n"));
    }
    fn ops(&self) -> Vec<String> {
        self.ops.lock().unwrap().clone()
    }
    fn sends_to(&self, r: &SessionRef) -> usize {
        let want = format!("send {r} continue");
        self.ops().iter().filter(|op| **op == want).count()
    }
}
#[async_trait]
impl SessionControl for World {
    async fn stop(&self, session: &SessionRef) -> Result<()> {
        self.ops.lock().unwrap().push(format!("stop {session}"));
        Ok(())
    }
    async fn send(&self, session: &SessionRef, text: &str) -> Result<()> {
        if self.fail_send.load(Ordering::SeqCst) {
            return Err(HubError::message("send failed"));
        }
        let n = {
            let mut ops = self.ops.lock().unwrap();
            ops.push(format!("send {session} {text}"));
            ops.len()
        };
        let line = if self.up.load(Ordering::SeqCst) {
            format!("continuing the work ({n})")
        } else {
            format!("{} ({n})", self.error.lock().unwrap())
        };
        if self.mute.load(Ordering::SeqCst) {
            return Ok(());
        }
        if self.spinner.load(Ordering::SeqCst) {
            self.push(session, &format!("* Thinking... ({n})"));
            return Ok(());
        }
        let mut tails = self.tails.lock().unwrap();
        let tail = tails.entry(session.clone()).or_default();
        tail.push_str(&format!("{line}\n"));
        Ok(())
    }
    async fn answer(&self, _: &SessionRef, _: &str, _: Option<&str>, _: usize) -> Result<()> {
        Ok(())
    }
    async fn read_tail(&self, session: &SessionRef, _lines: usize) -> Result<String> {
        if self.unreadable.lock().unwrap().contains(session) {
            return Err(HubError::message("terminal unreadable"));
        }
        Ok(self.tails.lock().unwrap().get(session).cloned().unwrap_or_default())
    }
}

struct Fleet(Mutex<Vec<FleetSession>>);
#[async_trait]
impl FleetSource for Fleet {
    async fn list(&self) -> Result<Vec<FleetSession>> {
        Ok(self.0.lock().unwrap().clone())
    }
}

struct Retry(Mutex<Vec<String>>);
#[async_trait]
impl LoopRetry for Retry {
    async fn retry(&self, lane_key: &str) -> Result<()> {
        self.0.lock().unwrap().push(lane_key.to_owned());
        Ok(())
    }
}

/// A goal retry that answers from a script: errors, then `Resumed` or `NothingToResume`.
struct ScriptedRetry {
    calls: Mutex<Vec<String>>,
    script: Mutex<Vec<Result<RetryOutcome>>>,
}
#[async_trait]
impl LoopRetry for ScriptedRetry {
    async fn retry(&self, lane_key: &str) -> Result<()> {
        self.retry_outcome(lane_key).await.map(|_| ())
    }
    async fn retry_outcome(&self, lane_key: &str) -> Result<RetryOutcome> {
        self.calls.lock().unwrap().push(lane_key.to_owned());
        let mut script = self.script.lock().unwrap();
        if script.is_empty() {
            Err(HubError::message("still limited"))
        } else {
            script.remove(0)
        }
    }
}

fn session(host: &str, id: &str, agent: &str, state: &str, source: &str, changed: i64) -> FleetSession {
    FleetSession {
        session_ref: SessionRef::new(host, id),
        host_id: host.to_owned(),
        host_label: host.to_owned(),
        title: format!("work {id}"),
        cwd: "/nonexistent/recovery".to_owned(),
        agent: agent.to_owned(),
        state: state.to_owned(),
        state_source: source.to_owned(),
        state_since_ms: Some(changed),
        screen_changed_at_ms: Some(changed),
        pending_prompt: None,
        last_assistant_line: None,
        updated_at_ms: changed,
        stale: false,
    }
}

struct Rig {
    sched: Arc<RecoveryScheduler>,
    fleet: Arc<Fleet>,
    rules: Arc<Rules>,
    refs: Vec<SessionRef>,
    clock: Arc<FakeClock>,
    pause: Arc<Pause>,
    log: Arc<Log>,
    cards: Arc<Cards>,
    world: Arc<World>,
}

fn rig(sessions: Vec<FleetSession>, initial_tail: &str) -> Rig {
    rig_with(sessions, initial_tail, Arc::new(Log::default()), Arc::new(FakeClock(AtomicI64::new(T0))))
}

/// A rig over an existing log and clock: a second scheduler over the first one's rows is a
/// restarted daemon.
fn rig_with(sessions: Vec<FleetSession>, initial_tail: &str, log: Arc<Log>, clock: Arc<FakeClock>) -> Rig {
    let world = Arc::new(World::default());
    *world.error.lock().unwrap() = initial_tail.lines().last().unwrap_or("").to_owned();
    for s in &sessions {
        world
            .tails
            .lock()
            .unwrap()
            .insert(s.session_ref.clone(), format!("{initial_tail}\n"));
    }
    let pause = Arc::new(Pause(AtomicBool::new(false)));
    let cards = Arc::new(Cards::default());
    let rules = Arc::new(Rules::default());
    let refs: Vec<SessionRef> = sessions.iter().map(|s| s.session_ref.clone()).collect();
    let fleet = Arc::new(Fleet(Mutex::new(sessions)));
    let sched = Arc::new(RecoveryScheduler::new(RecoveryDeps {
        fleet: fleet.clone(),
        control: world.clone(),
        rules: rules.clone(),
        log: log.clone(),
        classifier: Arc::new(SessionHealthClassifier),
        clock: clock.clone(),
        pause: pause.clone(),
        escalations: cards.clone(),
    }));
    Rig { sched, rules, fleet, refs, clock, pause, log, cards, world }
}

impl Rig {
    fn set_rule(&self, kind: RuleKind, params: serde_json::Value) {
        self.rules.0.lock().unwrap().push(StandingRule {
            id: 99,
            scope: Default::default(),
            kind,
            params,
            created_from_message_id: None,
            created_at_ms: 0,
        });
    }
    /// A fresh fault line appears in every session after the baseline pass (the initial tail
    /// is only old scrollback, which recovery must never act on).
    fn outbreak(&self, line: &str) {
        for r in &self.refs {
            self.world.push(r, line);
        }
    }
    async fn step(&self, by_ms: i64) {
        self.clock.0.fetch_add(by_ms, Ordering::SeqCst);
        self.sched.tick().await;
    }
    fn rows(&self) -> Vec<ActionEntry> {
        self.log.rows.lock().unwrap().clone()
    }
    fn opened(&self) -> Vec<Escalation> {
        self.cards.opened.lock().unwrap().clone()
    }
}

const BAD_GATEWAY: &str = "API Error: 502 Bad Gateway";

fn four_claude_sessions() -> Vec<FleetSession> {
    (1..=4)
        .map(|n| session("mac-2", &format!("s{n}"), "claude", "working", "screen", T0))
        .collect()
}

#[tokio::test]
async fn a_twenty_minute_outage_resumes_all_four_sessions_with_one_row_and_no_card() {
    let rig = rig(four_claude_sessions(), BAD_GATEWAY);
    rig.sched.tick().await;
    rig.outbreak(BAD_GATEWAY);
    rig.sched.tick().await;
    let mut elapsed = 0;
    while elapsed < 18 * MIN {
        rig.step(30_000).await;
        elapsed += 30_000;
        assert!(rig.opened().is_empty(), "no card while handling");
    }
    let failed_probes = rig.world.ops().len();
    assert!(failed_probes >= 3, "retried on backoff, not once: {failed_probes}");
    assert!(failed_probes <= 6, "backoff is bounded: {failed_probes}");
    rig.world.up.store(true, Ordering::SeqCst);
    for _ in 0..200 {
        if rig.rows().iter().any(|row| !row.live) {
            break;
        }
        rig.step(30_000).await;
    }
    for n in 1..=4 {
        let r = SessionRef::new("mac-2", &format!("s{n}"));
        assert!(rig.world.sends_to(&r) >= 1, "{r} was resumed");
    }
    assert!(rig.world.ops().iter().all(|op| op.starts_with("send ") && op.ends_with(" continue")));
    let rows = rig.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].kind, ActionKind::Resumed);
    assert!(!rows[0].live);
    assert_eq!(rows[0].evidence.len(), 4);
    assert!(rig.opened().is_empty(), "no card, no push");
}

#[tokio::test]
async fn an_outage_past_the_budget_escalates_once_with_a_recommendation() {
    let rig = rig(four_claude_sessions(), BAD_GATEWAY);
    rig.sched.tick().await;
    rig.outbreak(BAD_GATEWAY);
    rig.sched.tick().await;
    let mut elapsed = 0;
    while elapsed < BUDGET_MS - 30_000 {
        rig.step(30_000).await;
        elapsed += 30_000;
    }
    assert!(rig.opened().is_empty(), "nothing before the budget");
    for _ in 0..40 {
        rig.step(30_000).await;
    }
    let opened = rig.opened();
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert!(opened[0].recommended_index < opened[0].prompt.options.len());
    assert!(!opened[0].reason.is_empty());
    assert_eq!(rig.rows().len(), 1);
    assert_eq!(rig.rows()[0].kind, ActionKind::Escalated);
}

#[tokio::test]
async fn a_429_waits_for_the_reset_and_never_rereads_old_output() {
    let rig = rig(
        vec![session("mac-2", "s1", "claude", "working", "screen", T0)],
        "Usage limit reached. Try again in 3 minutes",
    );
    rig.world.up.store(true, Ordering::SeqCst);
    rig.sched.tick().await;
    rig.outbreak("Usage limit reached. Try again in 3 minutes");
    rig.sched.tick().await;
    rig.step(2 * MIN).await;
    assert!(rig.world.ops().is_empty(), "still before the reset");
    rig.step(61_000).await;
    assert_eq!(rig.world.ops(), vec!["send mac-2:s1 continue".to_owned()]);
    // The old limit line is still in the scrollback; only fresh lines are read.
    rig.step(31_000).await;
    rig.step(31_000).await;
    assert_eq!(rig.world.ops().len(), 1, "the old 429 line was not read twice");
    let rows = rig.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, ActionKind::Resumed);
    assert!(rig.opened().is_empty());
}

#[tokio::test]
async fn a_repeated_stall_cancels_and_resumes_once_then_escalates_once() {
    let stalled = session("mac-2", "s1", "claude", "working", "provider", T0 - 10 * MIN);
    let rig = rig(vec![stalled], "thinking...");
    rig.sched.tick().await;
    rig.step(1_000).await;
    assert_eq!(
        rig.world.ops(),
        vec!["stop mac-2:s1".to_owned(), "send mac-2:s1 continue".to_owned()]
    );
    assert!(rig.opened().is_empty());
    for _ in 0..30 {
        rig.step(30_000).await;
    }
    assert_eq!(rig.world.ops().len(), 2, "resumed once only");
    let opened = rig.opened();
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert!(opened[0].recommended_index < opened[0].prompt.options.len());
    assert!(!opened[0].reason.is_empty());
    assert_eq!(rig.rows().len(), 1);
}

#[tokio::test]
async fn pause_stops_every_recovery_action() {
    let rig = rig(four_claude_sessions(), BAD_GATEWAY);
    rig.pause.0.store(true, Ordering::SeqCst);
    rig.world.up.store(true, Ordering::SeqCst);
    rig.sched.report_goal_quota("lane", None, "Try again in 1 minutes").await;
    let retry = Arc::new(Retry(Mutex::new(Vec::new())));
    rig.sched.set_loop_retry(retry.clone());
    let writes = *rig.log.writes.lock().unwrap();
    for _ in 0..80 {
        rig.step(30_000).await;
    }
    assert!(rig.world.ops().is_empty());
    assert!(retry.0.lock().unwrap().is_empty());
    assert!(rig.opened().is_empty());
    assert_eq!(*rig.log.writes.lock().unwrap(), writes, "no row written while paused");
    rig.pause.0.store(false, Ordering::SeqCst);
    rig.step(MIN).await;
    assert_eq!(retry.0.lock().unwrap().as_slice(), ["lane".to_owned()]);
}

#[tokio::test]
async fn an_unknown_agent_is_refused_with_a_typed_error_and_nothing_is_sent() {
    assert_eq!(
        resume_for("omo"),
        Err(Refusal::UnknownAgent { agent: "omo".to_owned() })
    );
    let rig = rig(
        vec![session("mac-2", "s1", "omo", "working", "screen", T0)],
        BAD_GATEWAY,
    );
    rig.sched.tick().await;
    rig.outbreak(BAD_GATEWAY);
    for _ in 0..40 {
        rig.step(30_000).await;
    }
    assert!(rig.world.ops().is_empty(), "no guessed resume key");
    assert_eq!(rig.opened().len(), 1);
    assert_eq!(rig.rows().len(), 1);
    assert_eq!(rig.rows()[0].kind, ActionKind::Escalated);
}

#[tokio::test]
async fn a_goal_quota_fault_is_retried_at_the_parsed_reset_with_one_row() {
    let rig = rig(Vec::new(), "");
    let retry = Arc::new(Retry(Mutex::new(Vec::new())));
    rig.sched.set_loop_retry(retry.clone());
    rig.sched.report_goal_quota("lane-1", None, "429 usage limit. Try again in 5 minutes").await;
    rig.step(4 * MIN).await;
    assert!(retry.0.lock().unwrap().is_empty());
    rig.step(2 * MIN).await;
    assert_eq!(retry.0.lock().unwrap().as_slice(), ["lane-1".to_owned()]);
    let rows = rig.rows();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].live);
}

#[tokio::test]
async fn an_error_already_in_the_scrollback_at_startup_drives_no_recovery() {
    let rig = rig(four_claude_sessions(), BAD_GATEWAY);
    for _ in 0..60 {
        rig.step(30_000).await;
    }
    assert!(rig.world.ops().is_empty(), "old scrollback is not news");
    assert!(rig.rows().is_empty());
    assert!(rig.opened().is_empty());
}

#[tokio::test]
async fn a_session_that_lost_its_baseline_does_not_replay_its_old_fault() {
    let rig = rig(
        vec![session("mac-2", "s1", "claude", "working", "screen", T0)],
        BAD_GATEWAY,
    );
    rig.sched.tick().await;
    let kept = rig.fleet.0.lock().unwrap().clone();
    // The session leaves the fleet (baseline lost) and comes back with the same scrollback.
    rig.fleet.0.lock().unwrap().clear();
    rig.step(30_000).await;
    *rig.fleet.0.lock().unwrap() = kept;
    for _ in 0..60 {
        rig.step(30_000).await;
    }
    assert!(rig.world.ops().is_empty());
    assert!(rig.rows().is_empty());
}

#[tokio::test]
async fn thirty_seconds_of_silence_after_a_resume_does_not_close_the_outage() {
    let rig = rig(
        vec![session("mac-2", "s1", "claude", "working", "screen", T0)],
        BAD_GATEWAY,
    );
    rig.sched.tick().await;
    rig.outbreak(BAD_GATEWAY);
    rig.world.mute.store(true, Ordering::SeqCst);
    rig.sched.tick().await;
    rig.step(61_000).await;
    assert_eq!(rig.world.ops().len(), 1, "first probe sent");
    rig.step(31_000).await;
    rig.step(31_000).await;
    let rows = rig.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0].live, "silence is not a recovery: {rows:?}");
    assert_ne!(rows[0].kind, ActionKind::Resumed);
    // Output returns: the same incident, same row, now closed.
    rig.world.mute.store(false, Ordering::SeqCst);
    rig.world.up.store(true, Ordering::SeqCst);
    for _ in 0..80 {
        if rig.rows().iter().any(|row| !row.live) {
            break;
        }
        rig.step(30_000).await;
    }
    let rows = rig.rows();
    assert_eq!(rows.len(), 1, "one incident, one row: {rows:?}");
    assert_eq!(rows[0].kind, ActionKind::Resumed);
    assert!(!rows[0].live);
}

fn provider_working(screen_changed_at: i64) -> FleetSession {
    session("mac-2", "s1", "claude", "working", "provider", screen_changed_at)
}

#[tokio::test]
async fn a_stall_after_the_restart_screen_change_stays_in_one_incident_and_escalates_once() {
    let rig = rig(vec![provider_working(T0 - 10 * MIN)], "thinking...");
    rig.sched.tick().await;
    assert_eq!(rig.world.ops().len(), 2, "one Stop and one Send");
    // The Stop and Send moved the screen to the restart time; it then freezes again.
    *rig.fleet.0.lock().unwrap() = vec![provider_working(rig.clock.0.load(Ordering::SeqCst))];
    for _ in 0..40 {
        rig.step(30_000).await;
    }
    assert_eq!(rig.world.ops().len(), 2, "exactly one Stop+Send: {:?}", rig.world.ops());
    assert_eq!(rig.opened().len(), 1, "{:?}", rig.opened());
    assert_eq!(rig.rows().len(), 1, "{:?}", rig.rows());
    assert_eq!(rig.rows()[0].kind, ActionKind::Escalated);
}

#[tokio::test]
async fn a_stall_after_confirmed_recovery_gets_a_fresh_restart() {
    let rig = rig(vec![provider_working(T0 - 10 * MIN)], "thinking...");
    rig.sched.tick().await;
    assert_eq!(rig.world.ops().len(), 2);
    // The turn settles: the provider leaves Working, which confirms the recovery.
    let mut idle = provider_working(T0);
    idle.state = "idle".to_owned();
    *rig.fleet.0.lock().unwrap() = vec![idle];
    rig.step(MIN).await;
    let rows = rig.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, ActionKind::Resumed);
    assert!(!rows[0].live);
    // Much later a distinct stall: a fresh incident with a fresh restart, no card.
    rig.step(60 * MIN).await;
    *rig.fleet.0.lock().unwrap() = vec![provider_working(T0)];
    rig.step(MIN).await;
    assert_eq!(rig.world.ops().len(), 4, "{:?}", rig.world.ops());
    assert!(rig.opened().is_empty());
    let rows = rig.rows();
    assert_eq!(rows.len(), 2, "unique incident keys: {rows:?}");
    assert_ne!(rows[0].incident_key, rows[1].incident_key);
}

#[tokio::test]
async fn a_spinner_after_the_send_is_not_recovery_while_the_outage_continues() {
    let rig = rig(four_claude_sessions(), BAD_GATEWAY);
    rig.sched.tick().await;
    rig.outbreak(BAD_GATEWAY);
    rig.world.spinner.store(true, Ordering::SeqCst);
    rig.sched.tick().await;
    rig.step(61_000).await;
    assert_eq!(rig.world.ops().len(), 1, "first probe only: {:?}", rig.world.ops());
    // The spinner shows right after the send; the 502 arrives on a later pass.
    rig.step(10_000).await;
    rig.world.spinner.store(false, Ordering::SeqCst);
    rig.outbreak(BAD_GATEWAY);
    for _ in 0..3 {
        rig.step(30_000).await;
    }
    assert_eq!(rig.world.ops().len(), 1, "no sweep while the outage continues");
    let rows = rig.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0].live, "{rows:?}");
    assert_ne!(rows[0].kind, ActionKind::Resumed);
    // Retried on the backoff, then recovered in the same incident.
    rig.world.up.store(true, Ordering::SeqCst);
    for _ in 0..80 {
        if rig.rows().iter().any(|row| !row.live) {
            break;
        }
        rig.step(30_000).await;
    }
    let rows = rig.rows();
    assert_eq!(rows.len(), 1, "one incident, one row: {rows:?}");
    assert_eq!(rows[0].kind, ActionKind::Resumed);
    assert!(!rows[0].live);
}

#[tokio::test]
async fn a_confirmed_member_that_faults_again_keeps_the_incident_open_and_later_recovers() {
    let rig = rig(four_claude_sessions(), BAD_GATEWAY);
    rig.sched.tick().await;
    rig.outbreak(BAD_GATEWAY);
    rig.world.up.store(true, Ordering::SeqCst);
    rig.sched.tick().await;
    // s1 is the probe: confirmed after the window, which sweeps s2-s4 (still confirming).
    let s1 = SessionRef::new("mac-2", "s1");
    rig.step(61_000).await;
    assert_eq!(rig.world.sends_to(&s1), 1, "{:?}", rig.world.ops());
    rig.step(61_000).await;
    let swept = rig.world.ops().len();
    assert!(swept >= 4, "the sweep reached the other members: {:?}", rig.world.ops());
    // The confirmed s1 faults again while s2-s4 are still confirming.
    rig.world.push(&s1, BAD_GATEWAY);
    rig.step(10_000).await;
    let rows = rig.rows();
    assert_eq!(rows.len(), 1, "same incident, same row: {rows:?}");
    assert!(rows[0].live, "{rows:?}");
    assert_ne!(rows[0].kind, ActionKind::Resumed, "{rows:?}");
    // After its hold s1 is resumed again.
    for _ in 0..40 {
        if rig.world.sends_to(&s1) >= 2 {
            break;
        }
        rig.step(30_000).await;
        assert!(rig.rows()[0].live || rig.world.sends_to(&s1) >= 2);
    }
    assert!(rig.world.sends_to(&s1) >= 2, "{:?}", rig.world.ops());
    for _ in 0..80 {
        if rig.rows().iter().any(|row| !row.live) {
            break;
        }
        rig.step(30_000).await;
    }
    let rows = rig.rows();
    assert_eq!(rows.len(), 1, "exactly one closed row: {rows:?}");
    assert_eq!(rows[0].kind, ActionKind::Resumed);
    assert!(!rows[0].live);
}

#[tokio::test]
async fn a_freeze_after_post_restart_output_is_a_stall_in_the_same_incident() {
    let rig = rig(vec![provider_working(T0 - 10 * MIN)], "thinking...");
    rig.sched.tick().await;
    assert_eq!(rig.world.ops().len(), 2, "one Stop and one Send");
    // Output moved the screen 40 s after the restart, then it froze for good.
    *rig.fleet.0.lock().unwrap() = vec![provider_working(T0 + 40_000)];
    for _ in 0..40 {
        rig.step(30_000).await;
    }
    assert_eq!(rig.world.ops().len(), 2, "exactly one Stop+Send: {:?}", rig.world.ops());
    assert_eq!(rig.opened().len(), 1, "{:?}", rig.opened());
    let rows = rig.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].kind, ActionKind::Escalated);
}

#[tokio::test]
async fn restart_closes_stale_live_rows_and_never_reuses_incident_keys() {
    let first = rig(vec![provider_working(T0)], BAD_GATEWAY);
    first.sched.tick().await;
    first.outbreak(BAD_GATEWAY);
    first.sched.tick().await;
    first.step(61_000).await;
    let before = first.rows();
    assert_eq!(before.len(), 1);
    assert!(before[0].live);
    // The daemon restarts: a new scheduler over the same persisted rows.
    let again = rig_with(
        vec![provider_working(T0)],
        BAD_GATEWAY,
        first.log.clone(),
        first.clock.clone(),
    );
    again.sched.tick().await;
    again.outbreak(BAD_GATEWAY);
    again.sched.tick().await;
    again.step(61_000).await;
    let rows = again.rows();
    assert_eq!(rows.len(), 2, "the old row is kept, the new incident has its own: {rows:?}");
    assert_ne!(rows[0].incident_key, rows[1].incident_key);
    let old = rows.iter().find(|r| r.incident_key == before[0].incident_key).unwrap();
    assert!(!old.live, "an interrupted incident is not shown as handled: {old:?}");
}

#[tokio::test]
async fn a_deferred_goal_lane_survives_a_restart() {
    let first = rig(Vec::new(), "");
    first.sched.report_goal_quota("lane-9", None, "Try again in 5 minutes").await;
    let again = rig_with(Vec::new(), "", first.log.clone(), first.clock.clone());
    let retry = Arc::new(Retry(Mutex::new(Vec::new())));
    again.sched.set_loop_retry(retry.clone());
    again.step(4 * MIN).await;
    assert!(retry.0.lock().unwrap().is_empty(), "not before the stored reset time");
    again.step(2 * MIN).await;
    assert_eq!(retry.0.lock().unwrap().as_slice(), ["lane-9".to_owned()]);
    assert!(!again.rows()[0].live);
}

#[tokio::test]
async fn a_vanished_session_is_never_reported_as_resumed() {
    let rig = rig(vec![provider_working(T0)], BAD_GATEWAY);
    rig.sched.tick().await;
    rig.outbreak(BAD_GATEWAY);
    rig.sched.tick().await;
    rig.step(61_000).await;
    assert_eq!(rig.world.ops().len(), 1);
    rig.fleet.0.lock().unwrap().clear();
    for _ in 0..6 {
        rig.step(30_000).await;
    }
    let rows = rig.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(!rows[0].live, "{rows:?}");
    assert_ne!(rows[0].kind, ActionKind::Resumed, "{rows:?}");
}

#[tokio::test]
async fn a_goal_retry_with_nothing_to_resume_is_not_reported_as_resumed() {
    let rig = rig(Vec::new(), "");
    let retry = Arc::new(ScriptedRetry {
        calls: Mutex::new(Vec::new()),
        script: Mutex::new(vec![Ok(RetryOutcome::NothingToResume)]),
    });
    rig.sched.set_loop_retry(retry.clone());
    rig.sched.report_goal_quota("lane-n", None, "Try again in 1 minutes").await;
    rig.step(2 * MIN).await;
    assert_eq!(retry.calls.lock().unwrap().len(), 1);
    let rows = rig.rows();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].live);
    assert_ne!(rows[0].kind, ActionKind::Resumed, "{rows:?}");
}

#[tokio::test]
async fn goal_retries_are_bounded_by_the_rule() {
    let rig = rig(Vec::new(), "");
    rig.set_rule(
        RuleKind::QuotaResume,
        serde_json::json!({ "fallbackSecs": 600, "maxGoalAttempts": 2 }),
    );
    let retry = Arc::new(ScriptedRetry { calls: Mutex::new(Vec::new()), script: Mutex::new(Vec::new()) });
    rig.sched.set_loop_retry(retry.clone());
    rig.sched.report_goal_quota("lane-b", None, "Try again in 1 minutes").await;
    for _ in 0..6 {
        rig.step(11 * MIN).await;
    }
    assert_eq!(retry.calls.lock().unwrap().len(), 2, "no retry past the bound");
    let rows = rig.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, ActionKind::Escalated);
    assert!(!rows[0].live);
}

#[tokio::test]
async fn recovery_card_answers_do_what_they_say() {
    // Stall: "keep going" gives a fresh restart, "leave it" closes and stays away.
    let rig = rig(vec![provider_working(T0 - 10 * MIN)], "thinking...");
    rig.sched.tick().await;
    for _ in 0..20 {
        rig.step(30_000).await;
    }
    let card = rig.opened().remove(0);
    let id = card.prompt.prompt_id.clone();
    assert_eq!(rig.sched.answer_card(&id, 7).await, Err(CardError::BadOption));
    assert_eq!(rig.sched.answer_card("recovery:nope:g0", 0).await, Err(CardError::NotOpen));
    assert_eq!(rig.sched.answer_card("not-recovery", 0).await, Err(CardError::NotOpen));
    rig.sched.answer_card(&id, 0).await.unwrap();
    assert_eq!(rig.cards.resolved.lock().unwrap().len(), 1);
    rig.step(30_000).await;
    assert_eq!(rig.world.ops().len(), 4, "one more Stop+Send: {:?}", rig.world.ops());
    for _ in 0..30 {
        rig.step(30_000).await;
    }
    let second = rig.opened().pop().unwrap();
    assert_eq!(rig.opened().len(), 2);
    let ops = rig.world.ops().len();
    rig.sched.answer_card(&second.prompt.prompt_id, 1).await.unwrap();
    for _ in 0..30 {
        rig.step(30_000).await;
    }
    assert_eq!(rig.world.ops().len(), ops, "left alone after 'leave it'");
    assert_eq!(rig.sched.answer_card(&second.prompt.prompt_id, 1).await, Err(CardError::NotOpen));
}

#[tokio::test]
async fn the_stop_answer_interrupts_each_session_once_and_leaves_them_alone() {
    let rig = rig(four_claude_sessions(), BAD_GATEWAY);
    rig.sched.tick().await;
    rig.outbreak(BAD_GATEWAY);
    rig.sched.tick().await;
    for _ in 0..46 {
        rig.step(30_000).await;
    }
    let card = rig.opened().remove(0);
    let before = rig.world.ops().len();
    rig.sched.answer_card(&card.prompt.prompt_id, 1).await.unwrap();
    let ops = rig.world.ops();
    assert_eq!(ops[before..].iter().filter(|op| op.starts_with("stop ")).count(), 4, "{ops:?}");
    assert_eq!(rig.sched.answer_card(&card.prompt.prompt_id, 1).await, Err(CardError::NotOpen));
    for _ in 0..10 {
        rig.step(30_000).await;
    }
    assert_eq!(rig.world.ops().len(), ops.len(), "no Ctrl-C repeated, nothing resent");
    let rows = rig.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].kind, ActionKind::Stopped);
    assert!(!rows[0].live);
}

#[tokio::test]
async fn a_refused_row_and_card_are_cleared_when_the_session_moves_on() {
    let rig = rig(vec![session("mac-2", "s1", "omo", "working", "screen", T0)], BAD_GATEWAY);
    rig.sched.tick().await;
    rig.outbreak(BAD_GATEWAY);
    rig.sched.tick().await;
    assert_eq!(rig.opened().len(), 1);
    assert!(rig.rows()[0].live);
    rig.outbreak("all good, carrying on with the task");
    rig.step(30_000).await;
    assert_eq!(rig.cards.resolved.lock().unwrap().len(), 1, "card withdrawn");
    let rows = rig.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(!rows[0].live, "{rows:?}");
    // Fresh trouble is a new incident with its own key and card.
    rig.outbreak(BAD_GATEWAY);
    rig.step(30_000).await;
    assert_eq!(rig.opened().len(), 2);
    assert_eq!(rig.rows().len(), 2);
}

#[tokio::test]
async fn a_refused_row_and_card_are_cleared_when_the_session_ends() {
    let rig = rig(vec![session("mac-2", "s1", "omo", "working", "screen", T0)], BAD_GATEWAY);
    rig.sched.tick().await;
    rig.outbreak(BAD_GATEWAY);
    rig.sched.tick().await;
    rig.fleet.0.lock().unwrap().clear();
    for _ in 0..4 {
        rig.step(30_000).await;
    }
    assert_eq!(rig.cards.resolved.lock().unwrap().len(), 1);
    assert!(!rig.rows()[0].live);
}

#[tokio::test]
async fn a_transient_observation_loss_does_not_resend_the_resume() {
    let rig = rig(vec![provider_working(T0)], BAD_GATEWAY);
    rig.sched.tick().await;
    rig.outbreak(BAD_GATEWAY);
    rig.world.up.store(true, Ordering::SeqCst);
    rig.sched.tick().await;
    rig.step(61_000).await;
    let s1 = SessionRef::new("mac-2", "s1");
    assert_eq!(rig.world.sends_to(&s1), 1);
    // The fleet listing drops the session for one pass, then it is back.
    let kept = rig.fleet.0.lock().unwrap().clone();
    rig.fleet.0.lock().unwrap().clear();
    rig.step(20_000).await;
    *rig.fleet.0.lock().unwrap() = kept;
    // Then its terminal is unreadable for a pass.
    rig.world.unreadable.lock().unwrap().insert(s1.clone());
    rig.step(20_000).await;
    rig.world.unreadable.lock().unwrap().clear();
    for _ in 0..6 {
        rig.step(10_000).await;
    }
    assert_eq!(rig.world.sends_to(&s1), 1, "{:?}", rig.world.ops());
}

#[tokio::test]
async fn a_failed_send_after_stop_never_repeats_ctrl_c() {
    let rig = rig(vec![provider_working(T0 - 10 * MIN)], "thinking...");
    rig.world.fail_send.store(true, Ordering::SeqCst);
    rig.sched.tick().await;
    assert_eq!(rig.world.ops(), vec!["stop mac-2:s1".to_owned()]);
    for _ in 0..3 {
        rig.step(30_000).await;
    }
    assert_eq!(rig.world.ops().len(), 1, "backing off, nothing repeated");
    rig.world.fail_send.store(false, Ordering::SeqCst);
    rig.step(2 * MIN).await;
    assert_eq!(
        rig.world.ops(),
        vec!["stop mac-2:s1".to_owned(), "send mac-2:s1 continue".to_owned()],
        "one Ctrl-C, then only the text"
    );
}

#[tokio::test]
async fn budgets_and_resume_text_come_from_the_rules() {
    let rig = rig(four_claude_sessions(), BAD_GATEWAY);
    rig.set_rule(
        RuleKind::UpstreamRetry,
        serde_json::json!({ "budgetSecs": 120, "resumeText": "go on" }),
    );
    rig.sched.tick().await;
    rig.outbreak(BAD_GATEWAY);
    rig.sched.tick().await;
    for _ in 0..8 {
        rig.step(30_000).await;
    }
    let opened = rig.opened();
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert!(opened[0].prompt.question.contains("2 minutes"), "{:?}", opened[0].prompt.question);
    assert!(rig.world.ops().iter().any(|op| op.ends_with(" go on")), "{:?}", rig.world.ops());
    assert!(rig.world.ops().iter().all(|op| !op.ends_with(" continue")));
}
