//! The live re-checks a settlement runs at the instant it closes, never at claim time: unsent
//! work, every done check passing now, and no undelivered accepted message. Every reason is a
//! refusal; none is a warning.
use sqlx::SqlitePool;

use super::settle::SettleTarget;
use super::settle_worktree::worktree_reasons;
use super::signals::{check_signals_for_host, DoneSignal};
use crate::error::Result;

/// The three checks, in order.
pub(crate) async fn live_reasons(
    pool: &SqlitePool,
    target: SettleTarget<'_>,
    signals: &[DoneSignal],
) -> Result<Vec<String>> {
    let mut reasons = if target.cwd.trim().is_empty() {
        // A lane with no working directory has no worktree to check.
        Vec::new()
    } else {
        worktree_reasons(pool, target.key, target.cwd).await?
    };
    // A cron remediation lane settles on the hub's own verified rerun, never on the lane's
    // claim; the rerun runs now, not read from the claim.
    let checks = match Box::pin(crate::cron_remediate::settle_lane(pool, target.key)).await? {
        Some(checks) => checks,
        None => {
            let host = super::state::lane_host(pool, target.key).await?;
            check_signals_for_host(signals, std::path::Path::new(target.cwd), host.as_deref(), None)
                .await
        }
    };
    for check in checks.iter().filter(|check| !check.passed) {
        reasons.push(format!(
            "done signal failed now: {} ({})",
            check.signal, check.detail
        ));
    }
    reasons.extend(undelivered_reasons(pool, target.key).await?);
    Ok(reasons)
}

/// A lane event the lead never received, and an instruction a worker took but never started.
/// The settlement's own claim and objection notices are excluded: they are the settlement's
/// narration to the lead, not work still owed.
async fn undelivered_reasons(pool: &SqlitePool, key: &str) -> Result<Vec<String>> {
    let events: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, kind FROM lane_events WHERE lane_key = ? AND delivered_at IS NULL
           AND kind NOT IN ('settlement_claim','settlement_objection') ORDER BY id",
    )
    .bind(key)
    .fetch_all(pool)
    .await?;
    let mut reasons: Vec<String> = events
        .into_iter()
        .map(|(id, kind)| format!("undelivered {kind} message #{id}"))
        .collect();
    let dispatches: Vec<(String, String)> = sqlx::query_as(
        "SELECT d.request_id, d.state FROM supervisor_dispatches d
           JOIN supervisor_goals g ON g.goal_id = d.goal_id
          WHERE g.lane_key = ? AND d.state IN ('claimed','accepted') ORDER BY d.id",
    )
    .bind(key)
    .fetch_all(pool)
    .await?;
    reasons.extend(
        dispatches
            .into_iter()
            .map(|(request_id, state)| format!("undelivered instruction {request_id} ({state})")),
    );
    Ok(reasons)
}
