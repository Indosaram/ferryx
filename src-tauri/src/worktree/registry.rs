use crate::worktree::{Worktree, WorktreeError, WorktreeIdentity, WorktreeManager};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct WorkspaceRegistry {
    workspaces: Arc<RwLock<HashMap<String, WorktreeManager>>>,
    revision: Arc<std::sync::atomic::AtomicU64>,
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
            || workspace_id
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace())
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
            // If the existing entry was a bogus filesystem root, replace it.
            if existing.repo_root().parent().is_none() {
                workspaces.insert(workspace_id, manager);
                self.revision
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                return Ok(());
            }
            return Err(WorktreeError::WorkspaceAlreadyRegistered { workspace_id });
        }
        workspaces.insert(workspace_id, manager);
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Ok(())
    }

    /// Binds `workspace_id` to `repo_root` only if no workspace owns that
    /// canonical root yet, returning the existing owner otherwise. Find and
    /// insert happen under one write lock so concurrent registrations cannot
    /// bind two IDs to the same repository.
    pub fn register_unique_root(
        &self,
        workspace_id: impl AsRef<str>,
        repo_root: impl AsRef<Path>,
    ) -> Result<String, WorktreeError> {
        let workspace_id = Self::validate_workspace_id(workspace_id.as_ref())?.to_string();
        let manager = WorktreeManager::try_new(repo_root.as_ref().to_path_buf())?;
        let mut workspaces = self.workspaces.write();
        if let Some((owner, _)) = workspaces
            .iter()
            .find(|(_, candidate)| candidate.repo_root() == manager.repo_root())
        {
            return Ok(owner.clone());
        }
        // If the existing entry is a filesystem root (bogus startup artifact), replace it under this ID.
        if let Some(existing) = workspaces.get(&workspace_id) {
            if existing.repo_root().parent().is_none() {
                workspaces.insert(workspace_id.clone(), manager);
                self.revision
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                return Ok(workspace_id);
            }
        }
        // If the requested ID is occupied by a different repository, resolve to a unique slug.
        let mut resolved_id = workspace_id.clone();
        let mut suffix = 2;
        while let Some(existing) = workspaces.get(&resolved_id) {
            if existing.repo_root() == manager.repo_root() {
                return Ok(resolved_id);
            }
            if existing.repo_root().parent().is_none() {
                break;
            }
            resolved_id = format!("{workspace_id}-{suffix}");
            suffix += 1;
        }
        workspaces.insert(resolved_id.clone(), manager);
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Ok(resolved_id)
    }

    pub fn contains(&self, workspace_id: &str) -> bool {
        self.workspaces.read().contains_key(workspace_id)
    }

    pub fn revision(&self) -> u64 {
        let registry_rev = self.revision.load(std::sync::atomic::Ordering::Acquire);
        let mgr_revs: u64 = self.workspaces.read().values().map(|m| m.revision()).sum();
        registry_rev.wrapping_add(mgr_revs)
    }

    pub fn bump_revision(&self) -> u64 {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            + 1
    }

    pub fn list(&self) -> Vec<(String, WorktreeManager)> {
        self.workspaces
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn unregister(&self, workspace_id: &str) -> bool {
        let Ok(workspace_id) = Self::validate_workspace_id(workspace_id) else {
            return false;
        };
        let mut workspaces = self.workspaces.write();
        let removed = workspaces.remove(workspace_id).is_some();
        if removed {
            self.revision
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        removed
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
        let expected_branch = WorktreeManager::format_branch_name(&identity.ws_id, &identity.slug)?;
        if worktree.branch_short_name() != Some(expected_branch.as_str()) {
            return Err(WorktreeError::WorktreeIdentityNotFound {
                workspace_id: workspace_id.to_string(),
                ws_id: identity.ws_id.clone(),
                slug: identity.slug.clone(),
            });
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn plain_workspace(id: &str) -> (TempDir, WorkspaceRegistry) {
        let dir = TempDir::new().expect("tempdir");
        let registry = WorkspaceRegistry::new();
        registry
            .register(id, dir.path())
            .expect("register plain workspace");
        (dir, registry)
    }

    #[test]
    fn unregister_removes_registered_workspace_and_bumps_revision() {
        let (_dir, registry) = plain_workspace("ws-a");
        assert!(registry.contains("ws-a"));
        let revision_before = registry.revision();

        assert!(registry.unregister("ws-a"));

        assert!(!registry.contains("ws-a"));
        assert!(registry.list().is_empty());
        assert!(registry.revision() > revision_before);
    }

    #[test]
    fn unregister_unknown_workspace_returns_false_without_revision_change() {
        let registry = WorkspaceRegistry::new();
        let revision_before = registry.revision();

        assert!(!registry.unregister("never-registered"));

        assert_eq!(registry.revision(), revision_before);
    }

    #[test]
    fn unregister_rejects_unsafe_workspace_id() {
        let (_dir, registry) = plain_workspace("ws-b");
        let revision_before = registry.revision();

        assert!(!registry.unregister("../escape"));
        assert!(!registry.unregister(""));

        assert!(registry.contains("ws-b"));
        assert_eq!(registry.revision(), revision_before);
    }

    #[test]
    fn unregister_is_idempotent_on_the_same_id() {
        let (_dir, registry) = plain_workspace("ws-c");

        assert!(registry.unregister("ws-c"));
        assert!(!registry.unregister("ws-c"));
    }

    #[test]
    fn register_unique_root_disambiguates_conflicting_ids() {
        let dir1 = TempDir::new().expect("tempdir1");
        let dir2 = TempDir::new().expect("tempdir2");
        let registry = WorkspaceRegistry::new();

        let id1 = registry
            .register_unique_root("project", dir1.path())
            .expect("first project registers under project");
        assert_eq!(id1, "project");

        let id2 = registry
            .register_unique_root("project", dir2.path())
            .expect("second distinct root with same requested id disambiguates cleanly");
        assert_eq!(id2, "project-2");
        assert!(registry.contains("project"));
        assert!(registry.contains("project-2"));
    }

    #[test]
    fn register_unique_root_replaces_bogus_root_entry() {
        let dir = TempDir::new().expect("tempdir");
        let registry = WorkspaceRegistry::new();

        // Simulate a stale/bogus entry whose repo_root is a filesystem root
        {
            let mut workspaces = registry.workspaces.write();
            // Create a manager-like entry with a root path
            let root_manager = WorktreeManager::for_test_root(PathBuf::from("/"));
            workspaces.insert("project".to_string(), root_manager);
        }

        let registered = registry
            .register_unique_root("project", dir.path())
            .expect("replaces bogus root registration instead of failing");
        assert_eq!(registered, "project");
        assert_ne!(
            registry.repo_root("project").unwrap(),
            PathBuf::from("/")
        );
    }

    #[test]
    fn register_replaces_bogus_root_entry() {
        let dir = TempDir::new().expect("tempdir");
        let registry = WorkspaceRegistry::new();

        {
            let mut workspaces = registry.workspaces.write();
            let root_manager = WorktreeManager::for_test_root(PathBuf::from("/"));
            workspaces.insert("project".to_string(), root_manager);
        }

        assert!(registry.register("project", dir.path()).is_ok());
        assert_ne!(
            registry.repo_root("project").unwrap(),
            PathBuf::from("/")
        );
    }
}
