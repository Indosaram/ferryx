//! Pure, IO-free decision logic for supervisor-owned agent sessions.
//!
//! This module answers one question: given a snapshot of an owned session,
//! what (if anything) should the supervisor do right now? It performs no
//! IO, reads no clocks, and mutates nothing, so it can be exercised in
//! isolation and reasoned about deterministically.

/// What the supervisor believes an owned agent session is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// No reliable information yet.
    Unknown,
    /// The agent is actively working on the goal.
    Working,
    /// The agent is blocked waiting for a prompt/answer from us.
    PromptPending,
    /// The agent finished its turn and is waiting for input.
    Idle,
}

/// A read-only snapshot of an owned session as seen by the supervisor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedSessionView {
    /// Agent kind: "claude", "codex", "omp", ...
    pub agent: String,
    pub state: SessionState,
    pub paused: bool,
    /// Unix seconds of the last progress report.
    pub last_report_at: Option<i64>,
    /// Unix seconds of the last observed activity of any kind.
    pub last_activity_at: i64,
    /// Unix seconds of the last nudge we sent.
    pub nudged_at: Option<i64>,
    /// Whether a stall escalation was already emitted for this silence.
    pub stall_escalated: bool,
    pub turns_used: u32,
    pub turn_limit: u32,
}

/// The decision produced by [`next_action`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipAction {
    /// Leave the session alone.
    Nothing,
    /// Send a nudge asking for a progress report.
    Nudge(String),
    /// Escalate a stall to a human/operator.
    EscalateStall(String),
    /// Tell the agent to keep going.
    Continue(String),
    /// The session exhausted its turn budget; stop it.
    StopTurnLimit,
}

/// Default nudge threshold in minutes.
pub const DEFAULT_NUDGE_AFTER_MIN: i64 = 15;

/// Nudge threshold in seconds.
///
/// Reads `FERRYX_HUB_NUDGE_AFTER_MIN`; falls back to
/// [`DEFAULT_NUDGE_AFTER_MIN`] when unset, non-positive, or unparsable.
pub fn nudge_after_secs() -> i64 {
    let minutes = match std::env::var("FERRYX_HUB_NUDGE_AFTER_MIN") {
        Ok(raw) => raw.trim().parse::<i64>().ok().filter(|m| *m > 0),
        Err(_) => None,
    }
    .unwrap_or(DEFAULT_NUDGE_AFTER_MIN);
    minutes.saturating_mul(60)
}

/// Whether `text` is safe to type into an agent session.
///
/// Backticks and `$(` would be interpreted by the terminal/shell, so we
/// refuse them.
pub fn safe_to_send(text: &str) -> bool {
    !text.contains('`') && !text.contains("$(")
}

const NUDGE_TEXT: &str = "Please report progress on the goal.";
const CONTINUE_TEXT: &str = "Continue with the goal.";

fn stall_text(agent: &str, silent_secs: i64) -> String {
    format!(
        "session {agent} has been idle for {silent_secs}s without a report; investigate"
    )
}

/// Decide what to do with the owned session `v` at time `now`.
///
/// `nudge_after` is the silence threshold in seconds (see
/// [`nudge_after_secs`]).
pub fn next_action(v: &OwnedSessionView, now: i64, nudge_after: i64) -> OwnershipAction {
    let action = decide(v, now, nudge_after);
    match &action {
        OwnershipAction::Nudge(t)
        | OwnershipAction::EscalateStall(t)
        | OwnershipAction::Continue(t) => {
            debug_assert!(safe_to_send(t), "unsafe supervisor text: {t:?}");
        }
        OwnershipAction::Nothing | OwnershipAction::StopTurnLimit => {}
    }
    action
}

fn decide(v: &OwnedSessionView, now: i64, nudge_after: i64) -> OwnershipAction {
    if v.paused || matches!(v.state, SessionState::Unknown | SessionState::PromptPending) {
        return OwnershipAction::Nothing;
    }

    if v.turns_used >= v.turn_limit {
        return OwnershipAction::StopTurnLimit;
    }

    if v.state == SessionState::Working {
        return OwnershipAction::Nothing;
    }

    // From here on the session is Idle.
    let silent_secs = now.saturating_sub(v.last_activity_at);
    let silent = silent_secs >= nudge_after;

    // A report that arrived after our last nudge means the agent is alive
    // and responsive again.
    let fresh_report = match (v.last_report_at, v.nudged_at) {
        (Some(r), Some(n)) => r > n,
        (Some(_), None) => true,
        (None, _) => false,
    };

    if silent {
        if v.nudged_at.is_none() || fresh_report {
            return OwnershipAction::Nudge(NUDGE_TEXT.to_string());
        }
        let since_nudge = now.saturating_sub(v.nudged_at.unwrap_or(now));
        if !v.stall_escalated && since_nudge >= nudge_after {
            return OwnershipAction::EscalateStall(stall_text(&v.agent, silent_secs));
        }
        return OwnershipAction::Nothing;
    }

    if fresh_report {
        return OwnershipAction::Continue(CONTINUE_TEXT.to_string());
    }

    OwnershipAction::Nothing
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;
    const NUDGE: i64 = DEFAULT_NUDGE_AFTER_MIN * 60;

    fn view(state: SessionState) -> OwnedSessionView {
        OwnedSessionView {
            agent: "claude".to_string(),
            state,
            paused: false,
            last_report_at: None,
            last_activity_at: NOW,
            nudged_at: None,
            stall_escalated: false,
            turns_used: 0,
            turn_limit: 100,
        }
    }

    fn idle() -> OwnedSessionView {
        view(SessionState::Idle)
    }

    #[test]
    fn nudge_after_secs_defaults_to_fifteen_minutes() {
        // Env may be set in the harness; only assert the default constant.
        assert_eq!(DEFAULT_NUDGE_AFTER_MIN, 15);
        let secs = nudge_after_secs();
        assert!(secs >= 60, "nudge threshold must be positive, got {secs}");
        if std::env::var("FERRYX_HUB_NUDGE_AFTER_MIN").is_err() {
            assert_eq!(secs, 15 * 60);
        }
    }

    #[test]
    fn nudge_fires_once_after_fifteen_minutes() {
        let mut v = idle();
        v.last_activity_at = NOW - NUDGE;

        assert_eq!(
            next_action(&v, NOW, NUDGE),
            OwnershipAction::Nudge("Please report progress on the goal.".to_string())
        );

        // Once nudged, the same silence does not nudge again.
        v.nudged_at = Some(NOW);
        assert_eq!(next_action(&v, NOW + 1, NUDGE), OwnershipAction::Nothing);
    }

    #[test]
    fn no_second_nudge_until_a_new_report() {
        let mut v = idle();
        v.last_activity_at = NOW - NUDGE - 60;
        v.nudged_at = Some(NOW - 60);

        // Silence continues and the nudge is recent: neither re-nudge nor
        // escalation yet.
        assert_eq!(next_action(&v, NOW, NUDGE), OwnershipAction::Nothing);
    }

    #[test]
    fn a_new_report_rearms_the_nudge() {
        let mut v = idle();
        v.last_activity_at = NOW - 2 * NUDGE;
        v.nudged_at = Some(NOW - NUDGE);
        v.last_report_at = Some(NOW - NUDGE + 1);

        assert_eq!(
            next_action(&v, NOW, NUDGE),
            OwnershipAction::Nudge("Please report progress on the goal.".to_string())
        );
    }

    #[test]
    fn nothing_on_unknown_state() {
        let mut v = view(SessionState::Unknown);
        v.last_activity_at = NOW - 10 * NUDGE;
        assert_eq!(next_action(&v, NOW, NUDGE), OwnershipAction::Nothing);
    }

    #[test]
    fn nothing_on_pending_prompt() {
        let mut v = view(SessionState::PromptPending);
        v.last_activity_at = NOW - 10 * NUDGE;
        assert_eq!(next_action(&v, NOW, NUDGE), OwnershipAction::Nothing);
    }

    #[test]
    fn nothing_while_paused() {
        let mut v = idle();
        v.paused = true;
        v.last_activity_at = NOW - 10 * NUDGE;
        v.turns_used = 100;
        assert_eq!(next_action(&v, NOW, NUDGE), OwnershipAction::Nothing);
    }

    #[test]
    fn stall_escalates_once() {
        let mut v = idle();
        v.last_activity_at = NOW - 3 * NUDGE;
        v.nudged_at = Some(NOW - 2 * NUDGE);

        match next_action(&v, NOW, NUDGE) {
            OwnershipAction::EscalateStall(text) => assert!(safe_to_send(&text)),
            other => panic!("expected EscalateStall, got {other:?}"),
        }

        v.stall_escalated = true;
        assert_eq!(next_action(&v, NOW + NUDGE, NUDGE), OwnershipAction::Nothing);
    }

    fn continuation_stops_at_turn_limit(agent: &str) {
        let mut v = idle();
        v.agent = agent.to_string();
        v.last_report_at = Some(NOW - 1);
        v.nudged_at = Some(NOW - 2);
        v.turns_used = 3;
        v.turn_limit = 10;

        assert_eq!(
            next_action(&v, NOW, NUDGE),
            OwnershipAction::Continue("Continue with the goal.".to_string())
        );

        v.turns_used = 10;
        assert_eq!(next_action(&v, NOW, NUDGE), OwnershipAction::StopTurnLimit);
    }

    #[test]
    fn continuation_stops_at_turn_limit_for_claude() {
        continuation_stops_at_turn_limit("claude");
    }

    #[test]
    fn continuation_stops_at_turn_limit_for_codex() {
        continuation_stops_at_turn_limit("codex");
    }

    #[test]
    fn continuation_stops_at_turn_limit_for_omp() {
        continuation_stops_at_turn_limit("omp");
    }

    #[test]
    fn unsafe_text_is_rejected() {
        assert!(safe_to_send("Please report progress on the goal."));
        assert!(!safe_to_send("run `ls` now"));
        assert!(!safe_to_send("echo $(whoami)"));
    }
}
