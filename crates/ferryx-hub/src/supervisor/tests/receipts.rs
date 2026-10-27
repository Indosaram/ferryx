//! Causal worker receipts of the supervisor dispatcher: a run starts only from a target-side
//! receipt naming the dispatch's request and bound session; a missing or foreign receipt stays
//! uncertain and is never resent; a target that cannot observe holds before sending. Every
//! wait is on an observed signal with a bounded deadline, never on a fixed sleep.
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use crate::agent_control::{
    pty_run_id, AgentOp, Delivery, FixedLauncher, ReceiptScript, RecordingControl, StartReceipt,
    OBSERVER_CAPABILITY_MISSING, RECEIPT_MISMATCH,
};
use crate::goals::types::SendOutcome;
use crate::meta::mhc_stdio::StdioClient;
use crate::supervisor::admission::{
    submit_work, ActorContext, OperationClass, OwnerChoices, SubmitOutcome, SubmitWork,
};
use crate::supervisor::dispatcher::{DispatchResult, Dispatcher, MHC_WORKER_UNAVAILABLE};
use crate::supervisor::receipts::{
    mhc_run_id, record_pty_settle, settle_after_start, MhcWorker, StdioEngine, MHC_PROMPT_QUEUED,
};
use crate::supervisor::{
    attach_session, bind_project_host, create_project, ensure_identity, execution_binding_of,
    goal_of, ChannelProvider, DispatchState, ExecutionKind, RunState,
};
use crate::goals::contract::RequirementSpec;

const OWNER: &str = "slack:T1:U-owner";
const HOST: &str = "host-a";
const REPO: &str = "/home/owner/code/ferryx";
const BODY: &str = "Create result.txt containing READY. Run the verifier.";
const PANE: &str = "pane-sv-1";
const MHC_SESSION: &str = "h-worker-1";
/// Bound on every awaited signal. Reaching it fails the test; it never makes one pass.
const DEADLINE: Duration = Duration::from_secs(10);

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
    bind_project_host(&pool, &project.project_id, HOST, &format!("{REPO}/.git"))
        .await
        .unwrap();
    pool
}

fn actor(message_id: &str) -> ActorContext {
    ActorContext {
        principal: OWNER.into(),
        source_adapter: ChannelProvider::Slack,
        source_message_id: message_id.into(),
        source_body: BODY.into(),
        channel_binding_id: None,
        choices: OwnerChoices::default(),
    }
}

fn work(kind: Option<ExecutionKind>, instruction: &str) -> SubmitWork {
    SubmitWork {
        project_id: None,
        host_id: None,
        kind,
        title: "Write result.txt".into(),
        instruction: instruction.into(),
        requirements: vec![RequirementSpec {
            id: "r1".into(),
            text: "result.txt contains READY".into(),
            verifier: "verify.sh exits 0".into(),
        }],
        cwd: None,
        operations: OperationClass::BASELINE.to_vec(),
        turn_limit: None,
    }
}

/// Admits one goal and returns (goal id, dispatch request id).
async fn admit(pool: &SqlitePool, kind: Option<ExecutionKind>, instruction: &str) -> (String, String) {
    match submit_work(pool, &actor("m-1"), &work(kind, instruction)).await.unwrap() {
        SubmitOutcome::Admitted(work) => (work.receipt.goal_id, work.receipt.dispatch_request_id),
        other => panic!("expected an admitted goal, got {other:?}"),
    }
}

fn pty_dispatcher(pool: &SqlitePool, control: Arc<RecordingControl>) -> Dispatcher {
    Dispatcher::new(pool.clone(), control, Arc::new(FixedLauncher::new(PANE)), HOST)
}

fn send_count(control: &RecordingControl) -> usize {
    control
        .ops()
        .into_iter()
        .filter(|(_, op)| matches!(op, AgentOp::Send { .. }))
        .count()
}

/// (state, uncertain code, run id) of one dispatch.
async fn dispatch_row(pool: &SqlitePool, request_id: &str) -> (String, Option<String>, Option<String>) {
    sqlx::query_as(
        "SELECT state, uncertain_code, run_id FROM supervisor_dispatches WHERE request_id = ?",
    )
    .bind(request_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn run_state(pool: &SqlitePool, goal_id: &str) -> RunState {
    goal_of(pool, goal_id).await.unwrap().unwrap().run_state
}

async fn bound_run(pool: &SqlitePool, goal_id: &str) -> Option<String> {
    execution_binding_of(pool, goal_id).await.unwrap().unwrap().run_id
}

#[tokio::test]
async fn a_causal_start_receipt_starts_the_run_it_names() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool, None, "Create result.txt containing READY.").await;
    let control = Arc::new(RecordingControl::observing());
    assert_eq!(
        pty_dispatcher(&pool, control.clone()).drain().await,
        vec![DispatchResult::Transmitted {
            request_id: request_id.clone(),
            outcome: SendOutcome::Started,
        }]
    );
    let run = pty_run_id(PANE, 1);
    assert_eq!(
        dispatch_row(&pool, &request_id).await,
        ("started".to_owned(), None, Some(run.clone()))
    );
    assert_eq!(bound_run(&pool, &goal_id).await, Some(run));
    assert_eq!(run_state(&pool, &goal_id).await, RunState::Running);
    assert_eq!(send_count(&control), 1);
}

#[tokio::test]
async fn a_missing_start_receipt_stays_uncertain_and_is_not_resent() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool, None, "Create result.txt containing READY.").await;
    let control = Arc::new(RecordingControl::scripted(ReceiptScript::Fixed(
        Delivery::Uncertain {
            accepted: true,
            code: "TURN_NOT_OBSERVED".into(),
        },
    )));
    let dispatcher = pty_dispatcher(&pool, control.clone());
    assert_eq!(
        dispatcher.drain().await,
        vec![DispatchResult::Transmitted {
            request_id: request_id.clone(),
            outcome: SendOutcome::Uncertain {
                code: "TURN_NOT_OBSERVED".into()
            },
        }]
    );
    assert_eq!(
        dispatch_row(&pool, &request_id).await,
        ("uncertain".to_owned(), Some("TURN_NOT_OBSERVED".to_owned()), None)
    );
    let accepted_at: Option<String> =
        sqlx::query_scalar("SELECT accepted_at FROM supervisor_dispatches WHERE request_id = ?")
            .bind(&request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(accepted_at.is_some(), "acceptance is recorded as its own fact");
    assert_eq!(bound_run(&pool, &goal_id).await, None, "nothing started");
    assert_eq!(run_state(&pool, &goal_id).await, RunState::DeliveryUncertain);
    // The next drain finds nothing to send: an uncertain delivery waits for the owner.
    assert!(dispatcher.drain().await.is_empty());
    assert_eq!(send_count(&control), 1, "never resent");
}

#[tokio::test]
async fn a_receipt_for_another_request_or_session_never_starts_the_run() {
    for (request, session) in [("sv-other-request", PANE), ("", "pane-elsewhere")] {
        let pool = owner_pool().await;
        let (goal_id, request_id) = admit(&pool, None, "Create result.txt containing READY.").await;
        // Activity the target saw in this pane for another request, or in another pane for
        // this request: neither is this dispatch's start.
        let receipt = StartReceipt {
            request_id: if request.is_empty() {
                request_id.clone()
            } else {
                request.to_owned()
            },
            session_id: session.to_owned(),
            run_id: pty_run_id(session, 7),
        };
        let control = Arc::new(RecordingControl::scripted(ReceiptScript::Fixed(
            Delivery::Started(receipt),
        )));
        let dispatcher = pty_dispatcher(&pool, control.clone());
        assert_eq!(
            dispatcher.drain().await,
            vec![DispatchResult::Transmitted {
                request_id: request_id.clone(),
                outcome: SendOutcome::Uncertain {
                    code: RECEIPT_MISMATCH.into()
                },
            }]
        );
        assert_eq!(
            dispatch_row(&pool, &request_id).await,
            ("uncertain".to_owned(), Some(RECEIPT_MISMATCH.to_owned()), None)
        );
        assert_eq!(bound_run(&pool, &goal_id).await, None);
        assert_eq!(run_state(&pool, &goal_id).await, RunState::DeliveryUncertain);
        assert!(dispatcher.drain().await.is_empty());
        assert_eq!(send_count(&control), 1, "never resent");
    }
}

#[tokio::test]
async fn a_target_without_a_causal_observer_holds_before_sending() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool, None, "Create result.txt containing READY.").await;
    // The trait default: no observer can attribute a start to a request.
    let control = Arc::new(RecordingControl::default());
    let results = pty_dispatcher(&pool, control.clone()).drain().await;
    assert!(
        matches!(
            &results[..],
            [DispatchResult::Held { state: RunState::Degraded, detail, .. }]
                if detail.starts_with(OBSERVER_CAPABILITY_MISSING)
        ),
        "{results:?}"
    );
    assert_eq!(
        dispatch_row(&pool, &request_id).await,
        ("released".to_owned(), None, None),
        "the claim is given back: nothing was sent"
    );
    assert_eq!(run_state(&pool, &goal_id).await, RunState::Degraded);
    assert!(control.ops().is_empty(), "nothing was transmitted");
}

#[tokio::test]
async fn a_paired_host_without_receipts_holds_and_types_nothing() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool, None, "Create result.txt containing READY.").await;
    let control = Arc::new(RecordingControl::scripted(ReceiptScript::Fixed(
        Delivery::from_error_code(Some(crate::agent_control::PAIRED_HOST_CAPABILITY_UNAVAILABLE)),
    )));
    let results = pty_dispatcher(&pool, control.clone()).drain().await;
    assert!(
        matches!(
            &results[..],
            [DispatchResult::Held { state: RunState::Degraded, detail, .. }]
                if detail.starts_with(crate::agent_control::PAIRED_HOST_CAPABILITY_UNAVAILABLE)
        ),
        "{results:?}"
    );
    assert_eq!(dispatch_row(&pool, &request_id).await.0, "released");
    assert_eq!(run_state(&pool, &goal_id).await, RunState::Degraded);
    assert!(control.ops().is_empty());
}

#[tokio::test]
async fn only_a_later_idle_reading_of_the_bound_pane_settles_a_pty_run() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool, None, "Create result.txt containing READY.").await;
    let control = Arc::new(RecordingControl::observing());
    pty_dispatcher(&pool, control).drain().await;
    let run = pty_run_id(PANE, 1);
    assert_eq!(bound_run(&pool, &goal_id).await, Some(run.clone()));
    // The start reading itself, another host's pane and another pane settle nothing.
    assert!(record_pty_settle(&pool, HOST, PANE, 1).await.unwrap().is_none());
    assert!(record_pty_settle(&pool, "host-b", PANE, 9).await.unwrap().is_none());
    assert!(record_pty_settle(&pool, HOST, "pane-other", 9).await.unwrap().is_none());
    assert_eq!(dispatch_row(&pool, &request_id).await.0, "started");

    let settled = record_pty_settle(&pool, HOST, PANE, 2).await.unwrap().unwrap();
    assert_eq!(settled.state, DispatchState::Settled);
    assert_eq!(settled.run_id.as_deref(), Some(run.as_str()));
    assert_eq!(bound_run(&pool, &goal_id).await, None, "the live run ended");
    // A second idle reading has no live run to settle.
    assert!(record_pty_settle(&pool, HOST, PANE, 3).await.unwrap().is_none());
}

#[tokio::test]
async fn an_mhc_goal_without_a_worker_engine_is_held() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool, Some(ExecutionKind::Mhc), "Create result.txt.").await;
    let binding = execution_binding_of(&pool, &goal_id).await.unwrap().unwrap();
    attach_session(&pool, &goal_id, binding.generation, MHC_SESSION)
        .await
        .unwrap();
    let control = Arc::new(RecordingControl::observing());
    let results = pty_dispatcher(&pool, control.clone()).drain().await;
    assert!(
        matches!(
            &results[..],
            [DispatchResult::Held { request_id: held, state: RunState::Degraded, detail }]
                if held == &request_id && detail.starts_with(MHC_WORKER_UNAVAILABLE)
        ),
        "{results:?}"
    );
    assert!(control.ops().is_empty(), "an mhc goal never reaches a pane");
}

/// A scripted `mhc --mode rpc --multi-session` engine. A prompt containing `QUEUE` is queued,
/// `REFUSE` is refused as `session_closing`; any other starts a turn, after first replaying a
/// settle of an earlier turn. The turn settles when the test sends its handle on `settle`.
async fn mhc_peer(stream: DuplexStream, mut settle: mpsc::UnboundedReceiver<String>) {
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    loop {
        let out: Vec<Value> = tokio::select! {
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { return };
                let frame: Value = serde_json::from_str(&line).expect("the client writes JSON lines");
                if frame["type"] != "prompt" {
                    vec![json!({"type": "response", "id": frame["id"], "command": frame["type"], "success": true})]
                } else {
                    let handle = frame["sessionId"].clone();
                    let message = frame["message"].as_str().unwrap_or_default();
                    let ok = |disposition: &str| json!({"type": "response", "id": frame["id"],
                        "command": "prompt", "success": true, "data": {"disposition": disposition}});
                    if message.contains("QUEUE") {
                        vec![ok("queued")]
                    } else if message.contains("REFUSE") {
                        vec![json!({"type": "response", "id": frame["id"], "command": "prompt",
                            "success": false, "error": "closing", "errorCode": "session_closing"})]
                    } else {
                        vec![
                            json!({"type": "agent_settled", "sessionId": handle}),
                            ok("started"),
                            json!({"type": "agent_start", "sessionId": handle}),
                        ]
                    }
                }
            }
            handle = settle.recv() => {
                let Some(handle) = handle else { return };
                vec![json!({"type": "agent_settled", "sessionId": handle})]
            }
        };
        for value in out {
            if write.write_all(format!("{value}\n").as_bytes()).await.is_err() {
                return;
            }
        }
    }
}

/// An mhc goal bound to [`MHC_SESSION`] and a dispatcher wired to a scripted engine. Returns
/// (goal id, request id, dispatcher, settle trigger, settle probe).
async fn mhc_fixture(
    pool: &SqlitePool,
    instruction: &str,
) -> (
    String,
    String,
    Dispatcher,
    mpsc::UnboundedSender<String>,
    mpsc::UnboundedReceiver<String>,
) {
    let (goal_id, request_id) = admit(pool, Some(ExecutionKind::Mhc), instruction).await;
    let binding = execution_binding_of(pool, &goal_id).await.unwrap().unwrap();
    attach_session(pool, &goal_id, binding.generation, MHC_SESSION)
        .await
        .unwrap();
    let (client_side, peer_side) = tokio::io::duplex(1 << 16);
    let (settle_tx, settle_rx) = mpsc::unbounded_channel();
    tokio::spawn(mhc_peer(peer_side, settle_rx));
    let (read, write) = tokio::io::split(client_side);
    let (client, _work) = StdioClient::start(Box::new(read), Box::new(write), 1 << 20);
    let worker = MhcWorker::new(Arc::new(StdioEngine(client)), DEADLINE);
    let (probe_tx, probe_rx) = mpsc::unbounded_channel();
    let dispatcher = Dispatcher::new(
        pool.clone(),
        Arc::new(RecordingControl::observing()),
        Arc::new(FixedLauncher::new(PANE)),
        HOST,
    )
    .with_mhc_worker(Arc::new(worker))
    .with_settle_probe(probe_tx);
    (goal_id, request_id, dispatcher, settle_tx, probe_rx)
}

#[tokio::test]
async fn an_mhc_started_prompt_starts_and_its_own_turn_settles_the_run() {
    let pool = owner_pool().await;
    let (goal_id, request_id, dispatcher, settle, mut settled) =
        mhc_fixture(&pool, "Create result.txt containing READY.").await;
    assert_eq!(
        dispatcher.drain().await,
        vec![DispatchResult::Transmitted {
            request_id: request_id.clone(),
            outcome: SendOutcome::Started,
        }]
    );
    let run = mhc_run_id(MHC_SESSION, &request_id);
    assert_eq!(
        dispatch_row(&pool, &request_id).await,
        ("started".to_owned(), None, Some(run.clone()))
    );
    assert_eq!(run_state(&pool, &goal_id).await, RunState::Running);

    settle.send(MHC_SESSION.to_owned()).unwrap();
    let noticed = timeout(DEADLINE, settled.recv()).await.expect("settle probe");
    assert_eq!(noticed.as_deref(), Some(request_id.as_str()));
    assert_eq!(
        dispatch_row(&pool, &request_id).await,
        ("settled".to_owned(), None, Some(run))
    );
    assert_eq!(bound_run(&pool, &goal_id).await, None);
}

#[tokio::test]
async fn an_mhc_queued_prompt_is_accepted_but_not_started() {
    let pool = owner_pool().await;
    let (goal_id, request_id, dispatcher, _settle, _settled) =
        mhc_fixture(&pool, "QUEUE behind the running turn.").await;
    assert_eq!(
        dispatcher.drain().await,
        vec![DispatchResult::Transmitted {
            request_id: request_id.clone(),
            outcome: SendOutcome::Uncertain {
                code: MHC_PROMPT_QUEUED.into()
            },
        }]
    );
    assert_eq!(
        dispatch_row(&pool, &request_id).await,
        ("uncertain".to_owned(), Some(MHC_PROMPT_QUEUED.to_owned()), None)
    );
    assert_eq!(run_state(&pool, &goal_id).await, RunState::DeliveryUncertain);
    assert!(dispatcher.drain().await.is_empty(), "never resent");
}

#[tokio::test]
async fn an_mhc_refused_prompt_is_held_for_the_owner() {
    let pool = owner_pool().await;
    let (goal_id, request_id, dispatcher, _settle, _settled) =
        mhc_fixture(&pool, "REFUSE this one.").await;
    let results = dispatcher.drain().await;
    assert!(
        matches!(
            &results[..],
            [DispatchResult::Held { state: RunState::AwaitingOwner, detail, .. }]
                if detail.contains("MHC_SESSION_CLOSING")
        ),
        "{results:?}"
    );
    assert_eq!(dispatch_row(&pool, &request_id).await.0, "released");
    assert_eq!(run_state(&pool, &goal_id).await, RunState::AwaitingOwner);
}

fn receipt() -> StartReceipt {
    StartReceipt {
        request_id: "sv-goal-r1-d1".into(),
        session_id: MHC_SESSION.into(),
        run_id: mhc_run_id(MHC_SESSION, "sv-goal-r1-d1"),
    }
}

#[tokio::test]
async fn an_mhc_settle_before_the_runs_start_never_ends_it() {
    // An earlier turn's settle, then the session closes: no settle is reported.
    let (events_tx, events) = mpsc::unbounded_channel();
    let (notice, noticed) = oneshot::channel();
    events_tx.send(json!({"type": "agent_settled"})).unwrap();
    events_tx.send(json!({"type": "session_closed"})).unwrap();
    timeout(DEADLINE, settle_after_start(events, receipt(), notice))
        .await
        .expect("the watcher returns at session_closed");
    assert!(noticed.await.is_err(), "the notice was dropped unsent");

    // The settle after this run's start is the one reported.
    let (events_tx, events) = mpsc::unbounded_channel();
    let (notice, noticed) = oneshot::channel();
    for kind in ["agent_settled", "agent_start", "message_end", "agent_settled"] {
        events_tx.send(json!({"type": kind})).unwrap();
    }
    timeout(DEADLINE, settle_after_start(events, receipt(), notice))
        .await
        .expect("the watcher returns at the settle");
    assert_eq!(noticed.await.unwrap(), receipt());
}
