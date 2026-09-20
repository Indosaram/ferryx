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
        self.publish_event(None, kind, workspace, session, payload);
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
    ) {
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
    }
}

pub(crate) async fn serve(
    mut socket: axum::extract::ws::WebSocket,
    state: Arc<super::state::RemoteGatewayState>,
    mut receiver: broadcast::Receiver<Value>,
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
    let mut metadata_forwarders = tokio::task::JoinSet::new();
    let mut forwarded_targets =
        std::collections::HashMap::<String, tokio::sync::watch::Receiver<bool>>::new();
    let (owner_changed, mut owner_changes) = tokio::sync::mpsc::channel(64);
    loop {
        // The cursor is read BEFORE inventory construction. Any overlapping commit
        // is therefore replayed, never discarded as already in this boundary.
        let sequence = events.sequence();
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
                let payload = json!({"projects":catalog,"sessions":sessions});
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
