//! Ownership of supervisor-owned agent sessions: the decision, and the pass that acts on it.
//!
//! [`next_action`] is the pure half: given a snapshot of an owned session and a time, what (if
//! anything) should the supervisor do right now? It performs no IO, reads no clocks, and
//! mutates nothing, so it can be exercised in isolation and reasoned about deterministically.
//!
//! [`serve`] is the half that runs: it reads every running goal's bound pane from the fleet
//! ([`read_views`]), decides with [`tick`], and sends the one action that is a message to a
//! pane ([`OwnershipAction::Nudge`]), recording each send in `hub_meta` so a silence is nudged
//! once and a stall is escalated once.

use std::collections::HashMap;

use crate::agent_control::AgentControl;

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

/// The `hub_meta` key that records when an owned goal was last nudged (unix seconds).
pub const NUDGE_KEY_PREFIX: &str = "supervisor_owned_nudge:";
/// The `hub_meta` key whose presence pauses an owned goal ("잠깐 멈춰").
pub const PAUSE_KEY_PREFIX: &str = "supervisor_owned_pause:";
/// The `hub_meta` key recording that a stall was already escalated for an owned goal.
pub const STALL_KEY_PREFIX: &str = "supervisor_owned_stall:";

/// Decides, for every running goal of the supervisor that is bound to an agent pane, what to do
/// now.
///
/// The supervisor owns a session either because it launched it (the dispatcher records the pane
/// on the binding's first start) or because the owner handed it over ([`super::handover`]); both
/// carry a `provider_session_id` on a `pty` binding, so the binding is the whole set. A running
/// goal with no bound pane has no session to read and is not in it, an `mhc` binding's session is
/// the engine rather than a pane ([`read_views`] would never list it), and a session the hub
/// dispatched is that same goal rather than a second one, so nothing is ever owned twice.
///
/// This only decides: it sends nothing and changes nothing, so the caller that owns the pane
/// transport acts on the returned actions. A session whose state is not known yet is left alone.
/// `views` is [`read_views`]'s reading of each goal's bound pane; a goal with no view is unknown.
pub async fn tick(
    pool: &sqlx::SqlitePool,
    views: &HashMap<String, OwnedSessionView>,
    now: i64,
    nudge_after: i64,
) -> Result<Vec<(String, OwnershipAction)>, sqlx::Error> {
    let goals: Vec<String> = sqlx::query_scalar(
        "SELECT b.goal_id FROM supervisor_execution_bindings b
           JOIN supervisor_goals g ON g.goal_id = b.goal_id
          WHERE g.run_state = 'running' AND b.kind = 'pty'
            AND b.provider_session_id IS NOT NULL
          ORDER BY b.goal_id",
    )
    .fetch_all(pool)
    .await?;
    let mut actions = Vec::with_capacity(goals.len());
    for goal_id in goals {
        let paused: Option<String> = sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?")
            .bind(format!("{PAUSE_KEY_PREFIX}{goal_id}"))
            .fetch_optional(pool)
            .await?;
        let nudged: Option<String> = sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?")
            .bind(format!("{NUDGE_KEY_PREFIX}{goal_id}"))
            .fetch_optional(pool)
            .await?;
        let stalled: Option<String> = sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?")
            .bind(format!("{STALL_KEY_PREFIX}{goal_id}"))
            .fetch_optional(pool)
            .await?;
        let mut view = views.get(&goal_id).cloned().unwrap_or(OwnedSessionView {
            agent: "unknown".to_string(),
            state: SessionState::Unknown,
            paused: false,
            last_report_at: None,
            last_activity_at: now,
            nudged_at: None,
            stall_escalated: false,
            turns_used: 0,
            turn_limit: u32::MAX,
        });
        // These keys are the hub's own record; a view only supplies the pane reading.
        view.paused = paused.is_some();
        view.nudged_at = nudged.and_then(|value| value.parse().ok());
        view.stall_escalated = stalled.is_some();
        // A pane that last changed state after our nudge has reported since it: that re-arms a
        // nudge for the next silence. A pane that has sat still since the nudge has not.
        view.last_report_at = match view.nudged_at {
            Some(sent) if view.last_activity_at > sent => Some(view.last_activity_at),
            _ => None,
        };
        actions.push((goal_id, next_action(&view, now, nudge_after)));
    }
    Ok(actions)
}

/// How long the fleet inventory may take before a host's panes count as unreadable.
const INVENTORY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn session_state(raw: &str) -> SessionState {
    match raw {
        "working" => SessionState::Working,
        "blocked" | "question" => SessionState::PromptPending,
        "idle" => SessionState::Idle,
        _ => SessionState::Unknown,
    }
}

/// One string field of an `agents.list` entry, absent and non-string alike as `None`.
fn pane_field<'a>(pane: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    pane.get(key).and_then(serde_json::Value::as_str)
}

/// Reads every running goal's bound pane from the fleet and turns it into a supervisor view.
///
/// Silence is measured from the daemon's `stateSince`, the moment the pane entered its current
/// state, so a pane that has been idle since long before this pass is stale rather than fresh.
/// A goal whose bound session the fleet does not list is left out of the map, which [`tick`]
/// reads as an unknown session, and an unknown session is never nudged. A pane with no agent is
/// a plain shell, which the supervisor never owns ([`super::handover`] refuses one), so it is
/// left out too rather than treated as an agent that has gone quiet.
pub async fn read_views(
    pool: &sqlx::SqlitePool,
    control: &dyn AgentControl,
    now: i64,
) -> Result<HashMap<String, OwnedSessionView>, sqlx::Error> {
    let bound: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT b.goal_id, b.host_id, b.provider_session_id
           FROM supervisor_execution_bindings b
           JOIN supervisor_goals g ON g.goal_id = b.goal_id
          WHERE g.run_state = 'running' AND b.kind = 'pty'
            AND b.provider_session_id IS NOT NULL
          ORDER BY b.goal_id",
    )
    .fetch_all(pool)
    .await?;
    let mut hosts = vec!["local".to_string()];
    for (_, host, _) in &bound {
        if !hosts.contains(host) {
            hosts.push(host.clone());
        }
    }
    let entries = crate::fleet::list_persisted(control, pool, &hosts, INVENTORY_TIMEOUT).await;
    let mut views = HashMap::with_capacity(bound.len());
    for (goal_id, host, session_id) in bound {
        let pane = entries
            .iter()
            .find(|entry| entry.host_id == host)
            .and_then(|entry| {
                entry.sessions.iter().find(|pane| {
                    pane.get("sessionId").and_then(serde_json::Value::as_str)
                        == Some(session_id.as_str())
                })
            });
        let Some(pane) = pane else { continue };
        let agent = pane_field(pane, "agent").unwrap_or("").trim();
        if agent.is_empty() {
            continue;
        }
        let state = session_state(pane_field(pane, "state").unwrap_or("unknown"));
        let since = pane
            .get("stateSince")
            .and_then(serde_json::Value::as_i64)
            .map_or(now, |ms| ms / 1000);
        views.insert(
            goal_id,
            OwnedSessionView {
                agent: agent.to_string(),
                state,
                paused: false,
                // Entering the current state is the pane's last word to us; whether that counts
                // as a report depends on when we last nudged, which [`tick`] knows.
                last_report_at: None,
                last_activity_at: since.min(now),
                nudged_at: None,
                stall_escalated: false,
                turns_used: 0,
                turn_limit: u32::MAX,
            },
        );
    }
    Ok(views)
}

/// Records that the stall of `goal_id` was escalated, so it is escalated once, not every pass.
async fn record_stall(pool: &sqlx::SqlitePool, goal_id: &str, now: i64) {
    if let Err(error) = sqlx::query(
        "INSERT INTO hub_meta(key, value) VALUES(?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(format!("{STALL_KEY_PREFIX}{goal_id}"))
    .bind(now.to_string())
    .execute(pool)
    .await
    {
        tracing::warn!(%error, goal = %goal_id, "a stall escalation was not recorded");
    }
}

/// Pauses (`true`) or resumes (`false`) one owned goal, by writing [`PAUSE_KEY_PREFIX`].
///
/// The pass reads the key on every tick: a paused goal is never nudged, and resuming restores
/// it with the silence it had, so nothing is sent before its next report or next silence.
pub async fn set_paused(
    pool: &sqlx::SqlitePool,
    goal_id: &str,
    paused: bool,
) -> Result<(), sqlx::Error> {
    let key = format!("{PAUSE_KEY_PREFIX}{goal_id}");
    if paused {
        sqlx::query(
            "INSERT INTO hub_meta(key, value) VALUES(?, '1')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .execute(pool)
        .await?;
    } else {
        sqlx::query("DELETE FROM hub_meta WHERE key = ?")
            .bind(key)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Sends one nudge to the goal's bound pane and records when it was sent.
///
/// Only a confirmed start records [`NUDGE_KEY_PREFIX`]: a refused or uncertain send is left for
/// the next pass rather than being mistaken for a nudge that arrived.
async fn execute_nudge(
    pool: &sqlx::SqlitePool,
    control: &dyn AgentControl,
    goal_id: &str,
    text: &str,
    now: i64,
) {
    if !safe_to_send(text) {
        tracing::warn!(goal = %goal_id, "supervisor nudge was not sent: unsafe text");
        return;
    }
    let binding: Option<(String, String)> = sqlx::query_as(
        "SELECT host_id, provider_session_id FROM supervisor_execution_bindings
          WHERE goal_id = ? AND provider_session_id IS NOT NULL",
    )
    .bind(goal_id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);
    let Some((host, session_id)) = binding else {
        return;
    };
    let host_id = (host != "local").then_some(host.as_str());
    match control
        .send_confirmed(
            host_id,
            &session_id,
            text,
            &format!("hub-ownership-{goal_id}-{now}"),
        )
        .await
    {
        crate::goals::types::SendOutcome::Started => {
            if let Err(error) = sqlx::query(
                "INSERT INTO hub_meta(key, value) VALUES(?, ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            )
            .bind(format!("{NUDGE_KEY_PREFIX}{goal_id}"))
            .bind(now.to_string())
            .execute(pool)
            .await
            {
                tracing::warn!(%error, goal = %goal_id, "a sent nudge was not recorded");
            }
            // The session is live enough to nudge again: its next silence may escalate afresh.
            let _ = sqlx::query("DELETE FROM hub_meta WHERE key = ?")
                .bind(format!("{STALL_KEY_PREFIX}{goal_id}"))
                .execute(pool)
                .await;
            tracing::info!(goal = %goal_id, "supervisor nudged a silent owned session");
        }
        crate::goals::types::SendOutcome::Refused { code } => {
            tracing::debug!(%code, goal = %goal_id, "supervisor nudge was refused");
        }
        crate::goals::types::SendOutcome::Uncertain { code } => {
            tracing::warn!(%code, goal = %goal_id, "supervisor nudge outcome is unknown");
        }
    }
}

/// One ownership pass: read the panes, decide, then execute the nudges.
///
/// Only [`OwnershipAction::Nudge`] is executed here. A stall is logged for the owner alerts and
/// the recovery scheduler to see, a continuation and a turn-limit stop belong to the goal loop,
/// and a pause is the owner's own key ([`PAUSE_KEY_PREFIX`]), which this pass only reads.
pub async fn supervise(
    pool: &sqlx::SqlitePool,
    control: &dyn AgentControl,
    now: i64,
    nudge_after: i64,
) -> Result<Vec<(String, OwnershipAction)>, sqlx::Error> {
    // The owner's global pause ([`crate::control::set_paused`]) stops every supervisor action.
    if crate::control::is_paused(pool).await.unwrap_or(true) {
        return Ok(Vec::new());
    }
    let views = read_views(pool, control, now).await?;
    let actions = tick(pool, &views, now, nudge_after).await?;
    for (goal_id, action) in &actions {
        match action {
            OwnershipAction::Nudge(text) => {
                execute_nudge(pool, control, goal_id, text, now).await;
            }
            OwnershipAction::EscalateStall(text) => {
                tracing::warn!(goal = %goal_id, %text, "an owned session stalled");
                record_stall(pool, goal_id, now).await;
            }
            OwnershipAction::Nothing
            | OwnershipAction::Continue(_)
            | OwnershipAction::StopTurnLimit => {}
        }
    }
    Ok(actions)
}

/// Runs [`supervise`] once a minute for as long as the task lives.
///
/// The pane transport is [`crate::sessions::live::installed`], read on every pass because the
/// hub installs it after its tasks are spawned; with none installed there is nothing to read
/// a pane with, so the pass is skipped rather than nudging blind.
pub async fn serve(pool: sqlx::SqlitePool) {
    let mut ticks = tokio::time::interval(std::time::Duration::from_secs(60));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        let Some(control) = crate::sessions::live::installed() else {
            continue;
        };
        let now = chrono::Utc::now().timestamp();
        if let Err(error) = supervise(&pool, control.as_ref(), now, nudge_after_secs()).await {
            tracing::warn!(%error, "supervisor ownership pass failed");
        }
    }
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

    #[tokio::test]
    async fn an_owned_session_in_an_unknown_state_is_left_alone() {
        let pool = crate::storage::memory().await.unwrap();
        let actions = tick(&pool, &HashMap::new(), 10_000, nudge_after_secs())
            .await
            .unwrap();
        assert!(actions
            .iter()
            .all(|(_, action)| *action == OwnershipAction::Nothing));
    }

    /// A running goal handed over on one pane, plus the control that reports that pane idle
    /// since `idle_since_secs`.
    async fn handed_over(
        pool: &sqlx::SqlitePool,
        idle_since_secs: i64,
    ) -> crate::agent_control::RecordingControl {
        use crate::goals::contract::RequirementSpec;
        use crate::goals::types::GoalMode;
        use crate::supervisor::admission::OperationClass;
        use crate::supervisor::handover::handover;
        use crate::supervisor::{
            bind_project_host, create_project, ensure_identity, AdmitRequest, BoundSession,
            ChannelProvider, ExecutionKind, ExecutionTarget,
        };
        const OWNER: &str = "slack:T1:U-owner";
        const HOST: &str = "local";
        const PANE: &str = "pane-owned-1";
        let identity = ensure_identity(pool, OWNER).await.unwrap();
        let project = create_project(
            pool,
            &identity.supervisor_id,
            "ferryx",
            "github.com/example/ferryx",
        )
        .await
        .unwrap();
        bind_project_host(pool, &project.project_id, HOST, "/repo/ferryx/.git")
            .await
            .unwrap();
        handover(
            pool,
            AdmitRequest {
                supervisor_id: identity.supervisor_id.clone(),
                owner_principal: OWNER.into(),
                source_adapter: ChannelProvider::Slack,
                source_message_id: crate::supervisor::handover::handover_source_id(HOST, PANE),
                body: "Finish the fetch cache.".into(),
                project_id: project.project_id.clone(),
                channel_binding_id: None,
                target: ExecutionTarget {
                    kind: ExecutionKind::Pty,
                    host_id: HOST.into(),
                },
                title: "Finish the fetch cache".into(),
                cwd: "/repo/ferryx".into(),
                instruction: "Finish the fetch cache.".into(),
                requirements: vec![RequirementSpec {
                    id: "r1".into(),
                    text: "the fetch cache is wired".into(),
                    verifier: "cargo test -p ferryx-hub passes".into(),
                }],
                mode: GoalMode::Supervised,
                turn_limit: 5,
                operation_grant: vec![OperationClass::Edit],
            },
            BoundSession {
                host_id: HOST.into(),
                session_id: PANE.into(),
            },
            false,
            false,
        )
        .await
        .unwrap();
        let control = crate::agent_control::RecordingControl::default();
        *control.result.lock().expect("result") = serde_json::json!({
            "sessions": [{
                "sessionId": PANE,
                "agent": "claude",
                "state": "idle",
                "stateSince": idle_since_secs * 1000,
            }]
        });
        control
    }

    fn sent_texts(control: &crate::agent_control::RecordingControl) -> Vec<String> {
        control
            .ops
            .lock()
            .expect("ops")
            .iter()
            .filter_map(|(_, op)| match op {
                crate::agent_control::AgentOp::Send { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// A running goal the supervisor launched itself, bound to the pane its dispatcher was
    /// given: the binding names that pane exactly as a handover's binding does.
    async fn launched(pool: &sqlx::SqlitePool, pane: &str) -> String {
        use crate::goals::contract::RequirementSpec;
        use crate::goals::types::GoalMode;
        use crate::supervisor::admission::OperationClass;
        use crate::supervisor::{
            admit_request, attach_session, bind_project_host, claim_dispatch,
            confirm_claim_before_transmit, create_project, ensure_identity, record_start,
            AdmitRequest, ChannelProvider, ExecutionKind, ExecutionTarget,
        };
        const OWNER: &str = "slack:T1:U-owner";
        const HOST: &str = "local";
        let identity = ensure_identity(pool, OWNER).await.unwrap();
        let project = create_project(
            pool,
            &identity.supervisor_id,
            "ferryx",
            "github.com/example/ferryx",
        )
        .await
        .unwrap();
        bind_project_host(pool, &project.project_id, HOST, "/repo/ferryx/.git")
            .await
            .unwrap();
        let admitted = admit_request(
            pool,
            &AdmitRequest {
                supervisor_id: identity.supervisor_id.clone(),
                owner_principal: OWNER.into(),
                source_adapter: ChannelProvider::Slack,
                source_message_id: "launched-1".into(),
                body: "Finish the fetch cache.".into(),
                project_id: project.project_id.clone(),
                channel_binding_id: None,
                target: ExecutionTarget {
                    kind: ExecutionKind::Pty,
                    host_id: HOST.into(),
                },
                title: "Finish the fetch cache".into(),
                cwd: "/repo/ferryx".into(),
                instruction: "Finish the fetch cache.".into(),
                requirements: vec![RequirementSpec {
                    id: "r1".into(),
                    text: "the fetch cache is wired".into(),
                    verifier: "cargo test -p ferryx-hub passes".into(),
                }],
                mode: GoalMode::Full,
                turn_limit: 5,
                operation_grant: vec![OperationClass::Edit],
            },
        )
        .await
        .unwrap();
        let goal_id = admitted.receipt.goal_id;
        let request_id: String = sqlx::query_scalar(
            "SELECT request_id FROM supervisor_dispatches WHERE goal_id = ? AND state = 'ready'",
        )
        .bind(&goal_id)
        .fetch_one(pool)
        .await
        .unwrap();
        attach_session(pool, &goal_id, 1, pane).await.unwrap();
        claim_dispatch(pool, &request_id, 1, 1).await.unwrap();
        confirm_claim_before_transmit(pool, &request_id)
            .await
            .unwrap();
        record_start(pool, &request_id, 1, "run-1", pane).await.unwrap();
        goal_id
    }

    fn silent() -> OwnedSessionView {
        let mut view = idle();
        view.last_activity_at = NOW - NUDGE;
        view
    }

    /// The plan's "launched by the supervisor or handed over": a goal the supervisor launched
    /// itself is owned as soon as its pane is bound, not only a handover.
    #[tokio::test]
    async fn a_goal_the_supervisor_launched_is_owned_like_a_handed_over_one() {
        let pool = crate::storage::memory().await.unwrap();
        let goal_id = launched(&pool, "pane-launched-1").await;
        let views = HashMap::from([(goal_id.clone(), silent())]);

        let actions = tick(&pool, &views, NOW, NUDGE).await.unwrap();

        assert_eq!(
            actions,
            vec![(
                goal_id,
                OwnershipAction::Nudge("Please report progress on the goal.".to_string())
            )],
            "a launched goal is nudged like a handed-over one"
        );
    }

    /// A running goal with no bound pane has no session to read, so the pass leaves it alone:
    /// nothing is owned twice and nothing is nudged blind.
    #[tokio::test]
    async fn a_running_goal_with_no_bound_pane_is_not_owned() {
        let pool = crate::storage::memory().await.unwrap();
        let goal_id = launched(&pool, "pane-launched-2").await;
        sqlx::query(
            "UPDATE supervisor_execution_bindings SET provider_session_id = NULL WHERE goal_id = ?",
        )
        .bind(&goal_id)
        .execute(&pool)
        .await
        .unwrap();
        let views = HashMap::from([(goal_id, silent())]);

        let actions = tick(&pool, &views, NOW, NUDGE).await.unwrap();

        assert!(actions.is_empty());
    }

    #[tokio::test]
    async fn a_silent_owned_session_is_nudged_once_and_the_nudge_is_recorded() {
        let now = 10_000;
        let pool = crate::storage::memory().await.unwrap();
        let control = handed_over(&pool, now - 3 * NUDGE).await;

        let actions = supervise(&pool, &control, now, NUDGE).await.unwrap();
        assert_eq!(
            actions.iter().map(|(_, action)| action.clone()).collect::<Vec<_>>(),
            vec![OwnershipAction::Nudge(
                "Please report progress on the goal.".to_string()
            )]
        );
        assert_eq!(
            sent_texts(&control),
            vec!["Please report progress on the goal.".to_string()]
        );
        let recorded: Option<String> =
            sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?")
                .bind(format!("{NUDGE_KEY_PREFIX}{}", actions[0].0))
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert_eq!(recorded, Some(now.to_string()));

        // The same silence a second later is the nudge just sent: nothing more is typed.
        supervise(&pool, &control, now + 1, NUDGE).await.unwrap();
        assert_eq!(sent_texts(&control).len(), 1);
    }

    #[tokio::test]
    async fn a_paused_owned_session_is_left_alone_until_it_is_resumed() {
        let now = 10_000;
        let pool = crate::storage::memory().await.unwrap();
        let control = handed_over(&pool, now - 3 * NUDGE).await;
        let goal_id: String = sqlx::query_scalar("SELECT goal_id FROM supervisor_goals")
            .fetch_one(&pool)
            .await
            .unwrap();

        set_paused(&pool, &goal_id, true).await.unwrap();
        let actions = supervise(&pool, &control, now, NUDGE).await.unwrap();
        assert_eq!(
            actions.iter().map(|(_, action)| action.clone()).collect::<Vec<_>>(),
            vec![OwnershipAction::Nothing]
        );
        assert!(sent_texts(&control).is_empty(), "a paused goal is never nudged");

        set_paused(&pool, &goal_id, false).await.unwrap();
        supervise(&pool, &control, now, NUDGE).await.unwrap();
        assert_eq!(sent_texts(&control).len(), 1, "resuming restores the nudge");
    }
}
