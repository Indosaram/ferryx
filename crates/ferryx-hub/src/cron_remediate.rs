//! omo-gateway's cron failure triage and auto-fix (`cron/remediate.rs`).
//!
//! Each new cron problem queues a `cron_problem` lane event. The dispatcher hands it here:
//! the judge classifies the root cause, then
//! - a transient failure of a job with no external effect gets one verified rerun;
//! - a script bug (or any publishing job) opens a remediation lane with a brief, and the
//!   lane settles only when a gateway-owned rerun verifies (publishing waits for the owner);
//! - login, quota, or payment trouble goes to the owner.
//!
//! The owner gets a one-time token. Only its sha256 is stored, and an approval spends it.
use crate::storage::WriteTx;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::judge::{scrub_for_external as scrub, Judge, Purpose};
use crate::lanes::signals::SignalResult;
use crate::schedule::{publishing, RunReceipt};
use crate::storage::now_rfc3339;

pub const MAX_RERUNS: u64 = 3;
pub const STALE_REMEDIATING_SECS: i64 = 3600;
pub const CAUSES: [&str; 3] = [
    "transient_retry_once",
    "script_bug_needs_fix",
    "needs_owner_login_or_quota",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub id: i64,
    pub job_id: String,
    pub kind: String,
    pub detail: String,
    pub status: String,
    pub lane_key: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    pub cause: String,
    #[serde(default)]
    pub reruns: u64,
    #[serde(default)]
    pub owner_approved: bool,
    #[serde(default)]
    pub last_run_id: Option<String>,
    #[serde(default)]
    pub evidence: Option<String>,
    /// sha256 (hex) of the one-time owner token. The token itself is only in the owner's message.
    #[serde(default)]
    pub approval_hash: Option<String>,
}

impl Detail {
    /// A plain-text detail (how the scheduler records a problem) is the cause.
    pub fn parse(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_else(|_| Self {
            cause: text.into(),
            ..Self::default()
        })
    }

    fn encode(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|error| HubError::message(error.to_string()))
    }
}

fn mint_owner_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub fn hash_owner_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(token.trim().as_bytes()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// omo's `RunReceipt::verified`: success, delivered, and the expected output when one is set.
pub fn verified(receipt: &RunReceipt, payload: &Value) -> Result<bool> {
    if receipt.status != "succeeded" || receipt.last_delivery_error.is_some() {
        return Ok(false);
    }
    if let Some(pattern) = payload
        .pointer("/expect/output_regex")
        .and_then(Value::as_str)
    {
        let regex =
            regex::Regex::new(pattern).map_err(|error| HubError::message(error.to_string()))?;
        return Ok(receipt
            .output
            .as_deref()
            .is_some_and(|output| regex.is_match(output)));
    }
    Ok(true)
}

pub fn outcome_message(job: &str, cause: &str, fix: &str, receipt: &RunReceipt) -> Result<String> {
    if receipt.status != "succeeded" || receipt.last_delivery_error.is_some() {
        return Err(HubError::message(
            "cannot report an unsuccessful or undelivered rerun as fixed",
        ));
    }
    Ok(format!(
        "{job}: {cause} -> {fix} (re-run {} succeeded, evidence: {})",
        receipt.run_id,
        receipt
            .output
            .as_deref()
            .unwrap_or("native run committed; last_delivery_error=null")
    ))
}

pub fn owner_message(job: &str, action: &str) -> String {
    format!("{job}: needs you - {action}")
}

/// The stored copy of an owner message never keeps the approval token.
pub fn redact_token(message: &str) -> String {
    match message.find("--owner-token ") {
        Some(at) => {
            let start = at + "--owner-token ".len();
            let end = message[start..]
                .find(char::is_whitespace)
                .map_or(message.len(), |offset| start + offset);
            format!("{}<redacted>{}", &message[..start], &message[end..])
        }
        None => message.to_owned(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneRequest {
    pub key: String,
    pub cwd: PathBuf,
    pub brief: String,
    pub done: String,
    pub lead: String,
}

#[async_trait]
pub trait Runtime: Send + Sync {
    async fn open_lane(&self, request: LaneRequest) -> Result<()>;
    async fn rerun(&self, job: &str, approved: bool) -> Result<RunReceipt>;
    async fn quota_check(&self) -> Result<Value>;
    async fn log_excerpt(&self, job: &str) -> Result<String>;
    async fn outcome(&self, problem: &Problem, message: &str) -> Result<()>;
}

pub struct Remediator {
    pub pool: SqlitePool,
    pub judge: Judge,
    pub runtime: Arc<dyn Runtime>,
}

static REMEDIATOR: OnceLock<Arc<Remediator>> = OnceLock::new();

pub fn install(remediator: Arc<Remediator>) -> Result<()> {
    REMEDIATOR
        .set(remediator)
        .map_err(|_| HubError::message("cron remediator already installed"))
}

pub fn installed() -> Option<&'static Arc<Remediator>> {
    REMEDIATOR.get()
}

/// Handles one `cron_problem` event. False when no remediator is installed.
pub async fn handle_event(body: &str, lead: &str) -> Result<bool> {
    let Some(remediator) = REMEDIATOR.get() else {
        return Ok(false);
    };
    remediator.handle_event(body, lead).await?;
    Ok(true)
}

/// The cron socket's `approve_problem`. Errors when no remediator is installed.
pub async fn approve_rerun(id: i64, owner_token: &str) -> Result<RunReceipt> {
    let remediator = REMEDIATOR
        .get()
        .ok_or_else(|| HubError::message("cron remediator not installed"))?;
    remediator
        .approve_rerun_with_token(id, owner_token)
        .await?
        .ok_or_else(|| HubError::message("cron rerun budget exhausted or already fixed"))
}

/// Lane settlement for a remediation lane: the gateway's own verified rerun is the signal.
/// None for a lane that is not a cron fix, and for a publishing fix the owner has not approved
/// (its `.ready` file signal applies).
pub async fn settle_lane(pool: &SqlitePool, key: &str) -> Result<Option<Vec<SignalResult>>> {
    let Some(problem) = problem_by_lane(pool, key).await? else {
        return Ok(None);
    };
    if problem.status == "fixed" {
        return Ok(Some(vec![SignalResult {
            signal: "cron natural success".into(),
            passed: true,
            detail: "committed clean success".into(),
        }]));
    }
    let Some(remediator) = REMEDIATOR.get() else {
        return Ok(None);
    };
    let payload = job_payload(pool, &problem.job_id).await?;
    if publishing(&problem.job_id, &payload) && !Detail::parse(&problem.detail).owner_approved {
        return Ok(None);
    }
    let receipt = remediator.verify_once(problem.id).await?;
    let encode = |receipt: &RunReceipt| serde_json::to_string(receipt).unwrap_or_default();
    Ok(Some(vec![match receipt {
        Some(receipt) => SignalResult {
            signal: "cron rerun".into(),
            passed: verified(&receipt, &payload)?,
            detail: encode(&receipt),
        },
        None => SignalResult {
            signal: "cron rerun".into(),
            passed: false,
            detail: "owner action required".into(),
        },
    }]))
}

const PROBLEM_COLUMNS: &str =
    "CAST(id AS INTEGER), job_id, kind, detail, status, lane_key FROM cron_problems";

type ProblemRow = (i64, String, String, String, String, Option<String>);

fn problem_from(row: ProblemRow) -> Problem {
    let (id, job_id, kind, detail, status, lane_key) = row;
    Problem {
        id,
        job_id,
        kind,
        detail,
        status,
        lane_key,
    }
}

pub async fn problem(pool: &SqlitePool, id: i64) -> Result<Option<Problem>> {
    let row: Option<ProblemRow> = sqlx::query_as(&format!(
        "SELECT {PROBLEM_COLUMNS} WHERE CAST(id AS INTEGER) = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(problem_from))
}

async fn problem_by_lane(pool: &SqlitePool, key: &str) -> Result<Option<Problem>> {
    let row: Option<ProblemRow> = sqlx::query_as(&format!(
        "SELECT {PROBLEM_COLUMNS} WHERE lane_key = ? ORDER BY CAST(id AS INTEGER) DESC LIMIT 1"
    ))
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(problem_from))
}

async fn job_row(pool: &SqlitePool, job: &str) -> Result<(String, Value)> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT expression, payload_json FROM scheduled_jobs WHERE id = ?")
            .bind(job)
            .fetch_optional(pool)
            .await?;
    let (expression, payload) =
        row.ok_or_else(|| HubError::message(format!("cron job {job} not found")))?;
    Ok((
        expression,
        serde_json::from_str(&payload).unwrap_or(Value::Null),
    ))
}

async fn job_payload(pool: &SqlitePool, job: &str) -> Result<Value> {
    Ok(job_row(pool, job).await?.1)
}

/// omo's lane key: `cron-fix-<slug>-<4 hex>`, within the 48-character lane key limit.
pub fn lane_key(job: &str) -> String {
    let slug: String = job
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(29)
        .collect();
    let suffix: String = uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(4)
        .collect();
    format!("cron-fix-{slug}-{suffix}")
}

fn update_stamp() -> String {
    now_rfc3339()
}

impl Remediator {
    pub async fn handle_event(&self, body: &str, lead: &str) -> Result<()> {
        let event: Value =
            serde_json::from_str(body).map_err(|error| HubError::message(error.to_string()))?;
        let id = event
            .get("problem_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| HubError::message("cron_problem lacks problem_id"))?;
        // Every event also sweeps rows stranded in `remediating` by an earlier crash.
        if let Err(error) = self.sweep_stale_remediating().await {
            tracing::warn!(%error, "stale cron remediation sweep failed");
        }
        self.on_problem(id, lead).await
    }

    pub async fn on_problem(&self, id: i64, lead: &str) -> Result<()> {
        let result = self.on_problem_inner(id, lead).await;
        if let Err(error) = &result {
            self.record_failure(id, "diagnosis", error).await?;
        }
        result
    }

    async fn record_failure(&self, id: i64, stage: &str, error: &HubError) -> Result<()> {
        let Some(mut problem) = problem(&self.pool, id).await? else {
            return Ok(());
        };
        if matches!(problem.status.as_str(), "fixed" | "needs_owner") {
            return Ok(());
        }
        let mut detail = Detail::parse(&problem.detail);
        detail.evidence = Some(
            json!({"status": "failed", "stage": stage, "error": error.to_string()}).to_string(),
        );
        problem.detail = detail.encode()?;
        sqlx::query("UPDATE cron_problems SET detail = ?, status = 'needs_owner', lane_key = CASE WHEN EXISTS(SELECT 1 FROM lanes WHERE key = cron_problems.lane_key) THEN lane_key ELSE NULL END, updated_at = ? WHERE CAST(id AS INTEGER) = ? AND status != 'fixed'")
            .bind(&problem.detail)
            .bind(update_stamp())
            .bind(id)
            .execute(&self.pool)
            .await?;
        problem.lane_key =
            sqlx::query_scalar("SELECT lane_key FROM cron_problems WHERE CAST(id AS INTEGER) = ?")
                .bind(id)
                .fetch_one(&self.pool)
                .await?;
        self.needs_owner(
            &problem,
            &format!("Remediation failed during {stage}: {error}; inspect recorded evidence before approving another attempt"),
        )
        .await
    }

    async fn on_problem_inner(&self, id: i64, lead: &str) -> Result<()> {
        let Some(mut problem) = problem(&self.pool, id).await? else {
            return Ok(());
        };
        if problem.status != "open" {
            return Ok(());
        }
        let (expression, payload) = job_row(&self.pool, &problem.job_id).await?;
        let output: Option<String> = sqlx::query_scalar(
            "SELECT output FROM cron_runs WHERE job_id = ? AND output != '' ORDER BY started_at DESC LIMIT 1",
        )
        .bind(&problem.job_id)
        .fetch_optional(&self.pool)
        .await?;
        let detail = Detail::parse(&problem.detail);
        let state = format!("{}\n{}", detail.cause, output.as_deref().unwrap_or(""));
        let choice = self
            .judge
            .choice(
                Purpose::CronCause,
                &scrub(&state),
                "Classify the root cause; OAuth/login/payment/quota needs owner; traceback needs script fix; transient may retry once",
                &CAUSES,
            )
            .await?;
        let publishes = publishing(&problem.job_id, &payload);
        let key = lane_key(&problem.job_id);
        let lane = (choice.selected == "script_bug_needs_fix" || publishes).then_some(&key);
        let claimed = sqlx::query("UPDATE cron_problems SET status = 'remediating', detail = ?, lane_key = ?, updated_at = ? WHERE CAST(id AS INTEGER) = ? AND status = 'open'")
            .bind(detail.encode()?)
            .bind(lane)
            .bind(update_stamp())
            .bind(id)
            .execute(&self.pool)
            .await?;
        if claimed.rows_affected() == 0 {
            return Ok(());
        }
        problem.detail = detail.encode()?;
        match choice.selected.as_str() {
            "needs_owner_login_or_quota" => {
                let action = if state.to_ascii_lowercase().contains("quota")
                    || state.contains("429")
                {
                    match self.runtime.quota_check().await {
                        Ok(quota) => format!("Inspect provider quota/payment or credentials; mahoquot direct result: {quota}; error: {}", detail.cause),
                        Err(error) => format!("Restore access to mahoquot 127.0.0.1:18801 before diagnosing quota; direct check failed: {error}"),
                    }
                } else {
                    format!(
                        "Restore the job's login/credentials/payment, then approve a rerun: {}",
                        detail.cause
                    )
                };
                self.needs_owner(&problem, &action).await?;
            }
            // `verify_once` already escalates to the owner when the rerun is unverified.
            "transient_retry_once" if !publishes => {
                self.verify_once(id).await?;
            }
            "script_bug_needs_fix" | "transient_retry_once" => {
                let runs: Vec<(String, Option<String>)> = sqlx::query_as(
                    "SELECT status, error FROM cron_runs WHERE job_id = ? ORDER BY started_at DESC LIMIT 3",
                )
                .bind(&problem.job_id)
                .fetch_all(&self.pool)
                .await?;
                let logs = self.runtime.log_excerpt(&problem.job_id).await?;
                let request = make_lane(
                    &problem,
                    &expression,
                    &payload,
                    &key,
                    lead,
                    &runs,
                    output.as_deref(),
                    &logs,
                )?;
                self.runtime.open_lane(request).await?;
                if publishes {
                    problem.lane_key = Some(key);
                    self.needs_owner(
                        &problem,
                        "Review the prepared fix and explicitly approve the publishing rerun",
                    )
                    .await?;
                }
            }
            other => return Err(HubError::message(format!("invalid cron cause: {other}"))),
        }
        Ok(())
    }

    /// Escalates to the owner: the job's one open owner inbox item (`cron:job:<job_id>`) and a
    /// message carrying a fresh one-time approval token.
    pub async fn needs_owner(&self, problem: &Problem, action: &str) -> Result<()> {
        let now = update_stamp();
        let action = scrub(action);
        let token = mint_owner_token();
        let mut tx = self.pool.begin_write().await?;
        let escalated = sqlx::query("UPDATE cron_problems SET status = 'needs_owner', updated_at = ? WHERE CAST(id AS INTEGER) = ? AND status != 'fixed'")
            .bind(&now)
            .bind(problem.id)
            .execute(&mut *tx)
            .await?;
        if escalated.rows_affected() == 0 {
            // Fixed concurrently: no inbox row, no outcome, no token that can match nothing.
            tx.rollback().await?;
            return Ok(());
        }
        let stored: String =
            sqlx::query_scalar("SELECT detail FROM cron_problems WHERE CAST(id AS INTEGER) = ?")
                .bind(problem.id)
                .fetch_one(&mut *tx)
                .await?;
        let mut detail = Detail::parse(&stored);
        detail.approval_hash = Some(hash_owner_token(&token));
        sqlx::query(
            "UPDATE cron_problems SET detail = ?, owner_token = NULL WHERE CAST(id AS INTEGER) = ?",
        )
        .bind(detail.encode()?)
        .bind(problem.id)
        .execute(&mut *tx)
        .await?;
        crate::schedule::upsert_cron_owner_item(
            &mut tx,
            &crate::schedule::CronOwnerItem {
                job_id: &problem.job_id,
                title: &owner_message(&problem.job_id, &action),
                action: &action,
                reason: &problem.kind,
                lane_key: problem.lane_key.as_deref(),
                at: &now,
            },
        )
        .await?;
        tx.commit().await?;
        self.runtime
            .outcome(
                problem,
                &format!(
                    "{}\nTo approve a rerun, run: omo-gateway cron approve-rerun {} --owner-approved --owner-token {token}",
                    owner_message(&problem.job_id, &action),
                    problem.id
                ),
            )
            .await
    }

    /// Escalates `remediating` rows no live lane or rerun can still settle.
    pub async fn sweep_stale_remediating(&self) -> Result<usize> {
        let cutoff =
            (chrono::Utc::now() - chrono::TimeDelta::seconds(STALE_REMEDIATING_SECS)).to_rfc3339();
        let rows: Vec<ProblemRow> = sqlx::query_as(&format!(
            "SELECT {PROBLEM_COLUMNS} WHERE status = 'remediating' AND julianday(updated_at) < julianday(?)
             AND (lane_key IS NULL OR NOT EXISTS (SELECT 1 FROM lanes WHERE lanes.key = cron_problems.lane_key AND lanes.status NOT IN ('verified_done','closed')))"
        ))
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?;
        let mut swept = 0;
        for problem in rows.into_iter().map(problem_from) {
            let action = format!(
                "Remediation stalled in 'remediating' (the hub likely restarted mid-rerun); inspect the job and approve another attempt: {}",
                Detail::parse(&problem.detail).cause
            );
            match self.needs_owner(&problem, &action).await {
                Ok(()) => swept += 1,
                Err(error) => {
                    tracing::warn!(%error, problem = problem.id, "stale cron problem escalation failed");
                }
            }
        }
        Ok(swept)
    }

    pub async fn verify_once(&self, id: i64) -> Result<Option<RunReceipt>> {
        let result = self.verify_once_inner(id).await;
        if let Err(error) = &result {
            self.record_failure(id, "verification", error).await?;
        }
        result
    }

    async fn verify_once_inner(&self, id: i64) -> Result<Option<RunReceipt>> {
        let Some(problem) = problem(&self.pool, id).await? else {
            return Ok(None);
        };
        if matches!(problem.status.as_str(), "fixed" | "needs_owner" | "ignored") {
            return Ok(None);
        }
        let mut detail = Detail::parse(&problem.detail);
        let payload = job_payload(&self.pool, &problem.job_id).await?;
        let publishes = publishing(&problem.job_id, &payload);
        if detail.reruns >= MAX_RERUNS || (publishes && !detail.owner_approved) {
            self.needs_owner(
                &problem,
                "Approve further remediation; rerun budget exhausted or publishing approval required",
            )
            .await?;
            return Ok(None);
        }
        detail.reruns = detail.reruns.saturating_add(1);
        // The budget is spent before the rerun, so a crash mid-run cannot rerun forever.
        let spent = sqlx::query("UPDATE cron_problems SET detail = ?, updated_at = ? WHERE CAST(id AS INTEGER) = ? AND status = ?")
            .bind(detail.encode()?)
            .bind(update_stamp())
            .bind(id)
            .bind(&problem.status)
            .execute(&self.pool)
            .await?;
        if spent.rows_affected() == 0 {
            return Ok(None);
        }
        let receipt = self
            .runtime
            .rerun(&problem.job_id, detail.owner_approved)
            .await?;
        detail.last_run_id = Some(receipt.run_id.clone());
        detail.evidence = Some(
            serde_json::to_string(&receipt)
                .map_err(|error| HubError::message(error.to_string()))?,
        );
        let ok = verified(&receipt, &payload)?;
        sqlx::query("UPDATE cron_problems SET detail = ?, status = ?, updated_at = ? WHERE CAST(id AS INTEGER) = ?")
            .bind(detail.encode()?)
            .bind(if ok { "fixed" } else { "needs_owner" })
            .bind(update_stamp())
            .bind(id)
            .execute(&self.pool)
            .await?;
        if ok {
            sqlx::query(
                "UPDATE inbox_decisions SET status = 'done' WHERE item_id = ? AND status = 'open'",
            )
            .bind(format!("cron:{id}"))
            .execute(&self.pool)
            .await?;
            crate::schedule::close_cron_owner_items(&self.pool, &problem.job_id).await?;
            self.runtime
                .outcome(
                    &problem,
                    &outcome_message(
                        &problem.job_id,
                        &detail.cause,
                        "verified native rerun",
                        &receipt,
                    )?,
                )
                .await?;
        } else {
            self.needs_owner(
                &problem,
                &format!(
                    "Verification failed; inspect recorded machine results before approving another attempt: {}",
                    detail.evidence.as_deref().unwrap_or("")
                ),
            )
            .await?;
        }
        Ok(Some(receipt))
    }

    /// Approval for untrusted callers: needs the one-time token `needs_owner` sent the owner.
    pub async fn approve_rerun_with_token(
        &self,
        id: i64,
        owner_token: &str,
    ) -> Result<Option<RunReceipt>> {
        let stored: Option<String> = sqlx::query_scalar(
            "SELECT detail FROM cron_problems WHERE CAST(id AS INTEGER) = ? AND status = 'needs_owner'",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let stored = stored
            .ok_or_else(|| HubError::message("cron problem is not awaiting owner approval"))?;
        let accepted = Detail::parse(&stored)
            .approval_hash
            .as_deref()
            .is_some_and(|hash| {
                constant_time_eq(hash.as_bytes(), hash_owner_token(owner_token).as_bytes())
            });
        if !accepted {
            return Err(HubError::message("owner approval token rejected"));
        }
        self.approve_rerun(id).await
    }

    pub async fn approve_rerun(&self, id: i64) -> Result<Option<RunReceipt>> {
        let text: String = sqlx::query_scalar(
            "SELECT detail FROM cron_problems WHERE CAST(id AS INTEGER) = ? AND status = 'needs_owner'",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        let mut detail = Detail::parse(&text);
        detail.owner_approved = true;
        // Approval after an exhausted budget grants exactly one more attempt.
        detail.reruns = detail.reruns.min(MAX_RERUNS.saturating_sub(1));
        detail.approval_hash = None;
        let taken = sqlx::query("UPDATE cron_problems SET detail = ?, status = 'remediating', updated_at = ? WHERE CAST(id AS INTEGER) = ? AND status = 'needs_owner'")
            .bind(detail.encode()?)
            .bind(update_stamp())
            .bind(id)
            .execute(&self.pool)
            .await?;
        if taken.rows_affected() == 0 {
            return Ok(None);
        }
        self.verify_once(id).await
    }
}

/// The remediation lane's brief and done signal (omo `make_lane`).
#[allow(clippy::too_many_arguments)]
pub fn make_lane(
    problem: &Problem,
    expression: &str,
    payload: &Value,
    key: &str,
    lead: &str,
    runs: &[(String, Option<String>)],
    output: Option<&str>,
    logs: &str,
) -> Result<LaneRequest> {
    let job = &problem.job_id;
    // The job's working directory, else the directory of the script's last absolute argument.
    let cwd = ["workdir", "cwd"]
        .iter()
        .find_map(|key| payload.get(*key).and_then(Value::as_str))
        .map(PathBuf::from)
        .or_else(|| {
            crate::schedule::script_from_payload(payload)
                .and_then(|script| {
                    let args: Vec<&Path> = script.split_whitespace().map(Path::new).collect();
                    if args.len() == 1 {
                        args[0].parent().map(Path::to_path_buf)
                    } else {
                        args.into_iter()
                            .rev()
                            .find(|arg| arg.is_absolute())
                            .and_then(Path::parent)
                            .map(Path::to_path_buf)
                    }
                })
                .filter(|path| !path.as_os_str().is_empty())
        })
        .unwrap_or_else(|| {
            crate::hub_env::var("OMON_WORKSPACE_ROOT")
                .ok()
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
        });
    // In the lane grammar (`lanes::signals`). Settlement replaces the command with the hub's
    // own verified rerun; the publishing `.ready` file is the lane's claim the fix is staged.
    let done = if publishing(job, payload) {
        format!("file \".omo/cron-fix/{key}.ready\"")
    } else {
        let argv = [
            "omo-gateway",
            "cron",
            "run-now",
            job.as_str(),
            "--wait",
            "--json",
        ];
        format!(
            "command {}",
            serde_json::to_string(&argv).map_err(|error| HubError::message(error.to_string()))?
        )
    };
    let runs = serde_json::to_string(runs).map_err(|error| HubError::message(error.to_string()))?;
    let output: String = output.unwrap_or("").chars().take(4000).collect();
    let logs: String = logs
        .lines()
        .take(80)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(4000)
        .collect();
    let script = payload
        .get("script")
        .or_else(|| payload.get("command"))
        .or_else(|| payload.get("prompt"))
        .unwrap_or(&Value::Null);
    let brief = format!(
        "Job: {job} / {}\nExpression: {expression}\nProblem: {} / {}\nLast 3 runs: {}\nLast output: {}\nScript or prompt: {script}\nGateway log (UTC):\n{}\nRules: no .env edits; no model changes; no job disable/delete; quota claims require direct mahoquot 127.0.0.1:18801 check; katok forbidden. Diagnose, fix the root cause in the script/config/repo, then claim done. Do not execute the Done command yourself: gateway settlement owns the single verification rerun and validates output regex and delivery receipt.\nDone = {done}\n",
        payload.get("name").and_then(Value::as_str).unwrap_or(job),
        problem.kind,
        scrub(&Detail::parse(&problem.detail).cause),
        scrub(&runs),
        scrub(&output),
        scrub(&logs),
    );
    Ok(LaneRequest {
        key: key.into(),
        cwd,
        brief,
        done,
        lead: lead.into(),
    })
}

/// The hub's runtime: lanes in the hub database, reruns through the native runner, outcomes in
/// the owner's Slack.
pub struct HubRuntime {
    pub pool: SqlitePool,
    pub briefs: PathBuf,
    pub owner_session: Option<String>,
    pub quota_url: String,
    pub log_paths: Vec<PathBuf>,
}

impl HubRuntime {
    pub fn new(pool: SqlitePool, owner_session: Option<String>) -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            pool,
            briefs: home.join(".ferryx/hub/cron-briefs"),
            owner_session,
            quota_url: "http://127.0.0.1:18801".into(),
            log_paths: vec![home.join(".ferryx/hub/hub.log")],
        }
    }
}

/// Upper bound on how much of a log a remediation brief reads.
pub const LOG_TAIL_BYTES: u64 = 4 * 1024 * 1024;

pub async fn read_log_tail(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut file = tokio::fs::File::open(path).await?;
    let len = file.metadata().await?.len();
    let truncated = len > max_bytes;
    if truncated {
        file.seek(std::io::SeekFrom::Start(len - max_bytes)).await?;
    }
    let mut bytes = Vec::new();
    file.take(max_bytes).read_to_end(&mut bytes).await?;
    if truncated {
        if let Some(newline) = bytes.iter().position(|byte| *byte == b'\n') {
            bytes.drain(..=newline);
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[async_trait]
impl Runtime for HubRuntime {
    async fn open_lane(&self, request: LaneRequest) -> Result<()> {
        tokio::fs::create_dir_all(&self.briefs).await?;
        let brief = self.briefs.join(format!("{}.md", request.key));
        tokio::fs::write(&brief, &request.brief).await?;
        crate::lanes::state::insert_lane(
            &self.pool,
            &request.key,
            "Cron root-cause remediation",
            &request.lead,
            &request.cwd.to_string_lossy(),
            &request.done,
        )
        .await?;
        sqlx::query("UPDATE lanes SET brief_path = ? WHERE key = ?")
            .bind(brief.to_string_lossy().as_ref())
            .bind(&request.key)
            .execute(&self.pool)
            .await?;
        // The lead hears about the new lane the way it hears about any lane question.
        crate::lanes::dispatch::enqueue(
            &self.pool,
            &request.key,
            &request.lead,
            "question",
            &json!({"lane": request.key, "brief": brief, "action": "cron remediation lane queued; drive it with lane.drive"}).to_string(),
        )
        .await?;
        Ok(())
    }

    async fn rerun(&self, job: &str, approved: bool) -> Result<RunReceipt> {
        Box::pin(crate::schedule::run_native_job(
            &self.pool, job, approved, false,
        ))
        .await
    }

    async fn quota_check(&self) -> Result<Value> {
        #[cfg(any(feature = "hub-slack", feature = "hub-voice"))]
        {
            let response = reqwest::Client::new()
                .get(&self.quota_url)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
                .map_err(|error| {
                    HubError::message(format!("mahoquot direct check failed: {error}"))
                })?;
            let status = response.status().as_u16();
            let body = response
                .text()
                .await
                .map_err(|error| HubError::message(error.to_string()))?;
            Ok(json!({"http_status": status, "body": body}))
        }
        #[cfg(not(any(feature = "hub-slack", feature = "hub-voice")))]
        {
            Err(HubError::message(format!(
                "mahoquot direct check needs an http client ({})",
                self.quota_url
            )))
        }
    }

    async fn log_excerpt(&self, job: &str) -> Result<String> {
        let run_ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM cron_runs WHERE job_id = ? ORDER BY started_at DESC LIMIT 3",
        )
        .bind(job)
        .fetch_all(&self.pool)
        .await?;
        let mut lines = Vec::new();
        for path in &self.log_paths {
            // A missing log must not abort remediation: the brief still has the runs.
            let Ok(log) = read_log_tail(path, LOG_TAIL_BYTES).await else {
                continue;
            };
            lines.extend(
                log.lines()
                    .filter(|line| line.contains(job) || run_ids.iter().any(|id| line.contains(id)))
                    .map(str::to_owned),
            );
        }
        if lines.is_empty() {
            return Ok("(gateway log not found; no excerpt)".into());
        }
        let skip = lines.len().saturating_sub(80);
        Ok(lines[skip..].join("\n"))
    }

    async fn outcome(&self, problem: &Problem, message: &str) -> Result<()> {
        queue_outcome(&self.pool, problem.id, &problem.job_id, message).await?;
        let Some(owner) = &self.owner_session else {
            return Ok(());
        };
        let message = scrub_keeping_token(message);
        let inserted = sqlx::query(
            "INSERT INTO chat_outbox(session_key, platform, text, created_at) VALUES(?, 'slack', ?, ?)",
        )
        .bind(owner)
        .bind(&message)
        .bind(now_rfc3339())
        .execute(&self.pool)
        .await?;
        let delivered = crate::chat::reply::deliver_stored(
            &self.pool,
            inserted.last_insert_rowid(),
            owner,
            &message,
        )
        .await
        .unwrap_or(false);
        if delivered {
            sqlx::query("UPDATE cron_remediation_outcomes SET delivered_at = ? WHERE problem_id = ? AND message = ?")
                .bind(now_rfc3339())
                .bind(problem.id)
                .bind(redact_token(&scrub(message.as_str())))
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
}

/// External text is scrubbed, but the owner's approval token has to survive it.
fn scrub_keeping_token(message: &str) -> String {
    match message.find("--owner-token ") {
        Some(at) => {
            let (head, tail) = message.split_at(at);
            format!("{}{tail}", scrub(head))
        }
        None => scrub(message),
    }
}

pub async fn queue_outcome(
    pool: &SqlitePool,
    problem: i64,
    job: &str,
    message: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO cron_remediation_outcomes(problem_id, job_id, message, created_at) VALUES(?,?,?,?) ON CONFLICT(problem_id, message) DO NOTHING")
        .bind(problem)
        .bind(job)
        .bind(redact_token(&scrub(&scrub_keeping_token(message))))
        .bind(now_rfc3339())
        .execute(pool)
        .await?;
    Ok(())
}

/// The digest's `## Cron` section (omo `report::digest_section`): problems touched in the last
/// day and outcomes the owner was not told about yet.
pub async fn digest_section(
    pool: &SqlitePool,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Option<String>> {
    let since = (now - chrono::TimeDelta::hours(24)).to_rfc3339();
    let problems: Vec<(String, String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT job_id, kind, status, lane_key, detail FROM cron_problems WHERE updated_at IS NOT NULL AND julianday(updated_at) >= julianday(?) AND julianday(updated_at) <= julianday(?) AND status != 'ignored' AND kind != '' ORDER BY job_id, CAST(id AS INTEGER)",
    )
    .bind(&since)
    .bind(now.to_rfc3339())
    .fetch_all(pool)
    .await?;
    let outcomes: Vec<String> = sqlx::query_scalar(
        "SELECT message FROM cron_remediation_outcomes WHERE delivered_at IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    if problems.is_empty() && outcomes.is_empty() {
        return Ok(None);
    }
    let mut lines = vec!["## Cron".to_owned()];
    for (job, kind, status, lane, detail) in problems {
        lines.push(format!(
            "- {job}: {kind} / {status} (lane: {}): {}",
            lane.as_deref().unwrap_or("none"),
            scrub(&Detail::parse(&detail).cause)
        ));
    }
    lines.extend(outcomes.into_iter().map(|message| format!("- {message}")));
    Ok(Some(lines.join("\n")))
}

pub async fn acknowledge_digest(pool: &SqlitePool) -> Result<()> {
    sqlx::query("UPDATE cron_remediation_outcomes SET delivered_at = ? WHERE delivered_at IS NULL")
        .bind(now_rfc3339())
        .execute(pool)
        .await?;
    Ok(())
}

/// A clean success closes the job's remediation lanes (omo `reconcile_fixed`).
pub async fn reconcile_fixed(pool: &SqlitePool, job: &str) -> Result<()> {
    let lanes: Vec<String> = sqlx::query_scalar(
        "SELECT lane_key FROM cron_problems WHERE job_id = ? AND status = 'fixed' AND lane_key IS NOT NULL",
    )
    .bind(job)
    .fetch_all(pool)
    .await?;
    for key in lanes {
        let from: Option<String> = sqlx::query_scalar(
            "SELECT status FROM lanes WHERE key = ? AND status NOT IN ('verified_done','closed')",
        )
        .bind(&key)
        .fetch_optional(pool)
        .await?;
        if let Some(from) = from {
            let now = now_rfc3339();
            let mut tx = pool.begin_write().await?;
            sqlx::query("UPDATE lanes SET status = 'verified_done', updated_at = ? WHERE key = ? AND status = ?")
                .bind(&now)
                .bind(&key)
                .bind(&from)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO lane_history(lane_key, at, from_status, to_status, reason) VALUES(?,?,?,'verified_done','cron natural clean success')")
                .bind(&key)
                .bind(&now)
                .bind(from)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
    }
    Ok(())
}

/// The dispatcher's cron branch (omo `lanes::dispatch` tick): undelivered `cron_problem`
/// events go to the remediator; one that fails stays queued for the next tick.
pub async fn dispatch_once(pool: &SqlitePool, remediator: &Remediator) -> Result<usize> {
    let events: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, lead_session_key, body FROM lane_events WHERE kind = 'cron_problem' AND delivered_at IS NULL AND (release_at IS NULL OR release_at <= ?) ORDER BY id",
    )
    .bind(now_rfc3339())
    .fetch_all(pool)
    .await?;
    let mut handled = 0;
    for (id, lead, body) in events {
        match Box::pin(remediator.handle_event(&body, &lead)).await {
            Ok(()) => {
                sqlx::query("UPDATE lane_events SET delivered_at = ? WHERE id = ?")
                    .bind(now_rfc3339())
                    .bind(id)
                    .execute(pool)
                    .await?;
                handled += 1;
            }
            Err(error) => {
                tracing::warn!(event_id = id, %error, "cron problem handling failed; event remains queued");
            }
        }
    }
    Ok(handled)
}

/// Installs the hub remediator and runs the dispatcher every 5 seconds.
pub async fn serve(pool: SqlitePool, owner_session: Option<String>) {
    match crate::schedule::reconcile_cron_owner_items(&pool).await {
        Ok(dismissed) => {
            tracing::info!(
                dismissed,
                "collapsed legacy cron owner items into one per job"
            );
        }
        Err(error) => {
            tracing::warn!(%error, "legacy cron owner items were not collapsed");
        }
    }
    let remediator = Arc::new(Remediator {
        pool: pool.clone(),
        judge: Judge::from_config(crate::judge::typesafe_from_env()),
        runtime: Arc::new(HubRuntime::new(pool.clone(), owner_session)),
    });
    if install(Arc::clone(&remediator)).is_err() {
        tracing::debug!("cron remediator was already installed");
    }
    let mut ticks = tokio::time::interval(std::time::Duration::from_secs(5));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        if let Err(error) = dispatch_once(&pool, &remediator).await {
            tracing::warn!(%error, "cron remediation tick failed; events remain queued");
        }
    }
}

#[cfg(test)]
mod tests;
