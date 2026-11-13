//! Drives a lane by launching a pane and reading it through agent control.
//! A `[CLAIM_DONE]` line claims the lane. A `[REPORT]` line does not.
use std::sync::Arc;

use serde_json::json;
use sqlx::SqlitePool;

use super::guard::{GuardThresholds, HostLoad};
use super::settle::settle;
use super::state::{self, set_claim, set_host, transition, LaneStatus};
use crate::agent_control::{AgentControl, AgentOp, TaskLauncher};
use crate::error::{HubError, Result};
use crate::judge::Judge;

/// Code of a refused drive of an owner-supervised legacy lane after a cutover stopped the
/// legacy intake.
pub const LEGACY_INTAKE_STOPPED: &str = "LEGACY_INTAKE_STOPPED";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveReport {
    pub session_id: String,
    pub status: LaneStatus,
}

pub struct LaneRunner {
    pub pool: SqlitePool,
    pub control: Arc<dyn AgentControl>,
    pub launcher: Arc<dyn TaskLauncher>,
    pub judge: Judge,
    pub load: HostLoad,
    pub thresholds: GuardThresholds,
}

impl LaneRunner {
    pub async fn drive(&self, key: &str) -> Result<DriveReport> {
        let row: Option<(String, String, String, String, Option<String>, String)> = sqlx::query_as(
            "SELECT status, cwd, title, done_signals, omo_thread_id, brief_path FROM lanes WHERE key = ?",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        let Some((status, cwd, title, done_signals, pane, brief_path)) = row else {
            return Err(HubError::message(format!("lane {key} not found")));
        };
        // omo's runner skips a paused lane: no launch, no nudge, no claim reading.
        if super::pause::is_paused(&self.pool, key).await? {
            return Err(HubError::message(format!("lane {key} is paused")));
        }
        // A lane under a legacy goal contract is the owner's legacy supervision; once a cutover
        // stopped the legacy intake only an adoption moves it. Lanes without one (cron
        // remediation, tracker issues) keep running whatever the intake.
        if supervised_by_legacy_goal(&self.pool, key).await?
            && !crate::supervisor::cutover::legacy_intake_runs(&self.pool).await?
        {
            return Err(HubError::message(format!(
                "{LEGACY_INTAKE_STOPPED}: lane {key} is under a legacy goal and the supervisor cutover stopped the legacy intake; adopt it to continue"
            )));
        }
        let mut session_id = pane.unwrap_or_default();
        // Resume settlement before reading the pane: the claim may have survived a crash
        // even when its pane (or the claim line in its history) did not.
        if status == "done_claimed" {
            let evidence: Option<String> = sqlx::query_scalar(
                "SELECT claim_evidence FROM lanes WHERE key = ?",
            )
            .bind(key)
            .fetch_one(&self.pool)
            .await?;
            if let Some(evidence) = evidence.filter(|value| !value.trim().is_empty()) {
                settle(&self.pool, key, &cwd, &done_signals, &evidence, &self.judge).await?;
            } else {
                transition(
                    &self.pool,
                    key,
                    LaneStatus::NotDone,
                    Some("interrupted claim has no evidence"),
                )
                .await?;
            }
            let status = state::status_of(&self.pool, key)
                .await?
                .ok_or_else(|| HubError::message(format!("lane {key} disappeared")))?;
            return Ok(DriveReport { session_id, status });
        }
        let mut cwd = cwd;
        let mut host = state::lane_host(&self.pool, key).await?;
        if status == "queued" {
            if self.load.admit(&self.thresholds) {
                // omo's auto worktree: a lane at a repository root gets its own checkout.
                if let Some(tree) = super::worktree::create(&self.pool, key, &cwd).await? {
                    cwd = tree;
                }
                session_id = self.launcher.launch_pane(&cwd, &[]).await?;
            } else {
                // This machine is full: hand the lane to the least busy paired host, in a new
                // worktree there. Signals that read local files or run commands keep it here.
                let target = if super::placement::portable(&done_signals) {
                    let hosts = self.launcher.remote_hosts().await?;
                    super::placement::idle_host(self.control.as_ref(), &hosts, self.thresholds.lanes_max).await
                } else {
                    None
                };
                let Some(target) = target else {
                    return Err(HubError::message(format!(
                        "guard refused to launch lane {key}"
                    )));
                };
                let remote = self.launcher.launch_remote_pane(&target, &cwd, key).await?;
                set_host(&self.pool, key, &target).await?;
                tracing::info!(lane = key, host = %target, "lane placed on a paired host");
                session_id = remote.session_id;
                cwd = remote.cwd;
                host = Some(target);
            }
            // The attach refuses a lane the supervisor adopted while this launch ran, in the
            // same statement, so the legacy path never starts a second worker on adopted work.
            if !crate::supervisor::attach_legacy_pane(&self.pool, key, &session_id).await? {
                // The launcher has no close operation; nothing was sent to the pane.
                tracing::warn!(
                    lane = key,
                    session_id = %session_id,
                    "lane was adopted by the supervisor during its legacy launch; the launched pane is left unused"
                );
                return Err(HubError::message(format!(
                    "{}: lane {key} was adopted by the supervisor during its legacy launch; the hub dispatcher drives it. The launched pane {session_id} was left unused and can be closed",
                    crate::supervisor::SUPERVISOR_OWNED
                )));
            }
            transition(&self.pool, key, LaneStatus::Working, Some("launched")).await?;
            // A lane with a brief file (cron remediation) sends the brief with the title.
            self.send_brief(key, host.as_deref(), &session_id, &title, &done_signals, &brief_path)
                .await?;
        } else if host.is_none() {
            // A reopened lane gets its removed worktree back before the pane is read.
            super::worktree::restore(&self.pool, key).await?;
        }
        // A re-driven lane whose brief was never acknowledged (a lost or ambiguous send), or
        // whose brief changed since, gets the brief again.
        if matches!(status.as_str(), "working" | "blocked") && !session_id.is_empty() && !brief_path.trim().is_empty() {
            let ack: (Option<String>, Option<String>) = sqlx::query_as(
                "SELECT brief_ack_path, brief_ack_sha256 FROM lanes WHERE key = ?",
            )
            .bind(key)
            .fetch_one(&self.pool)
            .await?;
            let current = tokio::fs::read_to_string(brief_path.trim()).await.ok().map(|c| brief_sha256(&c));
            if ack.0.as_deref() != Some(brief_path.trim()) || current.is_none() || ack.1 != current {
                self.send_brief(key, host.as_deref(), &session_id, &title, &done_signals, &brief_path)
                    .await?;
            }
        }
        let reading = self
            .control
            .execute(
                host.as_deref(),
                AgentOp::Read {
                    session_id: session_id.clone(),
                },
            )
            .await?;
        let screen = reading
            .pointer("/session/screen/text")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let history = reading
            .pointer("/session/history")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let source = if history.is_empty() {
            screen.to_owned()
        } else {
            history
        };
        if let Some(evidence) = claim_from_text(key, &source) {
            let current = state::status_of(&self.pool, key).await?;
            if matches!(
                current,
                Some(LaneStatus::Working | LaneStatus::Blocked | LaneStatus::NotDone)
            ) && (current != Some(LaneStatus::NotDone)
                || reserve_reclaim(&self.pool, key, &evidence).await?) {
                accept_claim(&self.pool, key, &evidence, current == Some(LaneStatus::NotDone)).await?;
                settle(&self.pool, key, &cwd, &done_signals, &evidence, &self.judge).await?;
            }
        }
        let status = state::status_of(&self.pool, key)
            .await?
            .ok_or_else(|| HubError::message(format!("lane {key} disappeared")))?;
        let _ = json!({"sessionId": session_id});
        Ok(DriveReport { session_id, status })
    }
}

/// Code recorded on a lane whose brief send returned no success marker.
pub const BRIEF_SEND_AMBIGUOUS: &str = "BRIEF_SEND_AMBIGUOUS";

/// Lowercase hex sha256 of the brief file content, as stored in `lanes.brief_ack_sha256`.
pub fn brief_sha256(content: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

/// A positively identified send success. The CLI wrapper adds `"ok": true`; the daemon
/// bridge returns the gateway result unchanged, `{"receipt": {requestId, stage}}`, which is
/// built only for an accepted input (`ReferenceInputOutcome::Accepted`). Any present `ok`
/// that is not `true` overrides a receipt, and the receipt must name this request.
fn brief_send_succeeded(result: &serde_json::Value, request_id: &str) -> bool {
    match result.get("ok") {
        Some(ok) => ok.as_bool() == Some(true),
        None => result.get("receipt").is_some_and(|receipt| {
            receipt.get("requestId").and_then(|id| id.as_str()) == Some(request_id)
                && matches!(
                    receipt.get("stage").and_then(|stage| stage.as_str()),
                    Some("accepted" | "providerRead")
                )
        }),
    }
}

impl LaneRunner {
    /// Sends the title and brief once. Records the brief ack (path + sha256) only on a
    /// positively identified success (`brief_send_succeeded`); any other result, or an
    /// unknown-outcome error, is escalated as ambiguous (`brief_escalation`) and leaves the
    /// ack absent.
    async fn send_brief(
        &self,
        key: &str,
        host: Option<&str>,
        session_id: &str,
        title: &str,
        done_signals: &str,
        brief_path: &str,
    ) -> Result<()> {
        let path = brief_path.trim();
        let content = match path {
            "" => None,
            path => tokio::fs::read_to_string(path).await.ok(),
        };
        let brief = content.as_deref().map(|brief| format!("\n\n{brief}")).unwrap_or_default();
        let text = format!(
            "{title}{brief}\n\nDone = {done_signals}\n\nClaim completion with a standalone line: [CLAIM_DONE] {key} | <checkable evidence>. A [REPORT] done line is not a claim."
        );
        // Recheck the ack while atomically claiming delivery. The timestamp marker is a
        // five-minute lease; its random suffix fences off a sender whose lease expired.
        let sha = content.as_deref().map(brief_sha256);
        let lease: Option<String> = sqlx::query_scalar(
            "UPDATE lanes SET brief_ack_path = NULL, brief_ack_sha256 = NULL,
                 brief_ack_at = 'sending:' || datetime('now') || ':' || lower(hex(randomblob(16)))
             WHERE key = ?
               AND (brief_ack_at IS NULL OR brief_ack_at NOT LIKE 'sending:%'
                    OR substr(brief_ack_at, 9, 19) <= datetime('now', '-5 minutes'))
               AND (? IS NULL OR brief_ack_path IS NULL OR brief_ack_sha256 IS NULL
                    OR brief_ack_path <> ? OR brief_ack_sha256 <> ?)
             RETURNING brief_ack_at",
        )
        .bind(key)
        .bind(&sha)
        .bind(path)
        .bind(&sha)
        .fetch_optional(&self.pool)
        .await?;
        let Some(lease) = lease else {
            return Ok(());
        };
        let request_id = format!("lane-{key}-brief");
        let sent = self
            .control
            .execute(
                host,
                AgentOp::Send {
                    session_id: session_id.to_owned(),
                    text,
                    request_id: request_id.clone(),
                },
            )
            .await;
        let problem = match sent {
            Ok(result) if brief_send_succeeded(&result, &request_id) => {
                match (path, content.as_deref()) {
                    ("", _) => {
                        sqlx::query("UPDATE lanes SET brief_ack_at = NULL WHERE key = ? AND brief_ack_at = ?")
                            .bind(key)
                            .bind(&lease)
                            .execute(&self.pool)
                            .await?;
                        None
                    },
                    (path, Some(content)) => {
                        sqlx::query(
                            "UPDATE lanes SET brief_ack_path = ?, brief_ack_sha256 = ?, brief_ack_at = CURRENT_TIMESTAMP, brief_escalation = NULL WHERE key = ? AND brief_ack_at = ?",
                        )
                        .bind(path)
                        .bind(brief_sha256(content))
                        .bind(key)
                        .bind(&lease)
                        .execute(&self.pool)
                        .await?;
                        None
                    }
                    (path, None) => Some(format!("{BRIEF_SEND_AMBIGUOUS}: brief {path} unreadable; sent without it")),
                }
            }
            Ok(result) => Some(format!("{BRIEF_SEND_AMBIGUOUS}: send result has no success marker: {result}")),
            Err(error) if error.to_string().contains(crate::agent_control::OPERATION_OUTCOME_UNKNOWN) => {
                Some(format!("{BRIEF_SEND_AMBIGUOUS}: {error}"))
            }
            Err(error) => {
                sqlx::query("UPDATE lanes SET brief_ack_path = NULL, brief_ack_sha256 = NULL, brief_ack_at = NULL WHERE key = ? AND brief_ack_at = ?")
                    .bind(key)
                    .bind(&lease)
                    .execute(&self.pool)
                    .await?;
                return Err(error);
            }
        };
        if let Some(problem) = problem {
            tracing::warn!(lane = key, %problem, "lane brief delivery is ambiguous; escalated, not recorded as sent");
            sqlx::query("UPDATE lanes SET brief_ack_path = NULL, brief_ack_sha256 = NULL, brief_ack_at = NULL, brief_escalation = ? WHERE key = ? AND brief_ack_at = ?")
                .bind(&problem)
                .bind(key)
                .bind(&lease)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
}

/// The lane carries a legacy goal contract that is not off: owner supervision.
async fn supervised_by_legacy_goal(pool: &SqlitePool, key: &str) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM lane_goal WHERE lane_key = ? AND mode <> 'off')",
    )
    .bind(key)
    .fetch_one(pool)
    .await?)
}

const RECLAIM_LIMIT: i64 = 3;
const RECLAIM_LIMIT_REASON: &str = "claim_retry_limit";

/// Reserve a bounded retry in the dedicated claim_retries counter. No status writes:
/// accepted claims still use transition and its allowed_from guard.
async fn reserve_reclaim(pool: &SqlitePool, key: &str, evidence: &str) -> Result<bool> {
    let reserved = sqlx::query(
        "UPDATE lanes SET claim_retries = claim_retries + 1 WHERE key = ? AND status = 'not_done' AND claim_evidence IS NOT ? AND claim_retries < ?",
    )
    .bind(key)
    .bind(evidence)
    .bind(RECLAIM_LIMIT)
    .execute(pool)
    .await?;
    if reserved.rows_affected() > 0 {
        return Ok(true);
    }
    // A refusal is an audit note, not a lifecycle transition. Persist it only once so
    // later drives neither grow history nor call the judge after the cap is reached.
    sqlx::query(
        "INSERT INTO lane_history(lane_key, at, from_status, to_status, reason)
         SELECT key, ?, status, status, ? FROM lanes
         WHERE key = ? AND status = 'not_done' AND claim_retries >= ?
         AND NOT EXISTS(SELECT 1 FROM lane_history WHERE lane_key = lanes.key AND reason = ?)",
    )
    .bind(crate::storage::now_rfc3339())
    .bind(RECLAIM_LIMIT_REASON)
    .bind(key)
    .bind(RECLAIM_LIMIT)
    .bind(RECLAIM_LIMIT_REASON)
    .execute(pool)
    .await?;
    Ok(false)
}

/// Release a reservation when the guarded claim transition fails.
async fn accept_claim(pool: &SqlitePool, key: &str, evidence: &str, reserved: bool) -> Result<()> {
    let result: Result<()> = async {
        set_claim(pool, key, evidence).await?;
        transition(pool, key, LaneStatus::DoneClaimed, Some("claim")).await?;
        Ok(())
    }.await;
    if reserved && result.is_err() {
        sqlx::query("UPDATE lanes SET claim_retries = MAX(0, claim_retries - 1) WHERE key = ?")
            .bind(key)
            .execute(pool)
            .await?;
    }
    result
}

/// The last `[CLAIM_DONE] {key} | evidence` line wins. A `[REPORT]` line is not a claim.
pub fn claim_from_text(key: &str, text: &str) -> Option<String> {
    let prefix = format!("[CLAIM_DONE] {key} | ");
    for line in text.lines().rev() {
        let line = line.trim();
        if line.starts_with("[REPORT]") {
            continue;
        }
        if let Some(evidence) = line.strip_prefix(&prefix) {
            let evidence = evidence.trim();
            if !evidence.is_empty() {
                return Some(evidence.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::{FixedLauncher, RecordingControl};
    use crate::judge::Judge;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn a_report_line_is_not_a_claim() {
        let text = "[REPORT] done\n[CLAIM_DONE] lane-g4 | marker exists";
        assert_eq!(
            claim_from_text("lane-g4", text).as_deref(),
            Some("marker exists")
        );
        assert!(claim_from_text("lane-g4", "[REPORT] [CLAIM_DONE] lane-g4 | nope").is_none());
    }

    #[tokio::test]
    async fn a_lane_reaches_verified_done_on_a_pane() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-lane-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("marker"), "ok").unwrap();
        let pool = crate::storage::memory().await.unwrap();
        state::insert_lane(
            &pool,
            "lane-g4",
            "prove the marker",
            "lead",
            dir.to_str().unwrap(),
            "file marker",
        )
        .await
        .unwrap();
        let control = Arc::new(RecordingControl::default());
        *control.result.lock().unwrap() = json!({
            "session": {"screen": {"text": "[CLAIM_DONE] lane-g4 | marker exists\n", "revision": "7"}}
        });
        let runner = LaneRunner {
            pool: pool.clone(),
            control: control.clone(),
            launcher: Arc::new(FixedLauncher::new("pane-g4")),
            judge: Judge::fallback_yes(),
            load: HostLoad {
                load: 0.1,
                mem_free_pct: 50,
                disk_free_gb: 100,
                active_lanes: 0,
            },
            thresholds: GuardThresholds {
                load_alert: 10.0,
                mem_free_min_pct: 10,
                disk_free_min_gb: 1,
                lanes_max: 4,
            },
        };
        // A paused lane is not driven: no launch, no read.
        crate::lanes::pause::pause(&pool, "lane-g4", "session:lead")
            .await
            .unwrap();
        assert!(runner.drive("lane-g4").await.is_err());
        assert!(control.ops().is_empty());
        crate::lanes::pause::resume(&pool, "lane-g4").await.unwrap();
        let report = runner.drive("lane-g4").await.unwrap();
        assert_eq!(report.session_id, "pane-g4");
        assert_eq!(report.status, LaneStatus::VerifiedDone);
        let ops = control.ops();
        assert!(ops.iter().any(|(_, op)| matches!(op, AgentOp::Send { .. })));
        assert!(ops.iter().any(|(_, op)| matches!(op, AgentOp::Read { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn interrupted_claims_are_rechecked_without_a_pane_claim() {
        let dir = std::env::temp_dir().join(format!(
            "ferryx-hub-interrupted-{}", std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("marker"), "ok").unwrap();
        for (key, signals, evidence, expected) in [
            ("recheck-ok", "file marker", Some("marker exists"), LaneStatus::VerifiedDone),
            ("recheck-failed", "file absent", Some("marker exists"), LaneStatus::NotDone),
            ("recheck-empty", "file marker", None, LaneStatus::NotDone),
        ] {
            let pool = crate::storage::memory().await.unwrap();
            state::insert_lane(&pool, key, "check", "lead", dir.to_str().unwrap(), signals)
                .await.unwrap();
            transition(&pool, key, LaneStatus::Working, None).await.unwrap();
            if let Some(evidence) = evidence {
                set_claim(&pool, key, evidence).await.unwrap();
            }
            transition(&pool, key, LaneStatus::DoneClaimed, Some("claim"))
                .await.unwrap();
            let control = Arc::new(RecordingControl::default());
            let runner = full_runner(pool.clone(), control.clone(), Arc::new(RemoteLauncher::default()));
            assert_eq!(runner.drive(key).await.unwrap().status, expected, "{key}");
            assert_eq!(state::status_of(&pool, key).await.unwrap(), Some(expected));
            assert!(control.ops().is_empty(), "recovery must not depend on a pane");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn a_not_done_lane_can_settle_a_new_claim() {
        let dir = std::env::temp_dir().join(format!(
            "ferryx-hub-reclaim-{}", std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("marker"), "ok").unwrap();
        let pool = crate::storage::memory().await.unwrap();
        state::insert_lane(&pool, "reclaim", "check", "lead", dir.to_str().unwrap(), "file marker")
            .await.unwrap();
        for status in [LaneStatus::Working, LaneStatus::DoneClaimed, LaneStatus::NotDone] {
            transition(&pool, "reclaim", status, None).await.unwrap();
        }
        sqlx::query("UPDATE lanes SET turns_run = 99, claim_retries = 0 WHERE key = 'reclaim'")
            .execute(&pool).await.unwrap();
        let control = Arc::new(RecordingControl::default());
        *control.result.lock().unwrap() = json!({
            "session": {"screen": {"text": "[CLAIM_DONE] reclaim | marker exists"}}
        });
        let runner = full_runner(pool.clone(), control, Arc::new(RemoteLauncher::default()));
        assert_eq!(runner.drive("reclaim").await.unwrap().status, LaneStatus::VerifiedDone);
        let stored: (Option<String>, i64, i64) = sqlx::query_as(
            "SELECT claim_evidence, turns_run, claim_retries FROM lanes WHERE key = 'reclaim'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(stored, (Some("marker exists".into()), 99, 0));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_newer_claim_line_wins_over_a_stale_one() {
        assert_eq!(
            claim_from_text("retry", "[CLAIM_DONE] retry | stale\n[CLAIM_DONE] retry | newer\n[REPORT] [CLAIM_DONE] retry | ignored").as_deref(),
            Some("newer")
        );
    }

    async fn rejected_claim_runner() -> (SqlitePool, LaneRunner, Arc<RecordingControl>) {
        let pool = crate::storage::memory().await.unwrap();
        state::insert_lane(&pool, "retry", "check", "lead", "/", "file /dev/null/marker")
            .await.unwrap();
        transition(&pool, "retry", LaneStatus::Working, None).await.unwrap();
        set_claim(&pool, "retry", "old evidence").await.unwrap();
        transition(&pool, "retry", LaneStatus::DoneClaimed, Some("claim")).await.unwrap();
        transition(&pool, "retry", LaneStatus::NotDone, Some("signals failed")).await.unwrap();
        let control = Arc::new(RecordingControl::default());
        let runner = full_runner(pool.clone(), control.clone(), Arc::new(RemoteLauncher::default()));
        (pool, runner, control)
    }

    #[tokio::test]
    async fn identical_evidence_does_not_reclaim_from_not_done() {
        let (pool, runner, control) = rejected_claim_runner().await;
        *control.result.lock().unwrap() = json!({
            "session": {"history": ["[CLAIM_DONE] retry | old evidence"]}
        });
        for _ in 0..3 {
            assert_eq!(runner.drive("retry").await.unwrap().status, LaneStatus::NotDone);
        }
        let stored: (Option<String>, i64) = sqlx::query_as(
            "SELECT claim_evidence, claim_retries FROM lanes WHERE key = 'retry'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(stored, (Some("old evidence".into()), 0));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lane_history WHERE lane_key = 'retry'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn reclaim_cap_stops_the_loop_and_persists_a_reason() {
        let (pool, _, _) = rejected_claim_runner().await;
        for attempt in 0..RECLAIM_LIMIT + 3 {
            assert_eq!(reserve_reclaim(&pool, "retry", &format!("evidence {attempt}")).await.unwrap(), attempt < RECLAIM_LIMIT);
            let retries: i64 = sqlx::query_scalar("SELECT claim_retries FROM lanes WHERE key = 'retry'")
                .fetch_one(&pool).await.unwrap();
            assert_eq!(retries, (attempt + 1).min(RECLAIM_LIMIT));
        }
        let reasons: Vec<Option<String>> = sqlx::query_scalar(
            "SELECT reason FROM lane_history WHERE lane_key = 'retry' ORDER BY id",
        ).fetch_all(&pool).await.unwrap();
        assert_eq!(reasons.len(), 4);
        assert_eq!(reasons.last().and_then(|reason| reason.as_deref()), Some(RECLAIM_LIMIT_REASON));
    }

    #[tokio::test]
    async fn successful_claim_resets_claim_retries() {
        let (pool, _, _) = rejected_claim_runner().await;
        for _ in 0..RECLAIM_LIMIT {
            assert!(reserve_reclaim(&pool, "retry", "new evidence").await.unwrap());
        }
        accept_claim(&pool, "retry", "new evidence", true).await.unwrap();
        let retries: i64 = sqlx::query_scalar("SELECT claim_retries FROM lanes WHERE key = 'retry'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(retries, 0);
        transition(&pool, "retry", LaneStatus::NotDone, None).await.unwrap();
        assert!(reserve_reclaim(&pool, "retry", "next evidence").await.unwrap());
    }

    #[tokio::test]
    async fn lost_claim_transition_releases_reservation() {
        let (pool, _, _) = rejected_claim_runner().await;
        assert!(reserve_reclaim(&pool, "retry", "new evidence").await.unwrap());
        transition(&pool, "retry", LaneStatus::Working, None).await.unwrap();
        transition(&pool, "retry", LaneStatus::Failed, None).await.unwrap();
        assert!(accept_claim(&pool, "retry", "new evidence", true).await.is_err());
        let retries: i64 = sqlx::query_scalar("SELECT claim_retries FROM lanes WHERE key = 'retry'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(retries, 0);
        assert_eq!(state::status_of(&pool, "retry").await.unwrap(), Some(LaneStatus::Failed));
    }

    /// Opens remote panes on whichever host it is asked for and records the requests.
    #[derive(Default)]
    struct RemoteLauncher {
        remote: std::sync::Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl TaskLauncher for RemoteLauncher {
        async fn launch_pane(&self, _cwd: &str, _command: &[String]) -> Result<String> {
            Err(HubError::message("this machine is full"))
        }
        async fn remote_hosts(&self) -> Result<Vec<String>> {
            Ok(vec!["mac".into(), "linux".into()])
        }
        async fn launch_remote_pane(
            &self,
            host_id: &str,
            _cwd: &str,
            lane_key: &str,
        ) -> Result<crate::agent_control::RemotePane> {
            self.remote.lock().unwrap().push((host_id.into(), lane_key.into()));
            Ok(crate::agent_control::RemotePane {
                session_id: "remote-pane".into(),
                cwd: "/remote/lane".into(),
            })
        }
    }

    fn full_runner(pool: SqlitePool, control: Arc<RecordingControl>, launcher: Arc<RemoteLauncher>) -> LaneRunner {
        LaneRunner {
            pool,
            control,
            launcher,
            judge: Judge::fallback_yes(),
            load: HostLoad { load: 0.1, mem_free_pct: 50, disk_free_gb: 100, active_lanes: 4 },
            thresholds: GuardThresholds { load_alert: 10.0, mem_free_min_pct: 10, disk_free_min_gb: 1, lanes_max: 4 },
        }
    }

    #[tokio::test]
    async fn a_refused_lane_runs_on_a_paired_host_and_is_driven_there() {
        let pool = crate::storage::memory().await.unwrap();
        state::insert_lane(&pool, "lane-r", "ship it", "lead", "/repo", "merged acme/app#12")
            .await
            .unwrap();
        let control = Arc::new(RecordingControl::default());
        *control.result.lock().unwrap() = json!({"sessions": []});
        control.fail_hosts.lock().unwrap().insert("mac".into());
        let launcher = Arc::new(RemoteLauncher::default());
        let runner = full_runner(pool.clone(), control.clone(), launcher.clone());
        let report = runner.drive("lane-r").await.unwrap();
        assert_eq!(report.session_id, "remote-pane");
        assert_eq!(report.status, LaneStatus::Working);
        assert_eq!(*launcher.remote.lock().unwrap(), vec![("linux".to_owned(), "lane-r".to_owned())]);
        assert_eq!(state::lane_host(&pool, "lane-r").await.unwrap().as_deref(), Some("linux"));
        // The brief and the read both go to the host the lane runs on.
        let ops = control.ops();
        for (host, op) in &ops {
            if matches!(op, AgentOp::Send { .. } | AgentOp::Read { .. }) {
                assert_eq!(host.as_deref(), Some("linux"), "{op:?}");
            }
        }
        assert!(ops.iter().any(|(_, op)| matches!(op, AgentOp::Send { .. })));
        assert!(ops.iter().any(|(_, op)| matches!(op, AgentOp::Read { .. })));
        assert_eq!(state::local_active_lanes(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn a_lane_with_local_signals_is_refused_instead_of_moved() {
        let pool = crate::storage::memory().await.unwrap();
        state::insert_lane(&pool, "lane-f", "write it", "lead", "/repo", "file marker")
            .await
            .unwrap();
        let control = Arc::new(RecordingControl::default());
        let launcher = Arc::new(RemoteLauncher::default());
        let runner = full_runner(pool.clone(), control.clone(), launcher.clone());
        let error = runner.drive("lane-f").await.unwrap_err();
        assert!(error.to_string().contains("guard refused"), "{error}");
        assert!(launcher.remote.lock().unwrap().is_empty());
        assert!(control.ops().is_empty());
        assert_eq!(state::status_of(&pool, "lane-f").await.unwrap(), Some(LaneStatus::Queued));
    }

    #[tokio::test]
    async fn after_a_cutover_only_lanes_under_a_legacy_goal_are_refused() {
        use crate::goals::contract::{set_contract, RequirementSpec};
        use crate::goals::types::GoalMode;
        let pool = crate::storage::memory().await.unwrap();
        for key in ["lane-goal", "lane-cron"] {
            state::insert_lane(&pool, key, "work", "lead", "/repo", "merged acme/app#1")
                .await
                .unwrap();
        }
        set_contract(
            &pool,
            "lane-goal",
            &[RequirementSpec {
                id: "r1".into(),
                text: "ship".into(),
                verifier: "cargo test".into(),
            }],
            GoalMode::Supervised,
            5,
        )
        .await
        .unwrap();
        let identity = crate::supervisor::provision_identity(&pool, "local:owner").await.unwrap();
        sqlx::query(
            "INSERT INTO supervisor_cutover(supervisor_id, legacy_intake, unified_intake, origin, updated_at)
             VALUES(?, 'stopped', 'open', 'cutover', '2026-11-01T00:00:00Z')",
        )
        .bind(&identity.supervisor_id)
        .execute(&pool)
        .await
        .unwrap();
        let control = Arc::new(RecordingControl::default());
        *control.result.lock().unwrap() = json!({"session": {"screen": {"text": "", "revision": "1"}}});
        let runner = LaneRunner {
            pool: pool.clone(),
            control: control.clone(),
            launcher: Arc::new(FixedLauncher::new("pane-x")),
            judge: Judge::fallback_yes(),
            load: HostLoad { load: 0.1, mem_free_pct: 50, disk_free_gb: 100, active_lanes: 0 },
            thresholds: GuardThresholds { load_alert: 10.0, mem_free_min_pct: 10, disk_free_min_gb: 1, lanes_max: 4 },
        };
        let refused = runner.drive("lane-goal").await.unwrap_err();
        assert!(refused.to_string().contains(LEGACY_INTAKE_STOPPED), "{refused}");
        assert!(control.ops().is_empty(), "nothing was launched or sent");
        assert_eq!(state::status_of(&pool, "lane-goal").await.unwrap(), Some(LaneStatus::Queued));
        // A lane without a legacy goal (a cron remediation lane) still runs.
        let report = runner.drive("lane-cron").await.unwrap();
        assert_eq!(report.status, LaneStatus::Working);
    }

    async fn brief_lane(name: &str) -> (SqlitePool, LaneRunner, Arc<RecordingControl>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-brief-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let brief = dir.join("brief.md");
        std::fs::write(&brief, "fix the cron job").unwrap();
        let pool = crate::storage::memory().await.unwrap();
        state::insert_lane(&pool, "lane-b", "remediate", "lead", dir.to_str().unwrap(), "merged acme/app#1")
            .await
            .unwrap();
        sqlx::query("UPDATE lanes SET brief_path = ? WHERE key = 'lane-b'")
            .bind(brief.to_str().unwrap())
            .execute(&pool)
            .await
            .unwrap();
        let control = Arc::new(RecordingControl::default());
        let runner = LaneRunner {
            pool: pool.clone(),
            control: control.clone(),
            launcher: Arc::new(FixedLauncher::new("pane-b")),
            judge: Judge::fallback_yes(),
            load: HostLoad { load: 0.1, mem_free_pct: 50, disk_free_gb: 100, active_lanes: 0 },
            thresholds: GuardThresholds { load_alert: 10.0, mem_free_min_pct: 10, disk_free_min_gb: 1, lanes_max: 4 },
        };
        (pool, runner, control, brief)
    }

    async fn brief_ack(pool: &SqlitePool) -> (Option<String>, Option<String>, Option<String>) {
        sqlx::query_as("SELECT brief_ack_path, brief_ack_sha256, brief_escalation FROM lanes WHERE key = 'lane-b'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    fn brief_sends(control: &RecordingControl) -> usize {
        control
            .ops()
            .iter()
            .filter(|(_, op)| matches!(op, AgentOp::Send { request_id, .. } if request_id == "lane-lane-b-brief"))
            .count()
    }

    #[tokio::test]
    async fn a_first_drive_records_the_brief_path_and_sha256() {
        let (pool, runner, control, brief) = brief_lane("first").await;
        runner.drive("lane-b").await.unwrap();
        assert_eq!(brief_sends(&control), 1);
        let (path, sha, escalation) = brief_ack(&pool).await;
        assert_eq!(path.as_deref(), brief.to_str());
        assert_eq!(sha, Some(brief_sha256("fix the cron job")));
        assert_eq!(sha.unwrap().len(), 64);
        assert!(escalation.is_none());
    }

    #[tokio::test]
    async fn a_redrive_with_a_missing_ack_resends_the_brief() {
        let (pool, runner, control, brief) = brief_lane("missing").await;
        runner.drive("lane-b").await.unwrap();
        sqlx::query("UPDATE lanes SET brief_ack_path = NULL, brief_ack_sha256 = NULL WHERE key = 'lane-b'")
            .execute(&pool)
            .await
            .unwrap();
        runner.drive("lane-b").await.unwrap();
        assert_eq!(brief_sends(&control), 2);
        let (path, sha, _) = brief_ack(&pool).await;
        assert_eq!(path.as_deref(), brief.to_str());
        assert_eq!(sha, Some(brief_sha256("fix the cron job")));
    }

    #[tokio::test]
    async fn a_redrive_with_a_matching_ack_does_not_resend() {
        let (pool, runner, control, brief) = brief_lane("matching").await;
        runner.drive("lane-b").await.unwrap();
        let (path, sha, _) = brief_ack(&pool).await;
        assert_eq!(path.as_deref(), brief.to_str());
        assert_eq!(sha, Some(brief_sha256("fix the cron job")));
        runner.drive("lane-b").await.unwrap();
        assert_eq!(brief_sends(&control), 1);
    }

    #[tokio::test]
    async fn a_stale_ack_then_ambiguous_send_then_revert_resends() {
        let (pool, runner, control, brief) = brief_lane("revert").await;
        runner.drive("lane-b").await.unwrap();
        assert!(brief_ack(&pool).await.1.is_some());
        std::fs::write(&brief, "changed brief").unwrap();
        *control.result.lock().unwrap() = json!({"queued": true});
        runner.drive("lane-b").await.unwrap();
        let (path, sha, escalation) = brief_ack(&pool).await;
        assert!(path.is_none() && sha.is_none());
        assert!(escalation.is_some());
        let at: Option<String> = sqlx::query_scalar("SELECT brief_ack_at FROM lanes WHERE key = 'lane-b'")
            .fetch_one(&pool).await.unwrap();
        assert!(at.is_none());
        std::fs::write(&brief, "fix the cron job").unwrap();
        *control.result.lock().unwrap() = json!({"ok": true});
        runner.drive("lane-b").await.unwrap();
        assert_eq!(brief_sends(&control), 3);
        let (path, sha, escalation) = brief_ack(&pool).await;
        assert_eq!(path.as_deref(), brief.to_str());
        assert_eq!(sha, Some(brief_sha256("fix the cron job")));
        assert!(escalation.is_none());
    }

    struct HeldBriefControl {
        inner: Arc<RecordingControl>,
        entered: tokio::sync::Notify,
        release: tokio::sync::Semaphore,
    }

    #[async_trait::async_trait]
    impl AgentControl for HeldBriefControl {
        async fn execute(&self, host: Option<&str>, op: AgentOp) -> Result<serde_json::Value> {
            let is_send = matches!(&op, AgentOp::Send { .. });
            let result = self.inner.execute(host, op).await;
            if is_send {
                self.entered.notify_one();
                self.release.acquire().await.unwrap().forget();
            }
            result
        }
    }

    #[tokio::test]
    async fn two_concurrent_drives_send_the_brief_once() {
        let (pool, mut runner, control, _brief) = brief_lane("concurrent").await;
        sqlx::query("UPDATE lanes SET status = 'working', omo_thread_id = 'pane-b' WHERE key = 'lane-b'")
            .execute(&pool).await.unwrap();
        let held = Arc::new(HeldBriefControl {
            inner: control.clone(),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Semaphore::new(0),
        });
        runner.control = held.clone();
        // Subscribe before driving; keep the first send pending until the competing
        // drive has returned. A missing reservation deterministically times out.
        let entered = held.entered.notified();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let first = runner.drive("lane-b");
            let second = async {
                entered.await;
                runner.drive("lane-b").await.unwrap();
                held.release.add_permits(1);
            };
            let (first, ()) = tokio::join!(first, second);
            first.unwrap();
        }).await.unwrap();
        assert_eq!(brief_sends(&control), 1);
        assert!(brief_ack(&pool).await.1.is_some());
    }

    #[tokio::test]
    async fn an_ambiguous_brief_send_is_escalated_not_recorded_as_sent() {
        let (pool, runner, control, _brief) = brief_lane("ambiguous").await;
        // No success marker in the send result.
        *control.result.lock().unwrap() = json!({"queued": true});
        runner.drive("lane-b").await.unwrap();
        assert_eq!(brief_sends(&control), 1);
        let (path, sha, escalation) = brief_ack(&pool).await;
        assert!(path.is_none() && sha.is_none());
        let at: Option<String> = sqlx::query_scalar("SELECT brief_ack_at FROM lanes WHERE key = 'lane-b'")
            .fetch_one(&pool).await.unwrap();
        assert!(at.is_none());
        assert!(escalation.unwrap().starts_with(BRIEF_SEND_AMBIGUOUS));
        // The ack is still absent, so the next drive tries again.
        *control.result.lock().unwrap() = json!({"ok": true});
        runner.drive("lane-b").await.unwrap();
        assert_eq!(brief_sends(&control), 2);
        assert!(brief_ack(&pool).await.1.is_some());
    }

    fn production_receipt(stage: &str) -> serde_json::Value {
        json!({"receipt": {
            "requestId": "lane-lane-b-brief",
            "target": {"hostId": "local", "sessionId": "pane-b"},
            "stage": stage,
        }})
    }

    #[tokio::test]
    async fn a_production_receipt_without_ok_records_the_ack() {
        let (pool, runner, control, brief) = brief_lane("production").await;
        *control.result.lock().unwrap() = production_receipt("accepted");
        runner.drive("lane-b").await.unwrap();
        assert_eq!(brief_sends(&control), 1);
        let (path, sha, escalation) = brief_ack(&pool).await;
        assert_eq!(path.as_deref(), brief.to_str());
        assert_eq!(sha, Some(brief_sha256("fix the cron job")));
        assert!(escalation.is_none());
        runner.drive("lane-b").await.unwrap();
        assert_eq!(brief_sends(&control), 1);
    }

    #[tokio::test]
    async fn a_cli_wrapped_ok_result_records_the_ack() {
        let (pool, runner, control, brief) = brief_lane("cli").await;
        let mut wrapped = production_receipt("accepted");
        wrapped["ok"] = json!(true);
        *control.result.lock().unwrap() = wrapped;
        runner.drive("lane-b").await.unwrap();
        let (path, sha, escalation) = brief_ack(&pool).await;
        assert_eq!(path.as_deref(), brief.to_str());
        assert_eq!(sha, Some(brief_sha256("fix the cron job")));
        assert!(escalation.is_none());
    }

    struct UnknownOutcomeControl(Arc<RecordingControl>);

    #[async_trait::async_trait]
    impl AgentControl for UnknownOutcomeControl {
        async fn execute(&self, host: Option<&str>, op: AgentOp) -> Result<serde_json::Value> {
            let is_send = matches!(&op, AgentOp::Send { .. });
            let result = self.0.execute(host, op).await;
            if is_send {
                return Err(crate::error::HubError::message(format!(
                    "{} the writer did not answer",
                    crate::agent_control::OPERATION_OUTCOME_UNKNOWN
                )));
            }
            result
        }
    }

    #[tokio::test]
    async fn an_unknown_outcome_send_stays_ambiguous() {
        let (pool, mut runner, control, _brief) = brief_lane("unknown").await;
        runner.control = Arc::new(UnknownOutcomeControl(control.clone()));
        runner.drive("lane-b").await.unwrap();
        assert_eq!(brief_sends(&control), 1);
        let (path, sha, escalation) = brief_ack(&pool).await;
        assert!(path.is_none() && sha.is_none());
        assert!(escalation.unwrap().starts_with(BRIEF_SEND_AMBIGUOUS));
    }

    #[tokio::test]
    async fn a_negative_or_unidentified_marker_stays_ambiguous() {
        for (name, result) in [
            ("ok-false", json!({"ok": false})),
            ("ok-false-receipt", { let mut r = production_receipt("accepted"); r["ok"] = json!(false); r }),
            ("staged", production_receipt("staged")),
            ("other-request", json!({"receipt": {"requestId": "other", "stage": "accepted"}})),
        ] {
            let (pool, runner, control, _brief) = brief_lane(name).await;
            *control.result.lock().unwrap() = result;
            runner.drive("lane-b").await.unwrap();
            let (path, sha, escalation) = brief_ack(&pool).await;
            assert!(path.is_none() && sha.is_none(), "{name}");
            assert!(escalation.unwrap().starts_with(BRIEF_SEND_AMBIGUOUS), "{name}");
        }
    }
}
