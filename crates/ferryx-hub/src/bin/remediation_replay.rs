//! Replays stored rerun output through the real remediator and real Jev in an isolated DB.
use std::sync::Arc;
use async_trait::async_trait;
use ferryx_hub::{cron_remediate::{LaneRequest, Problem, Remediator, Runtime}, error::{HubError, Result}, judge::Judge, schedule::RunReceipt};
use serde_json::{json, Value};

struct ReplayRuntime { output: &'static str }

#[async_trait]
impl Runtime for ReplayRuntime {
    async fn open_lane(&self, _: LaneRequest) -> Result<()> {
        Err(HubError::message("replay must not open a lane"))
    }
    async fn rerun(&self, job: &str, _: bool) -> Result<RunReceipt> {
        Ok(RunReceipt { job_id: job.into(), run_id: "fixture-rerun".into(), status: "succeeded".into(),
            output: Some(self.output.into()), error: None, last_delivery_error: None })
    }
    async fn quota_check(&self) -> Result<Value> { Err(HubError::message("unexpected quota check")) }
    async fn log_excerpt(&self, _: &str) -> Result<String> { Err(HubError::message("unexpected log read")) }
    async fn outcome(&self, _: &Problem, message: &str) -> Result<()> {
        println!("{}", json!({"notice":message}));
        Ok(())
    }
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    for (case, output, expected, cause) in [
        ("audit47-login-skipped", "프리모아 로그인이 필요해 수집을 건너뜁니다. Aside(u0)에서 프리모아 Google 로그인(패스키 확인 포함)을 해 주세요.", "needs_owner", None),
        ("audit64-empty", "", "needs_owner", None),
        ("bare-ok", "ok", "needs_owner", None),
        ("collection-completed", "수집 완료: 새 공고 3건을 저장했습니다. item_ids: project-101, project-102, project-103. 실패 0건.", "fixed", None),
        ("network-timeout", "수집 완료: 새 공고 3건을 저장했습니다. item_ids: project-101, project-102, project-103. 실패 0건.", "fixed", Some(("error: fetch failed after 5 attempts: TimeoutError: The operation timed out. at page (kakao-archive.ts:61:35)", "transient_retry_once"))),
        ("explicit-login-rejected", "", "needs_owner", Some(("Authentication rejected: HTTP 401 invalid_grant; refresh token expired; sign in again", "needs_owner_login_or_quota"))),
    ] {
        let pool = ferryx_hub::storage::memory().await?;
        sqlx::query("INSERT INTO scheduled_jobs(id,expression,payload_json,enabled) VALUES('collect','every 1h',?,1)")
            .bind(json!({"command":"collect-projects","description":"Collect new project listings and persist them"}).to_string()).execute(&pool).await?;
        sqlx::query("INSERT INTO cron_problems(id,job_id,summary,created_at,status,kind,signature,detail) VALUES(1,'collect','collection failed','2026-10-30T00:00:00Z','remediating','failed','fixture','connection reset')")
            .execute(&pool).await?;
        let remediator = Remediator { pool: pool.clone(),
            judge: Judge::from_config(ferryx_hub::judge::typesafe_from_env()).with_pool(pool.clone()),
            runtime: Arc::new(ReplayRuntime { output }) };
        if let Some((detail, expected_cause)) = cause {
            sqlx::query("UPDATE cron_problems SET status='open',detail=? WHERE id=1").bind(detail).execute(&pool).await?;
            remediator.on_problem(1, "meta").await?;
            let causes: Vec<(String, bool)> = sqlx::query_as("SELECT outcome,used_fallback FROM judge_calls WHERE purpose='cron_cause'").fetch_all(&pool).await?;
            println!("{}", json!({"case":case,"causes":causes}));
            if causes != vec![(expected_cause.to_owned(), false)] { return Err(format!("{case}: incorrect or unavailable cause judgment").into()); }
        } else {
            remediator.verify_once(1).await?.ok_or("rerun was not executed")?;
        }
        let status: String = sqlx::query_scalar("SELECT status FROM cron_problems WHERE id=1").fetch_one(&pool).await?;
        let calls: Vec<(String, bool)> = sqlx::query_as("SELECT outcome,used_fallback FROM judge_calls WHERE purpose='cron_outcome'").fetch_all(&pool).await?;
        println!("{}", json!({"case":case,"status":status,"expected":expected,"calls":calls}));
        if status != expected { return Err(format!("{case}: incorrect recovery state").into()); }
        if !output.is_empty() && (calls.len() != 1 || calls.iter().any(|(_, fallback)| *fallback)) {
            return Err(format!("{case}: actual nonfallback Jev decision missing").into());
        }
        pool.close().await;
    }
    Ok(())
}
