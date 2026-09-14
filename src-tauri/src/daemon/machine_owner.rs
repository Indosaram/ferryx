//! Routed machine inventory keeps the predecessor's target authoritative.
use crate::daemon::{protocol::{DaemonRequest, DaemonResponse}, session_service::DaemonSessionService};
use crate::{remote::machine_protocol::{Completeness, SessionDetail, Sessions}, scoped_contracts::Epoch};
use std::sync::Arc;

impl DaemonSessionService {
    pub(crate) async fn machine_only_async(self: &Arc<Self>, id: &str) -> Result<bool, String> {
        let service = self.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || service.machine_only(&id)).await.map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".into())
    }

    pub(crate) async fn machine_detail_routed(self: &Arc<Self>, id: &str, epoch: Epoch) -> Result<SessionDetail, String> {
        if let Some(peer) = self.session_router.find_legacy_peer_for_session(id) {
            let response = peer.send_request(&DaemonRequest::MachineSessionDetail { session_id: id.into() }).await
                .map_err(|_| "HOST_UNAVAILABLE".to_owned())?;
            return match response {
                DaemonResponse::MachineSessionDetailOk { detail } => {
                    let target = match &detail {
                        SessionDetail::Running { session } | SessionDetail::Exited { session, .. } => &session.target,
                        SessionDetail::Expired { target } => target,
                    };
                    if target.session_id != id { return Err("SESSION_OWNERSHIP_CHANGED".into()); }
                    Ok(detail)
                },
                DaemonResponse::Error { message } if message == "SESSION_NOT_FOUND" => Err(message),
                DaemonResponse::Error { message } if message == "HOST_UNAVAILABLE" => Err(message),
                _ => Err("MACHINE_OWNER_UNSUPPORTED".into()),
            };
        }
        let service = self.clone();
        let id = id.to_owned();
        crate::ipc::run_blocking(move || Ok(service.machine_detail(&id, epoch)))
            .await.map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_owned())?
    }

    pub(crate) async fn machine_sessions_routed(self: &Arc<Self>, epoch: Epoch) -> Result<Sessions, String> {
        let service = self.clone();
        let mut inventory = crate::ipc::run_blocking(move || {
            let records = service.workspace_service.journal.sessions();
            let revision = service.workspace_service.journal.session_revision();
            Ok((|| {
                let mut sessions = Vec::new();
                for record in records? {
                    let mut session = record.session;
                    if service.session_router.find_legacy_peer_for_session(&session.target.session_id).is_none() {
                        match service.machine_detail(&session.target.session_id, epoch)? {
                            SessionDetail::Running { session: current } | SessionDetail::Exited { session: current, .. } => session = current,
                            SessionDetail::Expired { .. } => session.running = false,
                        }
                    }
                    sessions.push(session);
                }
                Ok::<_, String>(Sessions { revision: revision?, completeness: Completeness::Complete, sessions, unavailable_workspace_ids: Vec::new() })
            })())
        }).await.map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_owned())??;
        for session in &mut inventory.sessions {
            if self.session_router.find_legacy_peer_for_session(&session.target.session_id).is_none() { continue; }
            match self.machine_detail_routed(&session.target.session_id, epoch).await {
                Ok(SessionDetail::Running { session: current } | SessionDetail::Exited { session: current, .. }) => *session = current,
                Ok(SessionDetail::Expired { .. }) => session.running = false,
                Err(_) => {
                    inventory.completeness = Completeness::Partial;
                    if !inventory.unavailable_workspace_ids.contains(&session.workspace_id) {
                        inventory.unavailable_workspace_ids.push(session.workspace_id.clone());
                    }
                }
            }
        }
        Ok(inventory)
    }
}
