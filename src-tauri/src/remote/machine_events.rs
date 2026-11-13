//! One machine-only sequence domain per workspace authority. Never use mirror event_tx.
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::broadcast;

pub struct MachineEvents {
    sequence: Mutex<u64>,
    sender: broadcast::Sender<Value>,
    /// Bounds concurrent inventory construction independently of socket count.
    pub snapshot_slots: Arc<tokio::sync::Semaphore>,
    subscriptions: tokio::sync::watch::Sender<usize>,
    pub(crate) forwarders: tokio::sync::watch::Sender<usize>,
    pub(crate) owner_streams: tokio::sync::watch::Sender<usize>,
    socket_slots: tokio::sync::Semaphore,
    /// Retained by refresh workers after socket cancellation until native watches drain.
    watch_slots: Arc<tokio::sync::Semaphore>,
}

#[cfg(test)]
#[path = "machine_event_cancellation_tests.rs"]
mod machine_event_cancellation_tests;
#[cfg(test)]
#[path = "machine_event_snapshot_race_tests.rs"]
mod machine_event_snapshot_race_tests;
#[cfg(all(test, unix))]
#[path = "machine_event_prompt_push_tests.rs"]
mod machine_event_prompt_push_tests;

/// The event type of one session's state transition for the Supervisor panel.
pub(crate) const PANEL_SESSION_EVENT: &str = "panel.session";

/// The `panel.session` frame for one agent observation.
///
/// Every machine socket gets the slim form: ids and state, never a prompt body. `session` (the
/// fleet entry with its pending prompt and last assistant line) is passed only for an owner
/// subscriber, which is what puts the question text on the wire for the owner alone.
pub(crate) fn panel_session_frame(
    host_id: &str,
    observation: &crate::agent_control::events::AgentObservation,
    session: Option<&ferryx_hub::supervisor::panel_contract::FleetSession>,
) -> String {
    let mut frame = observation.slim();
    frame["type"] = json!(PANEL_SESSION_EVENT);
    frame["sequence"] = json!(observation.sequence);
    frame["sessionRef"] = json!(ferryx_hub::supervisor::panel_contract::SessionRef::new(
        host_id,
        &observation.session_id
    ));
    if let Some(session) = session {
        frame["session"] = serde_json::to_value(session).unwrap_or(Value::Null);
    }
    frame.to_string()
}

/// The event type that tells a subscriber it may have missed panel frames and must re-read
/// `GET /api/v1/supervisor/panel`. It never carries a prompt body.
pub(crate) const PANEL_RESYNC_EVENT: &str = "panel.resync";

/// The `panel.resync` frame. `reason` is `lagged`, `backlog` or `sessionUnavailable`;
/// `skipped` is how many frames a lagged receiver lost; `session_ref` names the one session
/// whose owner frame could not be built.
pub(crate) fn panel_resync_frame(
    reason: &str,
    skipped: Option<u64>,
    session_ref: Option<&ferryx_hub::supervisor::panel_contract::SessionRef>,
) -> String {
    json!({
        "type": PANEL_RESYNC_EVENT,
        "reason": reason,
        "skipped": skipped,
        "sessionRef": session_ref,
    })
    .to_string()
}

/// Transitions queued for the per-socket worker that reads the owner's fleet entry.
const PANEL_WORK_QUEUE: usize = 64;

/// How long the worker waits for one owner fleet read; under the two seconds a frame is due in.
pub(crate) const PANEL_SESSION_READ: std::time::Duration = std::time::Duration::from_millis(1500);

/// One transition after the owner read: `session` is the fleet entry, `failed` says the owner
/// read did not produce one (an owner frame without `session` is invalid, so it is never sent).
struct PanelWork {
    observation: crate::agent_control::events::AgentObservation,
    session: Option<ferryx_hub::supervisor::panel_contract::FleetSession>,
    failed: bool,
}

/// The frame for `work`, decided at send time: `owner` is the device's current policy answer,
/// so a device that lost agent control while the read ran gets the slim frame and no body.
fn panel_work_frame(host_id: &str, work: &PanelWork, owner: bool) -> String {
    if owner && work.failed {
        return panel_resync_frame(
            "sessionUnavailable",
            None,
            Some(&ferryx_hub::supervisor::panel_contract::SessionRef::new(
                host_id,
                &work.observation.session_id,
            )),
        );
    }
    panel_session_frame(host_id, &work.observation, work.session.as_ref().filter(|_| owner))
}

/// The text of an owner-channel frame for this device right now: `None` unless the device
/// is currently an owner, so a decision's question never reaches anyone else.
fn owner_panel_frame(owner: bool, frame: &Value) -> Option<String> {
    owner.then(|| frame.to_string())
}

async fn push_text(socket: &mut axum::extract::ws::WebSocket, text: String) -> bool {
    matches!(
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            socket.send(axum::extract::ws::Message::Text(text.into())),
        )
        .await,
        Ok(Ok(()))
    )
}

/// Remembers each session's last state, prompt and read flag so only a transition makes a frame.
/// An acknowledgement on an unchanged screen flips only `read`, and still makes one.
#[derive(Default)]
pub(crate) struct PanelTransitions {
    seen: std::collections::HashMap<String, (crate::agent_control::AgentControlState, Option<String>, bool)>,
}

/// Sessions remembered at once; past it the memory restarts and one spare frame per session is sent.
const PANEL_TRANSITIONS_MAX: usize = 1024;

impl PanelTransitions {
    /// Forgets every remembered reading, so the next observation of each session is a transition.
    pub(crate) fn clear(&mut self) {
        self.seen.clear();
    }

    pub(crate) fn moved(&mut self, observation: &crate::agent_control::events::AgentObservation) -> bool {
        let reading = (observation.state, observation.prompt_id.clone(), observation.read);
        if self.seen.get(&observation.session_id) == Some(&reading) {
            return false;
        }
        if self.seen.len() >= PANEL_TRANSITIONS_MAX && !self.seen.contains_key(&observation.session_id) {
            self.seen.clear();
        }
        self.seen.insert(observation.session_id.clone(), reading);
        true
    }
}

impl Default for MachineEvents {
    fn default() -> Self {
        Self::new(64)
    }
}

impl MachineEvents {
    pub fn new(capacity: usize) -> Self {
        Self {
            sequence: Mutex::new(0),
            sender: broadcast::channel(capacity).0,
            snapshot_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            subscriptions: tokio::sync::watch::channel(0).0,
            forwarders: tokio::sync::watch::channel(0).0,
            owner_streams: tokio::sync::watch::channel(0).0,
            socket_slots: tokio::sync::Semaphore::new(16),
            watch_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        }
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.sender.subscribe()
    }
    pub fn sequence(&self) -> u64 {
        *self.sequence.lock()
    }
    pub fn forwarder_count(&self) -> tokio::sync::watch::Receiver<usize> {
        self.forwarders.subscribe()
    }
    pub fn owner_stream_count(&self) -> tokio::sync::watch::Receiver<usize> {
        self.owner_streams.subscribe()
    }
    pub fn subscription_count(&self) -> tokio::sync::watch::Receiver<usize> {
        self.subscriptions.subscribe()
    }
    pub(crate) fn publish(
        &self,
        kind: &str,
        workspace: Option<&str>,
        session: Option<&str>,
        payload: Value,
    ) {
        let _ = self.publish_sequenced(kind, workspace, session, payload);
    }

    /// Publish one event and return the sequence it was stored under.
    ///
    /// Agent control reuses this sequence so `agents events` and the events socket name the
    /// same change. A payload over the per-event cap is replaced with `inventoryInvalidated`
    /// and still consumes the sequence.
    pub(crate) fn publish_sequenced(
        &self,
        kind: &str,
        workspace: Option<&str>,
        session: Option<&str>,
        payload: Value,
    ) -> u64 {
        self.publish_event(None, kind, workspace, session, payload)
    }
    pub(crate) fn publish_revision(
        &self,
        revision: u64,
        kind: &str,
        workspace: Option<&str>,
        session: Option<&str>,
        payload: Value,
    ) {
        self.publish_event(Some(revision), kind, workspace, session, payload);
    }
    fn publish_event(
        &self,
        domain_revision: Option<u64>,
        kind: &str,
        workspace: Option<&str>,
        session: Option<&str>,
        payload: Value,
    ) -> u64 {
        let mut sequence = self.sequence.lock();
        *sequence = sequence
            .checked_add(1)
            .expect("machine event sequence exhausted");
        let mut event = json!({"sequence":sequence.to_string(),"revision":sequence.to_string(),"type":kind,
            "workspaceId":workspace,"sessionId":session,"payload":payload});
        if let Some(revision) = domain_revision {
            event[if session.is_some() {
                "sessionRevision"
            } else {
                "projectRevision"
            }] = json!(revision.to_string());
        }
        // Bound each retained slot as well as the slot count. A too-large change
        // is recoverable through the bounded authoritative snapshot, not truncation.
        if event.to_string().len() > 16384 {
            event = json!({"sequence":sequence.to_string(),"revision":sequence.to_string(),"type":"inventoryInvalidated","payload":{"completeness":"partial"}});
        }
        // No subscribers is normal; state is reconciled on the next subscription.
        let _ = self.sender.send(event);
        *sequence
    }
}

pub(crate) async fn serve(
    mut socket: axum::extract::ws::WebSocket,
    state: Arc<super::state::RemoteGatewayState>,
    mut receiver: broadcast::Receiver<Value>,
    device_id: String,
    token: String,
) {
    use axum::extract::ws::Message;
    use futures_util::StreamExt;
    let services = state
        .machine_services
        .as_ref()
        .expect("authorized machine services");
    let events = &services.workspaces.machine_events;
    let Ok(_socket_slot) = events.socket_slots.try_acquire() else {
        return;
    };
    struct Subscription<'a>(&'a MachineEvents);
    impl Drop for Subscription<'_> {
        fn drop(&mut self) {
            self.0.subscriptions.send_modify(|count| *count -= 1);
        }
    }
    let _subscription = Subscription(events);
    events.subscriptions.send_modify(|count| *count += 1);
    let mut watcher: Option<crate::daemon::workspace_service::workspace_watcher::WorkspaceWatch> =
        None;
    let mut reason = "subscribe";
    let mut stale_sequence = None;
    let mut metadata_forwarders = tokio::task::JoinSet::new();
    let mut forwarded_targets =
        std::collections::HashMap::<String, tokio::sync::watch::Receiver<bool>>::new();
    let (owner_changed, mut owner_changes) = tokio::sync::mpsc::channel(64);
    let mut agent_rx = state.agent_events.subscribe();
    let mut agent_open = true;
    // Subscribed before the inventory boundary, so a frame published after that boundary is queued.
    let mut history_rx = state.agent_events.subscribe_history();
    let mut history_open = true;
    let mut prompts = crate::agent_control::events::PromptChangeFilter::default();
    let mut panel = PanelTransitions::default();
    // Decision, action and rule frames go to owner subscribers only. Ownership is the device's
    // current agent-control permission, asked again for every frame, so `set_enabled(false)`,
    // `exclude` and `restrict_to` stop prompt bodies at once.
    let mut panel_rx = crate::hub_runtime::subscribe_panel_events();
    let mut panel_open = true;
    let is_owner: Arc<dyn Fn() -> bool + Send + Sync> = {
        let policy = Arc::clone(&state.agent_control_policy);
        Arc::new(move || policy.permits(&device_id))
    };
    let host_id = super::reference_chat::files::reference_host_id();
    // The owner's fleet read can take a second; it runs on this worker so the socket loop keeps
    // forwarding other events. Dropping the set on exit aborts the worker.
    let (work_tx, mut work_rx) = tokio::sync::mpsc::channel::<crate::agent_control::events::AgentObservation>(PANEL_WORK_QUEUE);
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<PanelWork>(PANEL_WORK_QUEUE);
    let mut panel_worker = tokio::task::JoinSet::new();
    panel_worker.spawn({
        let state = Arc::clone(&state);
        let is_owner = Arc::clone(&is_owner);
        async move {
            while let Some(observation) = work_rx.recv().await {
                let (session, failed) = if is_owner() {
                    match tokio::time::timeout(
                        PANEL_SESSION_READ,
                        crate::hub_runtime::local_fleet_session(&state, &observation.session_id),
                    )
                    .await
                    {
                        Ok(Some(session)) => (Some(session), false),
                        _ => (None, true),
                    }
                } else {
                    (None, false)
                };
                if out_tx.send(PanelWork { observation, session, failed }).await.is_err() {
                    break;
                }
            }
        }
    });
    loop {
        // The cursor is read BEFORE inventory construction. Any overlapping commit
        // is therefore replayed, never discarded as already in this boundary.
        let sequence = stale_sequence.take().unwrap_or_else(|| events.sequence());
        let workspaces = services.workspaces.clone();
        let sessions = services.sessions.clone();
        let epoch = crate::scoped_contracts::Epoch(
            state
                .daemon_epoch
                .load(std::sync::atomic::Ordering::Acquire),
        );
        let snapshot = {
            let snapshot = async {
                let slot = events
                    .watch_slots
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| crate::ipc::IpcError::internal("MACHINE_SERVICE_UNAVAILABLE"))?;
                let watch = match watcher.take() {
                    Some(watch) => watch,
                    None => {
                        crate::daemon::workspace_service::workspace_watcher::WorkspaceWatch::new()
                            .map_err(crate::ipc::IpcError::internal)?
                    }
                };
                let watch = watch
                    .refresh_bounded(workspaces.clone(), slot)
                    .await
                    .map_err(crate::ipc::IpcError::internal)?;
                let _slot =
                    events.snapshot_slots.acquire().await.map_err(|_| {
                        crate::ipc::IpcError::internal("MACHINE_SERVICE_UNAVAILABLE")
                    })?;
                let catalog = match super::workspace_api::event_projects(workspaces).await {
                    Ok(catalog) => catalog,
                    Err(error) if error == "STALE_REVISION" => {
                        return Ok((
                            json!({"completeness":"partial","error":"STALE_REVISION"}),
                            watch,
                        ))
                    }
                    Err(error) => return Err(crate::ipc::IpcError::internal(error)),
                };
                let mut inventory = sessions
                    .machine_sessions_routed(epoch)
                    .await
                    .map_err(crate::ipc::IpcError::internal)?;
                sessions.merge_legacy_peer_sessions(&mut inventory, &token).await;
                for session in &inventory.sessions {
                    if sessions
                        .router()
                        .find_legacy_peer_for_session(&session.target.session_id)
                        .is_some()
                        && !forwarded_targets.contains_key(&session.target.session_id)
                    {
                        let (status, current) = tokio::sync::watch::channel(false);
                        forwarded_targets.insert(session.target.session_id.clone(), current);
                        let changed = owner_changed.clone();
                        let owner = sessions.clone();
                        let target = session.target.clone();
                        metadata_forwarders.spawn(async move {
                            for attempt in 0..3 {
                                let mut updates = status.subscribe();
                                let forwarding = owner.forward_metadata(target.clone(), status.clone());
                                tokio::pin!(forwarding);
                                let result = loop {
                                    tokio::select! {
                                        result = &mut forwarding => break result,
                                        changed_status = updates.changed() => {
                                            if changed_status.is_ok() { let _sent = changed.send("ownerRecovered").await; }
                                        }
                                    }
                                };
                                match result {
                                    Ok(()) => return target.session_id,
                                    Err(error) => tracing::debug!(%error, attempt, "Predecessor metadata stream unavailable"),
                                }
                                status.send_replace(false);
                                let _sent = changed.send("ownerDisconnected").await;
                                if attempt < 2 { tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await; }
                            }
                            target.session_id
                        });
                    }
                }
                for session in &inventory.sessions {
                    if forwarded_targets
                        .get(&session.target.session_id)
                        .is_some_and(|status| !*status.borrow())
                    {
                        inventory.completeness = super::machine_protocol::Completeness::Partial;
                        if !inventory
                            .unavailable_workspace_ids
                            .contains(&session.workspace_id)
                        {
                            inventory
                                .unavailable_workspace_ids
                                .push(session.workspace_id.clone());
                        }
                    }
                }
                let sessions = inventory;
                let payload = json!({"desktopLocalMembership": if catalog.desktop_local_snapshot_exists {
                    Some(json!({"scope":"desktopLocal","invalidated":true}))
                } else { None }, "projects":catalog,"sessions":sessions});
                if payload.to_string().len() > 1024 * 1024 {
                    return Err(crate::ipc::IpcError::internal("OUTPUT_LIMIT_EXCEEDED"));
                }
                Ok::<_, crate::ipc::IpcError>((payload, watch))
            };
            let snapshot = async {
                tokio::time::timeout(std::time::Duration::from_secs(10), snapshot)
                    .await
                    .map_err(|_| crate::ipc::IpcError::internal("TIMEOUT"))?
            };
            tokio::pin!(snapshot);
            loop {
                tokio::select! {
                    result = &mut snapshot => break result,
                    incoming = socket.next() => match incoming {
                        None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                        _ => {},
                    }
                }
            }
        };
        let (payload, refreshed_watch) = match snapshot {
            Ok((value, watch)) => (value, Some(watch)),
            Err(error) => {
                tracing::warn!(%error, "Machine inventory snapshot failed");
                (
                    json!({"completeness":"partial","error":"MACHINE_SERVICE_UNAVAILABLE"}),
                    None,
                )
            }
        };
        let revision = sequence.to_string();
        let completeness = if payload["sessions"]["completeness"] == "partial"
            || payload["projects"]["completeness"] == "partial"
            || payload["completeness"] == "partial"
        {
            "partial"
        } else {
            "complete"
        };
        let mut payload = payload;
        payload["completeness"] = json!(completeness);
        let boundary = json!({"sequence":sequence.to_string(),"revision":revision,
            "type":"inventoryInvalidated","reason":reason,"payload":payload});
        if !matches!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                socket.send(Message::Text(boundary.to_string().into()))
            )
            .await,
            Ok(Ok(()))
        ) {
            return;
        }
        let Some(mut watch) = refreshed_watch else {
            let _closed = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                socket.send(Message::Close(None)),
            )
            .await;
            return;
        };
        if payload["error"] == "STALE_REVISION" {
            watcher = Some(watch);
            stale_sequence = Some(sequence);
            reason = "concurrentCommit";
            continue;
        }
        loop {
            tokio::select! {
                incoming = socket.next() => match incoming {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                    _ => {},
                },
                Some(change) = owner_changes.recv() => { reason = change; break; },
                Some(completed) = metadata_forwarders.join_next() => {
                    match completed {
                        Ok(id) => { if let Some(status) = forwarded_targets.get_mut(&id) { let (ended, receiver) = tokio::sync::watch::channel(false); drop(ended); *status = receiver; } },
                        Err(error) => {
                            tracing::warn!(%error, "Metadata forwarder failed");
                            for status in forwarded_targets.values_mut() { let (ended, receiver) = tokio::sync::watch::channel(false); drop(ended); *status = receiver; }
                        }
                    }
                    reason = "ownerUnavailable"; break;
                },
                _ = watch.changed() => { reason = "filesystem"; break; },
                observation = agent_rx.recv(), if agent_open => match observation {
                    Ok(observation) => {
                        if let Some(frame) = prompts.frame(&observation) {
                            if !matches!(tokio::time::timeout(std::time::Duration::from_secs(10), socket.send(Message::Text(frame.into()))).await, Ok(Ok(()))) { return; }
                        }
                        if panel.moved(&observation) && work_tx.try_send(observation).is_err() {
                            // The worker is saturated: say so instead of dropping the transition.
                            panel.clear();
                            if !push_text(&mut socket, panel_resync_frame("backlog", None, None)).await { return; }
                        }
                    },
                    // A lagged reader loses pushes: tell the client to re-read the panel, and make
                    // the next reading of every session a transition again.
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        panel.clear();
                        if !push_text(&mut socket, panel_resync_frame("lagged", Some(skipped), None)).await { return; }
                    },
                    Err(broadcast::error::RecvError::Closed) => agent_open = false,
                },
                Some(work) = out_rx.recv() => {
                    let frame = panel_work_frame(&host_id, &work, is_owner());
                    if !push_text(&mut socket, frame).await { return; }
                },
                frame = panel_rx.recv(), if panel_open => match frame {
                    Ok(frame) => {
                        let Some(text) = owner_panel_frame(is_owner(), &frame) else { continue };
                        if !push_text(&mut socket, text).await { return; }
                    },
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        if is_owner() && !push_text(&mut socket, panel_resync_frame("lagged", Some(skipped), None)).await { return; }
                    },
                    Err(broadcast::error::RecvError::Closed) => panel_open = false,
                },
                history = history_rx.recv(), if history_open => match history {
                    Ok(frame) => {
                        // Machine scope sees every session, including machine-only ones. The
                        // frame is the contract object, forwarded unchanged.
                        if !matches!(tokio::time::timeout(std::time::Duration::from_secs(10), socket.send(Message::Text(frame.into()))).await, Ok(Ok(()))) { return; }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {},
                    Err(broadcast::error::RecvError::Closed) => history_open = false,
                },
                event = receiver.recv() => match event {
                    Ok(event) => {
                        if event["sequence"].as_str().and_then(|v| v.parse::<u64>().ok()).is_some_and(|v| v <= sequence) { continue; }
                        if !matches!(tokio::time::timeout(std::time::Duration::from_secs(10), socket.send(Message::Text(event.to_string().into()))).await, Ok(Ok(()))) { return; }
                        if matches!(event["type"].as_str(), Some("projectRegistered" | "projectRemoved" | "inventoryInvalidated")) { reason = "registration"; break; }
                    },
                    Err(broadcast::error::RecvError::Lagged(_)) => { reason = "lag"; break; },
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        }
        watcher = Some(watch);
    }
}

#[cfg(test)]
mod panel_frame_tests {
    use super::*;
    use crate::agent_control::{
        events::AgentObservation, AgentControlState, AgentStateSource, AgentTier,
    };
    use ferryx_hub::supervisor::panel_contract::{FleetSession, PendingPrompt, SessionRef};

    fn observation() -> AgentObservation {
        AgentObservation {
            sequence: 7,
            session_id: "s1".into(),
            state: AgentControlState::Blocked,
            state_source: AgentStateSource::Screen,
            tier: AgentTier::Screen,
            prompt_id: Some("p1".into()),
            kind: Some("approval".into()),
            screen_revision: Some("r1".into()),
            state_since_ms: None,
            screen_changed_at_ms: None,
            read: false,
        }
    }

    fn session() -> FleetSession {
        FleetSession {
            session_ref: SessionRef::new("mac", "s1"),
            host_id: "mac".into(),
            host_label: "This computer".into(),
            title: "ferryx".into(),
            cwd: "/work/ferryx".into(),
            agent: "claude".into(),
            state: "blocked".into(),
            state_source: "screen".into(),
            state_since_ms: None,
            screen_changed_at_ms: None,
            pending_prompt: Some(PendingPrompt {
                prompt_id: "p1".into(),
                kind: "approval".into(),
                question: "Run the migration?".into(),
                command: None,
                options: vec!["Allow".into(), "Deny".into()],
                screen_revision: Some("r1".into()),
            }),
            last_assistant_line: None,
            updated_at_ms: 1,
            stale: false,
            health: None,
            attention: None,
            reason: None,
            branch: None,
        }
    }

    #[test]
    fn a_non_owner_panel_frame_names_the_transition_and_carries_no_prompt_body() {
        let frame: Value = serde_json::from_str(&panel_session_frame("mac", &observation(), None)).unwrap();
        assert_eq!(frame["type"], PANEL_SESSION_EVENT);
        assert_eq!(frame["sessionRef"], "mac:s1");
        assert_eq!(frame["state"], "blocked");
        assert_eq!(frame["promptId"], "p1");
        assert!(frame.get("session").is_none());
        assert!(!frame.to_string().contains("Run the migration?"));
    }

    #[test]
    fn an_owner_panel_frame_carries_the_fleet_session_with_its_question() {
        let session = session();
        let frame: Value =
            serde_json::from_str(&panel_session_frame("mac", &observation(), Some(&session))).unwrap();
        assert_eq!(frame["type"], PANEL_SESSION_EVENT);
        assert_eq!(frame["session"]["pendingPrompt"]["question"], "Run the migration?");
        assert_eq!(frame["session"]["sessionRef"], "mac:s1");
    }

    #[test]
    fn an_owner_frame_without_its_session_is_a_resync_never_a_session_less_panel_session() {
        let failed = PanelWork { observation: observation(), session: None, failed: true };
        let frame: Value = serde_json::from_str(&panel_work_frame("mac", &failed, true)).unwrap();
        assert_eq!(frame["type"], PANEL_RESYNC_EVENT);
        assert_eq!(frame["reason"], "sessionUnavailable");
        assert_eq!(frame["sessionRef"], "mac:s1");
        assert!(frame.get("session").is_none() && !frame.to_string().contains("Run the migration?"));
    }

    #[test]
    fn a_device_that_lost_agent_control_during_the_read_gets_the_slim_frame_without_a_body() {
        let read = PanelWork { observation: observation(), session: Some(session()), failed: false };
        let owner: Value = serde_json::from_str(&panel_work_frame("mac", &read, true)).unwrap();
        assert_eq!(owner["session"]["pendingPrompt"]["question"], "Run the migration?");
        let demoted = panel_work_frame("mac", &read, false);
        let demoted: Value = serde_json::from_str(&demoted).unwrap();
        assert_eq!(demoted["type"], PANEL_SESSION_EVENT);
        assert!(demoted.get("session").is_none(), "{demoted}");
        assert!(!demoted.to_string().contains("Run the migration?"));
        // A non-owner never sees a failed read either: it is just a slim transition.
        let failed = PanelWork { observation: observation(), session: None, failed: true };
        let slim: Value = serde_json::from_str(&panel_work_frame("mac", &failed, false)).unwrap();
        assert_eq!(slim["type"], PANEL_SESSION_EVENT);
    }

    #[test]
    fn owner_channel_frames_are_text_for_an_owner_and_nothing_otherwise() {
        let frame = json!({"type": "panel.decision", "decision": {"prompt": {"question": "Run the migration?"}}});
        assert!(owner_panel_frame(true, &frame).is_some_and(|text| text.contains("Run the migration?")));
        assert_eq!(owner_panel_frame(false, &frame), None);
    }

    #[test]
    fn a_resync_frame_names_its_reason_and_the_frames_lost() {
        let frame: Value = serde_json::from_str(&panel_resync_frame("lagged", Some(12), None)).unwrap();
        assert_eq!(frame["type"], PANEL_RESYNC_EVENT);
        assert_eq!(frame["reason"], "lagged");
        assert_eq!(frame["skipped"], 12);
        assert!(frame["sessionRef"].is_null());
    }

    #[test]
    fn clearing_the_memory_makes_the_next_reading_a_transition_again() {
        let mut transitions = PanelTransitions::default();
        assert!(transitions.moved(&observation()));
        assert!(!transitions.moved(&observation()));
        transitions.clear();
        assert!(transitions.moved(&observation()), "a lag resync re-announces every session");
    }

    #[test]
    fn only_a_state_or_prompt_change_is_a_transition() {
        let mut transitions = PanelTransitions::default();
        let first = observation();
        assert!(transitions.moved(&first));
        let mut revision_only = first.clone();
        revision_only.screen_revision = Some("r2".into());
        assert!(!transitions.moved(&revision_only), "a screen change alone is not a transition");
        let mut working = first.clone();
        working.state = AgentControlState::Working;
        working.prompt_id = None;
        assert!(transitions.moved(&working));
    }

    #[test]
    fn an_ack_on_an_unchanged_screen_emits_panel_session() {
        let bus = crate::agent_control::events::AgentEventBus::new(8);
        let mut transitions = PanelTransitions::default();
        let open = bus.publish(crate::agent_control::events::ObservationDraft {
            session_id: "s1".into(),
            state: AgentControlState::Blocked,
            state_source: AgentStateSource::Screen,
            tier: AgentTier::Screen,
            prompt_id: Some("p1".into()),
            kind: Some("approval".into()),
            screen_revision: Some("r1".into()),
            workspace_id: None,
        });
        assert!(transitions.moved(&open));
        let acked = bus.acknowledge("s1").expect("a reading exists");
        assert_eq!((acked.state, acked.prompt_id.as_deref(), acked.screen_revision.as_deref()), (open.state, Some("p1"), Some("r1")));
        assert!(transitions.moved(&acked), "only read flipped, and that is a transition");
        let frame: Value = serde_json::from_str(&panel_session_frame("mac", &acked, None)).unwrap();
        assert_eq!(frame["type"], PANEL_SESSION_EVENT);
        assert_eq!(frame["sessionRef"], "mac:s1");
        assert_eq!(frame["read"], true);
        assert!(!transitions.moved(&acked), "the same read reading is not news twice");
    }
}
