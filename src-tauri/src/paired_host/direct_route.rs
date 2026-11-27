//! Per-host direct (P2P) route cache.
//!
//! The relay is always the default and is never delayed by a direct attempt:
//! callers read [`DirectRoutes::origin_for`] synchronously and get `None`
//! (use the relay) unless a verified direct connection is already published.
//! Attempts run in the background (see `direct_connect`); a lost connection is
//! marked `Disconnected` synchronously from its `closed()` watcher, which also
//! drops the loopback adapter, so the next request goes back to the relay.
use parking_lot::Mutex;
use super::direct::datagram::{DatagramMux, DirectDatagramPeer};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// Retry backoff after a failed attempt or a lost connection.
pub const RETRY_BASE: Duration = Duration::from_secs(5);
pub const RETRY_MAX: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteState {
    Relay,
    Attempting,
    Direct { origin: String },
    Ssh,
    Disconnected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteEvent {
    pub host_id: String,
    pub epoch: u64,
    pub state: RouteState,
}

/// Owns everything a live direct route keeps alive. Dropping it aborts the
/// loopback adapter and watcher tasks and closes the QUIC connection/endpoint;
/// a bare `JoinHandle` drop would only detach them.
#[derive(Default)]
pub struct DirectGuard {
    tasks: Vec<tokio::task::AbortHandle>,
    connection: Option<quinn::Connection>,
    endpoint: Option<quinn::Endpoint>,
    datagram_mux: Option<Arc<DatagramMux>>,
}

impl DirectGuard {
    pub fn new(connection: quinn::Connection, endpoint: quinn::Endpoint) -> Self {
        Self {
            tasks: Vec::new(),
            connection: Some(connection),
            endpoint: Some(endpoint),
            datagram_mux: None,
        }
    }
    pub fn track(&mut self, task: tokio::task::AbortHandle) {
        self.tasks.push(task);
    }
    /// Attaches the mux fed by this connection's datagram reader (track the reader too).
    pub fn with_datagram_mux(mut self, mux: Arc<DatagramMux>) -> Self {
        self.datagram_mux = Some(mux);
        self
    }
    /// `None` without a connection or a datagram mux.
    pub fn datagram_peer(&self) -> Option<DirectDatagramPeer> {
        Some(DirectDatagramPeer {
            connection: self.connection.clone()?,
            mux: Arc::clone(self.datagram_mux.as_ref()?),
        })
    }
}

impl Drop for DirectGuard {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        if let Some(mux) = &self.datagram_mux {
            mux.clear();
        }
        if let Some(connection) = &self.connection {
            connection.close(quinn::VarInt::from_u32(0), b"route closed");
        }
        if let Some(endpoint) = &self.endpoint {
            endpoint.close(quinn::VarInt::from_u32(0), b"route closed");
        }
    }
}

struct Entry {
    state: RouteState,
    epoch: u64,
    failures: u32,
    retry_at: Option<Instant>,
    guard: Option<DirectGuard>,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            state: RouteState::Relay,
            epoch: 0,
            failures: 0,
            retry_at: None,
            guard: None,
        }
    }
}

#[derive(Clone)]
pub struct DirectRoutes {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
    events: broadcast::Sender<RouteEvent>,
    // Never reused, even across reset(), so a stale attempt cannot publish.
    next_epoch: Arc<std::sync::atomic::AtomicU64>,
}

impl Default for DirectRoutes {
    fn default() -> Self {
        Self::new()
    }
}

pub static GLOBAL_DIRECT_ROUTES: LazyLock<DirectRoutes> = LazyLock::new(DirectRoutes::new);

fn backoff(failures: u32) -> Duration {
    let shift = failures.saturating_sub(1).min(6);
    (RETRY_BASE * (1u32 << shift)).min(RETRY_MAX)
}

impl DirectRoutes {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            events,
            next_epoch: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Subscribe BEFORE triggering an attempt to observe every transition.
    pub fn subscribe(&self) -> broadcast::Receiver<RouteEvent> {
        self.events.subscribe()
    }

    fn emit(&self, host_id: &str, epoch: u64, state: RouteState) {
        let _ = self.events.send(RouteEvent {
            host_id: host_id.to_owned(),
            epoch,
            state,
        });
    }

    pub fn state(&self, host_id: &str) -> RouteState {
        self.entries
            .lock()
            .get(host_id)
            .map(|e| e.state.clone())
            .unwrap_or(RouteState::Relay)
    }

    /// The loopback adapter origin if a verified direct route is live.
    pub fn origin_for(&self, host_id: &str) -> Option<String> {
        match self.entries.lock().get(host_id).map(|e| &e.state) {
            Some(RouteState::Direct { origin }) => Some(origin.clone()),
            _ => None,
        }
    }

    /// Origin plus the epoch that published it; failures must report that epoch
    /// so a stale request cannot tear down a newer route.
    pub fn direct_for(&self, host_id: &str) -> Option<(String, u64)> {
        match self.entries.lock().get(host_id) {
            Some(Entry { state: RouteState::Direct { origin }, epoch, .. }) => Some((origin.clone(), *epoch)),
            _ => None,
        }
    }

    /// Datagram access to the live direct route, only for the epoch the caller resolved.
    pub fn datagram_handle(&self, host_id: &str, epoch: u64) -> Option<DirectDatagramPeer> {
        match self.entries.lock().get(host_id) {
            Some(Entry {
                state: RouteState::Direct { .. },
                epoch: current,
                guard: Some(guard),
                ..
            }) if *current == epoch => guard.datagram_peer(),
            _ => None,
        }
    }

    /// True when this host is parked inside a retry backoff window.
    ///
    /// An attempt in flight or an already direct route is *not* backoff: a caller
    /// that waits on those may still be promoted. A host with no entry is not in
    /// backoff either, so a cold host keeps its legitimate first-attempt wait
    /// (its state reads as `Relay` before any attempt starts).
    pub fn in_backoff(&self, host_id: &str, now: Instant) -> bool {
        self.entries.lock().get(host_id).is_some_and(|entry| {
            !matches!(entry.state, RouteState::Attempting | RouteState::Direct { .. })
                && entry.retry_at.is_some_and(|at| at > now)
        })
    }

    /// Claims the next attempt. `None` while one is running, already direct,
    /// or still inside the retry backoff window.
    pub fn begin_attempt(&self, host_id: &str, now: Instant) -> Option<u64> {
        let epoch = {
            let mut map = self.entries.lock();
            let entry = map.entry(host_id.to_owned()).or_default();
            if matches!(entry.state, RouteState::Attempting | RouteState::Direct { .. }) {
                return None;
            }
            if entry.retry_at.is_some_and(|at| now < at) {
                return None;
            }
            entry.epoch = self
                .next_epoch
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            entry.state = RouteState::Attempting;
            entry.epoch
        };
        self.emit(host_id, epoch, RouteState::Attempting);
        Some(epoch)
    }

    /// Publishes a verified direct route. A stale epoch drops the guard.
    pub fn publish(&self, host_id: &str, epoch: u64, origin: String, guard: DirectGuard) -> bool {
        let stale = {
            let mut map = self.entries.lock();
            match map.get_mut(host_id) {
                Some(entry) if entry.epoch == epoch && entry.state == RouteState::Attempting => {
                    entry.state = RouteState::Direct {
                        origin: origin.clone(),
                    };
                    entry.failures = 0;
                    entry.retry_at = None;
                    entry.guard = Some(guard);
                    None
                }
                _ => Some(guard),
            }
        };
        if stale.is_some() {
            return false;
        }
        self.emit(host_id, epoch, RouteState::Direct { origin });
        true
    }

    /// An attempt did not produce a direct route: stay on the relay and back off.
    pub fn fail(&self, host_id: &str, epoch: u64, now: Instant) {
        if self.settle(host_id, epoch, now, RouteState::Relay, false) {
            self.emit(host_id, epoch, RouteState::Relay);
        }
    }

    /// Called synchronously from the connection's `closed()` watcher.
    pub fn mark_disconnected(&self, host_id: &str, epoch: u64, now: Instant) {
        if self.settle(host_id, epoch, now, RouteState::Disconnected, true) {
            self.emit(host_id, epoch, RouteState::Disconnected);
        }
    }

    pub fn record_active_transport(
        &self,
        host_id: &str,
        path: super::path_select::AttachPath,
    ) {
        use super::path_select::AttachPath;
        let next = match path {
            AttachPath::SshForward => RouteState::Ssh,
            AttachPath::Relay => RouteState::Relay,
            AttachPath::Lan | AttachPath::Direct | AttachPath::Tailscale => return,
        };
        let emitted = {
            let mut map = self.entries.lock();
            let entry = map.entry(host_id.to_owned()).or_default();
            if matches!(entry.state, RouteState::Direct { .. } | RouteState::Attempting) {
                None
            } else if entry.state != next {
                entry.state = next.clone();
                Some(entry.epoch)
            } else {
                None
            }
        };
        if let Some(epoch) = emitted {
            self.emit(host_id, epoch, next);
        }
    }

    /// Drops any direct route (trust revoked, host forgotten, re-pair).
    pub fn reset(&self, host_id: &str) {
        let removed = self.entries.lock().remove(host_id);
        if let Some(entry) = removed {
            drop(entry.guard);
            self.emit(host_id, entry.epoch, RouteState::Relay);
        }
    }

    fn settle(&self, host_id: &str, epoch: u64, now: Instant, to: RouteState, from_direct: bool) -> bool {
        let guard = {
            let mut map = self.entries.lock();
            let Some(entry) = map.get_mut(host_id) else {
                return false;
            };
            let expected = if from_direct {
                matches!(entry.state, RouteState::Direct { .. })
            } else {
                entry.state == RouteState::Attempting
            };
            if entry.epoch != epoch || !expected {
                return false;
            }
            entry.state = to;
            entry.failures = entry.failures.saturating_add(1);
            entry.retry_at = Some(now + backoff(entry.failures));
            entry.guard.take()
        };
        // Drop outside the lock: closing the connection may run watcher code.
        drop(guard);
        true
    }
}

#[cfg(test)]
#[path = "direct_route_tests.rs"]
mod tests;
