//! Quiet-lane oversight (todo 7): per-backend capabilities, one deadline per lane, reminders at
//! 10 and 20 quiet minutes, one stall alert to the lead at 40, and per-run ceilings that post an
//! obstacle report and hold the lane through [`crate::supervisor::hold::hold_lane`].
//!
//! "Quiet" is measured from the run's last new evidence ([`super::evidence`]), never from report
//! text or screen output. A backend whose idle/working state or hold is unknown is observe-only:
//! it gets no reminder, alert or hold, and its lane status says so.
use serde::Serialize;

mod store;
mod tick;
pub use store::{ensure_run_in, note_report_in, record_usage, start_run, status, totals, Row};
pub use tick::{
    serve, FleetLaneStates, LaneRef, LaneStates, Oversight, OversightEffects, PaneEffects,
};

#[cfg(test)]
mod tests;

const MINUTE_MS: i64 = 60_000;
/// Quiet minutes of the first and second reminder.
pub const REMINDER_MINUTES: [i64; 2] = [10, 20];
/// Quiet minutes of the single stall alert to the lead.
pub const STALL_MINUTES: i64 = 40;
/// Wall ceiling of a run when none is set at open.
pub const DEFAULT_WALL_MINUTES: i64 = 240;
/// `lane_events` kinds this module writes for the lead.
pub const STALL_EVENT: &str = "lane_stall";
pub const OBSTACLE_EVENT: &str = "lane_obstacle";

static WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();

/// A report or state change recomputes deadlines now rather than at the next poll.
pub fn poke() {
    WAKE.notify_one();
}

/// The worker backend of a lane, as far as the hub can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// A local Ferryx pane running mhc.
    MhcPane,
    /// A local Ferryx pane running Claude Code.
    Claude,
    /// A local Ferryx pane running Codex.
    Codex,
    /// A pane on a paired host.
    PairedHost,
    /// The mhc engine's native goal (supervisor `mhc` binding).
    MhcEngine,
    Unknown,
}

/// What the hub can do with a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// The worker can call hub tools (`lane_report`) from its own authenticated connection.
    pub hub_tools: bool,
    /// The hub reads a trustworthy idle/working state for it.
    pub idle_working: bool,
    /// The hub can hold it and read the hold back.
    pub hold: bool,
}

impl Backend {
    /// The capability matrix. Local panes are held by the hub-loop fence (`lane_pauses`), which
    /// only fences this machine's panes; a paired host pane and the mhc engine's native goal have
    /// no confirmed hold today (`hold::confirm` reports `HOLD_UNCONFIRMED` for the latter).
    pub const fn capabilities(self) -> Capabilities {
        match self {
            Self::MhcPane | Self::Claude | Self::Codex => {
                Capabilities { hub_tools: true, idle_working: true, hold: true }
            }
            Self::PairedHost => Capabilities { hub_tools: false, idle_working: true, hold: false },
            Self::MhcEngine | Self::Unknown => {
                Capabilities { hub_tools: false, idle_working: false, hold: false }
            }
        }
    }

    /// Reminders, stall alerts and ceiling holds apply only here.
    pub const fn active(self) -> bool {
        let caps = self.capabilities();
        caps.idle_working && caps.hold
    }

    /// The backend of a pane from its fleet `agent` and host.
    pub fn of_pane(agent: &str, local: bool) -> Self {
        if !local {
            return Self::PairedHost;
        }
        match agent.trim().to_ascii_lowercase().as_str() {
            "mhc" | "maho" | "omp" => Self::MhcPane,
            "claude" => Self::Claude,
            "codex" => Self::Codex,
            _ => Self::Unknown,
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "mhc_pane" => Self::MhcPane,
            "claude" => Self::Claude,
            "codex" => Self::Codex,
            "paired_host" => Self::PairedHost,
            "mhc_engine" => Self::MhcEngine,
            _ => Self::Unknown,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MhcPane => "mhc_pane",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::PairedHost => "paired_host",
            Self::MhcEngine => "mhc_engine",
            Self::Unknown => "unknown",
        }
    }
}

/// The worker's state as read this pass. `Unknown` never counts as idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Working,
    Idle,
    /// The agent shows a question for the owner.
    AwaitingOwner,
    Unknown,
}

impl AgentState {
    pub fn of_pane(state: &str) -> Self {
        match state {
            "working" => Self::Working,
            "idle" => Self::Idle,
            "blocked" | "question" => Self::AwaitingOwner,
            _ => Self::Unknown,
        }
    }
}

/// Ceilings of one run. `None` is off. Set at open; the wall ceiling defaults to 240 minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ceilings {
    pub wall_minutes: Option<i64>,
    pub tokens: Option<i64>,
    pub cost_micros: Option<i64>,
}

impl Default for Ceilings {
    fn default() -> Self {
        Self { wall_minutes: Some(DEFAULT_WALL_MINUTES), tokens: None, cost_micros: None }
    }
}

/// A usage total of a run. A run with no usage record, or any record that left it out, is
/// `Unknown`: never reported as under a ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Total {
    Known(i64),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    pub tokens: Total,
    pub cost_micros: Total,
}

/// The persisted oversight clock of one lane run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub run_started_at_ms: i64,
    pub last_progress_at_ms: i64,
    pub reminders_sent: i64,
    pub stall_alerted: bool,
    pub ceiling_reported: bool,
    pub waiting_on_owner: bool,
    pub ceilings: Ceilings,
}

/// One thing that is due now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Due {
    /// Reminder at this many quiet minutes.
    Remind(i64),
    StallAlert,
    /// The named ceiling was reached.
    Ceiling(String),
}

/// What is due for a lane at `now`. Pure: no IO, no clock.
pub fn decide(s: &Snapshot, backend: Backend, state: AgentState, totals: &Totals, now: i64) -> Vec<Due> {
    if !backend.active() {
        return Vec::new();
    }
    if !s.ceiling_reported {
        if let Some(hit) = ceiling_hit(s, totals, now) {
            return vec![Due::Ceiling(hit)];
        }
    }
    let quiet = now.saturating_sub(s.last_progress_at_ms);
    let waiting = s.waiting_on_owner || state == AgentState::AwaitingOwner;
    let mut due = Vec::new();
    let next = usize::try_from(s.reminders_sent).ok().and_then(|sent| REMINDER_MINUTES.get(sent));
    if let Some(&minutes) = next {
        if state == AgentState::Idle && !waiting && quiet >= minutes * MINUTE_MS {
            due.push(Due::Remind(minutes));
        }
    }
    if !s.stall_alerted && !waiting && quiet >= STALL_MINUTES * MINUTE_MS {
        due.push(Due::StallAlert);
    }
    due
}

fn ceiling_hit(s: &Snapshot, totals: &Totals, now: i64) -> Option<String> {
    let used_ms = now.saturating_sub(s.run_started_at_ms);
    if let Some(wall) = s.ceilings.wall_minutes.filter(|wall| used_ms >= wall * MINUTE_MS) {
        return Some(format!("wall ceiling of {wall} minutes reached"));
    }
    if let (Some(limit), Total::Known(used)) = (s.ceilings.tokens, totals.tokens) {
        if used >= limit {
            return Some(format!("token ceiling of {limit} reached ({used} used)"));
        }
    }
    if let (Some(limit), Total::Known(used)) = (s.ceilings.cost_micros, totals.cost_micros) {
        if used >= limit {
            return Some(format!("cost ceiling of {limit} micro-USD reached ({used} used)"));
        }
    }
    None
}

/// The next instant anything of `s` can become due, strictly after `now`. A due item that its
/// agent state blocked is re-evaluated on the next state change or poll, not here.
pub fn deadline(s: &Snapshot, backend: Backend, now: i64) -> Option<i64> {
    if !backend.active() {
        return None;
    }
    let mut at = Vec::with_capacity(3);
    if let Some(&minutes) = usize::try_from(s.reminders_sent).ok().and_then(|sent| REMINDER_MINUTES.get(sent)) {
        at.push(s.last_progress_at_ms + minutes * MINUTE_MS);
    }
    if !s.stall_alerted {
        at.push(s.last_progress_at_ms + STALL_MINUTES * MINUTE_MS);
    }
    if let (false, Some(wall)) = (s.ceiling_reported, s.ceilings.wall_minutes) {
        at.push(s.run_started_at_ms + wall * MINUTE_MS);
    }
    at.into_iter().filter(|at| *at > now).min()
}

/// The reminder sent to an idle worker.
pub fn reminder_text(lane: &str, minutes: i64) -> String {
    format!(
        "Lane {lane}: no new evidence for {minutes} minutes. Call lane_report with the files you changed \
         or a pushed commit / PR receipt. A report without new evidence does not reset the clock."
    )
}

/// A ceiling or unknown usage as shown in the lane status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum CeilingView {
    Off,
    Unknown { limit: i64 },
    Known { used: i64, limit: i64 },
}

/// The oversight part of a lane's status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OversightStatus {
    pub backend: Backend,
    /// `active` or `observe_only`.
    pub mode: &'static str,
    pub note: String,
    pub run: i64,
    pub last_progress_at_ms: i64,
    pub reminders_sent: i64,
    pub deadline_at_ms: Option<i64>,
    pub wall: CeilingView,
    pub tokens: CeilingView,
    pub cost_micros: CeilingView,
}

fn view(limit: Option<i64>, total: Total) -> CeilingView {
    match (limit, total) {
        (None, _) => CeilingView::Off,
        (Some(limit), Total::Unknown) => CeilingView::Unknown { limit },
        (Some(limit), Total::Known(used)) => CeilingView::Known { used, limit },
    }
}

/// Builds the status from a persisted row.
pub fn status_of(row: &Row, totals: &Totals, now: i64) -> OversightStatus {
    let backend = row.backend;
    let (mode, note) = if backend.active() {
        ("active", "reminders, stall alert and ceiling hold apply".to_owned())
    } else {
        ("observe_only", format!(
            "observe-only: backend {} has no confirmed idle/working state and hold; no reminders, alerts or holds",
            backend.as_str()
        ))
    };
    let used_minutes = now.saturating_sub(row.snapshot.run_started_at_ms) / MINUTE_MS;
    OversightStatus {
        backend,
        mode,
        note,
        run: row.run,
        last_progress_at_ms: row.snapshot.last_progress_at_ms,
        reminders_sent: row.snapshot.reminders_sent,
        deadline_at_ms: row.deadline_at_ms,
        wall: view(row.snapshot.ceilings.wall_minutes, Total::Known(used_minutes)),
        tokens: view(row.snapshot.ceilings.tokens, totals.tokens),
        cost_micros: view(row.snapshot.ceilings.cost_micros, totals.cost_micros),
    }
}
