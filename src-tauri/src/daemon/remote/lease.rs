use crate::terminal::output_hub::{BoundedBuffer, ReplayGap};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteLeaseState {
    Attached,
    Detached,
    Terminated,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePtyLease {
    pub session_id: String,
    pub target_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaf_id: Option<String>,
    pub state: RemoteLeaseState,
    #[serde(default)]
    pub pending_kill: bool,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detached_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminated_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    pub client_instance_id: String,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseError {
    NotFound(String),
    Inactive(RemoteLeaseState),
    GenerationMismatch {
        expected_client_instance_id: String,
        expected_generation: u64,
    },
}

#[derive(Debug, Default)]
pub struct RemoteLeaseStore {
    leases: HashMap<String, RemotePtyLease>,
}

impl RemoteLeaseStore {
    pub fn list(&self) -> Vec<RemotePtyLease> {
        let mut leases = self.leases.values().cloned().collect::<Vec<_>>();
        leases.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        leases
    }

    pub fn get(&self, session_id: &str) -> Option<&RemotePtyLease> {
        self.leases.get(session_id)
    }

    pub fn bind(
        &mut self,
        session_id: impl Into<String>,
        target_id: impl Into<String>,
        leaf_id: Option<String>,
        client_instance_id: impl Into<String>,
        generation: u64,
        now: u64,
    ) -> RemotePtyLease {
        let session_id = session_id.into();
        let target_id = target_id.into();
        let client_instance_id = client_instance_id.into();

        for lease in self.leases.values_mut() {
            let same_pane =
                lease.target_id == target_id && leaf_id.is_some() && lease.leaf_id == leaf_id;
            if same_pane
                && lease.session_id != session_id
                && matches!(
                    lease.state,
                    RemoteLeaseState::Attached | RemoteLeaseState::Detached
                )
            {
                lease.state = RemoteLeaseState::Expired;
                lease.pending_kill = false;
                lease.updated_at = now;
                lease.expires_at = Some(now);
            }
        }

        let lease = RemotePtyLease {
            session_id: session_id.clone(),
            target_id,
            leaf_id,
            state: RemoteLeaseState::Attached,
            pending_kill: false,
            created_at: now,
            updated_at: now,
            detached_at: None,
            terminated_at: None,
            expires_at: None,
            client_instance_id,
            generation,
        };
        self.leases.insert(session_id, lease.clone());
        lease
    }

    pub fn attach(
        &mut self,
        session_id: &str,
        client_instance_id: &str,
        generation: u64,
        now: u64,
    ) -> Result<RemotePtyLease, LeaseError> {
        let lease = self
            .leases
            .get_mut(session_id)
            .ok_or_else(|| LeaseError::NotFound(session_id.to_string()))?;
        if matches!(
            lease.state,
            RemoteLeaseState::Terminated | RemoteLeaseState::Expired
        ) {
            return Err(LeaseError::Inactive(lease.state));
        }
        // A reconnect from the same daemon instance advances the generation. Older
        // channels and channels owned by another daemon remain fenced out.
        if lease.client_instance_id != client_instance_id || generation < lease.generation {
            return Err(LeaseError::GenerationMismatch {
                expected_client_instance_id: lease.client_instance_id.clone(),
                expected_generation: lease.generation,
            });
        }
        lease.generation = generation;
        lease.state = RemoteLeaseState::Attached;
        lease.updated_at = now;
        lease.detached_at = None;
        Ok(lease.clone())
    }

    pub fn detach(&mut self, session_id: &str, now: u64) -> Result<RemotePtyLease, LeaseError> {
        let lease = self
            .leases
            .get_mut(session_id)
            .ok_or_else(|| LeaseError::NotFound(session_id.to_string()))?;
        if matches!(
            lease.state,
            RemoteLeaseState::Terminated | RemoteLeaseState::Expired
        ) {
            return Err(LeaseError::Inactive(lease.state));
        }
        lease.state = RemoteLeaseState::Detached;
        lease.updated_at = now;
        lease.detached_at = Some(now);
        Ok(lease.clone())
    }

    pub fn queue_terminate(
        &mut self,
        session_id: &str,
        now: u64,
    ) -> Result<RemotePtyLease, LeaseError> {
        let lease = self
            .leases
            .get_mut(session_id)
            .ok_or_else(|| LeaseError::NotFound(session_id.to_string()))?;
        if matches!(
            lease.state,
            RemoteLeaseState::Terminated | RemoteLeaseState::Expired
        ) {
            return Err(LeaseError::Inactive(lease.state));
        }
        lease.pending_kill = true;
        lease.updated_at = now;
        Ok(lease.clone())
    }

    pub fn terminate(&mut self, session_id: &str, now: u64) -> Result<RemotePtyLease, LeaseError> {
        let lease = self
            .leases
            .get_mut(session_id)
            .ok_or_else(|| LeaseError::NotFound(session_id.to_string()))?;
        if lease.state == RemoteLeaseState::Expired {
            return Err(LeaseError::Inactive(lease.state));
        }
        lease.state = RemoteLeaseState::Terminated;
        lease.pending_kill = false;
        lease.updated_at = now;
        lease.terminated_at = Some(now);
        Ok(lease.clone())
    }

    pub fn pending_terminations(&self) -> Vec<String> {
        let mut sessions = self
            .leases
            .values()
            .filter(|lease| lease.pending_kill)
            .map(|lease| lease.session_id.clone())
            .collect::<Vec<_>>();
        sessions.sort();
        sessions
    }

    /// Replays deferred Terminate requests in stable order. A lease is removed only
    /// after the remote endpoint acknowledges termination; failed requests remain
    /// pending for the next transport handshake.
    pub fn remove(&mut self, session_id: &str) -> Option<RemotePtyLease> {
        self.leases.remove(session_id)
    }

    pub fn replay_pending_terminations<F, E>(&mut self, mut terminate: F) -> Result<Vec<String>, E>
    where
        F: FnMut(&str) -> Result<(), E>,
    {
        let pending = self.pending_terminations();
        let mut terminated = Vec::with_capacity(pending.len());
        for session_id in pending {
            terminate(&session_id)?;
            self.leases.remove(&session_id);
            terminated.push(session_id);
        }
        Ok(terminated)
    }

    /// Removes leases whose remote PTYs are confirmed absent. Attached leases are
    /// never collected: loss of a liveness observation is not proof that an active
    /// consumer's PTY is dead.
    pub fn gc_dead_sessions(&mut self, live_session_ids: &HashSet<String>) -> Vec<String> {
        let mut removed = self
            .leases
            .values()
            .filter(|lease| {
                lease.state != RemoteLeaseState::Attached
                    && !live_session_ids.contains(&lease.session_id)
            })
            .map(|lease| lease.session_id.clone())
            .collect::<Vec<_>>();
        removed.sort();
        for session_id in &removed {
            self.leases.remove(session_id);
        }
        removed
    }
}

pub fn merge_replay_gap(
    buffer: &mut BoundedBuffer,
    gap: &ReplayGap,
    history: Vec<u8>,
) -> Option<u64> {
    buffer
        .merge_replay_gap(gap, history)
        .map(|chunk| chunk.sequence)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_session() -> RemoteLeaseStore {
        let mut store = RemoteLeaseStore::default();
        store.bind("old", "host-a", Some("leaf-a".into()), "client-a", 7, 10);
        store
    }

    #[test]
    fn superseding_a_pane_expires_without_terminating_the_old_lease() {
        let mut store = store_with_session();
        store.bind("new", "host-a", Some("leaf-a".into()), "client-a", 7, 20);

        let old = store.get("old").expect("old lease retained");
        assert_eq!(old.state, RemoteLeaseState::Expired);
        assert_eq!(old.expires_at, Some(20));
        assert_eq!(old.terminated_at, None);
        assert!(!old.pending_kill);
    }

    #[test]
    fn terminal_lease_states_refuse_reattach() {
        let mut expired = store_with_session();
        expired.bind("new", "host-a", Some("leaf-a".into()), "client-a", 7, 20);
        assert_eq!(
            expired.attach("old", "client-a", 7, 30),
            Err(LeaseError::Inactive(RemoteLeaseState::Expired))
        );

        let mut terminated = store_with_session();
        terminated.terminate("old", 20).expect("terminate");
        assert_eq!(
            terminated.attach("old", "client-a", 7, 30),
            Err(LeaseError::Inactive(RemoteLeaseState::Terminated))
        );
    }

    #[test]
    fn pending_kill_is_replayed_and_cleared_by_terminate_ack() {
        let mut store = store_with_session();
        store.queue_terminate("old", 20).expect("queue kill");
        assert_eq!(store.pending_terminations(), vec!["old"]);

        let terminated = store.terminate("old", 30).expect("terminate replay");
        assert_eq!(terminated.state, RemoteLeaseState::Terminated);
        assert!(!terminated.pending_kill);
        assert!(store.pending_terminations().is_empty());
    }

    #[test]
    fn reconnect_replays_terminate_and_removes_acknowledged_lease() {
        let mut store = store_with_session();
        store.detach("old", 20).expect("detach transport");
        store.queue_terminate("old", 30).expect("queue close");
        let mut requests = Vec::new();

        let removed = store
            .replay_pending_terminations(|session_id| {
                requests.push(session_id.to_string());
                Ok::<_, ()>(())
            })
            .expect("terminate acknowledged");

        assert_eq!(requests, vec!["old"]);
        assert_eq!(removed, vec!["old"]);
        assert!(store.get("old").is_none());
    }

    #[test]
    fn failed_terminate_replay_keeps_lease_pending() {
        let mut store = store_with_session();
        store.detach("old", 20).expect("detach transport");
        store.queue_terminate("old", 30).expect("queue close");

        assert_eq!(
            store.replay_pending_terminations(|_| Err("transport down")),
            Err("transport down")
        );
        assert!(store.get("old").expect("lease retained").pending_kill);
    }

    #[test]
    fn dead_session_gc_never_collects_attached_leases() {
        let mut store = store_with_session();
        store.bind("detached", "host-a", None, "client-a", 7, 10);
        store.detach("detached", 20).expect("detach stale lease");
        store.bind("live-detached", "host-a", None, "client-a", 7, 10);
        store
            .detach("live-detached", 20)
            .expect("detach live remote session");
        let live = HashSet::from(["live-detached".to_string()]);

        assert_eq!(store.gc_dead_sessions(&live), vec!["detached"]);
        assert!(store.get("old").is_some(), "attached lease must survive GC");
        assert!(store.get("live-detached").is_some());
        assert!(store.get("detached").is_none());
    }

    #[test]
    fn attach_is_generation_fenced() {
        let mut store = store_with_session();
        assert!(matches!(
            store.attach("old", "client-b", 7, 20),
            Err(LeaseError::GenerationMismatch { .. })
        ));
        assert!(store.attach("old", "client-a", 8, 20).is_ok());
        assert!(matches!(
            store.attach("old", "client-a", 7, 30),
            Err(LeaseError::GenerationMismatch { .. })
        ));
    }

    #[test]
    fn replay_gap_merges_remote_history_into_existing_ring_buffer() {
        let mut buffer = BoundedBuffer::new(32);
        buffer.push(b"local;".to_vec());
        let sequence = merge_replay_gap(
            &mut buffer,
            &ReplayGap {
                requested_after_sequence: 1,
                available_from_sequence: 5,
            },
            b"remote;".to_vec(),
        );

        assert_eq!(sequence, Some(5));
        assert_eq!(buffer.snapshot(), b"local;remote;");
        assert_eq!(buffer.end_sequence(), Some(5));
        assert_eq!(buffer.next_sequence(), 6);
    }
}
