//! The shared supervisor request service: every action's happy path, its typed refusal, and
//! no write at all for a malformed envelope or payload. Nothing here waits on time.
use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::agent_control::{AgentOp, RecordingControl};
use crate::goals::contract::{contract_of, RequirementSpec};
use crate::goals::types::GoalMode;
use crate::supervisor::request::{
    handle, parse_envelope, status_for_code, RequestContext, MEMORY_FORGET_UNAVAILABLE,
    MEMORY_UNAVAILABLE,
};
use crate::supervisor::wakes::{StateHandler, WakeScheduler};
use crate::supervisor::{
    admit_request, admit_request_with, attach_session, bind_project_host, claim_dispatch,
    confirm_claim_before_transmit, create_project, ensure_identity, goal_of, record_receipt,
    transition_goal, waits_of, AdmitRequest, AdmitTerms, ChannelProvider, ExecutionKind,
    ExecutionTarget, Receipt, RunState, WaitState, DEFAULT_TURN_LIMIT,
};

const OWNER: &str = "slack:T1:U-owner";
const STRANGER: &str = "slack:T1:U-stranger";
const HOST: &str = "local";

struct Fixture {
    pool: SqlitePool,
    supervisor_id: String,
    project_id: String,
}

async fn fixture() -> Fixture {
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
    bind_project_host(&pool, &project.project_id, HOST, "/repo/.git")
        .await
        .unwrap();
    Fixture {
        pool,
        supervisor_id: identity.supervisor_id,
        project_id: project.project_id,
    }
}

fn request(
    fixture: &Fixture,
    message_id: &str,
    owner: &str,
    supervisor_id: &str,
    project_id: &str,
) -> AdmitRequest {
    let _ = fixture;
    AdmitRequest {
        supervisor_id: supervisor_id.into(),
        owner_principal: owner.into(),
        source_adapter: ChannelProvider::Slack,
        source_message_id: message_id.into(),
        body: format!("Do {message_id}."),
        project_id: project_id.into(),
        channel_binding_id: None,
        target: ExecutionTarget {
            kind: ExecutionKind::Pty,
            host_id: HOST.into(),
        },
        title: format!("Goal {message_id}"),
        cwd: "/repo".into(),
        instruction: format!("Do {message_id}."),
        requirements: vec![RequirementSpec {
            id: "r1".into(),
            text: "result.txt contains READY".into(),
            verifier: "verify.sh exits 0".into(),
        }],
        mode: GoalMode::Full,
        turn_limit: DEFAULT_TURN_LIMIT,
        operation_grant: Vec::new(),
    }
}

async fn admit(fixture: &Fixture, message_id: &str) -> String {
    let request = request(
        fixture,
        message_id,
        OWNER,
        &fixture.supervisor_id,
        &fixture.project_id,
    );
    admit_request(&fixture.pool, &request)
        .await
        .unwrap()
        .receipt
        .goal_id
}

fn context(principal: &str) -> RequestContext {
    RequestContext {
        principal: principal.into(),
        adapter: ChannelProvider::Desktop,
        channel_binding_id: None,
        control: None,
        memory: None,
    }
}

fn goal_call(action: &str, goal_id: &str, revision: i64, payload: Value) -> Value {
    json!({"requestId": format!("req-{action}-{revision}"), "goalId": goal_id,
        "expectedRevision": revision, "action": action, "payload": payload})
}

/// Every row an action could touch, so a refusal is proven to have written nothing.
async fn fingerprint(pool: &SqlitePool) -> Vec<(String, i64)> {
    let mut rows = Vec::new();
    for table in [
        "supervisor_goals",
        "supervisor_dispatches",
        "supervisor_requests",
        "supervisor_waits",
        "supervisor_pending_wakes",
        "supervisor_state_events",
        "lane_goal",
        "lane_goal_requirements",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(pool)
            .await
            .unwrap();
        rows.push((table.to_owned(), count));
    }
    let digest: Option<String> = sqlx::query_scalar(
        "SELECT group_concat(goal_id || ':' || run_state || ':' || revision, ',') FROM supervisor_goals",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    rows.push((digest.unwrap_or_default(), 0));
    let dispatches: Option<String> = sqlx::query_scalar(
        "SELECT group_concat(request_id || ':' || state, ',') FROM supervisor_dispatches",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    rows.push((dispatches.unwrap_or_default(), 0));
    rows
}

async fn state(pool: &SqlitePool, goal_id: &str) -> (RunState, i64) {
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    (goal.run_state, goal.revision)
}

/// Claims the ready intent and records a started run, so the goal is `running`.
async fn start_run(pool: &SqlitePool, goal_id: &str) -> String {
    start_run_on(pool, goal_id, "pane-1").await
}

async fn start_run_on(pool: &SqlitePool, goal_id: &str, session: &str) -> String {
    let (_, revision) = state(pool, goal_id).await;
    let request_id: String = sqlx::query_scalar(
        "SELECT request_id FROM supervisor_dispatches WHERE goal_id = ? AND state = 'ready'",
    )
    .bind(goal_id)
    .fetch_one(pool)
    .await
    .unwrap();
    attach_session(pool, goal_id, 1, session).await.unwrap();
    claim_dispatch(pool, &request_id, revision, 1)
        .await
        .unwrap();
    confirm_claim_before_transmit(pool, &request_id)
        .await
        .unwrap();
    record_receipt(pool, &request_id, 1, &Receipt::Accepted)
        .await
        .unwrap();
    let started = Receipt::Started {
        run_id: "run-1".into(),
        provider_session_id: session.into(),
    };
    record_receipt(pool, &request_id, 1, &started)
        .await
        .unwrap();
    transition_goal(
        pool,
        goal_id,
        revision,
        RunState::Dispatching,
        RunState::Running,
        None,
    )
    .await
    .unwrap();
    request_id
}

#[tokio::test]
async fn malformed_envelopes_and_payloads_write_nothing() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "m1").await;
    let before = fingerprint(&fixture.pool).await;
    let cases = vec![
        json!({"requestId": "x", "action": "pause", "payload": {}}),
        json!({"requestId": "x", "goalId": goal_id, "expectedRevision": 1, "action": "explode", "payload": {}}),
        json!({"requestId": "x", "goalId": goal_id, "expectedRevision": 1, "action": "pause", "payload": {}, "actor": OWNER}),
        goal_call(
            "pause",
            &goal_id,
            1,
            json!({"reason": "x", "approveAll": true}),
        ),
        goal_call("resume", &goal_id, 1, json!({"why": 1})),
        goal_call("correct", &goal_id, 1, json!({"text": "   "})),
        goal_call(
            "correct",
            &goal_id,
            1,
            json!({"text": "Use DONE", "requirements": []}),
        ),
        goal_call("depend", &goal_id, 1, json!({})),
        goal_call(
            "depend",
            &goal_id,
            1,
            json!({"childGoalId": "a", "eventKey": "b"}),
        ),
        goal_call("approve", &goal_id, 1, json!({"operations": ["publish"]})),
        json!({"requestId": "x", "goalId": goal_id, "action": "submit", "payload": {}}),
        json!({"requestId": " ", "action": "message", "payload": {"text": "hi"}}),
        json!({"requestId": "x", "action": "teach", "payload": {"text": ""}}),
        json!({"requestId": "x", "action": "forget", "payload": {}}),
        json!("not an object"),
    ];
    for case in cases {
        let refused = handle(&fixture.pool, &context(OWNER), case.clone())
            .await
            .unwrap_err();
        assert_eq!(refused.code, "INVALID_REQUEST", "{case}: {refused}");
        assert_eq!(refused.status, 400, "{case}");
    }
    assert_eq!(fingerprint(&fixture.pool).await, before);
    assert!(parse_envelope(
        json!({"requestId": "x", "action": "message", "payload": {"text": "hi"}})
    )
    .is_ok());
}

#[tokio::test]
async fn pause_and_resume_hold_unsent_work_and_requeue_it() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "p1").await;

    let paused = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("pause", &goal_id, 1, json!({"reason": "lunch"})),
    )
    .await
    .unwrap();
    assert_eq!(paused["runState"], "paused");
    assert_eq!(paused["revision"], 2);
    assert!(paused["stateDetail"]
        .as_str()
        .unwrap()
        .starts_with("OWNER_PAUSED"));
    let ready: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM supervisor_dispatches WHERE goal_id = ? AND state = 'ready'",
    )
    .bind(&goal_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(ready, 0, "a pause gates unsent work");

    // Stale revision: 409, and nothing changes.
    let before = fingerprint(&fixture.pool).await;
    let stale = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("resume", &goal_id, 1, json!({})),
    )
    .await
    .unwrap_err();
    assert_eq!(
        (stale.code.as_str(), stale.status),
        ("REVISION_CONFLICT", 409)
    );
    assert_eq!(fingerprint(&fixture.pool).await, before);

    let resumed = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("resume", &goal_id, 2, json!({})),
    )
    .await
    .unwrap();
    assert_eq!(resumed["runState"], "ready");
    let requeued: String = sqlx::query_scalar(
        "SELECT instruction FROM supervisor_dispatches WHERE goal_id = ? AND state = 'ready'",
    )
    .bind(&goal_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(requeued, "Do p1.");

    // Resume of a goal that is not paused is a conflict, not a silent success.
    let again = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("resume", &goal_id, 2, json!({})),
    )
    .await
    .unwrap_err();
    assert_eq!(again.code, "REVISION_CONFLICT");
}

#[tokio::test]
async fn pause_keeps_a_running_worker_and_resume_returns_to_running() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "p2").await;
    start_run(&fixture.pool, &goal_id).await;
    let (_, revision) = state(&fixture.pool, &goal_id).await;
    let control = Arc::new(RecordingControl::default());
    let mut ctx = context(OWNER);
    ctx.control = Some(control.clone());
    handle(
        &fixture.pool,
        &ctx,
        goal_call("pause", &goal_id, revision, json!({})),
    )
    .await
    .unwrap();
    assert!(control.ops().is_empty(), "a pause never stops the worker");
    let resumed = handle(
        &fixture.pool,
        &ctx,
        goal_call("resume", &goal_id, revision + 1, json!({})),
    )
    .await
    .unwrap();
    assert_eq!(resumed["runState"], "running");
}

#[tokio::test]
async fn cancel_stops_the_live_run_and_fails_the_parents_wait() {
    let fixture = fixture().await;
    let parent = admit(&fixture, "parent").await;
    let child = admit(&fixture, "child").await;
    let wait = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("depend", &parent, 1, json!({"childGoalId": child})),
    )
    .await
    .unwrap();
    assert!(wait["waitId"].is_string());
    assert_eq!(wait["pendingWaits"].as_array().unwrap().len(), 1);
    start_run(&fixture.pool, &child).await;
    let (_, revision) = state(&fixture.pool, &child).await;

    let control = Arc::new(RecordingControl::default());
    let mut ctx = context(OWNER);
    ctx.control = Some(control.clone());
    let cancelled = handle(
        &fixture.pool,
        &ctx,
        goal_call("cancel", &child, revision, json!({"reason": "wrong idea"})),
    )
    .await
    .unwrap();
    assert_eq!(cancelled["runState"], "cancelled");
    assert_eq!(cancelled["workerStop"]["state"], "sent");
    let ops = control.ops();
    assert_eq!(ops.len(), 1);
    assert!(matches!(&ops[0].1, AgentOp::Stop { session_id, .. } if session_id == "pane-1"));

    // The parent's wait fails through the wake scheduler: failure, never success.
    let scheduler = WakeScheduler::new(
        fixture.pool.clone(),
        Arc::new(StateHandler::new(fixture.pool.clone())),
    );
    scheduler.tick(chrono::Utc::now()).await.unwrap();
    let waits = waits_of(&fixture.pool, &parent).await.unwrap();
    assert_eq!(waits[0].state, WaitState::Failed);

    // A cancelled goal accepts no further action.
    let (_, after) = state(&fixture.pool, &child).await;
    let closed = handle(
        &fixture.pool,
        &ctx,
        goal_call("pause", &child, after, json!({})),
    )
    .await
    .unwrap_err();
    assert_eq!(closed.status, 409, "{closed}");
}

#[tokio::test]
async fn correct_revises_and_queues_the_correction_and_refuses_stale_revisions() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "c1").await;
    let corrected = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("correct", &goal_id, 1, json!({"text": "Use DONE instead of READY",
            "requirements": [{"id": "r1", "text": "result.txt contains DONE", "verifier": "verify.sh exits 0"}]})),
    )
    .await
    .unwrap();
    assert_eq!(corrected["revision"], 2);
    let request_id = corrected["dispatchRequestId"].as_str().unwrap();
    assert!(request_id.contains("-r2-"), "{request_id}");
    let states: Vec<(String, String)> = sqlx::query_as(
        "SELECT request_id, state FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id",
    )
    .bind(&goal_id)
    .fetch_all(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(
        states[0].1, "superseded",
        "the old dispatch can never be sent"
    );
    assert_eq!(states[1], (request_id.to_owned(), "ready".to_owned()));
    assert_eq!(corrected["carriedOriginal"], true);
    let instruction: String =
        sqlx::query_scalar("SELECT instruction FROM supervisor_dispatches WHERE request_id = ?")
            .bind(request_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert!(
        instruction.starts_with("Do c1."),
        "the never-sent task is kept: {instruction}"
    );
    assert!(
        instruction.contains("Use DONE instead of READY"),
        "{instruction}"
    );
    assert!(
        instruction.contains("result.txt contains DONE"),
        "{instruction}"
    );
    let lane: String =
        sqlx::query_scalar("SELECT lane_key FROM supervisor_goals WHERE goal_id = ?")
            .bind(&goal_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    let contract = contract_of(&fixture.pool, &lane).await.unwrap().unwrap();
    assert_eq!(contract.requirements[0].text, "result.txt contains DONE");

    let before = fingerprint(&fixture.pool).await;
    let stale = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("correct", &goal_id, 1, json!({"text": "Use READY"})),
    )
    .await
    .unwrap_err();
    assert_eq!(
        (stale.code.as_str(), stale.status),
        ("REVISION_CONFLICT", 409)
    );
    assert_eq!(fingerprint(&fixture.pool).await, before);
}

#[tokio::test]
async fn approve_releases_an_owner_hold_without_widening_the_grant() {
    let fixture = fixture().await;
    let held = request(
        &fixture,
        "a1",
        OWNER,
        &fixture.supervisor_id,
        &fixture.project_id,
    );
    let terms = AdmitTerms {
        owner_hold: Some("SCOPE_NEEDS_OWNER: publish needs the owner's explicit approval".into()),
        ..AdmitTerms::default()
    };
    let goal_id = admit_request_with(
        &fixture.pool,
        &AdmitRequest {
            mode: GoalMode::Supervised,
            ..held
        },
        &terms,
    )
    .await
    .unwrap()
    .receipt
    .goal_id;
    let (run_state, revision) = state(&fixture.pool, &goal_id).await;
    assert_eq!(run_state, RunState::AwaitingOwner);

    let approved = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("approve", &goal_id, revision, json!({})),
    )
    .await
    .unwrap();
    assert_eq!(approved["runState"], "ready");
    assert_eq!(
        approved["grant"],
        json!([]),
        "an approval never adds operations"
    );
    let ready: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM supervisor_dispatches WHERE goal_id = ? AND state = 'ready'",
    )
    .bind(&goal_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(ready, 1);

    // Approving a goal that holds nothing is a conflict.
    let (_, now) = state(&fixture.pool, &goal_id).await;
    let nothing = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("approve", &goal_id, now, json!({})),
    )
    .await
    .unwrap_err();
    assert_eq!(nothing.code, "REVISION_CONFLICT");
}

#[tokio::test]
async fn approve_cannot_restore_a_revoked_grant_or_raise_the_budget() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "a2").await;
    transition_goal(
        &fixture.pool,
        &goal_id,
        1,
        RunState::Ready,
        RunState::AwaitingOwner,
        Some("GRANT_REVOKED: project is no longer mapped"),
    )
    .await
    .unwrap();
    let before = fingerprint(&fixture.pool).await;
    let refused = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("approve", &goal_id, 2, json!({})),
    )
    .await
    .unwrap_err();
    assert_eq!((refused.code.as_str(), refused.status), ("FORBIDDEN", 403));
    assert_eq!(fingerprint(&fixture.pool).await, before);

    let spent = admit(&fixture, "a3").await;
    transition_goal(
        &fixture.pool,
        &spent,
        1,
        RunState::Ready,
        RunState::AwaitingOwner,
        Some("TURN_LIMIT: 20 of 20 turns are used"),
    )
    .await
    .unwrap();
    let refused = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("approve", &spent, 2, json!({})),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code, "TURN_LIMIT");
}

#[tokio::test]
async fn non_owners_and_other_owners_goals_are_refused_without_writes() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "o1").await;
    let before = fingerprint(&fixture.pool).await;
    for action in ["pause", "cancel", "approve", "resume"] {
        let refused = handle(
            &fixture.pool,
            &context(STRANGER),
            goal_call(action, &goal_id, 1, json!({})),
        )
        .await
        .unwrap_err();
        assert_eq!(
            (refused.code.as_str(), refused.status),
            ("UNAUTHORIZED", 403),
            "{action}"
        );
    }
    let submit = json!({"requestId": "s-stranger", "action": "submit", "payload": {"request": "Ship it.",
        "work": {"title": "t", "instruction": "i", "requirements": [{"id": "r", "text": "t", "verifier": "v"}]}}});
    let refused = handle(&fixture.pool, &context(STRANGER), submit)
        .await
        .unwrap_err();
    assert_eq!(refused.status, 403);

    // A second owner on the same hub cannot touch the first owner's goal.
    ensure_identity(&fixture.pool, STRANGER).await.unwrap();
    let refused = handle(
        &fixture.pool,
        &context(STRANGER),
        goal_call("pause", &goal_id, 1, json!({})),
    )
    .await
    .unwrap_err();
    assert_eq!((refused.code.as_str(), refused.status), ("FORBIDDEN", 403));
    let missing = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call(
            "pause",
            "00000000-0000-4000-8000-000000000000",
            1,
            json!({}),
        ),
    )
    .await
    .unwrap_err();
    assert_eq!((missing.code.as_str(), missing.status), ("NOT_FOUND", 404));
    let unrelated: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM supervisor_identity")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(unrelated, 2);
    let mut after = fingerprint(&fixture.pool).await;
    after.retain(|row| row.0 != "supervisor_identity");
    assert_eq!(after, before);
}

#[tokio::test]
async fn depend_refuses_self_cycles_and_cross_owner_links_with_400() {
    let fixture = fixture().await;
    let a = admit(&fixture, "d-a").await;
    let b = admit(&fixture, "d-b").await;
    let self_wait = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("depend", &a, 1, json!({"childGoalId": a})),
    )
    .await
    .unwrap_err();
    assert_eq!(
        (self_wait.code.as_str(), self_wait.status),
        ("WAIT_CYCLE", 400)
    );
    handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("depend", &a, 1, json!({"childGoalId": b})),
    )
    .await
    .unwrap();
    let before = fingerprint(&fixture.pool).await;
    let cycle = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("depend", &b, 1, json!({"childGoalId": a})),
    )
    .await
    .unwrap_err();
    assert_eq!((cycle.code.as_str(), cycle.status), ("WAIT_CYCLE", 400));
    assert_eq!(fingerprint(&fixture.pool).await, before);

    let other = ensure_identity(&fixture.pool, STRANGER).await.unwrap();
    let project = create_project(
        &fixture.pool,
        &other.supervisor_id,
        "other",
        "github.com/example/other",
    )
    .await
    .unwrap();
    bind_project_host(&fixture.pool, &project.project_id, HOST, "/other/.git")
        .await
        .unwrap();
    let foreign = request(
        &fixture,
        "d-foreign",
        STRANGER,
        &other.supervisor_id,
        &project.project_id,
    );
    let foreign = admit_request(&fixture.pool, &foreign)
        .await
        .unwrap()
        .receipt
        .goal_id;
    let cross = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("depend", &a, 1, json!({"childGoalId": foreign})),
    )
    .await
    .unwrap_err();
    assert_eq!((cross.code.as_str(), cross.status), ("CROSS_OWNER", 400));

    let event = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("depend", &b, 1, json!({"eventKey": "ci:main"})),
    )
    .await
    .unwrap();
    assert_eq!(event["pendingWaits"][0]["eventKey"], "ci:main");
}

#[tokio::test]
async fn submit_and_message_go_through_admission_and_replay_the_same_goal() {
    let fixture = fixture().await;
    let submit = json!({"requestId": "s1", "projectId": fixture.project_id, "action": "submit", "payload": {
        "request": "Create result.txt containing READY. Run the verifier.",
        "work": {"title": "Write result.txt", "instruction": "Create result.txt containing READY.",
            "operations": ["edit", "test"],
            "requirements": [{"id": "r1", "text": "result.txt contains READY", "verifier": "verify.sh exits 0"}]}}});
    let first = handle(&fixture.pool, &context(OWNER), submit.clone())
        .await
        .unwrap();
    assert_eq!(first["outcome"], "admitted");
    let replay = handle(&fixture.pool, &context(OWNER), submit)
        .await
        .unwrap();
    assert_eq!(replay["receipt"]["goal_id"], first["receipt"]["goal_id"]);
    assert_eq!(replay["duplicate"], true);
    let chat =
        json!({"requestId": "q1", "action": "message", "payload": {"text": "What can you do?"}});
    assert_eq!(
        handle(&fixture.pool, &context(OWNER), chat).await.unwrap()["outcome"],
        "conversational"
    );
    let goals: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM supervisor_goals")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(goals, 1);
}

#[tokio::test]
async fn memory_actions_without_an_engine_are_unavailable_and_change_nothing() {
    let fixture = fixture().await;
    let before = fingerprint(&fixture.pool).await;
    let teach = json!({"requestId": "t1", "action": "teach", "payload": {"text": "use the QA remote host"}});
    let refused = handle(&fixture.pool, &context(OWNER), teach.clone())
        .await
        .unwrap_err();
    assert_eq!(
        (refused.code.as_str(), refused.status),
        (MEMORY_UNAVAILABLE, 503)
    );
    let stranger = handle(&fixture.pool, &context(STRANGER), teach)
        .await
        .unwrap_err();
    assert_eq!(stranger.status, 403, "a non-owner cannot teach");
    let forget = json!({"requestId": "f1", "action": "forget", "payload": {"memoryId": "m-1"}});
    let refused = handle(&fixture.pool, &context(OWNER), forget)
        .await
        .unwrap_err();
    assert_eq!(
        (refused.code.as_str(), refused.status),
        (MEMORY_FORGET_UNAVAILABLE, 503)
    );
    assert_eq!(fingerprint(&fixture.pool).await, before);
}

#[test]
fn wire_codes_map_to_the_contract_statuses() {
    for (code, status) in [
        ("INVALID_REQUEST", 400),
        ("WAIT_CYCLE", 400),
        ("UNAUTHORIZED", 403),
        ("FORBIDDEN", 403),
        ("NOT_FOUND", 404),
        ("REVISION_CONFLICT", 409),
        ("GOAL_CLOSED", 409),
        ("DISPATCH_IN_FLIGHT", 409),
        ("TURN_LIMIT", 409),
        ("SUPERVISOR_OWNED", 409),
        ("SUPERVISOR_UNAVAILABLE", 503),
        ("JUDGE_UNAVAILABLE", 503),
        ("STORAGE", 500),
        ("SOMETHING_NEW", 500),
        ("MHC_NOT_RUNNING", 503),
        ("MEMORY_UNAVAILABLE", 503),
    ] {
        assert_eq!(status_for_code(code), status, "{code}");
    }
}

async fn ready_instructions(pool: &SqlitePool, goal_id: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT instruction FROM supervisor_dispatches WHERE goal_id = ? AND state = 'ready'",
    )
    .bind(goal_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

/// Claims the ready intent so the goal is `dispatching`, without transmitting it.
async fn claim(pool: &SqlitePool, goal_id: &str) -> String {
    let (_, revision) = state(pool, goal_id).await;
    let request_id: String = sqlx::query_scalar(
        "SELECT request_id FROM supervisor_dispatches WHERE goal_id = ? AND state = 'ready'",
    )
    .bind(goal_id)
    .fetch_one(pool)
    .await
    .unwrap();
    attach_session(pool, goal_id, 1, "pane-1").await.unwrap();
    claim_dispatch(pool, &request_id, revision, 1)
        .await
        .unwrap();
    request_id
}

async fn settle(pool: &SqlitePool, request_id: &str) {
    let settled = Receipt::Settled {
        run_id: "run-1".into(),
        provider_session_id: "pane-1".into(),
    };
    record_receipt(pool, request_id, 1, &settled).await.unwrap();
}

#[tokio::test]
async fn resume_after_the_run_settled_during_the_pause_goes_to_verifying() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "s1").await;
    let request_id = start_run(&fixture.pool, &goal_id).await;
    let (_, revision) = state(&fixture.pool, &goal_id).await;
    handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("pause", &goal_id, revision, json!({})),
    )
    .await
    .unwrap();
    settle(&fixture.pool, &request_id).await;
    let resumed = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("resume", &goal_id, revision + 1, json!({})),
    )
    .await
    .unwrap();
    assert_eq!(resumed["runState"], "verifying", "{resumed}");
    assert_eq!(resumed["restored"], "verifying");
    assert_eq!(resumed["stateDetail"], "TURN_SETTLED");
    assert!(
        ready_instructions(&fixture.pool, &goal_id).await.is_empty(),
        "a settled run is not resent"
    );
    let held: Option<String> =
        sqlx::query_scalar("SELECT held_from FROM supervisor_goals WHERE goal_id = ?")
            .bind(&goal_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(held, None, "the pre-pause state is cleared on resume");
}

#[tokio::test]
async fn resume_with_pending_waits_returns_to_waiting() {
    let fixture = fixture().await;
    let parent = admit(&fixture, "w-parent").await;
    let child = admit(&fixture, "w-child").await;
    let request_id = start_run(&fixture.pool, &parent).await;
    let (_, revision) = state(&fixture.pool, &parent).await;
    handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("depend", &parent, revision, json!({"childGoalId": child})),
    )
    .await
    .unwrap();
    handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("pause", &parent, revision, json!({})),
    )
    .await
    .unwrap();
    settle(&fixture.pool, &request_id).await;
    let resumed = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("resume", &parent, revision + 1, json!({})),
    )
    .await
    .unwrap();
    assert_eq!(resumed["runState"], "waiting_children", "{resumed}");
    assert!(resumed["stateDetail"]
        .as_str()
        .unwrap()
        .starts_with("WAITING_ON"));
}

#[tokio::test]
async fn resume_requeues_an_intent_whose_claim_was_released_by_the_pause() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "r1").await;
    let request_id = claim(&fixture.pool, &goal_id).await;
    handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("pause", &goal_id, 1, json!({})),
    )
    .await
    .unwrap();
    // The dispatcher's pre-transmit check sees the moved revision and releases the claim.
    assert!(confirm_claim_before_transmit(&fixture.pool, &request_id)
        .await
        .is_err());
    let released: String =
        sqlx::query_scalar("SELECT state FROM supervisor_dispatches WHERE request_id = ?")
            .bind(&request_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(released, "released");
    let resumed = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("resume", &goal_id, 2, json!({})),
    )
    .await
    .unwrap();
    assert_eq!(resumed["runState"], "ready");
    assert_eq!(
        ready_instructions(&fixture.pool, &goal_id).await,
        vec!["Do r1.".to_owned()]
    );
}

#[tokio::test]
async fn approve_requeues_a_released_send_refused_intent_and_a_stale_approve_changes_nothing() {
    let fixture = fixture().await;
    let held = request(
        &fixture,
        "a4",
        OWNER,
        &fixture.supervisor_id,
        &fixture.project_id,
    );
    let goal_id = admit_request(
        &fixture.pool,
        &AdmitRequest {
            mode: GoalMode::Supervised,
            ..held
        },
    )
    .await
    .unwrap()
    .receipt
    .goal_id;
    let request_id = claim(&fixture.pool, &goal_id).await;
    transition_goal(
        &fixture.pool,
        &goal_id,
        1,
        RunState::Dispatching,
        RunState::AwaitingOwner,
        Some("SEND_REFUSED: pane busy"),
    )
    .await
    .unwrap();
    assert!(confirm_claim_before_transmit(&fixture.pool, &request_id)
        .await
        .is_err());

    let before = fingerprint(&fixture.pool).await;
    let stale = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("approve", &goal_id, 1, json!({})),
    )
    .await
    .unwrap_err();
    assert_eq!(
        (stale.code.as_str(), stale.status),
        ("REVISION_CONFLICT", 409)
    );
    assert_eq!(fingerprint(&fixture.pool).await, before);
    let lane: String =
        sqlx::query_scalar("SELECT lane_key FROM supervisor_goals WHERE goal_id = ?")
            .bind(&goal_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(
        contract_of(&fixture.pool, &lane)
            .await
            .unwrap()
            .unwrap()
            .mode,
        GoalMode::Supervised
    );

    let approved = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("approve", &goal_id, 2, json!({})),
    )
    .await
    .unwrap();
    assert_eq!(approved["runState"], "ready");
    assert_eq!(approved["modeChanged"], true);
    assert_eq!(
        ready_instructions(&fixture.pool, &goal_id).await,
        vec!["Do a4.".to_owned()]
    );
}

#[tokio::test]
async fn an_oversized_correction_is_refused_before_any_write() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "c3").await;
    let before = fingerprint(&fixture.pool).await;
    let text = "x".repeat(63_990);
    let refused = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call(
            "correct",
            &goal_id,
            1,
            json!({"text": text,
        "requirements": [{"id": "r1", "text": "t", "verifier": "v"}]}),
        ),
    )
    .await
    .unwrap_err();
    assert_eq!(
        (refused.code.as_str(), refused.status),
        ("INVALID_REQUEST", 400),
        "{refused}"
    );
    assert_eq!(fingerprint(&fixture.pool).await, before);
}

#[tokio::test]
async fn a_stale_depend_writes_no_wait() {
    let fixture = fixture().await;
    let parent = admit(&fixture, "d-parent").await;
    let child = admit(&fixture, "d-child").await;
    let before = fingerprint(&fixture.pool).await;
    let stale = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("depend", &parent, 7, json!({"childGoalId": child})),
    )
    .await
    .unwrap_err();
    assert_eq!(
        (stale.code.as_str(), stale.status),
        ("REVISION_CONFLICT", 409)
    );
    // The compare-and-set inside the wait's own transaction refuses the same way.
    let inner = crate::supervisor::wakes::add_dependency_at(
        &fixture.pool,
        &parent,
        &crate::supervisor::WaitTarget::ChildGoal(child.clone()),
        Some(7),
    )
    .await
    .unwrap_err();
    assert_eq!(inner.http_status(), 409);
    assert_eq!(fingerprint(&fixture.pool).await, before);
}

#[tokio::test]
async fn a_correction_fences_a_claimed_send_and_the_claim_is_released_untransmitted() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "cf1").await;
    let request_id = claim(&fixture.pool, &goal_id).await;
    let corrected = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("correct", &goal_id, 1, json!({"text": "Use DONE"})),
    )
    .await
    .unwrap();
    assert_eq!(corrected["revision"], 2);
    assert_eq!(
        corrected["carriedOriginal"], false,
        "a claimed send may be typed already; it is not carried"
    );
    // The dispatcher's pre-transmit check sees the moved revision: the claim is released, never typed.
    assert!(confirm_claim_before_transmit(&fixture.pool, &request_id)
        .await
        .is_err());
    let state: String =
        sqlx::query_scalar("SELECT state FROM supervisor_dispatches WHERE request_id = ?")
            .bind(&request_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(state, "released");
    let ready = ready_instructions(&fixture.pool, &goal_id).await;
    assert_eq!(ready.len(), 1);
    assert!(ready[0].contains("Use DONE"), "{ready:?}");
}

#[tokio::test]
async fn approve_of_a_hold_placed_after_a_settled_run_goes_to_verifying() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "v1").await;
    let request_id = start_run(&fixture.pool, &goal_id).await;
    settle(&fixture.pool, &request_id).await;
    let (_, revision) = state(&fixture.pool, &goal_id).await;
    // A verify-time hold (T10's OWNER_APPROVAL_REQUIRED / model stop): no unsent intent exists.
    transition_goal(
        &fixture.pool,
        &goal_id,
        revision,
        RunState::Running,
        RunState::AwaitingOwner,
        Some("OWNER_APPROVAL_REQUIRED: a supervised goal's verdict needs the owner"),
    )
    .await
    .unwrap();
    let approved = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("approve", &goal_id, revision + 1, json!({})),
    )
    .await
    .unwrap();
    assert_eq!(approved["runState"], "verifying", "{approved}");
    assert_eq!(approved["stateDetail"], "TURN_SETTLED");
    assert!(
        ready_instructions(&fixture.pool, &goal_id).await.is_empty(),
        "a settled run is not resent"
    );
}

#[tokio::test]
async fn approve_with_nothing_to_release_is_refused_and_resume_holds_instead_of_an_empty_ready() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "n1").await;
    start_run(&fixture.pool, &goal_id).await;
    let (_, revision) = state(&fixture.pool, &goal_id).await;
    // The run was fenced away (a new generation): no live run, nothing unsent, nothing settled.
    sqlx::query(
        "UPDATE supervisor_execution_bindings SET generation = generation + 1 WHERE goal_id = ?",
    )
    .bind(&goal_id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    transition_goal(
        &fixture.pool,
        &goal_id,
        revision,
        RunState::Running,
        RunState::AwaitingOwner,
        Some("OWNER_APPROVAL_REQUIRED: model stop"),
    )
    .await
    .unwrap();
    let before = fingerprint(&fixture.pool).await;
    let refused = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("approve", &goal_id, revision + 1, json!({})),
    )
    .await
    .unwrap_err();
    assert_eq!(
        (refused.code.as_str(), refused.status),
        ("NOTHING_TO_RELEASE", 409),
        "{refused}"
    );
    assert_eq!(fingerprint(&fixture.pool).await, before);

    let paused_goal = admit(&fixture, "n2").await;
    start_run_on(&fixture.pool, &paused_goal, "pane-2").await;
    let (_, at) = state(&fixture.pool, &paused_goal).await;
    handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("pause", &paused_goal, at, json!({})),
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE supervisor_execution_bindings SET generation = generation + 1 WHERE goal_id = ?",
    )
    .bind(&paused_goal)
    .execute(&fixture.pool)
    .await
    .unwrap();
    let resumed = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("resume", &paused_goal, at + 1, json!({})),
    )
    .await
    .unwrap();
    assert_eq!(resumed["runState"], "awaiting_owner", "{resumed}");
    assert!(resumed["stateDetail"]
        .as_str()
        .unwrap()
        .starts_with("OWNER_DECISION_NEEDED"));
}

#[tokio::test]
async fn resume_clears_a_legacy_lane_pause_row_so_the_dispatcher_does_not_hold_it_again() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "lp1").await;
    let lane: String =
        sqlx::query_scalar("SELECT lane_key FROM supervisor_goals WHERE goal_id = ?")
            .bind(&goal_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    // A row written by the pre-T13 legacy pause path on a supervisor lane.
    crate::lanes::pause::pause(&fixture.pool, &lane, "desktop")
        .await
        .unwrap();
    handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("pause", &goal_id, 1, json!({})),
    )
    .await
    .unwrap();
    let resumed = handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("resume", &goal_id, 2, json!({})),
    )
    .await
    .unwrap();
    assert_eq!(resumed["runState"], "ready");
    assert!(
        !crate::lanes::pause::is_paused(&fixture.pool, &lane)
            .await
            .unwrap(),
        "the lane pause row is cleared"
    );
    assert_eq!(ready_instructions(&fixture.pool, &goal_id).await.len(), 1);

    // The legacy lane resume adapter on a goal that is not paused clears a stranded row too.
    let other = admit(&fixture, "lp2").await;
    let other_lane: String =
        sqlx::query_scalar("SELECT lane_key FROM supervisor_goals WHERE goal_id = ?")
            .bind(&other)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    crate::lanes::pause::pause(&fixture.pool, &other_lane, "desktop")
        .await
        .unwrap();
    let changed =
        crate::supervisor::request::lane_pause(&fixture.pool, &context(OWNER), &other_lane, false)
            .await
            .unwrap();
    assert_eq!(changed, Some(true));
    assert!(!crate::lanes::pause::is_paused(&fixture.pool, &other_lane)
        .await
        .unwrap());
    assert_eq!(
        state(&fixture.pool, &other).await,
        (RunState::Ready, 1),
        "the goal itself is unchanged"
    );
}

#[tokio::test]
async fn cancel_clears_the_pre_pause_state() {
    let fixture = fixture().await;
    let goal_id = admit(&fixture, "cc1").await;
    handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("pause", &goal_id, 1, json!({})),
    )
    .await
    .unwrap();
    handle(
        &fixture.pool,
        &context(OWNER),
        goal_call("cancel", &goal_id, 2, json!({})),
    )
    .await
    .unwrap();
    let held: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT held_from, held_detail FROM supervisor_goals WHERE goal_id = ?")
            .bind(&goal_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(held, (None, None));
}
