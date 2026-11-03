//! The recovery scheduler: a per-incident state machine over the live fleet (plan todo 4,
//! D1/D2/D4/D10). Upstream/5xx errors are one incident per host and fault generation, retried
//! on the `upstreamRetry` backoff by one probe session and swept to the other members after
//! the first success. A usage limit waits for its reset. A stalled session is cancelled and
//! resumed once, then escalated. Every incident owns one action-log row edited in place.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex, PoisonError};
use std::time::Duration;

use tokio::sync::Mutex;

use super::{fresh_lines, LoopRetry, RecoveryDeps, RetryOutcome};
use crate::supervisor::health::reset_at_from_text;
use crate::supervisor::panel_contract::{
    default_rules, resume_text, ActionEntry, ActionKind, Escalation, EvidenceRef, FleetSession,
    HealthKind, PendingPrompt, RuleKind, SessionRef, StandingRule,
};

mod card;
mod goal;
mod restore;
mod tick;

/// Default retry budget of an upstream incident before it escalates (D1c), in ms. The
/// `upstreamRetry` rule's `budgetSecs` overrides it.
pub const BUDGET_MS: i64 = 20 * 60_000;
/// Default time a resume must stay fault-free before it counts as a recovery, in ms, which is
/// also how long a session may be missing from the fleet before it counts as gone. A spinner
/// or status line right after the send proves nothing: the next 502 usually arrives later.
/// The `upstreamRetry` rule's `confirmSecs` overrides it.
const CONFIRM_MS: i64 = 60_000;
const TAIL_LINES: usize = 40;
const DEFAULT_BACKOFF_SECS: [i64; 4] = [60, 120, 300, 600];
const DEFAULT_EVERY_SECS: i64 = 600;
const DEFAULT_QUOTA_SECS: i64 = 600;
const DEFAULT_MAX_RESTARTS: u64 = 1;
/// Default quota resumes that hit the limit again before the incident escalates
/// (`quotaResume` `maxAttempts`).
const QUOTA_ATTEMPTS: u32 = 3;
/// Default failed or re-limited goal-lane retries before the lane is given up on
/// (`quotaResume` `maxGoalAttempts`).
const GOAL_ATTEMPTS: u32 = 5;
/// Prefix of a recovery card's prompt id; the rest is the incident key.
pub const CARD_PREFIX: &str = "recovery:";

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

/// What an owner's tap on a recovery card does, whatever the card's wording.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Choice {
    /// Keep going with a fresh budget.
    KeepGoing,
    /// Stop the sessions of the incident that are not already resumed.
    Stop,
    /// Leave the sessions alone and close the incident.
    Leave,
}

/// Why an owner's tap on a recovery card did nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardError {
    /// No open recovery card has that id (answered, resolved, or from before a restart).
    NotOpen,
    /// The card has no such option.
    BadOption,
    /// The option needed a session operation that failed; the card stays open.
    Failed(String),
}

impl std::fmt::Display for CardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotOpen => write!(f, "that recovery card is no longer open"),
            Self::BadOption => write!(f, "the recovery card has no such option"),
            Self::Failed(why) => write!(f, "the choice could not be carried out: {why}"),
        }
    }
}

impl std::error::Error for CardError {}

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
    title: String,
    started_ms: i64,
    /// The retry budget runs from here; an owner's "keep going" moves it.
    budget_from_ms: i64,
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
    /// Confirmed members that faulted again, with the time they may be retried.
    hold: HashMap<SessionRef, i64>,
    /// Ctrl-C was delivered but the resume text was not: only the text is retried (L3).
    stopped_pending: bool,
    /// Sessions already interrupted by an owner's "stop" tap; never interrupted twice (L3).
    stopped: HashSet<SessionRef>,
    /// What each option of the open card does, by index.
    choices: Vec<Choice>,
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
    /// Since when a tracked session has been missing from the fleet (L2).
    absent_since: HashMap<SessionRef, i64>,
    /// Sessions the owner told to leave alone or stop; cleared once healthy or gone.
    dismissed: HashSet<SessionRef>,
    /// Unknown-agent sessions with their open refusal incident (row and card).
    refused: HashMap<SessionRef, Incident>,
    /// The rows of an earlier run were read (M1).
    restored: bool,
    /// Fault generation per scope; bumped when an incident closes.
    gens: HashMap<String, u32>,
}

struct GoalWait {
    at_ms: i64,
    started_ms: i64,
    session: Option<SessionRef>,
    /// Retries that failed or hit the limit again; bounded by `maxGoalAttempts` (L4).
    attempts: u32,
    /// The lane was reported again while its retry ran.
    requeued: bool,
    /// A retry of this lane is running right now.
    in_flight: bool,
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

fn positive(rules: &[StandingRule], kind: RuleKind, key: &str) -> Option<u64> {
    rule_param(rules, kind, key)
        .and_then(|v| v.as_u64())
        .filter(|n| *n > 0)
}

fn secs_to_ms(secs: u64) -> i64 {
    i64::try_from(secs).unwrap_or(i64::MAX / 1000).saturating_mul(1000)
}

fn budget_ms(rules: &[StandingRule]) -> i64 {
    positive(rules, RuleKind::UpstreamRetry, "budgetSecs").map_or(BUDGET_MS, secs_to_ms)
}

fn confirm_ms(rules: &[StandingRule]) -> i64 {
    positive(rules, RuleKind::UpstreamRetry, "confirmSecs").map_or(CONFIRM_MS, secs_to_ms)
}

fn every_secs(rules: &[StandingRule]) -> i64 {
    rule_param(rules, RuleKind::UpstreamRetry, "thenEverySecs")
        .and_then(|v| v.as_i64())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_EVERY_SECS)
}

fn quota_attempts(rules: &[StandingRule]) -> u32 {
    positive(rules, RuleKind::QuotaResume, "maxAttempts")
        .map_or(QUOTA_ATTEMPTS, |n| u32::try_from(n).unwrap_or(u32::MAX))
}

fn goal_attempts(rules: &[StandingRule]) -> u32 {
    positive(rules, RuleKind::QuotaResume, "maxGoalAttempts")
        .map_or(GOAL_ATTEMPTS, |n| u32::try_from(n).unwrap_or(u32::MAX))
}

/// The text sent to resume `agent`: a rule's `resumeText` when one sets it, else the agent's
/// built-in text. `None` for an unknown agent: a rule never makes recovery guess.
fn resume_text_for(rules: &[StandingRule], agent: &str) -> Option<String> {
    let builtin = resume_for(agent).ok()?;
    let text = [RuleKind::UpstreamRetry, RuleKind::StallRestart, RuleKind::QuotaResume]
        .into_iter()
        .find_map(|kind| rule_param(rules, kind, "resumeText").and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map_or_else(|| builtin.to_owned(), str::to_owned);
    Some(text)
}

/// Whole minutes of `ms` (at least one), for owner-facing text.
fn minutes(ms: i64) -> i64 {
    (ms / 60_000).max(1)
}

fn max_restarts(rules: &[StandingRule]) -> u64 {
    rule_param(rules, RuleKind::StallRestart, "maxRestarts")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_MAX_RESTARTS)
}

fn stall_threshold_ms(rules: &[StandingRule]) -> i64 {
    rule_param(rules, RuleKind::StallRestart, "thresholdSecs")
        .and_then(|v| v.as_i64())
        .filter(|s| *s > 0)
        .unwrap_or(300)
        * 1000
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
