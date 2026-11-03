//! The recovery scheduler: a per-incident state machine over the live fleet (plan todo 4,
//! D1/D2/D4/D10). Upstream/5xx errors are one incident per host and fault generation, retried
//! on the `upstreamRetry` backoff by one probe session and swept to the other members after
//! the first success. A usage limit waits for its reset. A stalled session is cancelled and
//! resumed once, then escalated. Every incident owns one action-log row edited in place.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex, PoisonError};
use std::time::Duration;

use tokio::sync::Mutex;

use super::{fresh_lines, LoopRetry, RecoveryDeps};
use crate::supervisor::health::reset_at_from_text;
use crate::supervisor::panel_contract::{
    default_rules, resume_text, ActionEntry, ActionKind, Escalation, EvidenceRef, FleetSession,
    HealthKind, PendingPrompt, RuleKind, SessionRef, StandingRule,
};

mod goal;
mod tick;

/// Retry budget of an upstream incident before it escalates (D1c), in ms.
pub const BUDGET_MS: i64 = 20 * 60_000;
/// How long a sent resume may take to show output before it counts as no progress, in ms.
const GRACE_MS: i64 = 30_000;
const TAIL_LINES: usize = 40;
const DEFAULT_BACKOFF_SECS: [i64; 4] = [60, 120, 300, 600];
const DEFAULT_EVERY_SECS: i64 = 600;
const DEFAULT_QUOTA_SECS: i64 = 600;
const DEFAULT_MAX_RESTARTS: u64 = 1;
/// Quota resumes that hit the limit again before the incident escalates.
const QUOTA_ATTEMPTS: u32 = 3;

/// Why recovery will not act on a session. Never a guessed resume key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    UnknownAgent { agent: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownAgent { agent } => write!(f, "no known resume text for agent {agent}"),
        }
    }
}

/// The per-agent resume text, or a typed refusal.
pub fn resume_for(agent: &str) -> Result<&'static str, Refusal> {
    resume_text(agent).ok_or_else(|| Refusal::UnknownAgent {
        agent: agent.to_owned(),
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    Upstream,
    Quota,
    Stall,
}

struct Incident {
    key: String,
    scope: String,
    fault: Fault,
    host: String,
    primary: SessionRef,
    started_ms: i64,
    attempts: u32,
    next_at_ms: i64,
    succeeded: bool,
    closed: bool,
    escalated: Option<SessionRef>,
    ever: HashSet<SessionRef>,
    done: HashSet<SessionRef>,
    last_row: String,
    /// Sessions sent the resume text that then showed no output at all: neither confirmed nor
    /// failed, so never counted as resumed (H3). Retried on the backoff like a failed probe.
    silent: HashSet<SessionRef>,
    /// Stall restarts sent in this incident and the time of the last one. Per incident, so a
    /// later, distinct stall of the same session starts with a fresh budget (H1).
    restarts: u64,
    last_restart_ms: i64,
}

#[derive(Default)]
struct State {
    incidents: Vec<Incident>,
    /// Sessions whose resume awaits judgement, with the send time.
    sent: HashMap<SessionRef, i64>,
    /// The terminal tail at the last resume: only lines after it count as news.
    base: HashMap<SessionRef, String>,
    /// Every session in the fleet this pass, readable or not.
    present: HashSet<SessionRef>,
    /// Sessions whose terminal could not be read this pass: not vanished, not recovered.
    unreadable: HashSet<SessionRef>,
    refused: HashSet<SessionRef>,
    /// Fault generation per scope; bumped when an incident closes.
    gens: HashMap<String, u32>,
}

struct GoalWait {
    at_ms: i64,
    started_ms: i64,
    session: Option<SessionRef>,
}

/// Drives recovery. `tick` is one deterministic pass; `run` repeats it.
pub struct RecoveryScheduler {
    deps: RecoveryDeps,
    state: Mutex<State>,
    goals: StdMutex<HashMap<String, GoalWait>>,
    retry: StdMutex<Option<Arc<dyn LoopRetry>>>,
}

fn rule_param<'a>(
    rules: &'a [StandingRule],
    kind: RuleKind,
    key: &str,
) -> Option<&'a serde_json::Value> {
    rules
        .iter()
        .find(|r| r.kind == kind)
        .and_then(|r| r.params.get(key))
}

/// Delay before send number `attempt` (0-based) of an upstream incident.
fn backoff_ms(rules: &[StandingRule], attempt: usize) -> i64 {
    let steps: Vec<i64> = rule_param(rules, RuleKind::UpstreamRetry, "backoffSecs")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_i64())
                .filter(|s| *s > 0)
                .collect()
        })
        .unwrap_or_else(|| DEFAULT_BACKOFF_SECS.to_vec());
    let every = rule_param(rules, RuleKind::UpstreamRetry, "thenEverySecs")
        .and_then(|v| v.as_i64())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_EVERY_SECS);
    steps.get(attempt).copied().unwrap_or(every) * 1000
}

fn quota_ms(rules: &[StandingRule]) -> i64 {
    rule_param(rules, RuleKind::QuotaResume, "fallbackSecs")
        .and_then(|v| v.as_i64())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_QUOTA_SECS)
        * 1000
}

fn max_restarts(rules: &[StandingRule]) -> u64 {
    rule_param(rules, RuleKind::StallRestart, "maxRestarts")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_MAX_RESTARTS)
}

fn sessions(n: usize) -> String {
    if n == 1 {
        "1 session".to_owned()
    } else {
        format!("{n} sessions")
    }
}

impl RecoveryScheduler {
    pub fn new(deps: RecoveryDeps) -> Self {
        Self {
            deps,
            state: Mutex::new(State::default()),
            goals: StdMutex::new(HashMap::new()),
            retry: StdMutex::new(None),
        }
    }

    /// Installs the goal-lane retry used for deferred supervisor quota faults.
    pub fn set_loop_retry(&self, retry: Arc<dyn LoopRetry>) {
        *self.retry.lock().unwrap_or_else(PoisonError::into_inner) = Some(retry);
    }

    /// Runs `tick` every `every` until the task is dropped.
    pub async fn run(&self, every: Duration) {
        loop {
            self.tick().await;
            tokio::time::sleep(every).await;
        }
    }

    pub(super) async fn rules(&self) -> Vec<StandingRule> {
        self.deps
            .rules
            .list()
            .await
            .unwrap_or_else(|_| default_rules())
    }

    /// `false` while the owner has paused the supervisor: no autonomous action may follow.
    pub(super) async fn go(&self) -> bool {
        !self.deps.pause.paused().await
    }
}
