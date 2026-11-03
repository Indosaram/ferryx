//! Deterministic recovery tests: a fake clock the test moves, a fake `SessionControl` that
//! scripts terminal output, an in-memory action log. Nothing sleeps or waits on time.
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;

use super::*;
use crate::error::HubError;
use crate::supervisor::health::SessionHealthClassifier;
use crate::supervisor::panel_contract::{
    default_rules, ActionEntry, ActionKind, FleetSession, StandingRule,
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

struct Rules;
#[async_trait]
impl RuleStore for Rules {
    async fn list(&self) -> Result<Vec<StandingRule>> {
        Ok(default_rules())
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
    up: AtomicBool,
    ops: Mutex<Vec<String>>,
    error: Mutex<String>,
}
impl World {
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
        let mut tails = self.tails.lock().unwrap();
        let tail = tails.entry(session.clone()).or_default();
        tail.push_str(&format!("{line}\n"));
        Ok(())
    }
    async fn answer(&self, _: &SessionRef, _: &str, _: Option<&str>, _: usize) -> Result<()> {
        Ok(())
    }
    async fn read_tail(&self, session: &SessionRef, _lines: usize) -> Result<String> {
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
    clock: Arc<FakeClock>,
    pause: Arc<Pause>,
    log: Arc<Log>,
    cards: Arc<Cards>,
    world: Arc<World>,
}

fn rig(sessions: Vec<FleetSession>, initial_tail: &str) -> Rig {
    let world = Arc::new(World::default());
    *world.error.lock().unwrap() = initial_tail.lines().last().unwrap_or("").to_owned();
    for s in &sessions {
        world
            .tails
            .lock()
            .unwrap()
            .insert(s.session_ref.clone(), format!("{initial_tail}\n"));
    }
    let clock = Arc::new(FakeClock(AtomicI64::new(T0)));
    let pause = Arc::new(Pause(AtomicBool::new(false)));
    let log = Arc::new(Log::default());
    let cards = Arc::new(Cards::default());
    let sched = Arc::new(RecoveryScheduler::new(RecoveryDeps {
        fleet: Arc::new(Fleet(Mutex::new(sessions))),
        control: world.clone(),
        rules: Arc::new(Rules),
        log: log.clone(),
        classifier: Arc::new(SessionHealthClassifier),
        clock: clock.clone(),
        pause: pause.clone(),
        escalations: cards.clone(),
    }));
    Rig { sched, clock, pause, log, cards, world }
}

impl Rig {
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
