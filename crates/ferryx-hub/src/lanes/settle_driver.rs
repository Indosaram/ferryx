//! The due-claim driver: settles a done claim when its objection window closes, for every lane,
//! with or without a supervisor goal. It reads the one earliest due deadline from the lane table
//! (`ix_lanes_objection_due`), sleeps until it, and is woken early by a new claim; it never polls.
//! Also the startup resume of closes a crash interrupted.
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use tokio::sync::Notify;

use super::settle::{Settler, SettleTarget, SETTLED_REASON};
use super::settle_close::{close_intent, finish_close, CloseRequest};
use super::settle_stop::SessionStopper;
use crate::error::Result;

/// How long the driver waits after a failed pass before reading the table again.
const RETRY_AFTER: Duration = Duration::from_secs(30);

/// Woken by every recorded claim, so a deadline earlier than the one slept on is picked up.
pub fn due_signal() -> Arc<Notify> {
    static SIGNAL: OnceLock<Arc<Notify>> = OnceLock::new();
    Arc::clone(SIGNAL.get_or_init(|| Arc::new(Notify::new())))
}

/// The claim whose window closes first: lane key, working directory, deadline. An unreadable
/// deadline is due now, so the settlement re-opens a full window for it.
async fn next_due(pool: &SqlitePool, now: DateTime<Utc>) -> Result<Option<(String, String, DateTime<Utc>)>> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT key, cwd, claim_objection_until FROM lanes
          WHERE status = 'done_claimed' AND claim_objection_until IS NOT NULL
            AND close_intent_json IS NULL
          ORDER BY claim_objection_until ASC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(key, cwd, until)| {
        let due = DateTime::parse_from_rfc3339(&until).map_or(now, |at| at.with_timezone(&Utc));
        (key, cwd, due)
    }))
}

pub struct DueDriver {
    pub pool: SqlitePool,
    pub stopper: Option<Arc<dyn SessionStopper>>,
    /// The wall clock; a test substitutes one that follows tokio's paused time.
    pub clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    /// Notified each time the driver parks on a deadline.
    pub armed: Arc<Notify>,
    /// Notified after each settlement pass.
    pub settled: Arc<Notify>,
}

impl DueDriver {
    pub fn new(pool: SqlitePool, stopper: Option<Arc<dyn SessionStopper>>) -> Self {
        Self {
            pool,
            stopper,
            clock: Arc::new(Utc::now),
            armed: Arc::new(Notify::new()),
            settled: Arc::new(Notify::new()),
        }
    }

    async fn retry_later(wake: &Notify) {
        tokio::select! {
            () = tokio::time::sleep(RETRY_AFTER) => {}
            () = wake.notified() => {}
        }
    }

    /// Runs until dropped: settle what is due, sleep until the next deadline or a new claim.
    pub async fn run(self, wake: Arc<Notify>) {
        let mut last: Option<(String, DateTime<Utc>)> = None;
        loop {
            let now = (self.clock)();
            let next = match next_due(&self.pool, now).await {
                Ok(next) => next,
                Err(error) => {
                    tracing::warn!(%error, "the next claim deadline was not read");
                    Self::retry_later(&wake).await;
                    continue;
                }
            };
            let Some((key, cwd, due)) = next else {
                wake.notified().await;
                continue;
            };
            if due > now {
                let wait = (due - now).to_std().unwrap_or_default();
                self.armed.notify_one();
                tokio::select! {
                    () = tokio::time::sleep_until(tokio::time::Instant::now() + wait) => {}
                    () = wake.notified() => {}
                }
                continue;
            }
            // A claim still due after its own pass is not progressing: do not spin on it.
            if last.as_ref() == Some(&(key.clone(), due)) {
                Self::retry_later(&wake).await;
            }
            last = Some((key.clone(), due));
            let settler = Settler {
                pool: &self.pool,
                stopper: self.stopper.as_deref(),
            };
            match settler
                .settle_claim(SettleTarget { key: &key, cwd: &cwd }, (self.clock)())
                .await
            {
                Ok(settlement) => tracing::info!(
                    lane = %key,
                    passed = settlement.passed(),
                    detail = %settlement.detail(),
                    "a claim's objection window closed"
                ),
                Err(error) => tracing::warn!(lane = %key, %error, "a claim was not settled"),
            }
            self.settled.notify_one();
        }
    }
}

/// Finishes every live lane that holds a current close intent, without re-running any check: a
/// crash between a goal's committed completion and the lane's own transition resumes here.
/// Returns how many closes were finished; one that fails is logged and left for the next start.
pub async fn resume_recorded_close_intents(
    pool: &SqlitePool,
    stopper: Option<&dyn SessionStopper>,
) -> Result<u64> {
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT key FROM lanes WHERE close_intent_json IS NOT NULL
            AND status IN ('working','blocked','not_done','done_claimed')",
    )
    .fetch_all(pool)
    .await?;
    let mut resumed = 0;
    for key in keys {
        if close_intent(pool, &key).await?.is_none() {
            continue;
        }
        let request = CloseRequest {
            key: &key,
            reason: SETTLED_REASON,
            resumed: true,
            stopper,
        };
        match finish_close(pool, request).await {
            Ok(()) => resumed += 1,
            Err(error) => tracing::warn!(lane = %key, %error, "a recorded close was not resumed"),
        }
    }
    Ok(resumed)
}

#[cfg(test)]
#[path = "settle_driver_tests.rs"]
mod tests;
