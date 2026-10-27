//! Goal-aware wakes (migration 0021): dependency waits through the real store, the durable
//! state-event cursor, batching on an injected clock and the scheduler loop on exact signals.
//! No test sleeps: virtual time is passed to `tick`, and the loop test awaits its own probes
//! under a bounded deadline.
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use tokio::sync::{mpsc, Notify};
use tokio::time::timeout;

use crate::goals::contract::RequirementSpec;
use crate::goals::types::GoalMode;
use crate::supervisor::wakes::{
    add_dependency, report, schedule_deadline, Deferral, DependencyError, StateHandler,
    WakeAck, WakeClock, WakeHandler, WakeJudge, WakeScheduler, WakeTurn, DEADLINE_PASSED,
    DEPENDENCIES_MET, DEPENDENCY_FAILED,
};
use crate::supervisor::{
    admit_request, attach_session, bind_project_host, claim_dispatch,
    confirm_claim_before_transmit, create_project, ensure_identity, goal_of, record_receipt,
    record_wait_event, transition_goal, waits_of, AdmitRequest, ChannelProvider, ExecutionKind,
    ExecutionTarget, Receipt, RunState, WaitOutcome, WaitState, WaitTarget, WakeCause,
    DEFAULT_TURN_LIMIT,
};

const OWNER: &str = "slack:T1:U-owner";
const STRANGER: &str = "slack:T1:U-stranger";
const HOST: &str = "host-a";
/// Bound on every awaited signal. Reaching it fails the test; it never makes one pass.
const DEADLINE: Duration = Duration::from_secs(10);

struct Fixture {
    pool: SqlitePool,
    owner: String,
    supervisor_id: String,
    project_id: String,
}

async fn owner_on(pool: SqlitePool, owner: &str, repo: &str) -> Fixture {
    let identity = ensure_identity(&pool, owner).await.unwrap();
    let project = create_project(&pool, &identity.supervisor_id, "ferryx", repo)
        .await
        .unwrap();
    bind_project_host(
        &pool,
        &project.project_id,
        HOST,
        &format!("/home/{owner}/{repo}/.git"),
    )
    .await
    .unwrap();
    Fixture {
        pool,
        owner: owner.to_owned(),
        supervisor_id: identity.supervisor_id,
        project_id: project.project_id,
    }
}

async fn fixture_on(pool: SqlitePool) -> Fixture {
    owner_on(pool, OWNER, "github.com/example/ferryx").await
}

async fn fixture() -> Fixture {
    fixture_on(crate::storage::memory().await.unwrap()).await
}

async fn file_pool(path: &std::path::Path) -> SqlitePool {
    crate::storage::connect(path).await.unwrap()
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("ferryx-hub-wakes-{}", uuid::Uuid::new_v4()))
}

/// Admits one goal for the fixture's owner; returns its goal id.
async fn admit(fixture: &Fixture, message_id: &str) -> String {
    let request = AdmitRequest {
        supervisor_id: fixture.supervisor_id.clone(),
        owner_principal: fixture.owner.clone(),
        source_adapter: ChannelProvider::Slack,
        source_message_id: message_id.into(),
        body: format!("Do part {message_id}."),
        project_id: fixture.project_id.clone(),
        channel_binding_id: None,
        target: ExecutionTarget {
            kind: ExecutionKind::Pty,
            host_id: HOST.into(),
        },
        title: format!("Part {message_id}"),
        cwd: "/home/owner/code/ferryx".into(),
        instruction: format!("Do part {message_id}."),
        requirements: vec![RequirementSpec {
            id: "r1".into(),
            text: "the part is done".into(),
            verifier: "verify.sh exits 0".into(),
        }],
        mode: GoalMode::Full,
        turn_limit: DEFAULT_TURN_LIMIT,
        operation_grant: Vec::new(),
    };
    admit_request(&fixture.pool, &request)
        .await
        .unwrap()
        .receipt
        .goal_id
}

fn session_of(run_id: &str) -> String {
    format!("pane-{run_id}")
}

/// Claims the goal's ready intent, transmits it and records a started run: the goal is
/// `running` with a live run. Returns the dispatch's request id.
async fn start_run(pool: &SqlitePool, goal_id: &str, run_id: &str) -> String {
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    let request_id: String = sqlx::query_scalar(
        "SELECT request_id FROM supervisor_dispatches WHERE goal_id = ? AND state = 'ready'",
    )
    .bind(goal_id)
    .fetch_one(pool)
    .await
    .unwrap();
    attach_session(pool, goal_id, 1, &session_of(run_id))
        .await
        .unwrap();
    claim_dispatch(pool, &request_id, goal.revision, 1)
        .await
        .unwrap();
    confirm_claim_before_transmit(pool, &request_id)
        .await
        .unwrap();
    record_receipt(pool, &request_id, 1, &Receipt::Accepted)
        .await
        .unwrap();
    let started = Receipt::Started {
        run_id: run_id.into(),
        provider_session_id: session_of(run_id),
    };
    record_receipt(pool, &request_id, 1, &started).await.unwrap();
    transition_goal(
        pool,
        goal_id,
        goal.revision,
        RunState::Dispatching,
        RunState::Running,
        None,
    )
    .await
    .unwrap();
    request_id
}

/// The worker's run ends: the settled receipt T8 records for a finished turn.
async fn settle_run(pool: &SqlitePool, request_id: &str, run_id: &str) {
    let settled = Receipt::Settled {
        run_id: run_id.into(),
        provider_session_id: session_of(run_id),
    };
    record_receipt(pool, request_id, 1, &settled).await.unwrap();
}

/// Verified completion of a goal whose run has settled.
async fn complete(pool: &SqlitePool, goal_id: &str) {
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    if goal.run_state != RunState::Verifying {
        transition_goal(
            pool,
            goal_id,
            goal.revision,
            goal.run_state,
            RunState::Verifying,
            None,
        )
        .await
        .unwrap();
    }
    transition_goal(
        pool,
        goal_id,
        goal.revision,
        RunState::Verifying,
        RunState::Completed,
        Some("verified"),
    )
    .await
    .unwrap();
}

async fn cancel(pool: &SqlitePool, goal_id: &str) {
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    transition_goal(
        pool,
        goal_id,
        goal.revision,
        goal.run_state,
        RunState::Cancelled,
        Some("owner cancelled"),
    )
    .await
    .unwrap();
}

async fn pause(pool: &SqlitePool, goal_id: &str) {
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    transition_goal(
        pool,
        goal_id,
        goal.revision,
        goal.run_state,
        RunState::Paused,
        Some("owner paused"),
    )
    .await
    .unwrap();
}

async fn run_state(pool: &SqlitePool, goal_id: &str) -> RunState {
    goal_of(pool, goal_id).await.unwrap().unwrap().run_state
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

async fn wait_on(pool: &SqlitePool, goal_id: &str, child_id: &str) -> String {
    waits_of(pool, goal_id)
        .await
        .unwrap()
        .into_iter()
        .find(|wait| wait.child_goal_id.as_deref() == Some(child_id))
        .unwrap()
        .wait_id
}

fn epoch() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-27T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn secs(seconds: i64) -> chrono::Duration {
    chrono::Duration::seconds(seconds)
}

/// Records every turn; optionally answers `Busy` first, applies [`StateHandler`] and signals.
struct Recorder {
    turns: Mutex<Vec<WakeTurn>>,
    busy_left: AtomicUsize,
    inner: Option<StateHandler>,
    signal: Option<mpsc::UnboundedSender<WakeTurn>>,
}

impl Recorder {
    fn new(inner: Option<StateHandler>, busy: usize) -> Self {
        Self {
            turns: Mutex::new(Vec::new()),
            busy_left: AtomicUsize::new(busy),
            inner,
            signal: None,
        }
    }

    fn plain() -> Arc<Self> {
        Arc::new(Self::new(None, 0))
    }

    fn applying(pool: &SqlitePool) -> Arc<Self> {
        Arc::new(Self::new(Some(StateHandler::new(pool.clone())), 0))
    }

    fn turns(&self) -> Vec<WakeTurn> {
        self.turns.lock().unwrap().clone()
    }

    fn turns_for(&self, goal_id: &str) -> Vec<WakeTurn> {
        self.turns()
            .into_iter()
            .filter(|turn| turn.goal_id == goal_id)
            .collect()
    }

    fn last(&self) -> WakeTurn {
        self.turns().last().cloned().unwrap()
    }
}

#[async_trait]
impl WakeHandler for Recorder {
    async fn handle(&self, turn: &WakeTurn) -> WakeAck {
        self.turns.lock().unwrap().push(turn.clone());
        if let Some(signal) = &self.signal {
            let _ = signal.send(turn.clone());
        }
        if self
            .busy_left
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(1))
            .is_ok()
        {
            return WakeAck::Busy;
        }
        match &self.inner {
            Some(inner) => inner.handle(turn).await,
            None => WakeAck::Done,
        }
    }
}

struct CountingJudge {
    score: f64,
    calls: AtomicUsize,
}

impl CountingJudge {
    fn new(score: f64) -> Arc<Self> {
        Arc::new(Self {
            score,
            calls: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl WakeJudge for CountingJudge {
    async fn score(&self, _state: &str) -> Result<f64, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.score)
    }
}

struct ManualClock(Mutex<DateTime<Utc>>);

impl ManualClock {
    fn set(&self, at: DateTime<Utc>) {
        *self.0.lock().unwrap() = at;
    }
}

impl WakeClock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

fn event_ids(turn: &WakeTurn) -> Vec<String> {
    turn.wakes.iter().map(|wake| wake.event_id.clone()).collect()
}

/// The `burst` schedule on a virtual clock: reports at 0, 1, 2 and 3s wait for the window
/// of the oldest, then make one judged turn with four receipts at 30s; an owner pause skips
/// the window and the judge, and reports to a paused goal only wait.
#[tokio::test]
async fn reports_batch_on_the_oldest_and_an_owner_pause_skips_the_window() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let goal = admit(&fixture, "burst").await;
    let request = start_run(pool, &goal, "run-1").await;
    settle_run(pool, &request, "run-1").await;
    let handler = Recorder::plain();
    let judge = CountingJudge::new(0.9);
    let scheduler = WakeScheduler::new(pool.clone(), handler.clone()).with_judge(judge.clone());

    let setup = scheduler.tick(epoch()).await.unwrap();
    assert_eq!(setup.delivered.len(), 1, "a settled run wakes its idle goal");
    assert!(handler.last().has(WakeCause::TurnSettled));
    assert!(handler.last().urgent);
    assert_eq!(judge.calls(), 0, "a settle is never judged");

    let t0 = epoch() + secs(100);
    for (offset, event_id) in [(0, "rep-0"), (1, "rep-1"), (2, "rep-2"), (3, "rep-3")] {
        let at = t0 + secs(offset);
        report(pool, &goal, event_id, at).await.unwrap();
        let tick = scheduler.tick(at).await.unwrap();
        assert!(tick.delivered.is_empty(), "{event_id}: inside the window");
        assert_eq!(tick.deferred, vec![(goal.clone(), Deferral::Batching)]);
        assert_eq!(
            tick.next_due,
            Some(t0 + secs(30)),
            "{event_id}: the window stays anchored on the oldest report"
        );
    }
    let early = scheduler.tick(t0 + secs(29)).await.unwrap();
    assert!(early.delivered.is_empty());
    assert_eq!(handler.turns().len(), 1);
    assert_eq!(judge.calls(), 0, "nothing is judged inside the window");

    let due = scheduler.tick(t0 + secs(30)).await.unwrap();
    assert_eq!(due.delivered.len(), 1);
    let turn = handler.last();
    assert!(!turn.urgent);
    assert_eq!(event_ids(&turn), ["rep-0", "rep-1", "rep-2", "rep-3"]);
    assert_eq!(judge.calls(), 1);
    let receipts = count(
        pool,
        &format!(
            "SELECT COUNT(*) FROM supervisor_wake_receipts
             WHERE turn_id = {} AND disposition = 'delivered'",
            turn.turn_id
        ),
    )
    .await;
    assert_eq!(receipts, 4, "one admitted turn with four event receipts");
    assert_eq!(
        count(
            pool,
            "SELECT COUNT(*) FROM supervisor_pending_wakes WHERE consumed_at IS NULL"
        )
        .await,
        0
    );

    let t1 = t0 + secs(60);
    report(pool, &goal, "rep-4", t1).await.unwrap();
    pause(pool, &goal).await;
    let paused = scheduler.tick(t1).await.unwrap();
    assert_eq!(paused.delivered.len(), 1, "the pause has zero batch delay");
    let turn = handler.last();
    assert!(turn.urgent);
    assert!(turn.has(WakeCause::OwnerControl));
    assert_eq!(turn.wakes.len(), 1, "the owner wake goes alone");
    assert_eq!(judge.calls(), 1, "an owner wake is never judged");

    let held = scheduler.tick(t1 + secs(45)).await.unwrap();
    assert!(held.delivered.is_empty());
    assert_eq!(
        held.deferred,
        vec![(goal.clone(), Deferral::Held(RunState::Paused))]
    );
    assert_eq!(handler.turns().len(), 3);
}

/// A busy handler keeps the turn; the retry reuses the turn id and the cached judgment, and a
/// low score receipts a report batch as skipped without reaching the handler.
#[tokio::test]
async fn a_report_batch_is_judged_once_per_event_set_revision_and_generation() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let goal = admit(&fixture, "judged").await;
    let request = start_run(pool, &goal, "run-1").await;
    settle_run(pool, &request, "run-1").await;
    let handler = Arc::new(Recorder::new(None, 0));
    let judge = CountingJudge::new(0.8);
    let scheduler = WakeScheduler::new(pool.clone(), handler.clone()).with_judge(judge.clone());
    scheduler.tick(epoch()).await.unwrap();

    let t0 = epoch() + secs(100);
    report(pool, &goal, "rep-a", t0).await.unwrap();
    handler.busy_left.store(1, Ordering::SeqCst);
    let busy = scheduler.tick(t0 + secs(30)).await.unwrap();
    assert!(busy.delivered.is_empty());
    assert_eq!(busy.deferred, vec![(goal.clone(), Deferral::HandlerBusy)]);
    let again = scheduler.tick(t0 + secs(31)).await.unwrap();
    assert_eq!(again.delivered.len(), 1);
    assert_eq!(judge.calls(), 1, "the same set at the same revision is judged once");
    let turns = handler.turns_for(&goal);
    assert_eq!(turns.len(), 3);
    assert_eq!(turns[1].turn_id, turns[2].turn_id, "a retry is the same turn");

    let low = CountingJudge::new(0.1);
    let quiet = Recorder::plain();
    let skipping = WakeScheduler::new(pool.clone(), quiet.clone()).with_judge(low.clone());
    let t1 = t0 + secs(200);
    report(pool, &goal, "rep-b", t1).await.unwrap();
    let skipped = skipping.tick(t1 + secs(30)).await.unwrap();
    assert_eq!(skipped.skipped.len(), 1);
    assert!(quiet.turns().is_empty(), "a skipped batch never reaches the handler");
    assert_eq!(
        count(
            pool,
            "SELECT COUNT(*) FROM supervisor_wake_receipts WHERE disposition = 'skipped'"
        )
        .await,
        1
    );
}

/// The `waiting` scenario through the store: the parent's run settles while two children
/// work, so it waits without any decision; one child completing changes status only; the
/// second completing resumes the parent once, with its settle and queued report, and no
/// judgment.
#[tokio::test]
async fn a_parent_waits_without_decisions_and_resumes_once_when_its_children_finish() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let parent = admit(&fixture, "parent").await;
    let a = admit(&fixture, "child-a").await;
    let b = admit(&fixture, "child-b").await;
    add_dependency(pool, &parent, &WaitTarget::ChildGoal(a.clone()))
        .await
        .unwrap();
    add_dependency(pool, &parent, &WaitTarget::ChildGoal(b.clone()))
        .await
        .unwrap();
    let handler = Recorder::applying(pool);
    let judge = CountingJudge::new(0.9);
    let scheduler = WakeScheduler::new(pool.clone(), handler.clone()).with_judge(judge.clone());

    let parent_run = start_run(pool, &parent, "run-p").await;
    let a_run = start_run(pool, &a, "run-a").await;
    let b_run = start_run(pool, &b, "run-b").await;
    settle_run(pool, &parent_run, "run-p").await;
    let now = epoch();
    let first = scheduler.tick(now).await.unwrap();
    assert_eq!(first.waiting, vec![parent.clone()]);
    assert_eq!(run_state(pool, &parent).await, RunState::WaitingChildren);
    assert!(first
        .deferred
        .contains(&(parent.clone(), Deferral::Dependencies)));
    assert!(handler.turns_for(&parent).is_empty());

    report(pool, &parent, "progress-1", now).await.unwrap();
    for minute in 1..=3 {
        let tick = scheduler.tick(now + secs(60 * minute)).await.unwrap();
        assert!(tick
            .deferred
            .contains(&(parent.clone(), Deferral::Dependencies)));
    }
    assert!(
        handler.turns_for(&parent).is_empty(),
        "no decision while a dependency is incomplete"
    );

    settle_run(pool, &a_run, "run-a").await;
    complete(pool, &a).await;
    let one = scheduler.tick(now + secs(300)).await.unwrap();
    assert_eq!(one.resolved_waits, 1);
    let states: Vec<WaitState> = waits_of(pool, &parent)
        .await
        .unwrap()
        .into_iter()
        .map(|wait| wait.state)
        .collect();
    assert_eq!(states, [WaitState::Satisfied, WaitState::Pending]);
    assert!(handler.turns_for(&parent).is_empty(), "one child is not enough");
    assert_eq!(run_state(pool, &parent).await, RunState::WaitingChildren);

    settle_run(pool, &b_run, "run-b").await;
    complete(pool, &b).await;
    let both = scheduler.tick(now + secs(301)).await.unwrap();
    assert_eq!(both.resolved_waits, 1);
    let turns = handler.turns_for(&parent);
    assert_eq!(turns.len(), 1, "one resume when eligible");
    assert!(turns[0].urgent);
    assert!(turns[0].has(WakeCause::DependencyReady));
    assert!(turns[0].has(WakeCause::TurnSettled), "the settle was kept, not dropped");
    assert!(turns[0].has(WakeCause::Report), "unrelated progress rides along");
    let resumed = goal_of(pool, &parent).await.unwrap().unwrap();
    assert_eq!(resumed.run_state, RunState::Verifying);
    assert_eq!(resumed.state_detail.as_deref(), Some(DEPENDENCIES_MET));
    assert_eq!(judge.calls(), 0, "a dependency resume is never judged");

    let after = scheduler.tick(now + secs(400)).await.unwrap();
    assert!(after.delivered.is_empty());
    assert_eq!(handler.turns_for(&parent).len(), 1);
}

/// The failure half of `waiting`: a cancelled child releases the parent once with failure
/// evidence; the parent is held for the owner and never completes from it.
#[tokio::test]
async fn a_failed_child_releases_its_parent_once_and_is_not_success() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let parent = admit(&fixture, "parent").await;
    let a = admit(&fixture, "child-a").await;
    let b = admit(&fixture, "child-b").await;
    for child in [&a, &b] {
        add_dependency(pool, &parent, &WaitTarget::ChildGoal(child.clone()))
            .await
            .unwrap();
    }
    let handler = Recorder::applying(pool);
    let scheduler = WakeScheduler::new(pool.clone(), handler.clone());
    let parent_run = start_run(pool, &parent, "run-p").await;
    let a_run = start_run(pool, &a, "run-a").await;
    start_run(pool, &b, "run-b").await;
    settle_run(pool, &parent_run, "run-p").await;
    scheduler.tick(epoch()).await.unwrap();
    settle_run(pool, &a_run, "run-a").await;
    complete(pool, &a).await;
    scheduler.tick(epoch() + secs(1)).await.unwrap();
    assert!(handler.turns_for(&parent).is_empty());

    cancel(pool, &b).await;
    scheduler.tick(epoch() + secs(2)).await.unwrap();
    let turns = handler.turns_for(&parent);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].has(WakeCause::DependencyFailed));
    let held = goal_of(pool, &parent).await.unwrap().unwrap();
    assert_eq!(held.run_state, RunState::AwaitingOwner);
    let detail = held.state_detail.unwrap();
    assert!(detail.starts_with(DEPENDENCY_FAILED), "{detail}");
    assert!(detail.contains(&b), "the failure names the child: {detail}");
    assert_ne!(held.run_state, RunState::Completed);

    scheduler.tick(epoch() + secs(60)).await.unwrap();
    assert_eq!(handler.turns_for(&parent).len(), 1, "released once");
}

/// Duplicate, replayed and late child reports never unblock the parent a second time, and a
/// child that ended before the wait existed releases it at once.
#[tokio::test]
async fn duplicate_and_out_of_order_child_reports_do_not_unblock_twice() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let parent = admit(&fixture, "parent").await;
    let a = admit(&fixture, "child-a").await;
    let b = admit(&fixture, "child-b").await;
    for child in [&a, &b] {
        add_dependency(pool, &parent, &WaitTarget::ChildGoal(child.clone()))
            .await
            .unwrap();
    }
    let handler = Recorder::plain();
    let scheduler = WakeScheduler::new(pool.clone(), handler.clone());
    let parent_run = start_run(pool, &parent, "run-p").await;
    let a_run = start_run(pool, &a, "run-a").await;
    let b_run = start_run(pool, &b, "run-b").await;
    settle_run(pool, &parent_run, "run-p").await;
    settle_run(pool, &a_run, "run-a").await;
    complete(pool, &a).await;
    scheduler.tick(epoch()).await.unwrap();
    let a_wait = wait_on(pool, &parent, &a).await;
    let wait_events = "SELECT COUNT(*) FROM supervisor_wait_events";
    let recorded = count(pool, wait_events).await;

    // A crash before the cursor moved: the whole log is applied again.
    sqlx::query("UPDATE supervisor_event_cursors SET last_seq = 0")
        .execute(pool)
        .await
        .unwrap();
    let replay = scheduler.tick(epoch() + secs(1)).await.unwrap();
    assert!(replay.events > 0);
    assert_eq!(replay.resolved_waits, 0);
    assert_eq!(count(pool, wait_events).await, recorded);

    let duplicate = record_wait_event(
        pool,
        &a_wait,
        &format!("goal:{a}:completed"),
        WaitOutcome::Satisfied,
        &[],
    )
    .await
    .unwrap();
    assert!(!duplicate.inserted, "the same child end is one event");
    for (event_id, outcome) in [
        ("late-progress", WaitOutcome::Progress),
        ("late-failure", WaitOutcome::Failed),
    ] {
        let late = record_wait_event(pool, &a_wait, event_id, outcome, &[])
            .await
            .unwrap();
        assert!(late.inserted);
        assert_eq!(late.wait.state, WaitState::Satisfied, "{event_id}");
        assert!(late.wake.is_none(), "{event_id} wakes nothing");
    }
    assert!(handler.turns_for(&parent).is_empty());

    settle_run(pool, &b_run, "run-b").await;
    complete(pool, &b).await;
    scheduler.tick(epoch() + secs(2)).await.unwrap();
    assert_eq!(handler.turns_for(&parent).len(), 1);
    sqlx::query("UPDATE supervisor_event_cursors SET last_seq = 0")
        .execute(pool)
        .await
        .unwrap();
    scheduler.tick(epoch() + secs(3)).await.unwrap();
    scheduler.tick(epoch() + secs(90)).await.unwrap();
    assert_eq!(handler.turns_for(&parent).len(), 1, "unblocked exactly once");
    assert_eq!(
        count(
            pool,
            "SELECT COUNT(*) FROM supervisor_pending_wakes
             WHERE cause = 'dependency_ready' AND consumed_at IS NOT NULL"
        )
        .await,
        1
    );

    // An end that arrived before the wait existed.
    let late_parent = admit(&fixture, "late-parent").await;
    let done = admit(&fixture, "already-done").await;
    let done_run = start_run(pool, &done, "run-d").await;
    settle_run(pool, &done_run, "run-d").await;
    complete(pool, &done).await;
    add_dependency(pool, &late_parent, &WaitTarget::ChildGoal(done.clone()))
        .await
        .unwrap();
    let waits = waits_of(pool, &late_parent).await.unwrap();
    assert_eq!(waits[0].state, WaitState::Satisfied);
    let ready: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM supervisor_pending_wakes
         WHERE goal_id = ? AND cause = 'dependency_ready'",
    )
    .bind(&late_parent)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(ready, 1);
}

/// Cycles, self-dependencies and links to another owner's goal are refused before commit
/// with a typed error that is HTTP 400.
#[tokio::test]
async fn a_cycle_a_self_dependency_and_a_cross_owner_link_are_refused_as_400() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let a = admit(&fixture, "a").await;
    let b = admit(&fixture, "b").await;
    let c = admit(&fixture, "c").await;
    let waits = "SELECT COUNT(*) FROM supervisor_waits";

    let own = add_dependency(pool, &a, &WaitTarget::ChildGoal(a.clone()))
        .await
        .unwrap_err();
    assert!(matches!(own, DependencyError::SelfDependency(_)), "{own}");
    assert_eq!((own.code(), own.http_status()), ("WAIT_CYCLE", 400));

    add_dependency(pool, &a, &WaitTarget::ChildGoal(b.clone()))
        .await
        .unwrap();
    add_dependency(pool, &b, &WaitTarget::ChildGoal(c.clone()))
        .await
        .unwrap();
    let before = count(pool, waits).await;
    let cycle = add_dependency(pool, &c, &WaitTarget::ChildGoal(a.clone()))
        .await
        .unwrap_err();
    assert!(matches!(cycle, DependencyError::Cycle { .. }), "{cycle}");
    assert_eq!((cycle.code(), cycle.http_status()), ("WAIT_CYCLE", 400));
    assert_eq!(count(pool, waits).await, before, "nothing was written");

    let stranger = owner_on(pool.clone(), STRANGER, "github.com/example/theirs").await;
    let theirs = admit(&stranger, "theirs").await;
    let cross = add_dependency(pool, &a, &WaitTarget::ChildGoal(theirs.clone()))
        .await
        .unwrap_err();
    assert!(matches!(cross, DependencyError::CrossOwner(_)), "{cross}");
    assert_eq!((cross.code(), cross.http_status()), ("CROSS_OWNER", 400));
    let reverse = add_dependency(pool, &theirs, &WaitTarget::ChildGoal(a.clone()))
        .await
        .unwrap_err();
    assert_eq!(reverse.http_status(), 400);
    assert_eq!(count(pool, waits).await, before);
}

/// Restart: every change the scheduler never saw is in the durable log and is applied from
/// the persisted cursor; a turn the handler never acknowledged is delivered again under the
/// same id, once.
#[tokio::test]
async fn a_restart_recovers_pending_ready_events_from_the_durable_cursor() {
    let dir = temp_dir();
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let path = dir.join("hub.sqlite");
    let fixture = fixture_on(file_pool(&path).await).await;
    let pool = &fixture.pool;
    let parent = admit(&fixture, "parent").await;
    let child = admit(&fixture, "child").await;
    add_dependency(pool, &parent, &WaitTarget::ChildGoal(child.clone()))
        .await
        .unwrap();
    let parent_run = start_run(pool, &parent, "run-p").await;
    let child_run = start_run(pool, &child, "run-c").await;
    settle_run(pool, &parent_run, "run-p").await;
    settle_run(pool, &child_run, "run-c").await;
    complete(pool, &child).await;
    let logged = count(pool, "SELECT COUNT(*) FROM supervisor_state_events").await;
    let logged_max = count(pool, "SELECT MAX(seq) FROM supervisor_state_events").await;
    assert!(logged > 0, "the changes were logged with their commits");
    assert_eq!(
        count(pool, "SELECT COUNT(*) FROM supervisor_pending_wakes").await,
        0,
        "no scheduler has run yet"
    );
    fixture.pool.close().await;

    // First start: the hub stops before the handler acknowledges.
    let pool = file_pool(&path).await;
    let stopping = Arc::new(Recorder::new(None, 1));
    let first = WakeScheduler::new(pool.clone(), stopping.clone())
        .tick(epoch())
        .await
        .unwrap();
    assert_eq!(first.events as i64, logged);
    assert_eq!(first.waiting, vec![parent.clone()]);
    assert_eq!(first.deferred, vec![(parent.clone(), Deferral::HandlerBusy)]);
    let pending_turn = stopping.turns_for(&parent)[0].turn_id;
    let cursor: i64 = sqlx::query_scalar(
        "SELECT last_seq FROM supervisor_event_cursors WHERE consumer = 'wakes'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cursor, logged_max, "the cursor persisted past every logged change");
    pool.close().await;

    // Second start: the same turn is delivered once.
    let pool = file_pool(&path).await;
    let handler = Recorder::applying(&pool);
    let scheduler = WakeScheduler::new(pool.clone(), handler.clone());
    let second = scheduler.tick(epoch() + secs(5)).await.unwrap();
    assert_eq!(second.delivered, vec![pending_turn]);
    let turn = handler.last();
    assert!(turn.has(WakeCause::DependencyReady));
    assert!(turn.has(WakeCause::TurnSettled));
    assert_eq!(run_state(&pool, &parent).await, RunState::Verifying);
    let third = scheduler.tick(epoch() + secs(60)).await.unwrap();
    assert!(third.delivered.is_empty());
    assert_eq!(handler.turns().len(), 1);
    pool.close().await;
    let _ = tokio::fs::remove_dir_all(&dir).await;
}

/// Busy-to-idle and deadlines: a due deadline waits while the worker is busy and is delivered
/// with the settle once it is idle; a deadline not yet due is only a timer.
#[tokio::test]
async fn a_deadline_waits_for_a_busy_worker_and_is_delivered_when_it_goes_idle() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let goal = admit(&fixture, "deadline").await;
    let request = start_run(pool, &goal, "run-1").await;
    let handler = Recorder::applying(pool);
    let scheduler = WakeScheduler::new(pool.clone(), handler.clone());
    let t0 = epoch();
    schedule_deadline(pool, &goal, "deadline-1", t0 + secs(10))
        .await
        .unwrap();

    let early = scheduler.tick(t0).await.unwrap();
    assert!(early.delivered.is_empty());
    assert_eq!(early.next_due, Some(t0 + secs(10)));
    let busy = scheduler.tick(t0 + secs(10)).await.unwrap();
    assert_eq!(busy.deferred, vec![(goal.clone(), Deferral::Busy)]);
    assert!(handler.turns().is_empty(), "a busy goal gets no decision");

    settle_run(pool, &request, "run-1").await;
    let idle = scheduler.tick(t0 + secs(11)).await.unwrap();
    assert_eq!(idle.delivered.len(), 1);
    let turn = handler.last();
    assert!(turn.has(WakeCause::Deadline));
    assert!(turn.has(WakeCause::TurnSettled));
    let held = goal_of(pool, &goal).await.unwrap().unwrap();
    assert_eq!(held.run_state, RunState::AwaitingOwner);
    assert!(held
        .state_detail
        .as_deref()
        .is_some_and(|detail| detail.starts_with(DEADLINE_PASSED)));
}

/// The loop: its start tick recovers a durable settle, a report inside the window only sets
/// the next timer, and an owner pause is delivered on the next poke with the clock unmoved.
#[tokio::test]
async fn the_loop_recovers_at_start_and_delivers_an_owner_pause_without_delay() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let goal = admit(&fixture, "loop").await;
    let request = start_run(pool, &goal, "run-1").await;
    settle_run(pool, &request, "run-1").await;

    let (signal, mut turns) = mpsc::unbounded_channel();
    let handler = Arc::new(Recorder {
        signal: Some(signal),
        ..Recorder::new(None, 0)
    });
    let (probe, mut ticks) = mpsc::unbounded_channel();
    let clock = Arc::new(ManualClock(Mutex::new(epoch())));
    let wake = Arc::new(Notify::new());
    let scheduler = WakeScheduler::new(pool.clone(), handler.clone())
        .with_clock(clock.clone())
        .with_rescan(Duration::from_secs(3600))
        .with_tick_probe(probe);
    let task = tokio::spawn(scheduler.run(Arc::clone(&wake)));

    let first = timeout(DEADLINE, turns.recv()).await.unwrap().unwrap();
    assert!(first.has(WakeCause::TurnSettled));
    let start = timeout(DEADLINE, ticks.recv()).await.unwrap().unwrap();
    assert_eq!(start.delivered, vec![first.turn_id]);

    report(pool, &goal, "rep-0", epoch()).await.unwrap();
    wake.notify_one();
    let batching = timeout(DEADLINE, ticks.recv()).await.unwrap().unwrap();
    assert!(batching.delivered.is_empty());
    assert_eq!(batching.next_due, Some(epoch() + secs(30)));

    pause(pool, &goal).await;
    wake.notify_one();
    let paused = timeout(DEADLINE, turns.recv()).await.unwrap().unwrap();
    assert!(paused.has(WakeCause::OwnerControl));
    assert_eq!(clock.now(), epoch(), "no batch delay");
    timeout(DEADLINE, ticks.recv()).await.unwrap().unwrap();

    clock.set(epoch() + secs(30));
    wake.notify_one();
    let held = timeout(DEADLINE, ticks.recv()).await.unwrap().unwrap();
    assert!(held.delivered.is_empty());
    assert_eq!(
        held.deferred,
        vec![(goal.clone(), Deferral::Held(RunState::Paused))]
    );
    assert_eq!(handler.turns().len(), 2);
    task.abort();
}
