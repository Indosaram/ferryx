//! Renderer-facing view of the daemon's per-host direct route (`direct_route`). It travels in
//! its own struct and event, beside the frozen `HostView`/`HostReachability` shapes. The
//! loopback origin of a direct route is deliberately absent: an address never reaches the
//! renderer, only which path is live.
use super::direct_route::{DirectRoutes, RouteEvent, RouteState};
use crate::daemon::protocol::DaemonRemoteEvent;
use serde::{Deserialize, Serialize};
use std::future::Future;
use tokio::sync::broadcast;

/// Tauri/daemon event name; the payload is one `HostRoute` JSON object.
pub const PAIRED_HOST_ROUTE_CHANGED_EVENT: &str = "paired_host_route_changed";
/// Daemon capability that gates `DaemonRequest::PairedHostRouteList`.
pub const PAIRED_HOST_ROUTE_CAPABILITY: &str = "pairedHostRouteV1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HostRouteState {
    Relay,
    Attempting,
    Direct,
    Disconnected,
}

impl From<&RouteState> for HostRouteState {
    fn from(state: &RouteState) -> Self {
        match state {
            RouteState::Relay => Self::Relay,
            RouteState::Attempting => Self::Attempting,
            RouteState::Direct { .. } => Self::Direct,
            RouteState::Disconnected => Self::Disconnected,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostRoute {
    pub host_id: String,
    pub state: HostRouteState,
}

impl HostRoute {
    pub fn current(routes: &DirectRoutes, host_id: String) -> Self {
        let state = HostRouteState::from(&routes.state(&host_id));
        Self { host_id, state }
    }

    fn from_event(event: &RouteEvent) -> Self {
        Self {
            host_id: event.host_id.clone(),
            state: HostRouteState::from(&event.state),
        }
    }

    pub fn remote_event(&self) -> DaemonRemoteEvent {
        DaemonRemoteEvent {
            event: PAIRED_HOST_ROUTE_CHANGED_EVENT.to_string(),
            payload: serde_json::to_value(self).unwrap_or(serde_json::Value::Null),
        }
    }
}

/// Forwards every route transition to the daemon's remote-event broadcast. `events` must be
/// subscribed before the caller can serve a route list, so no transition falls between them.
/// When the receiver lags, the dropped transitions are replaced by the current state of every
/// listed host. Stops when `alive` turns false or the route channel closes.
pub async fn forward_route_events<L, Fut, E>(
    routes: DirectRoutes,
    mut events: broadcast::Receiver<RouteEvent>,
    sink: broadcast::Sender<DaemonRemoteEvent>,
    list_hosts: L,
    alive: impl Fn() -> bool,
) where
    L: Fn() -> Fut,
    Fut: Future<Output = Result<Vec<String>, E>>,
    E: std::fmt::Debug,
{
    loop {
        let received = events.recv().await;
        if !alive() {
            return;
        }
        match received {
            Ok(event) => {
                let _ = sink.send(HostRoute::from_event(&event).remote_event());
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => match list_hosts().await {
                Ok(hosts) => {
                    for host_id in hosts {
                        let _ = sink.send(HostRoute::current(&routes, host_id).remote_event());
                    }
                }
                Err(error) => {
                    tracing::warn!("paired route resync after {skipped} dropped events failed: {error:?}");
                }
            },
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

#[cfg(test)]
#[path = "route_view_tests.rs"]
mod tests;
