use crate::worktree::git::{
    git_branch_delete, git_branch_is_ancestor_of_head, git_status_porcelain, git_worktree_add,
    git_worktree_list, git_worktree_prune, git_worktree_remove, inspect_worktree, run_git,
};
use crate::worktree::model::{
    BranchDeletionPreview, CreateWorktreeOptions, DirtyState, ExternalWorktree, OrcaWorktreeInfo,
    Worktree, WorktreeError,
};
use crate::worktree::naming;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[cfg(test)]
thread_local! {
    pub(crate) static PRUNE_PROBE: std::cell::RefCell<Option<Box<dyn Fn(&Path, bool)>>> = const { std::cell::RefCell::new(None) };
}

/// Formats a worktree session ID scoped to a remote host, e.g. `"host-abc::sess-123"`.
pub fn format_host_scoped_session_id(host_id: &str, session_id: &str) -> String {
    format!("{host_id}::{session_id}")
}

/// Parses a host-scoped session ID produced by [`format_host_scoped_session_id`].
///
/// Returns `Some((host_id, session_id))` when `scoped` contains the `::` separator,
/// otherwise `None` (e.g. for raw, unscoped session IDs).
pub fn parse_host_scoped_session_id(scoped: &str) -> Option<(&str, &str)> {
    let mut parts = scoped.splitn(2, "::");
    let host_id = parts.next()?;
    let session_id = parts.next()?;
    Some((host_id, session_id))
}

/// A local->remote port forward for a dev server running inside a worktree on a remote host.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct DevServerPortForward {
    pub host_id: String,
    pub local_port: u16,
    pub remote_port: u16,
}

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
/// canonicalization. Construction accepts either that root or any directory within it,
/// then resolves the canonical root. IPC callers should prefer identity-based resolution
/// through `WorkspaceRegistry` instead of passing paths directly.
#[derive(Debug, Clone)]
pub struct WorktreeManager {
    repo_root: PathBuf,
    git_backed: bool,
    /// `<data>/worktree-settings.json`; `None` when no data directory resolves.
    settings_path: Option<PathBuf>,
    writer_leases: WriterLeaseRegistry,
    delete_lock: Arc<Mutex<()>>,
    dirty_snapshots: Arc<Mutex<HashMap<PathBuf, bool>>>,
    revision: Arc<std::sync::atomic::AtomicU64>,
}

impl WorktreeManager {
    /// Creates a manager and immediately validates/canonicalizes the Git repository root.
    /// Invalid roots are programmer configuration errors; fallible callers should use `try_new`.
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        let requested = repo_root.into();
        Self::try_new(&requested).unwrap_or_else(|error| {
            panic!(
                "invalid worktree manager root '{}': {error}",
                requested.display()
            )
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test_root(root: PathBuf) -> Self {
        Self {
            repo_root: root,
            git_backed: false,
            settings_path: None,
            writer_leases: WriterLeaseRegistry::default(),
            delete_lock: Arc::new(Mutex::new(())),
            dirty_snapshots: Arc::new(Mutex::new(HashMap::new())),
            revision: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub fn try_new(repo_root: impl Into<PathBuf>) -> Result<Self, WorktreeError> {
        let requested = repo_root.into();
        let canonical =
            fs::canonicalize(&requested).map_err(|_| WorktreeError::InvalidRepoRoot {
                path: requested.clone(),
            })?;
        if !canonical.is_dir() || canonical.parent().is_none() {
            return Err(WorktreeError::InvalidRepoRoot { path: canonical });
        }

        // Plain-folder mode: a non-Git directory is a valid (terminal-only)
        // workspace root; only worktree operations are unavailable.
        let (repo_root, git_backed) = match run_git(&canonical, &["rev-parse", "--show-toplevel"]) {
            Ok(top_level) => match fs::canonicalize(PathBuf::from(top_level.trim())) {
                Ok(git_root) => (git_root, true),
                Err(_) => (canonical, false),
            },
            Err(_) => (canonical, false),
        };
        Ok(Self {
            repo_root,
            git_backed,
            settings_path: naming::settings_path(),
            writer_leases: WriterLeaseRegistry::default(),
            delete_lock: Arc::new(Mutex::new(())),
            dirty_snapshots: Arc::new(Mutex::new(HashMap::new())),
            revision: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        })
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn bump_revision(&self) -> u64 {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            + 1
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// Uses another worktree settings file (tests and isolated QA daemons).
    pub fn with_settings_path(mut self, settings_path: Option<PathBuf>) -> Self {
        self.settings_path = settings_path;
        self
    }

    pub fn settings_path(&self) -> Option<&Path> {
        self.settings_path.as_deref()
    }

    /// Directory and branch prefix new worktrees of this repository use.
    pub fn layout(&self) -> naming::WorktreeLayout {
        naming::layout_for_repo(self.settings_path.as_deref(), &self.repo_root)
    }

    /// The branch a new worktree for `ws_id/slug` is created on.
    pub fn branch_name_for(&self, ws_id: &str, slug: &str) -> Result<String, WorktreeError> {
        Self::format_branch_name_with_prefix(
            &self.layout().branch_prefix,
            ws_id,
            slug,
            cfg!(windows),
        )
    }

    /// Every branch prefix this repository's managed worktrees may carry.
    pub fn known_branch_prefixes(&self) -> Vec<String> {
        let mut known: Vec<String> = naming::builtin_branch_prefixes()
            .map(str::to_string)
            .collect();
        for prefix in naming::configured_prefixes(self.settings_path.as_deref()) {
            if !known.contains(&prefix) {
                known.push(prefix);
            }
        }
        known
    }

    /// Parses a managed branch with every prefix this repository may use.
    pub fn parse_managed_branch(&self, branch: &str) -> Option<OrcaWorktreeInfo> {
        let known = self.known_branch_prefixes();
        naming::parse_branch(branch, known.iter().map(String::as_str))
            .map(|(ws_id, slug)| OrcaWorktreeInfo { ws_id, slug })
    }

    /// True when `branch` is the managed branch of `ws_id/slug` under any known prefix.
    pub fn is_identity_branch(&self, branch: Option<&str>, ws_id: &str, slug: &str) -> bool {
        branch
            .and_then(|branch| self.parse_managed_branch(branch))
            .is_some_and(|info| info.ws_id == ws_id && info.slug == slug)
    }

    fn managed_path_in(&self, dir: &str, ws_id: &str, slug: &str) -> PathBuf {
        let mut target = self.repo_root.join(dir).join(ws_id);
        for segment in slug.split('/') {
            target.push(segment);
        }
        target
    }

    /// Managed dirs to probe for `ws_id/slug`: this repository's layout dir first.
    fn managed_dirs(&self) -> Vec<&'static str> {
        let current = self.layout().worktree_dir;
        std::iter::once(current)
            .chain(naming::all_worktree_dirs().filter(|dir| *dir != current))
            .collect()
    }

    /// Whether this workspace root is a Git repository (worktree operations
    /// require it). Plain-folder workspaces are terminal-only.
    pub fn is_git_backed(&self) -> bool {
        self.git_backed
    }

    fn require_git_backed(&self) -> Result<(), WorktreeError> {
        if self.git_backed {
            Ok(())
        } else {
            Err(WorktreeError::NotAGitRepository {
                path: self.repo_root.clone(),
            })
        }
    }

    fn validate_path_components(&self, path: &Path) -> Result<(), WorktreeError> {
        if !path.is_absolute() {
            return Err(WorktreeError::InvalidPath {
                path: path.to_path_buf(),
                reason: "path must be absolute".into(),
            });
        }
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
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
            ancestor = ancestor
                .parent()
                .ok_or_else(|| WorktreeError::InvalidPath {
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

    /// The managed path of `ws_id/slug`: an existing checkout in any managed dir (current or
    /// legacy) wins, otherwise the location a new worktree is created at.
    pub fn worktree_path_for(&self, ws_id: &str, slug: &str) -> Result<PathBuf, WorktreeError> {
        Self::format_branch_name(ws_id, slug)?;
        let dirs = self.managed_dirs();
        let target = dirs
            .iter()
            .map(|dir| self.managed_path_in(dir, ws_id, slug))
            .find(|candidate| fs::symlink_metadata(candidate).is_ok())
            .unwrap_or_else(|| self.managed_path_in(dirs[0], ws_id, slug));
        self.validate_new_worktree_path(&target)?;
        Ok(target)
    }

    pub fn acquire_writer(
        &self,
        worktree_path: &Path,
        owner_id: &str,
    ) -> Result<(), WorktreeError> {
        let _delete_guard = self.delete_lock.lock();
        let canonical = self.canonical_worktree_path(worktree_path)?;
        self.writer_leases.acquire_canonical(canonical, owner_id)
    }

    pub fn release_writer(
        &self,
        worktree_path: &Path,
        owner_id: &str,
    ) -> Result<(), WorktreeError> {
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

    /// Branch for `ws_id/slug` under the default Ferryx prefix. Creation uses the repository's
    /// prefix through [`Self::branch_name_for`]; this static form also validates an identity.
    pub fn format_branch_name(ws_id: &str, slug: &str) -> Result<String, WorktreeError> {
        Self::format_branch_name_platform(ws_id, slug, cfg!(windows))
    }

    pub fn format_branch_name_platform(
        ws_id: &str,
        slug: &str,
        is_windows: bool,
    ) -> Result<String, WorktreeError> {
        Self::format_branch_name_with_prefix(naming::DEFAULT_BRANCH_PREFIX, ws_id, slug, is_windows)
    }

    pub fn format_branch_name_with_prefix(
        prefix: &str,
        ws_id: &str,
        slug: &str,
        is_windows: bool,
    ) -> Result<String, WorktreeError> {
        naming::validate_branch_prefix(prefix).map_err(|error| WorktreeError::InvalidNamespace {
            reason: error.reason,
        })?;
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
        Self::validate_ref_component_platform(ws_id, "workspace ID", is_windows)?;
        Self::validate_ref_component_platform(slug, "slug", is_windows)?;
        Ok(format!("{prefix}/{ws_id}/{slug}"))
    }

    /// Parses a managed branch under the current, legacy or any configured prefix.
    pub fn parse_branch_name(branch: &str) -> Option<OrcaWorktreeInfo> {
        naming::parse_known_branch(branch).map(|(ws_id, slug)| OrcaWorktreeInfo { ws_id, slug })
    }

    fn validate_ref_component(component: &str, label: &str) -> Result<(), WorktreeError> {
        Self::validate_ref_component_platform(component, label, cfg!(windows))
    }

    pub fn is_windows_reserved_segment(segment: &str) -> bool {
        let stem = segment.split('.').next().unwrap_or(segment).trim();
        let lower = stem.to_ascii_lowercase();
        match lower.as_str() {
            "con" | "prn" | "aux" | "nul" | "conin$" | "conout$" => return true,
            _ => {}
        }
        if lower.starts_with("com") || lower.starts_with("lpt") {
            let suffix = &stem[3..];
            match suffix {
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³" => {
                    return true
                }
                _ => {}
            }
        }
        false
    }

    pub fn validate_ref_component_platform(
        component: &str,
        label: &str,
        is_windows: bool,
    ) -> Result<(), WorktreeError> {
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
            if is_windows && Self::is_windows_reserved_segment(segment) {
                return Err(WorktreeError::InvalidNamespace {
                    reason: format!(
                        "{label} path segment '{segment}' is a reserved Windows device name"
                    ),
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
                if is_windows && matches!(ch, '<' | '>' | '"' | '|') {
                    return Err(WorktreeError::InvalidNamespace {
                        reason: format!("{label} contains invalid character '{ch}' on Windows"),
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
        self.require_git_backed()?;
        let branch_name = self.branch_name_for(&options.ws_id, &options.slug)?;
        self.validate_new_worktree_path(&options.path)?;
        if options.path.exists() {
            self.canonical_allowed_path(&options.path)?;
            return Err(WorktreeError::WorktreeAlreadyExists { path: options.path });
        }

        // Resolve before creating even parent directories; never pass an option-like ref.
        let base = options.base_ref.as_deref().unwrap_or("HEAD");
        if base.is_empty() || base.starts_with('-') || base.chars().any(char::is_control) {
            return Err(WorktreeError::InvalidNamespace {
                reason: "Invalid base ref".into(),
            });
        }
        let commit = run_git(
            &self.repo_root,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{base}^{{commit}}"),
            ],
        )?;
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
            Some(commit.trim()),
        )?;

        let target_canonical = self.canonical_allowed_path(&options.path)?;
        let result = inspect_worktree(&target_canonical).unwrap_or_else(|_| Worktree {
            path: target_canonical,
            head: String::new(),
            branch: Some(format!("refs/heads/{branch_name}")),
            bare: false,
            detached: false,
            locked: None,
            prunable: None,
        });
        self.bump_revision();
        Ok(result)
    }

    pub fn list_worktrees(&self) -> Result<Vec<Worktree>, WorktreeError> {
        if !self.git_backed {
            return Ok(Vec::new());
        }
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

    /// Live worktrees Git records for this repository whose checkout sits outside the root jail,
    /// such as ones Orca creates under `~/orca/workspaces`. They never join this workspace; the UI
    /// asks the user whether to import each as its own project. Only `git worktree list` is read;
    /// other tools' metadata files are never opened.
    pub fn list_external_worktrees(&self) -> Result<Vec<ExternalWorktree>, WorktreeError> {
        if !self.git_backed {
            return Ok(Vec::new());
        }
        let worktrees = git_worktree_list(&self.repo_root)?;
        Ok(worktrees
            .into_iter()
            .filter(|wt| !wt.bare && wt.prunable.is_none())
            .filter_map(|mut wt| {
                let canonical = fs::canonicalize(&wt.path).ok()?;
                if !canonical.is_dir() {
                    return None;
                }
                if self
                    .ensure_canonical_inside_root(&wt.path, canonical.clone())
                    .is_ok()
                {
                    return None;
                }
                wt.path = canonical;
                Some(ExternalWorktree::new(wt))
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
        Self::format_branch_name(ws_id, slug)?;
        for dir in self.managed_dirs() {
            let target = self.managed_path_in(dir, ws_id, slug);
            if !target.exists() {
                continue;
            }
            let canonical = match self.canonical_allowed_path(&target) {
                Ok(path) => path,
                Err(WorktreeError::PathOutsideWorkspace { .. })
                | Err(WorktreeError::WorktreeNotFound { .. }) => continue,
                Err(error) => return Err(error),
            };
            match inspect_worktree(&canonical) {
                Ok(worktree) => return Ok(Some(worktree)),
                Err(WorktreeError::GitError { .. }) => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    pub fn check_dirty(&self, worktree_path: &Path) -> Result<DirtyState, WorktreeError> {
        self.require_git_backed()?;
        let canonical = self.canonical_worktree_path(worktree_path)?;
        let output = run_git(&canonical, &["status", "--porcelain=v1", "-z"])?;
        let mut fields = output.split('\0').filter(|field| !field.is_empty());
        let mut files = Vec::new();
        while let Some(field) = fields.next() {
            if field.len() < 4 || !field.is_char_boundary(3) || field.as_bytes()[2] != b' ' {
                return Err(WorktreeError::ParseError("Invalid status record".into()));
            }
            let status_code = field[..2].to_owned();
            let path = field[3..].to_owned();
            // In -z mode rename/copy destinations come first, followed by the source.
            if status_code.contains(['R', 'C']) && fields.next().is_none() {
                return Err(WorktreeError::ParseError("Missing rename source".into()));
            }
            files.push(crate::worktree::DirtyFile { status_code, path });
        }
        Ok(DirtyState::dirty(files))
    }

    /// Deletion-only resolution; ordinary consumers still require existing paths.
    pub(crate) fn deletion_record(&self, path: &Path) -> Result<Worktree, WorktreeError> {
        match fs::symlink_metadata(path) {
            Ok(_) => {
                return self
                    .find_worktree(path)?
                    .ok_or_else(|| WorktreeError::WorktreeNotFound { path: path.into() })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        self.validate_new_worktree_path(path)?;
        let mut ancestor = path.parent().unwrap();
        loop {
            match fs::symlink_metadata(ancestor) {
                Ok(_) => {
                    self.canonical_allowed_path(ancestor)?;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    ancestor = ancestor.parent().unwrap()
                }
                Err(error) => return Err(error.into()),
            }
        }
        for record in git_worktree_list(&self.repo_root)? {
            if record.path != path || record.prunable.is_none() {
                continue;
            }
            if let Some(info) = record.orca_info() {
                Self::format_branch_name(&info.ws_id, &info.slug)?;
                let managed = naming::all_worktree_dirs()
                    .any(|dir| self.managed_path_in(dir, &info.ws_id, &info.slug) == path);
                if managed {
                    return Ok(record);
                }
            }
        }
        Err(WorktreeError::WorktreeNotFound { path: path.into() })
    }

    pub fn observe_dirty_state(
        &self,
        worktree_path: &Path,
    ) -> Result<(DirtyState, bool), WorktreeError> {
        let canonical = self.canonical_worktree_path(worktree_path)?;
        let state = git_status_porcelain(&canonical)?;
        let previous = self
            .dirty_snapshots
            .lock()
            .insert(canonical, state.is_dirty);
        let changed = matches!(previous, Some(previous_dirty) if previous_dirty != state.is_dirty);
        Ok((state, changed))
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
    ) -> Result<bool, WorktreeError> {
        let canonical = self.deletion_record(worktree_path)?.path;
        let missing = !canonical.try_exists()?;
        if let Some(owner_id) = self.writer_leases.owner_canonical(&canonical) {
            return Err(WorktreeError::WriterAlreadyActive {
                path: canonical,
                owner_id,
            });
        }
        let dirty_state = if missing {
            DirtyState::clean()
        } else {
            self.check_dirty(&canonical)?
        };
        if !force && dirty_state.is_dirty {
            let count = dirty_state.files.len();
            let files = dirty_state
                .files
                .into_iter()
                .map(|file| file.path)
                .collect();
            return Err(WorktreeError::DirtyWorktree {
                path: canonical,
                count,
                files,
            });
        }

        git_worktree_remove(&self.repo_root, &canonical, force)?;
        self.dirty_snapshots.lock().remove(&canonical);
        self.bump_revision();
        // Missing-checkout repair is targeted: leave other stale records alone.
        // Ordinary deletion retains the metadata-cleanup followthrough contract.
        if !missing {
            #[cfg(test)]
            PRUNE_PROBE.with(|probe| {
                if let Some(probe) = probe.borrow().as_ref() {
                    probe(&self.repo_root, true);
                }
            });
            let prune = git_worktree_prune(&self.repo_root);
            #[cfg(test)]
            PRUNE_PROBE.with(|probe| {
                if let Some(probe) = probe.borrow_mut().take() {
                    probe(&self.repo_root, false);
                }
            });
            prune.map_err(|source| WorktreeError::WorktreeRemovedPruneFailed {
                path: canonical,
                source: Box::new(source),
            })?;
        }
        Ok(true)
    }

    pub fn remove_worktree(&self, worktree_path: &Path, force: bool) -> Result<(), WorktreeError> {
        let _delete_guard = self.delete_lock.lock();
        let canonical = self.canonical_worktree_path(worktree_path)?;
        self.ensure_no_writer(&canonical)?;
        let dirty_state = self.check_dirty(&canonical)?;
        if dirty_state.is_dirty {
            let count = dirty_state.files.len();
            let files = dirty_state
                .files
                .into_iter()
                .map(|file| file.path)
                .collect();
            return Err(WorktreeError::DirtyWorktree {
                path: canonical,
                count,
                files,
            });
        }
        self.remove_worktree_locked(worktree_path, force)
            .map(|_| ())
    }

    pub fn safe_delete(&self, worktree_path: &Path) -> Result<(), WorktreeError> {
        self.remove_worktree(worktree_path, false)
    }

    pub(crate) fn branch_is_merged(&self, branch: &str) -> Result<bool, WorktreeError> {
        git_branch_is_ancestor_of_head(&self.repo_root, branch)
    }

    pub fn branch_deletion_preview(
        &self,
        worktree_path: &Path,
    ) -> Result<BranchDeletionPreview, WorktreeError> {
        let existing = self.deletion_record(worktree_path)?;
        let missing = !existing.path.try_exists()?;
        let dirty_state = if missing {
            DirtyState::clean()
        } else {
            self.check_dirty(&existing.path)?
        };
        let branch = existing
            .branch_short_name()
            .ok_or_else(|| WorktreeError::ParseError("Detached worktree has no branch".into()))?
            .to_string();
        let mut preview = self.branch_deletion_preview_for_ref(&branch)?;
        preview.dirty_state = dirty_state;
        preview.missing = missing;
        Ok(preview)
    }

    /// Repository-ref inspection does not require a linked checkout to exist.
    /// Callers must first validate the managed identity and its path jail.
    pub(crate) fn branch_deletion_preview_for_ref(
        &self,
        branch: &str,
    ) -> Result<BranchDeletionPreview, WorktreeError> {
        let branch = branch.to_owned();
        let head = run_git(
            &self.repo_root,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("refs/heads/{branch}^{{commit}}"),
            ],
        )?
        .trim()
        .to_owned();
        let merged = self.branch_is_merged(&branch)?;
        let upstream = run_git(
            &self.repo_root,
            &[
                "rev-parse",
                "--abbrev-ref",
                &format!("{branch}@{{upstream}}"),
            ],
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
            // Ref-only inspection has no checkout state; path-aware callers fill it in.
            dirty_state: DirtyState::clean(),
            missing: true,
            branch,
            head,
            upstream,
            merged,
            ahead,
            behind,
        })
    }

    fn delete_worktree_and_branch_inner(
        &self,
        worktree_path: &Path,
        delete_branch: bool,
        destructive: bool,
    ) -> Result<bool, WorktreeError> {
        let _delete_guard = self.delete_lock.lock();
        let record = self.deletion_record(worktree_path)?;
        if let Some(owner_id) = self.writer_leases.owner_canonical(&record.path) {
            return Err(WorktreeError::WriterAlreadyActive {
                path: record.path,
                owner_id,
            });
        }

        let branch = if delete_branch {
            let preview = self.branch_deletion_preview(worktree_path)?;
            if !destructive && !preview.merged {
                return Err(WorktreeError::UnmergedBranch {
                    branch: preview.branch,
                    head: preview.head,
                });
            }
            Some(preview.branch)
        } else {
            None
        };

        let pruned = self.remove_worktree_locked(worktree_path, destructive)?;
        if let Some(branch) = branch {
            git_branch_delete(&self.repo_root, &branch, destructive)?;
        }
        Ok(pruned)
    }

    pub(crate) fn delete_worktree_and_branch_with_prune_status(
        &self,
        worktree_path: &Path,
        delete_branch: bool,
        destructive: bool,
    ) -> Result<bool, WorktreeError> {
        self.delete_worktree_and_branch_inner(worktree_path, delete_branch, destructive)
    }

    pub fn delete_worktree_and_branch(
        &self,
        worktree_path: &Path,
        delete_branch: bool,
    ) -> Result<(), WorktreeError> {
        self.delete_worktree_and_branch_inner(worktree_path, delete_branch, false)
            .map(|_| ())
    }

    pub fn delete_worktree_and_branch_destructive(
        &self,
        worktree_path: &Path,
        delete_branch: bool,
    ) -> Result<(), WorktreeError> {
        self.delete_worktree_and_branch_inner(worktree_path, delete_branch, true)
            .map(|_| ())
    }
}

#[cfg(test)]
mod host_scoped_session_id_tests {
    use super::*;

    #[test]
    fn test_host_scoped_worktree_session_id() {
        // Round-trip: format then parse recovers the original host/session IDs.
        let scoped = format_host_scoped_session_id("host-abc", "sess-123");
        assert_eq!(scoped, "host-abc::sess-123");
        assert_eq!(
            parse_host_scoped_session_id(&scoped),
            Some(("host-abc", "sess-123"))
        );

        // Session IDs may themselves contain "::"; splitn(2, ..) keeps the remainder intact.
        let nested = format_host_scoped_session_id("host-1", "sess::with::colons");
        assert_eq!(nested, "host-1::sess::with::colons");
        assert_eq!(
            parse_host_scoped_session_id(&nested),
            Some(("host-1", "sess::with::colons"))
        );

        // Invalid / unscoped strings (no "::" separator) return None.
        assert_eq!(parse_host_scoped_session_id("raw-session-id"), None);
        assert_eq!(parse_host_scoped_session_id(""), None);
        assert_eq!(parse_host_scoped_session_id("no-separator-here"), None);

        // DevServerPortForward serializes and deserializes losslessly.
        let forward = DevServerPortForward {
            host_id: "host-abc".to_string(),
            local_port: 4200,
            remote_port: 3000,
        };
        let json = serde_json::to_string(&forward).expect("serialize DevServerPortForward");
        let round_tripped: DevServerPortForward =
            serde_json::from_str(&json).expect("deserialize DevServerPortForward");
        assert_eq!(forward, round_tripped);
    }
}
