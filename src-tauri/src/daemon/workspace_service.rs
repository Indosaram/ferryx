use crate::worktree::{WorkspaceRegistry, WorktreeManager};
use std::{fs, path::PathBuf};
use crate::remote::{machine_protocol::Availability, workspace_catalog::{self, Catalog, CatalogRow}};

/// Shared admission gate for daemon registry mutations and validated PTY spawn.
pub struct DaemonWorkspaceService {
    pub(crate) registry: WorkspaceRegistry,
    pub(crate) mutation_gate: parking_lot::Mutex<()>,
    catalog: parking_lot::Mutex<Result<Catalog, String>>,
    catalog_path: PathBuf,
}

impl DaemonWorkspaceService {
    pub(crate) fn new(registry: WorkspaceRegistry, catalog_path: PathBuf) -> Self {
        let mut catalog = workspace_catalog::load(&catalog_path);
        if let Ok(catalog) = &mut catalog {
            for (id, row) in &mut catalog.workspaces {
                row.availability = match fs::metadata(&row.repo_root) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Availability::Missing,
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Availability::PermissionDenied,
                    Err(_) => Availability::Invalid,
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
            registry,
            mutation_gate: parking_lot::Mutex::new(()),
            catalog: parking_lot::Mutex::new(catalog),
            catalog_path,
        }
    }
    pub fn register(&self, workspace_id: &str, repo_root: &str) -> Result<(), String> {
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
        candidate.workspaces.insert(workspace_id.to_string(), CatalogRow {
            repo_root: canonical, mirror_exposed: true, availability: Availability::Ready,
        });
        candidate.revision.0 = candidate.revision.0.checked_add(1).ok_or("Catalog revision exhausted")?;
        if let Err(error) = workspace_catalog::persist(&self.catalog_path, &candidate) {
            *state = Err(format!("Catalog mutations disabled after persistence failure: {error}"));
            return Err(error);
        }
        *state = Ok(candidate);
        self.registry.publish(workspace_id.to_string(), manager);
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
        candidate.workspaces.insert(id.clone(), CatalogRow {
            repo_root: manager.repo_root().to_owned(), mirror_exposed: false, availability: Availability::Ready,
        });
        candidate.revision.0 = candidate.revision.0.checked_add(1).ok_or("Catalog revision exhausted")?;
        if let Err(error) = workspace_catalog::persist(&self.catalog_path, &candidate) {
            *state = Err(format!("Catalog mutations disabled after persistence failure: {error}"));
            return Err(error);
        }
        *state = Ok(candidate);
        Ok(id)
    }

    pub(crate) fn unregister(&self, workspace_id: &str) -> Result<(), String> {
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
        *state = Ok(candidate);
        self.registry.unregister(workspace_id);
        Ok(())
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
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
