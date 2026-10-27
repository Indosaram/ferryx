//! An existing pane the owner chose for a new goal: bound in the admission transaction, so a
//! paired-host goal is delivered through the paired path instead of being held
//! `REMOTE_LAUNCH_UNAVAILABLE`; the choice comes only from the owner's adapter; a pane another
//! open goal holds is refused; and the dispatch status reader writes nothing.
use std::sync::Arc;

use sqlx::SqlitePool;

use crate::agent_control::{
    pty_run_id, AgentOp, Delivery, FixedLauncher, ReceiptScript, RecordingControl, LOCAL_HOST_ID,
};
use crate::goals::contract::RequirementSpec;
use crate::goals::types::SendOutcome;
use crate::supervisor::admission::{
    submit_work, ActorContext, OperationClass, OwnerChoices, SubmitOutcome, SubmitWork,
};
use crate::supervisor::dispatcher::{DispatchResult, Dispatcher, REMOTE_LAUNCH_UNAVAILABLE};
use crate::supervisor::*;

const OWNER: &str = "qa-owner";
const PAIRED: &str = "paired-host-1";
const PANE: &str = "pane-remote-1";
const BODY: &str = "Create result.txt containing READY. Run the verifier.";

async fn owner_pool() -> SqlitePool {
    let pool = crate::storage::memory().await.unwrap();
    provision_owner(&pool, OWNER, "qa-supervisor", "qa:qa-supervisor", PAIRED, "/work/qa/.git")
        .await
        .unwrap();
    pool
}

fn actor(principal: &str, message_id: &str, choices: OwnerChoices) -> ActorContext {
    ActorContext {
        principal: principal.into(),
        source_adapter: ChannelProvider::Cli,
        source_message_id: message_id.into(),
        source_body: BODY.into(),
        channel_binding_id: None,
        choices,
    }
}

fn pane_choice(session: &str) -> OwnerChoices {
    OwnerChoices {
        host_id: Some(PAIRED.into()),
        session_id: Some(session.into()),
        ..OwnerChoices::default()
    }
}

fn work() -> SubmitWork {
    SubmitWork {
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
    }
}

async fn admit(pool: &SqlitePool, message_id: &str, choices: OwnerChoices) -> (String, String) {
    match submit_work(pool, &actor(OWNER, message_id, choices), &work()).await.unwrap() {
        SubmitOutcome::Admitted(work) => (work.receipt.goal_id, work.receipt.dispatch_request_id),
        other => panic!("expected an admitted goal, got {other:?}"),
    }
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

/// The dispatcher of a hub on another machine: `PAIRED` is a paired host to it.
fn dispatcher(pool: &SqlitePool, control: Arc<RecordingControl>) -> Dispatcher {
    Dispatcher::new(pool.clone(), control, Arc::new(FixedLauncher::new("never-launched")), LOCAL_HOST_ID)
}

#[tokio::test]
async fn a_goal_bound_to_a_paired_pane_is_delivered_through_the_paired_path() {
    let pool = owner_pool().await;
    // Without a pane choice a paired-host goal cannot be launched from here: it is held.
    let (unbound, unbound_request) = admit(&pool, "m-unbound", OwnerChoices::default()).await;
    let control = Arc::new(RecordingControl::observing());
    match dispatcher(&pool, control.clone()).drain().await.as_slice() {
        [DispatchResult::Held { request_id, state: RunState::Degraded, detail }]
            if *request_id == unbound_request && detail.starts_with(REMOTE_LAUNCH_UNAVAILABLE) => {}
        other => panic!("an unbound paired goal is held {REMOTE_LAUNCH_UNAVAILABLE}: {other:?}"),
    }
    assert_eq!(goal_of(&pool, &unbound).await.unwrap().unwrap().run_state, RunState::Degraded);

    // With the owner's pane choice the binding already names the pane at commit.
    let (goal_id, request_id) = admit(&pool, "m-bound", pane_choice(PANE)).await;
    let binding = execution_binding_of(&pool, &goal_id).await.unwrap().unwrap();
    assert_eq!(
        (binding.host_id.as_str(), binding.provider_session_id.as_deref(), binding.generation),
        (PAIRED, Some(PANE), 1)
    );
    // The held goal's intent is skipped (it is not ready); the bound one is transmitted.
    let drained = dispatcher(&pool, control.clone()).drain().await;
    assert!(
        drained.contains(&DispatchResult::Transmitted {
            request_id: request_id.clone(),
            outcome: SendOutcome::Started,
        }),
        "{drained:?}"
    );
    assert!(
        drained.iter().all(|result| !matches!(result, DispatchResult::Held { .. })),
        "the bound goal is not held REMOTE_LAUNCH_UNAVAILABLE: {drained:?}"
    );
    // One send, to the paired host, to the chosen pane, carrying the dispatch's request id.
    let sends: Vec<_> = control
        .ops()
        .into_iter()
        .filter_map(|(host, op)| match op {
            AgentOp::Send { session_id, request_id, .. } => Some((host, session_id, request_id)),
            _ => None,
        })
        .collect();
    assert_eq!(sends, vec![(Some(PAIRED.to_owned()), PANE.to_owned(), request_id.clone())]);
    let dispatches = dispatches_of(&pool, &goal_id).await.unwrap();
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].state, DispatchState::Started);
    assert_eq!(dispatches[0].run_id.as_deref(), Some(pty_run_id(PANE, RecordingControl::EPOCH, 1).as_str()));
    // The run's end is then watched on the paired host in the background (covered by
    // `receipts`); the goal is not asserted here because that watch may already hold it.
}

#[tokio::test]
async fn a_paired_pane_that_accepts_without_a_start_receipt_stays_uncertain_and_is_not_resent() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool, "m-1", pane_choice(PANE)).await;
    let control = Arc::new(RecordingControl::scripted(ReceiptScript::Fixed(Delivery::Uncertain {
        accepted: true,
        code: "TURN_NOT_OBSERVED".into(),
    })));
    let dispatcher = dispatcher(&pool, control.clone());
    assert_eq!(
        dispatcher.drain().await,
        vec![DispatchResult::Transmitted {
            request_id: request_id.clone(),
            outcome: SendOutcome::Uncertain { code: "TURN_NOT_OBSERVED".into() },
        }]
    );
    let dispatch = &dispatches_of(&pool, &goal_id).await.unwrap()[0];
    assert_eq!(dispatch.state, DispatchState::Uncertain);
    assert!(dispatch.accepted_at.is_some() && dispatch.started_at.is_none() && dispatch.run_id.is_none());
    assert_eq!(goal_of(&pool, &goal_id).await.unwrap().unwrap().run_state, RunState::DeliveryUncertain);
    assert!(dispatcher.drain().await.is_empty());
    let sends = control.ops().into_iter().filter(|(_, op)| matches!(op, AgentOp::Send { .. })).count();
    assert_eq!(sends, 1, "an accepted send without a start receipt is never resent");
}

#[tokio::test]
async fn only_the_owner_can_choose_a_pane() {
    let pool = owner_pool().await;
    let refused = submit_work(&pool, &actor("slack:T1:U-stranger", "m-1", pane_choice(PANE)), &work())
        .await
        .unwrap_err();
    assert_eq!(refused.code(), "UNAUTHORIZED", "{refused}");
    // The model's proposal has no pane field at all: a session in its arguments is refused.
    let smuggled = serde_json::from_value::<SubmitWork>(serde_json::json!({
        "title": "t", "instruction": "i", "requirements": [], "sessionId": PANE,
    }));
    assert!(smuggled.is_err(), "SubmitWork must not accept a session from the model");
    // A pane choice without its host, or on another host than the goal's, is refused.
    let hostless = OwnerChoices { session_id: Some(PANE.into()), ..OwnerChoices::default() };
    let error = submit_work(&pool, &actor(OWNER, "m-2", hostless), &work()).await.unwrap_err();
    assert_eq!(error.code(), "INVALID_REQUEST", "{error}");
    for table in ["supervisor_goals", "supervisor_execution_bindings", "supervisor_dispatches"] {
        assert_eq!(count(&pool, &format!("SELECT COUNT(*) FROM {table}")).await, 0, "{table}");
    }
}

#[tokio::test]
async fn a_pane_bound_to_an_open_goal_is_not_bound_twice() {
    let pool = owner_pool().await;
    let (first, _) = admit(&pool, "m-1", pane_choice(PANE)).await;
    let error = submit_work(&pool, &actor(OWNER, "m-2", pane_choice(PANE)), &work())
        .await
        .unwrap_err();
    assert_eq!(error.code(), SESSION_ALREADY_BOUND, "{error}");
    assert!(error.to_string().contains(&first), "{error}");
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM supervisor_goals").await, 1);
    // A replay of the first admission is still answered, not refused.
    let replay = submit_work(&pool, &actor(OWNER, "m-1", pane_choice(PANE)), &work()).await.unwrap();
    assert!(matches!(replay, SubmitOutcome::Admitted(work) if work.duplicate && work.receipt.goal_id == first));
    // Once the first goal is cancelled the pane is free again.
    let goal = goal_of(&pool, &first).await.unwrap().unwrap();
    transition_goal(&pool, &first, goal.revision, goal.run_state, RunState::Cancelled, Some("owner cancel"))
        .await
        .unwrap();
    let (second, _) = admit(&pool, "m-3", pane_choice(PANE)).await;
    assert_ne!(second, first);
}

#[tokio::test]
async fn dispatch_status_reading_writes_nothing() {
    let pool = owner_pool().await;
    let (goal_id, request_id) = admit(&pool, "m-1", pane_choice(PANE)).await;
    let snapshot = || async {
        sqlx::query_scalar::<_, String>(
            "SELECT group_concat(state || ':' || updated_at, ',') FROM supervisor_dispatches",
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    let before = snapshot().await;
    let goal_before = goal_of(&pool, &goal_id).await.unwrap().unwrap();
    let dispatches = dispatches_of(&pool, &goal_id).await.unwrap();
    assert_eq!(dispatches.len(), 1);
    assert_eq!((dispatches[0].request_id.as_str(), dispatches[0].state), (request_id.as_str(), DispatchState::Ready));
    assert!(dispatches_of(&pool, "00000000-0000-4000-8000-000000000000").await.unwrap().is_empty());
    assert_eq!(snapshot().await, before);
    assert_eq!(goal_of(&pool, &goal_id).await.unwrap().unwrap(), goal_before);
}
