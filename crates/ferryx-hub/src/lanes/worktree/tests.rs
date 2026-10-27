use super::*;
use crate::lanes::state::insert_lane;

fn repo(name: &str) -> PathBuf {
    isolate_test_env();
    let base = std::env::temp_dir().join(format!("ferryx-wt-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("app");
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "lane@example.com"],
        &["config", "user.name", "lane"],
    ] {
        run(&root, args);
    }
    std::fs::write(root.join("a.txt"), "a").unwrap();
    run(&root, &["add", "a.txt"]);
    run(&root, &["commit", "-qm", "a"]);
    // Plain on Windows too, as `create` stores and compares paths.
    without_verbatim_prefix(std::fs::canonicalize(root).unwrap())
}

fn run(dir: &Path, args: &[&str]) -> String {
    let output = crate::child_env::std_command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

async fn verified(pool: &SqlitePool, key: &str) {
    for (to, reason) in [
        (LaneStatus::Working, "launched"),
        (LaneStatus::DoneClaimed, "claim"),
        (LaneStatus::VerifiedDone, "verified"),
    ] {
        transition(pool, key, to, Some(reason)).await.unwrap();
    }
}

async fn events(pool: &SqlitePool, key: &str) -> Vec<(String, String)> {
    sqlx::query_as("SELECT kind, body FROM lane_events WHERE lane_key = ? ORDER BY id")
        .bind(key)
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_lane_at_a_repo_root_gets_its_own_worktree_and_close_keeps_the_branch() {
    let root = repo("close");
    let cwd = root.to_string_lossy().into_owned();
    let pool = crate::storage::memory().await.unwrap();
    insert_lane(&pool, "ledger", "Ledger", "lead", &cwd, "file a.txt")
        .await
        .unwrap();

    let path = create(&pool, "ledger", &cwd)
        .await
        .unwrap()
        .expect("worktree");
    let expected = root.parent().unwrap().join("app-lanes").join("ledger");
    assert_eq!(Path::new(&path), expected);
    assert_eq!(run(&expected, &["branch", "--show-current"]), "lane/ledger");
    let lane_cwd: String = sqlx::query_scalar("SELECT cwd FROM lanes WHERE key = 'ledger'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(lane_cwd, path);
    // Once owned, a second launch does not make another tree.
    assert_eq!(create(&pool, "ledger", &cwd).await.unwrap(), None);

    // Work committed on the lane branch, then the lane is verified and closed.
    std::fs::write(expected.join("b.txt"), "b").unwrap();
    run(&expected, &["add", "b.txt"]);
    run(&expected, &["commit", "-qm", "b"]);
    verified(&pool, "ledger").await;
    close(&pool, "ledger", None).await.unwrap();
    assert!(!expected.exists());
    assert_eq!(
        run(&root, &["branch", "--list", "lane/ledger"]),
        "lane/ledger"
    );
    let status: String = sqlx::query_scalar("SELECT status FROM lanes WHERE key = 'ledger'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "closed");
    let reported = events(&pool, "ledger").await;
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0].0, "unmerged_branch");
    assert!(reported[0].1.contains("lane/ledger (1 commit(s) ahead)"));

    // Reopen restores the worktree from the branch with the lane's commit.
    transition(&pool, "ledger", LaneStatus::Working, Some("note: reopened"))
        .await
        .unwrap();
    restore(&pool, "ledger").await.unwrap();
    assert!(expected.join("b.txt").exists());
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[tokio::test]
async fn a_dirty_worktree_refuses_to_close_and_the_lead_is_told() {
    let root = repo("dirty");
    let cwd = root.to_string_lossy().into_owned();
    let pool = crate::storage::memory().await.unwrap();
    insert_lane(&pool, "gate", "Gate", "lead", &cwd, "file a.txt")
        .await
        .unwrap();
    let path = PathBuf::from(create(&pool, "gate", &cwd).await.unwrap().unwrap());
    std::fs::write(path.join("a.txt"), "changed").unwrap();
    verified(&pool, "gate").await;
    assert!(close(&pool, "gate", None).await.is_err());
    assert!(path.join("a.txt").exists(), "the owner's edit stays");
    let status: String = sqlx::query_scalar("SELECT status FROM lanes WHERE key = 'gate'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "verified_done");
    assert_eq!(events(&pool, "gate").await[0].0, "close_failed");
    // Not verified and not forced: refused before anything is touched.
    sqlx::query("UPDATE lanes SET status = 'working' WHERE key = 'gate'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(close(&pool, "gate", None).await.is_err());
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[tokio::test]
async fn a_subdirectory_or_a_plain_directory_keeps_its_cwd() {
    let root = repo("skip");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let pool = crate::storage::memory().await.unwrap();
    let sub = root.join("sub").to_string_lossy().into_owned();
    assert_eq!(create(&pool, "sub-lane", &sub).await.unwrap(), None);
    let plain = std::env::temp_dir().join(format!("ferryx-wt-plain-{}", std::process::id()));
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(
        create(&pool, "plain", &plain.to_string_lossy())
            .await
            .unwrap(),
        None
    );
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
    let _ = std::fs::remove_dir_all(plain);
}

#[tokio::test]
async fn a_missing_directory_keeps_its_cwd_without_reading_git_messages() {
    isolate_test_env();
    let pool = crate::storage::memory().await.unwrap();
    let missing = std::env::temp_dir()
        .join(format!("ferryx-wt-missing-{}", std::process::id()))
        .join("nowhere");
    assert_eq!(
        create(&pool, "missing", &missing.to_string_lossy())
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn a_worktree_that_cannot_be_recorded_is_removed_so_a_retry_succeeds() {
    let root = repo("unrecorded");
    let cwd = root.to_string_lossy().into_owned();
    let pool = crate::storage::memory().await.unwrap();
    insert_lane(&pool, "ledger", "Ledger", "lead", &cwd, "file a.txt")
        .await
        .unwrap();
    // The tree and branch are created, then recording them fails.
    sqlx::query(
        "CREATE TRIGGER refuse_owned BEFORE INSERT ON lane_owned_worktrees
         BEGIN SELECT RAISE(ABORT, 'refused for the test'); END",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(create(&pool, "ledger", &cwd).await.is_err());
    let expected = root.parent().unwrap().join("app-lanes").join("ledger");
    assert!(!expected.exists(), "the unrecorded tree is removed");
    assert_eq!(run(&root, &["branch", "--list", "lane/ledger"]), "");
    let lane_cwd: String = sqlx::query_scalar("SELECT cwd FROM lanes WHERE key = 'ledger'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(lane_cwd, cwd, "the lane still names its own cwd");
    let owned: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lane_owned_worktrees")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(owned, 0);

    // Nothing is left behind, so the next attempt does not hit "branch already exists".
    sqlx::query("DROP TRIGGER refuse_owned")
        .execute(&pool)
        .await
        .unwrap();
    let path = create(&pool, "ledger", &cwd)
        .await
        .unwrap()
        .expect("worktree");
    assert_eq!(Path::new(&path), expected);
    assert_eq!(run(&expected, &["branch", "--show-current"]), "lane/ledger");
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn a_windows_verbatim_path_is_stored_in_its_plain_form() {
    assert_eq!(
        without_verbatim_prefix(PathBuf::from(r"\\?\C:\code\app-lanes\sv-1")),
        PathBuf::from(r"C:\code\app-lanes\sv-1")
    );
    assert_eq!(
        without_verbatim_prefix(PathBuf::from(r"\\?\UNC\host\share\app")),
        PathBuf::from(r"\\host\share\app")
    );
    assert_eq!(
        without_verbatim_prefix(PathBuf::from("/home/owner/app")),
        PathBuf::from("/home/owner/app")
    );
    // Forms with no plain equivalent keep their prefix.
    for kept in [
        r"\\?\Volume{4c1b02c1-d990-11dc-99ae-806e6f6e6963}\code\app",
        r"\\?\GLOBALROOT\Device\HarddiskVolume2\code\app",
        r"\\?\UNC\host",
    ] {
        assert_eq!(without_verbatim_prefix(PathBuf::from(kept)), PathBuf::from(kept));
    }
    // So does a path that is valid only with it: longer than MAX_PATH.
    let long = format!(r"\\?\C:\{}", "d".repeat(300));
    assert_eq!(without_verbatim_prefix(PathBuf::from(&long)), PathBuf::from(&long));
    let fits = format!(r"C:\{}", "d".repeat(256));
    assert_eq!(
        without_verbatim_prefix(PathBuf::from(format!(r"\\?\{fits}"))),
        PathBuf::from(fits)
    );
}
