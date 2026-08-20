use crate::worktree::git::{
    git_branch_delete, git_status_porcelain, git_worktree_add, git_worktree_list,
    git_worktree_prune, git_worktree_remove,
};
use crate::worktree::model::{
    CreateWorktreeOptions, DirtyState, OrcaWorktreeInfo, Worktree, WorktreeError,
};
use std::path::{Path, PathBuf};

/// Manages the atomic lifecycle of Git worktrees under the Orca workspace namespace (`orca/<ws-id>/<slug>`).
#[derive(Debug, Clone)]
pub struct WorktreeManager {
    repo_root: PathBuf,
}

impl WorktreeManager {
    /// Creates a new `WorktreeManager` rooted at the given repository path.
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }

    /// Returns the repository root path.
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// Validates workspace ID and slug, returning the formatted branch name (`orca/<ws-id>/<slug>`).
    pub fn format_branch_name(ws_id: &str, slug: &str) -> Result<String, WorktreeError> {
        let ws_id = ws_id.trim();
        let slug = slug.trim();

        if ws_id.is_empty() {
            return Err(WorktreeError::InvalidNamespace {
                reason: "Workspace ID cannot be empty".into(),
            });
        }
        if slug.is_empty() {
            return Err(WorktreeError::InvalidNamespace {
                reason: "Slug cannot be empty".into(),
            });
        }

        Self::validate_ref_component(ws_id, "workspace ID")?;
        Self::validate_ref_component(slug, "slug")?;

        Ok(format!("orca/{ws_id}/{slug}"))
    }

    /// Parses an Orca branch name (`orca/<ws-id>/<slug>` or `refs/heads/orca/<ws-id>/<slug>`) into its components.
    pub fn parse_branch_name(branch: &str) -> Option<OrcaWorktreeInfo> {
        let short = branch.strip_prefix("refs/heads/").unwrap_or(branch);
        let parts: Vec<&str> = short.split('/').collect();
        if parts.len() >= 3 && parts[0] == "orca" {
            Some(OrcaWorktreeInfo {
                ws_id: parts[1].to_string(),
                slug: parts[2..].join("/"),
            })
        } else {
            None
        }
    }

    /// Helper to validate git ref component characters.
    fn validate_ref_component(component: &str, label: &str) -> Result<(), WorktreeError> {
        if component.starts_with('/')
            || component.ends_with('/')
            || component.starts_with('.')
            || component.ends_with('.')
        {
            return Err(WorktreeError::InvalidNamespace {
                reason: format!("{label} cannot start or end with '/' or '.'"),
            });
        }

        if component.contains("..") || component.contains("//") || component.contains("@{") {
            return Err(WorktreeError::InvalidNamespace {
                reason: format!("{label} cannot contain '..', '//', or '@{{'"),
            });
        }

        for c in component.chars() {
            if c.is_ascii_control()
                || c.is_whitespace()
                || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
            {
                return Err(WorktreeError::InvalidNamespace {
                    reason: format!("{label} contains invalid character '{c}'"),
                });
            }
        }

        if component.ends_with(".lock") {
            return Err(WorktreeError::InvalidNamespace {
                reason: format!("{label} cannot end with '.lock'"),
            });
        }

        Ok(())
    }

    /// Creates a new worktree with isolated branch `orca/<ws-id>/<slug>`.
    pub fn create_worktree(
        &self,
        options: CreateWorktreeOptions,
    ) -> Result<Worktree, WorktreeError> {
        let branch_name = Self::format_branch_name(&options.ws_id, &options.slug)?;

        // Ensure target directory doesn't already exist or conflict
        if options.path.exists() {
            return Err(WorktreeError::WorktreeAlreadyExists { path: options.path });
        }

        // Add the git worktree
        git_worktree_add(
            &self.repo_root,
            &options.path,
            &branch_name,
            options.base_ref.as_deref(),
        )?;

        // Retrieve created worktree info from list
        let worktrees = self.list_worktrees()?;
        let target_canonical = options.path.canonicalize().unwrap_or_else(|_| options.path.clone());

        for wt in worktrees {
            let wt_canonical = wt.path.canonicalize().unwrap_or_else(|_| wt.path.clone());
            if wt_canonical == target_canonical || wt.path == options.path {
                return Ok(wt);
            }
        }

        // Fallback if not found in list for some reason
        Ok(Worktree {
            path: options.path,
            head: String::new(),
            branch: Some(format!("refs/heads/{branch_name}")),
            bare: false,
            detached: false,
            locked: None,
            prunable: None,
        })
    }

    /// Lists all worktrees in the repository.
    pub fn list_worktrees(&self) -> Result<Vec<Worktree>, WorktreeError> {
        git_worktree_list(&self.repo_root)
    }

    /// Finds a worktree matching the specified path.
    pub fn find_worktree(&self, path: &Path) -> Result<Option<Worktree>, WorktreeError> {
        let target_canonical = path.canonicalize().ok();
        let worktrees = self.list_worktrees()?;

        for wt in worktrees {
            if wt.path == path {
                return Ok(Some(wt));
            }
            if let (Some(tc), Ok(wt_c)) = (&target_canonical, wt.path.canonicalize()) {
                if tc == &wt_c {
                    return Ok(Some(wt));
                }
            }
        }

        Ok(None)
    }

    /// Finds a worktree by workspace ID and slug.
    pub fn find_worktree_by_slug(
        &self,
        ws_id: &str,
        slug: &str,
    ) -> Result<Option<Worktree>, WorktreeError> {
        let target_branch = Self::format_branch_name(ws_id, slug)?;
        let worktrees = self.list_worktrees()?;

        for wt in worktrees {
            if let Some(branch) = wt.branch_short_name() {
                if branch == target_branch {
                    return Ok(Some(wt));
                }
            }
        }

        Ok(None)
    }

    /// Checks the dirty status of a worktree using `git status --porcelain`.
    pub fn check_dirty(&self, worktree_path: &Path) -> Result<DirtyState, WorktreeError> {
        if !worktree_path.exists() {
            return Err(WorktreeError::WorktreeNotFound {
                path: worktree_path.to_path_buf(),
            });
        }
        git_status_porcelain(worktree_path)
    }

    /// Returns `true` if the worktree has any untracked, modified, or uncommitted files.
    pub fn is_dirty(&self, worktree_path: &Path) -> Result<bool, WorktreeError> {
        self.check_dirty(worktree_path).map(|state| state.is_dirty)
    }

    /// Safely removes a worktree.
    ///
    /// IMPORTANT: Deletion is strictly forbidden on dirty worktrees to guarantee
    /// 1-writer-1-worktree isolation. Even if `force: true` is passed, a dirty worktree
    /// will return `WorktreeError::DirtyWorktree` and NOT be removed.
    pub fn remove_worktree(&self, worktree_path: &Path, force: bool) -> Result<(), WorktreeError> {
        let dirty_state = self.check_dirty(worktree_path)?;
        if dirty_state.is_dirty {
            let count = dirty_state.files.len();
            let files = dirty_state.files.into_iter().map(|f| f.path).collect();
            return Err(WorktreeError::DirtyWorktree {
                path: worktree_path.to_path_buf(),
                count,
                files,
            });
        }

        git_worktree_remove(&self.repo_root, worktree_path, force)?;
        let _ = git_worktree_prune(&self.repo_root);
        Ok(())
    }

    /// Deletes a clean worktree safely (alias for `remove_worktree(path, false)`).
    pub fn safe_delete(&self, worktree_path: &Path) -> Result<(), WorktreeError> {
        self.remove_worktree(worktree_path, false)
    }

    /// Safely removes a worktree and optionally deletes its underlying branch.
    pub fn delete_worktree_and_branch(
        &self,
        worktree_path: &Path,
        delete_branch: bool,
    ) -> Result<(), WorktreeError> {
        // Find existing worktree to extract branch name before removal
        let existing = self.find_worktree(worktree_path)?;
        let branch_name = existing.and_then(|wt| wt.branch_short_name().map(|s| s.to_string()));

        // Perform safe removal (will fail if dirty)
        self.remove_worktree(worktree_path, false)?;

        if delete_branch {
            if let Some(branch) = branch_name {
                git_branch_delete(&self.repo_root, &branch, true)?;
            }
        }

        Ok(())
    }
}
