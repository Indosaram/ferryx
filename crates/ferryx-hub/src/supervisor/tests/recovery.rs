//! Restart reconciliation (todo 12): the hub "crashes" at every effect boundary of one
//! dispatch (before send, after claim, after send before receipt, after start, after settle)
//! by dropping its dispatcher and scheduler mid-way, then a fresh dispatcher and scheduler
//! start on the same durable store. No goal is lost, nothing is sent twice, and a
//! predecessor's late completion is ignored. Waits are on exact probes with a bounded
//! deadline, never on fixed sleeps.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use sqlx::SqlitePool;
use tokio::sync::{mpsc, Notify};
use tokio::time::timeout;

use crate::agent_control::{pty_run_id, AgentOp, FixedLauncher, RecordingControl};
use crate::goals::contract::RequirementSpec;
use crate::supervisor::admission::{
    submit_work, ActorContext, OperationClass, OwnerChoices, SubmitOutcome, SubmitWork,
};
use crate::supervisor::dispatcher::Dispatcher;
use crate::supervisor::receipts::{record_pty_settle, record_pty_settle_across_restart, PtySettle};
use crate::supervisor::wakes::{report, Deferral, StateHandler, WakeAck, WakeHandler, WakeJudge, WakeScheduler, WakeTurn};
use crate::supervisor::{
    advance_generation, bind_project_host, claim_dispatch, confirm_claim_before_transmit,
    create_project, ensure_identity, execution_binding_of, goal_of, reconcile_claims_after_restart,
    record_receipt, transition_goal, ChannelProvider, Receipt, RunState,
    CLAIM_UNCONFIRMED_AT_RESTART, START_UNCONFIRMED_AT_RESTART,
};

const OWNER: &str = "slack:T1:U-owner";
const HOST: &str = "host-a";
const REPO: &str = "/home/owner/code/ferryx";
const BODY: &str = "Create result.txt containing READY. Run the verifier.";
const PANE: &str = "pane-sv-1";
/// Bound on every awaited signal. Reaching it fails the test; it never makes one pass.
const DEADLINE: Duration = Duration::from_secs(10);
/// The observer epoch of the daemon that crashed, and of the one that replaced it.
const OLD_EPOCH: &str = RecordingControl::EPOCH;
const NEW_EPOCH: &str = "rc-restarted";

async fn owner_pool() -> SqlitePool {
    let pool = crate::storage::memory().await.unwrap();
    let identity = ensure_identity(&pool, OWNER).await.unwrap();
    let project = create_project(&pool, &identity.supervisor_id, "ferryx", "github.com/example/ferryx")
        .await
        .unwrap();
    bind_project_host(&pool, &project.project_id, HOST, &format!("{REPO}/.git"))
        .await
        .unwrap();
    pool
}

/// Admits one goal and returns (goal id, dispatch request id).
async fn admit(pool: &SqlitePool) -> (String, String) {
    let actor = ActorContext {
        principal: OWNER.into(),
        source_adapter: ChannelProvider::Slack,
        source_message_id: "m-1".into(),
        source_body: BODY.into(),
        channel_binding_id: None,
        choices: OwnerChoices::default(),
    };
    let work = SubmitWork {
        project_id: None,
        host_id: None,
        kind: None,
        title: "Write result.txt".into(),
        instruction: "Create result.txt containing READY.".into(),
        requirements: vec![RequirementSpec {
            id: "r1".into(),
            text: "result.txt contains READY".into(),
            verifier: "verify.sh exits 0".into(),
        }],
        cwd: None,
        operations: OperationClass::BASELINE.to_vec(),
        turn_limit: None,
    };
    match submit_work(pool, &actor, &work).await.unwrap() {
        SubmitOutcome::Admitted(work) => (work.receipt.goal_id, work.receipt.dispatch_request_id),
        other => panic!("expected an admitted goal, got {other:?}"),
    }
}

fn dispatcher(pool: &SqlitePool, control: Arc<RecordingControl>) -> Dispatcher {
    Dispatcher::new(pool.clone(), control, Arc::new(FixedLauncher::new(PANE)), HOST)
}

/// Starts a fresh hub dispatcher on the store (restart reconciliation, then the startup
/// drain) and returns how many intents its startup drain handled. The task is stopped after
/// its first drain, as a crash right after it would.
async fn restart(pool: &SqlitePool, control: Arc<RecordingControl>) -> usize {
    let (probe_tx, mut probe_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(
        dispatcher(pool, control)
            .with_rescan(Duration::from_secs(3600))
            .with_drain_probe(probe_tx)
            .run(Arc::new(Notify::new())),
    );
    let handled = timeout(DEADLINE, probe_rx.recv())
        .await
        .expect("the startup drain ran")
        .expect("drain probe");
    task.abort();
    handled
}

fn sends(control: &RecordingControl) -> usize {
    control
        .ops()
        .into_iter()
        .filter(|(_, op)| matches!(op, AgentOp::Send { .. }))
        .count()
}

/// No restart path ever stops, closes or answers a worker pane: the PTY census is untouched.
fn no_pane_mutation_but_sends(control: &RecordingControl) -> bool {
    control
        .ops()
        .into_iter()
        .all(|(_, op)| !op.is_mutation() || matches!(op, AgentOp::Send { .. }))
}

async fn goal_state(pool: &SqlitePool, goal_id: &str) -> (RunState, Option<String>) {
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    (goal.run_state, goal.state_detail)
}

async fn dispatch_state(pool: &SqlitePool, request_id: &str) -> (String, Option<String>) {
    sqlx::query_as("SELECT state, uncertain_code FROM supervisor_dispatches WHERE request_id = ?")
        .bind(request_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Goal-scheduler tick on a fresh scheduler (its cursor is durable) at `now`.
async fn fresh_tick(pool: &SqlitePool) {
    let scheduler = WakeScheduler::new(pool.clone(), Arc::new(StateHandler::new(pool.clone())));
    scheduler.tick(Utc::now()).await.unwrap();
}

#[tokio::test]
async fn a_crash_before_send_dispatches_the_pending_intent_exactly_once_after_restart() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool).await;
    // The hub went down after admission committed and before any dispatcher claimed it.
    let control = Arc::new(RecordingControl::observing());
    assert_eq!(restart(&pool, control.clone()).await, 1, "the proven-unsent intent runs");
    assert_eq!(sends(&control), 1);
    assert_eq!(dispatch_state(&pool, &request_id).await.0, "started");
    assert_eq!(goal_state(&pool, &goal_id).await.0, RunState::Running);
    // A second restart finds nothing to send.
    assert_eq!(restart(&pool, control.clone()).await, 0);
    assert_eq!(sends(&control), 1, "never sent twice");
    assert!(no_pane_mutation_but_sends(&control));
}

#[tokio::test]
async fn a_crash_after_claim_is_uncertain_and_never_resent() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool).await;
    claim_dispatch(&pool, &request_id, 1, 1).await.unwrap();
    // The confirm passed: from here the bytes may have left.
    confirm_claim_before_transmit(&pool, &request_id).await.unwrap();
    let control = Arc::new(RecordingControl::observing());
    assert_eq!(restart(&pool, control.clone()).await, 0);
    assert_eq!(
        dispatch_state(&pool, &request_id).await,
        ("uncertain".to_owned(), Some(CLAIM_UNCONFIRMED_AT_RESTART.to_owned()))
    );
    assert_eq!(goal_state(&pool, &goal_id).await.0, RunState::DeliveryUncertain);
    assert_eq!(restart(&pool, control.clone()).await, 0);
    assert_eq!(sends(&control), 0, "sent-unknown is never resent");
}

#[tokio::test]
async fn a_crash_after_send_before_the_start_receipt_is_uncertain_and_a_late_start_still_counts() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool).await;
    let generation = execution_binding_of(&pool, &goal_id).await.unwrap().unwrap().generation;
    crate::supervisor::attach_session(&pool, &goal_id, generation, PANE).await.unwrap();
    claim_dispatch(&pool, &request_id, 1, generation).await.unwrap();
    confirm_claim_before_transmit(&pool, &request_id).await.unwrap();
    record_receipt(&pool, &request_id, generation, &Receipt::Accepted).await.unwrap();
    // Crash: the target took the bytes; its start receipt never reached the store.
    let control = Arc::new(RecordingControl::observing());
    assert_eq!(restart(&pool, control.clone()).await, 0);
    assert_eq!(
        dispatch_state(&pool, &request_id).await,
        ("uncertain".to_owned(), Some(START_UNCONFIRMED_AT_RESTART.to_owned()))
    );
    assert_eq!(
        goal_state(&pool, &goal_id).await,
        (RunState::DeliveryUncertain, Some(START_UNCONFIRMED_AT_RESTART.to_owned()))
    );
    assert_eq!(restart(&pool, control.clone()).await, 0);
    assert_eq!(sends(&control), 0, "accepted bytes are never sent again");

    // The target's start receipt, consumed late, still records the run that really happened.
    let run = pty_run_id(PANE, OLD_EPOCH, 4);
    let started = Receipt::Started { run_id: run.clone(), provider_session_id: PANE.into() };
    record_receipt(&pool, &request_id, generation, &started).await.unwrap();
    let goal = goal_of(&pool, &goal_id).await.unwrap().unwrap();
    transition_goal(&pool, &goal_id, goal.revision, RunState::DeliveryUncertain, RunState::Running, None)
        .await
        .unwrap();
    assert_eq!(execution_binding_of(&pool, &goal_id).await.unwrap().unwrap().run_id, Some(run));
}

#[tokio::test]
async fn a_crash_between_an_uncertain_receipt_and_the_goal_move_does_not_strand_the_goal() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool).await;
    claim_dispatch(&pool, &request_id, 1, 1).await.unwrap();
    confirm_claim_before_transmit(&pool, &request_id).await.unwrap();
    let uncertain = Receipt::Uncertain { code: "TURN_NOT_OBSERVED".into() };
    record_receipt(&pool, &request_id, 1, &uncertain).await.unwrap();
    // Crash before the dispatcher moved the goal out of dispatching.
    assert_eq!(goal_state(&pool, &goal_id).await.0, RunState::Dispatching);
    reconcile_claims_after_restart(&pool).await.unwrap();
    assert_eq!(
        goal_state(&pool, &goal_id).await,
        (RunState::DeliveryUncertain, Some("TURN_NOT_OBSERVED".to_owned()))
    );
    let control = Arc::new(RecordingControl::observing());
    assert_eq!(restart(&pool, control.clone()).await, 0);
    assert_eq!(sends(&control), 0);
}

#[tokio::test]
async fn a_crash_after_start_keeps_the_run_and_the_new_daemon_settles_it() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool).await;
    let control = Arc::new(RecordingControl::observing());
    assert_eq!(restart(&pool, control.clone()).await, 1);
    let run = pty_run_id(PANE, OLD_EPOCH, 1);
    assert_eq!(execution_binding_of(&pool, &goal_id).await.unwrap().unwrap().run_id, Some(run.clone()));

    // The daemon restarted while the worker ran: the run is reattached, not resent.
    assert_eq!(restart(&pool, control.clone()).await, 0);
    assert_eq!(goal_state(&pool, &goal_id).await.0, RunState::Running);
    assert_eq!(sends(&control), 1);
    // The old epoch's sequences cannot be ordered against the new observer's.
    assert!(matches!(
        record_pty_settle(&pool, HOST, PANE, NEW_EPOCH, 1).await.unwrap(),
        PtySettle::OtherEpoch { .. }
    ));
    // Every reading of the new epoch was taken after the restart, so after the run's start.
    let PtySettle::Settled(settled) =
        record_pty_settle_across_restart(&pool, HOST, PANE, NEW_EPOCH, 1).await.unwrap()
    else {
        panic!("the new daemon's idle reading ends the predecessor's run");
    };
    assert_eq!(settled.request_id, request_id);
    assert_eq!(settled.run_id.as_deref(), Some(run.as_str()));
    // A second reading has nothing left to settle.
    assert_eq!(
        record_pty_settle_across_restart(&pool, HOST, PANE, NEW_EPOCH, 2).await.unwrap(),
        PtySettle::Ignored
    );
    fresh_tick(&pool).await;
    assert_eq!(goal_state(&pool, &goal_id).await.0, RunState::Verifying, "the goal is not lost");
    assert!(no_pane_mutation_but_sends(&control));
}

#[tokio::test]
async fn a_crash_after_settle_resumes_the_goal_from_the_durable_cursor_without_resending() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool).await;
    let control = Arc::new(RecordingControl::observing());
    assert_eq!(restart(&pool, control.clone()).await, 1);
    let PtySettle::Settled(_) = record_pty_settle(&pool, HOST, PANE, OLD_EPOCH, 2).await.unwrap() else {
        panic!("the run settles");
    };
    // Crash before any scheduler applied the settle.
    assert_eq!(goal_state(&pool, &goal_id).await.0, RunState::Running);
    assert_eq!(restart(&pool, control.clone()).await, 0);
    fresh_tick(&pool).await;
    assert_eq!(goal_state(&pool, &goal_id).await.0, RunState::Verifying);
    assert_eq!(dispatch_state(&pool, &request_id).await.0, "settled");
    assert_eq!(sends(&control), 1, "never sent twice");
}

#[tokio::test]
async fn a_predecessor_completion_after_a_new_generation_is_ignored() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool).await;
    let control = Arc::new(RecordingControl::observing());
    assert_eq!(restart(&pool, control.clone()).await, 1);
    let run = pty_run_id(PANE, OLD_EPOCH, 1);
    // The goal is rebound to a new worker generation (handover or reattach policy).
    advance_generation(&pool, &goal_id, 1, Some("pane-sv-2")).await.unwrap();
    let late = Receipt::Settled { run_id: run, provider_session_id: PANE.into() };
    let refused = record_receipt(&pool, &request_id, 1, &late).await.unwrap_err();
    assert_eq!(refused.code(), "STALE_GENERATION");
    assert_eq!(dispatch_state(&pool, &request_id).await.0, "started", "nothing recorded");
    assert_eq!(
        record_pty_settle(&pool, HOST, PANE, OLD_EPOCH, 9).await.unwrap(),
        PtySettle::Ignored,
        "the old pane no longer belongs to the goal"
    );
    fresh_tick(&pool).await;
    assert_eq!(goal_state(&pool, &goal_id).await.0, RunState::Running, "no completion from it");
    let settled_wakes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM supervisor_pending_wakes WHERE goal_id = ? AND cause = 'turn_settled'",
    )
    .bind(&goal_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(settled_wakes, 0);
}

#[derive(Default)]
struct Counting {
    judged: AtomicUsize,
    handled: AtomicUsize,
}

#[async_trait]
impl WakeJudge for Counting {
    async fn score(&self, _state: &str) -> Result<f64, String> {
        self.judged.fetch_add(1, Ordering::SeqCst);
        Ok(1.0)
    }
}

#[async_trait]
impl WakeHandler for Counting {
    async fn handle(&self, _turn: &WakeTurn) -> WakeAck {
        self.handled.fetch_add(1, Ordering::SeqCst);
        WakeAck::Done
    }
}

#[tokio::test]
async fn a_degraded_goal_gets_no_model_call_over_an_hour_of_reports() {
    let pool = owner_pool().await;
    let (goal_id, _) = admit(&pool).await;
    transition_goal(&pool, &goal_id, 1, RunState::Ready, RunState::Degraded, Some("MHC_CAPABILITY_MISSING"))
        .await
        .unwrap();
    let start = Utc::now();
    for n in 0..4 {
        report(&pool, &goal_id, &format!("report-{n}"), start + chrono::Duration::seconds(n))
            .await
            .unwrap();
    }
    let counting = Arc::new(Counting::default());
    let scheduler = WakeScheduler::new(pool.clone(), counting.clone()).with_judge(counting.clone());
    // Virtual time: one tick per minute for an hour, all of it past the batch window.
    for minute in 1..=60 {
        let tick = scheduler.tick(start + chrono::Duration::minutes(minute)).await.unwrap();
        assert!(
            tick.deferred.contains(&(goal_id.clone(), Deferral::Held(RunState::Degraded))),
            "{tick:?}"
        );
    }
    assert_eq!(counting.judged.load(Ordering::SeqCst), 0, "no judge call while degraded");
    assert_eq!(counting.handled.load(Ordering::SeqCst), 0, "no decision turn while degraded");
}
