use std::collections::{BTreeMap, BTreeSet, HashSet};
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
        operation_grant: Vec::new(),
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

async fn dispatch_state(pool: &SqlitePool, request_id: &str) -> String {
    sqlx::query_scalar("SELECT state FROM supervisor_dispatches WHERE request_id = ?")
        .bind(request_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn live(pool: &SqlitePool, goal_id: &str) -> bool {
    let mut conn = pool.acquire().await.unwrap();
    crate::supervisor::has_live_run(&mut conn, goal_id)
        .await
        .unwrap()
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
    assert_eq!(pending, std::slice::from_ref(&wake));
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
    let early_settle = settled("run-1", "pane-7");
    assert!(
        matches!(
            record_receipt(pool, request_id, 1, &early_settle).await,
            Err(SupervisorError::ReceiptOutOfOrder {
                from: DispatchState::Claimed,
                ..
            })
        ),
        "a claim that was never accepted or started cannot settle"
    );

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

#[tokio::test]
async fn a_late_reconcile_of_an_older_dispatch_leaves_the_newer_run_live() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    let goal_id = admitted.goal_id.as_str();
    let r1 = admitted.dispatch_request_id.as_str();
    claim_dispatch(pool, r1, 1, 1).await.unwrap();
    attach_session(pool, goal_id, 1, "S1").await.unwrap();
    record_receipt(
        pool,
        r1,
        1,
        &Receipt::Uncertain {
            code: "SEND_TIMEOUT".into(),
        },
    )
    .await
    .unwrap();
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
    let unresolved = transition_goal(
        pool,
        goal_id,
        1,
        RunState::DeliveryUncertain,
        RunState::Ready,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(unresolved.code(), "DISPATCH_IN_FLIGHT", "{unresolved}");
    resolve_uncertain_delivery(
        pool,
        goal_id,
        1,
        RunState::DeliveryUncertain,
        Some("owner chose to resend"),
    )
    .await
    .unwrap();
    let r2 = enqueue_dispatch(pool, goal_id, 1, "Resend the instruction.")
        .await
        .unwrap();
    assert_eq!((r2.goal_revision, r2.ordinal), (1, 2));
    claim_dispatch(pool, &r2.request_id, 1, 1).await.unwrap();
    confirm_claim_before_transmit(pool, &r2.request_id)
        .await
        .unwrap();
    record_receipt(pool, &r2.request_id, 1, &started("run-b", "S1"))
        .await
        .unwrap();
    transition_goal(
        pool,
        goal_id,
        1,
        RunState::Dispatching,
        RunState::Running,
        None,
    )
    .await
    .unwrap();
    assert!(live(pool, goal_id).await);

    // The first send did run after all, and its receipts arrive late. Its start comes through
    // the dispatcher's atomic start, which keeps the evidence and refuses the start itself.
    let error = record_start(pool, r1, 1, "run-a", "S1").await.unwrap_err();
    assert_eq!(error.code(), "RECEIPT_OUT_OF_ORDER", "{error}");
    let error = record_receipt(pool, r1, 1, &settled("run-a", "S1"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "RECEIPT_OUT_OF_ORDER", "{error}");
    let evidence: (String, Option<String>, Option<String>, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT state, run_id, provider_session_id, started_at, settled_at
             FROM supervisor_dispatches WHERE request_id = ?",
        )
        .bind(r1)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(evidence.0, "uncertain", "the older dispatch never becomes live");
    assert_eq!(evidence.1.as_deref(), Some("run-a"), "its run is kept as evidence");
    assert_eq!(evidence.2.as_deref(), Some("S1"));
    assert!(evidence.3.is_some() && evidence.4.is_some());
    let binding = execution_binding_of(pool, goal_id).await.unwrap().unwrap();
    assert_eq!(binding.run_id.as_deref(), Some("run-b"));
    assert!(live(pool, goal_id).await, "run-b keeps the live-run marker");

    // The newer run settles normally and ends the live run.
    record_receipt(pool, &r2.request_id, 1, &settled("run-b", "S1"))
        .await
        .unwrap();
    let binding = execution_binding_of(pool, goal_id).await.unwrap().unwrap();
    assert_eq!(binding.run_id, None);
    assert!(!live(pool, goal_id).await);
}

#[tokio::test]
async fn a_live_run_resumes_to_running_and_refuses_ready_and_new_claims() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    let goal_id = admitted.goal_id.as_str();
    let r1 = admitted.dispatch_request_id.as_str();
    attach_session(pool, goal_id, 1, "S1").await.unwrap();
    claim_dispatch(pool, r1, 1, 1).await.unwrap();
    record_receipt(pool, r1, 1, &started("run-a", "S1"))
        .await
        .unwrap();
    transition_goal(
        pool,
        goal_id,
        1,
        RunState::Dispatching,
        RunState::Running,
        None,
    )
    .await
    .unwrap();
    let paused = transition_goal(
        pool,
        goal_id,
        1,
        RunState::Running,
        RunState::Paused,
        Some("owner paused"),
    )
    .await
    .unwrap();
    assert_eq!(paused.revision, 2);
    let to_ready = transition_goal(pool, goal_id, 2, RunState::Paused, RunState::Ready, None)
        .await
        .unwrap_err();
    assert_eq!(to_ready.code(), "DISPATCH_IN_FLIGHT", "{to_ready}");
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    assert_eq!((goal.run_state, goal.revision), (RunState::Paused, 2));
    let resumed = transition_goal(pool, goal_id, 2, RunState::Paused, RunState::Running, None)
        .await
        .unwrap();
    assert_eq!((resumed.run_state, resumed.revision), (RunState::Running, 2));

    // A late start for an owner-resolved uncertain dispatch, with no newer dispatch sent,
    // makes it the live run even though the goal is ready: no new claim, resume to running.
    let other = admit(&fixture, "msg-2").await;
    let other_goal = other.goal_id.as_str();
    attach_session(pool, other_goal, 1, "S2").await.unwrap();
    claim_dispatch(pool, &other.dispatch_request_id, 1, 1)
        .await
        .unwrap();
    record_receipt(
        pool,
        &other.dispatch_request_id,
        1,
        &Receipt::Uncertain {
            code: "SEND_TIMEOUT".into(),
        },
    )
    .await
    .unwrap();
    transition_goal(
        pool,
        other_goal,
        1,
        RunState::Dispatching,
        RunState::DeliveryUncertain,
        Some("SEND_TIMEOUT"),
    )
    .await
    .unwrap();
    let ready = resolve_uncertain_delivery(
        pool,
        other_goal,
        1,
        RunState::DeliveryUncertain,
        None,
    )
    .await
    .unwrap();
    assert_eq!((ready.run_state, ready.revision), (RunState::Ready, 1));
    let next = enqueue_dispatch(pool, other_goal, 1, "Next step.")
        .await
        .unwrap();
    assert_eq!(ready_dispatches(pool, 10).await.unwrap().len(), 1);
    let late = record_receipt(pool, &other.dispatch_request_id, 1, &started("run-c", "S2"))
        .await
        .unwrap();
    assert_eq!(late.state, DispatchState::Started);
    assert!(late.owner_resolved_at.is_some());
    assert!(live(pool, other_goal).await);
    let busy = claim_dispatch(pool, &next.request_id, 1, 1)
        .await
        .unwrap_err();
    assert_eq!(busy.code(), "DISPATCH_IN_FLIGHT", "{busy}");
    assert_eq!(dispatch_state(pool, &next.request_id).await, "ready");
    assert!(ready_dispatches(pool, 10).await.unwrap().is_empty());
    let running = transition_goal(pool, other_goal, 1, RunState::Ready, RunState::Running, None)
        .await
        .unwrap();
    assert_eq!(running.run_state, RunState::Running);
}

#[tokio::test]
async fn a_dispatch_claimed_before_an_owner_hold_is_released_before_transmit() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    let goal_id = admitted.goal_id.as_str();
    let r1 = admitted.dispatch_request_id.as_str();
    // Claimed, and the owner's hold lands before the dispatcher's last check.
    claim_dispatch(pool, r1, 1, 1).await.unwrap();

    let paused = transition_goal(
        pool,
        goal_id,
        1,
        RunState::Dispatching,
        RunState::Paused,
        Some("owner paused"),
    )
    .await
    .unwrap();
    assert_eq!(paused.revision, 2);
    assert_eq!(
        dispatch_state(pool, r1).await,
        "claimed",
        "the hold leaves a claim alone: it may be in flight"
    );
    let released = confirm_claim_before_transmit(pool, r1).await.unwrap_err();
    assert_eq!(released.code(), "SUPERSEDED", "{released}");
    assert_eq!(dispatch_state(pool, r1).await, "released");
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    assert_eq!((goal.run_state, goal.revision), (RunState::Paused, 2));
    assert_eq!(
        confirm_claim_before_transmit(pool, r1)
            .await
            .unwrap_err()
            .code(),
        "NOT_CLAIMABLE"
    );
    assert!(matches!(
        record_receipt(pool, r1, 1, &Receipt::Accepted).await,
        Err(SupervisorError::ReceiptOutOfOrder {
            from: DispatchState::Released,
            ..
        })
    ));

    // A correction during the claim releases it and hands the goal back to ready.
    let corrected = admit(&fixture, "msg-2").await;
    let corrected_goal = corrected.goal_id.as_str();
    claim_dispatch(pool, &corrected.dispatch_request_id, 1, 1)
        .await
        .unwrap();
    revise_goal(pool, corrected_goal, 1).await.unwrap();
    let released = confirm_claim_before_transmit(pool, &corrected.dispatch_request_id)
        .await
        .unwrap_err();
    assert_eq!(released.code(), "SUPERSEDED", "{released}");
    let goal = goal_of(pool, corrected_goal).await.unwrap().unwrap();
    assert_eq!((goal.run_state, goal.revision), (RunState::Ready, 2));
    let fresh = enqueue_dispatch(pool, corrected_goal, 2, "Corrected instruction.")
        .await
        .unwrap();
    let claimed = claim_dispatch(pool, &fresh.request_id, 2, 1).await.unwrap();
    assert_eq!(
        confirm_claim_before_transmit(pool, &fresh.request_id)
            .await
            .unwrap(),
        claimed,
        "an unchanged claim is confirmed as is"
    );

    // A generation change during the claim releases it too.
    let rebound = admit(&fixture, "msg-3").await;
    let rebound_goal = rebound.goal_id.as_str();
    claim_dispatch(pool, &rebound.dispatch_request_id, 1, 1)
        .await
        .unwrap();
    advance_generation(pool, rebound_goal, 1, Some("S9"))
        .await
        .unwrap();
    let released = confirm_claim_before_transmit(pool, &rebound.dispatch_request_id)
        .await
        .unwrap_err();
    assert_eq!(released.code(), "SUPERSEDED", "{released}");
    let goal = goal_of(pool, rebound_goal).await.unwrap().unwrap();
    assert_eq!((goal.run_state, goal.revision), (RunState::Ready, 1));
}

/// Review scenario A: an accepted dispatch is in flight, so neither a resume into ready nor a
/// claim nor a raw write can put a second dispatch on the busy worker.
#[tokio::test]
async fn scenario_a_an_accepted_dispatch_blocks_every_path_to_a_second_send() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    let goal_id = admitted.goal_id.as_str();
    let r1 = admitted.dispatch_request_id.as_str();
    attach_session(pool, goal_id, 1, "S1").await.unwrap();
    claim_dispatch(pool, r1, 1, 1).await.unwrap();
    confirm_claim_before_transmit(pool, r1).await.unwrap();
    record_receipt(pool, r1, 1, &Receipt::Accepted)
        .await
        .unwrap();
    transition_goal(
        pool,
        goal_id,
        1,
        RunState::Dispatching,
        RunState::Degraded,
        Some("observer lost"),
    )
    .await
    .unwrap();
    let to_ready = transition_goal(pool, goal_id, 1, RunState::Degraded, RunState::Ready, None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            &to_ready,
            SupervisorError::DispatchInFlight { request_id, .. } if request_id == r1
        ),
        "{to_ready}"
    );
    let r2 = enqueue_dispatch(pool, goal_id, 1, "Second instruction.")
        .await
        .unwrap();
    assert!(claim_dispatch(pool, &r2.request_id, 1, 1).await.is_err());
    assert!(ready_dispatches(pool, 10).await.unwrap().is_empty());
    let forced = sqlx::query(
        "UPDATE supervisor_dispatches SET state = 'claimed', claimed_at = 't' WHERE request_id = ?",
    )
    .bind(&r2.request_id)
    .execute(pool)
    .await;
    assert!(
        forced.is_err(),
        "the schema refuses a second dispatch in flight"
    );
    assert_eq!(dispatch_state(pool, &r2.request_id).await, "ready");

    // The accepted dispatch really starts: it becomes the live run.
    record_receipt(pool, r1, 1, &started("run-c", "S1"))
        .await
        .unwrap();
    assert!(live(pool, goal_id).await);
    transition_goal(pool, goal_id, 1, RunState::Degraded, RunState::Running, None)
        .await
        .unwrap();
    record_receipt(pool, r1, 1, &settled("run-c", "S1"))
        .await
        .unwrap();
    transition_goal(pool, goal_id, 1, RunState::Running, RunState::Verifying, None)
        .await
        .unwrap();
    transition_goal(pool, goal_id, 1, RunState::Verifying, RunState::Ready, None)
        .await
        .unwrap();
    claim_dispatch(pool, &r2.request_id, 1, 1).await.unwrap();
}

/// Review scenario B: a claim left unconfirmed across a pause blocks the resume until the
/// dispatcher releases it, and a stale confirm never reopens a goal whose newer claim is in
/// flight.
#[tokio::test]
async fn scenario_b_a_stale_confirm_never_reopens_a_goal_with_a_newer_claim() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    let goal_id = admitted.goal_id.as_str();
    let r1 = admitted.dispatch_request_id.as_str();
    claim_dispatch(pool, r1, 1, 1).await.unwrap();
    transition_goal(pool, goal_id, 1, RunState::Dispatching, RunState::Paused, None)
        .await
        .unwrap();
    let blocked = transition_goal(pool, goal_id, 2, RunState::Paused, RunState::Ready, None)
        .await
        .unwrap_err();
    assert_eq!(blocked.code(), "DISPATCH_IN_FLIGHT", "{blocked}");
    let released = confirm_claim_before_transmit(pool, r1).await.unwrap_err();
    assert_eq!(released.code(), "SUPERSEDED");
    assert_eq!(dispatch_state(pool, r1).await, "released");
    transition_goal(pool, goal_id, 2, RunState::Paused, RunState::Ready, None)
        .await
        .unwrap();
    let r2 = enqueue_dispatch(pool, goal_id, 2, "After the pause.")
        .await
        .unwrap();
    claim_dispatch(pool, &r2.request_id, 2, 1).await.unwrap();
    confirm_claim_before_transmit(pool, &r2.request_id)
        .await
        .unwrap();
    assert_eq!(
        confirm_claim_before_transmit(pool, r1)
            .await
            .unwrap_err()
            .code(),
        "NOT_CLAIMABLE"
    );
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    assert_eq!((goal.run_state, goal.revision), (RunState::Dispatching, 2));
    let r3 = enqueue_dispatch(pool, goal_id, 2, "A third instruction.")
        .await
        .unwrap();
    assert!(claim_dispatch(pool, &r3.request_id, 2, 1).await.is_err());
    assert!(ready_dispatches(pool, 10).await.unwrap().is_empty());

    // Across a generation change the old claim is not in flight on the new generation, so a
    // newer claim can exist when the stale confirm arrives: the goal must stay dispatching.
    let other = admit(&fixture, "msg-2").await;
    let other_goal = other.goal_id.as_str();
    let q1 = other.dispatch_request_id.as_str();
    claim_dispatch(pool, q1, 1, 1).await.unwrap();
    advance_generation(pool, other_goal, 1, Some("S9"))
        .await
        .unwrap();
    transition_goal(pool, other_goal, 1, RunState::Dispatching, RunState::Paused, None)
        .await
        .unwrap();
    transition_goal(pool, other_goal, 2, RunState::Paused, RunState::Ready, None)
        .await
        .unwrap();
    let q2 = enqueue_dispatch(pool, other_goal, 2, "On the new session.")
        .await
        .unwrap();
    assert_eq!(q2.generation, 2);
    claim_dispatch(pool, &q2.request_id, 2, 2).await.unwrap();
    let stale = confirm_claim_before_transmit(pool, q1).await.unwrap_err();
    assert_eq!(stale.code(), "SUPERSEDED");
    let goal = goal_of(pool, other_goal).await.unwrap().unwrap();
    assert_eq!(
        goal.run_state,
        RunState::Dispatching,
        "the newer claim keeps the goal dispatching"
    );
    confirm_claim_before_transmit(pool, &q2.request_id)
        .await
        .unwrap();
}

/// Review scenario C: a resend released before transmission does not fence the original
/// dispatch, whose late start becomes the live run.
#[tokio::test]
async fn scenario_c_a_released_claim_does_not_hide_the_run_that_really_started() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    let goal_id = admitted.goal_id.as_str();
    let r1 = admitted.dispatch_request_id.as_str();
    attach_session(pool, goal_id, 1, "S1").await.unwrap();
    claim_dispatch(pool, r1, 1, 1).await.unwrap();
    confirm_claim_before_transmit(pool, r1).await.unwrap();
    record_receipt(
        pool,
        r1,
        1,
        &Receipt::Uncertain {
            code: "SEND_TIMEOUT".into(),
        },
    )
    .await
    .unwrap();
    transition_goal(
        pool,
        goal_id,
        1,
        RunState::Dispatching,
        RunState::DeliveryUncertain,
        None,
    )
    .await
    .unwrap();
    resolve_uncertain_delivery(pool, goal_id, 1, RunState::DeliveryUncertain, None)
        .await
        .unwrap();
    let r2 = enqueue_dispatch(pool, goal_id, 1, "Resend.").await.unwrap();
    claim_dispatch(pool, &r2.request_id, 1, 1).await.unwrap();
    transition_goal(pool, goal_id, 1, RunState::Dispatching, RunState::Paused, None)
        .await
        .unwrap();
    let released = confirm_claim_before_transmit(pool, &r2.request_id)
        .await
        .unwrap_err();
    assert_eq!(released.code(), "SUPERSEDED");
    assert_eq!(dispatch_state(pool, &r2.request_id).await, "released");

    let late = record_receipt(pool, r1, 1, &started("run-a", "S1"))
        .await
        .unwrap();
    assert_eq!(late.state, DispatchState::Started);
    assert!(live(pool, goal_id).await);
    let binding = execution_binding_of(pool, goal_id).await.unwrap().unwrap();
    assert_eq!(binding.run_id.as_deref(), Some("run-a"));
    let to_ready = transition_goal(pool, goal_id, 2, RunState::Paused, RunState::Ready, None)
        .await
        .unwrap_err();
    assert_eq!(to_ready.code(), "DISPATCH_IN_FLIGHT", "{to_ready}");
    transition_goal(pool, goal_id, 2, RunState::Paused, RunState::Running, None)
        .await
        .unwrap();
    let r3 = enqueue_dispatch(pool, goal_id, 2, "A third instruction.")
        .await
        .unwrap();
    assert!(claim_dispatch(pool, &r3.request_id, 2, 1).await.is_err());
    assert!(ready_dispatches(pool, 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_restart_treats_an_unconfirmed_claim_as_uncertain() {
    let fixture = fixture().await;
    let pool = &fixture.pool;
    let admitted = admit(&fixture, "msg-1").await;
    let goal_id = admitted.goal_id.as_str();
    let r1 = admitted.dispatch_request_id.as_str();
    claim_dispatch(pool, r1, 1, 1).await.unwrap();

    let changed = reconcile_claims_after_restart(pool).await.unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].request_id, r1);
    assert_eq!(changed[0].state, DispatchState::Uncertain);
    assert_eq!(
        changed[0].uncertain_code.as_deref(),
        Some(CLAIM_UNCONFIRMED_AT_RESTART)
    );
    assert!(changed[0].is_in_flight());
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    assert_eq!((goal.run_state, goal.revision), (RunState::DeliveryUncertain, 1));
    assert!(reconcile_claims_after_restart(pool).await.unwrap().is_empty());
    assert_eq!(
        confirm_claim_before_transmit(pool, r1)
            .await
            .unwrap_err()
            .code(),
        "NOT_CLAIMABLE",
        "an unconfirmed claim is never transmitted after a restart"
    );
    let unresolved = transition_goal(
        pool,
        goal_id,
        1,
        RunState::DeliveryUncertain,
        RunState::Ready,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(unresolved.code(), "DISPATCH_IN_FLIGHT");

    let ready = resolve_uncertain_delivery(
        pool,
        goal_id,
        1,
        RunState::DeliveryUncertain,
        Some("owner resends"),
    )
    .await
    .unwrap();
    assert_eq!(ready.run_state, RunState::Ready);
    let resolved: Option<String> = sqlx::query_scalar(
        "SELECT owner_resolved_at FROM supervisor_dispatches WHERE request_id = ?",
    )
    .bind(r1)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(resolved.is_some());
    assert_eq!(dispatch_state(pool, r1).await, "uncertain", "its row is kept");
    let resend = enqueue_dispatch(pool, goal_id, 1, "Resend.").await.unwrap();
    claim_dispatch(pool, &resend.request_id, 1, 1).await.unwrap();
}

/// Exploration bounds: dispatches per goal and steps per ordering.
const WALK_DISPATCHES: usize = 2;
const WALK_DEPTH: usize = 8;
const WALK_SESSION: &str = "S1";

/// One event of the delivery protocol. Indexes name a goal's dispatches in creation order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Claim the ready intent, writing one first if the goal has none.
    Claim,
    /// The dispatcher's last check; on success the dispatch is transmitted at once.
    Confirm(usize),
    Accepted(usize),
    Started(usize),
    /// A settle; a running goal then moves to verifying.
    Settled(usize),
    /// A failed or unobserved send; the goal then moves to delivery_uncertain.
    Uncertain(usize),
    Pause,
    /// Resume to running, else to ready.
    Resume,
    /// The owner resolves the uncertain delivery.
    Resolve,
    Restart,
}

/// What the walk observes of one dispatch: state, written for the current revision, run id,
/// owner-resolved.
#[derive(Clone, Debug)]
struct WalkRow {
    request_id: String,
    state: String,
    current: bool,
    run_id: Option<String>,
    resolved: bool,
}

impl WalkRow {
    fn in_flight(&self) -> bool {
        matches!(self.state.as_str(), "claimed" | "accepted" | "started")
            || (self.state == "uncertain" && !self.resolved)
    }
}

#[derive(Clone, Debug)]
struct WalkState {
    run_state: RunState,
    bound_run: Option<String>,
    rows: Vec<WalkRow>,
    /// Dispatches whose bytes left: confirmed and transmitted.
    transmitted: BTreeSet<usize>,
}

impl WalkState {
    /// Everything later steps can depend on; ids and timestamps are left out.
    fn fingerprint(&self) -> String {
        let rows: Vec<String> = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                format!(
                    "{}/{}/{}/{}/{}",
                    row.state,
                    row.current,
                    row.run_id.as_deref().unwrap_or("-"),
                    row.resolved,
                    self.transmitted.contains(&index)
                )
            })
            .collect();
        format!(
            "{}|{}|{}",
            self.run_state,
            self.bound_run.as_deref().unwrap_or("-"),
            rows.join(",")
        )
    }

    fn candidates(&self) -> Vec<Step> {
        let mut steps = vec![Step::Pause, Step::Resume];
        if self.rows.len() < WALK_DISPATCHES || self.rows.iter().any(|row| row.state == "ready") {
            steps.push(Step::Claim);
        }
        for (index, row) in self.rows.iter().enumerate() {
            let sent = self.transmitted.contains(&index);
            if row.state == "claimed" && !sent {
                steps.push(Step::Confirm(index));
            }
            if sent && row.state != "settled" {
                steps.extend([
                    Step::Accepted(index),
                    Step::Started(index),
                    Step::Settled(index),
                    Step::Uncertain(index),
                ]);
            }
        }
        if self
            .rows
            .iter()
            .any(|row| row.state == "uncertain" && !row.resolved)
        {
            steps.push(Step::Resolve);
        }
        if self.rows.iter().any(|row| row.state == "claimed") {
            steps.push(Step::Restart);
        }
        steps
    }
}

/// True when the store refused the step; a storage error (a tripped constraint) fails the test:
/// every refusal must come from the protocol, not from the schema catching a bug.
fn refused<T>(outcome: Result<T, SupervisorError>, path: &[Step]) -> bool {
    match outcome {
        Ok(_) => false,
        Err(error) => {
            assert_ne!(error.code(), "STORAGE", "{path:?}: {error}");
            true
        }
    }
}

type WalkRowTuple = (String, String, i64, Option<String>, Option<String>);

async fn walk_rows(pool: &SqlitePool, goal_id: &str, revision: i64) -> Vec<WalkRow> {
    let rows: Vec<WalkRowTuple> = sqlx::query_as(
        "SELECT request_id, state, goal_revision, run_id, owner_resolved_at
         FROM supervisor_dispatches WHERE goal_id = ? ORDER BY id",
    )
    .bind(goal_id)
    .fetch_all(pool)
    .await
    .unwrap();
    rows.into_iter()
        .map(
            |(request_id, state, goal_revision, run_id, resolved)| WalkRow {
                request_id,
                state,
                current: goal_revision == revision,
                run_id,
                resolved: resolved.is_some(),
            },
        )
        .collect()
}

async fn apply_step(
    pool: &SqlitePool,
    goal_id: &str,
    step: Step,
    transmitted: &mut BTreeSet<usize>,
    path: &[Step],
) {
    let goal = goal_of(pool, goal_id).await.unwrap().unwrap();
    let rows = walk_rows(pool, goal_id, goal.revision).await;
    let (rev, state) = (goal.revision, goal.run_state);
    let run = |index: usize| format!("run-{index}");
    match step {
        Step::Claim => {
            let request_id = match rows.iter().find(|row| row.state == "ready") {
                Some(row) => Some(row.request_id.clone()),
                None => enqueue_dispatch(pool, goal_id, rev, "Next step.")
                    .await
                    .map(|dispatch| dispatch.request_id)
                    .map_err(|error| refused::<()>(Err(error), path))
                    .ok(),
            };
            if let Some(request_id) = request_id {
                refused(claim_dispatch(pool, &request_id, rev, 1).await, path);
            }
        }
        Step::Confirm(index) => {
            let outcome = confirm_claim_before_transmit(pool, &rows[index].request_id).await;
            if !refused(outcome, path) {
                transmitted.insert(index);
            }
        }
        Step::Accepted(index) => {
            let receipt = Receipt::Accepted;
            refused(
                record_receipt(pool, &rows[index].request_id, 1, &receipt).await,
                path,
            );
        }
        Step::Started(index) => {
            let receipt = started(&run(index), WALK_SESSION);
            refused(
                record_receipt(pool, &rows[index].request_id, 1, &receipt).await,
                path,
            );
        }
        Step::Settled(index) => {
            let receipt = settled(&run(index), WALK_SESSION);
            let outcome = record_receipt(pool, &rows[index].request_id, 1, &receipt).await;
            if !refused(outcome, path) && state == RunState::Running {
                let moved = transition_goal(pool, goal_id, rev, state, RunState::Verifying, None);
                refused(moved.await, path);
            }
        }
        Step::Uncertain(index) => {
            let receipt = Receipt::Uncertain {
                code: "SEND_TIMEOUT".into(),
            };
            let outcome = record_receipt(pool, &rows[index].request_id, 1, &receipt).await;
            if !refused(outcome, path)
                && matches!(state, RunState::Dispatching | RunState::Running)
            {
                let to = RunState::DeliveryUncertain;
                refused(
                    transition_goal(pool, goal_id, rev, state, to, None).await,
                    path,
                );
            }
        }
        Step::Pause => {
            let outcome = transition_goal(pool, goal_id, rev, state, RunState::Paused, None);
            refused(outcome.await, path);
        }
        Step::Resume => {
            let running = transition_goal(pool, goal_id, rev, state, RunState::Running, None);
            if refused(running.await, path)
                && matches!(
                    state,
                    RunState::Paused | RunState::DeliveryUncertain | RunState::Verifying
                )
            {
                let ready = transition_goal(pool, goal_id, rev, state, RunState::Ready, None);
                refused(ready.await, path);
            }
        }
        Step::Resolve => {
            let outcome = resolve_uncertain_delivery(pool, goal_id, rev, state, None);
            refused(outcome.await, path);
        }
        Step::Restart => {
            refused(reconcile_claims_after_restart(pool).await, path);
        }
    }
}

/// The invariant, checked after the last step of an ordering.
async fn assert_delivery_invariant(
    pool: &SqlitePool,
    goal_id: &str,
    state: &WalkState,
    path: &[Step],
) {
    let in_flight: Vec<usize> = (0..state.rows.len())
        .filter(|index| state.rows[*index].in_flight())
        .collect();
    assert!(in_flight.len() <= 1, "{path:?}: in flight {in_flight:?}");
    let outstanding: Vec<usize> = state
        .transmitted
        .iter()
        .copied()
        .filter(|index| {
            let row = &state.rows[*index];
            row.state != "settled" && !(row.state == "uncertain" && row.resolved)
        })
        .collect();
    assert!(
        outstanding.len() <= 1,
        "{path:?}: transmitted and unsettled {outstanding:?}"
    );
    if let Some(index) = in_flight.first() {
        let claimable = ready_dispatches(pool, 100).await.unwrap();
        assert!(
            claimable.iter().all(|dispatch| dispatch.goal_id != goal_id),
            "{path:?}: claimable while {index} is in flight"
        );
        if state.run_state == RunState::Ready {
            assert_eq!(
                state.rows[*index].state, "started",
                "{path:?}: only a late live run may be in flight on a ready goal"
            );
        }
    }
}

/// Replays `path` on a fresh goal, checks the invariant after its last step, then cancels the
/// goal so it never shows up among later goals' claimable intents.
async fn walk(fixture: &Fixture, walk_id: usize, path: &[Step]) -> WalkState {
    let pool = &fixture.pool;
    let goal_id = admit(fixture, &format!("walk-{walk_id}")).await.goal_id;
    attach_session(pool, &goal_id, 1, WALK_SESSION)
        .await
        .unwrap();
    let mut transmitted = BTreeSet::new();
    for step in path {
        apply_step(pool, &goal_id, *step, &mut transmitted, path).await;
    }
    let goal = goal_of(pool, &goal_id).await.unwrap().unwrap();
    let binding = execution_binding_of(pool, &goal_id).await.unwrap().unwrap();
    let state = WalkState {
        run_state: goal.run_state,
        bound_run: binding.run_id,
        rows: walk_rows(pool, &goal_id, goal.revision).await,
        transmitted,
    };
    assert_delivery_invariant(pool, &goal_id, &state, path).await;
    transition_goal(
        pool,
        &goal_id,
        goal.revision,
        goal.run_state,
        RunState::Cancelled,
        None,
    )
    .await
    .unwrap();
    state
}

/// Every ordering of claim, confirm, accepted, started, settled, uncertain, pause, resume,
/// owner resolution and restart over two dispatches, up to [`WALK_DEPTH`] steps, explored
/// breadth-first with orderings that reach an already seen state pruned. Deterministic.
#[tokio::test]
async fn every_bounded_event_ordering_keeps_at_most_one_dispatch_in_flight() {
    let fixture = fixture().await;
    let mut walks = 0;
    let root = walk(&fixture, walks, &[]).await;
    let mut seen = HashSet::from([root.fingerprint()]);
    let mut reached = vec![root.clone()];
    let mut frontier = vec![(Vec::<Step>::new(), root)];
    for _ in 0..WALK_DEPTH {
        let mut next = Vec::new();
        for (path, state) in &frontier {
            for step in state.candidates() {
                walks += 1;
                let mut child = path.clone();
                child.push(step);
                let observed = walk(&fixture, walks, &child).await;
                if seen.insert(observed.fingerprint()) {
                    reached.push(observed.clone());
                    next.push((child, observed));
                }
            }
        }
        frontier = next;
    }

    // The exploration reached the orderings the reviews found.
    let any_row = |wanted: &dyn Fn(&WalkRow) -> bool| {
        reached
            .iter()
            .any(|state| state.rows.iter().any(wanted))
    };
    assert!(
        any_row(&|row: &WalkRow| row.state == "released"),
        "released claim"
    );
    assert!(
        any_row(&|row: &WalkRow| row.state == "started" && row.resolved),
        "late live run after an owner resolution"
    );
    assert!(
        any_row(&|row: &WalkRow| {
            row.state == "uncertain" && row.resolved && row.run_id.is_some()
        }),
        "late receipt kept as evidence only"
    );
    assert!(
        reached
            .iter()
            .any(|state| state.rows.len() == WALK_DISPATCHES && state.transmitted.len() == 2),
        "two transmitted dispatches"
    );
    assert!(seen.len() > 50, "explored only {} states", seen.len());
}
