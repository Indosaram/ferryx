//! The daemon side of the parent watch: who a pane's spawn parent is, whether that parent can
//! take a line, and the tick that types the lines a child's block or stall earns.
//!
//! The judgment lives in [`crate::agent_control::parent_watch`], which holds no clock and does
//! no IO. This module supplies the two facts the bus cannot know (a pane's lineage, and whether
//! its parent still exists) and performs the send. Nothing here retries: a notice the watcher
//! does not return is never sent, and a parent that went away is dropped by the watcher itself.

use std::sync::{Arc, Weak};

use crate::agent_control::parent_watch::{
    ChildFacts, ParentNotice, ParentPresence, ParentWatchConfig, ParentWatchDriver, WatchSource,
};
use crate::remote::RemoteGatewayState;

/// How often the daemon judges watched children. A block is noticed by the observer first; this
/// only bounds how long a notice waits for the parent's next idle reading.
pub const PARENT_WATCH_TICK: std::time::Duration = std::time::Duration::from_secs(2);

fn facts_for(services: &crate::daemon::MachineServices, session_id: &str) -> Option<ChildFacts> {
    let lineage = services.sessions.lineage_for(session_id)?;
    Some(ChildFacts { parent: lineage.parent, name: lineage.name, reason: None })
}

fn presence_for(services: &crate::daemon::MachineServices, parent_session_id: &str) -> ParentPresence {
    if !services.sessions.session_is_live(parent_session_id) {
        return ParentPresence::Gone;
    }
    // A parent mid-turn is never interrupted; it hears about the child on its next idle.
    match services.sessions.agent_states.current(parent_session_id) {
        Some(state) if state.state == "working" => ParentPresence::Working,
        _ => ParentPresence::Idle,
    }
}

/// Spawn lineage and parent liveness, read from the daemon's own session metadata.
pub struct DaemonWatchSource {
    services: Arc<crate::daemon::MachineServices>,
}

impl DaemonWatchSource {
    pub fn new(services: Arc<crate::daemon::MachineServices>) -> Self {
        Self { services }
    }
}

impl WatchSource for DaemonWatchSource {
    fn facts(&self, session_id: &str) -> Option<ChildFacts> {
        facts_for(&self.services, session_id)
    }

    fn presence(&self, parent_session_id: &str) -> ParentPresence {
        presence_for(&self.services, parent_session_id)
    }
}

/// Weak-backed watch source for [`spawn`]: never keeps `MachineServices` or `DaemonSessionService`
/// alive, so dropping the gateway closes the lineage watch channel and exits a parked task.
struct WeakWatchSource {
    services: Weak<crate::daemon::MachineServices>,
}

impl WatchSource for WeakWatchSource {
    fn facts(&self, session_id: &str) -> Option<ChildFacts> {
        let services = self.services.upgrade()?;
        facts_for(&services, session_id)
    }

    fn presence(&self, parent_session_id: &str) -> ParentPresence {
        let Some(services) = self.services.upgrade() else {
            return ParentPresence::Gone;
        };
        presence_for(&services, parent_session_id)
    }
}

/// One tick of the parent watch: judge every watched child, then type what each parent can take.
///
/// A send that fails is logged and not retried. The watcher already recorded the episode as
/// announced, so a broken pane cannot turn into a retry loop against it.
pub async fn tick(state: &Arc<RemoteGatewayState>, driver: &mut ParentWatchDriver, now_ms: i64) {
    let notices = driver.tick(&state.agent_events, now_ms);
    for notice in notices {
        if let Err(error) = deliver(state, &notice).await {
            tracing::warn!(
                parent = %notice.parent_session_id,
                child = %notice.child_session_id,
                %error,
                "parent watch notice was not delivered"
            );
        }
    }
}

/// Type one notice into its parent pane. Never answers, never resolves: it is a line of text the
/// parent agent reads like anything the person typed.
async fn deliver(state: &Arc<RemoteGatewayState>, notice: &ParentNotice) -> Result<(), String> {
    crate::remote::server::agent_control_service::send_text(
        state,
        notice.parent_session_id.clone(),
        notice.text.clone(),
        Some(format!("parent-watch:{}:{}", notice.child_session_id, notice.episode)),
        None,
        None,
    )
    .await
    .map(|_| ())
    .map_err(|error| error.message)
}

/// Start the tick loop for the life of the gateway. Returns `None` when this gateway has no
/// session services (legacy and test constructors), so nothing is watched there.
pub fn spawn(state: &Arc<RemoteGatewayState>) -> Option<tokio::task::JoinHandle<()>> {
    let services = state.machine_services.as_ref()?;
    let mut lineage_rx = services.sessions.subscribe_lineage();
    let driver = ParentWatchDriver::new(
        ParentWatchConfig::default(),
        Arc::new(WeakWatchSource {
            services: Arc::downgrade(services),
        }),
    );
    let state = Arc::downgrade(state);
    Some(tokio::spawn(async move {
        let mut driver = driver;
        let mut ticker = tokio::time::interval(PARENT_WATCH_TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            // Observe the current lineage generation before checking so a child registered
            // between the check and `changed().await` wakes immediately instead of parking.
            lineage_rx.borrow_and_update();
            let active = {
                let Some(state) = state.upgrade() else {
                    break;
                };
                let has_lineage = state
                    .machine_services
                    .as_ref()
                    .is_some_and(|services| services.sessions.has_lineage_children());
                has_lineage || driver.watcher().watched_children() > 0
            };
            if !active {
                // No watched children exist: park with zero timer wakeups until a child pane
                // with lineage is spawned or the session service is dropped.
                if lineage_rx.changed().await.is_err() {
                    break;
                }
                continue;
            }
            ticker.tick().await;
            let Some(state) = state.upgrade() else {
                break;
            };
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX));
            tick(&state, &mut driver, now_ms).await;
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn parked_parent_notice_exits_when_gateway_drops() {
        let server = crate::daemon::server::DaemonServer::new();
        let handle = spawn(server.remote_state()).expect("gateway with machine services");
        tokio::task::yield_now().await;
        assert!(!handle.is_finished(), "watcher stays parked while gateway is alive");
        drop(server);
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("parked watcher exits promptly on drop")
            .expect("watcher task finishes cleanly");
    }
}
