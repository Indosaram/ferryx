//! The due-claim driver and the startup resume. Time is tokio's paused clock: nothing sleeps.
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::*;
use crate::lanes::settle::{claim_done, ClaimRequest};
use crate::lanes::settle_close::record_close_intent;
use crate::lanes::state::{insert_lane, status_of, transition, LaneStatus};

fn git(root: &Path, args: &[&str]) {
    let output = crate::child_env::std_command("git")
        .args(["-c", "user.name=qa", "-c", "user.email=qa@example.invalid", "-C"])
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

/// A clean repository whose HEAD is also `refs/remotes/origin/main`.
fn repo() -> PathBuf {
    let root = std::env::temp_dir().join(format!("ferryx-due6-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    std::fs::write(root.join("marker"), "ok").unwrap();
    git(&root, &["add", "marker"]);
    git(&root, &["commit", "-q", "-m", "marker"]);
    git(&root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    root
}

async fn working_lane(pool: &SqlitePool, key: &str, cwd: &Path) {
    insert_lane(pool, key, "due", "lead", &cwd.to_string_lossy(), "file marker")
        .await
        .unwrap();
    transition(pool, key, LaneStatus::Working, None).await.unwrap();
}

#[tokio::test]
async fn a_claim_without_a_supervisor_goal_settles_when_its_window_elapses() {
    // Given a claimed lane that has no supervisor_goals row, so no wake is ever scheduled for it
    let pool = crate::storage::memory().await.unwrap();
    let root = repo();
    working_lane(&pool, "lane-a", &root).await;
    // The driver's clock follows tokio's clock, so the frozen advance below moves both.
    let start = tokio::time::Instant::now();
    let base = Utc::now();
    let clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync> = Arc::new(move || {
        base + chrono::Duration::from_std(tokio::time::Instant::now() - start).unwrap()
    });
    let cwd = root.to_string_lossy().into_owned();
    claim_done(
        &pool,
        ClaimRequest {
            target: crate::lanes::settle::SettleTarget { key: "lane-a", cwd: &cwd },
            evidence: "marker exists",
            advisory: None,
        },
        clock(),
    )
    .await
    .unwrap();
    let mut driver = DueDriver::new(pool.clone(), None);
    driver.clock = clock;
    let (armed, settled) = (driver.armed.clone(), driver.settled.clone());
    let wake = Arc::new(Notify::new());
    let task = tokio::spawn(driver.run(wake));
    armed.notified().await;
    assert_eq!(status_of(&pool, "lane-a").await.unwrap(), Some(LaneStatus::DoneClaimed));

    // When the objection window elapses. The clock is frozen only for the advance: sqlx waits
    // on its worker thread, and a frozen clock would auto-advance past its acquire timeout.
    let settled_once = settled.notified();
    tokio::pin!(settled_once);
    settled_once.as_mut().enable();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(5 * 60)).await;
    tokio::time::resume();
    settled_once.await;

    // Then the driver settled it without anyone calling settle_claim
    assert_eq!(status_of(&pool, "lane-a").await.unwrap(), Some(LaneStatus::VerifiedDone));
    task.abort();
    std::fs::remove_dir_all(&root).unwrap();
}

#[tokio::test]
async fn a_crash_after_a_goal_commit_resumes_a_working_lane_to_verified_done() {
    // Given a lane still Working whose close intent was committed (completion.rs crashed)
    let pool = crate::storage::memory().await.unwrap();
    let root = repo();
    working_lane(&pool, "lane-a", &root).await;
    record_close_intent(&pool, "lane-a", "goal converged", Utc::now()).await.unwrap();
    // and a check that would fail now: resume must not re-run it
    std::fs::remove_file(root.join("marker")).unwrap();

    // When the hub starts
    let resumed = resume_recorded_close_intents(&pool, None).await.unwrap();

    // Then the lane is finished from the intent
    assert_eq!(resumed, 1);
    assert_eq!(status_of(&pool, "lane-a").await.unwrap(), Some(LaneStatus::VerifiedDone));
    std::fs::remove_dir_all(&root).unwrap();
}

/// Records, at the moment of the stop, whether the intent was persisted and the tree existed.
struct Recorder {
    pool: SqlitePool,
    tree: PathBuf,
    seen: Mutex<Vec<(String, bool, bool)>>,
}

#[async_trait::async_trait]
impl SessionStopper for Recorder {
    async fn stop(&self, _host: Option<&str>, session_id: &str, lane_key: &str) -> Result<()> {
        let intent = close_intent(&self.pool, lane_key).await?.is_some();
        self.seen
            .lock()
            .unwrap()
            .push((session_id.to_owned(), intent, self.tree.exists()));
        Ok(())
    }
}

#[tokio::test]
async fn closing_stops_the_session_after_the_intent_and_before_the_worktree_goes() {
    // Given a Working lane with a session and a registered, clean worktree
    let pool = crate::storage::memory().await.unwrap();
    let root = repo();
    working_lane(&pool, "lane-a", &root).await;
    crate::lanes::state::set_pane(&pool, "lane-a", "pane-9").await.unwrap();
    let tree = std::env::temp_dir().join(format!("ferryx-due6-wt-{}", uuid::Uuid::new_v4()));
    git(&root, &["worktree", "add", "-q", "-b", "lane/lane-a", &tree.to_string_lossy()]);
    let tree = std::fs::canonicalize(&tree).unwrap();
    sqlx::query(
        "INSERT INTO lane_owned_worktrees(lane_key, path, state, repo, branch, start_commit)
         VALUES('lane-a', ?, 'registered', ?, 'lane/lane-a', NULL)",
    )
    .bind(tree.to_string_lossy().as_ref())
    .bind(root.to_string_lossy().as_ref())
    .execute(&pool)
    .await
    .unwrap();
    record_close_intent(&pool, "lane-a", "goal converged", Utc::now()).await.unwrap();
    let recorder = Recorder { pool: pool.clone(), tree: tree.clone(), seen: Mutex::new(Vec::new()) };

    // When the close is finished
    resume_recorded_close_intents(&pool, Some(&recorder)).await.unwrap();

    // Then stop ran once, with the intent persisted and the tree still present, and the tree is gone after
    assert_eq!(*recorder.seen.lock().unwrap(), vec![("pane-9".to_owned(), true, true)]);
    assert!(!tree.exists());
    std::fs::remove_dir_all(&root).unwrap();
}
