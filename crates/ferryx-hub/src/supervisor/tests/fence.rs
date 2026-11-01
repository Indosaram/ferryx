//! The supervisor lease fence on the dispatcher (plan task 6): a stale fence refusal releases
//! the dispatch, holds the goal with `held_detail` STALE_SUPERVISOR_FENCE (never settled or
//! completed) and stands the hub down; a new holder pushes its fence to every daemon before
//! its first dispatch, and a host that refuses the push keeps its bindings undispatched.
//! Every wait is on an observed signal with a bounded deadline.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use sqlx::SqlitePool;
use tokio::sync::{mpsc, Notify};
use tokio::time::timeout;

use crate::agent_control::{
    AgentOp, Delivery, FixedLauncher, ReceiptScript, RecordingControl, LOCAL_HOST_ID,
    STALE_SUPERVISOR_FENCE,
};
use crate::goals::contract::RequirementSpec;
use crate::goals::types::SendOutcome;
use crate::supervisor::admission::{
    submit_work, ActorContext, OperationClass, OwnerChoices, SubmitOutcome, SubmitWork,
};
use crate::supervisor::dispatcher::{DispatchResult, Dispatcher};
use crate::supervisor::fence_gate::{FenceGate, FencePush, SUPERVISOR_FENCE_PENDING};
use crate::supervisor::*;

const OWNER: &str = "qa-owner";
const PAIRED: &str = "paired-host-1";
const PANE: &str = "pane-remote-1";
const BODY: &str = "Create result.txt containing READY. Run the verifier.";
/// Bound on every awaited signal. Reaching it fails the test; it never makes one pass.
const DEADLINE: Duration = Duration::from_secs(10);

async fn owner_pool() -> SqlitePool {
    let pool = crate::storage::memory().await.unwrap();
    provision_owner(&pool, OWNER, "qa-supervisor", "qa:qa-supervisor", PAIRED, "/work/qa/.git")
        .await
        .unwrap();
    pool
}

/// Admits one goal bound to the owner's chosen pane on the paired host.
async fn admit(pool: &SqlitePool, message_id: &str) -> (String, String) {
    admit_on(pool, message_id, PANE).await
}

async fn admit_on(pool: &SqlitePool, message_id: &str, pane: &str) -> (String, String) {
    let actor = ActorContext {
        principal: OWNER.into(),
        source_adapter: ChannelProvider::Cli,
        source_message_id: message_id.into(),
        source_body: BODY.into(),
        channel_binding_id: None,
        choices: OwnerChoices {
            host_id: Some(PAIRED.into()),
            session_id: Some(pane.into()),
            ..OwnerChoices::default()
        },
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
    Dispatcher::new(pool.clone(), control, Arc::new(FixedLauncher::new("never-launched")), LOCAL_HOST_ID)
}

fn sends(control: &RecordingControl) -> Vec<(Option<String>, String)> {
    control
        .ops()
        .into_iter()
        .filter_map(|(host, op)| match op {
            AgentOp::Send { request_id, .. } => Some((host, request_id)),
            _ => None,
        })
        .collect()
}

async fn dispatch_state(pool: &SqlitePool, request_id: &str) -> String {
    sqlx::query_scalar("SELECT state FROM supervisor_dispatches WHERE request_id = ?")
        .bind(request_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A fence push side: records each push (target, sends the control had seen by then),
/// refuses pushes to `PAIRED` while `refuse_paired` is set, and counts stand-downs.
struct FakePush {
    control: Arc<RecordingControl>,
    pushes: Mutex<Vec<(Option<String>, usize)>>,
    refuse_paired: AtomicBool,
    stood_down: Mutex<usize>,
}

impl FakePush {
    fn new(control: Arc<RecordingControl>) -> Arc<Self> {
        Arc::new(Self {
            control,
            pushes: Mutex::new(Vec::new()),
            refuse_paired: AtomicBool::new(false),
            stood_down: Mutex::new(0),
        })
    }

    fn pushes(&self) -> Vec<(Option<String>, usize)> {
        self.pushes.lock().expect("pushes").clone()
    }
}

#[async_trait]
impl FencePush for FakePush {
    async fn hosts(&self) -> Vec<String> {
        vec![PAIRED.to_owned()]
    }

    async fn push_fence(&self, host_id: Option<&str>) -> Result<(), String> {
        let seen = sends(&self.control).len();
        self.pushes
            .lock()
            .expect("pushes")
            .push((host_id.map(str::to_owned), seen));
        if host_id == Some(PAIRED) && self.refuse_paired.load(Ordering::SeqCst) {
            return Err("PAIRED_HOST_OFFLINE the host did not answer".to_owned());
        }
        Ok(())
    }

    fn stand_down(&self) {
        *self.stood_down.lock().expect("stood down") += 1;
    }
}

#[tokio::test]
async fn a_stale_fence_releases_the_dispatch_and_holds_the_goal_without_completing_it() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool, "m-stale").await;
    let control = Arc::new(RecordingControl::scripted(ReceiptScript::Fixed(
        Delivery::from_error_code(Some(STALE_SUPERVISOR_FENCE)),
    )));
    let push = FakePush::new(control.clone());
    let gate = Arc::new(FenceGate::new(push.clone()));
    let dispatcher = dispatcher(&pool, control.clone()).with_fence_gate(gate.clone());

    let results = dispatcher.drain().await;
    assert!(
        matches!(
            &results[..],
            [DispatchResult::Held { request_id: held, state: RunState::Paused, detail }]
                if *held == request_id && detail.starts_with(STALE_SUPERVISOR_FENCE)
        ),
        "{results:?}"
    );
    assert_eq!(dispatch_state(&pool, &request_id).await, "released", "no new dispatch state");
    let goal = goal_of(&pool, &goal_id).await.unwrap().unwrap();
    assert_eq!(goal.run_state, RunState::Paused, "held, never settled or completed");
    let (held_from, held_detail): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT held_from, held_detail FROM supervisor_goals WHERE goal_id = ?")
            .bind(&goal_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(held_from.as_deref(), Some("ready"));
    assert_eq!(held_detail.as_deref(), Some(STALE_SUPERVISOR_FENCE));
    assert!(sends(&control).is_empty(), "nothing was typed");
    assert!(gate.stood_down(), "the hub stands down");
    assert_eq!(*push.stood_down.lock().expect("stood down"), 1);
    // A stood-down holder dispatches nothing more.
    let (_other, other_request) = admit_on(&pool, "m-after", "pane-remote-2").await;
    assert_eq!(
        dispatcher.drain().await,
        vec![DispatchResult::Skipped {
            request_id: other_request.clone(),
            code: SUPERVISOR_FENCE_PENDING.into(),
        }]
    );
    assert_eq!(dispatch_state(&pool, &other_request).await, "ready");
}

#[tokio::test]
async fn the_fence_reaches_every_daemon_before_the_first_dispatch() {
    let pool = owner_pool().await;
    let (_goal_id, request_id) = admit(&pool, "m-push").await;
    let control = Arc::new(RecordingControl::observing());
    let push = FakePush::new(control.clone());
    let (probe_tx, mut probe_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(
        dispatcher(&pool, control.clone())
            .with_fence_gate(Arc::new(FenceGate::new(push.clone())))
            .with_drain_probe(probe_tx)
            .run(Arc::new(Notify::new())),
    );
    let handled = timeout(DEADLINE, probe_rx.recv()).await.expect("the first drain").unwrap();
    task.abort();
    assert_eq!(handled, 1);
    assert_eq!(
        push.pushes(),
        vec![(None, 0), (Some(PAIRED.to_owned()), 0)],
        "this machine's daemon and the paired host took the fence before anything was sent"
    );
    assert_eq!(sends(&control), vec![(Some(PAIRED.to_owned()), request_id)]);
}

#[tokio::test]
async fn a_host_that_refuses_the_fence_keeps_its_bindings_undispatched() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool, "m-refuse").await;
    let control = Arc::new(RecordingControl::observing());
    let push = FakePush::new(control.clone());
    push.refuse_paired.store(true, Ordering::SeqCst);
    let gate = Arc::new(FenceGate::new(push.clone()));
    let dispatcher = dispatcher(&pool, control.clone()).with_fence_gate(gate.clone());

    gate.push_on_start().await;
    assert_eq!(
        gate.unreachable_hosts().into_iter().map(|(host, _)| host).collect::<Vec<_>>(),
        vec![Some(PAIRED.to_owned())],
        "status lists the host the fence did not reach"
    );
    assert_eq!(
        dispatcher.drain().await,
        vec![DispatchResult::Skipped {
            request_id: request_id.clone(),
            code: SUPERVISOR_FENCE_PENDING.into(),
        }]
    );
    assert!(sends(&control).is_empty(), "nothing went to the unfenced host");
    assert_eq!(dispatch_state(&pool, &request_id).await, "ready");
    assert_eq!(goal_of(&pool, &goal_id).await.unwrap().unwrap().run_state, RunState::Ready);

    // Once its push succeeds, its binding is dispatched.
    push.refuse_paired.store(false, Ordering::SeqCst);
    assert_eq!(
        dispatcher.drain().await,
        vec![DispatchResult::Transmitted {
            request_id: request_id.clone(),
            outcome: SendOutcome::Started,
        }]
    );
    assert!(gate.unreachable_hosts().is_empty());
    assert_eq!(sends(&control), vec![(Some(PAIRED.to_owned()), request_id)]);
}
