use crate::worktree::git::{
    git_branch_delete, git_status_porcelain, git_worktree_add, git_worktree_list,
    git_worktree_prune, git_worktree_remove, run_git,
};
use crate::worktree::model::{
    BranchDeletionPreview, CreateWorktreeOptions, DirtyState, OrcaWorktreeInfo, Worktree,
    WorktreeError,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct WriterLeaseRegistry {
    writers: Arc<Mutex<HashMap<PathBuf, String>>>,
}

impl WriterLeaseRegistry {
    fn acquire_canonical(&self, path: PathBuf, owner_id: &str) -> Result<(), WorktreeError> {
        let mut writers = self.writers.lock();
        if let Some(existing_owner) = writers.get(&path) {
            return Err(WorktreeError::WriterAlreadyActive {
                path,
                owner_id: existing_owner.clone(),
            });
        }
        writers.insert(path, owner_id.to_string());
        Ok(())
    }

    fn release_canonical(&self, path: &Path, owner_id: &str) -> Result<(), WorktreeError> {
        let mut writers = self.writers.lock();
        let Some(existing_owner) = writers.get(path) else {
            return Ok(());
        };
        if existing_owner != owner_id {
            return Err(WorktreeError::WriterLeaseOwnerMismatch {
                path: path.to_path_buf(),
                owner_id: existing_owner.clone(),
                requested_owner_id: owner_id.to_string(),
            });
        }
        writers.remove(path);
        Ok(())
    }

    fn owner_canonical(&self, path: &Path) -> Option<String> {
        self.writers.lock().get(path).cloned()
    }
}

#[derive(Debug)]
pub(crate) struct WriterLeaseGuard {
    registry: WriterLeaseRegistry,
    canonical_path: PathBuf,
    owner_id: String,
}

impl WriterLeaseGuard {
    fn new(registry: WriterLeaseRegistry, canonical_path: PathBuf, owner_id: String) -> Self {
        Self {
            registry,
            canonical_path,
            owner_id,
        }
    }

    pub(crate) fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    pub(crate) fn owner_id(&self) -> &str {
        &self.owner_id
    }
}

impl Drop for WriterLeaseGuard {
    fn drop(&mut self) {
        let _ = self
            .registry
            .release_canonical(&self.canonical_path, &self.owner_id);
    }
}

/// Manages the atomic lifecycle of Git worktrees under the Orca workspace namespace (`orca/<ws-id>/<slug>`).
#[derive(Debug, Clone)]
pub struct WorktreeManager {
    repo_root: PathBuf,
    writer_leases: WriterLeaseRegistry,
    delete_lock: Arc<Mutex<()>>,
}

impl WorktreeManager {
    /// Creates a new `WorktreeManager` rooted at the given repository path.
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
            writer_leases: WriterLeaseRegistry::default(),
            delete_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Returns the repository root path.
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    fn canonical_worktree_path(&self, path: &Path) -> Result<PathBuf, WorktreeError> {
        path.canonicalize()
            .map_err(|_| WorktreeError::WorktreeNotFound {
                path: path.to_path_buf(),
            })
    }

    pub fn acquire_writer(&self, worktree_path: &Path, owner_id: &str) -> Result<(), WorktreeError> {
        let _delete_guard = self.delete_lock.lock();
        let canonical = self.canonical_worktree_path(worktree_path)?;
        self.writer_leases.acquire_canonical(canonical, owner_id)
    }

    pub fn release_writer(&self, worktree_path: &Path, owner_id: &str) -> Result<(), WorktreeError> {
        let canonical = self.canonical_worktree_path(worktree_path)?;
        self.writer_leases.release_canonical(&canonical, owner_id)
    }

    pub fn writer_owner(&self, worktree_path: &Path) -> Result<Option<String>, WorktreeError> {
        let canonical = self.canonical_worktree_path(worktree_path)?;
        Ok(self.writer_leases.owner_canonical(&canonical))
    }

    pub(crate) fn acquire_writer_lease(
        &self,
        worktree_path: &Path,
        owner_id: &str,
    ) -> Result<WriterLeaseGuard, WorktreeError> {
        let _delete_guard = self.delete_lock.lock();
        let canonical = self.canonical_worktree_path(worktree_path)?;
        self.writer_leases
            .acquire_canonical(canonical.clone(), owner_id)?;
        Ok(WriterLeaseGuard::new(
            self.writer_leases.clone(),
            canonical,
            owner_id.to_string(),
        ))
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

        if options.path.exists() {
            return Err(WorktreeError::WorktreeAlreadyExists { path: options.path });
        }

        git_worktree_add(
            &self.repo_root,
            &options.path,
            &branch_name,
            options.base_ref.as_deref(),
        )?;

        let worktrees = self.list_worktrees()?;
        let target_canonical = options
            .path
            .canonicalize()
            .unwrap_or_else(|_| options.path.clone());

        for wt in worktrees {
            let wt_canonical = wt.path.canonicalize().unwrap_or_else(|_| wt.path.clone());
            if wt_canonical == target_canonical || wt.path == options.path {
                return Ok(wt);
            }
        }

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

    fn ensure_no_writer(&self, worktree_path: &Path) -> Result<(), WorktreeError> {
        let canonical = self.canonical_worktree_path(worktree_path)?;
        if let Some(owner_id) = self.writer_leases.owner_canonical(&canonical) {
            return Err(WorktreeError::WriterAlreadyActive {
                path: canonical,
                owner_id,
            });
        }
        Ok(())
    }

    fn remove_worktree_locked(
        &self,
        worktree_path: &Path,
        force: bool,
    ) -> Result<(), WorktreeError> {
        self.ensure_no_writer(worktree_path)?;
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

    /// Safely removes a worktree while serializing active-writer and dirty checks with removal.
    pub fn remove_worktree(&self, worktree_path: &Path, force: bool) -> Result<(), WorktreeError> {
        let _delete_guard = self.delete_lock.lock();
        self.remove_worktree_locked(worktree_path, force)
    }

    /// Deletes a clean worktree safely (alias for `remove_worktree(path, false)`).
    pub fn safe_delete(&self, worktree_path: &Path) -> Result<(), WorktreeError> {
        self.remove_worktree(worktree_path, false)
    }

    fn branch_is_merged(&self, branch: &str) -> Result<bool, WorktreeError> {
        let merged = run_git(
            &self.repo_root,
            &["branch", "--merged", "HEAD", "--format=%(refname:short)"],
        )?;
        Ok(merged.lines().any(|line| line.trim() == branch))
    }

    pub fn branch_deletion_preview(
        &self,
        worktree_path: &Path,
    ) -> Result<BranchDeletionPreview, WorktreeError> {
        let existing = self
            .find_worktree(worktree_path)?
            .ok_or_else(|| WorktreeError::WorktreeNotFound {
                path: worktree_path.to_path_buf(),
            })?;
        let branch = existing
            .branch_short_name()
            .ok_or_else(|| WorktreeError::ParseError("Detached worktree has no branch".into()))?
            .to_string();
        let head = if existing.head.is_empty() {
            run_git(&self.repo_root, &["rev-parse", "--verify", &branch])?
                .trim()
                .to_string()
        } else {
            existing.head
        };
        let merged = self.branch_is_merged(&branch)?;
        let upstream = run_git(
            &self.repo_root,
            &["rev-parse", "--abbrev-ref", &format!("{branch}@{{upstream}}")],
        )
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

        let (ahead, behind) = if let Some(upstream_ref) = upstream.as_deref() {
            let counts = run_git(
                &self.repo_root,
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("{upstream_ref}...{branch}"),
                ],
            )?;
            let mut values = counts.split_whitespace();
            let behind = values.next().and_then(|value| value.parse::<u64>().ok());
            let ahead = values.next().and_then(|value| value.parse::<u64>().ok());
            (ahead, behind)
        } else {
            (None, None)
        };

        Ok(BranchDeletionPreview {
            branch,
            head,
            upstream,
            merged,
            ahead,
            behind,
        })
    }

    /// Safely removes a worktree and optionally deletes its branch using `git branch -d`.
    pub fn delete_worktree_and_branch(
        &self,
        worktree_path: &Path,
        delete_branch: bool,
    ) -> Result<(), WorktreeError> {
        let _delete_guard = self.delete_lock.lock();
        self.ensure_no_writer(worktree_path)?;

        let branch_preview = if delete_branch {
            let preview = self.branch_deletion_preview(worktree_path)?;
            if !preview.merged {
                return Err(WorktreeError::UnmergedBranch {
                    branch: preview.branch,
                    head: preview.head,
                });
            }
            Some(preview)
        } else {
            None
        };

        self.remove_worktree_locked(worktree_path, false)?;
        if let Some(preview) = branch_preview {
            git_branch_delete(&self.repo_root, &preview.branch, false)?;
        }
        Ok(())
    }

    /// Explicit destructive branch deletion. Dirty worktrees and active writers remain protected.
    pub fn delete_worktree_and_branch_destructive(
        &self,
        worktree_path: &Path,
        delete_branch: bool,
    ) -> Result<(), WorktreeError> {
        let _delete_guard = self.delete_lock.lock();
        self.ensure_no_writer(worktree_path)?;
        let branch = if delete_branch {
            Some(self.branch_deletion_preview(worktree_path)?.branch)
        } else {
            None
        };

        self.remove_worktree_locked(worktree_path, false)?;
        if let Some(branch) = branch {
            git_branch_delete(&self.repo_root, &branch, true)?;
        }
        Ok(())
    }
}
