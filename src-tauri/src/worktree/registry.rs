use crate::worktree::{Worktree, WorktreeError, WorktreeIdentity, WorktreeManager};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct WorkspaceRegistry {
    workspaces: Arc<RwLock<HashMap<String, WorktreeManager>>>,
}

impl WorkspaceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn validate_workspace_id(workspace_id: &str) -> Result<&str, WorktreeError> {
        let workspace_id = workspace_id.trim();
        if workspace_id.is_empty() {
            return Err(WorktreeError::InvalidNamespace {
                reason: "Workspace registry ID cannot be empty".into(),
            });
        }
        if workspace_id.starts_with('-')
            || workspace_id.contains('/')
            || workspace_id.contains('\\')
            || workspace_id.chars().any(|ch| ch.is_control() || ch.is_whitespace())
        {
            return Err(WorktreeError::InvalidNamespace {
                reason: "Workspace registry ID contains unsafe characters".into(),
            });
        }
        Ok(workspace_id)
    }

    pub fn register(
        &self,
        workspace_id: impl AsRef<str>,
        repo_root: impl AsRef<Path>,
    ) -> Result<(), WorktreeError> {
        let workspace_id = Self::validate_workspace_id(workspace_id.as_ref())?.to_string();
        let manager = WorktreeManager::try_new(repo_root.as_ref().to_path_buf())?;
        let mut workspaces = self.workspaces.write();
        if let Some(existing) = workspaces.get(&workspace_id) {
            if existing.repo_root() == manager.repo_root() {
                return Ok(());
            }
            return Err(WorktreeError::WorkspaceAlreadyRegistered { workspace_id });
        }
        workspaces.insert(workspace_id, manager);
        Ok(())
    }

    pub fn contains(&self, workspace_id: &str) -> bool {
        self.workspaces.read().contains_key(workspace_id)
    }

    pub fn list(&self) -> Vec<(String, WorktreeManager)> {
        self.workspaces
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn manager(&self, workspace_id: &str) -> Result<WorktreeManager, WorktreeError> {
        let workspace_id = Self::validate_workspace_id(workspace_id)?;
        self.workspaces
            .read()
            .get(workspace_id)
            .cloned()
            .ok_or_else(|| WorktreeError::WorkspaceNotFound {
                workspace_id: workspace_id.to_string(),
            })
    }

    pub fn repo_root(&self, workspace_id: &str) -> Result<PathBuf, WorktreeError> {
        Ok(self.manager(workspace_id)?.repo_root().to_path_buf())
    }

    pub fn target_path(
        &self,
        workspace_id: &str,
        identity: &WorktreeIdentity,
    ) -> Result<PathBuf, WorktreeError> {
        self.manager(workspace_id)?
            .worktree_path_for(&identity.ws_id, &identity.slug)
    }

    pub fn resolve_worktree(
        &self,
        workspace_id: &str,
        identity: &WorktreeIdentity,
    ) -> Result<(WorktreeManager, Worktree), WorktreeError> {
        let manager = self.manager(workspace_id)?;
        let worktree = manager
            .find_worktree_by_slug(&identity.ws_id, &identity.slug)?
            .ok_or_else(|| WorktreeError::WorktreeIdentityNotFound {
                workspace_id: workspace_id.to_string(),
                ws_id: identity.ws_id.clone(),
                slug: identity.slug.clone(),
            })?;
        manager.canonical_allowed_path(&worktree.path)?;
        Ok((manager, worktree))
    }

    pub fn resolve_terminal_target(
        &self,
        workspace_id: &str,
        identity: Option<&WorktreeIdentity>,
    ) -> Result<(WorktreeManager, PathBuf), WorktreeError> {
        if let Some(identity) = identity {
            let (manager, worktree) = self.resolve_worktree(workspace_id, identity)?;
            let path = manager.canonical_allowed_path(&worktree.path)?;
            return Ok((manager, path));
        }

        let manager = self.manager(workspace_id)?;
        let path = manager.repo_root().to_path_buf();
        Ok((manager, path))
    }
}
