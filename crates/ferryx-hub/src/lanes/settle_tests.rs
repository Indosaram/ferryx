//! Todo 6: a done claim is a claim. These tests pin the window, the live re-checks, the lead's
//! objection, and the crash-safe close intent. No test sleeps: the clock is an argument, so
//! "after the window" is a value the test hands in, not a duration it waits for.
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use super::*;
use crate::lanes::state::{insert_lane, status_of, transition};

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

const T0: &str = "2026-11-27T10:00:00Z";
const AFTER: &str = "2026-11-27T10:06:00Z";

/// A temporary git repository with one commit whose only tracked file is `marker`.
struct Repo(PathBuf);

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = crate::child_env::std_command("git")
        .args([
            "-c",
            "user.name=qa",
            "-c",
            "user.email=qa@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-C",
        ])
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A clean repository: one commit on `refs/remotes/origin/main`, nothing uncommitted.
fn repo() -> Repo {
    let root = std::env::temp_dir().join(format!("ferryx-settle6-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let repo = Repo(root.clone());
    git(&root, &["init", "-q"]);
    std::fs::write(root.join("marker"), "ok").unwrap();
    git(&root, &["add", "marker"]);
    git(&root, &["commit", "-q", "-m", "marker"]);
    git(&root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    repo
}

/// One lane key with its repository path, kept alive for the borrow a settlement takes.
struct Case {
    key: &'static str,
    path: String,
}

impl Case {
    fn target(&self) -> SettleTarget<'_> {
        SettleTarget {
            key: self.key,
            cwd: &self.path,
        }
    }

    fn claim<'a>(&'a self, evidence: &'a str, advisory: Option<&'a str>) -> ClaimRequest<'a> {
        ClaimRequest {
            target: self.target(),
            evidence,
            advisory,
        }
    }
}

/// A working lane whose done check is `signals`, at `repo`'s path.
async fn lane(pool: &sqlx::SqlitePool, key: &'static str, repo: &Repo, signals: &str) -> Case {
    let path = repo.0.to_string_lossy().into_owned();
    insert_lane(pool, key, "settle", "lead", &path, signals)
        .await
        .unwrap();
    transition(pool, key, LaneStatus::Working, None)
        .await
        .unwrap();
    Case { key, path }
}

#[tokio::test]
async fn a_claim_closes_only_after_the_window_and_live_checks() {
    let pool = crate::storage::memory().await.unwrap();
    let repo = repo();
    let case = lane(&pool, "lane-a", &repo, "file marker").await;

    let claim = claim_done(&pool, case.claim("marker exists", Some("yes")), at(T0))
        .await
        .unwrap();
    assert_eq!(claim.close_at, at("2026-11-27T10:05:00Z"));
    assert!(claim.announced);
    assert_eq!(
        status_of(&pool, "lane-a").await.unwrap(),
        Some(LaneStatus::DoneClaimed)
    );

    // Before the window the claim is pending: nothing is closed and nothing is refused.
    let pending = settle_claim(&pool, case.target(), at("2026-11-27T10:04:59Z"))
        .await
        .unwrap();
    assert!(matches!(pending, Settlement::Pending { .. }), "{pending:?}");
    assert_eq!(
        status_of(&pool, "lane-a").await.unwrap(),
        Some(LaneStatus::DoneClaimed)
    );

    // At the deadline the live checks pass and the lane settles.
    let settled = settle_claim(&pool, case.target(), at("2026-11-27T10:05:00Z"))
        .await
        .unwrap();
    assert!(settled.passed(), "{settled:?}");
    assert_eq!(
        status_of(&pool, "lane-a").await.unwrap(),
        Some(LaneStatus::VerifiedDone)
    );
}

#[tokio::test]
async fn unsent_commits_turn_a_claim_into_an_objection() {
    let pool = crate::storage::memory().await.unwrap();
    let repo = repo();
    let case = lane(&pool, "lane-a", &repo, "file marker").await;
    claim_done(&pool, case.claim("marker exists", None), at(T0))
        .await
        .unwrap();
    // A commit the lane never pushed, made after the claim.
    std::fs::write(repo.0.join("later.txt"), "work").unwrap();
    git(&repo.0, &["add", "later.txt"]);
    git(&repo.0, &["commit", "-q", "-m", "unpushed"]);

    let settled = settle_claim(&pool, case.target(), at(AFTER)).await.unwrap();
    let Settlement::Objected { reasons } = &settled else {
        panic!("expected an objection, got {settled:?}");
    };
    assert!(
        reasons.iter().any(|reason| reason.contains(&case.path)),
        "the objection names the worktree holding the unpushed commit: {reasons:?}"
    );
    assert_eq!(
        status_of(&pool, "lane-a").await.unwrap(),
        Some(LaneStatus::NotDone)
    );
    assert!(repo.0.join("later.txt").exists(), "the work stays put");
}

#[tokio::test]
async fn a_failing_check_at_settle_rejects_the_claim_with_reasons() {
    let pool = crate::storage::memory().await.unwrap();
    let repo = repo();
    let case = lane(&pool, "lane-a", &repo, "file absent-marker").await;
    claim_done(&pool, case.claim("absent-marker exists", Some("yes")), at(T0))
        .await
        .unwrap();

    let settled = settle_claim(&pool, case.target(), at(AFTER)).await.unwrap();
    let Settlement::Objected { reasons } = &settled else {
        panic!("expected an objection, got {settled:?}");
    };
    assert!(
        reasons.iter().any(|reason| reason.contains("absent-marker")),
        "the objection names the failing check: {reasons:?}"
    );
    assert_eq!(
        status_of(&pool, "lane-a").await.unwrap(),
        Some(LaneStatus::NotDone)
    );
}

#[tokio::test]
async fn a_lead_objection_in_the_window_returns_not_done() {
    let pool = crate::storage::memory().await.unwrap();
    let repo = repo();
    let case = lane(&pool, "lane-a", &repo, "file marker").await;
    claim_done(&pool, case.claim("marker exists", None), at(T0))
        .await
        .unwrap();

    assert!(
        object_claim(&pool, "lane-a", &["the diff is not what I asked".to_owned()])
            .await
            .unwrap()
    );
    assert_eq!(
        status_of(&pool, "lane-a").await.unwrap(),
        Some(LaneStatus::NotDone)
    );
    let told: String =
        sqlx::query_scalar("SELECT body FROM lane_events WHERE lane_key = 'lane-a' AND kind = ?")
            .bind(OBJECTION_EVENT)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(told.contains("the diff is not what I asked"), "{told}");

    // The window is gone: settling now does not verify the lane.
    let after = settle_claim(&pool, case.target(), at(AFTER)).await.unwrap();
    assert!(!after.passed(), "{after:?}");
    assert_eq!(
        status_of(&pool, "lane-a").await.unwrap(),
        Some(LaneStatus::NotDone)
    );
}

#[tokio::test]
async fn a_crash_after_close_intent_resumes_the_same_close() {
    let pool = crate::storage::memory().await.unwrap();
    let repo = repo();
    let case = lane(&pool, "lane-a", &repo, "file marker").await;
    claim_done(&pool, case.claim("marker exists", None), at(T0))
        .await
        .unwrap();
    // The close was decided and recorded, then the process died before the status write.
    record_close_intent(&pool, "lane-a", "done claim", at("2026-11-27T10:05:00Z"))
        .await
        .unwrap();
    assert_eq!(
        status_of(&pool, "lane-a").await.unwrap(),
        Some(LaneStatus::DoneClaimed)
    );

    // A later pass resumes it; a check that would now fail cannot change the outcome, because
    // the intent is recorded and the checks are not run again.
    std::fs::write(repo.0.join("marker"), "changed").unwrap();
    let resumed = settle_claim(&pool, case.target(), at("2026-11-27T11:00:00Z"))
        .await
        .unwrap();
    assert!(
        matches!(
            resumed,
            Settlement::Settled {
                resumed: true,
                ..
            }
        ),
        "{resumed:?}"
    );
    assert_eq!(
        status_of(&pool, "lane-a").await.unwrap(),
        Some(LaneStatus::VerifiedDone)
    );
}

#[tokio::test]
async fn a_settled_claim_is_announced_once_and_records_its_deadline() {
    let pool = crate::storage::memory().await.unwrap();
    let repo = repo();
    let case = lane(&pool, "lane-a", &repo, "file marker").await;
    claim_done(&pool, case.claim("marker exists", None), at(T0))
        .await
        .unwrap();
    let again = claim_done(
        &pool,
        case.claim("marker exists", None),
        at("2026-11-27T10:01:00Z"),
    )
    .await
    .unwrap();
    assert!(!again.announced, "the lead hears a claim once");
    assert_eq!(again.close_at, at("2026-11-27T10:05:00Z"));
    let notices: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE lane_key = 'lane-a' AND kind = ?")
            .bind(CLAIM_EVENT)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(notices, 1);
    // The deadline is durable on the row, which is what the lane's wake reads.
    let stored: Option<String> =
        sqlx::query_scalar("SELECT claim_objection_until FROM lanes WHERE key = 'lane-a'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        stored.as_deref(),
        Some(at("2026-11-27T10:05:00Z").to_rfc3339().as_str())
    );
    let told: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM lane_transcript WHERE lane_key = 'lane-a' AND role = ?",
    )
    .bind(CLAIM_EVENT)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(told, 1, "the lane is told its close instant once");
}

#[tokio::test]
async fn an_undelivered_message_objection_names_the_message() {
    let pool = crate::storage::memory().await.unwrap();
    let repo = repo();
    let case = lane(&pool, "lane-a", &repo, "file marker").await;
    sqlx::query(
        "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at)
         VALUES('lane-a','lead','worker_report','not yet handed over','2026-11-27T10:00:30Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    claim_done(&pool, case.claim("marker exists", None), at(T0))
        .await
        .unwrap();

    let settled = settle_claim(&pool, case.target(), at(AFTER)).await.unwrap();
    let Settlement::Objected { reasons } = &settled else {
        panic!("expected an objection, got {settled:?}");
    };
    assert!(
        reasons.iter().any(|reason| reason.contains("worker_report")),
        "the objection names the undelivered message: {reasons:?}"
    );
}
