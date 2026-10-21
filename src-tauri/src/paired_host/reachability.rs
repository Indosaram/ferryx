//! Daemon-owned per-host reachability. It travels beside `HostView` in its own struct
//! and event, so the frozen `HostView` wire shape never changes (rolling-handover compat).
use crate::scoped_contracts::Epoch;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

/// Tauri/daemon event name; the payload is one `HostReachability` JSON object.
pub const PAIRED_HOST_REACHABILITY_CHANGED_EVENT: &str = "paired_host_reachability_changed";
/// Daemon capability that gates `DaemonRequest::PairedHostReachabilityList`.
pub const PAIRED_HOST_REACHABILITY_CAPABILITY: &str = "pairedHostReachabilityV1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReachabilityState {
    Checking,
    Online,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostReachability {
    pub host_id: String,
    pub generation: Epoch,
    pub state: ReachabilityState,
    pub checked_at_ms: u64,
    pub last_error: Option<String>,
}

impl HostReachability {
    /// A host nobody has probed yet for this generation.
    pub fn checking(host_id: String, generation: Epoch) -> Self {
        Self {
            host_id,
            generation,
            state: ReachabilityState::Checking,
            checked_at_ms: 0,
            last_error: None,
        }
    }
}

pub type ReachabilitySink = Arc<dyn Fn(HostReachability) + Send + Sync>;

/// Last recorded reachability per host, tagged with the generation it was recorded for.
#[derive(Default)]
pub(crate) struct ReachabilityBook {
    map: HashMap<String, HostReachability>,
}

impl ReachabilityBook {
    /// Stores `entry` and reports whether it must be published: only when its state differs
    /// from the previous state recorded for the same host and generation.
    pub(crate) fn record(&mut self, entry: HostReachability) -> bool {
        let changed = self.map.get(&entry.host_id).is_none_or(|previous| {
            previous.generation != entry.generation || previous.state != entry.state
        });
        self.map.insert(entry.host_id.clone(), entry);
        changed
    }

    /// The entry for exactly this generation; an older generation's entry reads as absent.
    pub(crate) fn get(&self, host_id: &str, generation: Epoch) -> Option<&HostReachability> {
        self.map
            .get(host_id)
            .filter(|entry| entry.generation == generation)
    }

    pub(crate) fn remove(&mut self, host_id: &str) {
        self.map.remove(host_id);
    }
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "reachability_tests.rs"]
mod tests;
