use super::*;

#[tokio::test]
async fn a_file_only_signal_list_is_refused_at_open() {
    // Given an isolated hub and an owner brief with its source thread.
    let pool = crate::storage::memory().await.unwrap();
    // When a lane offers only a worker-writable marker as proof.
    let result = open_lane(&pool, "file-only", "Fix it", "Owner request: fix it\nSource: slack:C123:1.2", "/tmp", "file done.txt", Some("slack:C123:1.2")).await;
    // Then no lane is admitted.
    assert!(result.is_err(), "file-only proof admitted: {result:?}");
}

#[tokio::test]
async fn a_command_check_that_already_passes_at_open_is_refused() {
    // Given an isolated hub and a command that passes before any work.
    let pool = crate::storage::memory().await.unwrap();
    // When that command is offered as the done check.
    let result = open_lane(&pool, "already", "Fix it", "Owner request: fix it\nSource: slack:C123:1.2", "/tmp", r#"command ["git","--version"]"#, Some("slack:C123:1.2")).await;
    // Then the contract is refused rather than certifying existing behavior.
    assert!(result.is_err(), "already-passing check admitted: {result:?}");
}

#[tokio::test]
async fn a_file_check_next_to_a_command_check_is_accepted() {
    // Given a contract whose command fails before work starts.
    let pool = crate::storage::memory().await.unwrap();
    // When file evidence accompanies a meaningful command.
    let result = open_lane(&pool, "combined", "Fix it", "Owner request: fix it\nSource: slack:C123:1.2", "/tmp", r#"file done.txt; command ["false"]"#, Some("slack:C123:1.2")).await;
    // Then the lane is accepted.
    assert!(result.is_ok(), "combined proof refused: {result:?}");
}

#[tokio::test]
async fn lane_open_posts_a_kickoff_with_its_checks() {
    // Given an isolated hub.
    let pool = crate::storage::memory().await.unwrap();
    // When a lane opens with a failing check.
    let key = open_lane(&pool, "kickoff", "Fix it", "Owner request: fix it\nSource: slack:C123:1.2", "/tmp", r#"command ["false"]"#, Some("slack:C123:1.2")).await.unwrap();
    // Then the kickoff records both the work and check.
    let body: String = sqlx::query_scalar("SELECT body FROM lane_events WHERE lane_key = ? AND kind = 'kickoff'").bind(key).fetch_one(&pool).await.unwrap();
    assert!(body.contains("Fix it") && body.contains("command"), "kickoff omitted work/check: {body}");
}

#[tokio::test]
async fn an_open_legacy_file_only_lane_is_blocked_at_start() {
    let pool = crate::storage::memory().await.unwrap();
    crate::lanes::state::insert_lane(&pool, "old-file", "Old", "lead", "/tmp", "file marker").await.unwrap();
    assert_eq!(migrate_legacy_file_only_lanes(&pool).await.unwrap(), 1);
    let status: String = sqlx::query_scalar("SELECT status FROM lanes WHERE key = 'old-file'").fetch_one(&pool).await.unwrap();
    assert_eq!(status, "blocked");
    let notices: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE lane_key = 'old-file' AND kind = 'legacy_file_only_done_check'").fetch_one(&pool).await.unwrap();
    assert_eq!(notices, 1);
    assert_eq!(migrate_legacy_file_only_lanes(&pool).await.unwrap(), 1);
    let notices: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE lane_key = 'old-file' AND kind = 'legacy_file_only_done_check'").fetch_one(&pool).await.unwrap();
    assert_eq!(notices, 1);
}

#[tokio::test]
async fn a_cron_fix_lane_keeps_its_rerun_settlement() {
    let pool = crate::storage::memory().await.unwrap();
    crate::lanes::state::insert_lane(&pool, "cron-fix-task", "Cron fix", "lead", "/tmp", "file marker").await.unwrap();
    let stored: String = sqlx::query_scalar("SELECT done_signals FROM lanes WHERE key = 'cron-fix-task'").fetch_one(&pool).await.unwrap();
    assert!(stored.contains("file marker"));
    assert!(crate::cron_remediate::settle_lane(&pool, "cron-fix-task").await.unwrap().is_none());
}

#[tokio::test]
async fn lane_open_without_brief_or_thread_is_refused() {
    // Given an isolated hub.
    let pool = crate::storage::memory().await.unwrap();
    // When a caller omits the owner's brief and source thread.
    let result = open_lane(&pool, "missing", "Fix it", "", "/tmp", "merged acme/app#12", None).await;
    // Then it is not admitted.
    assert!(result.is_err(), "incomplete contract admitted: {result:?}");
}

#[tokio::test]
async fn qa_happy_lane_open_with_failing_test_succeeds_and_posts_kickoff() {
    let pool = crate::storage::memory().await.unwrap();
    let dir = std::env::temp_dir().join(format!("ferryx-qa-happy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Command check fails at open ("cargo test t_fix" fails in empty dir), so it is accepted
    let result = open_lane(
        &pool,
        "qa-happy",
        "Fix test t_fix",
        "Owner request: fix test t_fix\nSource: slack:C123:1700000000.000100",
        dir.to_str().unwrap(),
        r#"command ["cargo","test","t_fix"]"#,
        Some("slack:C123:1700000000.000100"),
    ).await;
    assert!(result.is_ok(), "failing test at open should be accepted: {result:?}");
    let key = result.unwrap();

    let kickoff: String = sqlx::query_scalar("SELECT body FROM lane_events WHERE lane_key = ? AND kind = 'kickoff'")
        .bind(&key)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(kickoff.contains("Fix test t_fix") && kickoff.contains(r#"command ["cargo","test","t_fix"]"#));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn qa_failure_file_only_and_true_command_refused_at_open() {
    let pool = crate::storage::memory().await.unwrap();
    let dir = std::env::temp_dir().join(format!("ferryx-qa-fail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // failure 1: open with file done.txt only is refused
    let file_only = open_lane(
        &pool,
        "qa-fail-file",
        "Fix file",
        "Owner request: fix file\nSource: slack:C123:1700000000.000100",
        dir.to_str().unwrap(),
        "file done.txt",
        Some("slack:C123:1700000000.000100"),
    ).await;
    assert!(file_only.is_err(), "file done.txt only must be refused");

    // failure 2: open with command ["true"] is refused
    let true_cmd = open_lane(
        &pool,
        "qa-fail-true",
        "Fix true",
        "Owner request: fix true\nSource: slack:C123:1700000000.000100",
        dir.to_str().unwrap(),
        r#"command ["true"]"#,
        Some("slack:C123:1700000000.000100"),
    ).await;
    assert!(true_cmd.is_err(), "command [\"true\"] must be refused");
    let _ = std::fs::remove_dir_all(&dir);
}
