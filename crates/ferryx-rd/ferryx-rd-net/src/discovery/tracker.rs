use super::DiscoveredHost;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Hard upper bound on tracked hosts so a hostile or noisy LAN cannot grow the map without bound.
const MAX_TRACKED_HOSTS: usize = 256;
/// Entries not refreshed within this window are treated as stale and dropped.
const HOST_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
struct TrackedHost {
    host: DiscoveredHost,
    last_seen: Instant,
}

#[derive(Debug, Default)]
pub struct DiscoveryTracker {
    hosts: BTreeMap<String, TrackedHost>,
}

impl DiscoveryTracker {
    pub fn new() -> Self {
        Self {
            hosts: BTreeMap::new(),
        }
    }

    pub fn upsert(&mut self, host: DiscoveredHost) {
        self.upsert_at(host, Instant::now());
    }

    fn upsert_at(&mut self, host: DiscoveredHost, now: Instant) {
        self.hosts
            .retain(|_, entry| now.saturating_duration_since(entry.last_seen) < HOST_TTL);

        if !self.hosts.contains_key(&host.id) && self.hosts.len() >= MAX_TRACKED_HOSTS {
            let oldest = self
                .hosts
                .iter()
                .min_by_key(|(_, entry)| entry.last_seen)
                .map(|(id, _)| id.clone());
            if let Some(id) = oldest {
                self.hosts.remove(&id);
            }
        }

        self.hosts.insert(
            host.id.clone(),
            TrackedHost {
                host,
                last_seen: now,
            },
        );
    }

    pub fn remove(&mut self, id: &str) {
        self.hosts.remove(id);
    }

    pub fn snapshot(&self) -> Vec<DiscoveredHost> {
        self.snapshot_at(Instant::now())
    }

    fn snapshot_at(&self, now: Instant) -> Vec<DiscoveredHost> {
        self.hosts
            .values()
            .filter(|entry| now.saturating_duration_since(entry.last_seen) < HOST_TTL)
            .map(|entry| entry.host.clone())
            .collect()
    }

    pub fn clear(&mut self) {
        self.hosts.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(id: &str) -> DiscoveredHost {
        DiscoveredHost {
            id: id.into(),
            name: id.into(),
            ip: "192.168.1.50".into(),
            os: "linux".into(),
            tcp_port: 19730,
            udp_port: 19731,
            mac: None,
        }
    }

    #[test]
    fn tracker_caps_entries_at_max_tracked_hosts() {
        let mut tracker = DiscoveryTracker::new();
        let base = Instant::now();
        for i in 0..(MAX_TRACKED_HOSTS + 25) {
            tracker.upsert_at(host(&format!("h{i:04}._maho-rd._tcp.local.")), base);
        }
        assert_eq!(tracker.snapshot_at(base).len(), MAX_TRACKED_HOSTS);
    }

    #[test]
    fn tracker_evicts_least_recently_seen_when_full() {
        let mut tracker = DiscoveryTracker::new();
        let base = Instant::now();
        tracker.upsert_at(host("old._maho-rd._tcp.local."), base);
        for i in 0..(MAX_TRACKED_HOSTS - 1) {
            tracker.upsert_at(
                host(&format!("h{i:04}._maho-rd._tcp.local.")),
                base + Duration::from_secs(1),
            );
        }
        assert_eq!(tracker.snapshot_at(base).len(), MAX_TRACKED_HOSTS);

        tracker.upsert_at(
            host("new._maho-rd._tcp.local."),
            base + Duration::from_secs(2),
        );
        let ids: Vec<String> = tracker
            .snapshot_at(base + Duration::from_secs(2))
            .into_iter()
            .map(|h| h.id)
            .collect();
        assert_eq!(ids.len(), MAX_TRACKED_HOSTS);
        assert!(!ids.iter().any(|id| id == "old._maho-rd._tcp.local."));
        assert!(ids.iter().any(|id| id == "new._maho-rd._tcp.local."));
    }

    #[test]
    fn tracker_drops_entries_past_ttl() {
        let mut tracker = DiscoveryTracker::new();
        let base = Instant::now();
        tracker.upsert_at(host("stale._maho-rd._tcp.local."), base);
        assert_eq!(tracker.snapshot_at(base).len(), 1);

        let later = base + HOST_TTL + Duration::from_secs(1);
        assert!(tracker.snapshot_at(later).is_empty());

        tracker.upsert_at(host("fresh._maho-rd._tcp.local."), later);
        let ids: Vec<String> = tracker
            .snapshot_at(later)
            .into_iter()
            .map(|h| h.id)
            .collect();
        assert_eq!(ids, vec!["fresh._maho-rd._tcp.local.".to_string()]);
    }

    #[test]
    fn tracker_refreshed_entry_survives_ttl() {
        let mut tracker = DiscoveryTracker::new();
        let base = Instant::now();
        tracker.upsert_at(host("live._maho-rd._tcp.local."), base);
        let refreshed = base + HOST_TTL - Duration::from_secs(1);
        tracker.upsert_at(host("live._maho-rd._tcp.local."), refreshed);
        assert_eq!(tracker.snapshot_at(refreshed + HOST_TTL / 2).len(), 1);
    }
}
