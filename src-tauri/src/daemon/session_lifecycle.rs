use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Resource lifecycle of a logical terminal/agent session.
///
/// This is intentionally independent from agent activity (working/waiting/done):
/// a completed agent can still have a Running backing process until the idle policy
/// moves it to Hibernated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionProcessState {
    Standby,
    Running,
    Hibernated,
    Suspended,
}

impl Default for SessionProcessState {
    fn default() -> Self {
        Self::Standby
    }
}

#[derive(Debug, Clone)]
pub struct SessionLifecycleRecord {
    pub state: SessionProcessState,
    pub active: bool,
    pub idle_since: Option<Instant>,
}

impl Default for SessionLifecycleRecord {
    fn default() -> Self {
        Self {
            state: SessionProcessState::Standby,
            active: false,
            idle_since: None,
        }
    }
}

/// Backend-side lifecycle registry for logical sessions.
///
/// The daemon/session owner can use the registry without platform-specific process
/// primitives. Process creation/termination remains in terminal/session services;
/// this type owns only deterministic lifecycle and idle-policy decisions.
#[derive(Debug, Default)]
pub struct SessionLifecycleRegistry {
    records: HashMap<String, SessionLifecycleRecord>,
}

impl SessionLifecycleRegistry {
    pub fn insert_standby(&mut self, session_id: impl Into<String>) {
        self.records.insert(session_id.into(), SessionLifecycleRecord::default());
    }

    pub fn mark_running(&mut self, session_id: impl Into<String>) {
        let record = self.records.entry(session_id.into()).or_default();
        record.state = SessionProcessState::Running;
        record.idle_since = None;
    }

    pub fn mark_hibernated(&mut self, session_id: impl Into<String>) {
        let record = self.records.entry(session_id.into()).or_default();
        record.state = SessionProcessState::Hibernated;
        record.idle_since = None;
    }

    pub fn mark_suspended(&mut self, session_id: impl Into<String>) {
        let record = self.records.entry(session_id.into()).or_default();
        record.state = SessionProcessState::Suspended;
        record.idle_since = None;
    }

    pub fn set_active(&mut self, session_id: &str, active: bool) {
        let record = self.records.entry(session_id.to_owned()).or_default();
        record.active = active;
    }

    pub fn mark_idle_at(&mut self, session_id: &str, now: Instant) {
        let record = self.records.entry(session_id.to_owned()).or_default();
        if record.state == SessionProcessState::Running && record.idle_since.is_none() {
            record.idle_since = Some(now);
        }
    }

    pub fn mark_busy(&mut self, session_id: &str) {
        if let Some(record) = self.records.get_mut(session_id) {
            record.idle_since = None;
        }
    }

    pub fn state(&self, session_id: &str) -> Option<SessionProcessState> {
        self.records.get(session_id).map(|record| record.state)
    }

    pub fn remove(&mut self, session_id: &str) -> Option<SessionLifecycleRecord> {
        self.records.remove(session_id)
    }

    pub fn hibernation_candidates_at(
        &self,
        now: Instant,
        idle_timeout: Duration,
    ) -> Vec<String> {
        self.records
            .iter()
            .filter_map(|(session_id, record)| {
                let idle_since = record.idle_since?;
                (record.state == SessionProcessState::Running
                    && !record.active
                    && now.saturating_duration_since(idle_since) >= idle_timeout)
                    .then(|| session_id.clone())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_transitions_standby_running_hibernated() {
        let mut registry = SessionLifecycleRegistry::default();
        registry.insert_standby("session-a");
        assert_eq!(registry.state("session-a"), Some(SessionProcessState::Standby));

        registry.mark_running("session-a");
        assert_eq!(registry.state("session-a"), Some(SessionProcessState::Running));

        registry.mark_hibernated("session-a");
        assert_eq!(registry.state("session-a"), Some(SessionProcessState::Hibernated));

        registry.mark_suspended("session-a");
        assert_eq!(registry.state("session-a"), Some(SessionProcessState::Suspended));
    }

    #[test]
    fn only_background_running_idle_sessions_become_candidates() {
        let mut registry = SessionLifecycleRegistry::default();
        let start = Instant::now();

        registry.mark_running("background-idle");
        registry.mark_idle_at("background-idle", start);

        registry.mark_running("active-idle");
        registry.mark_idle_at("active-idle", start);
        registry.set_active("active-idle", true);

        registry.mark_hibernated("already-hibernated");
        registry.mark_idle_at("already-hibernated", start);

        registry.mark_running("busy");
        registry.mark_idle_at("busy", start);
        registry.mark_busy("busy");

        let candidates = registry.hibernation_candidates_at(
            start + Duration::from_secs(30 * 60),
            Duration::from_secs(30 * 60),
        );
        assert_eq!(candidates, vec!["background-idle".to_string()]);
    }

    #[test]
    fn idle_timeout_is_not_reached_early() {
        let mut registry = SessionLifecycleRegistry::default();
        let start = Instant::now();
        registry.mark_running("session-a");
        registry.mark_idle_at("session-a", start);

        let candidates = registry.hibernation_candidates_at(
            start + Duration::from_secs(29 * 60),
            Duration::from_secs(30 * 60),
        );
        assert!(candidates.is_empty());
    }
}
