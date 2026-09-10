use crate::daemon::protocol::AgentProviderSession;
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

    /// The successor owns the report socket. Predecessor frames are only a quiet
    /// baseline; replaying their backlog through multiple attaches must not mint live edges.
    pub fn publish_legacy(&self, state: AgentState, _is_snapshot: bool) -> bool {
        let mut retained = self.retained.lock();
        if retained.contains_key(&state.session_id) {
            return false;
        }
        retained.insert(state.session_id.clone(), state.clone());
        let _ = self.tx.send(AgentStateUpdate { state, is_snapshot: true });
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

    #[cfg(test)]
    pub fn current(&self, session_id: &str) -> Option<AgentState> {
        self.retained
            .lock()
            .get(session_id)
            .cloned()
    }

    pub fn remove(&self, session_id: &str) {
        self.retained.lock().remove(session_id);
    }
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
        }
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
