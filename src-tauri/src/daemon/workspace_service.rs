use crate::worktree::{WorkspaceRegistry, WorktreeManager};
use std::{fs, path::PathBuf};

/// Shared admission gate for daemon registry mutations and validated PTY spawn.
pub struct DaemonWorkspaceService {
    pub(crate) registry: WorkspaceRegistry,
    pub(crate) mutation_gate: parking_lot::Mutex<()>,
}

impl DaemonWorkspaceService {
    pub(crate) fn new(registry: WorkspaceRegistry) -> Self {
        Self {
            registry,
            mutation_gate: parking_lot::Mutex::new(()),
        }
    }
    pub fn register(&self, workspace_id: &str, repo_root: &str) -> Result<(), String> {
        let _gate = self.mutation_gate.lock();
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
        self.registry
            .register(workspace_id, &canonical)
            .map_err(|e| e.to_string())
    }

    pub(crate) fn unregister(&self, workspace_id: &str) {
        let _gate = self.mutation_gate.lock();
        self.registry.unregister(workspace_id);
    }
}
