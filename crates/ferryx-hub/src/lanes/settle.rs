//! The one settlement gate every path that can mark a lane done goes through.
//!
//! A done claim is a *claim*, not a verdict. [`claim_done`] records the evidence and the claim
//! time, opens an objection window ([`OBJECTION_WINDOW_SECS`]), tells the lead once, and tells
//! the lane the UTC instant it will close unless the lead objects. [`settle_claim`] runs only
//! after that window and re-checks the world live, in this order:
//!
//! 1. **No unsent work** — [`super::settle_worktree::worktree_reasons`], hardened: `GIT_*`
//!    overrides cleared, `LC_ALL=C`, and an unreadable git state counts as unsent work.
//! 2. **Every done signal passes now** — [`super::signals::check_signals_for_host`], never a
//!    cached reading from claim time.
//! 3. **No undelivered accepted message** — a lane event the lead never received, or an
//!    instruction a worker took but never started.
//!
//! Any failure is an automatic objection: the lane goes `NotDone` naming the reasons. A pass
//! records the close intent ([`settle_close::record_close_intent`]) *before* the lane closes, so
//! a crash between the checks and the close resumes the same close instead of re-running the
//! checks to a different outcome.
//!
//! The goal paths (`supervisor/completion.rs`, `goals/loop_driver.rs`) close on an explicit
//! supervisor decision, not on a claim window: they call [`verify_before_close`], which runs the
//! same three live checks and records the same close intent in the transaction that completes the
//! goal. Model grading is advisory input to those paths and never a settlement by itself.
//!
//! The window is durable on the lane row (`claim_objection_until`, migration 0064). Its close is
//! driven by [`super::settle_driver::DueDriver`], which sleeps until the earliest deadline and is
//! woken by each new claim (never a poll); a lane with a supervisor goal also gets a scheduled
//! [`crate::supervisor::wakes`] deadline.
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use super::settle_checks::live_reasons;
use super::settle_close::{
    announce, finish_close, schedule_window, settle_wake_event, CloseRequest, CLAIM_EVENT,
    OBJECTION_EVENT,
};
use super::settle_stop::SessionStopper;
use super::signals::{parse_signals, DoneSignal};
use super::state::{transition, LaneStatus};
use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

/// The close half of the gate, re-exported where callers already look for the settlement.
pub use super::settle_close::{
    close_intent, close_intent_json_for, record_close_intent, settle_wake_lane, SETTLE_WAKE_PREFIX,
};

/// How long the lead may object to a done claim before it settles: five minutes.
pub const OBJECTION_WINDOW_SECS: i64 = 300;

/// Transition reasons of a claim, an objection and a settlement.
pub const CLAIM_REASON: &str = "claim";
pub const OBJECTION_REASON: &str = "claim objected";
pub const SETTLED_REASON: &str = "settlement_started";

/// The lane a settlement acts on: its key and the working directory to check.
#[derive(Debug, Clone, Copy)]
pub struct SettleTarget<'a> {
    pub key: &'a str,
    pub cwd: &'a str,
}

/// A done claim as the lane runner read it. `advisory` is the model's opinion of the evidence:
/// recorded for the lead, never a reason to settle.
#[derive(Debug, Clone, Copy)]
pub struct ClaimRequest<'a> {
    pub target: SettleTarget<'a>,
    pub evidence: &'a str,
    pub advisory: Option<&'a str>,
}

/// What [`claim_done`] recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// The UTC instant the lane closes unless the lead objects.
    pub close_at: DateTime<Utc>,
    /// False when the claim (and its notice) was already recorded: the lead hears it once.
    pub announced: bool,
}

/// What one settlement pass did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settlement {
    /// The objection window is still open; nothing was written.
    Pending { close_at: DateTime<Utc> },
    /// Closed: `resumed` when a recorded close intent finished a close a crash interrupted.
    Settled {
        close_at: DateTime<Utc>,
        resumed: bool,
    },
    /// Refused, with the reasons the lane was told. The lane is `not_done`.
    Objected { reasons: Vec<String> },
}

impl Settlement {
    pub fn passed(&self) -> bool {
        matches!(self, Settlement::Settled { .. })
    }

    pub fn detail(&self) -> String {
        match self {
            Settlement::Pending { close_at } => {
                format!("objection window open until {}", close_at.to_rfc3339())
            }
            Settlement::Settled { .. } => "settled".to_owned(),
            Settlement::Objected { reasons } => reasons.join("; "),
        }
    }
}

/// Records a done claim: evidence, claim time, the objection deadline, the lead's one notice
/// and the lane's own notice of the UTC instant it closes unless objected. A second claim on
/// the same open window is the same claim and writes nothing.
pub async fn claim_done(
    pool: &SqlitePool,
    request: ClaimRequest<'_>,
    now: DateTime<Utc>,
) -> Result<Claim> {
    let key = request.target.key;
    let existing: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT status, claim_objection_until FROM lanes WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?;
    let Some((status, open_until)) = existing else {
        return Err(HubError::message(format!("lane {key} not found")));
    };
    let open_until = open_until
        .as_deref()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|at| at.with_timezone(&Utc));
    if let (Some(until), true) = (open_until, status == LaneStatus::DoneClaimed.as_str()) {
        return Ok(Claim {
            close_at: until,
            announced: false,
        });
    }
    let close_at = now + chrono::Duration::seconds(OBJECTION_WINDOW_SECS);
    let stamp = now.to_rfc3339();
    sqlx::query(
        "UPDATE lanes SET claim_evidence = ?, claim_at = ?, claim_objection_until = ?,
             close_intent_json = NULL, updated_at = ?
           WHERE key = ?",
    )
    .bind(request.evidence)
    .bind(&stamp)
    .bind(close_at.to_rfc3339())
    .bind(now_rfc3339())
    .bind(key)
    .execute(pool)
    .await?;
    if status != LaneStatus::DoneClaimed.as_str() {
        transition(pool, key, LaneStatus::DoneClaimed, Some(CLAIM_REASON)).await?;
    }
    let mut body = format!(
        "done claimed: {}\ncloses at {} unless objected; stay idle until then",
        request.evidence,
        close_at.to_rfc3339()
    );
    if let Some(advisory) = request.advisory {
        body.push_str(&format!("\nadvisory grading (not a settlement): {advisory}"));
    }
    announce(
        pool,
        key,
        CLAIM_EVENT,
        &body,
        Some(&settle_wake_event(key, &stamp)),
    )
    .await?;
    schedule_window(pool, key, close_at).await?;
    super::settle_driver::due_signal().notify_one();
    Ok(Claim {
        close_at,
        announced: true,
    })
}

/// The lead objects inside the window: the lane returns to `not_done` with the reasons and is
/// told them. An objection after the close intent is recorded is refused: the close already
/// happened or is finishing, and re-running the checks after that would be a second outcome.
///
/// A lane the goal paths settle is not `done_claimed`, and `not_done` is reachable only from a
/// claim, so there it keeps its state: the objection is recorded for the lead and its goal is
/// held or stopped by the caller.
pub async fn object_claim(pool: &SqlitePool, key: &str, reasons: &[String]) -> Result<bool> {
    if close_intent(pool, key).await?.is_some() {
        return Ok(false);
    }
    let status = super::state::status_of(pool, key).await?;
    let detail = if reasons.is_empty() {
        "the lead objected".to_owned()
    } else {
        reasons.join("; ")
    };
    match status {
        Some(LaneStatus::DoneClaimed) => {
            sqlx::query("UPDATE lanes SET claim_objection_until = NULL WHERE key = ?")
                .bind(key)
                .execute(pool)
                .await?;
            transition(pool, key, LaneStatus::NotDone, Some(OBJECTION_REASON)).await?;
        }
        Some(LaneStatus::Working | LaneStatus::Blocked) => {}
        _ => return Ok(false),
    }
    announce(pool, key, OBJECTION_EVENT, &detail, None).await?;
    Ok(true)
}

/// The claim path: settle `target` at `now`, after its window, on live checks.
pub async fn settle_claim(
    pool: &SqlitePool,
    target: SettleTarget<'_>,
    now: DateTime<Utc>,
) -> Result<Settlement> {
    Settler { pool, stopper: None }.settle_claim(target, now).await
}

/// The claim path with a session stopper: the close stops the lane's session before its
/// worktree is removed.
#[derive(Clone, Copy)]
pub struct Settler<'a> {
    pub pool: &'a SqlitePool,
    pub stopper: Option<&'a dyn SessionStopper>,
}

impl Settler<'_> {
    /// Settles `target` at `now`, after its window, on live checks.
    pub async fn settle_claim(
        &self,
        target: SettleTarget<'_>,
        now: DateTime<Utc>,
    ) -> Result<Settlement> {
    let pool = self.pool;
    let key = target.key;
    let intent = close_intent(pool, key).await?;
    let row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT status, done_signals, claim_objection_until FROM lanes WHERE key = ?",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    let Some((status, done_signals, open_until)) = row else {
        return Err(HubError::message(format!("lane {key} not found")));
    };
    let recorded = open_until
        .as_deref()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|at| at.with_timezone(&Utc));
    if intent.is_some() {
        // A recorded close intent is finished, never re-decided.
        self.finish(key, true).await?;
        return Ok(Settlement::Settled {
            close_at: recorded.unwrap_or(now),
            resumed: true,
        });
    }
    if status != LaneStatus::DoneClaimed.as_str() {
        return Ok(Settlement::Objected {
            reasons: vec![format!("lane is {status}, not done_claimed")],
        });
    }
    // A claim recorded without a window (a crash between the status write and the claim, or an
    // older writer) gets its full window now: a claim never closes on the instant it arrived.
    let Some(close_at) = recorded else {
        let evidence: Option<String> =
            sqlx::query_scalar("SELECT claim_evidence FROM lanes WHERE key = ?")
                .bind(key)
                .fetch_optional(pool)
                .await?
                .flatten();
        let claim = claim_done(
            pool,
            ClaimRequest {
                target,
                evidence: evidence.as_deref().unwrap_or("a done claim"),
                advisory: None,
            },
            now,
        )
        .await?;
        return Ok(Settlement::Pending {
            close_at: claim.close_at,
        });
    };
    if now < close_at {
        return Ok(Settlement::Pending { close_at });
    }
    let signals = match parse_signals(&done_signals) {
        Ok(signals) => signals,
        Err(error) => {
            return object(pool, key, vec![format!("done signals unreadable: {error}")]).await;
        }
    };
    let reasons = live_reasons(pool, target, &signals).await?;
    if !reasons.is_empty() {
        return object(pool, key, reasons).await;
    }
    record_close_intent(pool, key, "done claim", close_at).await?;
    self.finish(key, false).await?;
    Ok(Settlement::Settled {
        close_at,
        resumed: false,
    })
    }

    async fn finish(&self, key: &str, resumed: bool) -> Result<()> {
        let request = CloseRequest {
            key,
            reason: SETTLED_REASON,
            resumed,
            stopper: self.stopper,
        };
        finish_close(self.pool, request).await
    }
}

/// The goal paths' entry point: the same live checks, run on the checks the goal's own contract
/// names. An empty result means the close may proceed; any reason is an automatic objection the
/// caller must not overrule. The caller records the close intent itself, in the transaction that
/// commits the close: recording it here would leave an intent behind for a transition that then
/// skipped.
pub async fn verify_before_close(
    pool: &SqlitePool,
    target: SettleTarget<'_>,
    verifiers: &[String],
) -> Result<Vec<String>> {
    if let Some(intent) = close_intent(pool, target.key).await? {
        // A close already decided is resumed, not re-decided.
        tracing::info!(lane = target.key, intent = %intent, "a recorded close intent is resumed");
        return Ok(Vec::new());
    }
    // A verifier Ferryx cannot run is not a check it can re-run here; the goal path already
    // refuses to grade such a contract, so it is not this gate's refusal.
    let signals: Vec<DoneSignal> = verifiers
        .iter()
        .filter_map(|verifier| crate::goals::evidence::verifier_signal(verifier))
        .collect();
    let reasons = live_reasons(pool, target, &signals).await?;
    if reasons.is_empty() {
        return Ok(Vec::new());
    }
    match object(pool, target.key, reasons).await? {
        Settlement::Objected { reasons } => Ok(reasons),
        other => Ok(vec![other.detail()]),
    }
}

/// Turns a failed check into the automatic objection the lane is told about.
async fn object(pool: &SqlitePool, key: &str, reasons: Vec<String>) -> Result<Settlement> {
    object_claim(pool, key, &reasons).await?;
    Ok(Settlement::Objected { reasons })
}

#[cfg(test)]
#[path = "settle_tests.rs"]
mod tests;
