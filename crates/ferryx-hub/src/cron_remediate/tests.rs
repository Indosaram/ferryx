use std::sync::Mutex;

use super::*;

/// Records what remediation asked of the hub; reruns answer from a script.
#[derive(Default)]
struct Fake {
    receipts: Mutex<Vec<RunReceipt>>,
    reruns: Mutex<Vec<(String, bool)>>,
    lanes: Mutex<Vec<LaneRequest>>,
    outcomes: Mutex<Vec<String>>,
}

fn receipt(status: &str, output: &str) -> RunReceipt {
    RunReceipt {
        job_id: "job".into(),
        run_id: format!("run-{status}"),
        status: status.into(),
        output: Some(output.into()),
        last_delivery_error: None,
        error: (status != "succeeded").then(|| "boom".into()),
    }
}

#[async_trait]
impl Runtime for Fake {
    async fn open_lane(&self, request: LaneRequest) -> Result<()> {
        self.lanes.lock().unwrap().push(request);
        Ok(())
    }
    async fn rerun(&self, job: &str, approved: bool) -> Result<RunReceipt> {
        self.reruns.lock().unwrap().push((job.into(), approved));
        let mut receipts = self.receipts.lock().unwrap();
        Ok(if receipts.len() > 1 {
            receipts.remove(0)
        } else {
            receipts
                .first()
                .cloned()
                .unwrap_or_else(|| receipt("succeeded", "ok"))
        })
    }
    async fn quota_check(&self) -> Result<Value> {
        Ok(json!({"http_status": 200, "body": "remaining 0"}))
    }
    async fn log_excerpt(&self, job: &str) -> Result<String> {
        Ok(format!("{job} Traceback: KeyError 'x'"))
    }
    async fn outcome(&self, _problem: &Problem, message: &str) -> Result<()> {
        self.outcomes.lock().unwrap().push(message.to_owned());
        Ok(())
    }
}

async fn seeded(job: &str, payload: Value, detail: &str) -> (SqlitePool, i64) {
    let pool = crate::storage::memory().await.unwrap();
    sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES(?, 'every 1h', ?, 1)")
        .bind(job)
        .bind(payload.to_string())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO cron_problems(id, job_id, summary, created_at, status, kind, signature, detail, run_id, updated_at) VALUES('1', ?, ?, ?, 'open', 'failed', 'sig', ?, 'r0', ?)")
        .bind(job)
        .bind(detail)
        .bind(now_rfc3339())
        .bind(detail)
        .bind(now_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
    (pool, 1)
}

fn remediator(pool: &SqlitePool, cause: &str, fake: Arc<Fake>) -> Remediator {
    Remediator {
        pool: pool.clone(),
        judge: Judge::forced(cause),
        runtime: fake,
    }
}

async fn status(pool: &SqlitePool, id: i64) -> (String, Detail) {
    let p = problem(pool, id).await.unwrap().unwrap();
    (p.status, Detail::parse(&p.detail))
}

fn token_of(message: &str) -> String {
    message
        .split("--owner-token ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn a_transient_failure_gets_one_verified_rerun_and_is_fixed() {
    let (pool, id) = seeded("job", json!({"command": "true"}), "connection reset").await;
    let fake = Arc::new(Fake::default());
    let r = remediator(&pool, "transient_retry_once", fake.clone());
    r.handle_event(&json!({"problem_id": id}).to_string(), "meta")
        .await
        .unwrap();
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "fixed");
    assert_eq!(detail.reruns, 1);
    assert_eq!(detail.last_run_id.as_deref(), Some("run-succeeded"));
    assert_eq!(
        fake.reruns.lock().unwrap().as_slice(),
        &[("job".to_owned(), false)]
    );
    let outcomes = fake.outcomes.lock().unwrap().clone();
    assert!(
        outcomes[0].starts_with(
            "job: connection reset -> verified native rerun (re-run run-succeeded succeeded"
        ),
        "{outcomes:?}"
    );
    // A second event for the same problem does nothing: it is no longer open.
    r.on_problem(id, "meta").await.unwrap();
    assert_eq!(fake.reruns.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_unverified_rerun_goes_to_the_owner_with_a_one_time_token() {
    let (pool, id) = seeded(
        "job",
        json!({"command": "true", "expect": {"output_regex": "^sent \\d+"}}),
        "timeout",
    )
    .await;
    let fake = Arc::new(Fake::default());
    // Succeeds but prints the wrong thing: not verified.
    fake.receipts
        .lock()
        .unwrap()
        .push(receipt("succeeded", "nothing to do"));
    let r = remediator(&pool, "transient_retry_once", fake.clone());
    r.on_problem(id, "meta").await.unwrap();
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "needs_owner");
    let message = fake.outcomes.lock().unwrap().last().cloned().unwrap();
    assert!(
        message.starts_with("job: needs you - Verification failed"),
        "{message}"
    );
    assert!(message.contains(&format!(
        "omo-gateway cron approve-rerun {id} --owner-approved --owner-token "
    )));
    let token = token_of(&message);
    // Only the hash is stored, never the token.
    assert_eq!(
        detail.approval_hash.as_deref(),
        Some(hash_owner_token(&token).as_str())
    );
    let stored: String = sqlx::query_scalar("SELECT detail FROM cron_problems")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!stored.contains(&token));
    let decision: (String, String) =
        sqlx::query_as("SELECT class, status FROM inbox_decisions WHERE item_id = 'cron:1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(decision, ("owner".to_owned(), "open".to_owned()));

    // A wrong token is refused; the right one grants one more verified rerun and is spent.
    assert!(r.approve_rerun_with_token(id, "wrong").await.is_err());
    fake.receipts.lock().unwrap().clear();
    fake.receipts
        .lock()
        .unwrap()
        .push(receipt("succeeded", "sent 3"));
    let approved = r
        .approve_rerun_with_token(id, &token)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(approved.output.as_deref(), Some("sent 3"));
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "fixed");
    assert!(detail.owner_approved);
    assert!(detail.approval_hash.is_none());
    assert_eq!(
        fake.reruns.lock().unwrap().last(),
        Some(&("job".to_owned(), true))
    );
    assert!(r.approve_rerun_with_token(id, &token).await.is_err());
    let decision: String =
        sqlx::query_scalar("SELECT status FROM inbox_decisions WHERE item_id = 'cron:1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(decision, "done");
}

#[tokio::test]
async fn login_or_quota_trouble_is_the_owners_with_a_direct_quota_check() {
    let (pool, id) = seeded("job", json!({"command": "true"}), "HTTP 429 quota exceeded").await;
    let fake = Arc::new(Fake::default());
    let r = remediator(&pool, "needs_owner_login_or_quota", fake.clone());
    r.on_problem(id, "meta").await.unwrap();
    assert_eq!(status(&pool, id).await.0, "needs_owner");
    assert!(fake.reruns.lock().unwrap().is_empty());
    let message = fake.outcomes.lock().unwrap()[0].clone();
    assert!(
        message.contains("mahoquot direct result: {\"body\":\"remaining 0\",\"http_status\":200}"),
        "{message}"
    );
    let title: String = sqlx::query_scalar("SELECT title FROM inbox_items WHERE id = 'cron:1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        title.starts_with("job: needs you - Inspect provider quota"),
        "{title}"
    );
}

#[tokio::test]
async fn a_script_bug_opens_a_lane_that_settles_on_the_hubs_rerun() {
    let (pool, id) = seeded(
        "Daily Report",
        json!({"name": "daily", "script": "/usr/bin/python3 /opt/jobs/report.py"}),
        "Traceback (most recent call last): KeyError 'x'",
    )
    .await;
    let fake = Arc::new(Fake::default());
    let r = remediator(&pool, "script_bug_needs_fix", fake.clone());
    r.on_problem(id, "lead-1").await.unwrap();
    let (state, _) = status(&pool, id).await;
    assert_eq!(state, "remediating");
    let lane = fake.lanes.lock().unwrap()[0].clone();
    assert!(
        lane.key.starts_with("cron-fix-daily-report-"),
        "{}",
        lane.key
    );
    assert!(crate::lanes::state::valid_key(&lane.key));
    assert_eq!(lane.cwd, PathBuf::from("/opt/jobs"));
    assert_eq!(lane.lead, "lead-1");
    assert!(lane.brief.contains("Job: Daily Report / daily"));
    assert!(lane
        .brief
        .contains("Gateway log (UTC):\nDaily Report Traceback"));
    let signals = crate::lanes::signals::parse_signals(&lane.done).unwrap();
    assert!(
        matches!(&signals[0], crate::lanes::signals::DoneSignal::Command { argv, .. } if argv[3] == "Daily Report")
    );
    let stored_lane = problem(&pool, id).await.unwrap().unwrap().lane_key;
    assert_eq!(stored_lane.as_deref(), Some(lane.key.as_str()));
    assert!(
        fake.reruns.lock().unwrap().is_empty(),
        "the lane fixes first; nothing reruns yet"
    );
}

#[tokio::test]
async fn a_publishing_job_waits_for_the_owner_before_any_rerun() {
    let (pool, id) = seeded(
        "omon-playstore-daily-publish",
        json!({"command": "true"}),
        "connection reset",
    )
    .await;
    let fake = Arc::new(Fake::default());
    let r = remediator(&pool, "transient_retry_once", fake.clone());
    r.on_problem(id, "meta").await.unwrap();
    assert_eq!(status(&pool, id).await.0, "needs_owner");
    let lane = fake.lanes.lock().unwrap()[0].clone();
    assert!(lane.done.contains(".omo/cron-fix/"), "{}", lane.done);
    assert!(fake.reruns.lock().unwrap().is_empty());
    let message = fake.outcomes.lock().unwrap()[0].clone();
    assert!(message.contains("explicitly approve the publishing rerun"));
    // Approval runs it as an approved (publishing) rerun.
    r.approve_rerun_with_token(id, &token_of(&message))
        .await
        .unwrap();
    assert_eq!(
        fake.reruns.lock().unwrap().as_slice(),
        &[("omon-playstore-daily-publish".to_owned(), true)]
    );
    assert_eq!(status(&pool, id).await.0, "fixed");
}

#[tokio::test]
async fn an_exhausted_budget_and_a_stale_row_both_reach_the_owner() {
    let detail = Detail {
        cause: "boom".into(),
        reruns: MAX_RERUNS,
        ..Detail::default()
    };
    let (pool, id) = seeded(
        "job",
        json!({"command": "true"}),
        &serde_json::to_string(&detail).unwrap(),
    )
    .await;
    sqlx::query("UPDATE cron_problems SET status = 'remediating'")
        .execute(&pool)
        .await
        .unwrap();
    let fake = Arc::new(Fake::default());
    let r = remediator(&pool, "transient_retry_once", fake.clone());
    assert!(r.verify_once(id).await.unwrap().is_none());
    assert!(fake.reruns.lock().unwrap().is_empty());
    assert_eq!(status(&pool, id).await.0, "needs_owner");
    // Approval after an exhausted budget grants exactly one more attempt.
    let token = token_of(&fake.outcomes.lock().unwrap()[0]);
    r.approve_rerun_with_token(id, &token)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status(&pool, id).await.1.reruns, MAX_RERUNS);

    let stale =
        (chrono::Utc::now() - chrono::TimeDelta::seconds(STALE_REMEDIATING_SECS + 60)).to_rfc3339();
    sqlx::query("UPDATE cron_problems SET status = 'remediating', updated_at = ?")
        .bind(stale)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(r.sweep_stale_remediating().await.unwrap(), 1);
    assert_eq!(status(&pool, id).await.0, "needs_owner");
    assert!(fake
        .outcomes
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .contains("Remediation stalled"));
}

#[tokio::test]
async fn the_hub_runtime_queues_lanes_reruns_and_redacted_outcomes() {
    let (pool, id) = seeded(
        "job",
        json!({"command": "printf 'sent 1'"}),
        "Traceback: boom",
    )
    .await;
    let dir = std::env::temp_dir().join(format!("fx-remediate-{}", uuid::Uuid::new_v4().simple()));
    let mut hub = HubRuntime::new(pool.clone(), None);
    hub.briefs = dir.clone();
    hub.log_paths = vec![dir.join("missing.log")];
    let r = Remediator {
        pool: pool.clone(),
        judge: Judge::forced("script_bug_needs_fix"),
        runtime: Arc::new(hub),
    };
    // The scheduler's queued event is what the dispatcher hands over.
    crate::lanes::dispatch::enqueue(
        &pool,
        "job",
        "meta",
        "cron_problem",
        &json!({"problem_id": id}).to_string(),
    )
    .await
    .unwrap();
    assert_eq!(dispatch_once(&pool, &r).await.unwrap(), 1);
    assert_eq!(
        dispatch_once(&pool, &r).await.unwrap(),
        0,
        "a handled event is delivered"
    );
    let key = problem(&pool, id).await.unwrap().unwrap().lane_key.unwrap();
    let (lane_status, brief_path): (String, String) =
        sqlx::query_as("SELECT status, brief_path FROM lanes WHERE key = ?")
            .bind(&key)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(lane_status, "queued");
    let brief = std::fs::read_to_string(&brief_path).unwrap();
    assert!(brief.contains("(gateway log not found; no excerpt)"));
    let question: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM lane_events WHERE kind = 'question' AND lane_key = ?",
    )
    .bind(&key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(question, 1);

    // Settling the lane reruns the job natively; its real output verifies the fix.
    let remediator = Arc::new(r);
    let checks = {
        // settle_lane uses the installed remediator; call its parts directly.
        let receipt = remediator.verify_once(id).await.unwrap().unwrap();
        assert_eq!(receipt.status, "succeeded");
        assert_eq!(receipt.output.as_deref(), Some("sent 1"));
        receipt
    };
    assert!(verified(&checks, &json!({})).unwrap());
    assert_eq!(status(&pool, id).await.0, "fixed");
    let stored: Vec<String> = sqlx::query_scalar("SELECT message FROM cron_remediation_outcomes")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(
        stored[0].starts_with("job: Traceback: boom -> verified native rerun"),
        "{stored:?}"
    );
    let section = digest_section(&pool, chrono::Utc::now())
        .await
        .unwrap()
        .unwrap();
    assert!(
        section.starts_with("## Cron\n- job: failed / fixed (lane: cron-fix-job-"),
        "{section}"
    );
    // A clean success closes the remediation lane.
    reconcile_fixed(&pool, "job").await.unwrap();
    let closed: String = sqlx::query_scalar("SELECT status FROM lanes WHERE key = ?")
        .bind(&key)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(closed, "verified_done");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn owner_tokens_are_redacted_in_stored_copies() {
    assert_eq!(
        redact_token(
            "x\nrun: omo-gateway cron approve-rerun 4 --owner-approved --owner-token abc123"
        ),
        "x\nrun: omo-gateway cron approve-rerun 4 --owner-approved --owner-token <redacted>"
    );
    assert_eq!(redact_token("no token"), "no token");
    let detail = Detail::parse("plain cause");
    assert_eq!(detail.cause, "plain cause");
    assert_eq!(detail.reruns, 0);
}

#[tokio::test]
async fn a_fixed_problem_reopens_when_a_new_run_fails_the_same_way() {
    let pool = crate::storage::memory().await.unwrap();
    sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES('job','every 1h','{}',1)")
        .execute(&pool)
        .await
        .unwrap();
    let fail = |run: &'static str| {
        let pool = pool.clone();
        async move {
            crate::schedule::note_run_health(
                &pool,
                "job",
                run,
                "failed",
                "",
                Some("disk full"),
                &json!({}),
            )
            .await
            .unwrap();
        }
    };
    fail("r1").await;
    fail("r2").await;
    let events = || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM lane_events WHERE kind = 'cron_problem'")
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    assert_eq!(
        events().await,
        1,
        "the same open problem is not queued twice"
    );
    crate::schedule::note_run_health(&pool, "job", "r3", "succeeded", "ok", None, &json!({}))
        .await
        .unwrap();
    let state: String = sqlx::query_scalar("SELECT status FROM cron_problems")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "fixed");
    fail("r4").await;
    let (state, run): (String, String) = sqlx::query_as("SELECT status, run_id FROM cron_problems")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((state.as_str(), run.as_str()), ("open", "r4"));
    assert_eq!(events().await, 2);
}
