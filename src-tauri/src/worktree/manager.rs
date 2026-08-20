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
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

const ORCA_WORKTREE_DIR: &str = ".orca-worktrees";

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

/// Manages Git worktrees under a canonical repository root.
///
/// Every filesystem path accepted by this manager must stay inside `repo_root` after
/// canonicalization. IPC callers should prefer identity-based resolution through
/// `WorkspaceRegistry` instead of passing paths directly.
#[derive(Debug, Clone)]
pub struct WorktreeManager {
    repo_root: PathBuf,
    writer_leases: WriterLeaseRegistry,
    delete_lock: Arc<Mutex<()>>,
}

impl WorktreeManager {
    /// Creates a manager and immediately validates/canonicalizes the Git repository root.
    /// Invalid roots are programmer configuration errors; fallible callers should use `try_new`.
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        let requested = repo_root.into();
        Self::try_new(&requested)
            .unwrap_or_else(|error| panic!("invalid worktree manager root '{}': {error}", requested.display()))
    }

    pub fn try_new(repo_root: impl Into<PathBuf>) -> Result<Self, WorktreeError> {
        let requested = repo_root.into();
        let canonical = fs::canonicalize(&requested)
            .map_err(|_| WorktreeError::InvalidRepoRoot {
                path: requested.clone(),
            })?;
        if !canonical.is_dir() {
            return Err(WorktreeError::InvalidRepoRoot { path: canonical });
        }

        let top_level = run_git(&canonical, &["rev-parse", "--show-toplevel"])
            .map_err(|_| WorktreeError::InvalidRepoRoot {
                path: canonical.clone(),
            })?;
        let git_root = fs::canonicalize(PathBuf::from(top_level.trim())).map_err(|_| {
            WorktreeError::InvalidRepoRoot {
                path: canonical.clone(),
            }
        })?;
        if git_root != canonical {
            return Err(WorktreeError::InvalidRepoRoot { path: canonical });
        }

        Ok(Self {
            repo_root: git_root,
            writer_leases: WriterLeaseRegistry::default(),
            delete_lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    fn validate_path_components(&self, path: &Path) -> Result<(), WorktreeError> {
        if !path.is_absolute() {
            return Err(WorktreeError::InvalidPath {
                path: path.to_path_buf(),
                reason: "path must be absolute".into(),
            });
        }
        if path.components().any(|component| matches!(component, Component::ParentDir)) {
            return Err(WorktreeError::InvalidPath {
                path: path.to_path_buf(),
                reason: "parent traversal ('..') is forbidden".into(),
            });
        }
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            if name.starts_with('-') {
                return Err(WorktreeError::InvalidPath {
                    path: path.to_path_buf(),
                    reason: "path component cannot start with a dash".into(),
                });
            }
            if name.chars().any(char::is_control) {
                return Err(WorktreeError::InvalidPath {
                    path: path.to_path_buf(),
                    reason: "path component cannot contain control characters".into(),
                });
            }
        }
        Ok(())
    }

    fn ensure_canonical_inside_root(
        &self,
        original: &Path,
        canonical: PathBuf,
    ) -> Result<PathBuf, WorktreeError> {
        if canonical == self.repo_root || canonical.starts_with(&self.repo_root) {
            Ok(canonical)
        } else {
            Err(WorktreeError::PathOutsideWorkspace {
                path: original.to_path_buf(),
                root: self.repo_root.clone(),
            })
        }
    }

    pub fn canonical_allowed_path(&self, path: &Path) -> Result<PathBuf, WorktreeError> {
        self.validate_path_components(path)?;
        let canonical = fs::canonicalize(path).map_err(|_| WorktreeError::WorktreeNotFound {
            path: path.to_path_buf(),
        })?;
        self.ensure_canonical_inside_root(path, canonical)
    }

    fn validate_new_worktree_path(&self, path: &Path) -> Result<(), WorktreeError> {
        self.validate_path_components(path)?;
        if !path.starts_with(&self.repo_root) {
            return Err(WorktreeError::PathOutsideWorkspace {
                path: path.to_path_buf(),
                root: self.repo_root.clone(),
            });
        }

        let mut ancestor = path.parent().ok_or_else(|| WorktreeError::InvalidPath {
            path: path.to_path_buf(),
            reason: "worktree target has no parent".into(),
        })?;
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or_else(|| WorktreeError::InvalidPath {
                path: path.to_path_buf(),
                reason: "worktree target has no existing ancestor".into(),
            })?;
        }
        let canonical_ancestor = fs::canonicalize(ancestor)?;
        self.ensure_canonical_inside_root(path, canonical_ancestor)?;
        Ok(())
    }

    fn canonical_worktree_path(&self, path: &Path) -> Result<PathBuf, WorktreeError> {
        self.canonical_allowed_path(path)
    }

    pub fn worktree_path_for(&self, ws_id: &str, slug: &str) -> Result<PathBuf, WorktreeError> {
        Self::format_branch_name(ws_id, slug)?;
        let mut target = self.repo_root.join(ORCA_WORKTREE_DIR).join(ws_id);
        for segment in slug.split('/') {
            target.push(segment);
        }
        self.validate_new_worktree_path(&target)?;
        Ok(target)
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
        if ws_id.contains('/') {
            return Err(WorktreeError::InvalidNamespace {
                reason: "Workspace ID cannot contain '/'".into(),
            });
        }
        Self::validate_ref_component(ws_id, "workspace ID")?;
        Self::validate_ref_component(slug, "slug")?;
        Ok(format!("orca/{ws_id}/{slug}"))
    }

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

    fn validate_ref_component(component: &str, label: &str) -> Result<(), WorktreeError> {
        if component.contains("..") || component.contains("//") || component.contains("@{") {
            return Err(WorktreeError::InvalidNamespace {
                reason: format!("{label} cannot contain '..', '//', or '@{{'"),
            });
        }

        for segment in component.split('/') {
            if segment.is_empty()
                || segment.starts_with('.')
                || segment.ends_with('.')
                || segment.starts_with('-')
            {
                return Err(WorktreeError::InvalidNamespace {
                    reason: format!(
                        "{label} path segments cannot be empty or start/end with '.', or start with '-'"
                    ),
                });
            }
            if segment.ends_with(".lock") {
                return Err(WorktreeError::InvalidNamespace {
                    reason: format!("{label} cannot end with '.lock'"),
                });
            }
            for ch in segment.chars() {
                if ch.is_ascii_control()
                    || ch.is_whitespace()
                    || matches!(ch, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
                {
                    return Err(WorktreeError::InvalidNamespace {
                        reason: format!("{label} contains invalid character '{ch}'"),
                    });
                }
            }
        }
        Ok(())
    }

    pub fn create_worktree(
        &self,
        options: CreateWorktreeOptions,
    ) -> Result<Worktree, WorktreeError> {
        let branch_name = Self::format_branch_name(&options.ws_id, &options.slug)?;
        self.validate_new_worktree_path(&options.path)?;
        if options.path.exists() {
            self.canonical_allowed_path(&options.path)?;
            return Err(WorktreeError::WorktreeAlreadyExists { path: options.path });
        }

        let parent = options
            .path
            .parent()
            .ok_or_else(|| WorktreeError::InvalidPath {
                path: options.path.clone(),
                reason: "worktree target has no parent".into(),
            })?;
        fs::create_dir_all(parent)?;
        let canonical_parent = fs::canonicalize(parent)?;
        self.ensure_canonical_inside_root(&options.path, canonical_parent)?;

        git_worktree_add(
            &self.repo_root,
            &options.path,
            &branch_name,
            options.base_ref.as_deref(),
        )?;

        let target_canonical = self.canonical_allowed_path(&options.path)?;
        for wt in self.list_worktrees()? {
            if let Ok(wt_canonical) = self.canonical_allowed_path(&wt.path) {
                if wt_canonical == target_canonical {
                    return Ok(wt);
                }
            }
        }

        Ok(Worktree {
            path: target_canonical,
            head: String::new(),
            branch: Some(format!("refs/heads/{branch_name}")),
            bare: false,
            detached: false,
            locked: None,
            prunable: None,
        })
    }

    pub fn list_worktrees(&self) -> Result<Vec<Worktree>, WorktreeError> {
        let worktrees = git_worktree_list(&self.repo_root)?;
        Ok(worktrees
            .into_iter()
            .filter_map(|mut wt| {
                let canonical = self.canonical_allowed_path(&wt.path).ok()?;
                wt.path = canonical;
                Some(wt)
            })
            .collect())
    }

    pub fn find_worktree(&self, path: &Path) -> Result<Option<Worktree>, WorktreeError> {
        let target_canonical = self.canonical_allowed_path(path)?;
        for wt in self.list_worktrees()? {
            if wt.path == target_canonical {
                return Ok(Some(wt));
            }
        }
        Ok(None)
    }

    pub fn find_worktree_by_slug(
        &self,
        ws_id: &str,
        slug: &str,
    ) -> Result<Option<Worktree>, WorktreeError> {
        let target_branch = Self::format_branch_name(ws_id, slug)?;
        for wt in self.list_worktrees()? {
            if wt.branch_short_name() == Some(target_branch.as_str()) {
                return Ok(Some(wt));
            }
        }
        Ok(None)
    }

    pub fn check_dirty(&self, worktree_path: &Path) -> Result<DirtyState, WorktreeError> {
        let canonical = self.canonical_worktree_path(worktree_path)?;
        git_status_porcelain(&canonical)
    }

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
        let canonical = self.canonical_worktree_path(worktree_path)?;
        self.ensure_no_writer(&canonical)?;
        let dirty_state = self.check_dirty(&canonical)?;
        if dirty_state.is_dirty {
            let count = dirty_state.files.len();
            let files = dirty_state.files.into_iter().map(|file| file.path).collect();
            return Err(WorktreeError::DirtyWorktree {
                path: canonical,
                count,
                files,
            });
        }

        git_worktree_remove(&self.repo_root, &canonical, force)?;
        let _ = git_worktree_prune(&self.repo_root);
        Ok(())
    }

    pub fn remove_worktree(&self, worktree_path: &Path, force: bool) -> Result<(), WorktreeError> {
        let _delete_guard = self.delete_lock.lock();
        self.remove_worktree_locked(worktree_path, force)
    }

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
        let canonical = self.canonical_worktree_path(worktree_path)?;
        let existing = self
            .find_worktree(&canonical)?
            .ok_or_else(|| WorktreeError::WorktreeNotFound {
                path: canonical.clone(),
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
