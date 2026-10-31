use crate::daemon::protocol::{AgentBlockedKind, AgentOutcome, AgentProviderSession, AgentStateOrigin};
use crate::terminal::program_status::{BlockedKind, ProgramState, ProgramStatusSummary};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentState {
    pub session_id: String,
    pub state: String,
    pub agent: Option<String>,
    pub provider_session: Option<AgentProviderSession>,
    /// What the agent is blocked on, when it can name it. Carried to the inbox so a waiting
    /// session can show the question instead of only a state word.
    pub detail: Option<String>,
    /// Who produced this state. `idle` from a running agent and `idle` from a daemon release are
    /// the same word with opposite meanings for screen inference, so the producer travels along.
    pub origin: AgentStateOrigin,
    /// How the turn behind an `idle` ended, when the producer said so (OSC 7501 done/error).
    pub outcome: Option<AgentOutcome>,
    /// What a `blocked` session needs from the user, when the producer said so.
    pub kind: Option<AgentBlockedKind>,
}

#[derive(Clone, Debug)]
pub(crate) struct AgentStateUpdate {
    pub state: AgentState,
    pub is_snapshot: bool,
}

pub(crate) struct AgentStateSubscription {
    pub snapshot: Option<AgentState>,
    pub receiver: broadcast::Receiver<AgentStateUpdate>,
    hub: Arc<AgentStateHub>,
}

pub(crate) struct AgentStateHub {
    retained: Mutex<HashMap<String, AgentState>>,
    /// Revision of the newest OSC 7501 summary seen per session. Locked only while `retained`
    /// is held, so the revision check and the state it guards change together.
    protocol_revisions: Mutex<HashMap<String, u64>>,
    tx: broadcast::Sender<AgentStateUpdate>,
}

impl Default for AgentStateHub {
    fn default() -> Self {
        Self::new(64)
    }
}

impl AgentStateHub {
    fn new(capacity: usize) -> Self {
        Self {
            retained: Mutex::new(HashMap::new()),
            protocol_revisions: Mutex::new(HashMap::new()),
            tx: broadcast::channel(capacity).0,
        }
    }

    pub fn publish_canonical(&self, state: AgentState) {
        let mut retained = self.retained.lock();
        retained.insert(state.session_id.clone(), state.clone());
        let _ = self.tx.send(AgentStateUpdate {
            state,
            is_snapshot: false,
        });
    }

    /// Publishes a provider session the daemon discovered from the pane's agent process, unless
    /// the session already carries a live one. The check and the insert share the publishers'
    /// lock, so an identity the agent reported for itself is never replaced by a discovered one.
    ///
    /// `baseline` is the provider session the hub retained when this agent process was first
    /// seen. A release keeps the exited agent's identity, so that leftover (and only that one) may
    /// be replaced by the identity the next agent in the pane is discovered to hold.
    ///
    /// Discovery names the session only. The retained state word and detail are kept, and a
    /// session whose agent reports its own state keeps that origin, so a discovered id never lifts
    /// the screen-inference suppression a self-report installed.
    pub(crate) fn publish_discovered(
        &self,
        mut state: AgentState,
        baseline: Option<&AgentProviderSession>,
    ) -> bool {
        let mut retained = self.retained.lock();
        if let Some(previous) = retained.get(&state.session_id) {
            if holds_live_provider(Some(previous), baseline) {
                return false;
            }
            state.state = previous.state.clone();
            state.detail = previous.detail.clone();
            state.outcome = previous.outcome;
            state.kind = previous.kind;
            if previous.origin.is_provider_report() {
                state.origin = previous.origin;
            }
        }
        retained.insert(state.session_id.clone(), state.clone());
        let _ = self.tx.send(AgentStateUpdate {
            state,
            is_snapshot: false,
        });
        true
    }

    /// Publishes the pane program's OSC 7501 status (`None` once its records are cleared).
    ///
    /// `revision` orders the summaries of a session: the mirror numbers each change under its
    /// lock, but publication runs after the lock is released, so two changes can arrive here in
    /// the opposite order. One older than a revision already seen is dropped, and the mirror never
    /// re-reports its newer summary, so accepting it would pin the session to stale state.
    ///
    /// Returns whether anything was published. A session whose agent reports through the
    /// extension (`Agent` origin) is never touched: that report is authoritative while it lives.
    /// A clear only releases a state the protocol itself installed, so a terminal reset in a pane
    /// that never spoke OSC 7501 does not suppress screen inference. The release hands the
    /// session back to screen inference (an origin that claims no activity); an explicit `idle`
    /// report stays the program's own. An unchanged state is not re-broadcast.
    pub(crate) fn publish_protocol(
        &self,
        session_id: &str,
        revision: u64,
        summary: Option<&ProgramStatusSummary>,
    ) -> bool {
        let mut retained = self.retained.lock();
        {
            let mut revisions = self.protocol_revisions.lock();
            let newest = revisions.entry(session_id.to_string()).or_insert(0);
            if revision <= *newest {
                return false;
            }
            *newest = revision;
        }
        let previous = retained.get(session_id);
        if previous.is_some_and(|state| state.origin == AgentStateOrigin::Agent) {
            return false;
        }
        let state = match summary.filter(|summary| summary.state != ProgramState::Clear) {
            Some(reported) => {
                let (word, outcome) = match reported.state {
                    ProgramState::Working => ("working", None),
                    ProgramState::Blocked => ("blocked", None),
                    ProgramState::Done => ("idle", Some(AgentOutcome::Done)),
                    ProgramState::Error => ("idle", Some(AgentOutcome::Error)),
                    ProgramState::Idle | ProgramState::Clear => ("idle", None),
                };
                let blocked = Some(reported).filter(|summary| summary.state == ProgramState::Blocked);
                AgentState {
                    session_id: session_id.to_string(),
                    state: word.to_string(),
                    agent: reported
                        .app
                        .clone()
                        .or_else(|| previous.and_then(|state| state.agent.clone())),
                    provider_session: previous.and_then(|state| state.provider_session.clone()),
                    detail: blocked.and_then(|summary| summary.message.clone()),
                    origin: AgentStateOrigin::Protocol,
                    outcome,
                    kind: blocked.and_then(|summary| summary.kind).map(|kind| match kind {
                        BlockedKind::Permission => AgentBlockedKind::Permission,
                        BlockedKind::Question => AgentBlockedKind::Question,
                        BlockedKind::Auth => AgentBlockedKind::Auth,
                    }),
                }
            }
            None => {
                let Some(previous) =
                    previous.filter(|state| state.origin == AgentStateOrigin::Protocol)
                else {
                    return false;
                };
                // No record is left, so the program claims nothing any more. A `Protocol`
                // origin here would suppress screen inference forever. `ProcessObserved` is the
                // existing wire origin that carries no activity of its own and re-arms
                // inference, exactly as a process sighting does after a release.
                AgentState {
                    session_id: session_id.to_string(),
                    state: "idle".to_string(),
                    agent: previous.agent.clone(),
                    provider_session: previous.provider_session.clone(),
                    detail: None,
                    origin: AgentStateOrigin::ProcessObserved,
                    outcome: None,
                    kind: None,
                }
            }
        };
        if previous == Some(&state) {
            return false;
        }
        retained.insert(session_id.to_string(), state.clone());
        let _ = self.tx.send(AgentStateUpdate {
            state,
            is_snapshot: false,
        });
        true
    }

    pub(crate) fn release_manual(&self, session_id: &str) {
        let mut retained = self.retained.lock();
        let previous = retained.get(session_id).cloned();
        let state = AgentState {
            session_id: session_id.to_string(),
            state: "idle".to_string(),
            agent: previous.as_ref().and_then(|s| s.agent.clone()),
            provider_session: previous.as_ref().and_then(|s| s.provider_session.clone()),
            detail: None,
            origin: AgentStateOrigin::ManualReset,
            outcome: None,
            kind: None,
        };
        tracing::info!(session_id, reason = "manual_reset",
            previous_state = ?previous.as_ref().map(|s| &s.state),
            "agent activity released");
        retained.insert(session_id.to_string(), state.clone());
        let _ = self.tx.send(AgentStateUpdate {
            state,
            is_snapshot: false,
        });
    }

    pub(crate) fn release_foreground(&self, session_id: &str) {
        let mut retained = self.retained.lock();
        let previous = retained.get(session_id).cloned();
        let state = AgentState {
            session_id: session_id.to_string(),
            state: "idle".to_string(),
            agent: previous.as_ref().and_then(|s| s.agent.clone()),
            provider_session: previous.as_ref().and_then(|s| s.provider_session.clone()),
            detail: None,
            origin: AgentStateOrigin::ProcessReleased,
            outcome: None,
            kind: None,
        };
        tracing::info!(session_id, reason = "foreground_agent_to_shell",
            previous_state = ?previous.as_ref().map(|s| &s.state),
            "agent activity released");
        retained.insert(session_id.to_string(), state.clone());
        let _ = self.tx.send(AgentStateUpdate {
            state,
            is_snapshot: false,
        });
    }

    /// Publishes the positive evidence that an agent process owns this PTY again.
    ///
    /// This carries no activity of its own — only a released session needs it, to undo the
    /// inference lockout the release installed. Sessions whose agent reports its own state are
    /// left alone: their retained state is authoritative and must not be overwritten by a
    /// process sighting.
    pub(crate) fn observe_foreground_agent(&self, session_id: &str) {
        let mut retained = self.retained.lock();
        let Some(previous) = retained.get(session_id).cloned() else {
            return;
        };
        if previous.origin != AgentStateOrigin::ProcessReleased {
            return;
        }
        let state = AgentState {
            origin: AgentStateOrigin::ProcessObserved,
            ..previous
        };
        tracing::info!(
            session_id,
            reason = "foreground_shell_to_agent",
            "agent process observed; screen inference re-armed"
        );
        retained.insert(session_id.to_string(), state.clone());
        let _ = self.tx.send(AgentStateUpdate {
            state,
            is_snapshot: false,
        });
    }

    /// The successor owns the report socket. Predecessor frames are only a quiet
    /// baseline; replaying their backlog through multiple attaches must not mint live edges.
    pub fn publish_legacy(&self, state: AgentState, _is_snapshot: bool) -> bool {
        let mut retained = self.retained.lock();
        if retained.contains_key(&state.session_id) {
            return false;
        }
        retained.insert(state.session_id.clone(), state.clone());
        let _ = self.tx.send(AgentStateUpdate {
            state,
            is_snapshot: true,
        });
        true
    }

    /// Subscribes while holding the same lock used by publishers, then reads the snapshot. A
    /// publication is therefore either represented by the snapshot or queued on the receiver.
    pub fn subscribe(self: &Arc<Self>, session_id: &str) -> AgentStateSubscription {
        let retained = self.retained.lock();
        let receiver = self.tx.subscribe();
        let snapshot = retained.get(session_id).cloned();
        AgentStateSubscription {
            snapshot,
            receiver,
            hub: Arc::clone(self),
        }
    }

    pub fn current(&self, session_id: &str) -> Option<AgentState> {
        self.retained.lock().get(session_id).cloned()
    }

    pub fn remove(&self, session_id: &str) {
        let mut retained = self.retained.lock();
        retained.remove(session_id);
        self.protocol_revisions.lock().remove(session_id);
    }
}

/// Whether `current` carries a provider identity discovery must leave alone: one the agent
/// reported itself, or any identity published since `baseline` (the one retained when the
/// current agent process was first seen) was taken.
pub(crate) fn holds_live_provider(
    current: Option<&AgentState>,
    baseline: Option<&AgentProviderSession>,
) -> bool {
    current.is_some_and(|state| {
        state.provider_session.as_ref().is_some_and(|provider| {
            state.origin == AgentStateOrigin::Agent || Some(provider) != baseline
        })
    })
}

impl AgentStateSubscription {
    pub fn resynchronize(&mut self, session_id: &str) -> Option<AgentState> {
        let fresh = self.hub.subscribe(session_id);
        self.receiver = fresh.receiver;
        self.snapshot = fresh.snapshot;
        self.snapshot.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(session_id: &str, value: &str) -> AgentState {
        AgentState {
            session_id: session_id.to_string(),
            state: value.to_string(),
            agent: Some("omo".to_string()),
            provider_session: None,
            detail: None,
            origin: AgentStateOrigin::Agent,
            outcome: None,
            kind: None,
        }
    }

    fn provider(id: &str) -> AgentProviderSession {
        AgentProviderSession {
            launcher: None,
            key: crate::daemon::protocol::AgentProviderSessionKey::SessionId,
            id: id.to_string(),
            transcript_path: None,
        }
    }

    fn discovered(session_id: &str, agent: &str, id: &str) -> AgentState {
        AgentState {
            session_id: session_id.to_string(),
            state: "idle".to_string(),
            agent: Some(agent.to_string()),
            provider_session: Some(provider(id)),
            detail: None,
            origin: AgentStateOrigin::ProcessDiscovered,
            outcome: None,
            kind: None,
        }
    }

    // Re-explore-4 G2: a discovered identity fills a gap and never overwrites what an agent
    // reported for itself.
    #[test]
    fn a_discovered_identity_never_replaces_one_the_agent_reported() {
        let hub = AgentStateHub::new(8);
        hub.publish_canonical(AgentState {
            provider_session: Some(provider("self-reported")),
            ..state("s1", "working")
        });
        assert!(!hub.publish_discovered(discovered("s1", "omo", "discovered"), None));
        let current = hub.current("s1").unwrap();
        assert_eq!(current.provider_session, Some(provider("self-reported")));
        assert_eq!(current.origin, AgentStateOrigin::Agent);
        // Even when it equals the baseline: the agent's own report is authoritative.
        let own = provider("self-reported");
        assert!(!hub.publish_discovered(discovered("s1", "omo", "x"), Some(&own)));
    }

    #[test]
    fn a_discovered_identity_fills_a_pane_without_one_and_keeps_its_state() {
        let hub = AgentStateHub::new(8);
        for agent in ["kimi", "copilot", "gjc"] {
            let session = format!("pane-{agent}");
            assert!(hub.publish_discovered(discovered(&session, agent, "id-1"), None));
            let current = hub.current(&session).unwrap();
            assert_eq!(current.provider_session, Some(provider("id-1")));
            assert_eq!(current.origin, AgentStateOrigin::ProcessDiscovered);
            // A second discovery does not overwrite the first.
            assert!(!hub.publish_discovered(discovered(&session, agent, "id-2"), None));
            assert_eq!(hub.current(&session).unwrap().provider_session, Some(provider("id-1")));
        }
        // A retained activity word and detail survive the discovery.
        hub.publish_canonical(AgentState {
            detail: Some("approve?".into()),
            agent: Some("kimi".into()),
            ..state("s2", "blocked")
        });
        assert!(hub.publish_discovered(discovered("s2", "kimi", "id-3"), None));
        let current = hub.current("s2").unwrap();
        assert_eq!(current.state, "blocked");
        assert_eq!(current.detail.as_deref(), Some("approve?"));
        // The self-reported origin is kept, so inference stays suppressed for that pane.
        assert_eq!(current.origin, AgentStateOrigin::Agent);
    }

    #[test]
    fn a_released_agents_leftover_identity_yields_to_the_next_agents_discovery() {
        let hub = AgentStateHub::new(8);
        hub.publish_discovered(discovered("s3", "kimi", "old"), None);
        hub.release_foreground("s3");
        let leftover = hub.current("s3").unwrap().provider_session;
        assert_eq!(leftover, Some(provider("old")));
        // A new agent process starts with the leftover as its baseline: that one may be replaced.
        assert!(!holds_live_provider(hub.current("s3").as_ref(), leftover.as_ref()));
        assert!(hub.publish_discovered(discovered("s3", "gjc", "new"), leftover.as_ref()));
        assert_eq!(hub.current("s3").unwrap().provider_session, Some(provider("new")));
        // Anything published since the baseline is live.
        assert!(holds_live_provider(hub.current("s3").as_ref(), leftover.as_ref()));
        assert!(!hub.publish_discovered(discovered("s3", "gjc", "newer"), leftover.as_ref()));
        assert!(!holds_live_provider(None, None));
    }


    #[tokio::test]
    async fn manual_reset_clears_state_and_recovers_on_next_transition() {
        let hub = Arc::new(AgentStateHub::new(8));
        hub.publish_canonical(state("s1", "working"));
        let mut subscription = hub.subscribe("s1");
        assert_eq!(hub.current("s1").unwrap().state, "working");

        hub.release_manual("s1");

        let update = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            subscription.receiver.recv(),
        )
        .await
        .expect("manual reset event")
        .expect("state stream");
        assert_eq!(update.state.state, "idle");
        assert!(!update.is_snapshot);
        assert_eq!(hub.current("s1").unwrap().state, "idle");

        hub.publish_canonical(state("s1", "working"));
        let next_update = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            subscription.receiver.recv(),
        )
        .await
        .expect("next transition event")
        .expect("state stream");
        assert_eq!(next_update.state.state, "working");
        assert_eq!(hub.current("s1").unwrap().state, "working");
    }

    #[tokio::test]
    async fn agent_to_shell_transition_releases_state() {
        use crate::terminal::foreground::{AgentProcessEdge, Foreground, ProcessTransition};
        let hub = Arc::new(AgentStateHub::new(8));
        hub.publish_canonical(state("s1", "working"));
        let mut subscription = hub.subscribe("s1");
        let mut transition = ProcessTransition::default();
        assert_eq!(
            transition.observe(Some(Foreground::Agent { pid: 42, agent: "omo" })),
            Some(AgentProcessEdge::Observed)
        );
        if transition.observe(Some(Foreground::Shell)) == Some(AgentProcessEdge::Released) {
            hub.release_foreground("s1");
        }
        let update = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            subscription.receiver.recv(),
        )
        .await
        .expect("release event")
        .expect("state stream");
        assert_eq!(update.state.state, "idle");
        assert!(!update.is_snapshot);
        assert_eq!(hub.current("s1").unwrap().state, "idle");
        assert_eq!(
            transition.observe(Some(Foreground::Shell)),
            None,
            "one release per edge"
        );
    }

    #[test]
    fn released_state_is_distinguishable_from_an_agents_own_idle_report() {
        // Screen inference must stay disarmed after a process release: the exited agent's last
        // frame (spinner, "esc to interrupt" footer) is still on screen and would otherwise
        // re-promote the pane to working with no agent alive. An agent reporting idle between
        // turns is the opposite case and must keep its own ownership.
        let hub = Arc::new(AgentStateHub::new(8));
        hub.publish_canonical(state("s1", "working"));
        hub.release_foreground("s1");
        let released = hub.current("s1").expect("released state retained");
        assert_eq!(released.state, "idle");
        assert_eq!(released.origin, AgentStateOrigin::ProcessReleased);

        hub.publish_canonical(state("s2", "idle"));
        assert_eq!(hub.current("s2").unwrap().origin, AgentStateOrigin::Agent);

        hub.publish_canonical(state("s3", "working"));
        hub.release_manual("s3");
        assert_eq!(
            hub.current("s3").unwrap().origin,
            AgentStateOrigin::ManualReset
        );
    }

    #[tokio::test]
    async fn a_new_agent_process_re_arms_a_released_session() {
        // A release disarms screen inference; without a re-arm edge the next agent started in
        // that same pane would never show activity again.
        let hub = Arc::new(AgentStateHub::new(8));
        hub.publish_canonical(state("s1", "working"));
        hub.release_foreground("s1");
        let mut subscription = hub.subscribe("s1");

        hub.observe_foreground_agent("s1");
        let update = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            subscription.receiver.recv(),
        )
        .await
        .expect("re-arm event")
        .expect("state stream");
        assert_eq!(update.state.origin, AgentStateOrigin::ProcessObserved);
        assert_eq!(
            update.state.state, "idle",
            "a process sighting carries no activity of its own"
        );

        // A session the agent itself owns must not be disturbed by a process sighting.
        hub.publish_canonical(state("s2", "working"));
        let mut owned = hub.subscribe("s2");
        hub.observe_foreground_agent("s2");
        assert_eq!(hub.current("s2"), Some(state("s2", "working")));
        assert!(matches!(
            owned.receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn quiet_agent_with_no_output_and_live_process_is_not_released() {
        use crate::terminal::foreground::{AgentProcessEdge, Foreground, ProcessTransition};
        let hub = Arc::new(AgentStateHub::new(8));
        hub.publish_canonical(state("s1", "working"));
        let mut subscription = hub.subscribe("s1");
        let mut transition = ProcessTransition::default();
        for observation in [
            Some(Foreground::Agent { pid: 42, agent: "omo" }),
            None,
            Some(Foreground::Agent { pid: 42, agent: "omo" }),
        ] {
            if transition.observe(observation) == Some(AgentProcessEdge::Released) {
                hub.release_foreground("s1");
            }
        }
        assert_eq!(hub.current("s1"), Some(state("s1", "working")));
        assert!(matches!(
            subscription.receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    fn summary(state: ProgramState) -> ProgramStatusSummary {
        ProgramStatusSummary {
            state,
            kind: Some(BlockedKind::Permission),
            app: Some("claude".into()),
            title: None,
            message: Some("Allow edit?".into()),
            progress: None,
        }
    }

    /// Publishes under a fresh revision, as the screen mirror numbers each change.
    fn protocol(hub: &AgentStateHub, session_id: &str, summary: Option<&ProgramStatusSummary>) -> bool {
        hub.publish_protocol(
            session_id,
            crate::terminal::program_status_ingest::next_revision(),
            summary,
        )
    }

    #[test]
    fn protocol_states_map_to_state_words_outcomes_and_kinds() {
        let hub = AgentStateHub::new(16);
        assert!(protocol(&hub, "p", Some(&summary(ProgramState::Working))));
        let working = hub.current("p").unwrap();
        assert_eq!((working.state.as_str(), working.origin), ("working", AgentStateOrigin::Protocol));
        assert_eq!((working.detail, working.kind, working.outcome), (None, None, None));
        assert_eq!(working.agent.as_deref(), Some("claude"));

        assert!(protocol(&hub, "p", Some(&summary(ProgramState::Blocked))));
        let blocked = hub.current("p").unwrap();
        assert_eq!(blocked.state, "blocked");
        assert_eq!(blocked.detail.as_deref(), Some("Allow edit?"));
        assert_eq!(blocked.kind, Some(AgentBlockedKind::Permission));

        assert!(protocol(&hub, "p", Some(&summary(ProgramState::Done))));
        let done = hub.current("p").unwrap();
        assert_eq!((done.state.as_str(), done.outcome), ("idle", Some(AgentOutcome::Done)));
        assert_eq!((done.detail, done.kind), (None, None));

        assert!(protocol(&hub, "p", Some(&summary(ProgramState::Error))));
        assert_eq!(hub.current("p").unwrap().outcome, Some(AgentOutcome::Error));

        assert!(protocol(&hub, "p", Some(&summary(ProgramState::Idle))));
        let idle = hub.current("p").unwrap();
        assert_eq!((idle.state.as_str(), idle.outcome), ("idle", None));
        // An explicit idle report is the program's own: it keeps claiming the activity.
        assert_eq!(idle.origin, AgentStateOrigin::Protocol);
        // The same report again is not a new edge.
        assert!(!protocol(&hub, "p", Some(&summary(ProgramState::Idle))));
    }

    /// Finding 2: once no record is left the program claims nothing; the release must hand the
    /// session back to screen inference instead of keeping a `Protocol` claim forever.
    #[test]
    fn a_protocol_clear_releases_only_what_the_protocol_installed() {
        let hub = AgentStateHub::new(16);
        assert!(!protocol(&hub, "fresh", None), "nothing to release");
        assert!(hub.current("fresh").is_none());

        assert!(protocol(&hub, "p", Some(&summary(ProgramState::Working))));
        assert!(protocol(&hub, "p", None));
        let released = hub.current("p").unwrap();
        assert_eq!(released.state, "idle");
        assert!(!released.origin.claims_activity(), "{:?}", released.origin);
        assert!(!released.origin.is_provider_report());
        assert_eq!(released.agent.as_deref(), Some("claude"));
        assert_eq!((released.detail, released.outcome, released.kind), (None, None, None));
        assert!(!protocol(&hub, "p", None), "a second clear is not a new edge");
        // A later report claims the session again.
        assert!(protocol(&hub, "p", Some(&summary(ProgramState::Working))));
        assert_eq!(hub.current("p").unwrap().origin, AgentStateOrigin::Protocol);

        hub.release_foreground("r");
        assert!(!protocol(&hub, "r", None));
        assert_eq!(hub.current("r").unwrap().origin, AgentStateOrigin::ProcessReleased);
    }

    #[test]
    fn a_protocol_release_keeps_the_provider_session_and_never_touches_an_agent_origin() {
        let hub = AgentStateHub::new(16);
        assert!(protocol(&hub, "p", Some(&summary(ProgramState::Blocked))));
        assert!(hub.publish_discovered(discovered("p", "claude", "id-1"), None));
        assert!(protocol(&hub, "p", None));
        assert_eq!(hub.current("p").unwrap().provider_session, Some(provider("id-1")));

        // An extension report that took the session over is never released by the protocol.
        hub.publish_canonical(state("p", "working"));
        assert!(!protocol(&hub, "p", None));
        assert_eq!(hub.current("p"), Some(state("p", "working")));
    }

    /// Finding 3: an older revision arriving after a newer one is dropped, also across a
    /// release, and a removed session starts its ordering afresh.
    #[test]
    fn protocol_publications_older_than_the_newest_revision_are_dropped() {
        let hub = AgentStateHub::new(16);
        assert!(hub.publish_protocol("p", 10, Some(&summary(ProgramState::Working))));
        assert!(!hub.publish_protocol("p", 9, None), "stale clear");
        assert_eq!(hub.current("p").unwrap().state, "working");
        assert!(!hub.publish_protocol("p", 10, Some(&summary(ProgramState::Done))), "same revision");
        assert!(hub.publish_protocol("p", 12, None));
        assert!(!hub.publish_protocol("p", 11, Some(&summary(ProgramState::Working))), "stale working");
        assert!(!hub.current("p").unwrap().origin.claims_activity());

        // A replacement session under the same id is not held back by the old one's revisions.
        hub.remove("p");
        assert!(hub.publish_protocol("p", 13, Some(&summary(ProgramState::Working))));
        assert_eq!(hub.current("p").unwrap().state, "working");
    }

    #[test]
    fn a_protocol_report_never_overwrites_a_live_extension_report() {
        let hub = Arc::new(AgentStateHub::new(16));
        hub.publish_canonical(state("s1", "working"));
        let mut subscription = hub.subscribe("s1");
        assert!(!protocol(&hub, "s1", Some(&summary(ProgramState::Done))));
        assert!(!protocol(&hub, "s1", None));
        assert_eq!(hub.current("s1"), Some(state("s1", "working")));
        assert!(matches!(
            subscription.receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        // Once the extension's agent is released, the protocol may report again.
        hub.release_foreground("s1");
        assert!(protocol(&hub, "s1", Some(&summary(ProgramState::Working))));
        assert_eq!(hub.current("s1").unwrap().origin, AgentStateOrigin::Protocol);
        // And an extension report takes the session back.
        hub.publish_canonical(state("s1", "idle"));
        assert!(!protocol(&hub, "s1", Some(&summary(ProgramState::Working))));
    }

    #[test]
    fn a_discovered_identity_keeps_a_protocol_origin_and_outcome() {
        let hub = AgentStateHub::new(16);
        assert!(protocol(&hub, "p", Some(&summary(ProgramState::Done))));
        assert!(hub.publish_discovered(discovered("p", "claude", "id-1"), None));
        let current = hub.current("p").unwrap();
        assert_eq!(current.origin, AgentStateOrigin::Protocol);
        assert_eq!(current.outcome, Some(AgentOutcome::Done));
        assert_eq!(current.provider_session, Some(provider("id-1")));
    }

    #[test]
    fn canonical_report_wins_over_stale_and_duplicate_legacy_reports() {
        let hub = Arc::new(AgentStateHub::new(8));
        assert!(hub.publish_legacy(state("s1", "blocked"), false));
        hub.publish_canonical(state("s1", "working"));
        assert!(!hub.publish_legacy(state("s1", "idle"), false));
        assert!(!hub.publish_legacy(state("s1", "blocked"), true));
        assert_eq!(hub.current("s1"), Some(state("s1", "working")));
    }

    #[test]
    fn predecessor_snapshot_does_not_overwrite_newer_predecessor_transition() {
        let hub = Arc::new(AgentStateHub::new(8));
        assert!(hub.publish_legacy(state("s1", "working"), false));
        assert!(!hub.publish_legacy(state("s1", "idle"), true));
        assert_eq!(hub.current("s1"), Some(state("s1", "working")));
    }

    #[tokio::test]
    async fn local_working_blocked_idle_edges_are_not_coalesced() {
        let hub = Arc::new(AgentStateHub::new(8));
        let mut subscription = hub.subscribe("s1");
        for value in ["working", "blocked", "idle"] {
            hub.publish_canonical(state("s1", value));
        }
        for expected in ["working", "blocked", "idle"] {
            assert_eq!(
                subscription.receiver.recv().await.unwrap().state.state,
                expected
            );
        }
    }

    #[tokio::test]
    async fn legacy_working_blocked_idle_edges_are_not_coalesced() {
        let hub = Arc::new(AgentStateHub::new(8));
        hub.publish_legacy(state("s1", "idle"), true);
        let mut subscription = hub.subscribe("s1");
        for value in ["working", "blocked", "idle"] {
            hub.publish_canonical(state("s1", value));
        }
        for expected in ["working", "blocked", "idle"] {
            let update = subscription.receiver.recv().await.unwrap();
            assert_eq!(update.state.state, expected);
            assert!(!update.is_snapshot);
        }
    }

    #[tokio::test]
    async fn lag_recovery_reads_retained_current_state() {
        let hub = Arc::new(AgentStateHub::new(1));
        let mut subscription = hub.subscribe("s1");
        hub.publish_canonical(state("s1", "working"));
        hub.publish_canonical(state("s1", "blocked"));
        assert!(matches!(
            subscription.receiver.recv().await,
            Err(broadcast::error::RecvError::Lagged(_))
        ));
        assert_eq!(hub.current("s1"), Some(state("s1", "blocked")));
    }

    #[test]
    fn removing_session_clears_retained_snapshot() {
        let hub = Arc::new(AgentStateHub::new(8));
        hub.publish_canonical(state("s1", "working"));
        hub.remove("s1");
        assert!(hub.subscribe("s1").snapshot.is_none());
    }

    #[test]
    fn predecessor_backlog_is_a_quiet_baseline_not_a_new_completion() {
        let hub = Arc::new(AgentStateHub::new(8));
        let mut subscription = hub.subscribe("s1");
        hub.publish_legacy(state("s1", "idle"), false);
        assert!(subscription.receiver.try_recv().unwrap().is_snapshot);
        assert!(!hub.publish_legacy(state("s1", "working"), false));
    }

    #[test]
    fn lag_resynchronization_discards_queued_states_older_than_snapshot() {
        let hub = Arc::new(AgentStateHub::new(2));
        let mut subscription = hub.subscribe("s1");
        for value in ["working", "blocked", "idle"] {
            hub.publish_canonical(state("s1", value));
        }
        assert!(matches!(
            subscription.receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ));
        assert_eq!(subscription.resynchronize("s1"), Some(state("s1", "idle")));
        assert!(matches!(
            subscription.receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }
}
