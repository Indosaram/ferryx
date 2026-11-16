use super::{DaemonSessionService, DurableRemoteSession};

impl DaemonSessionService {
    pub(crate) async fn recover_retained_session(
        &self,
        id: &str,
        paired_hosts: &crate::paired_host::service::PairedHostService,
    ) -> Result<(), String> {
        if self.session_router.find_legacy_peer_for_session(id).is_none() {
            return Ok(());
        }
        let _recovery = self.session_router.recovery_lock.lock().await;
        let Some(peer) = self.session_router.find_legacy_peer_for_session(id) else {
            return Ok(());
        };
        let handover = self.handover_manager.upgrade()
            .ok_or_else(|| "HOST_UNAVAILABLE".to_owned())?;
        let _retirement = handover.retain_request(self.terminal_service.clone())?;
        if !peer.confirmed_dead().await {
            return Ok(());
        }
        if crate::terminal::paired_runtime::Runtime::owns(id) {
            let descriptor = self.terminal_service.paired().descriptor(id)
                .ok_or_else(|| "Retained paired session descriptor is unavailable".to_owned())?;
            self.terminal_service.paired().get_or_rearm(
                descriptor,
                self.terminal_service.output_hub().clone(),
                paired_hosts.clone(),
            ).map_err(|error| error.code)?;
        } else {
            let path = self.remote_sessions_path.clone();
            let records: Vec<DurableRemoteSession> = crate::ipc::run_blocking(move || {
                let value = crate::session::load_session_from_path(&path)?
                    .and_then(|mut session| session.extra.remove("remoteSessions"))
                    .unwrap_or_else(|| serde_json::json!([]));
                serde_json::from_value(value)
                    .map_err(|error| crate::ipc::IpcError::internal(error.to_string()))
            }).await.map_err(|error| error.to_string())?;
            let mut record = records.into_iter()
                .find(|record| record.descriptor.backend_session_id == id)
                .ok_or_else(|| "Retained SSH session descriptor is unavailable".to_owned())?;
            if record.terminal {
                return Err("Retained SSH session has already exited".to_owned());
            }
            record.descriptor.remote_cursor = crate::ssh::bridge::RemoteCursor(0);
            self.ensure_agent_sink();
            self.session_router.register_workspace(
                id, &record.descriptor.config.project_id, Some(self.ssh_store_path.clone()),
            );
            if let Some(metadata) = record.metadata {
                self.session_metadata.write().insert(id.to_owned(), metadata);
            }
            if let Err(error) = self.terminal_service.remote().restore(record.descriptor) {
                self.session_router.remove_workspace(id);
                self.session_metadata.write().remove(id);
                return Err(error.to_string());
            }
            self.watch_remote_session(id)?;
            self.watch_agent_session(id);
        }
        peer.remove_session(id);
        if let Err(error) = peer.persist_sessions().await {
            tracing::warn!(session_id = id, %error, "Recovered retained session but could not prune its legacy route");
        }
        Ok(())
    }
}
