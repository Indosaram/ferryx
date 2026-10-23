use crate::daemon::protocol::{AgentProviderSession, AgentStateOrigin};
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
            if previous.origin == AgentStateOrigin::Agent {
                state.origin = AgentStateOrigin::Agent;
            }
        }
        retained.insert(state.session_id.clone(), state.clone());
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
        self.retained.lock().remove(session_id);
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
        }
    }

    fn provider(id: &str) -> AgentProviderSession {
        AgentProviderSession {
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
