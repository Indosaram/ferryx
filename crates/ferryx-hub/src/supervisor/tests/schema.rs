use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use chrono::Utc;
use sqlx::error::BoxDynError;
use sqlx::migrate::{Migration, MigrationSource, Migrator};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::goals::contract::{contract_of, RequirementSpec};
use crate::goals::types::GoalMode;
use crate::supervisor::*;

/// Last migration of a hub that predates the unified supervisor.
const OLD_SCHEMA_VERSION: i64 = 19;

/// Version of the migration under test.
const SUPERVISOR_SCHEMA_VERSION: i64 = 20;

const SUPERVISOR_TABLES: &[&str] = &[
    "supervisor_identity",
    "supervisor_projects",
    "supervisor_project_hosts",
    "supervisor_channel_bindings",
    "supervisor_goals",
    "supervisor_requests",
    "supervisor_execution_bindings",
    "supervisor_dispatches",
    "supervisor_waits",
    "supervisor_wait_events",
    "supervisor_pending_wakes",
];

const OWNER: &str = "slack:T1:U-owner";
const STRANGER: &str = "slack:T1:U-stranger";
const HOST: &str = "host-a";
const BODY: &str = "Create result.txt containing READY. Run the verifier.";

type Resolved = Pin<Box<dyn Future<Output = Result<Vec<Migration>, BoxDynError>> + Send>>;

/// The embedded migrations up to [`OLD_SCHEMA_VERSION`]: the schema an existing hub has.
#[derive(Debug)]
struct OldSchema;

impl MigrationSource<'static> for OldSchema {
    fn resolve(self) -> Resolved {
        Box::pin(async {
            Ok(sqlx::migrate!("./migrations")
                .iter()
                .filter(|migration| migration.version <= OLD_SCHEMA_VERSION)
                .cloned()
                .collect())
        })
    }
}

/// An in-memory database with no migrations applied. One connection: each connection of a
/// memory pool would be a different database.
async fn bare_memory_pool() -> SqlitePool {
    let options = "sqlite::memory:"
        .parse::<SqliteConnectOptions>()
        .unwrap()
        .foreign_keys(true);
    SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(options)
        .await
        .unwrap()
}

/// A WAL file database with several connections, so writers really contend.
async fn file_pool() -> (SqlitePool, PathBuf) {
    let dir = std::env::temp_dir().join(format!("ferryx-hub-supervisor-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let pool = crate::storage::connect(&dir.join("hub.sqlite"))
        .await
        .unwrap();
    (pool, dir)
}

struct Fixture {
    pool: SqlitePool,
    identity: SupervisorIdentity,
    project: Project,
}

async fn fixture_on(pool: SqlitePool) -> Fixture {
    let identity = ensure_identity(&pool, OWNER).await.unwrap();
    let project = create_project(
        &pool,
        &identity.supervisor_id,
        "ferryx",
        "github.com/example/ferryx",
    )
    .await
    .unwrap();
    bind_project_host(
        &pool,
        &project.project_id,
        HOST,
        "/home/owner/code/ferryx/.git",
    )
    .await
    .unwrap();
    Fixture {
        pool,
        identity,
        project,
    }
}

async fn fixture() -> Fixture {
    fixture_on(crate::storage::memory().await.unwrap()).await
}

fn request(fixture: &Fixture, message_id: &str, body: &str) -> AdmitRequest {
    AdmitRequest {
        supervisor_id: fixture.identity.supervisor_id.clone(),
        owner_principal: OWNER.into(),
        source_adapter: ChannelProvider::Slack,
        source_message_id: message_id.into(),
        body: body.into(),
        project_id: fixture.project.project_id.clone(),
        channel_binding_id: None,
        target: ExecutionTarget {
            kind: ExecutionKind::Pty,
            host_id: HOST.into(),
        },
        title: "Write result.txt".into(),
        cwd: "/home/owner/code/ferryx".into(),
        instruction: body.into(),
        requirements: vec![RequirementSpec {
            id: "r1".into(),
            text: "result.txt contains READY".into(),
            verifier: "verify.sh exits 0".into(),
        }],
        mode: GoalMode::Full,
        turn_limit: DEFAULT_TURN_LIMIT,
    }
}

async fn admit(fixture: &Fixture, message_id: &str) -> AdmissionReceipt {
    admit_request(&fixture.pool, &request(fixture, message_id, BODY))
        .await
        .unwrap()
        .receipt
}

fn started(run_id: &str, session: &str) -> Receipt {
    Receipt::Started {
        run_id: run_id.into(),
        provider_session_id: session.into(),
    }
}

fn settled(run_id: &str, session: &str) -> Receipt {
    Receipt::Settled {
        run_id: run_id.into(),
        provider_session_id: session.into(),
    }
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

async fn ready_intents(pool: &SqlitePool) -> i64 {
    count(
        pool,
        "SELECT COUNT(*) FROM supervisor_dispatches WHERE state = 'ready'",
    )
    .await
}

/// Rows of every table one admission writes: lane, contract, requirement, goal, binding,
/// request and dispatch intent.
async fn admission_rows(pool: &SqlitePool) -> [i64; 7] {
    [
        count(pool, "SELECT COUNT(*) FROM lanes WHERE key LIKE 'sv-%'").await,
        count(
            pool,
            "SELECT COUNT(*) FROM lane_goal WHERE lane_key LIKE 'sv-%'",
        )
        .await,
        count(
            pool,
            "SELECT COUNT(*) FROM lane_goal_requirements WHERE lane_key LIKE 'sv-%'",
        )
        .await,
        count(pool, "SELECT COUNT(*) FROM supervisor_goals").await,
        count(pool, "SELECT COUNT(*) FROM supervisor_execution_bindings").await,
        count(pool, "SELECT COUNT(*) FROM supervisor_requests").await,
        count(pool, "SELECT COUNT(*) FROM supervisor_dispatches").await,
    ]
}

/// Row count of every table except the migration ledger.
async fn row_counts(pool: &SqlitePool) -> BTreeMap<String, i64> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name <> '_sqlx_migrations'
         ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let mut counts = BTreeMap::new();
    for table in tables {
        let sql = format!("SELECT COUNT(*) FROM \"{table}\"");
        let rows = count(pool, &sql).await;
        counts.insert(table, rows);
    }
    counts
}

fn assert_vocabulary<T>(all: &[T], as_str: fn(T) -> &'static str)
where
    T: Copy
        + std::fmt::Debug
        + PartialEq
        + serde::Serialize
        + std::str::FromStr<Err = SupervisorError>,
{
    for value in all {
        let text = as_str(*value);
        assert_eq!(text.parse::<T>().unwrap(), *value);
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::Value::String(text.to_owned())
        );
    }
}

#[tokio::test]
async fn an_old_schema_upgrades_keeping_every_row_and_scheduling_nothing() {
    let pool = bare_memory_pool().await;
    Migrator::new(OldSchema)
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    let fixture_rows = [
        "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, omo_thread_id, status,
                done_signals, last_activity_at)
         VALUES('old-working', 'Working', 'lead', '/tmp', 'lead', 'pane-1', 'working', 'file a',
                '2026-10-20T00:00:00Z')",
        "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, status, done_signals,
                last_activity_at)
         VALUES('old-queued', 'Queued', 'lead', '/tmp', 'lead', 'queued', 'file b',
                '2026-10-20T00:00:00Z')",
        "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, status, done_signals,
                last_activity_at)
         VALUES('old-blocked', 'Blocked', 'lead', '/tmp', 'lead', 'blocked', 'file c',
                '2026-10-20T00:00:00Z')",
        "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, status, done_signals,
                last_activity_at)
         VALUES('old-closed', 'Closed', 'lead', '/tmp', 'lead', 'closed', 'file d',
                '2026-10-20T00:00:00Z')",
        "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, status, done_signals,
                last_activity_at)
         VALUES('old-failed', 'Failed', 'lead', '/tmp', 'lead', 'failed', 'file e',
                '2026-10-20T00:00:00Z')",
        "INSERT INTO lane_history(lane_key, at, from_status, to_status, reason)
         VALUES('old-working', '2026-10-20T00:00:00Z', 'queued', 'working', 'launched')",
        "INSERT INTO lane_goal(lane_key, mode, turn_limit, turns_used, revision, updated_at)
         VALUES('old-working', 'full', 20, 3, 2, '2026-10-20T00:00:00Z')",
        r#"INSERT INTO lane_goal_requirements(lane_key, req_id, text, verifier, status,
                evidence_json, updated_at)
         VALUES('old-working', 'r1', 'tests pass', 'cargo test', 'met', '["sig:0"]',
                '2026-10-20T00:00:00Z')"#,
        "INSERT INTO lane_goal_revisions(lane_key, change_json, instruction_quote, state,
                created_at)
         VALUES('old-working', '{}', 'use DONE', 'accepted', '2026-10-20T00:00:00Z')",
        "INSERT INTO lane_turns(lane_key, seq, origin, text_excerpt, at)
         VALUES('old-working', 1, 'submitted_request', 'do it', '2026-10-20T00:00:00Z')",
        "INSERT INTO lane_goal(lane_key, mode, updated_at)
         VALUES('old-failed', 'supervised', '2026-10-20T00:00:00Z')",
        "INSERT INTO lane_goal_requirements(lane_key, req_id, text, verifier, status,
                evidence_json, updated_at)
         VALUES('old-failed', 'r1', 'ship it', 'git log', 'pending', '[]',
                '2026-10-20T00:00:00Z')",
    ];
    for sql in fixture_rows {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    type LaneRow = (String, String, Option<String>, String);
    let lanes_sql = "SELECT key, status, omo_thread_id, updated_at FROM lanes ORDER BY key";
    let lanes_before: Vec<LaneRow> = sqlx::query_as(lanes_sql).fetch_all(&pool).await.unwrap();
    let contract_before = contract_of(&pool, "old-working").await.unwrap();
    assert!(contract_before.is_some());
    let before = row_counts(&pool).await;
    for table in SUPERVISOR_TABLES {
        assert!(
            !before.contains_key(*table),
            "{table} predates its migration"
        );
    }

    crate::storage::migrate(&pool).await.unwrap();

    let after = row_counts(&pool).await;
    for (table, rows) in &before {
        assert_eq!(after.get(table), Some(rows), "{table} lost or gained rows");
    }
    for table in SUPERVISOR_TABLES {
        assert_eq!(after.get(*table), Some(&0), "{table} must start empty");
    }
    let applied = count(
        &pool,
        &format!(
            "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = {SUPERVISOR_SCHEMA_VERSION} \
             AND success = 1"
        ),
    )
    .await;
    assert_eq!(applied, 1);
    let lanes_after: Vec<LaneRow> = sqlx::query_as(lanes_sql).fetch_all(&pool).await.unwrap();
    assert_eq!(lanes_after, lanes_before, "legacy lanes stay historical");
    assert_eq!(
        contract_of(&pool, "old-working").await.unwrap(),
        contract_before
    );
    assert_eq!(ready_intents(&pool).await, 0);
    assert!(ready_dispatches(&pool, 100).await.unwrap().is_empty());
    let due = Utc::now() + chrono::Duration::days(1);
    assert!(pending_wakes(&pool, due, 100).await.unwrap().is_empty());

    crate::storage::migrate(&pool).await.unwrap();
    assert_eq!(
        row_counts(&pool).await,
        after,
        "re-running migrations is a no-op"
    );
}

#[tokio::test]
async fn a_replayed_admission_key_returns_the_same_receipt() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let first = admit_request(pool, &request(&fixture, "msg-1", BODY))
        .await
        .unwrap();
    assert!(!first.duplicate);
    let replay = admit_request(pool, &request(&fixture, "msg-1", BODY))
        .await
        .unwrap();
    assert!(replay.duplicate);
    assert_eq!(replay.receipt, first.receipt);
    assert_eq!(admission_rows(pool).await, [1; 7]);
    assert_eq!(ready_intents(pool).await, 1);

    let receipt = &first.receipt;
    let goal = goal_of(pool, &receipt.goal_id).await.unwrap().unwrap();
    assert_eq!(goal.run_state, RunState::Ready);
    assert_eq!(goal.revision, 1);
    assert_eq!(goal.lane_key, receipt.lane_key);
    assert_eq!(goal.supervisor_id, fixture.identity.supervisor_id);
    assert_eq!(goal.project_id, fixture.project.project_id);
    let contract = contract_of(pool, &receipt.lane_key).await.unwrap().unwrap();
    assert_eq!(contract.mode, GoalMode::Full);
    assert_eq!(contract.turn_limit, DEFAULT_TURN_LIMIT);
    assert_eq!(contract.requirements.len(), 1);
    let binding = execution_binding_of(pool, &receipt.goal_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (binding.kind, binding.host_id.as_str(), binding.generation),
        (ExecutionKind::Pty, HOST, 1)
    );
    assert_eq!(binding.provider_session_id, None);
    let (status, pane): (String, Option<String>) =
        sqlx::query_as("SELECT status, omo_thread_id FROM lanes WHERE key = ?")
            .bind(&receipt.lane_key)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(status, "queued", "the lane is projected, not driven");
    assert_eq!(pane, None, "the typed binding never reuses omo_thread_id");
    let ready = ready_dispatches(pool, 10).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].request_id, receipt.dispatch_request_id);
    assert_eq!(
        (
            ready[0].goal_revision,
            ready[0].ordinal,
            ready[0].generation
        ),
        (1, 1, 1)
    );
    assert_eq!(ready[0].instruction, BODY);

    let mut other_adapter = request(&fixture, "msg-1", BODY);
    other_adapter.source_adapter = ChannelProvider::Desktop;
    let distinct = admit_request(pool, &other_adapter).await.unwrap();
    assert!(!distinct.duplicate, "the key is adapter plus message id");
    assert_ne!(distinct.receipt.goal_id, receipt.goal_id);

    assert_eq!(
        ensure_identity(pool, OWNER).await.unwrap(),
        fixture.identity
    );
    assert_eq!(fixture.identity.memory_name, MEMORY_NAME);
}

#[tokio::test]
async fn a_changed_body_under_the_same_key_conflicts_without_new_work() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let first = admit(&fixture, "msg-1").await;
    let error = admit_request(
        pool,
        &request(&fixture, "msg-1", "Use DONE instead of READY."),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), "REQUEST_CONFLICT", "{error}");
    assert!(matches!(
        &error,
        SupervisorError::RequestBodyChanged { goal_id, revision: 1, .. } if *goal_id == first.goal_id
    ));
    assert_eq!(admission_rows(pool).await, [1; 7]);
    assert_eq!(ready_intents(pool).await, 1);
}

#[tokio::test]
async fn a_stale_revision_is_a_revision_conflict_and_changes_nothing() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    let goal_id = admitted.goal_id.as_str();
    let revised = revise_goal(pool, goal_id, 1).await.unwrap();
    assert_eq!(revised.revision, 2);
    assert_eq!(
        ready_intents(pool).await,
        0,
        "the revision-1 intent is superseded"
    );

    let stale = [
        transition_goal(pool, goal_id, 1, RunState::Ready, RunState::Paused, None)
            .await
            .map(|_| ()),
        revise_goal(pool, goal_id, 1).await.map(|_| ()),
        enqueue_dispatch(pool, goal_id, 1, "Use DONE")
            .await
            .map(|_| ()),
        claim_dispatch(pool, &admitted.dispatch_request_id, 1, 1)
            .await
            .map(|_| ()),
    ];
    for outcome in stale {
        let error = outcome.unwrap_err();
        assert_eq!(error.code(), "REVISION_CONFLICT", "{error}");
        assert!(matches!(
            error,
            SupervisorError::RevisionConflict {
                expected: 1,
                actual: 2,
                ..
            }
        ));
    }
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    assert_eq!((goal.run_state, goal.revision), (RunState::Ready, 2));
    assert_eq!(ready_intents(pool).await, 0);

    let old = claim_dispatch(pool, &admitted.dispatch_request_id, 2, 1)
        .await
        .unwrap_err();
    assert_eq!(old.code(), "NOT_CLAIMABLE", "{old}");
    let corrected = enqueue_dispatch(pool, goal_id, 2, "Use DONE instead of READY.")
        .await
        .unwrap();
    assert_eq!((corrected.goal_revision, corrected.ordinal), (2, 1));
    assert!(matches!(
        enqueue_dispatch(pool, goal_id, 2, "again").await,
        Err(SupervisorError::DispatchPending { .. })
    ));
    let claimed = claim_dispatch(pool, &corrected.request_id, 2, 1)
        .await
        .unwrap();
    assert_eq!(claimed.state, DispatchState::Claimed);
    assert_eq!(
        goal_of(pool, goal_id).await.unwrap().unwrap().run_state,
        RunState::Dispatching
    );
    let second = claim_dispatch(pool, &corrected.request_id, 2, 1)
        .await
        .unwrap_err();
    assert_eq!(second.code(), "NOT_CLAIMABLE", "a second claimer loses");
}

#[tokio::test]
async fn a_failed_admission_rolls_back_every_row_and_leaves_no_ready_intent() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    // Interrupts admission at its last write, after the lane, contract, goal, binding and
    // request rows were written inside the transaction.
    sqlx::query(
        "CREATE TRIGGER interrupt_dispatch BEFORE INSERT ON supervisor_dispatches
         BEGIN SELECT RAISE(ABORT, 'injected interruption'); END",
    )
    .execute(pool)
    .await
    .unwrap();
    for _ in 0..2 {
        let error = admit_request(pool, &request(&fixture, "msg-1", BODY))
            .await
            .unwrap_err();
        assert!(matches!(error, SupervisorError::Sql(_)), "{error}");
        assert!(
            error.to_string().contains("injected interruption"),
            "{error}"
        );
        assert_eq!(admission_rows(pool).await, [0; 7], "no half contract");
        assert_eq!(ready_intents(pool).await, 0);
    }
    sqlx::query("DROP TRIGGER interrupt_dispatch")
        .execute(pool)
        .await
        .unwrap();
    let admitted = admit_request(pool, &request(&fixture, "msg-1", BODY))
        .await
        .unwrap();
    assert!(
        !admitted.duplicate,
        "the rolled-back attempts left no dedupe row"
    );
    assert_eq!(admission_rows(pool).await, [1; 7]);
    assert_eq!(ready_intents(pool).await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_admissions_of_one_request_write_exactly_one_row() {
    let (pool, dir) = file_pool().await;
    let fixture = fixture_on(pool).await;
    let race = request(&fixture, "msg-race", BODY);
    let admitters: Vec<_> = (0..8)
        .map(|_| {
            let pool = fixture.pool.clone();
            let race = race.clone();
            tokio::spawn(async move { admit_request(&pool, &race).await })
        })
        .collect();
    let mut admissions = Vec::new();
    for admitter in admitters {
        admissions.push(admitter.await.unwrap().unwrap());
    }
    assert_eq!(
        admissions
            .iter()
            .filter(|admission| !admission.duplicate)
            .count(),
        1
    );
    assert!(admissions
        .iter()
        .all(|admission| admission.receipt == admissions[0].receipt));
    assert_eq!(admission_rows(&fixture.pool).await, [1; 7]);
    assert_eq!(ready_intents(&fixture.pool).await, 1);

    // Two writers decided against the same revision: exactly one wins.
    let goal_id = admissions[0].receipt.goal_id.clone();
    let writers: Vec<_> = (0..2)
        .map(|_| {
            let pool = fixture.pool.clone();
            let goal_id = goal_id.clone();
            tokio::spawn(async move { revise_goal(&pool, &goal_id, 1).await })
        })
        .collect();
    let mut outcomes = Vec::new();
    for writer in writers {
        outcomes.push(writer.await.unwrap());
    }
    let wins: Vec<i64> = outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().ok().map(|goal| goal.revision))
        .collect();
    assert_eq!(wins, [2]);
    let conflicts: Vec<&str> = outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().err().map(SupervisorError::code))
        .collect();
    assert_eq!(conflicts, ["REVISION_CONFLICT"]);

    fixture.pool.close().await;
    tokio::fs::remove_dir_all(&dir).await.unwrap();
}

#[tokio::test]
async fn unknown_vocabulary_and_malformed_input_are_refused() {
    assert_vocabulary(RunState::ALL, RunState::as_str);
    assert_vocabulary(ChannelProvider::ALL, ChannelProvider::as_str);
    assert_vocabulary(ExecutionKind::ALL, ExecutionKind::as_str);
    assert_vocabulary(DispatchState::ALL, DispatchState::as_str);
    assert_vocabulary(WaitKind::ALL, WaitKind::as_str);
    assert_vocabulary(WaitState::ALL, WaitState::as_str);
    assert_vocabulary(WaitOutcome::ALL, WaitOutcome::as_str);
    assert_vocabulary(WakeCause::ALL, WakeCause::as_str);
    let states: Vec<&str> = RunState::ALL.iter().map(|state| state.as_str()).collect();
    assert_eq!(
        states,
        [
            "ready",
            "dispatching",
            "running",
            "waiting_children",
            "waiting_event",
            "awaiting_owner",
            "verifying",
            "completed",
            "paused",
            "degraded",
            "delivery_uncertain",
            "cancelled",
        ]
    );
    let unknown = "done".parse::<RunState>().unwrap_err();
    assert_eq!(unknown.code(), "INVALID_REQUEST");
    assert!(matches!(
        unknown,
        SupervisorError::Unknown { kind: "run state", ref value } if value == "done"
    ));
    assert!("email".parse::<ChannelProvider>().is_err());
    assert!("ssh".parse::<ExecutionKind>().is_err());
    assert!(serde_json::from_str::<RunState>("\"done\"").is_err());

    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    for sql in [
        "UPDATE supervisor_goals SET run_state = 'done'",
        "UPDATE supervisor_execution_bindings SET kind = 'ssh'",
        "UPDATE supervisor_dispatches SET state = 'sent'",
        "UPDATE supervisor_requests SET source_adapter = 'email'",
    ] {
        assert!(sqlx::query(sql).execute(pool).await.is_err(), "{sql}");
    }

    let padded = request(&fixture, " msg-2", BODY);
    let mut not_uuid = request(&fixture, "msg-3", BODY);
    not_uuid.project_id = "not-a-uuid".into();
    let mut empty = request(&fixture, "msg-4", BODY);
    empty.requirements.clear();
    let mut blank = request(&fixture, "msg-5", "   ");
    blank.instruction = BODY.into();
    let mut unmapped = request(&fixture, "msg-6", BODY);
    unmapped.target.host_id = "host-without-checkout".into();
    let mut intruder = request(&fixture, "msg-7", BODY);
    intruder.owner_principal = STRANGER.into();
    for (bad, code) in [
        (padded, "INVALID_REQUEST"),
        (not_uuid, "INVALID_REQUEST"),
        (empty, "INVALID_REQUEST"),
        (blank, "INVALID_REQUEST"),
        (unmapped, "NOT_FOUND"),
        (intruder, "CROSS_OWNER"),
    ] {
        let error = admit_request(pool, &bad).await.unwrap_err();
        assert_eq!(error.code(), code, "{error}");
    }
    assert_eq!(admission_rows(pool).await, [1; 7]);

    let goal_id = admitted.goal_id.as_str();
    assert!(matches!(
        transition_goal(pool, goal_id, 1, RunState::Ready, RunState::Completed, None).await,
        Err(SupervisorError::IllegalTransition {
            from: RunState::Ready,
            to: RunState::Completed,
            ..
        })
    ));
    assert!(matches!(
        transition_goal(pool, goal_id, 1, RunState::Ready, RunState::Dispatching, None).await,
        Err(SupervisorError::Invalid(_))
    ));
    let cancelled = transition_goal(
        pool,
        goal_id,
        1,
        RunState::Ready,
        RunState::Cancelled,
        Some("owner cancelled"),
    )
    .await
    .unwrap();
    assert_eq!(cancelled.revision, 2, "an owner hold bumps the revision");
    assert_eq!(
        ready_intents(pool).await,
        0,
        "cancelling supersedes the unsent intent"
    );
    assert!(matches!(
        transition_goal(pool, goal_id, 2, RunState::Cancelled, RunState::Ready, None).await,
        Err(SupervisorError::IllegalTransition { .. })
    ));
    assert_eq!(
        revise_goal(pool, goal_id, 2).await.unwrap_err().code(),
        "GOAL_CLOSED"
    );
}

#[tokio::test]
async fn waits_refuse_self_cycles_and_cross_owner_links_and_wake_once() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let a = admit(&fixture, "msg-a").await.goal_id;
    let b = admit(&fixture, "msg-b").await.goal_id;
    let c = admit(&fixture, "msg-c").await.goal_id;

    let self_wait = add_wait(pool, &a, &WaitTarget::ChildGoal(a.clone()))
        .await
        .unwrap_err();
    assert!(matches!(self_wait, SupervisorError::SelfWait(_)));
    let a_on_b = add_wait(pool, &a, &WaitTarget::ChildGoal(b.clone()))
        .await
        .unwrap();
    let a_on_c = add_wait(pool, &a, &WaitTarget::ChildGoal(c.clone()))
        .await
        .unwrap();
    add_wait(pool, &b, &WaitTarget::ChildGoal(c.clone()))
        .await
        .unwrap();
    let cycle = add_wait(pool, &c, &WaitTarget::ChildGoal(a.clone()))
        .await
        .unwrap_err();
    assert!(
        matches!(cycle, SupervisorError::WaitCycle { .. }),
        "{cycle}"
    );
    assert_eq!(cycle.code(), "WAIT_CYCLE");
    let event_wait = add_wait(pool, &c, &WaitTarget::ExternalEvent("ci:main".into()))
        .await
        .unwrap();
    assert_eq!(event_wait.kind, WaitKind::ExternalEvent);

    let stranger = ensure_identity(pool, STRANGER).await.unwrap();
    assert_ne!(stranger.supervisor_id, fixture.identity.supervisor_id);
    let theirs_project = create_project(
        pool,
        &stranger.supervisor_id,
        "theirs",
        "github.com/example/theirs",
    )
    .await
    .unwrap();
    bind_project_host(
        pool,
        &theirs_project.project_id,
        HOST,
        "/home/stranger/theirs/.git",
    )
    .await
    .unwrap();
    let mut theirs = request(&fixture, "msg-d", BODY);
    theirs.supervisor_id = stranger.supervisor_id.clone();
    theirs.owner_principal = STRANGER.into();
    theirs.project_id = theirs_project.project_id.clone();
    let d = admit_request(pool, &theirs).await.unwrap().receipt.goal_id;
    let cross = add_wait(pool, &a, &WaitTarget::ChildGoal(d.clone()))
        .await
        .unwrap_err();
    assert_eq!(cross.code(), "CROSS_OWNER", "{cross}");
    let mut borrowed_project = theirs.clone();
    borrowed_project.source_message_id = "msg-e".into();
    borrowed_project.project_id = fixture.project.project_id.clone();
    assert_eq!(
        admit_request(pool, &borrowed_project)
            .await
            .unwrap_err()
            .code(),
        "CROSS_OWNER"
    );
    assert_eq!(waits_of(pool, &a).await.unwrap().len(), 2);

    let satisfied = record_wait_event(
        pool,
        &a_on_b.wait_id,
        "evt-1",
        WaitOutcome::Satisfied,
        &["sig:0".to_owned()],
    )
    .await
    .unwrap();
    assert!(satisfied.inserted);
    assert_eq!(satisfied.wait.state, WaitState::Satisfied);
    assert!(satisfied.wake.is_none(), "a's other wait is still pending");
    let replay = record_wait_event(pool, &a_on_b.wait_id, "evt-1", WaitOutcome::Satisfied, &[])
        .await
        .unwrap();
    assert!(!replay.inserted);
    assert!(replay.wake.is_none());
    let progress = record_wait_event(pool, &a_on_c.wait_id, "evt-2", WaitOutcome::Progress, &[])
        .await
        .unwrap();
    assert!(progress.inserted);
    assert_eq!(progress.wait.state, WaitState::Pending);
    assert!(progress.wake.is_none());
    let failed = record_wait_event(pool, &a_on_c.wait_id, "evt-3", WaitOutcome::Failed, &[])
        .await
        .unwrap();
    assert_eq!(failed.wait.state, WaitState::Failed);
    let wake = failed.wake.expect("a failed dependency wakes its parent");
    assert_eq!(wake.cause, WakeCause::DependencyFailed);
    assert_eq!(
        (wake.goal_id.as_str(), wake.goal_revision, wake.generation),
        (a.as_str(), 1, 1)
    );
    assert_eq!(
        count(pool, "SELECT COUNT(*) FROM supervisor_wait_events").await,
        3
    );

    let due = Utc::now() + chrono::Duration::seconds(1);
    let pending = pending_wakes(pool, due, 10).await.unwrap();
    assert_eq!(pending, [wake.clone()]);
    assert!(consume_wake(pool, wake.id).await.unwrap());
    assert!(
        !consume_wake(pool, wake.id).await.unwrap(),
        "a wake is consumed once"
    );
    assert!(pending_wakes(pool, due, 10).await.unwrap().is_empty());

    let report = enqueue_wake(pool, &b, WakeCause::Report, "report-1", Utc::now())
        .await
        .unwrap();
    let again = enqueue_wake(pool, &b, WakeCause::Report, "report-1", Utc::now())
        .await
        .unwrap();
    assert_eq!(report, again, "a replayed wake collapses onto its row");
}

#[tokio::test]
async fn dispatch_receipts_keep_accepted_started_settled_and_uncertain_apart() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    let goal_id = admitted.goal_id.as_str();
    let request_id = admitted.dispatch_request_id.as_str();

    assert!(matches!(
        record_receipt(pool, request_id, 1, &Receipt::Accepted).await,
        Err(SupervisorError::ReceiptOutOfOrder {
            from: DispatchState::Ready,
            ..
        })
    ));
    claim_dispatch(pool, request_id, 1, 1).await.unwrap();

    attach_session(pool, goal_id, 1, "pane-7").await.unwrap();
    attach_session(pool, goal_id, 1, "pane-7").await.unwrap();
    assert!(matches!(
        attach_session(pool, goal_id, 1, "pane-8").await,
        Err(SupervisorError::Invalid(_))
    ));

    let accepted = record_receipt(pool, request_id, 1, &Receipt::Accepted)
        .await
        .unwrap();
    assert_eq!(accepted.state, DispatchState::Accepted);
    assert!(accepted.accepted_at.is_some());
    assert_eq!(accepted.started_at, None, "acceptance is not a start");
    let repeated = record_receipt(pool, request_id, 1, &Receipt::Accepted)
        .await
        .unwrap();
    assert_eq!(repeated, accepted);
    let early_settle = settled("run-1", "pane-7");
    assert!(matches!(
        record_receipt(pool, request_id, 1, &early_settle).await,
        Err(SupervisorError::ReceiptOutOfOrder { .. })
    ));
    let other_session = record_receipt(pool, request_id, 1, &started("run-1", "pane-8"))
        .await
        .unwrap_err();
    assert_eq!(other_session.code(), "SESSION_MISMATCH", "{other_session}");
    assert_eq!(
        transition_goal(pool, goal_id, 1, RunState::Dispatching, RunState::Running, None)
            .await
            .unwrap_err()
            .code(),
        "NO_LIVE_RUN",
        "acceptance alone is not a running worker"
    );

    let start = record_receipt(pool, request_id, 1, &started("run-1", "pane-7"))
        .await
        .unwrap();
    assert_eq!(start.state, DispatchState::Started);
    assert_eq!(start.run_id.as_deref(), Some("run-1"));
    assert_eq!(start.provider_session_id.as_deref(), Some("pane-7"));
    assert_eq!(start.accepted_at, accepted.accepted_at);
    let repeated_start = record_receipt(pool, request_id, 1, &started("run-1", "pane-7"))
        .await
        .unwrap();
    assert_eq!(repeated_start, start);
    let binding = execution_binding_of(pool, goal_id).await.unwrap().unwrap();
    assert_eq!(binding.run_id.as_deref(), Some("run-1"));
    assert_eq!(binding.provider_session_id.as_deref(), Some("pane-7"));
    let running = transition_goal(pool, goal_id, 1, RunState::Dispatching, RunState::Running, None)
        .await
        .unwrap();
    assert_eq!((running.run_state, running.revision), (RunState::Running, 1));
    let wrong_run = record_receipt(pool, request_id, 1, &settled("run-2", "pane-7"))
        .await
        .unwrap_err();
    assert_eq!(wrong_run.code(), "RUN_MISMATCH", "{wrong_run}");

    let advanced = advance_generation(pool, goal_id, 1, Some("pane-9"))
        .await
        .unwrap();
    assert_eq!((advanced.generation, advanced.run_id), (2, None));
    for generation in [1, 2] {
        let stale = record_receipt(pool, request_id, generation, &early_settle)
            .await
            .unwrap_err();
        assert_eq!(stale.code(), "STALE_GENERATION", "{stale}");
    }
    assert_eq!(
        advance_generation(pool, goal_id, 1, None)
            .await
            .unwrap_err()
            .code(),
        "STALE_GENERATION"
    );
    let states: Vec<String> =
        sqlx::query_scalar("SELECT state FROM supervisor_dispatches ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(states, ["started"], "stale receipts changed nothing");
}

#[tokio::test]
async fn an_owner_pause_fences_stale_run_state_writes_and_blocks_dispatch() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    let goal_id = admitted.goal_id.as_str();
    let paused = transition_goal(
        pool,
        goal_id,
        1,
        RunState::Ready,
        RunState::Paused,
        Some("owner paused"),
    )
    .await
    .unwrap();
    assert_eq!((paused.run_state, paused.revision), (RunState::Paused, 2));
    assert_eq!(
        ready_intents(pool).await,
        0,
        "the hold supersedes the intent decided before it"
    );

    // Decisions made before the pause, against revision 1.
    for (from, to) in [
        (RunState::Ready, RunState::AwaitingOwner),
        (RunState::Paused, RunState::Ready),
        (RunState::Dispatching, RunState::Running),
    ] {
        let error = transition_goal(pool, goal_id, 1, from, to, None)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "REVISION_CONFLICT", "{from} -> {to}: {error}");
    }
    // The current revision but a stale state.
    let stale_state = transition_goal(
        pool,
        goal_id,
        2,
        RunState::Ready,
        RunState::AwaitingOwner,
        None,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            stale_state,
            SupervisorError::StateConflict {
                expected: RunState::Ready,
                actual: RunState::Paused,
                ..
            }
        ),
        "{stale_state}"
    );
    assert_eq!(stale_state.code(), "REVISION_CONFLICT");
    let stale_claim = claim_dispatch(pool, &admitted.dispatch_request_id, 1, 1)
        .await
        .unwrap_err();
    assert_eq!(stale_claim.code(), "REVISION_CONFLICT", "{stale_claim}");
    let superseded = claim_dispatch(pool, &admitted.dispatch_request_id, 2, 1)
        .await
        .unwrap_err();
    assert_eq!(superseded.code(), "NOT_CLAIMABLE", "{superseded}");
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    assert_eq!((goal.run_state, goal.revision), (RunState::Paused, 2));

    // No worker was ever started, so resuming cannot claim a running one.
    let no_run = transition_goal(pool, goal_id, 2, RunState::Paused, RunState::Running, None)
        .await
        .unwrap_err();
    assert_eq!(no_run.code(), "NO_LIVE_RUN", "{no_run}");

    let resumed = transition_goal(pool, goal_id, 2, RunState::Paused, RunState::Ready, None)
        .await
        .unwrap();
    assert_eq!((resumed.run_state, resumed.revision), (RunState::Ready, 2));
    assert!(
        ready_dispatches(pool, 10).await.unwrap().is_empty(),
        "the pre-pause intent stays superseded after resuming"
    );
    let fresh = enqueue_dispatch(pool, goal_id, 2, "Continue after the pause.")
        .await
        .unwrap();
    assert_eq!(fresh.goal_revision, 2);
    let claimed = claim_dispatch(pool, &fresh.request_id, 2, 1)
        .await
        .unwrap();
    assert_eq!(claimed.state, DispatchState::Claimed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_run_state_writes_at_one_revision_have_one_winner() {
    let (pool, dir) = file_pool().await;
    let fixture = fixture_on(pool).await;
    let goal_id = admit(&fixture, "msg-1").await.goal_id;

    // Two owner holds decided against revision 1 in state ready.
    let holds: Vec<_> = [RunState::Paused, RunState::AwaitingOwner]
        .into_iter()
        .map(|to| {
            let pool = fixture.pool.clone();
            let goal_id = goal_id.clone();
            tokio::spawn(
                async move { transition_goal(&pool, &goal_id, 1, RunState::Ready, to, None).await },
            )
        })
        .collect();
    let mut outcomes = Vec::new();
    for hold in holds {
        outcomes.push(hold.await.unwrap());
    }
    let winners: Vec<RunState> = outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().ok().map(|goal| goal.run_state))
        .collect();
    assert_eq!(winners.len(), 1);
    let conflicts: Vec<&str> = outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().err().map(SupervisorError::code))
        .collect();
    assert_eq!(conflicts, ["REVISION_CONFLICT"]);
    let held = winners[0];
    let goal = goal_of(&fixture.pool, &goal_id).await.unwrap().unwrap();
    assert_eq!((goal.run_state, goal.revision), (held, 2));
    assert_eq!(ready_intents(&fixture.pool).await, 0);

    // Two moves that keep the revision, decided against the same state: the state is the fence.
    let moves: Vec<_> = [RunState::Ready, RunState::Degraded]
        .into_iter()
        .map(|to| {
            let pool = fixture.pool.clone();
            let goal_id = goal_id.clone();
            tokio::spawn(async move { transition_goal(&pool, &goal_id, 2, held, to, None).await })
        })
        .collect();
    let mut outcomes = Vec::new();
    for other in moves {
        outcomes.push(other.await.unwrap());
    }
    let winners: Vec<RunState> = outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().ok().map(|goal| goal.run_state))
        .collect();
    assert_eq!(winners.len(), 1);
    let conflicts: Vec<&str> = outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().err().map(SupervisorError::code))
        .collect();
    assert_eq!(conflicts, ["REVISION_CONFLICT"]);
    let goal = goal_of(&fixture.pool, &goal_id).await.unwrap().unwrap();
    assert_eq!((goal.run_state, goal.revision), (winners[0], 2));

    fixture.pool.close().await;
    tokio::fs::remove_dir_all(&dir).await.unwrap();
}

#[tokio::test]
async fn an_uncertain_dispatch_reconciles_to_the_run_that_happened() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    let goal_id = admitted.goal_id.as_str();
    let request_id = admitted.dispatch_request_id.as_str();
    claim_dispatch(pool, request_id, 1, 1).await.unwrap();
    attach_session(pool, goal_id, 1, "pane-7").await.unwrap();
    let uncertain = record_receipt(
        pool,
        request_id,
        1,
        &Receipt::Uncertain {
            code: "SEND_TIMEOUT".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(uncertain.state, DispatchState::Uncertain);
    assert_eq!(uncertain.started_at, None);
    transition_goal(
        pool,
        goal_id,
        1,
        RunState::Dispatching,
        RunState::DeliveryUncertain,
        Some("SEND_TIMEOUT"),
    )
    .await
    .unwrap();

    // A late receipt must name this dispatch's generation and the bound session.
    let wrong_session = record_receipt(pool, request_id, 1, &started("run-1", "pane-8"))
        .await
        .unwrap_err();
    assert_eq!(wrong_session.code(), "SESSION_MISMATCH", "{wrong_session}");
    let wrong_generation = record_receipt(pool, request_id, 2, &started("run-1", "pane-7"))
        .await
        .unwrap_err();
    assert_eq!(wrong_generation.code(), "STALE_GENERATION", "{wrong_generation}");
    let not_yet = transition_goal(
        pool,
        goal_id,
        1,
        RunState::DeliveryUncertain,
        RunState::Running,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(not_yet.code(), "NO_LIVE_RUN", "{not_yet}");

    let reconciled = record_receipt(pool, request_id, 1, &started("run-1", "pane-7"))
        .await
        .unwrap();
    assert_eq!(reconciled.state, DispatchState::Started);
    assert_eq!(reconciled.run_id.as_deref(), Some("run-1"));
    assert_eq!(reconciled.provider_session_id.as_deref(), Some("pane-7"));
    assert!(reconciled.started_at.is_some());
    assert_eq!(reconciled.accepted_at, None, "no acceptance was ever seen");
    assert_eq!(reconciled.uncertain_code.as_deref(), Some("SEND_TIMEOUT"));
    assert_eq!(reconciled.uncertain_at, uncertain.uncertain_at);
    transition_goal(
        pool,
        goal_id,
        1,
        RunState::DeliveryUncertain,
        RunState::Running,
        None,
    )
    .await
    .unwrap();
    let done = record_receipt(pool, request_id, 1, &settled("run-1", "pane-7"))
        .await
        .unwrap();
    assert_eq!(done.state, DispatchState::Settled);
    assert_eq!(done.started_at, reconciled.started_at);
    let facts: (Option<String>, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT accepted_at, started_at, settled_at, uncertain_at FROM supervisor_dispatches
         WHERE request_id = ?",
    )
    .bind(request_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(facts.0.is_none() && facts.1.is_some() && facts.2.is_some() && facts.3.is_some());

    // An uncertain dispatch whose start was never seen can still settle on its run.
    let other = admit(&fixture, "msg-2").await;
    claim_dispatch(pool, &other.dispatch_request_id, 1, 1)
        .await
        .unwrap();
    attach_session(pool, &other.goal_id, 1, "pane-8")
        .await
        .unwrap();
    record_receipt(
        pool,
        &other.dispatch_request_id,
        1,
        &Receipt::Uncertain {
            code: "TURN_NOT_OBSERVED".into(),
        },
    )
    .await
    .unwrap();
    let direct = record_receipt(pool, &other.dispatch_request_id, 1, &settled("run-9", "pane-8"))
        .await
        .unwrap();
    assert_eq!(direct.state, DispatchState::Settled);
    assert_eq!(direct.run_id.as_deref(), Some("run-9"));
    assert_eq!(direct.started_at, None, "a start that was never observed is not invented");
    assert_eq!(direct.uncertain_code.as_deref(), Some("TURN_NOT_OBSERVED"));
    assert!(matches!(
        record_receipt(pool, &other.dispatch_request_id, 1, &started("run-9", "pane-8")).await,
        Err(SupervisorError::ReceiptOutOfOrder { .. })
    ));
    assert_eq!(ready_intents(pool).await, 0, "reconciliation never resends");
}

#[tokio::test]
async fn a_goal_owner_is_pinned_to_its_supervisor_identity() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    ensure_identity(pool, STRANGER).await.unwrap();
    for sql in [
        "UPDATE supervisor_goals SET owner_principal = ?",
        "UPDATE supervisor_requests SET owner_principal = ?",
    ] {
        let refused = sqlx::query(sql).bind(STRANGER).execute(pool).await;
        assert!(refused.is_err(), "{sql}");
    }
    let foreign_binding = sqlx::query(
        "INSERT INTO supervisor_channel_bindings(binding_id, supervisor_id, provider, channel_id,
                owner_principal, created_at, updated_at)
         VALUES(?, ?, 'slack', 'C1', ?, 't', 't')",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&fixture.identity.supervisor_id)
    .bind(STRANGER)
    .execute(pool)
    .await;
    assert!(foreign_binding.is_err());

    let address = ChannelAddress {
        provider: ChannelProvider::Slack,
        workspace_id: "T1".into(),
        account_id: "B1".into(),
        channel_id: "D1".into(),
        thread_id: String::new(),
        owner_principal: OWNER.into(),
    };
    let binding = bind_channel(
        pool,
        &fixture.identity.supervisor_id,
        &address,
        Some(&fixture.project.project_id),
        Some(&admitted.goal_id),
    )
    .await
    .unwrap();
    assert_eq!(binding.address.owner_principal, OWNER);
    let owner: String =
        sqlx::query_scalar("SELECT owner_principal FROM supervisor_goals WHERE goal_id = ?")
            .bind(&admitted.goal_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(owner, OWNER);
}
