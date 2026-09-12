use super::workspace_service::DaemonWorkspaceService;
use crate::daemon::agent_state::AgentStateHub;
use crate::daemon::protocol::{
    AgentProviderSessionKey, DaemonRemoteEvent, DaemonResponse, DaemonSessionDetails,
    TerminalStartup,
};
use crate::session::{load_session_from_path, save_session_to_path};
use crate::terminal::{PtySessionState, TerminalService};
use crate::worktree::WorktreeIdentity;
use parking_lot::{Mutex, RwLock};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

pub(crate) fn normalize_process_cwd(path: &Path) -> PathBuf {
    let Some(path_str) = path.to_str() else {
        return path.to_path_buf();
    };

    if let Some(rest) = path_str.strip_prefix(r"\\?\") {
        if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(r"UNC\") {
            return PathBuf::from(format!(r"\\{}", &rest[4..]));
        }

        let bytes = rest.as_bytes();
        if bytes.len() >= 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes.len() == 2 || bytes[2] == b'\\' || bytes[2] == b'/')
        {
            return PathBuf::from(rest);
        }
    }

    path.to_path_buf()
}

const SPAWN_REQUEST_TTL: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(super) struct SpawnCacheEntry {
    session_id: String,
    created_at: Instant,
    fingerprint: SpawnRequestFingerprint,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct StoredSessionMeta {
    pub(super) client_request_id: String,
    pub(super) workspace_id: String,
    pub(super) worktree: Option<WorktreeIdentity>,
    pub(super) cwd: PathBuf,
    pub(super) provider_claim: Option<ProviderSessionClaimKey>,
    pub(super) spawn_fingerprint: SpawnRequestFingerprint,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct SpawnRequestFingerprint {
    pub(super) workspace_id: String,
    pub(super) worktree: Option<WorktreeIdentity>,
    pub(super) cwd: Option<String>,
    pub(super) cols: u16,
    pub(super) rows: u16,
    pub(super) shell: Option<String>,
    pub(super) provider_claim: Option<ProviderSessionClaimKey>,
    pub(super) startup: Option<TerminalStartup>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) struct ProviderSessionClaimKey {
    agent_type: String,
    provider_key: AgentProviderSessionKey,
    provider_id: String,
    transcript_path: Option<String>,
}

impl ProviderSessionClaimKey {
    pub(super) fn from_startup(startup: Option<&TerminalStartup>) -> Option<Self> {
        let TerminalStartup::AgentResume {
            agent_type,
            provider_session,
        } = startup?
        else {
            return None;
        };
        let agent_type = agent_type.trim().to_ascii_lowercase();
        let transcript_path = if matches!(agent_type.as_str(), "pi" | "prime-agent") {
            provider_session
                .transcript_path
                .as_deref()
                .map(str::trim)
                .map(str::to_string)
        } else {
            None
        };
        Some(Self {
            agent_type,
            provider_key: provider_session.key,
            provider_id: provider_session.id.trim().to_string(),
            transcript_path,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum SpawnError {
    #[error(
        "AgentSessionConflict: {agent_type} {provider_key:?} '{provider_id}' is already owned by session '{existing_session_id}'"
    )]
    AgentSessionConflict {
        agent_type: String,
        provider_key: AgentProviderSessionKey,
        provider_id: String,
        existing_session_id: String,
    },
    #[error("{0}")]
    InvalidAgentResume(String),
    #[error("{0}")]
    Other(String),
}

impl SpawnError {
    #[cfg(test)]
    pub(super) fn contains(&self, needle: &str) -> bool {
        self.to_string().contains(needle)
    }
}

impl From<String> for SpawnError {
    fn from(value: String) -> Self {
        Self::Other(value)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DurableRemoteSession {
    descriptor: crate::terminal::remote::RemoteSessionDescriptor,
    metadata: Option<StoredSessionMeta>,
}

/// Headless session authority; owns metadata, claims and spawn idempotency.
/// Holds no server/gateway or AppHandle. Handover is weak to avoid a callback cycle.
pub struct DaemonSessionService {
    pub(crate) workspace_service: Arc<DaemonWorkspaceService>,
    pub(super) terminal_service: Arc<TerminalService>,
    pub(super) session_router: Arc<super::proxy::SessionRouter>,
    pub(super) handover_manager: Weak<super::handover::HandoverManager>,
    pub(super) remote_event_tx: broadcast::Sender<DaemonRemoteEvent>,
    pub(super) spawn_idempotency_cache: Arc<Mutex<HashMap<String, SpawnCacheEntry>>>,
    pub(super) spawn_lock: Arc<tokio::sync::Mutex<()>>,
    pub(super) remote_persistence_lock: Arc<tokio::sync::Mutex<()>>,
    pub(super) remote_sessions_path: PathBuf,
    pub(super) ssh_store_path: PathBuf,
    pub(super) session_metadata: Arc<RwLock<HashMap<String, StoredSessionMeta>>>,
    pub(super) provider_session_claims: Arc<Mutex<HashMap<ProviderSessionClaimKey, String>>>,
    pub(super) agent_states: Arc<AgentStateHub>,
}

impl DaemonSessionService {
    pub(crate) fn router(&self) -> &super::proxy::SessionRouter {
        &self.session_router
    }

    pub(crate) fn project_session_metadata(
        &self,
        details: &mut crate::remote::backend::RemoteSessionDetails,
    ) {
        if let Some(meta) = self.session_metadata.read().get(&details.session_id) {
            details.workspace_id = Some(meta.workspace_id.clone());
            details.worktree_label = meta.worktree.as_ref().map(|worktree| worktree.slug.clone());
            details.worktree_path = Some(meta.cwd.clone());
        }
    }

    pub(super) async fn write_session_input(&self, id: &str, data: Vec<u8>) -> Result<(), String> {
        self.validate_session_ssh_target(id)
            .await
            .map_err(|e| e.to_string())?;
        let generation = self
            .terminal_service
            .remote()
            .details(id)
            .map(|d| d.generation)
            .unwrap_or(0);
        self.terminal_service
            .write_input_operation(id, generation, data)
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())
    }

    pub(super) async fn resize_session(
        &self,
        id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        self.validate_session_ssh_target(id)
            .await
            .map_err(|e| e.to_string())?;
        let generation = self
            .terminal_service
            .remote()
            .details(id)
            .map(|d| d.generation)
            .unwrap_or(0);
        self.terminal_service
            .resize_operation(id, generation, cols, rows)
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())
    }

    pub(super) async fn persist_remote_sessions_at(&self, path: PathBuf) -> Result<(), String> {
        let _guard = self.remote_persistence_lock.lock().await;
        let records: Vec<_> = self
            .terminal_service
            .remote()
            .list()
            .iter()
            .filter_map(|id| {
                self.terminal_service
                    .remote()
                    .details(id)
                    .map(|d| DurableRemoteSession {
                        descriptor: d.descriptor,
                        metadata: self.session_metadata.read().get(id).cloned(),
                    })
            })
            .collect();
        crate::ipc::run_blocking(move || {
            let mut session = crate::session::PersistedWorkspaceSession::default();
            session.version = 3;
            session.extra.insert(
                "remoteSessions".into(),
                serde_json::to_value(records)
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?,
            );
            save_session_to_path(&path, &session)
        })
        .await
        .map_err(|e| e.to_string())
    }

    pub(super) async fn restore_remote_sessions_at(&self, path: PathBuf) -> Result<(), String> {
        let records: Vec<DurableRemoteSession> = crate::ipc::run_blocking(move || {
            let value = load_session_from_path(&path)?
                .and_then(|mut s| s.extra.remove("remoteSessions"))
                .unwrap_or_else(|| serde_json::json!([]));
            serde_json::from_value(value).map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
        })
        .await
        .map_err(|e| e.to_string())?;
        for mut record in records {
            // A new daemon has no hub backlog. Replay retained remote output from zero;
            // Attach emits a reset boundary instead of treating old local sequences as cursors.
            record.descriptor.remote_cursor = crate::ssh::bridge::RemoteCursor(0);
            let id = record.descriptor.backend_session_id.clone();
            if self
                .session_router
                .find_legacy_peer_for_session(&id)
                .is_some()
            {
                continue;
            }
            self.session_router.register_workspace(
                &id,
                &record.descriptor.config.project_id,
                Some(self.ssh_store_path.clone()),
            );
            if let Some(meta) = record.metadata {
                self.session_metadata.write().insert(id.clone(), meta);
            }
            self.terminal_service
                .remote()
                .restore(record.descriptor)
                .map_err(|e| e.to_string())?;
            self.watch_remote_session(&id)?;
        }
        Ok(())
    }

    pub(super) fn watch_remote_session(&self, id: &str) -> Result<(), String> {
        let mut rx = self
            .terminal_service
            .remote()
            .subscribe(id)
            .map_err(|e| e.to_string())?;
        let tx = self.remote_event_tx.clone();
        let runtime = Arc::downgrade(self.terminal_service.remote());
        let metadata = self.session_metadata.clone();
        let path = self.remote_sessions_path.clone();
        let lock = self.remote_persistence_lock.clone();
        tokio::spawn(async move {
            loop {
                let details = rx.borrow_and_update().clone();
                {
                    let _guard = lock.lock().await;
                    let Some(runtime) = runtime.upgrade() else {
                        break;
                    };
                    let records: Vec<_> = runtime
                        .list()
                        .iter()
                        .filter_map(|id| {
                            runtime.details(id).map(|d| DurableRemoteSession {
                                descriptor: d.descriptor,
                                metadata: metadata.read().get(id).cloned(),
                            })
                        })
                        .collect();
                    drop(runtime);
                    let path = path.clone();
                    if let Err(error) = crate::ipc::run_blocking(move || {
                        let mut session = crate::session::PersistedWorkspaceSession::default();
                        session.version = 3;
                        session.extra.insert(
                            "remoteSessions".into(),
                            serde_json::to_value(records)
                                .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?,
                        );
                        save_session_to_path(&path, &session)
                    })
                    .await
                    {
                        tracing::error!(%error, "Remote session checkpoint persistence failed");
                    }
                }
                let _ = tx.send(DaemonRemoteEvent { event: "terminal_remote_status".into(), payload: serde_json::json!({"sessionId":details.descriptor.backend_session_id,"state":details.state,"generation":details.generation,"failure":details.failure,"replayGap":details.replay_gap}) });
                if rx.changed().await.is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    pub(super) async fn spawn_remote(
        &self,
        project: crate::ssh::projects::RemoteProject,
        host: crate::ssh::SshHost,
        request: &str,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        fingerprint: SpawnRequestFingerprint,
        #[cfg(test)] helper_home: Option<String>,
    ) -> Result<String, SpawnError> {
        let environment = crate::ssh::runtime::detect(&host)
            .await
            .map_err(|e| e.to_string())?;
        if project
            .platform
            .unwrap_or(crate::ssh::runtime::RemotePlatform::Posix)
            != environment.platform
        {
            return Err(SpawnError::Other(
                "Remote platform changed; register the project again".into(),
            ));
        }
        let root = crate::ssh::worktree::resolve_remote_spawn_root(
            environment.platform,
            &project.repo_root,
            worktree.as_ref(),
            cwd.as_deref(),
        )
        .map_err(|e| e.to_string())?;
        #[cfg(test)]
        let environment = {
            let mut environment = environment;
            if let Some(home) = &helper_home {
                environment.home = home.clone();
            }
            environment
        };
        let helper = crate::ssh::helper_setup::default_location(&host, &environment)
            .map_err(|e| e.to_string())?;
        let relative = root
            .strip_prefix(&project.repo_root)
            .unwrap_or("")
            .trim_start_matches(&['/', '\\'][..])
            .replace('\\', "/");
        let config = crate::terminal::remote::RemoteSessionConfig {
            host,
            environment,
            helper,
            project_id: project.workspace_id.clone(),
            project_path: project.repo_root,
            worktree: if relative.is_empty() {
                None
            } else {
                Some(relative)
            },
            agent_identity: None,
        };
        // Persist the immutable request before any potentially ambiguous remote spawn.
        use sha2::{Digest, Sha256};
        let request_key = format!("{:x}", Sha256::digest(request.as_bytes()));
        let request_path = self
            .remote_sessions_path
            .with_file_name(format!("remote-request-{request_key}.json"));
        let request_value = serde_json::json!({"clientRequestId":request,"config":config,"fingerprint":fingerprint});
        crate::ipc::run_blocking(move || {
            if let Some(previous) = load_session_from_path(&request_path)? {
                if previous.extra.get("request") != Some(&request_value) {
                    return Err(crate::ipc::IpcError::internal("clientRequestId was reused with a different remote spawn request"));
                }
                // Uncertain requests require recovery of their original target, not a new shell.
                return Err(crate::ipc::IpcError::internal("Remote spawn request is pending recovery; refusing to create a replacement target"));
            }
            let mut session = crate::session::PersistedWorkspaceSession::default(); session.version = 3;
            session.extra.insert("request".into(), request_value);
            save_session_to_path(&request_path, &session)
        }).await.map_err(|e| e.to_string())?;
        let descriptor = self
            .terminal_service
            .remote()
            .create(
                config,
                crate::ssh::bridge::SpawnParams {
                    cols: Some(cols),
                    rows: Some(rows),
                    ..Default::default()
                },
                request.into(),
            )
            .await
            .map_err(|e| e.to_string())?;
        let id = descriptor.backend_session_id.clone();
        self.session_metadata.write().insert(
            id.clone(),
            StoredSessionMeta {
                client_request_id: request.into(),
                workspace_id: project.workspace_id.clone(),
                worktree,
                cwd: PathBuf::from(root),
                provider_claim: None,
                spawn_fingerprint: fingerprint,
            },
        );
        self.session_router.register_workspace(
            &id,
            &project.workspace_id,
            Some(self.ssh_store_path.clone()),
        );
        self.persist_remote_sessions_at(self.remote_sessions_path.clone())
            .await?;
        self.watch_remote_session(&id)?;
        Ok(id)
    }

    /// Revokes a workspace binding and terminates every live session the
    /// daemon owns for it, so remote clients cannot keep using already-spawned
    /// PTYs of a workspace the user removed. Idempotent: unregistering an
    /// unknown workspace (e.g. after a daemon restart) succeeds as a no-op.
    pub async fn handle_unregister_workspace(&self, workspace_id: &str) -> Result<(), String> {
        let _spawn_guard = Arc::clone(&self.spawn_lock).lock_owned().await;
        // SSH owns a separate inventory, but still needs the session cleanup
        // below. Never send remote identities through the local catalog gate.
        if !crate::ssh::projects::is_remote(workspace_id) {
            let workspaces = Arc::clone(&self.workspace_service);
            let workspace = workspace_id.to_string();
            crate::ipc::run_blocking(move || {
                workspaces.unregister(&workspace).map_err(crate::ipc::IpcError::internal)
            }).await.map_err(|error| error.to_string())?;
        }

        let owned_sessions: Vec<String> = self
            .session_metadata
            .read()
            .iter()
            .filter(|(_, meta)| meta.workspace_id == workspace_id)
            .map(|(session_id, _)| session_id.clone())
            .collect();
        for session_id in owned_sessions {
            if self.session_router.is_local_session(&session_id) {
                self.handle_close(&session_id)
                    .await
                    .map_err(|e| e.to_string())?;
            } else if let Some(peer) = self
                .session_router
                .find_legacy_peer_for_session(&session_id)
            {
                peer.close(&session_id).await.map_err(|message| {
                    format!("failed to close peer session '{session_id}': {message}")
                })?;
            }
            self.release_session_ownership(&session_id);
        }
        Ok(())
    }

    pub(super) async fn handle_spawn(
        &self,
        client_request_id: &str,
        workspace_id: &str,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        shell: Option<String>,
        startup: Option<TerminalStartup>,
        #[cfg(test)] helper_home: Option<String>,
    ) -> Result<String, SpawnError> {
        if self
            .handover_manager
            .upgrade()
            .is_none_or(|manager| manager.is_draining())
        {
            return Err(SpawnError::Other(
                "Daemon is in draining mode and does not accept new sessions".into(),
            ));
        }

        if client_request_id.trim().is_empty() {
            return Err(SpawnError::Other("clientRequestId cannot be empty".into()));
        }

        let remote = match (
            crate::ssh::projects::is_remote(workspace_id),
            startup.as_ref(),
        ) {
            (true, Some(TerminalStartup::RemoteSsh { host_store_path })) => {
                if shell.is_some() {
                    return Err(SpawnError::Other(
                        "Local shell overrides are unsupported for SSH sessions".into(),
                    ));
                }
                if host_store_path != &self.ssh_store_path {
                    return Err(SpawnError::Other(
                        "SSH inventory path is not daemon-configured".into(),
                    ));
                }
                let path = self.ssh_store_path.clone();
                let id = workspace_id.to_string();
                Some(
                    crate::ipc::run_blocking(move || crate::ssh::projects::resolve(&path, &id))
                        .await
                        .map_err(|e| SpawnError::Other(e.to_string()))?,
                )
            }
            (true, _) | (false, Some(TerminalStartup::RemoteSsh { .. })) => {
                return Err(SpawnError::Other(
                    "SSH workspace requires stored SSH routing; local fallback is forbidden".into(),
                ));
            }
            (false, _) => None,
        };
        let _spawn_guard = Arc::clone(&self.spawn_lock).lock_owned().await;

        let now = Instant::now();
        let provider_claim = ProviderSessionClaimKey::from_startup(startup.as_ref());
        let spawn_fingerprint = SpawnRequestFingerprint {
            workspace_id: workspace_id.to_string(),
            worktree: worktree.clone(),
            cwd: cwd.clone(),
            cols,
            rows,
            shell: shell.clone(),
            provider_claim: provider_claim.clone(),
            startup: startup.clone(),
        };
        self.prune_dead_spawn_ownership(now);
        {
            let mut cache = self.spawn_idempotency_cache.lock();
            if let Some(entry) = cache.get_mut(client_request_id) {
                if entry.fingerprint != spawn_fingerprint {
                    return Err(SpawnError::Other(format!(
                        "clientRequestId '{client_request_id}' was reused with a different spawn request"
                    )));
                }
                entry.created_at = now;
                return Ok(entry.session_id.clone());
            }
        }

        let existing_request = self
            .session_metadata
            .read()
            .iter()
            .find(|(session_id, meta)| {
                meta.client_request_id == client_request_id && self.session_is_live(session_id)
            })
            .map(|(session_id, meta)| (session_id.clone(), meta.spawn_fingerprint.clone()));
        if let Some((live_session_id, existing_fingerprint)) = existing_request {
            if existing_fingerprint != spawn_fingerprint {
                return Err(SpawnError::Other(format!(
                    "clientRequestId '{client_request_id}' was reused with a different spawn request"
                )));
            }
            self.spawn_idempotency_cache.lock().insert(
                client_request_id.to_string(),
                SpawnCacheEntry {
                    session_id: live_session_id.clone(),
                    created_at: now,
                    fingerprint: spawn_fingerprint.clone(),
                },
            );
            return Ok(live_session_id);
        }

        if let Some((project, host)) = remote {
            return self
                .spawn_remote(
                    project,
                    host,
                    client_request_id,
                    worktree,
                    cwd,
                    cols,
                    rows,
                    spawn_fingerprint,
                    #[cfg(test)]
                    helper_home,
                )
                .await;
        }
        let resume_startup = startup.clone();
        let resume_cwd = crate::ipc::run_blocking(move || {
            crate::terminal::resume_cwd::resolve_agent_resume_cwd(resume_startup.as_ref()).map_err(
                |error| {
                    crate::ipc::IpcError::new(
                        crate::ipc::IpcErrorCode::AgentResumeInvalid,
                        error.to_string(),
                    )
                },
            )
        })
        .await
        .map_err(|error| SpawnError::InvalidAgentResume(error.to_string()))?;
        let workspace_service = Arc::clone(&self.workspace_service);
        let terminal_service = Arc::clone(&self.terminal_service);
        let claims = Arc::clone(&self.provider_session_claims);
        let workspace_id_owned = workspace_id.to_string();
        let spawn_worktree = worktree.clone();
        let spawn_startup = startup.clone();
        let spawn_claim = provider_claim.clone();
        let session_router = Arc::clone(&self.session_router);
        let spawn_idempotency_cache = Arc::clone(&self.spawn_idempotency_cache);
        let session_metadata = Arc::clone(&self.session_metadata);
        let provider_session_claims = Arc::clone(&self.provider_session_claims);
        let agent_states = Arc::clone(&self.agent_states);
        let handover_manager = self.handover_manager.clone();
        let client_request_id = client_request_id.to_string();
        crate::ipc::run_blocking(move || {
            // Cancellation cannot release admission before PTY ownership is published.
            let _spawn_guard = _spawn_guard;
            let _gate = workspace_service.mutation_gate.lock();
            let result = (|| -> Result<_, SpawnError> {
                // Resolve manager from workspace registry; workspace MUST be registered.
                let (mgr, default_cwd) = workspace_service
                    .registry
                    .resolve_terminal_target(&workspace_id_owned, spawn_worktree.as_ref())
                    .map_err(|e| SpawnError::Other(e.to_string()))?;

                let cwd = resume_cwd
                    .map(|path| path.to_string_lossy().into_owned())
                    .or(cwd);
                let resolved_cwd = if let Some(ref custom_cwd_str) = cwd {
                    let custom_path = PathBuf::from(custom_cwd_str);
                    if !custom_path.exists() {
                        return Err(SpawnError::Other(format!(
                            "CWD does not exist: {custom_cwd_str}"
                        )));
                    }
                    if !custom_path.is_dir() {
                        return Err(SpawnError::Other(format!(
                            "CWD is not a directory: {custom_cwd_str}"
                        )));
                    }
                    let canonical = fs::canonicalize(&custom_path).map_err(|e| {
                        SpawnError::Other(format!("Cannot canonicalize CWD {custom_cwd_str}: {e}"))
                    })?;
                    let allowed = mgr.canonical_allowed_path(&canonical).map_err(|e| {
                        SpawnError::Other(format!(
                            "CWD '{custom_cwd_str}' is outside workspace: {e}"
                        ))
                    })?;
                    if allowed != default_cwd && !allowed.starts_with(&default_cwd) {
                        return Err(SpawnError::Other(format!(
                    "CWD '{custom_cwd_str}' is outside the resolved workspace/worktree root '{}'",
                    default_cwd.display()
                )));
                    }
                    allowed
                } else {
                    default_cwd
                };

                let mut cmd = match crate::terminal::shell::resolve_startup_command(
                    shell.as_deref(),
                    spawn_startup.as_ref(),
                ) {
                    Ok(cmd) => cmd,
                    Err(err) => return Err(SpawnError::InvalidAgentResume(err.to_string())),
                };
                if let Some(claim) = spawn_claim.as_ref() {
                    if let Some(existing_session_id) = claims.lock().get(claim) {
                        return Err(SpawnError::AgentSessionConflict {
                            agent_type: claim.agent_type.clone(),
                            provider_key: claim.provider_key,
                            provider_id: claim.provider_id.clone(),
                            existing_session_id: existing_session_id.clone(),
                        });
                    }
                }
                cmd.env("PROMPT_EOL_MARK", "");
                cmd.cwd(normalize_process_cwd(&resolved_cwd));

                let (session_id, mut lifecycle_rx) = terminal_service
                    .spawn_in_worktree(cmd, cols, rows, &mgr, &resolved_cwd)
                    .map_err(|e| SpawnError::Other(e.to_string()))?;
                // Store idempotency entry and session metadata before releasing the request lock.
                let ssh_store = match startup.as_ref() {
                    Some(TerminalStartup::RemoteSsh { host_store_path }) => {
                        Some(host_store_path.clone())
                    }
                    _ => None,
                };
                session_router.register_workspace(&session_id, &workspace_id_owned, ssh_store);
                spawn_idempotency_cache.lock().insert(
                    client_request_id.to_string(),
                    SpawnCacheEntry {
                        session_id: session_id.clone(),
                        created_at: now,
                        fingerprint: spawn_fingerprint.clone(),
                    },
                );
                session_metadata.write().insert(
                    session_id.clone(),
                    StoredSessionMeta {
                        client_request_id: client_request_id.to_string(),
                        workspace_id: workspace_id_owned.clone(),
                        worktree,
                        cwd: resolved_cwd,
                        provider_claim: provider_claim.clone(),
                        spawn_fingerprint,
                    },
                );
                if let Some(claim) = provider_claim {
                    provider_session_claims
                        .lock()
                        .insert(claim, session_id.clone());
                }

                let cleanup_session_id = session_id.clone();
                let cleanup_router = Arc::clone(&session_router);
                let cleanup_cache = Arc::clone(&spawn_idempotency_cache);
                let cleanup_metadata = Arc::clone(&session_metadata);
                let cleanup_claims = Arc::clone(&provider_session_claims);
                let cleanup_agent_states = Arc::clone(&agent_states);
                let handover_manager = handover_manager.clone();
                let terminal_service = Arc::clone(&terminal_service);
                tokio::spawn(async move {
                    loop {
                        match lifecycle_rx.recv().await {
                            Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    cleanup_router.remove_workspace(&cleanup_session_id);
                    cleanup_agent_states.remove(&cleanup_session_id);
                    cleanup_cache
                        .lock()
                        .retain(|_, entry| entry.session_id != cleanup_session_id);
                    if let Some(meta) = cleanup_metadata.write().remove(&cleanup_session_id) {
                        if let Some(claim) = meta.provider_claim {
                            cleanup_claims
                                .lock()
                                .retain(|key, owner| key != &claim || owner != &cleanup_session_id);
                        }
                    }
                    if let Some(manager) = handover_manager.upgrade() {
                        manager.check_retirement_if_empty(&terminal_service);
                    }
                });

                Ok(session_id)
            })();
            Ok(result)
        })
        .await
        .map_err(|error| SpawnError::Other(error.to_string()))?
    }

    pub(super) async fn validate_session_ssh_target(
        &self,
        session_id: &str,
    ) -> Result<(), crate::ipc::IpcError> {
        let meta = self.session_metadata.read().get(session_id).cloned();
        if let Some(meta) = meta {
            if crate::ssh::projects::is_remote(&meta.workspace_id) {
                let path = self.ssh_store_path.clone();
                crate::ipc::run_blocking(move || {
                    crate::ssh::projects::resolve(&path, &meta.workspace_id)
                })
                .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn handle_close(
        &self,
        session_id: &str,
    ) -> Result<(), crate::terminal::PtyError> {
        let remote = self.terminal_service.remote().contains(session_id);
        self.terminal_service.close_session(session_id).await?;
        self.release_session_ownership(session_id);
        self.agent_states.remove(session_id);
        if remote {
            self.persist_remote_sessions_at(self.remote_sessions_path.clone())
                .await
                .map_err(crate::terminal::PtyError::Other)?;
        }
        Ok(())
    }

    pub(super) fn session_is_live(&self, session_id: &str) -> bool {
        if self.terminal_service.remote().contains(session_id) {
            return true;
        }
        self.terminal_service
            .get_session(session_id)
            .is_some_and(|session| {
                matches!(
                    session.state(),
                    PtySessionState::Starting | PtySessionState::Running
                )
            })
    }

    pub(super) fn release_session_ownership(&self, session_id: &str) {
        self.session_router.remove_workspace(session_id);
        self.spawn_idempotency_cache
            .lock()
            .retain(|_, entry| entry.session_id != session_id);
        if let Some(meta) = self.session_metadata.write().remove(session_id) {
            if let Some(claim) = meta.provider_claim {
                self.provider_session_claims
                    .lock()
                    .retain(|key, owner| key != &claim || owner != session_id);
            }
        }
    }

    pub(super) fn prune_dead_spawn_ownership(&self, now: Instant) {
        let dead_sessions: Vec<String> = self
            .session_metadata
            .read()
            .keys()
            .filter(|session_id| !self.session_is_live(session_id))
            .cloned()
            .collect();
        for session_id in dead_sessions {
            self.release_session_ownership(&session_id);
        }
        self.spawn_idempotency_cache.lock().retain(|_, entry| {
            now.duration_since(entry.created_at) <= SPAWN_REQUEST_TTL
                && self.session_is_live(&entry.session_id)
        });
        self.provider_session_claims
            .lock()
            .retain(|_, session_id| self.session_is_live(session_id));
    }

    #[cfg(test)]
    pub(super) fn expire_spawn_request_for_test(&self, client_request_id: &str) {
        if let Some(entry) = self
            .spawn_idempotency_cache
            .lock()
            .get_mut(client_request_id)
        {
            entry.created_at = Instant::now() - SPAWN_REQUEST_TTL - Duration::from_secs(1);
        }
    }

    #[cfg(test)]
    pub(super) fn spawn_cache_len_for_test(&self) -> usize {
        self.spawn_idempotency_cache.lock().len()
    }

    #[cfg(test)]
    pub(super) fn provider_claim_len_for_test(&self) -> usize {
        self.provider_session_claims.lock().len()
    }

    #[cfg(test)]
    pub(super) fn reserve_provider_claim_for_test(
        &self,
        client_request_id: &str,
        session_id: &str,
        startup: &TerminalStartup,
    ) -> Result<String, SpawnError> {
        let claim = ProviderSessionClaimKey::from_startup(Some(startup))
            .expect("agent resume startup has a provider claim");
        let mut claims = self.provider_session_claims.lock();
        if let Some(existing_session_id) = claims.get(&claim) {
            return Err(SpawnError::AgentSessionConflict {
                agent_type: claim.agent_type,
                provider_key: claim.provider_key,
                provider_id: claim.provider_id,
                existing_session_id: existing_session_id.clone(),
            });
        }
        claims.insert(claim.clone(), session_id.to_string());
        self.session_metadata.write().insert(
            session_id.to_string(),
            StoredSessionMeta {
                client_request_id: client_request_id.to_string(),
                workspace_id: "test".to_string(),
                worktree: None,
                cwd: PathBuf::from("/test"),
                provider_claim: Some(claim),
                spawn_fingerprint: SpawnRequestFingerprint {
                    workspace_id: "test".to_string(),
                    worktree: None,
                    cwd: None,
                    cols: 80,
                    rows: 24,
                    shell: None,
                    provider_claim: ProviderSessionClaimKey::from_startup(Some(startup)),
                    startup: Some(startup.clone()),
                },
            },
        );
        Ok(session_id.to_string())
    }

    pub(super) fn handle_describe_session(&self, session_id: &str) -> DaemonResponse {
        if let Some(details) = self.terminal_service.remote().details(session_id) {
            let d = details.descriptor;
            let meta = self.session_metadata.read().get(session_id).cloned();
            let (start_sequence, end_sequence) = self
                .terminal_service
                .output_hub()
                .session_sequence_range(session_id)
                .unwrap_or((None, None));
            return DaemonResponse::DescribeSessionOk {
                session: DaemonSessionDetails {
                    session_id: session_id.into(),
                    workspace_id: Some(d.config.project_id),
                    worktree: meta.as_ref().and_then(|m| m.worktree.clone()),
                    cwd: Some(
                        meta.map(|m| m.cwd.to_string_lossy().into_owned())
                            .unwrap_or(d.config.project_path),
                    ),
                    cols: d.cols,
                    rows: d.rows,
                    running: details.state
                        == crate::terminal::remote::RemoteConnectionState::Connected,
                    start_sequence,
                    end_sequence,
                },
            };
        }
        let Some(pty_session) = self.terminal_service.get_session(session_id) else {
            return DaemonResponse::Error {
                message: format!("Session '{session_id}' not found"),
            };
        };

        let (cols, rows) = pty_session.get_size();
        let running = matches!(
            pty_session.state(),
            PtySessionState::Starting | PtySessionState::Running
        );
        let (start_sequence, end_sequence) = self
            .terminal_service
            .output_hub()
            .session_sequence_range(session_id)
            .unwrap_or((None, None));

        let meta = self.session_metadata.read().get(session_id).cloned();
        let (workspace_id, worktree, cwd) = match meta {
            Some(m) => (
                Some(m.workspace_id),
                m.worktree,
                Some(m.cwd.to_string_lossy().to_string()),
            ),
            None => (
                None,
                None,
                pty_session
                    .worktree_path()
                    .map(|p| p.to_string_lossy().to_string()),
            ),
        };

        DaemonResponse::DescribeSessionOk {
            session: DaemonSessionDetails {
                session_id: session_id.to_string(),
                workspace_id,
                worktree,
                cwd,
                cols,
                rows,
                running,
                start_sequence,
                end_sequence,
            },
        }
    }
}
