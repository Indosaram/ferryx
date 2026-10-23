use super::*;
use crate::lanes::state::insert_lane;

fn repo(name: &str) -> PathBuf {
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
    std::fs::canonicalize(root).unwrap()
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
