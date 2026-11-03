//! Session recovery foundation (plan todo 4: D2, D4, G4, G5): the small seams the recovery
//! scheduler is built on, kept separate so each can be faked in tests without a wall clock,
//! a database or a daemon.
//!
//! - [`Clock`]: the time source; tests use a fake one and never sleep.
//! - [`PauseGate`]: whether the owner paused the supervisor. Recovery never acts while paused.
//! - [`RecoveryEscalations`]: where a session the supervisor cannot recover on its own is
//!   handed to the owner, and where that hand-off is withdrawn once the session is healthy.
//! - [`LoopRetry`]: how a goal-loop lane that was waiting on a failed worker is retried.
//! - [`RecoveryDeps`]: every collaborator the scheduler needs, as trait objects.
//! - [`fresh_lines`]: the terminal output that appeared between two tail snapshots.
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::autonomy::DecisionGate;
use super::panel_contract::{
    ActionLog, Escalation, HealthClassifier, RuleStore, SessionControl, SessionRef,
};
use crate::error::Result;
use crate::meta::mhc_supervisor::FleetSource;

#[cfg(test)]
mod tests;

/// Incident-key prefix of a goal pass deferred by the supervisor's own usage limit.
pub const GOAL_QUOTA_PREFIX: &str = "recovery:goalquota:";

mod scheduler;
pub use scheduler::{resume_for, RecoveryScheduler, Refusal, BUDGET_MS};

/// Wall clock in epoch milliseconds.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}

/// The real clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
}

/// Whether the owner paused the supervisor.
#[async_trait]
pub trait PauseGate: Send + Sync {
    /// `true` while paused. A pause flag that cannot be read counts as paused.
    async fn paused(&self) -> bool;
}

#[async_trait]
impl PauseGate for DecisionGate {
    async fn paused(&self) -> bool {
        DecisionGate::paused(self).await
    }
}

/// The owner-facing decisions recovery raises and withdraws.
pub trait RecoveryEscalations: Send + Sync {
    /// Raise `escalation` for its session, replacing a recovery escalation already open for it.
    fn open(&self, escalation: Escalation);
    /// Withdraw the recovery escalation of `session`, if one is open.
    fn resolve(&self, session: &SessionRef);
}

/// Retries a goal-loop lane whose worker was recovered.
#[async_trait]
pub trait LoopRetry: Send + Sync {
    async fn retry(&self, lane_key: &str) -> Result<()>;
}

/// Everything the recovery scheduler works with.
pub struct RecoveryDeps {
    pub fleet: Arc<dyn FleetSource>,
    pub control: Arc<dyn SessionControl>,
    pub rules: Arc<dyn RuleStore>,
    pub log: Arc<dyn ActionLog>,
    pub classifier: Arc<dyn HealthClassifier>,
    pub clock: Arc<dyn Clock>,
    pub pause: Arc<dyn PauseGate>,
    pub escalations: Arc<dyn RecoveryEscalations>,
}

/// The lines of `after` that `before` did not already contain, in the order of `after`.
/// Lines are compared as a multiset: a line that occurs twice in `after` and once in `before`
/// is reported once.
pub fn fresh_lines(before: &str, after: &str) -> Vec<String> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for line in before.lines() {
        *seen.entry(line).or_insert(0) += 1;
    }
    let mut fresh = Vec::new();
    for line in after.lines() {
        match seen.get_mut(line) {
            Some(count) if *count > 0 => *count -= 1,
            _ => fresh.push(line.to_owned()),
        }
    }
    fresh
}

#[cfg(test)]
mod fresh_tests {
    use super::fresh_lines;

    #[test]
    fn identical_snapshots_have_nothing_fresh() {
        assert!(fresh_lines("a\nb\n", "a\nb\n").is_empty());
        assert!(fresh_lines("", "").is_empty());
    }

    #[test]
    fn everything_is_fresh_after_an_empty_snapshot() {
        assert_eq!(fresh_lines("", "a\nb\n"), vec!["a".to_owned(), "b".to_owned()]);
    }

    #[test]
    fn new_lines_keep_the_order_of_after() {
        assert_eq!(
            fresh_lines("a\nc\n", "a\nb\nc\nd\n"),
            vec!["b".to_owned(), "d".to_owned()]
        );
    }

    #[test]
    fn repeated_lines_are_compared_as_a_multiset() {
        assert_eq!(
            fresh_lines("ok\n", "ok\nok\nok\n"),
            vec!["ok".to_owned(), "ok".to_owned()]
        );
        assert!(fresh_lines("ok\nok\n", "ok\n").is_empty());
    }

    #[test]
    fn scrolled_off_lines_are_not_reported() {
        assert_eq!(fresh_lines("a\nb\nc\n", "b\nc\nd\n"), vec!["d".to_owned()]);
    }
}
