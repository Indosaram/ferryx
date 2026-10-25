#[path = "workspace_watcher.rs"]
pub(crate) mod workspace_watcher;
use crate::remote::{
    machine_protocol::Availability,
    workspace_catalog::{self, Catalog, CatalogRow},
};
use crate::worktree::{WorkspaceRegistry, WorktreeManager};
use std::{fs, path::PathBuf};

/// Internal invalidation only; A12 owns any future authenticated stream projection.
#[derive(Clone, Debug)]
pub struct WorktreeCommittedChange {
    pub workspace_id: String,
    pub removed: bool,
    pub revision: crate::scoped_contracts::Epoch,
}

/// Shared admission gate for daemon registry mutations and validated PTY spawn.
pub struct DaemonWorkspaceService {
    pub machine_events: std::sync::Arc<crate::remote::machine_events::MachineEvents>,
    pub(crate) registry: WorkspaceRegistry,
    pub(crate) mutation_gate: parking_lot::Mutex<()>,
    pub(crate) catalog: parking_lot::Mutex<Result<Catalog, String>>,
    pub(crate) catalog_path: PathBuf,
    pub(crate) sidebar: super::sidebar_snapshot::SidebarSnapshotStore,
    worktree_gates: parking_lot::Mutex<
        std::collections::HashMap<String, std::sync::Weak<parking_lot::Mutex<()>>>,
    >,
    pub journal: crate::remote::machine_operation_journal::MachineOperationJournal,
    worktree_changes: tokio::sync::broadcast::Sender<WorktreeCommittedChange>,

    pub(crate) project_reads: std::sync::Arc<tokio::sync::Semaphore>,
    pub(crate) project_mutations: std::sync::Arc<tokio::sync::Semaphore>,
    #[cfg(test)]
    pub(crate) transaction_probe: TransactionProbe,
}

#[cfg(test)]
type TransactionProbe = parking_lot::RwLock<Option<std::sync::Arc<dyn Fn(&str) + Send + Sync>>>;

impl DaemonWorkspaceService {
    pub(crate) fn new(registry: WorkspaceRegistry, catalog_path: PathBuf) -> Self {
        let mut catalog = workspace_catalog::load(&catalog_path);
        let journal = crate::remote::machine_operation_journal::MachineOperationJournal::open(
            catalog_path.with_file_name("machine-operations.v1.json"),
        );
        if let Ok(c) = &catalog {
            if let Some(receipt) = &c.transaction {
                if let Err(error) = journal.recover_catalog(receipt) {
                    catalog = Err(error);
                }
            }
        }
        if let Ok(catalog) = &mut catalog {
            for (id, row) in &mut catalog.workspaces {
                row.availability = match fs::metadata(&row.repo_root) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Availability::Missing,
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                        Availability::PermissionDenied
                    }
                    Err(_) => Availability::Invalid,
                    Ok(metadata) if !metadata.is_dir() => Availability::Invalid,
                    Ok(_) if !row.mirror_exposed => match fs::read_dir(&row.repo_root) {
                        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                            Availability::PermissionDenied
                        }
                        Err(_) => Availability::Invalid,
                        Ok(_)
                            if fs::canonicalize(&row.repo_root).ok().as_ref()
                                == Some(&row.repo_root) =>
                        {
                            Availability::Ready
                        }
                        Ok(_) => Availability::Invalid,
                    },
                    Ok(_) => match WorktreeManager::try_new(&row.repo_root) {
                        Ok(manager) if manager.repo_root() == row.repo_root => {
                            // Machine-only entries are not exposed to the legacy mirror registry.
                            if row.mirror_exposed {
                                registry.publish(id.clone(), manager);
                            }
                            Availability::Ready
                        }
                        _ => Availability::Invalid,
                    },
                };
            }
        }
        Self {
            machine_events: std::sync::Arc::new(Default::default()),
            journal,
            sidebar: super::sidebar_snapshot::SidebarSnapshotStore::load(
                catalog_path.with_file_name("desktop-sidebar.v1.json"),
            ),
            worktree_changes: tokio::sync::broadcast::channel(64).0,

            project_reads: std::sync::Arc::new(tokio::sync::Semaphore::new(8)),
            project_mutations: std::sync::Arc::new(tokio::sync::Semaphore::new(8)),
            #[cfg(test)]
            transaction_probe: parking_lot::RwLock::new(None),
            registry,
            mutation_gate: parking_lot::Mutex::new(()),
            catalog: parking_lot::Mutex::new(catalog),
            catalog_path,
            worktree_gates: parking_lot::Mutex::new(Default::default()),
        }
    }
    pub fn subscribe_worktree_changes(
        &self,
    ) -> tokio::sync::broadcast::Receiver<WorktreeCommittedChange> {
        self.worktree_changes.subscribe()
    }
    pub(crate) fn observe_worktrees(
        &self,
        workspace: &str,
        digest: String,
        deadline: std::time::Instant,
    ) -> Result<crate::scoped_contracts::Epoch, String> {
        let _publication = self
            .mutation_gate
            .try_lock_until(deadline)
            .ok_or("TIMEOUT")?;
        let mut catalog = self.catalog()?;
        if !catalog.workspaces.contains_key(workspace) && !catalog.desktop_local_projects.iter().flatten()
            .any(|member| member.workspace_id == workspace) {
            return Err("PROJECT_NOT_FOUND".into());
        }
        if catalog.worktree_observations.get(workspace) != Some(&digest) {
            // Missing baselines (including old catalogs) invalidate old previews.
            catalog.revision.0 = catalog
                .revision
                .0
                .checked_add(1)
                .ok_or("CAPACITY_EXCEEDED")?;
            catalog
                .worktree_observations
                .insert(workspace.into(), digest);
            if workspace_catalog::persist(&self.catalog_path, &catalog).is_err() {
                *self.catalog.lock() = Err("MACHINE_SERVICE_UNAVAILABLE".into());
                return Err("OPERATION_OUTCOME_UNKNOWN".into());
            }
            *self.catalog.lock() = Ok(catalog);
            self.registry.bump_revision();
        }
        Ok(self.catalog()?.revision)
    }
    pub(crate) fn worktree_committed(
        &self,
        workspace: &str,
        removed: bool,
        revision: crate::scoped_contracts::Epoch,
        worktree: &crate::remote::machine_protocol::Worktree,
    ) {
        self.machine_events.publish_revision(
            revision.0,
            if removed {
                "worktreeRemoved"
            } else {
                "worktreeCreated"
            },
            Some(workspace),
            None,
            serde_json::json!(worktree),
        );
        self.registry.bump_revision();
        // No receivers is normal until an inventory consumer subscribes.
        let _ = self.worktree_changes.send(WorktreeCommittedChange {
            workspace_id: workspace.into(),
            removed,
            revision,
        });
    }
    pub(crate) fn worktree_gate(&self, workspace: &str) -> std::sync::Arc<parking_lot::Mutex<()>> {
        #[cfg(test)]
        if let Some(probe) = self.transaction_probe.read().clone() {
            probe("workspaceGateRequested");
        }
        let mut gates = self.worktree_gates.lock();
        gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = gates.get(workspace).and_then(std::sync::Weak::upgrade) {
            return gate;
        }
        let gate = std::sync::Arc::new(parking_lot::Mutex::new(()));
        gates.insert(workspace.to_owned(), std::sync::Arc::downgrade(&gate));
        gate
    }

    /// Resolve from the durable authority without exposing machine roots in the mirror registry.
    pub(crate) fn worktree_manager(
        &self,
        workspace: &str,
        mirror: bool,
    ) -> Result<WorktreeManager, String> {
        WorkspaceRegistry::validate_workspace_id(workspace).map_err(|_| "INVALID_REQUEST")?;
        let catalog = self.catalog()?;
        let saved;
        let row = if let Some(row) = catalog.workspaces.get(workspace) { row } else {
            if mirror { return Err("PROJECT_NOT_FOUND".into()); }
            let member = catalog.desktop_local_projects.iter().flatten()
                .find(|member| member.workspace_id == workspace).ok_or("PROJECT_NOT_FOUND")?;
            saved = CatalogRow { repo_root: member.repo_root.clone(), mirror_exposed: false,
                availability: Availability::Invalid };
            &saved
        };
        if mirror && !row.mirror_exposed {
            return Err("MACHINE_ACCESS_REQUIRED".into());
        }
        if fs::canonicalize(&row.repo_root).ok().as_ref() != Some(&row.repo_root) {
            return Err("INVALID_PATH".into());
        }
        if let Ok(manager) = self.registry.manager(workspace) {
            if manager.repo_root() == row.repo_root {
                return Ok(manager);
            }
        }
        let manager = WorktreeManager::try_new(&row.repo_root).map_err(|_| "INVALID_PATH")?;
        if manager.repo_root() != row.repo_root {
            return Err("INVALID_PATH".into());
        }
        Ok(manager)
    }

    pub fn register(&self, workspace_id: &str, repo_root: &str) -> Result<(), String> {
        #[cfg(test)]
        if let Some(probe) = self.transaction_probe.read().clone() { probe("registerWorkspace"); }
        let workspace_gate = self.worktree_gate(workspace_id);
        let _workspace_gate = workspace_gate.lock();
        let _gate = self.mutation_gate.lock();
        let workspace_id =
            WorkspaceRegistry::validate_workspace_id(workspace_id).map_err(|e| e.to_string())?;
        let path = PathBuf::from(repo_root);
        if !path.is_absolute() {
            return Err("repo_root must be an absolute path".into());
        }
        let canonical = fs::canonicalize(&path)
            .map_err(|e| format!("Invalid repo_root path '{}': {e}", path.display()))?;
        if !canonical.is_dir() || canonical.parent().is_none() {
            return Err(format!(
                "Repo root '{}' is not a valid project directory",
                canonical.display()
            ));
        }
        let manager = WorktreeManager::try_new(&canonical).map_err(|e| e.to_string())?;
        if manager.repo_root() != canonical {
            return Err(format!(
                "repo_root '{}' must be the canonical repository root '{}'",
                path.display(),
                manager.repo_root().display()
            ));
        }
        let mut state = self.catalog.lock();
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        if let Some(existing) = candidate.workspaces.get(workspace_id) {
            if existing.repo_root != canonical {
                return Err("Workspace already registered".into());
            }
            if existing.mirror_exposed && self.registry.contains(workspace_id) {
                return Ok(());
            }
        }
        let mut project =
            crate::remote::workspace_api::registration_project(workspace_id, &canonical)?;
        candidate.workspaces.insert(
            workspace_id.to_string(),
            CatalogRow {
                repo_root: canonical,
                mirror_exposed: true,
                availability: Availability::Ready,
            },
        );
        candidate.ownership.entry(workspace_id.into()).or_default().desktop = true;
        candidate.revision.0 = candidate
            .revision
            .0
            .checked_add(1)
            .ok_or("Catalog revision exhausted")?;
        if let Err(error) = workspace_catalog::persist(&self.catalog_path, &candidate) {
            *state = Err(format!(
                "Catalog mutations disabled after persistence failure: {error}"
            ));
            return Err(error);
        }
        let revision = candidate.revision.0;
        *state = Ok(candidate);
        self.registry.publish(workspace_id.to_string(), manager);
        project.revision = crate::scoped_contracts::Epoch(revision);
        self.machine_events.publish_revision(
            revision,
            "projectRegistered",
            Some(workspace_id),
            None,
            serde_json::json!(project),
        );
        Ok(())
    }

    pub fn sync_desktop_local_projects(
        &self,
        projects: Vec<workspace_catalog::DesktopLocalProject>,
    ) -> Result<(), String> {
        let _gate = self.mutation_gate.lock();
        let mut candidate = self.catalog()?;
        let mut ids = std::collections::BTreeSet::new();
        let mut members = Vec::with_capacity(projects.len());
        for mut project in projects {
            let validated = WorkspaceRegistry::validate_workspace_id(&project.workspace_id)
                .map_err(|_| "INVALID_REQUEST")?;
            if validated != project.workspace_id || validated == "default" {
                return Err("INVALID_REQUEST".into());
            }
            if !ids.insert(project.workspace_id.clone()) || !project.repo_root.is_absolute()
                || project.repo_root.parent().is_none() {
                return Err("INVALID_REQUEST".into());
            }
            // Membership is the desktop's list; validation only decides which root is
            // recorded. A member that cannot be validated now (offline volume, a Git
            // parent appearing above a folder, an I/O error) stays a member under its
            // last trusted root and is reported unavailable by the inventory probe, so
            // one bad project can never freeze the whole list.
            let validated_root = fs::canonicalize(&project.repo_root).ok().filter(|root| {
                WorktreeManager::try_new(root).is_ok_and(|manager| manager.repo_root() == root)
                    && candidate.workspaces.get(&project.workspace_id)
                        .is_none_or(|row| &row.repo_root == root)
            });
            if let Some(root) = validated_root {
                project.repo_root = root;
            } else if let Some(row) = candidate.workspaces.get(&project.workspace_id) {
                project.repo_root = row.repo_root.clone();
            } else if let Some(previous) = candidate.desktop_local_projects.iter().flatten()
                .find(|member| member.workspace_id == project.workspace_id) {
                project.repo_root = previous.repo_root.clone();
            }
            members.push(project);
        }
        if candidate.desktop_local_projects.as_ref() == Some(&members) { return Ok(()); }
        for owner in candidate.ownership.values_mut() { owner.desktop = false; }
        for member in &members {
            candidate.ownership.entry(member.workspace_id.clone()).or_default().desktop = true;
        }
        candidate.desktop_local_projects = Some(members);
        candidate.revision.0 = candidate.revision.0.checked_add(1).ok_or("CAPACITY_EXCEEDED")?;
        if let Err(error) = workspace_catalog::persist(&self.catalog_path, &candidate) {
            *self.catalog.lock() = Err("MACHINE_SERVICE_UNAVAILABLE".into());
            return Err(error);
        }
        let revision = candidate.revision.0;
        *self.catalog.lock() = Ok(candidate);
        self.registry.bump_revision();
        self.machine_events.publish_revision(revision, "inventoryInvalidated", None, None,
            serde_json::json!({"desktopLocalSnapshotExists": true}));
        Ok(())
    }

    pub fn catalog(&self) -> Result<Catalog, String> {
        self.catalog.lock().clone()
    }

    /// Blocking: stores the desktop sidebar model and announces its new revision once.
    pub fn sync_desktop_sidebar(
        &self,
        model: serde_json::Value,
    ) -> Result<u64, super::sidebar_snapshot::SidebarRejection> {
        self.sidebar.store(model, &self.machine_events)
    }

    /// A desktop opened its event stream.
    pub fn desktop_stream_opened(self: &std::sync::Arc<Self>) -> DesktopStreamGuard {
        self.sidebar.desktop_stream_opened(&self.machine_events);
        DesktopStreamGuard(Some(std::sync::Arc::clone(self)))
    }

    /// Open desktop event streams, updated once each open or close has taken effect.
    pub fn desktop_stream_count(&self) -> tokio::sync::watch::Receiver<usize> {
        self.sidebar.desktop_stream_count()
    }

    /// Blocking: every close revokes the current epoch's signals, including non-final closes.
    pub fn desktop_stream_closed(&self) {
        if let Err(error) = self.sidebar.desktop_stream_closed(&self.machine_events) {
            tracing::warn!(%error, "Desktop sidebar invalidation could not be persisted");
        }
    }
}

/// Held for the life of one desktop event stream; dropping it (stream ended, connection closed or
/// the serving task cancelled) runs `desktop_stream_closed` off the async runtime.
pub struct DesktopStreamGuard(Option<std::sync::Arc<DaemonWorkspaceService>>);

impl DesktopStreamGuard {
    /// Finish normal EOF cleanup before the stream handler continues. Drop remains the
    /// cancellation fallback, retaining the service until its blocking cleanup completes.
    pub async fn close(mut self) {
        if let Some(service) = self.0.take() {
            if let Err(error) = crate::ipc::run_blocking(move || {
                service.desktop_stream_closed();
                Ok(())
            }).await {
                tracing::warn!(%error, "Desktop sidebar close task failed");
            }
        }
    }
}

impl Drop for DesktopStreamGuard {
    fn drop(&mut self) {
        let Some(service) = self.0.take() else { return; };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn_blocking(move || service.desktop_stream_closed());
            }
            Err(_) => service.desktop_stream_closed(),
        }
    }
}

impl DaemonWorkspaceService {
    /// Blocking domain operation; async transports must use run_blocking.
    /// Machine identities adopt a canonical root, unlike compatibility IPC IDs.
    pub fn register_machine(&self, repo_root: &str) -> Result<String, String> {
        let _gate = self.mutation_gate.lock();
        if !PathBuf::from(repo_root).is_absolute() {
            return Err("Absolute root required".into());
        }
        let manager =
            WorktreeManager::try_new(PathBuf::from(repo_root)).map_err(|e| e.to_string())?;
        let mut state = self.catalog.lock();
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        if let Some((id, _)) = candidate
            .workspaces
            .iter()
            .find(|(_, row)| row.repo_root == manager.repo_root())
        {
            let id = id.clone();
            if !candidate.ownership.get(&id).is_some_and(|owner| owner.machine) {
                candidate.ownership.entry(id.clone()).or_default().machine = true;
                if let Err(error) = workspace_catalog::persist(&self.catalog_path, &candidate) {
                    *state = Err("MACHINE_SERVICE_UNAVAILABLE".into());
                    return Err(error);
                }
                *state = Ok(candidate);
            }
            return Ok(id);
        }
        let id = format!("project-{}", uuid::Uuid::new_v4().simple());
        let mut project =
            crate::remote::workspace_api::registration_project(&id, manager.repo_root())?;
        candidate.workspaces.insert(
            id.clone(),
            CatalogRow {
                repo_root: manager.repo_root().to_owned(),
                mirror_exposed: false,
                availability: Availability::Ready,
            },
        );
        candidate.ownership.entry(id.clone()).or_default().machine = true;
        candidate.revision.0 = candidate
            .revision
            .0
            .checked_add(1)
            .ok_or("Catalog revision exhausted")?;
        if let Err(error) = workspace_catalog::persist(&self.catalog_path, &candidate) {
            *state = Err(format!(
                "Catalog mutations disabled after persistence failure: {error}"
            ));
            return Err(error);
        }
        let revision = candidate.revision.0;
        *state = Ok(candidate);
        project.revision = crate::scoped_contracts::Epoch(revision);
        self.machine_events.publish_revision(
            revision,
            "projectRegistered",
            Some(&id),
            None,
            serde_json::json!(project),
        );
        Ok(id)
    }

    pub(crate) fn unregister(&self, workspace_id: &str) -> Result<(), String> {
        let workspace_gate = self.worktree_gate(workspace_id);
        let _workspace_gate = workspace_gate.lock();
        let _gate = self.mutation_gate.lock();
        let workspace_id =
            WorkspaceRegistry::validate_workspace_id(workspace_id).map_err(|e| e.to_string())?;
        let mut state = self.catalog.lock();
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        if candidate.workspaces.remove(workspace_id).is_none() {
            return Ok(());
        }
        candidate.revision.0 = candidate
            .revision
            .0
            .checked_add(1)
            .ok_or("Catalog revision exhausted")?;
        if let Err(error) = workspace_catalog::persist(&self.catalog_path, &candidate) {
            *state = Err(format!(
                "Catalog mutations disabled after persistence failure: {error}"
            ));
            return Err(error);
        }
        let revision = candidate.revision.0;
        *state = Ok(candidate);
        self.registry.unregister(workspace_id);
        self.machine_events.publish_revision(
            revision,
            "projectRemoved",
            Some(workspace_id),
            None,
            serde_json::json!({}),
        );
        Ok(())
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    #[tokio::test]
    async fn r28_rust_one_unvalidatable_member_does_not_freeze_desktop_membership() {
        crate::ipc::run_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            let home = fs::canonicalize(root.path()).unwrap();
            let scratch = home.join("scratch");
            let added = home.join("added");
            fs::create_dir(&scratch).unwrap();
            fs::create_dir(&added).unwrap();
            let service = DaemonWorkspaceService::new(WorkspaceRegistry::new(), home.join("data/catalog"));
            let member = |id: &str, path: &std::path::Path| workspace_catalog::DesktopLocalProject {
                workspace_id: id.into(), repo_root: path.to_path_buf(),
            };
            service.sync_desktop_local_projects(vec![member("scratch", &scratch)]).unwrap();
            // A Git repository appears above the plain folder, so it no longer validates.
            let status = std::process::Command::new("git").arg("init").arg("-q").arg(&home).status().unwrap();
            assert!(status.success());
            service.sync_desktop_local_projects(vec![member("scratch", &scratch), member("added", &added)]).unwrap();
            let members = service.catalog().unwrap().desktop_local_projects.unwrap();
            assert_eq!(members.iter().map(|m| m.workspace_id.as_str()).collect::<Vec<_>>(), vec!["scratch", "added"]);
            assert_eq!(members[0].repo_root, scratch);
            drop(service);
            root.close().unwrap();
            Ok(())
        }).await.unwrap();
    }

    #[tokio::test]
    async fn desktop_snapshot_is_idempotent_retains_rows_and_persists_empty() {
        crate::ipc::run_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            let plain = root.path().join("plain");
            fs::create_dir(&plain).unwrap();
            let path = root.path().join("data/catalog");
            let service = DaemonWorkspaceService::new(WorkspaceRegistry::new(), path.clone());
            service.register("legacy", plain.to_str().unwrap()).unwrap();
            let members = vec![workspace_catalog::DesktopLocalProject {
                workspace_id: "offline".into(), repo_root: root.path().join("absent/repo"),
            }];
            service.sync_desktop_local_projects(members.clone()).unwrap();
            let revision = service.catalog().unwrap().revision;
            service.sync_desktop_local_projects(members).unwrap();
            assert_eq!(service.catalog().unwrap().revision, revision);
            service.sync_desktop_local_projects(vec![]).unwrap();
            drop(service);
            let restarted = DaemonWorkspaceService::new(WorkspaceRegistry::new(), path);
            let catalog = restarted.catalog().unwrap();
            assert_eq!(catalog.desktop_local_projects, Some(vec![]));
            assert!(catalog.workspaces.contains_key("legacy"));
            assert!(plain.is_dir());
            drop(restarted);
            root.close().unwrap();
            Ok(())
        }).await.unwrap();
    }

    #[tokio::test]
    async fn desktop_snapshot_rejects_remote_duplicate_and_placeholder_without_mutation() {
        crate::ipc::run_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            let service = DaemonWorkspaceService::new(WorkspaceRegistry::new(), root.path().join("data/catalog"));
            for id in ["ssh:remote", "daemon:remote", "default"] {
                assert!(service.sync_desktop_local_projects(vec![workspace_catalog::DesktopLocalProject {
                    workspace_id: id.into(), repo_root: root.path().join("missing"),
                }]).is_err());
            }
            let member = workspace_catalog::DesktopLocalProject { workspace_id: "a".into(), repo_root: root.path().join("missing") };
            assert!(service.sync_desktop_local_projects(vec![member.clone(), member]).is_err());
            assert!(service.catalog().unwrap().desktop_local_projects.is_none());
            assert_eq!(service.catalog().unwrap().revision.0, 0);
            drop(service);
            root.close().unwrap();
            Ok(())
        }).await.unwrap();
    }

    #[tokio::test]
    async fn unrelated_workspace_registration_progresses_while_workspace_fenced() {
        let (root, service) = crate::ipc::run_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            let service = std::sync::Arc::new(DaemonWorkspaceService::new(
                WorkspaceRegistry::new(),
                root.path().join("data/catalog"),
            ));
            fs::create_dir(root.path().join("other")).unwrap();
            Ok((root, service))
        })
        .await
        .unwrap();
        let other = root.path().join("other");
        let worker_service = service.clone();
        crate::ipc::run_blocking(move || {
            let fence = worker_service.worktree_gate("busy");
            let held = fence.lock();
            assert!(worker_service.worktree_gate("busy").try_lock().is_none());
            let (completed, progress) = std::sync::mpsc::sync_channel(1);
            let registering_service = worker_service.clone();
            let worker = std::thread::spawn(move || {
                let result = registering_service.register("other", other.to_str().unwrap());
                completed.send(result).unwrap();
            });
            let progress = progress.recv_timeout(std::time::Duration::from_secs(5));
            drop(held);
            worker.join().unwrap();
            progress.unwrap().unwrap();
            assert!(worker_service.registry.contains("other"));
            Ok(())
        }).await.unwrap();
        drop(service);
        crate::ipc::run_blocking(move || {
            root.close().unwrap();
            Ok(())
        })
        .await
        .unwrap();
    }
    #[test]
    fn post_rename_sync_failure_fences_publication() {
        let root = tempfile::tempdir().unwrap();
        let plain = root.path().join("plain");
        fs::create_dir(&plain).unwrap();
        let path = root.path().join("data/machine-workspaces.v1.json");
        let registry = WorkspaceRegistry::new();
        let service = DaemonWorkspaceService::new(registry.clone(), path.clone());
        service
            .register("existing", plain.to_str().unwrap())
            .unwrap();
        let revision = registry.revision();
        workspace_catalog::FAIL_DIRECTORY_SYNC.with(|fail| fail.set(true));
        let error = service
            .register("ambiguous", plain.to_str().unwrap())
            .unwrap_err();
        assert!(error.contains("post-rename"));
        assert!(!registry.contains("ambiguous"));
        assert_eq!(registry.revision(), revision);
        assert!(service.catalog().is_err());
        assert!(service.register("fenced", plain.to_str().unwrap()).is_err());
        let disk = workspace_catalog::load(&path).unwrap();
        assert!(disk.workspaces.contains_key("ambiguous"));
        assert!(!disk.workspaces.contains_key("fenced"));
        eprintln!(
            "SYNC_FAILURE after rename: candidate on disk, no success/publication, mutation fenced"
        );
        let receipt = root.path().to_owned();
        root.close().unwrap();
        eprintln!(
            "SYNC_FAILURE CLEANUP root={} absent={}",
            receipt.display(),
            !receipt.exists()
        );
    }
}
