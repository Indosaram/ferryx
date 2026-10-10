//! Routed machine inventory keeps the predecessor's target authoritative.
use crate::daemon::{
    protocol::{DaemonRequest, DaemonResponse},
    session_service::DaemonSessionService,
};
use crate::{
    remote::machine_protocol::{Completeness, SessionDetail, Sessions},
    scoped_contracts::Epoch,
};
use std::sync::Arc;

impl DaemonSessionService {
    pub(crate) async fn machine_only_async(self: &Arc<Self>, id: &str) -> Result<bool, String> {
        let service = self.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || service.machine_only(&id))
            .await
            .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".into())
    }

    pub(crate) async fn machine_detail_routed(
        self: &Arc<Self>,
        id: &str,
        epoch: Epoch,
    ) -> Result<SessionDetail, String> {
        if let Some(peer) = self.session_router.find_legacy_peer_for_session(id) {
            let response = peer
                .send_request(&DaemonRequest::MachineSessionDetail {
                    session_id: id.into(),
                })
                .await
                .map_err(|_| "HOST_UNAVAILABLE".to_owned())?;
            return match response {
                DaemonResponse::MachineSessionDetailOk { detail } => {
                    let target = match &detail {
                        SessionDetail::Running { session }
                        | SessionDetail::Exited { session, .. } => &session.target,
                        SessionDetail::Expired { target } => target,
                    };
                    if target.session_id != id {
                        return Err("SESSION_OWNERSHIP_CHANGED".into());
                    }
                    Ok(detail)
                }
                DaemonResponse::Error { message, .. } if message == "SESSION_NOT_FOUND" => {
                    Err(message)
                }
                DaemonResponse::Error { message, .. } if message == "HOST_UNAVAILABLE" => {
                    Err(message)
                }
                _ => Err("MACHINE_OWNER_UNSUPPORTED".into()),
            };
        }
        let service = self.clone();
        let id = id.to_owned();
        crate::ipc::run_blocking(move || Ok(service.machine_detail(&id, epoch)))
            .await
            .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_owned())?
    }

    /// The workspace a live session belongs to, derived from its own path when nothing
    /// registered it. A rolling handover transfers only the PTY fd and a snapshot with a
    /// worktree path, so adopted sessions carry no workspace anywhere; the catalog is the
    /// authority for which repo root a path belongs to. Managed worktrees live under
    /// `<root>/.orca-worktrees/<workspace>/<slug>`, which also recovers the worktree identity.
    fn derive_workspace_for_path(
        &self,
        cwd: &std::path::Path,
    ) -> Option<(String, Option<crate::worktree::model::WorktreeIdentity>)> {
        let catalog = self.workspace_service.catalog().ok()?;
        let real_cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        let mut best: Option<(usize, String, Option<crate::worktree::model::WorktreeIdentity>)> = None;
        for (workspace_id, row) in catalog.workspaces.iter() {
            let root = std::fs::canonicalize(&row.repo_root)
                .unwrap_or_else(|_| row.repo_root.clone());
            if !real_cwd.starts_with(&root) {
                continue;
            }
            let mut worktree = None;
            if let Ok(relative) = real_cwd.strip_prefix(&root) {
                let parts: Vec<_> = relative.components().collect();
                if parts.len() >= 3 {
                    let (first, second, third) = (parts[0], parts[1], parts[2]);
                    if first.as_os_str() == ".orca-worktrees"
                        && second.as_os_str() == workspace_id.as_str()
                    {
                        worktree = Some(crate::worktree::model::WorktreeIdentity {
                            ws_id: workspace_id.clone(),
                            slug: third.as_os_str().to_string_lossy().into_owned(),
                        });
                    }
                }
            }
            let depth = root.as_os_str().len();
            if best.as_ref().is_none_or(|(len, _, _)| depth > *len) {
                best = Some((depth, workspace_id.clone(), worktree));
            }
        }
        best.map(|(_, workspace_id, worktree)| (workspace_id, worktree))
    }

    /// The machine API answers about a session from `session_metadata`, which only the
    /// machine spawn paths write. Sessions this daemon owns but did not create that way
    /// (GUI-created local terminals, PTYs adopted by a rolling handover) have no entry, so
    /// every machine answer about them is SESSION_NOT_FOUND and every attach is
    /// SESSION_EXPIRED. Rebuild the missing entries from the live terminal service, deriving
    /// the workspace from the session's own path when nothing registered it, so machine
    /// clients can see and attach to what this daemon actually runs. A session whose path
    /// cannot be resolved is left alone rather than guessed at.
    pub(crate) fn reconcile_machine_session_metadata(&self) -> usize {
        let live = self.terminal_service.list_sessions();
        let mut added = 0;
        for session_id in live {
            if self.session_metadata.read().contains_key(&session_id) {
                continue;
            }
            let registered = self.session_router.workspace_for(&session_id);
            let (workspace_id, worktree, cwd) = match registered {
                Some((workspace_id, _ssh_store)) => {
                    let remote = crate::ssh::projects::is_remote(&workspace_id)
                        || workspace_id.starts_with("ssh:")
                        || workspace_id.contains("::");
                    if remote {
                        // A remote/ssh session's cwd lives on the remote host, so there is no
                        // local repo root to resolve against; take it from the remote descriptor
                        // so the inventory can describe the session the desktop is showing.
                        let Some(details) = self.terminal_service.remote().details(&session_id)
                        else {
                            continue;
                        };
                        let path = std::path::PathBuf::from(details.descriptor.config.project_path);
                        (workspace_id, None, path)
                    } else {
                        let Ok(catalog) = self.workspace_service.catalog() else {
                            continue;
                        };
                        let Some(row) = catalog.workspaces.get(&workspace_id) else {
                            continue;
                        };
                        let cwd = row.repo_root.clone();
                        drop(catalog);
                        (workspace_id, None, cwd)
                    }
                }
                None => {
                    let Some(pty) = self.terminal_service.get_session(&session_id) else {
                        continue;
                    };
                    let path = pty.worktree_path().or_else(|| {
                        pty.pid().and_then(crate::ipc::terminal::process_cwd)
                    });
                    let Some(path) = path else {
                        continue;
                    };
                    let Some((workspace_id, worktree)) = self.derive_workspace_for_path(&path)
                    else {
                        continue;
                    };
                    // Register the mapping so describe/projection agree with this entry.
                    self.session_router
                        .register_workspace(&session_id, &workspace_id, None);
                    (workspace_id, worktree, path)
                }
            };
            // Remote/ssh sessions are projected from the remote runtime by
            // project_desktop_gui_session, so they belong in the inventory too: desktop parity
            // means every session the desktop shows is describable and attachable here.
            let meta = crate::daemon::session_service::StoredSessionMeta {
                client_request_id: format!("adopted:{session_id}"),
                machine_session: None,
                workspace_id: workspace_id.clone(),
                worktree: worktree.clone(),
                cwd: cwd.clone(),
                provider_claim: None,
                spawn_fingerprint: crate::daemon::session_service::SpawnRequestFingerprint {
                    requested_session_id: Some(session_id.clone()),
                    workspace_id,
                    worktree,
                    cwd: Some(cwd.to_string_lossy().into_owned()),
                    cols: 0,
                    rows: 0,
                    shell: None,
                    provider_claim: None,
                    startup: None,
                },
            };
            self.session_metadata.write().insert(session_id, meta);
            added += 1;
        }
        added
    }

    pub(crate) async fn machine_sessions_routed(
        self: &Arc<Self>,
        epoch: Epoch,
    ) -> Result<Sessions, String> {
        let service = self.clone();
        let mut inventory = crate::ipc::run_blocking(move || {
            let _adopted = service.reconcile_machine_session_metadata();
            let records = service.workspace_service.journal.sessions();
            let revision = service.workspace_service.journal.session_revision();
            Ok((|| {
                let mut sessions = Vec::new();
                for record in records? {
                    let mut session = record.session;
                    if service
                        .session_router
                        .find_legacy_peer_for_session(&session.target.session_id)
                        .is_none()
                    {
                        match service.machine_detail(&session.target.session_id, epoch)? {
                            SessionDetail::Running { session: current }
                            | SessionDetail::Exited {
                                session: current, ..
                            } => session = current,
                            SessionDetail::Expired { .. } => {
                                // A record whose PTY this daemon no longer runs must not be
                                // published: clients would attach to a dead id and get
                                // SESSION_EXPIRED. Keep it only while the PTY is live.
                                if service
                                    .terminal_service
                                    .get_session(&session.target.session_id)
                                    .is_none()
                                {
                                    continue;
                                }
                                session.running = false;
                            }
                        }
                    }
                    sessions.push(session);
                }

                let mut completeness = Completeness::Complete;
                let active_pty_ids = service.terminal_service.list_sessions();
                match service.workspace_service.catalog() {
                    Ok(catalog) => {
                        for session_id in active_pty_ids {
                            if sessions.iter().any(|s| s.target.session_id == session_id) {
                                continue;
                            }
                            match service.project_desktop_gui_session(&session_id, epoch, &catalog) {
                                Ok(desktop_session) => sessions.push(desktop_session),
                                Err(err) if err == "MACHINE_SERVICE_UNAVAILABLE" => {
                                    completeness = Completeness::Partial;
                                }
                                Err(_) => {}
                            }
                        }
                    }
                    Err(_) => {
                        completeness = Completeness::Partial;
                    }
                }

                Ok::<_, String>(Sessions {
                    revision: revision?,
                    completeness,
                    sessions,
                    unavailable_workspace_ids: Vec::new(),
                })
            })())
        })
        .await
        .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_owned())??;
        for session in &mut inventory.sessions {
            if self
                .session_router
                .find_legacy_peer_for_session(&session.target.session_id)
                .is_none()
            {
                continue;
            }
            match self
                .machine_detail_routed(&session.target.session_id, epoch)
                .await
            {
                Ok(
                    SessionDetail::Running { session: current }
                    | SessionDetail::Exited {
                        session: current, ..
                    },
                ) => *session = current,
                Ok(SessionDetail::Expired { .. }) => session.running = false,
                Err(_) => {
                    inventory.completeness = Completeness::Partial;
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
        }
        Ok(inventory)
    }
}
