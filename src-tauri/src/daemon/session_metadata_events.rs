//! Owner-lifetime metadata consumption; never depends on a desktop attachment.
use crate::daemon::{
    session_service::StoredSessionMeta, workspace_service::DaemonWorkspaceService,
};
use crate::{
    native_terminal::{NativeTerminal, TerminalEngine},
    remote::machine_protocol::RemoteTerminalTarget,
    terminal::TerminalService,
};
use parking_lot::RwLock;
use std::{collections::HashMap, sync::Arc};

pub(super) struct MetadataOwner {
    pub workspaces: Arc<DaemonWorkspaceService>,
    pub terminals: Arc<TerminalService>,
    pub metadata: Arc<RwLock<HashMap<String, StoredSessionMeta>>>,
}

impl MetadataOwner {
    /// The target comes from the committed spawn, never from the gateway epoch.
    pub(super) fn subscribe(
        self,
        target: RemoteTerminalTarget,
    ) -> Result<tokio::task::JoinHandle<()>, String> {
        let (history, mut receiver) = self
            .terminals
            .output_hub()
            .subscribe(&target.session_id)
            .ok_or("SESSION_EXPIRED")?;
        let mut engine = NativeTerminal::new(80, 24).map_err(|_| "METADATA_PARSER_UNAVAILABLE")?;
        engine
            .set_scrollback_limit_lines(Some(0))
            .map_err(|_| "METADATA_PARSER_UNAVAILABLE")?;
        engine
            .feed(&history)
            .map_err(|_| "METADATA_PARSER_UNAVAILABLE")?;
        Ok(tokio::spawn(async move {
            loop {
                let title = if engine.take_title_changed() {
                    match engine.title() {
                        Ok(title) => Some(title),
                        Err(error) => {
                            tracing::warn!(%error, "Machine metadata title unavailable");
                            break;
                        }
                    }
                } else {
                    None
                };
                let owner = Self {
                    workspaces: self.workspaces.clone(),
                    terminals: self.terminals.clone(),
                    metadata: self.metadata.clone(),
                };
                let current_target = target.clone();
                let result = crate::ipc::run_blocking(move || {
                    owner
                        .refresh(&current_target, title)
                        .map_err(crate::ipc::IpcError::internal)
                })
                .await;
                if let Err(error) = result {
                    tracing::debug!(%error, "Machine metadata owner no longer available");
                    break;
                }
                match receiver.recv().await {
                    Ok(bytes) => {
                        if let Err(error) = engine.feed(&bytes) {
                            tracing::warn!(%error, "Machine metadata parser failed");
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        self.workspaces.machine_events.publish(
                            "inventoryInvalidated",
                            None,
                            None,
                            serde_json::json!({"completeness":"partial","reason":"metadataLag"}),
                        );
                        let Some((history, fresh)) =
                            self.terminals.output_hub().subscribe(&target.session_id)
                        else {
                            break;
                        };
                        receiver = fresh;
                        engine.reset();
                        if let Err(error) = engine.feed(&history) {
                            tracing::warn!(%error, "Machine metadata replay failed");
                            break;
                        }
                    }
                }
            }
        }))
    }

    fn refresh(&self, target: &RemoteTerminalTarget, title: Option<String>) -> Result<(), String> {
        // Holding metadata through commit fences removal/replacement. The local PID
        // is acquired only after exact durable/in-memory ownership agrees.
        let _workspace_gate = self.workspaces.mutation_gate.lock();
        let mut metadata = self.metadata.write();
        let meta = metadata
            .get_mut(&target.session_id)
            .ok_or("SESSION_NOT_FOUND")?;
        let session = meta.machine_session.as_mut().ok_or("SESSION_NOT_FOUND")?;
        if session.target != *target {
            return Err("STALE_EPOCH".into());
        }
        if !self
            .workspaces
            .catalog
            .lock()
            .as_ref()
            .map_err(Clone::clone)?
            .workspaces
            .contains_key(&session.workspace_id)
        {
            return Err("SESSION_OWNERSHIP_CHANGED".into());
        }
        let pty = self
            .terminals
            .get_session(&target.session_id)
            .ok_or("SESSION_EXPIRED")?;
        let pid = pty.pid().ok_or("SESSION_EXPIRED")?;
        let cwd = crate::ipc::terminal::process_cwd(pid).ok_or("CWD_UNAVAILABLE")?;
        let mut updated = session.clone();
        updated.cwd = cwd.to_str().ok_or("INVALID_PATH")?.to_owned();
        if let Some(title) = title {
            updated.title = Some(title);
        }
        if updated == *session {
            return Ok(());
        }
        if let Some((committed, revision)) =
            self.workspaces.journal.update_session_metadata(&updated)?
        {
            meta.cwd = cwd;
            *session = committed.clone();
            self.workspaces.machine_events.publish_revision(
                revision.0,
                "sessionMetadataChanged",
                Some(&committed.workspace_id),
                Some(&target.session_id),
                serde_json::json!(committed),
            );
        }
        Ok(())
    }
}
