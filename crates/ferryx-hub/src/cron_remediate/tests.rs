use std::sync::Mutex;

use super::*;

/// Records what remediation asked of the hub; reruns answer from a script.
#[derive(Default)]
struct RecordingFake {
    receipts: Mutex<Vec<RunReceipt>>,
    reruns: Mutex<Vec<(String, bool)>>,
    lanes: Mutex<Vec<LaneRequest>>,
    lane_updates: Mutex<Vec<LaneRequest>>,
    outcomes: Mutex<Vec<String>>,
    gate: tokio::sync::OnceCell<HubRuntime>,
}

async fn allocation_db() -> (SqlitePool, PathBuf) {
    let dir = std::env::temp_dir().join(format!("fx-cron-claims-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let pool = crate::storage::connect(&dir.join("hub.db")).await.unwrap();
    sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES('job', 'every 1h', '{}', 1), ('other', 'every 1h', '{}', 1)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO cron_problems(id, job_id, summary, created_at, status, kind, signature, detail, run_id, updated_at) VALUES('1', 'job', 'timeout', ?, 'open', 'failed', 'sig', 'timeout', 'r0', ?)")
        .bind(now_rfc3339()).bind(now_rfc3339()).execute(&pool).await.unwrap();
    (pool, dir)
}

#[tokio::test]
async fn concurrent_job_claims_have_one_winner() {
    let (pool, dir) = allocation_db().await;
    sqlx::query("INSERT INTO cron_problems(id, job_id, summary, created_at, status, kind, signature, detail, run_id, updated_at) SELECT '2', job_id, summary, created_at, status, kind, 'sig2', detail, 'r1', updated_at FROM cron_problems WHERE id = '1'")
        .execute(&pool).await.unwrap();
    // Distinct incidents must contend: status='open' alone lets both win.
    for second in [2] {
        sqlx::query("UPDATE cron_problems SET status = 'open'").execute(&pool).await.unwrap();
        let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            tokio::join!(
                claim_problem(&pool, 1, "timeout", None),
                claim_problem(&pool, second, "timeout", None),
            )
        }).await.unwrap();
        let results = [first, second];
        assert_eq!(results.iter().filter(|r| matches!(r, Ok(1))).count(), 1);
        assert_eq!(results.iter().filter(|r| matches!(r, Err(e) if sibling_claim_busy(e))).count(), 1);
        let claimed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cron_problems WHERE status = 'remediating'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(claimed, 1);
    }
    pool.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn shared_lane_stays_open_until_all_jobs_are_fixed() {
    let (pool, dir) = allocation_db().await;
    crate::lanes::state::insert_lane(&pool, "shared-cron", "repair", "meta", "/tmp", "[]")
        .await.unwrap();
    sqlx::query("UPDATE cron_problems SET status = 'fixed', lane_key = 'shared-cron' WHERE id = '1'")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO cron_problems(id, job_id, summary, created_at, status, kind, signature, detail, run_id, updated_at, lane_key) SELECT '2', 'other', summary, created_at, 'remediating', kind, 'sig2', detail, 'r1', updated_at, lane_key FROM cron_problems WHERE id = '1'")
        .execute(&pool).await.unwrap();
    reconcile_fixed(&pool, "job").await.unwrap();
    let state: String = sqlx::query_scalar("SELECT status FROM lanes WHERE key = 'shared-cron'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(state, "queued");
    let history: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lane_history WHERE lane_key = 'shared-cron'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(history, 0);
    sqlx::query("UPDATE cron_problems SET status = 'fixed' WHERE id = '2'")
        .execute(&pool).await.unwrap();
    reconcile_fixed(&pool, "job").await.unwrap();
    let state: String = sqlx::query_scalar("SELECT status FROM lanes WHERE key = 'shared-cron'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(state, "verified_done");
    pool.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn dispatch_failure_leaves_delivered_at_null() {
    let (pool, dir) = allocation_db().await;
    let r = remediator(&pool, "unknown", Arc::new(RecordingFake::default()));
    // Malformed input fails handle_event before it can acknowledge the event.
    crate::lanes::dispatch::enqueue(&pool, "job", "meta", "cron_problem", "{invalid")
        .await.unwrap();
    assert_eq!(dispatch_once(&pool, &r).await.unwrap(), 0);
    let deliveries: Vec<Option<String>> = sqlx::query_scalar("SELECT delivered_at FROM lane_events WHERE kind = 'cron_problem'")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(deliveries, vec![None]);
    pool.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn sibling_loss_stays_queued_then_retries_after_settlement() {
    let (pool, dir) = allocation_db().await;
    sqlx::query("INSERT INTO cron_problems(id, job_id, summary, created_at, status, kind, signature, detail, run_id, updated_at) SELECT '2', job_id, summary, created_at, status, kind, 'sig2', detail, 'r1', updated_at FROM cron_problems WHERE id = '1'")
        .execute(&pool).await.unwrap();
    assert_eq!(claim_problem(&pool, 1, "timeout", None).await.unwrap(), 1);
    let fake = Arc::new(RecordingFake::default());
    let r = remediator(&pool, "transient_retry_once", fake.clone());
    crate::lanes::dispatch::enqueue(&pool, "job", "meta", "cron_problem", &json!({"problem_id": 2}).to_string())
        .await.unwrap();
    let before = chrono::Utc::now();
    assert_eq!(dispatch_once(&pool, &r).await.unwrap(), 0);
    let (delivered, release): (Option<String>, String) = sqlx::query_as("SELECT delivered_at, release_at FROM lane_events WHERE kind = 'cron_problem'")
        .fetch_one(&pool).await.unwrap();
    assert!(delivered.is_none());
    let release = chrono::DateTime::parse_from_rfc3339(&release).unwrap();
    assert!(release >= before + chrono::TimeDelta::seconds(30));
    assert!(release <= chrono::Utc::now() + chrono::TimeDelta::seconds(30));
    assert_eq!(status(&pool, 2).await.0, "open");
    assert!(fake.outcomes.lock().unwrap().is_empty());
    // A far-future release proves the dispatcher honors backoff without sleeping.
    sqlx::query("UPDATE lane_events SET release_at = '9999-01-01T00:00:00+00:00'")
        .execute(&pool).await.unwrap();
    assert_eq!(dispatch_once(&pool, &r).await.unwrap(), 0);
    sqlx::query("UPDATE cron_problems SET status = 'needs_owner' WHERE id = '1'")
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE lane_events SET release_at = '2000-01-01T00:00:00+00:00'")
        .execute(&pool).await.unwrap();
    assert_eq!(dispatch_once(&pool, &r).await.unwrap(), 1);
    assert_eq!(status(&pool, 2).await.0, "fixed");
    assert_eq!(fake.outcomes.lock().unwrap().len(), 1);
    let delivered: Option<String> = sqlx::query_scalar("SELECT delivered_at FROM lane_events WHERE kind = 'cron_problem'")
        .fetch_one(&pool).await.unwrap();
    assert!(delivered.is_some());
    assert_eq!(dispatch_once(&pool, &r).await.unwrap(), 0);
    assert_eq!(fake.outcomes.lock().unwrap().len(), 1);
    pool.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn successful_claim_consumes_event_once() {
    let (pool, dir) = allocation_db().await;
    let fake = Arc::new(RecordingFake::default());
    let r = remediator(&pool, "transient_retry_once", fake.clone());
    crate::lanes::dispatch::enqueue(&pool, "job", "meta", "cron_problem", &json!({"problem_id": 1}).to_string())
        .await.unwrap();
    assert_eq!(dispatch_once(&pool, &r).await.unwrap(), 1);
    assert_eq!(status(&pool, 1).await.0, "fixed");
    let delivered: Option<String> = sqlx::query_scalar("SELECT delivered_at FROM lane_events WHERE kind = 'cron_problem'")
        .fetch_one(&pool).await.unwrap();
    assert!(delivered.is_some());
    assert_eq!(dispatch_once(&pool, &r).await.unwrap(), 0);
    assert_eq!(fake.outcomes.lock().unwrap().len(), 1);
    pool.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn an_automatic_recovery_is_recorded_without_an_owner_post() {
    let (pool, id) = seeded("job", json!({"command":"true"}), "timeout").await;
    let runtime = HubRuntime::new(pool.clone(), Some("owner-dm".into()));
    let problem = problem(&pool, id).await.unwrap().unwrap();
    runtime.outcome(&problem, "검증한 자동 복구 결과입니다.").await.unwrap();
    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cron_remediation_outcomes")
        .fetch_one(&pool).await.unwrap();
    let queued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_outbox")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(stored, 1);
    assert_eq!(queued, 0);
}

#[tokio::test]
async fn a_collector_failure_preserves_one_new_approval_but_not_repeated_notices() {
    let (pool, id) = seeded("collect", json!({"command":"collect-projects"}), "LOGIN_FAILED").await;
    let initial = crate::report_gate::Report::new(
        "collector", "autopilot", "autopilot:collect-error",
        crate::report_gate::Salience::Action, "수집을 완료하지 못했습니다. 로그인 확인이 필요합니다.",
    ).with_state("LOGIN_FAILED");
    crate::report_gate::queue(&pool, "owner-dm", &initial, false).await.unwrap();
    let r = Remediator {
        pool: pool.clone(), judge: Judge::forced("unused"),
        runtime: Arc::new(HubRuntime::new(pool.clone(), Some("owner-dm".into()))),
    };
    let problem = problem(&pool, id).await.unwrap().unwrap();
    for _ in 0..2 {
        r.needs_owner(&problem, &Outcome::Unverified("수집 결과가 없습니다".into()), None).await.unwrap();
    }
    let reports: Vec<String> = sqlx::query_scalar("SELECT report_json FROM report_delivery_parts ORDER BY outbox_id")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(reports.len(), 2);
    let notice: crate::report_gate::Report = serde_json::from_str(&reports[1]).unwrap();
    assert_eq!(notice.topic_key, "cron-remediation:collect");
    assert_eq!(notice.salience, crate::report_gate::Salience::Action);
    assert_eq!(crate::owner_alert::sweep(&pool, "owner-dm", chrono::Utc::now()).await.unwrap(), 0);
    let decisions: Vec<String> = sqlx::query_scalar("SELECT decision FROM report_suppressions ORDER BY id")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(decisions, ["sent", "sent", "duplicate"]);
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
impl Runtime for RecordingFake {
    async fn open_lane(&self, request: LaneRequest) -> Result<()> {
        self.lanes.lock().unwrap().push(request);
        Ok(())
    }
    async fn update_lane(&self, request: LaneRequest) -> Result<()> {
        self.lane_updates.lock().unwrap().push(request);
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
    fn records_obligations(&self) -> bool {
        true
    }
    async fn owner_notice_obligated(&self, problem: &Problem, message: &str, obligation: &str) -> Result<()> {
        let gate = self.gate.get_or_try_init(|| async {
            let pool = crate::storage::memory().await?;
            let mut hub = HubRuntime::new(pool, Some("fake-owner".into()));
            hub.transport = false;
            Ok::<_, HubError>(hub)
        }).await?;
        gate.owner_notice_obligated(problem, message, obligation).await?;
        self.outcome(problem, message).await
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

fn remediator(pool: &SqlitePool, cause: &str, fake: Arc<RecordingFake>) -> Remediator {
    let cause = cause.to_owned();
    Remediator {
        pool: pool.clone(),
        judge: Judge::scripted(move |purpose, _, _| match purpose {
            Purpose::CronOutcome => "completed".into(),
            _ => cause.clone(),
        }),
        runtime: fake,
    }
}

async fn status(pool: &SqlitePool, id: i64) -> (String, Detail) {
    let p = problem(pool, id).await.unwrap().unwrap();
    (p.status, Detail::parse(&p.detail))
}

#[tokio::test]
async fn a_transient_failure_gets_one_verified_rerun_and_is_fixed() {
    let (pool, id) = seeded("job", json!({"command": "true"}), "connection reset").await;
    let fake = Arc::new(RecordingFake::default());
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
    assert_eq!(
        outcomes[0], "job: 재실행했고 결과를 확인했습니다: ok",
        "{outcomes:?}"
    );
    assert_eq!(detail.outcome.as_deref(), Some("verified"));
    // A second event for the same problem does nothing: it is no longer open.
    r.on_problem(id, "meta").await.unwrap();
    assert_eq!(fake.reruns.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_unverified_rerun_goes_to_the_owner_with_a_local_approval_command() {
    let (pool, id) = seeded(
        "job",
        json!({"command": "true", "expect": {"output_regex": "^sent \\d+"}}),
        "timeout",
    )
    .await;
    let fake = Arc::new(RecordingFake::default());
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
        message.starts_with("job: 재실행은 끝났지만 결과를 확인하지 못했습니다: 출력이 기대한 형식"),
        "{message}"
    );
    assert_eq!(detail.outcome.as_deref(), Some("unverified"));
    let gate = fake.gate.get().unwrap();
    let recorded: (String, bool) = sqlx::query_as(
        "SELECT o.state, EXISTS(SELECT 1 FROM report_delivery_parts p WHERE p.report_id = o.report_id) FROM delivery_obligations o WHERE o.id = ?",
    ).bind(format!("{}cron:job:job", crate::owner_alert::OBLIGATION_PREFIX))
        .fetch_one(&gate.pool).await.unwrap();
    assert_eq!(recorded, ("pending".to_owned(), true));
    // The notice names the command that exists in this build and carries no secret.
    assert!(
        message.ends_with(&format!("ferryx hub cron approve-rerun {id}")),
        "{message}"
    );
    assert!(!message.contains("--owner-token"), "{message}");
    assert!(!message.contains("omo-gateway"), "{message}");
    assert!(detail.approval_hash.is_none());
    let decision: (String, String) =
        sqlx::query_as("SELECT class, status FROM inbox_decisions WHERE item_id = 'cron:job:job'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(decision, ("owner".to_owned(), "open".to_owned()));

    // No token was minted, so a token approval (the chat-leakable path) matches nothing.
    assert!(r.approve_rerun_with_token(id, "anything").await.is_err());
    fake.receipts.lock().unwrap().clear();
    fake.receipts
        .lock()
        .unwrap()
        .push(receipt("succeeded", "sent 3"));
    // The owner's control-socket approval grants one more verified rerun.
    let approved = r.approve_rerun(id).await.unwrap().unwrap();
    assert_eq!(approved.output.as_deref(), Some("sent 3"));
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "fixed");
    assert!(detail.owner_approved);
    assert!(detail.approval_hash.is_none());
    assert_eq!(
        fake.reruns.lock().unwrap().last(),
        Some(&("job".to_owned(), true))
    );
    assert!(
        r.approve_rerun(id).await.unwrap().is_none(),
        "approval is spent"
    );
    let decision: String =
        sqlx::query_scalar("SELECT status FROM inbox_decisions WHERE item_id = 'cron:job:job'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(decision, "done");
}

#[tokio::test]
async fn login_or_quota_trouble_is_the_owners_with_a_direct_quota_check() {
    let (pool, id) = seeded("job", json!({"command": "true"}), "HTTP 429 quota exceeded").await;
    let fake = Arc::new(RecordingFake::default());
    let r = remediator(&pool, "needs_owner_login_or_quota", fake.clone());
    r.on_problem(id, "meta").await.unwrap();
    assert_eq!(status(&pool, id).await.0, "needs_owner");
    assert!(fake.reruns.lock().unwrap().is_empty());
    let message = fake.outcomes.lock().unwrap()[0].clone();
    assert!(
        message.contains("할당량 직접 확인 결과: HTTP 200: remaining 0"),
        "{message}"
    );
    assert!(!message.contains('{'), "no raw JSON reaches the owner: {message}");
    let title: String =
        sqlx::query_scalar("SELECT title FROM inbox_items WHERE id = 'cron:job:job'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        title.starts_with("job: 모델 제공자 할당량·결제나 인증 정보를 확인해 주셔야 다시 돌 수 있습니다"),
        "{title}"
    );
}

/// Without a configured quota endpoint no probe runs and the owner text names no provider.
#[tokio::test]
async fn an_unconfigured_quota_endpoint_skips_the_probe_and_names_no_provider() {
    let (pool, id) = seeded("job", json!({"command": "true"}), "HTTP 429 quota exceeded").await;
    let mut hub = HubRuntime::new(pool.clone(), Some("owner-dm".into()));
    hub.quota_url = None;
    hub.transport = false;
    assert!(hub.quota_check().await.is_err());
    let r = Remediator {
        pool: pool.clone(),
        judge: Judge::forced("needs_owner_login_or_quota"),
        runtime: Arc::new(hub),
    };
    crate::locale::scope(crate::locale::OwnerLocale::english_utc(), r.on_problem(id, "meta"))
        .await
        .unwrap();
    let notice: String = sqlx::query_scalar("SELECT text FROM chat_outbox")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        notice.starts_with("job: it can run again once you check the model provider quota, billing or credentials"),
        "{notice}"
    );
    assert!(notice.contains("error: HTTP 429 quota exceeded"), "{notice}");
    assert!(!notice.to_ascii_lowercase().contains("mahoquot"), "{notice}");
    assert!(!notice.contains("127.0.0.1"), "{notice}");
    assert!(!notice.chars().any(crate::locale::is_hangul), "{notice}");
}

#[tokio::test]
async fn a_script_bug_opens_a_lane_that_settles_on_the_hubs_rerun() {
    let script = if cfg!(windows) { "C:/jobs/report.py" } else { "/opt/jobs/report.py" };
    let (pool, id) = seeded(
        "Daily Report",
        json!({"name": "daily", "script": format!("python3 {script}")}),
        "Traceback (most recent call last): KeyError 'x'",
    )
    .await;
    let fake = Arc::new(RecordingFake::default());
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
    assert_eq!(lane.title, "Cron fix: Daily Report");
    assert_eq!(lane.cwd, PathBuf::from(script).parent().unwrap());
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
async fn repeated_cron_failure_reuses_the_existing_active_lane() {
    let (pool, first_id) = seeded(
        "daily-report",
        json!({"script": "/opt/jobs/report.py"}),
        "first failure",
    )
    .await;
    let lane_key = "cron-fix-daily-report-live";
    crate::lanes::state::insert_lane(&pool, lane_key, "Cron root-cause remediation", "lead-1", "/opt/jobs", "command true")
        .await
        .unwrap();
    sqlx::query("UPDATE cron_problems SET status = 'remediating', lane_key = ? WHERE CAST(id AS INTEGER) = ?")
        .bind(lane_key)
        .bind(first_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO cron_problems(id, job_id, summary, created_at, status, kind, signature, detail, run_id, updated_at) VALUES('2', 'daily-report', 'second failure', ?, 'open', 'failed', 'sig-2', 'second failure', 'r2', ?)")
        .bind(now_rfc3339())
        .bind(now_rfc3339())
        .execute(&pool)
        .await
        .unwrap();

    let fake = Arc::new(RecordingFake::default());
    remediator(&pool, "script_bug_needs_fix", fake.clone())
        .on_problem(2, "lead-2")
        .await
        .unwrap();

    assert!(fake.lanes.lock().unwrap().is_empty(), "must not create a second lane");
    {
        let updates = fake.lane_updates.lock().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].key, lane_key);
        assert_eq!(updates[0].title, "Cron fix: daily-report");
    }
    assert_eq!(problem(&pool, 2).await.unwrap().unwrap().lane_key.as_deref(), Some(lane_key));
}

#[tokio::test]
async fn a_publishing_job_waits_for_the_owner_before_any_rerun() {
    let (pool, id) = seeded(
        "omon-playstore-daily-publish",
        json!({"command": "true"}),
        "connection reset",
    )
    .await;
    let fake = Arc::new(RecordingFake::default());
    let r = remediator(&pool, "transient_retry_once", fake.clone());
    r.on_problem(id, "meta").await.unwrap();
    assert_eq!(status(&pool, id).await.0, "needs_owner");
    let lane = fake.lanes.lock().unwrap()[0].clone();
    assert!(lane.done.contains(".omo/cron-fix/"), "{}", lane.done);
    assert!(fake.reruns.lock().unwrap().is_empty());
    let message = fake.outcomes.lock().unwrap()[0].clone();
    assert!(message.contains("게시 재실행을 명시적으로 승인해 주셔야 다시 돌 수 있습니다"));
    // Approval runs it as an approved (publishing) rerun; its output names the published item.
    fake.receipts
        .lock()
        .unwrap()
        .push(receipt("succeeded", "published https://play.google.com/store/apps/details?id=x"));
    r.approve_rerun(id).await.unwrap();
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
    let fake = Arc::new(RecordingFake::default());
    let r = remediator(&pool, "transient_retry_once", fake.clone());
    assert!(r.verify_once(id).await.unwrap().is_none());
    assert!(fake.reruns.lock().unwrap().is_empty());
    assert_eq!(status(&pool, id).await.0, "needs_owner");
    // Approval after an exhausted budget grants exactly one more attempt.
    r.approve_rerun(id).await.unwrap().unwrap();
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
        .contains("자동 복구가 진행 중 상태에서 멈췄습니다"));
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
        judge: Judge::scripted(|purpose, _, _| match purpose {
            Purpose::CronOutcome => "completed".into(),
            _ => "script_bug_needs_fix".into(),
        }),
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
    assert_eq!(
        stored[0], "job: 재실행했고 결과를 확인했습니다: sent 1",
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
    // Every occurrence, and a second pass changes nothing.
    let twice = redact_token("--owner-token a1 then --owner-token b2 end");
    assert_eq!(
        twice,
        "--owner-token <redacted> then --owner-token <redacted> end"
    );
    assert_eq!(redact_token(&twice), twice);
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

#[tokio::test]
async fn two_problems_of_one_job_share_one_open_owner_item() {
    let (pool, first) = seeded("job", json!({"command": "true"}), "HTTP 429 quota exceeded").await;
    sqlx::query("INSERT INTO cron_problems(id, job_id, summary, created_at, status, kind, signature, detail, run_id, updated_at) VALUES('2', 'job', 'disk full', ?, 'open', 'failed', 'sig2', 'disk full', 'r1', ?)")
        .bind(now_rfc3339())
        .bind(now_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
    let fake = Arc::new(RecordingFake::default());
    let r = remediator(&pool, "needs_owner_login_or_quota", fake.clone());
    r.on_problem(first, "meta").await.unwrap();
    let opened: String =
        sqlx::query_scalar("SELECT decided_at FROM inbox_decisions WHERE item_id = 'cron:job:job'")
            .fetch_one(&pool)
            .await
            .unwrap();
    r.on_problem(2, "meta").await.unwrap();
    assert_eq!(status(&pool, 2).await.0, "needs_owner");
    let decisions: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT item_id, status, decided_at FROM inbox_decisions WHERE item_id LIKE 'cron:%'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        decisions,
        vec![("cron:job:job".to_owned(), "open".to_owned(), opened)]
    );
    // Each escalation still tells the owner how to approve it.
    assert_eq!(fake.outcomes.lock().unwrap().len(), 2);
}

const LEAKED: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// 2026-10-29: the same freemoa login failure, first plain and then wrapped in a CDP timeout,
/// reached the owner twice. An unchanged ask is told once until a verified recovery ends it.
#[tokio::test]
async fn the_same_owner_ask_is_told_once_until_the_job_recovers() {
    let pool = crate::storage::memory().await.unwrap();
    sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES('collect', 'every 20m', '{\"command\":\"true\"}', 1)")
        .execute(&pool).await.unwrap();
    let hub = |pool: &SqlitePool| {
        let mut hub = HubRuntime::new(pool.clone(), Some("slack:DOWNER".into()));
        hub.transport = false;
        hub
    };
    let r = Remediator {
        pool: pool.clone(),
        judge: Judge::scripted(|purpose, _, _| match purpose {
            Purpose::CronOutcome => "completed".into(),
            _ => "needs_owner_login_or_quota".into(),
        }),
        runtime: Arc::new(hub(&pool)),
    };
    let fail = |id: &'static str, run: &'static str, cause: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query("INSERT INTO cron_problems(id, job_id, summary, created_at, status, kind, signature, detail, run_id, updated_at) VALUES(?, 'collect', ?, ?, 'open', 'failed', ?, ?, ?, ?)")
                .bind(id).bind(cause).bind(now_rfc3339()).bind(format!("{id}:{cause}")).bind(cause).bind(run).bind(now_rfc3339())
                .execute(&pool).await.unwrap();
        }
    };
    let decisions = |pool: SqlitePool| async move {
        sqlx::query_scalar::<_, String>("SELECT decision FROM report_suppressions ORDER BY id").fetch_all(&pool).await.unwrap()
    };
    fail("1", "run-1", "freemoa: LOGIN_FAILED").await;
    r.on_problem(1, "meta").await.unwrap();
    fail("2", "run-2", "freemoa: LOGIN_FAILED Error: CDP command timeout").await;
    r.on_problem(2, "meta").await.unwrap();
    assert_eq!(decisions(pool.clone()).await, ["sent", "duplicate"]);
    // A verified recovery ends the ask: the next login failure is news again.
    let fake = Arc::new(RecordingFake::default());
    fake.receipts.lock().unwrap().push(receipt("succeeded", "saved items: 3 (ids 11, 12, 13)"));
    let healer = Remediator { runtime: fake, ..remediator(&pool, "transient_retry_once", Arc::new(RecordingFake::default())) };
    fail("3", "run-3", "connection reset").await;
    healer.on_problem(3, "meta").await.unwrap();
    fail("4", "run-4", "freemoa: LOGIN_FAILED").await;
    r.on_problem(4, "meta").await.unwrap();
    assert_eq!(decisions(pool.clone()).await, ["sent", "duplicate", "sent"]);
}

/// 2026-10-28: the owner notice went through `HubRuntime::outcome` to the owner session (the
/// public home channel) with the full approval token, and was kept in `chat_outbox`.
#[tokio::test]
async fn an_owner_escalation_never_puts_a_secret_in_chat_or_storage() {
    let (pool, id) = seeded("job", json!({"command": "true"}), "HTTP 401 login expired").await;
    let r = Remediator {
        pool: pool.clone(),
        judge: Judge::forced("needs_owner_login_or_quota"),
        runtime: Arc::new(HubRuntime::new(pool.clone(), Some("slack:CHOME".into()))),
    };
    r.on_problem(id, "meta").await.unwrap();
    assert_eq!(status(&pool, id).await.0, "needs_owner");
    let outbox: Vec<String> = sqlx::query_scalar("SELECT text FROM chat_outbox")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(outbox.len(), 1, "{outbox:?}");
    assert!(
        outbox[0].ends_with(&format!("ferryx hub cron approve-rerun {id}")),
        "{outbox:?}"
    );
    let outcomes: Vec<String> = sqlx::query_scalar("SELECT message FROM cron_remediation_outcomes")
        .fetch_all(&pool)
        .await
        .unwrap();
    let detail: String = sqlx::query_scalar("SELECT detail FROM cron_problems")
        .fetch_one(&pool)
        .await
        .unwrap();
    for text in outbox
        .iter()
        .chain(&outcomes)
        .chain(std::iter::once(&detail))
    {
        assert!(!text.contains("--owner-token"), "{text}");
        assert!(!text.contains("approval_hash\":\""), "{text}");
        assert!(
            !text
                .split(|c: char| !c.is_ascii_hexdigit())
                .any(|word| word.len() >= 32),
            "a long hex secret is stored: {text}"
        );
    }
}

/// The owner is told once, in the owner DM: the needs-you notice carries the owner item's alert
/// obligation, so the owner alert sweep does not post the item again. A verified fix is
/// informational and goes to the home channel only.
#[tokio::test]
async fn needs_you_reaches_the_owner_dm_once_and_a_fix_only_the_home_channel() {
    let (pool, id) = seeded("job", json!({"command": "true"}), "HTTP 401 login expired").await;
    let runtime = HubRuntime {
        home_session: Some("home".into()),
        ..HubRuntime::new(pool.clone(), Some("owner-dm".into()))
    };
    let r = Remediator {
        pool: pool.clone(),
        judge: Judge::forced("needs_owner_login_or_quota"),
        runtime: Arc::new(runtime),
    };
    r.on_problem(id, "meta").await.unwrap();
    assert_eq!(
        crate::owner_alert::sweep(&pool, "owner-dm", chrono::Utc::now())
            .await
            .unwrap(),
        0,
        "the cron owner item is not alerted a second time"
    );
    let posted: Vec<String> = sqlx::query_scalar("SELECT session_key FROM chat_outbox")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(posted, vec!["owner-dm".to_owned()]);

    let fixed = HubRuntime {
        home_session: Some("home".into()),
        ..HubRuntime::new(pool.clone(), Some("owner-dm".into()))
    };
    let problem = problem(&pool, id).await.unwrap().unwrap();
    fixed
        .outcome(&problem, "job: cause -> fixed")
        .await
        .unwrap();
    let last: String =
        sqlx::query_scalar("SELECT session_key FROM chat_outbox ORDER BY id DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(last, "home");
}

#[tokio::test]
async fn leaked_tokens_are_redacted_from_stored_chat_and_their_hashes_revoked() {
    let (pool, id) = seeded("job", json!({"command": "true"}), "timeout").await;
    let leaked = format!(
        "job: needs you - x\nTo approve a rerun, run: omo-gateway cron approve-rerun {id} --owner-approved --owner-token {LEAKED}"
    );
    sqlx::query("INSERT INTO chat_outbox(session_key, platform, text, created_at, delivered_at) VALUES('slack:CHOME', 'slack', ?, ?, ?), ('slack:CHOME', 'slack', 'unrelated', ?, ?)")
        .bind(&leaked)
        .bind(now_rfc3339())
        .bind(now_rfc3339())
        .bind(now_rfc3339())
        .bind(now_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO messages(id, session_key, role, content) VALUES('m1', 'slack:CHOME', 'assistant', ?)")
        .bind(&leaked)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(redact_stored_owner_tokens(&pool).await.unwrap(), 2);
    assert_eq!(
        redact_stored_owner_tokens(&pool).await.unwrap(),
        0,
        "idempotent"
    );
    let texts: Vec<String> =
        sqlx::query_scalar("SELECT text FROM chat_outbox UNION ALL SELECT content FROM messages")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(texts.iter().all(|text| !text.contains(LEAKED)), "{texts:?}");
    assert!(texts.contains(&"unrelated".to_owned()));
    assert!(texts
        .iter()
        .any(|text| text.ends_with("--owner-token <redacted>")));

    // A hash minted before the fix is public now: migration 0020 clears it, so the leaked
    // token can no longer approve anything.
    let pre_fix = Detail {
        cause: "timeout".into(),
        approval_hash: Some(hash_owner_token(LEAKED)),
        ..Detail::default()
    };
    sqlx::query("UPDATE cron_problems SET status = 'needs_owner', detail = ?")
        .bind(serde_json::to_string(&pre_fix).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let r = remediator(&pool, "transient_retry_once", Arc::new(RecordingFake::default()));
    assert!(
        r.approve_rerun_with_token(id, LEAKED).await.is_ok(),
        "precondition: the leaked token works before the migration"
    );
    sqlx::query("UPDATE cron_problems SET status = 'needs_owner', detail = ?")
        .bind(serde_json::to_string(&pre_fix).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/0021_revoke_leaked_owner_tokens.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    assert!(status(&pool, id).await.1.approval_hash.is_none());
    assert_eq!(status(&pool, id).await.1.cause, "timeout");
    assert!(r.approve_rerun_with_token(id, LEAKED).await.is_err());
}

#[tokio::test]
async fn a_delivered_digest_stops_repeating_its_cron_outcomes() {
    let pool = crate::storage::memory().await.unwrap();
    queue_outcome(&pool, 1, "job", "job: old needs you").await.unwrap();
    let digest = crate::inbox::DIGEST_JOB_ID;
    sqlx::query("INSERT INTO cron_runs(id, job_id, status, started_at, finished_at, output) VALUES('d1', ?, 'succeeded', ?, ?, 'digest')")
        .bind(digest)
        .bind(now_rfc3339())
        .bind(now_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO cron_remediation_outcomes(problem_id, job_id, message, created_at) VALUES(2, 'job', 'job: after the digest', '2999-01-01T00:00:00Z')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json) VALUES(?, '0 9 * * *', '{}')")
        .bind(digest)
        .execute(&pool)
        .await
        .unwrap();

    crate::schedule::remember_delivery(&pool, digest, Some("slack down")).await.unwrap();
    let pending = || async {
        sqlx::query_scalar::<_, String>("SELECT message FROM cron_remediation_outcomes WHERE delivered_at IS NULL ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap()
    };
    assert_eq!(pending().await.len(), 2, "a failed delivery acknowledges nothing");

    crate::schedule::remember_delivery(&pool, digest, None).await.unwrap();
    assert_eq!(pending().await, vec!["job: after the digest".to_owned()]);
}

/// 2026-10-30: a rerun that exited 0 and printed nothing was reported to the owner as fixed.
#[tokio::test]
async fn exit_zero_without_output_is_unverified() {
    let empty = receipt("succeeded", "");
    assert_eq!(
        assess("job", &empty, &json!({})).unwrap().label(),
        "unverified"
    );
    assert!(!verified(&empty, &json!({})).unwrap());
    let silent = receipt("succeeded", "[SILENT]");
    assert_eq!(
        assess("job", &silent, &json!({})).unwrap().label(),
        "unverified"
    );
    let marked = receipt("succeeded", "Traceback (most recent call last):\n  KeyError 'x'");
    assert_eq!(assess("job", &marked, &json!({})).unwrap().label(), "failed");
    let published = json!({"external_effect": true});
    assert_eq!(
        assess("post", &receipt("succeeded", "done, posted"), &published)
            .unwrap()
            .label(),
        "unverified"
    );
    assert_eq!(
        assess("post", &receipt("succeeded", "posted id: 18231"), &published)
            .unwrap()
            .label(),
        "verified"
    );
    assert!(outcome_message("job", &empty, &json!({})).is_err());

    // Through the remediator: the problem goes to the owner, never to `fixed`.
    let (pool, id) = seeded("job", json!({"command": "true"}), "connection reset").await;
    let fake = Arc::new(RecordingFake::default());
    fake.receipts.lock().unwrap().push(receipt("succeeded", ""));
    let r = remediator(&pool, "transient_retry_once", fake.clone());
    r.on_problem(id, "meta").await.unwrap();
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "needs_owner");
    assert_eq!(detail.outcome.as_deref(), Some("unverified"));
    let message = fake.outcomes.lock().unwrap().last().cloned().unwrap();
    assert!(
        message.starts_with("job: 재실행은 끝났지만 결과를 확인하지 못했습니다: 종료 코드는 0이지만"),
        "{message}"
    );
}

#[tokio::test]
async fn skipped_or_unassessable_output_never_closes_the_problem() {
    for verdict in ["unverified", "failed", "unavailable"] {
        let (pool, id) = seeded("collect", json!({"command":"true"}), "connection reset").await;
        let fake = Arc::new(RecordingFake::default());
        fake.receipts.lock().unwrap().push(receipt("succeeded", "프리모아 로그인이 필요해 수집을 건너뜁니다. Google 로그인을 해 주세요."));
        let r = Remediator {
            pool: pool.clone(), runtime: fake,
            judge: Judge::scripted(move |purpose, _, _| match purpose {
                Purpose::CronCause => "transient_retry_once".into(),
                Purpose::CronOutcome => verdict.into(),
                _ => panic!("unexpected judgment"),
            }),
        };
        r.on_problem(id, "meta").await.unwrap();
        let (state, detail) = status(&pool, id).await;
        assert_eq!(state, "needs_owner");
        assert_ne!(detail.outcome.as_deref(), Some("verified"));
    }
    assert_eq!(Purpose::CronOutcome.capped_choice(), "unverified");
    assert_eq!(Purpose::CronOutcome.conservative_choice(), "unverified");
}

#[tokio::test]
async fn unavailable_cause_judgment_neither_retries_nor_claims_login_failure() {
    let (pool, id) = seeded("collect", json!({"command":"true"}), "TimeoutError: The operation timed out").await;
    let fake = Arc::new(RecordingFake::default());
    let r = Remediator {
        pool: pool.clone(), runtime: fake.clone(),
        judge: Judge::bare().with_jev_enabled(false),
    };
    r.on_problem(id, "meta").await.unwrap();
    assert_eq!(status(&pool, id).await.0, "needs_owner");
    assert!(fake.reruns.lock().unwrap().is_empty());
    assert!(fake.lanes.lock().unwrap().is_empty());
    assert_eq!(fake.outcomes.lock().unwrap().len(), 1);
    assert_eq!(Purpose::CronCause.capped_choice(), "unknown");
    assert_eq!(Purpose::CronCause.conservative_choice(), "unknown");
}

#[test]
fn owner_text_is_korean() {
    let texts = [
        Outcome::Verified("sent 3".into()).owner_text("job"),
        Outcome::Unverified("출력이 비어 있습니다".into()).owner_text("job"),
        Outcome::NeedsOwner("로그인을 복구".into()).owner_text("job"),
        Outcome::Failed("timeout".into()).owner_text("job"),
        approval_instruction(7),
    ];
    assert_eq!(texts[0], "job: 재실행했고 결과를 확인했습니다: sent 3");
    assert_eq!(
        texts[1],
        "job: 재실행은 끝났지만 결과를 확인하지 못했습니다: 출력이 비어 있습니다"
    );
    assert_eq!(texts[2], "job: 로그인을 복구해 주셔야 다시 돌 수 있습니다");
    assert_eq!(texts[3], "job: 재실행도 실패했습니다: timeout");
    assert!(texts[4].ends_with("ferryx hub cron approve-rerun 7"));
    for text in &texts {
        assert!(
            text.chars().any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c)),
            "not Korean: {text}"
        );
        assert!(!text.contains("needs you"), "{text}");
        assert!(!text.contains("To approve"), "{text}");
    }
}

fn missing_dir() -> (PathBuf, crate::cron_exec::EnvironmentFault) {
    let root = std::env::temp_dir().join(format!("fx-env-{}", uuid::Uuid::new_v4().simple()));
    let fault = crate::cron_exec::EnvironmentFault {
        missing: crate::cron_exec::MissingKind::WorkingDirectory,
        path: root.to_string_lossy().into_owned(),
    };
    (root, fault)
}

async fn env_jobs(pool: &SqlitePool, fault: &crate::cron_exec::EnvironmentFault, jobs: &[&str]) {
    for job in jobs {
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES(?, 'every 1h', '{\"command\":\"true\"}', 1)")
            .bind(job).execute(pool).await.unwrap();
        crate::schedule::note_run_health(pool, job, &format!("run-{job}"), "failed", "", Some(fault.message().as_str()), &json!({}))
            .await
            .unwrap();
    }
}

fn env_remediator(pool: &SqlitePool) -> Remediator {
    let mut hub = HubRuntime::new(pool.clone(), Some("slack:DOWNER".into()));
    hub.home_session = Some("slack:CHOME".into());
    hub.transport = false;
    Remediator {
        pool: pool.clone(),
        // A deterministic environment fault never reaches the judge.
        judge: Judge::scripted(|purpose, _, _| panic!("unexpected judgment {purpose:?}")),
        runtime: Arc::new(hub),
    }
}

/// 2026-11-01 09:06-09:30: the cron working directory was deleted and four jobs each sent the
/// owner an English "restore the job's login/credentials" approve-rerun notice. One missing path
/// is one notice naming the path and the jobs, with no approval command and no login guess.
#[tokio::test]
async fn one_missing_path_is_one_owner_notice_for_every_job_it_fails() {
    let pool = crate::storage::memory().await.unwrap();
    let (root, fault) = missing_dir();
    env_jobs(&pool, &fault, &["calendar-alerts", "collect", "playstore"]).await;
    let ids: Vec<i64> = sqlx::query_scalar("SELECT CAST(id AS INTEGER) FROM cron_problems ORDER BY CAST(id AS INTEGER)")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(ids.len(), 3);
    let r = env_remediator(&pool);
    for id in &ids {
        r.on_problem(*id, "meta").await.unwrap();
    }
    let states: Vec<String> = sqlx::query_scalar("SELECT status FROM cron_problems ORDER BY CAST(id AS INTEGER)")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(states, ["needs_owner"; 3]);
    let detail = Detail::parse(&problem(&pool, ids[0]).await.unwrap().unwrap().detail);
    assert_eq!(detail.environment.as_deref(), Some(fault.message().as_str()));
    assert_eq!(detail.reruns, 0, "no rerun can help while the path is missing");
    let outbox: Vec<(String, String)> = sqlx::query_as("SELECT session_key, text FROM chat_outbox")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(outbox.len(), 1, "{outbox:?}");
    let notice = &outbox[0].1;
    assert_eq!(outbox[0].0, "slack:DOWNER");
    assert!(notice.contains(&fault.path) || notice.contains('~'), "{notice}");
    for job in ["calendar-alerts", "collect", "playstore"] {
        assert!(notice.contains(job), "{notice}");
    }
    assert!(notice.contains("로그인이나 결제 문제가 아니라"), "{notice}");
    assert!(!notice.contains("approve-rerun"), "{notice}");
    let items: Vec<String> = sqlx::query_scalar("SELECT item_id FROM inbox_decisions WHERE status = 'open'")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(items, [environment_item_id(&fault)]);
    let incident: String = sqlx::query_scalar("SELECT incident FROM report_suppressions WHERE decision = 'sent'")
        .fetch_one(&pool).await.unwrap();
    assert!(incident.starts_with(&format!("env:{}@", fault.path)), "{incident}");

    // The path is back: the next clean run closes the incident with one recovery result, and
    // the other jobs' clean runs add nothing.
    std::fs::create_dir_all(&root).unwrap();
    sqlx::query("INSERT INTO hub_meta(key, value) VALUES('slack_home_channel', 'CHOME')")
        .execute(&pool).await.unwrap();
    for job in ["collect", "playstore", "calendar-alerts"] {
        crate::schedule::note_run_health(&pool, job, &format!("ok-{job}"), "succeeded", "done 1", None, &json!({}))
            .await
            .unwrap();
    }
    let texts: Vec<String> = sqlx::query_scalar("SELECT text FROM chat_outbox ORDER BY id")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(texts[1].contains("다시 생겨 크론이 정상으로 실행됩니다"), "{texts:?}");
    for job in ["calendar-alerts", "collect", "playstore"] {
        assert!(texts[1].contains(job), "{texts:?}");
    }
    let open: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inbox_decisions WHERE status = 'open'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(open, 0);
    let states: Vec<String> = sqlx::query_scalar("SELECT status FROM cron_problems")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(states, ["fixed"; 3]);
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn an_english_owner_reads_the_environment_notice_in_english() {
    crate::locale::scope(crate::locale::OwnerLocale::english_utc(), async {
        let pool = crate::storage::memory().await.unwrap();
        let (_, fault) = missing_dir();
        env_jobs(&pool, &fault, &["alerts", "collect"]).await;
        let r = env_remediator(&pool);
        for id in [1, 2] {
            r.on_problem(id, "meta").await.unwrap();
        }
        let texts: Vec<String> = sqlx::query_scalar("SELECT text FROM chat_outbox")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(texts.len(), 1, "{texts:?}");
        assert!(texts[0].starts_with("The cron working directory `"), "{texts:?}");
        assert!(texts[0].contains("is missing, so 2 cron job(s) could not run: alerts, collect"), "{texts:?}");
        assert!(texts[0].contains("not a login or billing problem"), "{texts:?}");
        assert!(!texts[0].chars().any(crate::locale::is_hangul), "{texts:?}");
    })
    .await;
}

/// The path came back before remediation looked: one verified rerun settles the job, no judge.
#[tokio::test]
async fn an_environment_fault_whose_path_is_back_gets_one_verified_rerun() {
    let (root, fault) = missing_dir();
    std::fs::create_dir_all(&root).unwrap();
    let (pool, id) = seeded("job", json!({"command": "true"}), &fault.message()).await;
    let fake = Arc::new(RecordingFake::default());
    fake.receipts.lock().unwrap().push(receipt("succeeded", "sent 2"));
    let r = Remediator {
        pool: pool.clone(),
        judge: Judge::scripted(|purpose, _, _| match purpose {
            Purpose::CronOutcome => "completed".into(),
            other => panic!("unexpected judgment {other:?}"),
        }),
        runtime: fake.clone(),
    };
    r.on_problem(id, "meta").await.unwrap();
    assert_eq!(status(&pool, id).await.0, "fixed");
    assert_eq!(fake.reruns.lock().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

/// 2026-11-01 12:22: a failed rerun posted "needs you - Verification failed ... {receipt JSON
/// with ANSI escapes}". The notice says what failed and what happens next, without either.
#[tokio::test]
async fn a_failed_verification_tells_what_failed_without_the_receipt_or_ansi() {
    let (pool, id) = seeded("collect", json!({"command": "true"}), "connection reset").await;
    let fake = Arc::new(RecordingFake::default());
    fake.receipts.lock().unwrap().push(RunReceipt {
        job_id: "collect".into(),
        run_id: "932f5e3c-4ba1-4cfd-8a21-aa6f3526fe75".into(),
        status: "failed".into(),
        output: None,
        last_delivery_error: None,
        error: Some("cron script failed with Some(1): kmong: \u{1b}[31mError: at repl.js:7:56\u{1b}[0m".into()),
    });
    let r = remediator(&pool, "transient_retry_once", fake.clone());
    r.on_problem(id, "meta").await.unwrap();
    assert_eq!(status(&pool, id).await.0, "needs_owner");
    let message = fake.outcomes.lock().unwrap().last().cloned().unwrap();
    assert!(message.starts_with("collect: 재실행도 실패했습니다: cron script failed with Some(1): kmong: Error: at repl.js:7:56"), "{message}");
    assert!(message.contains("근거: 원래 오류: connection reset"), "{message}");
    assert!(message.contains("다음 실행이 성공하면 이 건은 자동으로 닫힙니다"), "{message}");
    assert!(!message.contains('\u{1b}'), "{message}");
    assert!(!message.contains("[31m"), "{message}");
    assert!(!message.contains('{'), "{message}");
    assert!(!message.contains("932f5e3c"), "{message}");
    // The receipt stays in the problem's evidence for the digest and a remediation lane.
    assert!(status(&pool, id).await.1.evidence.unwrap().contains("932f5e3c"));
    // The rerun ran as part of the failing run's incident.
    assert_eq!(
        crate::cron_exec::with_incident("r0", async { crate::cron_exec::exported_run() }).await.as_deref(),
        Some("r0")
    );

    assert_eq!(strip_ansi("\u{1b}[31mError\u{1b}[0m \u{1b}]0;title\u{7}ok"), "Error ok");
}

/// The collector's own report about a failing run (FERRYX_CRON_RUN_ID) and remediation's
/// notice about the same run are one incident: the owner is told once.
#[tokio::test]
async fn a_collector_report_and_the_remediation_notice_of_one_run_are_told_once() {
    let (pool, id) = seeded("collect", json!({"command": "true"}), "kmong failed").await;
    let collector = crate::report_gate::Report::new(
        "collector", "autopilot", "autopilot:collect-error",
        crate::report_gate::Salience::Action, "외주 오토파일럿 수집 오류",
    ).with_incident(Some("r0".into()));
    crate::report_gate::queue(&pool, "slack:DOWNER", &collector, false).await.unwrap();
    let problem = problem(&pool, id).await.unwrap().unwrap();
    let report = remediation_report(&problem, "collect: 재실행도 실패했습니다: boom", false);
    assert_eq!(report.incident.as_deref(), Some("r0"));
    let (decision, _) = crate::report_gate::queue_delivery(&pool, "slack:DOWNER", &report, false, "x", None)
        .await
        .unwrap();
    assert_eq!(decision, crate::report_gate::Decision::Duplicate);
}

#[test]
fn owner_text_follows_an_english_owner() {
    crate::locale::with(crate::locale::OwnerLocale::english_utc(), || {
        assert_eq!(Outcome::Verified("sent 3".into()).owner_text("job"), "job: reran it and confirmed the result: sent 3");
        assert_eq!(
            Outcome::NeedsOwner("approve another rerun".into()).owner_text("job"),
            "job: it can run again once you approve another rerun"
        );
        assert_eq!(approval_instruction(7), "To approve a rerun, run this on the hub computer: ferryx hub cron approve-rerun 7");
        let report = remediation_report(
            &Problem { id: 1, job_id: "job".into(), kind: "failed".into(), detail: "x".into(),
                status: "needs_owner".into(), lane_key: None, run_id: Some("r1".into()) },
            "job: it can run again once you approve another rerun",
            false,
        );
        assert!(!report.body.chars().any(crate::locale::is_hangul), "{}", report.body);
    });
}

/// 2026-11-12: `needs_owner` committed a bare 'pending' obligation before the gate answered
/// Duplicate, so the item was never posted and the alert sweep skipped it forever.
#[tokio::test]
async fn a_repeated_owner_notice_never_leaves_a_pending_obligation_without_its_outbox_row() {
    let (pool, id) = seeded("collect", json!({"command":"collect-projects"}), "LOGIN_FAILED").await;
    let r = Remediator {
        pool: pool.clone(), judge: Judge::forced("unused"),
        runtime: Arc::new(HubRuntime { transport: false, ..HubRuntime::new(pool.clone(), Some("owner-dm".into())) }),
    };
    let problem = problem(&pool, id).await.unwrap().unwrap();
    for _ in 0..2 {
        r.needs_owner(&problem, &Outcome::Unverified("수집 결과가 없습니다".into()), None).await.unwrap();
    }
    let obligation = format!("{}{}", crate::owner_alert::OBLIGATION_PREFIX, crate::schedule::cron_owner_item_id("collect"));
    let bare: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM delivery_obligations o WHERE o.id = ? AND o.state = 'pending'
           AND NOT EXISTS (SELECT 1 FROM report_delivery_parts p WHERE p.report_id = o.report_id)",
    ).bind(&obligation).fetch_one(&pool).await.unwrap();
    assert_eq!(bare, 0);
    let state: String = sqlx::query_scalar("SELECT state FROM delivery_obligations WHERE id = ?")
        .bind(&obligation).fetch_one(&pool).await.unwrap();
    assert_eq!(state, "pending");
}

/// Seeds a `remediating` problem whose current verification attempt is `current`.
async fn claimed(current: &str) -> (SqlitePool, i64) {
    let detail = Detail {
        cause: "timeout".into(),
        reruns: 1,
        attempt: Some(current.into()),
        ..Detail::default()
    };
    let (pool, id) = seeded("job", json!({"command": "true"}), &detail.encode().unwrap()).await;
    sqlx::query("UPDATE cron_problems SET status = 'remediating' WHERE CAST(id AS INTEGER) = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    (pool, id)
}

#[tokio::test]
async fn a_stale_verification_attempt_writes_no_row_and_keeps_the_status() {
    let (pool, id) = claimed("newer").await;
    let stale = Detail { cause: "timeout".into(), attempt: Some("older".into()), ..Detail::default() };
    let owned = record_attempt_outcome(&pool, id, "older", &stale.encode().unwrap(), "fixed")
        .await
        .unwrap();
    assert!(!owned);
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "remediating");
    assert_eq!(detail.attempt.as_deref(), Some("newer"));
}

#[tokio::test]
async fn the_current_verification_attempt_writes_its_outcome() {
    let (pool, id) = claimed("current").await;
    let next = Detail {
        cause: "timeout".into(),
        reruns: 1,
        attempt: Some("current".into()),
        outcome: Some("verified".into()),
        ..Detail::default()
    };
    let owned = record_attempt_outcome(&pool, id, "current", &next.encode().unwrap(), "fixed")
        .await
        .unwrap();
    assert!(owned);
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "fixed");
    assert_eq!(detail.outcome.as_deref(), Some("verified"));
    assert_eq!(detail.attempt, None);
}

async fn stored_detail(pool: &SqlitePool, id: i64) -> String {
    sqlx::query_scalar("SELECT detail FROM cron_problems WHERE CAST(id AS INTEGER) = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn next_attempt(token: &str) -> String {
    Detail { cause: "timeout".into(), reruns: 2, attempt: Some(token.into()), ..Detail::default() }
        .encode()
        .unwrap()
}

#[tokio::test]
async fn two_verifiers_reading_the_same_detail_spend_exactly_one_rerun() {
    let (pool, id) = claimed("previous").await;
    let prior = stored_detail(&pool, id).await;
    let first = spend_attempt(&pool, id, "remediating", &prior, &next_attempt("a")).await.unwrap();
    let second = spend_attempt(&pool, id, "remediating", &prior, &next_attempt("b")).await.unwrap();
    assert!(first);
    assert!(!second);
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "remediating");
    assert_eq!(detail.attempt.as_deref(), Some("a"));
    assert_eq!(detail.reruns, 2);
}

#[tokio::test]
async fn a_stale_verifier_cannot_overwrite_a_newer_attempts_detail() {
    let (pool, id) = claimed("previous").await;
    let stale_read = stored_detail(&pool, id).await;
    // A newer attempt claims the problem after the stale verifier read it.
    assert!(spend_attempt(&pool, id, "remediating", &stale_read, &next_attempt("newer")).await.unwrap());
    let newer = stored_detail(&pool, id).await;
    assert!(!spend_attempt(&pool, id, "remediating", &stale_read, &next_attempt("stale")).await.unwrap());
    assert_eq!(stored_detail(&pool, id).await, newer);
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "remediating");
    assert_eq!(detail.attempt.as_deref(), Some("newer"));
}

#[tokio::test]
async fn two_outcome_writes_with_the_same_token_record_once() {
    let (pool, id) = claimed("current").await;
    // The first write leaves a nonterminal status, so only the consumed token can refuse the second.
    let first = Detail { cause: "timeout".into(), attempt: Some("current".into()), outcome: Some("rerun".into()), ..Detail::default() };
    let failed = Detail { cause: "timeout".into(), attempt: Some("current".into()), outcome: Some("failed".into()), ..Detail::default() };
    assert!(record_attempt_outcome(&pool, id, "current", &first.encode().unwrap(), "remediating").await.unwrap());
    assert!(!record_attempt_outcome(&pool, id, "current", &failed.encode().unwrap(), "needs_owner").await.unwrap());
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "remediating");
    assert_eq!(detail.outcome.as_deref(), Some("rerun"));
    assert_eq!(detail.attempt, None);
}

#[tokio::test]
async fn an_escalation_that_lost_the_token_leaves_a_newer_attempt_untouched() {
    // Attempt A read the row, then an approval re-claimed it for attempt B.
    let (pool, id) = claimed("newer").await;
    let fake = Arc::new(RecordingFake::default());
    let r = remediator(&pool, "timeout", fake.clone());
    let problem = problem(&pool, id).await.unwrap().unwrap();
    let before = stored_detail(&pool, id).await;
    let stale = Detail { cause: "timeout".into(), evidence: Some("stale".into()), ..Detail::default() };
    let escalated = r
        .escalate(
            &problem,
            &Outcome::NeedsOwner("approve a retry".into()),
            Some("stale failure"),
            Some((&stale.encode().unwrap(), Some("older"))),
        )
        .await
        .unwrap();
    assert!(!escalated);
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "remediating");
    assert_eq!(detail.attempt.as_deref(), Some("newer"));
    assert_eq!(stored_detail(&pool, id).await, before);
    assert!(fake.outcomes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_current_attempts_failure_escalates_to_the_owner() {
    let (pool, id) = claimed("current").await;
    let fake = Arc::new(RecordingFake::default());
    let r = remediator(&pool, "timeout", fake.clone());
    let owned = r
        .record_failure(id, "verification", &HubError::message("rerun spawn failed"), Some("current"))
        .await
        .unwrap();
    assert!(owned);
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "needs_owner");
    assert_eq!(detail.attempt, None);
    assert!(detail.evidence.as_deref().unwrap().contains("rerun spawn failed"));
    assert_eq!(fake.outcomes.lock().unwrap().len(), 1);
}

/// A rerun that is overtaken by a newer attempt and then errors.
struct OvertakenThenFails {
    pool: SqlitePool,
    id: i64,
    fake: RecordingFake,
}

#[async_trait]
impl Runtime for OvertakenThenFails {
    async fn open_lane(&self, request: LaneRequest) -> Result<()> {
        self.fake.open_lane(request).await
    }
    async fn rerun(&self, _job: &str, _approved: bool) -> Result<RunReceipt> {
        let (_, mut detail) = status(&self.pool, self.id).await;
        detail.attempt = Some("newer".into());
        sqlx::query("UPDATE cron_problems SET detail = ? WHERE CAST(id AS INTEGER) = ?")
            .bind(detail.encode().unwrap())
            .bind(self.id)
            .execute(&self.pool)
            .await
            .unwrap();
        Err(HubError::message("rerun spawn failed"))
    }
    async fn quota_check(&self) -> Result<Value> {
        self.fake.quota_check().await
    }
    async fn log_excerpt(&self, job: &str) -> Result<String> {
        self.fake.log_excerpt(job).await
    }
    async fn outcome(&self, problem: &Problem, message: &str) -> Result<()> {
        self.fake.outcome(problem, message).await
    }
}

#[tokio::test]
async fn a_superseded_attempt_that_errors_leaves_the_row_untouched() {
    let (pool, id) = claimed("previous").await;
    let runtime = Arc::new(OvertakenThenFails { pool: pool.clone(), id, fake: RecordingFake::default() });
    let r = Remediator {
        pool: pool.clone(),
        judge: Judge::scripted(|_, _, _| "completed".into()),
        runtime: runtime.clone(),
    };
    let before = stored_detail(&pool, id).await;
    assert!(r.verify_once(id).await.unwrap().is_none());
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "remediating");
    assert_eq!(detail.attempt.as_deref(), Some("newer"));
    assert_eq!(detail.evidence, Detail::parse(&before).evidence);
    assert!(runtime.fake.outcomes.lock().unwrap().is_empty());
}

/// A rerun that finishes after a newer attempt claimed the problem must not close owner items.
struct Overtaken {
    pool: SqlitePool,
    id: i64,
    fake: RecordingFake,
}

#[async_trait]
impl Runtime for Overtaken {
    async fn open_lane(&self, request: LaneRequest) -> Result<()> {
        self.fake.open_lane(request).await
    }
    async fn rerun(&self, job: &str, approved: bool) -> Result<RunReceipt> {
        // A newer verifier claims the attempt while this rerun is still running.
        let (_, mut detail) = status(&self.pool, self.id).await;
        detail.attempt = Some("newer".into());
        sqlx::query("UPDATE cron_problems SET detail = ? WHERE CAST(id AS INTEGER) = ?")
            .bind(detail.encode().unwrap())
            .bind(self.id)
            .execute(&self.pool)
            .await
            .unwrap();
        self.fake.rerun(job, approved).await
    }
    async fn quota_check(&self) -> Result<Value> {
        self.fake.quota_check().await
    }
    async fn log_excerpt(&self, job: &str) -> Result<String> {
        self.fake.log_excerpt(job).await
    }
    async fn outcome(&self, problem: &Problem, message: &str) -> Result<()> {
        self.fake.outcome(problem, message).await
    }
}

#[tokio::test]
async fn a_superseded_verification_attempt_closes_no_owner_items() {
    let (pool, id) = seeded("job", json!({"command": "true"}), "connection reset").await;
    sqlx::query("UPDATE cron_problems SET status = 'remediating' WHERE CAST(id AS INTEGER) = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    for item in [format!("cron:{id}"), "cron:job:job".to_owned()] {
        sqlx::query("INSERT INTO inbox_items(id, source, source_ref, snippet, received_at, raw_hash, collected_at) VALUES(?, 'cron', ?, 's', ?, 'h', ?)")
            .bind(&item)
            .bind(&item)
            .bind(now_rfc3339())
            .bind(now_rfc3339())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO inbox_decisions(item_id, class, reason, proposed_action, status, decided_at) VALUES(?, 'owner', 'r', 'a', 'open', ?)")
            .bind(item)
            .bind(now_rfc3339())
            .execute(&pool)
            .await
            .unwrap();
    }
    let runtime = Arc::new(Overtaken { pool: pool.clone(), id, fake: RecordingFake::default() });
    let r = Remediator {
        pool: pool.clone(),
        judge: Judge::scripted(|_, _, _| "completed".into()),
        runtime: runtime.clone(),
    };
    assert!(r.verify_once(id).await.unwrap().is_none());
    let (state, detail) = status(&pool, id).await;
    assert_eq!(state, "remediating");
    assert_eq!(detail.attempt.as_deref(), Some("newer"));
    let open: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inbox_decisions WHERE status = 'open'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(open, 2);
    assert!(runtime.fake.outcomes.lock().unwrap().is_empty());
}

/// Deliberately gate-less runtime for the rejection contract in the parent test module.
#[derive(Default)]
struct Fake {
    outcomes: Mutex<Vec<String>>,
}

#[async_trait]
impl Runtime for Fake {
    async fn open_lane(&self, _: LaneRequest) -> Result<()> {
        Err(HubError::message("unexpected lane request"))
    }
    async fn rerun(&self, _: &str, _: bool) -> Result<RunReceipt> {
        Err(HubError::message("unexpected rerun"))
    }
    async fn quota_check(&self) -> Result<Value> {
        Err(HubError::message("unexpected quota check"))
    }
    async fn log_excerpt(&self, _: &str) -> Result<String> {
        Err(HubError::message("unexpected log read"))
    }
    async fn outcome(&self, _: &Problem, message: &str) -> Result<()> {
        self.outcomes.lock().unwrap().push(message.to_owned());
        Ok(())
    }
}
