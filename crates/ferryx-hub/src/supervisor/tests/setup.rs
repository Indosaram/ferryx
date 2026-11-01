//! Installation set-up: one identity per installation, idempotent re-runs, and refusals that
//! write nothing instead of rebinding.
use sqlx::SqlitePool;

use crate::supervisor::*;

const OWNER: &str = "qa-owner";
const REPO: &str = "github.com/example/qa-supervisor";
const HOST: &str = "local";
const DIR: &str = "/work/qa-supervisor/.git";

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

async fn rows(pool: &SqlitePool) -> [i64; 3] {
    [
        count(pool, "SELECT COUNT(*) FROM supervisor_identity").await,
        count(pool, "SELECT COUNT(*) FROM supervisor_projects").await,
        count(pool, "SELECT COUNT(*) FROM supervisor_project_hosts").await,
    ]
}

#[tokio::test]
async fn setup_rerun_with_the_same_arguments_returns_the_same_ids() {
    let pool = crate::storage::memory().await.unwrap();
    assert!(identities(&pool).await.unwrap().is_empty());
    let (identity, project, host) = provision_owner(&pool, OWNER, "qa-supervisor", REPO, HOST, DIR)
        .await
        .unwrap();
    assert_eq!(identity.owner_principal, OWNER);
    assert_eq!(identity.memory_name, MEMORY_NAME);
    assert_eq!(project.supervisor_id, identity.supervisor_id);
    assert_eq!(host.project_id, project.project_id);
    assert_eq!(host.git_common_dir, DIR);
    assert_eq!(rows(&pool).await, [1, 1, 1]);

    let (again_identity, again_project, again_host) =
        provision_owner(&pool, OWNER, "qa-supervisor", REPO, HOST, DIR)
            .await
            .unwrap();
    assert_eq!(again_identity, identity);
    assert_eq!(again_project, project);
    assert_eq!(again_host, host, "an unchanged mapping is not rewritten");
    assert_eq!(rows(&pool).await, [1, 1, 1]);
    assert_eq!(identities(&pool).await.unwrap(), vec![identity.clone()]);
    // Admission resolves the set-up project and host without any choice.
    assert_eq!(
        project_for_host(&pool, HOST, DIR).await.unwrap(),
        Some(project)
    );
}

#[tokio::test]
async fn setup_refuses_to_rebind_and_writes_nothing() {
    let pool = crate::storage::memory().await.unwrap();
    let (_, project, _) = provision_owner(&pool, OWNER, "qa-supervisor", REPO, HOST, DIR)
        .await
        .unwrap();

    let moved = provision_owner(&pool, OWNER, "qa-supervisor", REPO, HOST, "/elsewhere/.git")
        .await
        .unwrap_err();
    assert_eq!(moved.code(), SETUP_CONFLICT, "{moved}");

    let renamed = provision_owner(&pool, OWNER, "renamed", REPO, HOST, DIR)
        .await
        .unwrap_err();
    assert_eq!(renamed.code(), SETUP_CONFLICT, "{renamed}");

    // The same directory on the same host cannot also be another project's checkout, and the
    // refused project is not left behind.
    let claimed = provision_owner(&pool, OWNER, "other", "github.com/example/other", HOST, DIR)
        .await
        .unwrap_err();
    assert_eq!(claimed.code(), SETUP_CONFLICT, "{claimed}");

    assert_eq!(rows(&pool).await, [1, 1, 1]);
    let dir: String = sqlx::query_scalar(
        "SELECT git_common_dir FROM supervisor_project_hosts WHERE project_id = ?",
    )
    .bind(&project.project_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(dir, DIR);
}

#[tokio::test]
async fn setup_reuses_an_existing_identity_and_never_adds_a_second_owner() {
    let pool = crate::storage::memory().await.unwrap();
    let existing = ensure_identity(&pool, OWNER).await.unwrap();

    let (identity, _, _) = provision_owner(&pool, OWNER, "qa-supervisor", REPO, HOST, DIR)
        .await
        .unwrap();
    assert_eq!(identity, existing, "the installation's identity is reused");

    let stranger = provision_owner(&pool, "someone-else", "theirs", "github.com/x/y", HOST, "/y/.git")
        .await
        .unwrap_err();
    assert_eq!(stranger.code(), SETUP_CONFLICT, "{stranger}");
    assert_eq!(rows(&pool).await, [1, 1, 1]);
    assert!(identity_of(&pool, "someone-else").await.unwrap().is_none());
    // A second project for the same owner is fine: one identity, several projects.
    provision_owner(&pool, OWNER, "second", "github.com/example/second", HOST, "/second/.git")
        .await
        .unwrap();
    assert_eq!(rows(&pool).await, [1, 2, 2]);
}

#[tokio::test]
async fn setup_validates_its_arguments() {
    let pool = crate::storage::memory().await.unwrap();
    for (owner, host, dir) in [(" qa-owner", HOST, DIR), (OWNER, "", DIR), (OWNER, HOST, "")] {
        let refused = provision_owner(&pool, owner, "qa-supervisor", REPO, host, dir)
            .await
            .unwrap_err();
        assert_eq!(refused.code(), "INVALID_REQUEST", "{refused}");
    }
    assert_eq!(rows(&pool).await, [0, 0, 0]);
}

/// A WAL file database with several connections, so the two provisions really contend for the
/// write lock (BEGIN IMMEDIATE) instead of sharing one in-memory connection.
async fn file_pool() -> (SqlitePool, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("ferryx-hub-setup-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let pool = crate::storage::connect(&dir.join("hub.sqlite")).await.unwrap();
    (pool, dir)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_setups_keep_one_owner_and_one_mapping() {
    let (pool, dir) = file_pool().await;
    let (mine, theirs) = tokio::join!(
        tokio::spawn({
            let pool = pool.clone();
            async move { provision_owner(&pool, OWNER, "qa-supervisor", REPO, HOST, DIR).await }
        }),
        tokio::spawn({
            let pool = pool.clone();
            async move { provision_owner(&pool, "someone-else", "theirs", "github.com/x/y", HOST, "/y/.git").await }
        }),
    );
    let (mine, theirs) = (mine.unwrap(), theirs.unwrap());
    assert_eq!(
        usize::from(mine.is_ok()) + usize::from(theirs.is_ok()),
        1,
        "exactly one owner wins: {mine:?} / {theirs:?}"
    );
    let loser = mine.as_ref().err().or(theirs.as_ref().err()).unwrap();
    assert_eq!(loser.code(), SETUP_CONFLICT, "{loser}");
    assert_eq!(rows(&pool).await, [1, 1, 1]);
    pool.close().await;
    let _ = tokio::fs::remove_dir_all(&dir).await;

    let (pool, dir) = file_pool().await;
    let (first, second) = tokio::join!(
        tokio::spawn({
            let pool = pool.clone();
            async move { provision_owner(&pool, OWNER, "qa-supervisor", REPO, HOST, DIR).await }
        }),
        tokio::spawn({
            let pool = pool.clone();
            async move { provision_owner(&pool, OWNER, "qa-supervisor", REPO, HOST, DIR).await }
        }),
    );
    let (first, second) = (first.unwrap().unwrap(), second.unwrap().unwrap());
    assert_eq!(first.0, second.0, "one identity");
    assert_eq!(first.1, second.1, "one project");
    assert_eq!(first.2.project_id, second.2.project_id);
    assert_eq!(first.2.git_common_dir, second.2.git_common_dir);
    assert_eq!(rows(&pool).await, [1, 1, 1]);
    pool.close().await;
    let _ = tokio::fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn an_identity_alone_is_provisioned_once_and_never_for_a_second_owner() {
    let pool = crate::storage::memory().await.unwrap();
    let identity = provision_identity(&pool, OWNER).await.unwrap();
    assert_eq!(identity.owner_principal, OWNER);
    assert_eq!(identity.memory_name, MEMORY_NAME);
    assert_eq!(rows(&pool).await, [1, 0, 0], "no project is created");
    assert_eq!(provision_identity(&pool, OWNER).await.unwrap(), identity);
    let stranger = provision_identity(&pool, "someone-else").await.unwrap_err();
    assert_eq!(stranger.code(), SETUP_CONFLICT, "{stranger}");
    assert_eq!(provision_identity(&pool, " padded").await.unwrap_err().code(), "INVALID_REQUEST");
    // A later full setup reuses it.
    let (again, _, _) = provision_owner(&pool, OWNER, "qa-supervisor", REPO, HOST, DIR)
        .await
        .unwrap();
    assert_eq!(again, identity);
    assert_eq!(rows(&pool).await, [1, 1, 1]);
}

#[test]
fn the_default_owner_is_the_slack_owner_else_the_local_account() {
    use crate::supervisor::setup::default_owner_principal;
    assert_eq!(default_owner_principal(Some("T1"), Some("U1"), "indo"), "slack:T1:U1");
    assert_eq!(default_owner_principal(Some("T1"), None, "indo"), "local:indo");
    assert_eq!(default_owner_principal(Some(" "), Some("U1"), "indo"), "local:indo");
    assert_eq!(default_owner_principal(None, None, " in\tdo\n"), "local:indo");
    assert_eq!(default_owner_principal(None, None, ""), "local:owner");
    let long = "a".repeat(400);
    assert_eq!(default_owner_principal(None, None, &long).chars().count(), 256);
}

mod status {
    #[tokio::test]
    async fn the_known_slack_team_is_the_one_stored_slack_workspace() {
        use crate::supervisor::setup::known_slack_team;
        let pool = crate::storage::memory().await.unwrap();
        assert_eq!(known_slack_team(&pool).await.unwrap(), None);
        for (key, platform, team) in [("s1", "slack", "T1"), ("s2", "discord", "G1")] {
            sqlx::query(
                "INSERT INTO sessions (session_key, platform, guild_id, channel_id, user_id)
                 VALUES (?, ?, ?, 'C1', 'U1')",
            )
            .bind(key)
            .bind(platform)
            .bind(team)
            .execute(&pool)
            .await
            .unwrap();
        }
        assert_eq!(known_slack_team(&pool).await.unwrap().as_deref(), Some("T1"));
        sqlx::query(
            "INSERT INTO sessions (session_key, platform, guild_id, channel_id, user_id)
             VALUES ('s3', 'slack', 'T2', 'C2', 'U1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(known_slack_team(&pool).await.unwrap(), None);
    }

    use super::*;
    use crate::meta::mhc_fault::SupervisorFault;
    use crate::meta::mhc_profile::REQUIRED_CAPABILITIES;
    use crate::meta::mhc_supervisor::{HealthState, SupervisorHealth};
    use crate::supervisor::setup::{set_setup_flags, setup_status, EngineView, NextAction};

    fn health(state: HealthState, drop: Option<&str>) -> SupervisorHealth {
        SupervisorHealth {
            state,
            generation: 1,
            pid: None,
            capabilities: REQUIRED_CAPABILITIES
                .iter()
                .filter(|capability| Some(**capability) != drop)
                .map(|capability| (*capability).to_owned())
                .collect(),
            server_version: None,
            fault: None,
            detail: None,
            queued_decisions: 0,
        }
    }

    fn ready() -> EngineView {
        EngineView { enabled: true, installed: true, health: Some(health(HealthState::Ready, None)) }
    }

    async fn next(pool: &SqlitePool, engine: EngineView) -> NextAction {
        setup_status(pool, engine).await.unwrap().next_action
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_async_engine_view_reads_off_the_runtime_and_falls_back_to_off() {
        // The reader runs on the blocking pool even on a single-threaded runtime.
        let read = EngineView::read_with(ready).await;
        assert_eq!(read, ready());
        // A reader that cannot finish counts as off; no engine is installed in tests.
        let failed = EngineView::read_with(|| panic!("the hub env file read failed")).await;
        assert_eq!(failed, EngineView { enabled: false, installed: false, health: None });
    }

    #[tokio::test]
    async fn the_engine_steps_come_first_and_name_the_fault() {
        let pool = crate::storage::memory().await.unwrap();
        assert_eq!(next(&pool, EngineView::default()).await, NextAction::EnableEngine);
        let restart = setup_status(&pool, EngineView { enabled: true, installed: false, health: None })
            .await
            .unwrap();
        assert_eq!(restart.next_action, NextAction::RestartRequired);
        assert!(restart.restart_required);
        assert_eq!(restart.engine_state, "absent");
        let mut waiting = ready();
        waiting.health = Some(health(HealthState::Waiting, None));
        assert_eq!(next(&pool, waiting).await, NextAction::WaitEngine);
        let mut never_started = ready();
        never_started.health = Some(SupervisorHealth { generation: 0, ..health(HealthState::Stopped, None) });
        let starting = setup_status(&pool, never_started).await.unwrap();
        assert_eq!((starting.engine_state.as_str(), starting.next_action), ("starting", NextAction::WaitEngine));

        let mut short = ready();
        short.health = Some(health(HealthState::Ready, Some("memory_forget")));
        let fix = setup_status(&pool, short).await.unwrap();
        assert_eq!(fix.next_action, NextAction::FixEngine);
        assert_eq!(fix.missing_capabilities, ["memory_forget"]);
        assert_eq!(fix.engine_fault.unwrap().code, "MHC_CAPABILITY_MISSING");
        let mut degraded = ready();
        degraded.health = Some(SupervisorHealth {
            fault: Some(SupervisorFault::missing_executable("mhc")),
            ..health(HealthState::Degraded, None)
        });
        let fault = setup_status(&pool, degraded).await.unwrap();
        assert_eq!(fault.next_action, NextAction::FixEngine);
        assert_eq!(fault.engine_state, "degraded");
        assert_eq!(fault.engine_fault.unwrap().code, "MHC_MISSING_EXECUTABLE");
    }

    #[tokio::test]
    async fn an_upgrade_walks_owner_projects_cutover_memory_and_adoption_in_order() {
        let pool = crate::storage::memory().await.unwrap();
        crate::lanes::state::insert_lane(&pool, "nightly", "Nightly", "lead", "/repo", "file marker")
            .await
            .unwrap();
        crate::supervisor::cutover::note_import(&pool).await.unwrap();
        assert_eq!(next(&pool, ready()).await, NextAction::CreateOwner);
        provision_identity(&pool, OWNER).await.unwrap();
        assert_eq!(next(&pool, ready()).await, NextAction::MapProjects);
        provision_owner(&pool, OWNER, "qa-supervisor", REPO, HOST, DIR).await.unwrap();
        let closed = setup_status(&pool, ready()).await.unwrap();
        assert_eq!(closed.next_action, NextAction::Cutover);
        assert_eq!(closed.projects, 1);
        assert_eq!(
            (closed.intake.unified_open, closed.intake.legacy.as_str(), closed.intake.origin.as_str()),
            (false, "open", "upgrade")
        );
        assert!(closed.intake.legacy_history && !closed.auto_cutover && !closed.memory_import.enabled);

        let engine = ready();
        crate::supervisor::cutover::cutover(&pool, engine.health.as_ref()).await.unwrap();
        set_setup_flags(&pool, Some(true), Some(true)).await.unwrap();
        let import = setup_status(&pool, ready()).await.unwrap();
        assert_eq!(import.next_action, NextAction::ImportMemory);
        assert!(import.auto_cutover && import.memory_import.enabled);
        assert_eq!(import.intake.legacy, "stopped");
        assert_eq!(import.adoptable, 1);
        assert_eq!(
            crate::supervisor::setup::adoptable_lane_keys(&pool).await.unwrap(),
            vec!["nightly".to_owned()]
        );

        set_setup_flags(&pool, None, Some(false)).await.unwrap();
        assert_eq!(next(&pool, ready()).await, NextAction::Adopt);
        crate::lanes::state::transition(&pool, "nightly", crate::lanes::state::LaneStatus::Failed, Some("given up"))
            .await
            .unwrap();
        let done = setup_status(&pool, ready()).await.unwrap();
        assert_eq!((done.adoptable, done.next_action), (0, NextAction::None));
    }

    #[tokio::test]
    async fn after_a_rollback_setup_does_not_ask_for_the_cutover_again() {
        let pool = crate::storage::memory().await.unwrap();
        crate::supervisor::cutover::note_import(&pool).await.unwrap();
        provision_owner(&pool, OWNER, "qa-supervisor", REPO, HOST, DIR).await.unwrap();
        assert_eq!(next(&pool, ready()).await, NextAction::Cutover);
        let engine = ready();
        crate::supervisor::cutover::cutover(&pool, engine.health.as_ref()).await.unwrap();
        crate::supervisor::cutover::rollback(&pool).await.unwrap();
        let rolled_back = setup_status(&pool, ready()).await.unwrap();
        assert_eq!(rolled_back.intake.origin, "rollback");
        assert!(!rolled_back.intake.unified_open);
        assert_eq!(rolled_back.next_action, NextAction::None);
    }

    #[tokio::test]
    async fn the_status_wire_shape_is_camel_case_with_snake_case_actions() {
        let pool = crate::storage::memory().await.unwrap();
        let value = serde_json::to_value(setup_status(&pool, EngineView::default()).await.unwrap()).unwrap();
        let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "adoptable", "autoCutover", "engineEnabled", "engineFault", "engineInstalled",
                "engineState", "intake", "memoryImport", "missingCapabilities", "nextAction",
                "owner", "projects", "restartRequired",
            ]
        );
        assert_eq!(value["nextAction"], "enable_engine");
        assert_eq!(value["engineState"], "absent");
        assert!(value["owner"].is_null() && value["engineFault"].is_null());
        assert_eq!(value["intake"], serde_json::json!({"unifiedOpen": true, "legacy": "idle", "origin": "fresh", "legacyHistory": false}));
        assert_eq!(value["memoryImport"], serde_json::json!({"enabled": false, "sessions": 0, "turns": 0, "lastAt": null}));
    }
}
