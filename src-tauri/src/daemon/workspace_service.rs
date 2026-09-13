#[path = "workspace_watcher.rs"]
pub(crate) mod workspace_watcher;
use crate::worktree::{WorkspaceRegistry, WorktreeManager};
use std::{fs, path::PathBuf};
use crate::remote::{machine_protocol::Availability, workspace_catalog::{self, Catalog, CatalogRow}};

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
    worktree_gates: parking_lot::Mutex<std::collections::HashMap<String, std::sync::Weak<parking_lot::Mutex<()>>>>,
    pub journal: crate::remote::machine_operation_journal::MachineOperationJournal,
    worktree_changes: tokio::sync::broadcast::Sender<WorktreeCommittedChange>,

    pub(crate) project_reads: std::sync::Arc<tokio::sync::Semaphore>,
    pub(crate) project_mutations: std::sync::Arc<tokio::sync::Semaphore>,
    #[cfg(test)]
    pub(crate) transaction_probe: parking_lot::RwLock<Option<std::sync::Arc<dyn Fn(&str) + Send + Sync>>>,
}

impl DaemonWorkspaceService {
    pub(crate) fn new(registry: WorkspaceRegistry, catalog_path: PathBuf) -> Self {
        let mut catalog = workspace_catalog::load(&catalog_path);
        let journal = crate::remote::machine_operation_journal::MachineOperationJournal::open(catalog_path.with_file_name("machine-operations.v1.json"));
        if let Ok(c) = &catalog {
            if let Some(receipt) = &c.transaction {
                if let Err(error) = journal.recover_catalog(receipt) { catalog = Err(error); }
            }
        }
        if let Ok(catalog) = &mut catalog {
            for (id, row) in &mut catalog.workspaces {
                row.availability = match fs::metadata(&row.repo_root) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Availability::Missing,
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Availability::PermissionDenied,
                    Err(_) => Availability::Invalid,
                    Ok(metadata) if !metadata.is_dir() => Availability::Invalid,
                    Ok(_) if !row.mirror_exposed => match fs::read_dir(&row.repo_root) {
                        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Availability::PermissionDenied,
                        Err(_) => Availability::Invalid,
                        Ok(_) if fs::canonicalize(&row.repo_root).ok().as_ref() == Some(&row.repo_root) => Availability::Ready,
                        Ok(_) => Availability::Invalid,
                    },
                    Ok(_) => match WorktreeManager::try_new(&row.repo_root) {
                        Ok(manager) if manager.repo_root() == row.repo_root => {
                            // Machine-only entries are not exposed to the legacy mirror registry.
                            if row.mirror_exposed { registry.publish(id.clone(), manager); }
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
    pub fn subscribe_worktree_changes(&self) -> tokio::sync::broadcast::Receiver<WorktreeCommittedChange> {
        self.worktree_changes.subscribe()
    }
    pub(crate) fn observe_worktrees(&self, workspace: &str, digest: String, deadline: std::time::Instant) -> Result<crate::scoped_contracts::Epoch, String> {
        let _publication = self.mutation_gate.try_lock_until(deadline).ok_or("TIMEOUT")?;
        let mut catalog = self.catalog()?;
        if !catalog.workspaces.contains_key(workspace) { return Err("PROJECT_NOT_FOUND".into()); }
        if catalog.worktree_observations.get(workspace) != Some(&digest) {
            // Missing baselines (including old catalogs) invalidate old previews.
            catalog.revision.0 = catalog.revision.0.checked_add(1).ok_or("CAPACITY_EXCEEDED")?;
            catalog.worktree_observations.insert(workspace.into(), digest);
            if workspace_catalog::persist(&self.catalog_path, &catalog).is_err() {
                *self.catalog.lock() = Err("MACHINE_SERVICE_UNAVAILABLE".into());
                return Err("OPERATION_OUTCOME_UNKNOWN".into());
            }
            *self.catalog.lock() = Ok(catalog);
            self.registry.bump_revision();
        }
        Ok(self.catalog()?.revision)
    }
    pub(crate) fn worktree_committed(&self, workspace: &str, removed: bool, revision: crate::scoped_contracts::Epoch, worktree: &crate::remote::machine_protocol::Worktree) {
        self.machine_events.publish_revision(revision.0, if removed { "worktreeRemoved" } else { "worktreeCreated" }, Some(workspace), None, serde_json::json!(worktree));
        self.registry.bump_revision();
        // No receivers is normal until an inventory consumer subscribes.
        let _ = self.worktree_changes.send(WorktreeCommittedChange { workspace_id: workspace.into(), removed, revision });
    }
    pub(crate) fn worktree_gate(&self, workspace: &str) -> std::sync::Arc<parking_lot::Mutex<()>> {
        #[cfg(test)]
        if let Some(probe) = self.transaction_probe.read().clone() { probe("workspaceGateRequested"); }
        let mut gates = self.worktree_gates.lock();
        gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = gates.get(workspace).and_then(std::sync::Weak::upgrade) { return gate; }
        let gate = std::sync::Arc::new(parking_lot::Mutex::new(()));
        gates.insert(workspace.to_owned(), std::sync::Arc::downgrade(&gate));
        gate
    }

    /// Resolve from the durable authority without exposing machine roots in the mirror registry.
    pub(crate) fn worktree_manager(&self, workspace: &str, mirror: bool) -> Result<WorktreeManager, String> {
        WorkspaceRegistry::validate_workspace_id(workspace).map_err(|_| "INVALID_REQUEST")?;
        let catalog = self.catalog()?;
        let row = catalog.workspaces.get(workspace).ok_or("PROJECT_NOT_FOUND")?;
        if mirror && !row.mirror_exposed { return Err("MACHINE_ACCESS_REQUIRED".into()); }
        if fs::canonicalize(&row.repo_root).ok().as_ref() != Some(&row.repo_root) { return Err("INVALID_PATH".into()); }
        if let Ok(manager) = self.registry.manager(workspace) {
            if manager.repo_root() == row.repo_root { return Ok(manager); }
        }
        let manager = WorktreeManager::try_new(&row.repo_root).map_err(|_| "INVALID_PATH")?;
        if manager.repo_root() != row.repo_root { return Err("INVALID_PATH".into()); }
        Ok(manager)
    }

    pub fn register(&self, workspace_id: &str, repo_root: &str) -> Result<(), String> {
        let workspace_gate = self.worktree_gate(workspace_id);
        let _workspace_gate = workspace_gate.lock();
        let _gate = self.mutation_gate.lock();
        let workspace_id = WorkspaceRegistry::validate_workspace_id(workspace_id).map_err(|e| e.to_string())?;
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
            if existing.repo_root != canonical { return Err("Workspace already registered".into()); }
            if existing.mirror_exposed && self.registry.contains(workspace_id) { return Ok(()); }
        }
        let mut project = crate::remote::workspace_api::registration_project(workspace_id, &canonical)?;
        candidate.workspaces.insert(workspace_id.to_string(), CatalogRow {
            repo_root: canonical, mirror_exposed: true, availability: Availability::Ready,
        });
        candidate.revision.0 = candidate.revision.0.checked_add(1).ok_or("Catalog revision exhausted")?;
        if let Err(error) = workspace_catalog::persist(&self.catalog_path, &candidate) {
            *state = Err(format!("Catalog mutations disabled after persistence failure: {error}"));
            return Err(error);
        }
        let revision = candidate.revision.0;
        *state = Ok(candidate);
        self.registry.publish(workspace_id.to_string(), manager);
        project.revision = crate::scoped_contracts::Epoch(revision);
        self.machine_events.publish_revision(revision, "projectRegistered", Some(workspace_id), None, serde_json::json!(project));
        Ok(())
    }

    pub fn catalog(&self) -> Result<Catalog, String> { self.catalog.lock().clone() }

    /// Blocking domain operation; async transports must use run_blocking.
    /// Machine identities adopt a canonical root, unlike compatibility IPC IDs.
    pub fn register_machine(&self, repo_root: &str) -> Result<String, String> {
        let _gate = self.mutation_gate.lock();
        if !PathBuf::from(repo_root).is_absolute() { return Err("Absolute root required".into()); }
        let manager = WorktreeManager::try_new(PathBuf::from(repo_root)).map_err(|e| e.to_string())?;
        let mut state = self.catalog.lock();
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        if let Some((id, _)) = candidate.workspaces.iter().find(|(_, row)| row.repo_root == manager.repo_root()) {
            return Ok(id.clone());
        }
        let id = format!("project-{}", uuid::Uuid::new_v4().simple());
        let mut project = crate::remote::workspace_api::registration_project(&id, manager.repo_root())?;
        candidate.workspaces.insert(id.clone(), CatalogRow {
            repo_root: manager.repo_root().to_owned(), mirror_exposed: false, availability: Availability::Ready,
        });
        candidate.revision.0 = candidate.revision.0.checked_add(1).ok_or("Catalog revision exhausted")?;
        if let Err(error) = workspace_catalog::persist(&self.catalog_path, &candidate) {
            *state = Err(format!("Catalog mutations disabled after persistence failure: {error}"));
            return Err(error);
        }
        let revision = candidate.revision.0;
        *state = Ok(candidate);
        project.revision = crate::scoped_contracts::Epoch(revision);
        self.machine_events.publish_revision(revision, "projectRegistered", Some(&id), None, serde_json::json!(project));
        Ok(id)
    }

    pub(crate) fn unregister(&self, workspace_id: &str) -> Result<(), String> {
        let workspace_gate = self.worktree_gate(workspace_id);
        let _workspace_gate = workspace_gate.lock();
        let _gate = self.mutation_gate.lock();
        let workspace_id = WorkspaceRegistry::validate_workspace_id(workspace_id).map_err(|e| e.to_string())?;
        let mut state = self.catalog.lock();
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        if candidate.workspaces.remove(workspace_id).is_none() { return Ok(()); }
        candidate.revision.0 = candidate.revision.0.checked_add(1).ok_or("Catalog revision exhausted")?;
        if let Err(error) = workspace_catalog::persist(&self.catalog_path, &candidate) {
            *state = Err(format!("Catalog mutations disabled after persistence failure: {error}"));
            return Err(error);
        }
        let revision = candidate.revision.0;
        *state = Ok(candidate);
        self.registry.unregister(workspace_id);
        self.machine_events.publish_revision(revision, "projectRemoved", Some(workspace_id), None, serde_json::json!({}));
        Ok(())
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    #[tokio::test]
    async fn unrelated_workspace_registration_progresses_while_workspace_fenced() {
        let (root, service) = crate::ipc::run_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            let service = std::sync::Arc::new(DaemonWorkspaceService::new(WorkspaceRegistry::new(), root.path().join("data/catalog")));
            fs::create_dir(root.path().join("other")).unwrap();
            Ok((root, service))
        }).await.unwrap();
        let fence = service.worktree_gate("busy");
        let held = fence.lock();
        assert!(service.worktree_gate("busy").try_lock().is_none());
        let other = root.path().join("other");
        let worker_service = service.clone();
        let mut worker = tokio::spawn(crate::ipc::run_blocking(move || {
            worker_service.register("other", other.to_str().unwrap()).unwrap();
            Ok(())
        }));
        let progress = tokio::time::timeout(std::time::Duration::from_secs(5), &mut worker).await;
        drop(held);
        if progress.is_err() { worker.await.unwrap().unwrap(); }
        progress.unwrap().unwrap().unwrap();
        assert!(service.registry.contains("other"));
        drop(service);
        crate::ipc::run_blocking(move || { root.close().unwrap(); Ok(()) }).await.unwrap();
    }
    #[test]
    fn post_rename_sync_failure_fences_publication() {
        let root = tempfile::tempdir().unwrap();
        let plain = root.path().join("plain");
        fs::create_dir(&plain).unwrap();
        let path = root.path().join("data/machine-workspaces.v1.json");
        let registry = WorkspaceRegistry::new();
        let service = DaemonWorkspaceService::new(registry.clone(), path.clone());
        service.register("existing", plain.to_str().unwrap()).unwrap();
        let revision = registry.revision();
        workspace_catalog::FAIL_DIRECTORY_SYNC.with(|fail| fail.set(true));
        let error = service.register("ambiguous", plain.to_str().unwrap()).unwrap_err();
        assert!(error.contains("post-rename"));
        assert!(!registry.contains("ambiguous"));
        assert_eq!(registry.revision(), revision);
        assert!(service.catalog().is_err());
        assert!(service.register("fenced", plain.to_str().unwrap()).is_err());
        let disk = workspace_catalog::load(&path).unwrap();
        assert!(disk.workspaces.contains_key("ambiguous"));
        assert!(!disk.workspaces.contains_key("fenced"));
        eprintln!("SYNC_FAILURE after rename: candidate on disk, no success/publication, mutation fenced");
        let receipt = root.path().to_owned();
        root.close().unwrap();
        eprintln!("SYNC_FAILURE CLEANUP root={} absent={}", receipt.display(), !receipt.exists());
    }
}
