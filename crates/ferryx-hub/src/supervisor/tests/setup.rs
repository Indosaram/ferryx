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
