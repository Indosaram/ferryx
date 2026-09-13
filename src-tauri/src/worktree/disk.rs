//! On-demand apparent-size measurement. Never follows links or publishes partial sizes.
use crate::ipc::{IpcError, IpcErrorCode};
use crate::worktree::{git, DirtyFile, Worktree, WorktreeManager};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskWalkProgress {
    pub bytes: u64,
    pub files: u64,
    pub entries: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeDiskRow {
    pub worktree: Worktree,
    /// Apparent bytes, not space guaranteed to be reclaimed. None means unknown.
    pub size_bytes: Option<u64>,
    /// Committer timestamp in Unix seconds, NOT the last edit/access time.
    pub last_commit_at: Option<i64>,
    pub is_dirty: Option<bool>,
    pub dirty_files: Vec<DirtyFile>,
    pub error: Option<IpcError>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiskProgress {
    pub completed_worktrees: usize,
    pub total_worktrees: usize,
    pub current_path: Option<PathBuf>,
    pub scanned_bytes: u64,
    pub scanned_files: u64,
    pub scanned_entries: u64,
}

pub(crate) fn check_cancelled(cancelled: &AtomicBool) -> Result<(), IpcError> {
    if cancelled.load(Ordering::Acquire) {
        Err(IpcError::new(
            IpcErrorCode::ScanCancelled,
            "Disk scan cancelled",
        ))
    } else {
        Ok(())
    }
}

fn io_error(path: &Path, error: std::io::Error) -> IpcError {
    IpcError::new(IpcErrorCode::IoError, error.to_string())
        .with_details(json!({"path": path, "kind": format!("{:?}", error.kind())}))
}

// Windows junctions can behave like directories rather than symlinks. Treat all
// reparse points as links; the portable fallback uses symlink_metadata's file type.
mod platform {
    #[cfg(windows)]
    pub fn is_link(metadata: &std::fs::Metadata) -> bool {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }

    #[cfg(not(windows))]
    pub fn is_link(metadata: &std::fs::Metadata) -> bool {
        metadata.file_type().is_symlink()
    }
}

/// Iterative depth-first walk keeps directory handles proportional to depth;
/// a canonical-directory set additionally guards against cycles during tree changes.
/// Cancellation is checked between every filesystem operation/entry. Individual OS
/// calls cannot be interrupted, but no subsequent traversal proceeds after cancel.
/// Nested worktree roots are excluded so a repository row does not count them twice.
pub fn scan_path(
    root: &Path,
    excluded_roots: &[PathBuf],
    cancelled: &AtomicBool,
    mut progress: impl FnMut(DiskWalkProgress) -> Result<(), IpcError>,
) -> Result<DiskWalkProgress, IpcError> {
    let mut total = DiskWalkProgress::default();
    let mut directories: Vec<(PathBuf, fs::ReadDir)> = Vec::new();
    let mut seen = HashSet::new();
    let mut next = Some(root.to_path_buf());
    loop {
        check_cancelled(cancelled)?;
        let path = if let Some(path) = next.take() {
            path
        } else if let Some((parent, entries)) = directories.last_mut() {
            match entries.next() {
                Some(entry) => entry.map_err(|error| io_error(parent, error))?.path(),
                None => {
                    directories.pop();
                    continue;
                }
            }
        } else {
            break;
        };
        check_cancelled(cancelled)?;
        if excluded_roots.iter().any(|excluded| excluded == &path) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, error))?;
        total.entries += 1;
        if !platform::is_link(&metadata) {
            if metadata.is_dir() {
                check_cancelled(cancelled)?;
                let canonical = fs::canonicalize(&path).map_err(|error| io_error(&path, error))?;
                if seen.insert(canonical) {
                    check_cancelled(cancelled)?;
                    directories.push((
                        path.clone(),
                        fs::read_dir(&path).map_err(|error| io_error(&path, error))?,
                    ));
                }
            } else if metadata.is_file() {
                total.bytes = total
                    .bytes
                    .checked_add(metadata.len())
                    .ok_or_else(|| IpcError::internal("Disk size exceeds u64"))?;
                total.files += 1;
            }
        }
        progress(total.clone())?;
    }
    check_cancelled(cancelled)?;
    progress(total.clone())?;
    Ok(total)
}

/// Preserve missing/prunable Git records without weakening the registry's root jail.
/// Existing paths use the manager's canonical check; missing paths validate their
/// nearest existing ancestor so a symlinked parent cannot escape the repository.
fn allowed_listing_path(manager: &WorktreeManager, path: &Path) -> Result<PathBuf, IpcError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(
            IpcError::new(IpcErrorCode::InvalidPath, "Invalid worktree listing path")
                .with_details(json!({"path": path})),
        );
    }
    let mut ancestor = path;
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let canonical = manager
                    .canonical_allowed_path(ancestor)
                    .map_err(IpcError::from)?;
                let suffix = path
                    .strip_prefix(ancestor)
                    .expect("ancestor of listing path");
                return Ok(canonical.join(suffix));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or_else(|| io_error(path, error))?;
            }
            Err(error) => return Err(io_error(ancestor, error)),
        }
    }
}

fn commit_times(manager: &WorktreeManager) -> Result<HashMap<String, i64>, IpcError> {
    let output = git::run_git(
        manager.repo_root(),
        &[
            "for-each-ref",
            "--format=%(refname)%09%(committerdate:unix)",
            "refs/heads/",
        ],
    )
    .map_err(IpcError::from)?;
    output
        .lines()
        .map(|line| {
            let (branch, timestamp) = line.split_once('\t').ok_or_else(|| {
                IpcError::new(IpcErrorCode::ParseError, "Invalid commit metadata")
            })?;
            let timestamp = timestamp
                .parse::<i64>()
                .map_err(|error| IpcError::new(IpcErrorCode::ParseError, error.to_string()))?;
            Ok((branch.to_owned(), timestamp))
        })
        .collect()
}

pub fn collect_workspace(
    manager: &WorktreeManager,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(WorkspaceDiskProgress) -> Result<(), IpcError>,
) -> Result<Vec<WorktreeDiskRow>, IpcError> {
    check_cancelled(cancelled)?;
    if !manager.is_git_backed() {
        return Ok(Vec::new());
    }
    let mut worktrees = Vec::new();
    for mut worktree in git::git_worktree_list(manager.repo_root()).map_err(IpcError::from)? {
        check_cancelled(cancelled)?;
        match allowed_listing_path(manager, &worktree.path) {
            Ok(path) => {
                worktree.path = path;
                worktrees.push(worktree);
            }
            // External worktrees are outside Ferryx's existing workspace boundary.
            Err(error) if error.code == IpcErrorCode::PathOutsideWorkspace => {}
            Err(error) => return Err(error),
        }
    }
    check_cancelled(cancelled)?;
    let times = commit_times(manager)?;
    let roots: Vec<_> = worktrees
        .iter()
        .map(|worktree| worktree.path.clone())
        .collect();
    let mut status = WorkspaceDiskProgress {
        total_worktrees: worktrees.len(),
        ..Default::default()
    };
    let mut rows = Vec::new();
    for worktree in worktrees {
        check_cancelled(cancelled)?;
        status.current_path = Some(worktree.path.clone());
        progress(status.clone())?;
        let mut row = WorktreeDiskRow {
            last_commit_at: worktree
                .branch
                .as_ref()
                .and_then(|branch| times.get(branch))
                .copied(),
            worktree,
            size_bytes: None,
            is_dirty: None,
            dirty_files: Vec::new(),
            error: None,
        };
        // Detached HEADs have no ref; read the listed commit, including missing paths.
        if row.last_commit_at.is_none()
            && !row.worktree.head.is_empty()
            && !row.worktree.head.chars().all(|c| c == '0')
        {
            check_cancelled(cancelled)?;
            match git::run_git(
                manager.repo_root(),
                &["show", "-s", "--format=%ct", &row.worktree.head, "--"],
            )
            .map_err(IpcError::from)
            .and_then(|output| {
                output
                    .trim()
                    .parse::<i64>()
                    .map_err(|error| IpcError::new(IpcErrorCode::ParseError, error.to_string()))
            }) {
                Ok(timestamp) => row.last_commit_at = Some(timestamp),
                Err(error) => row.error = Some(error),
            }
        }
        check_cancelled(cancelled)?;
        let excluded: Vec<_> = roots
            .iter()
            .filter(|path| *path != &row.worktree.path)
            .cloned()
            .collect();
        let base = status.clone();
        let size = scan_path(&row.worktree.path, &excluded, cancelled, |walk| {
            status.scanned_bytes = base.scanned_bytes + walk.bytes;
            status.scanned_files = base.scanned_files + walk.files;
            status.scanned_entries = base.scanned_entries + walk.entries;
            if walk.entries == 1 || walk.entries % 256 == 0 {
                progress(status.clone())?;
            }
            Ok(())
        });
        match size {
            Ok(size) => row.size_bytes = Some(size.bytes),
            Err(error) if error.code == IpcErrorCode::ScanCancelled => return Err(error),
            Err(error) => row.error = Some(error),
        }
        check_cancelled(cancelled)?;
        if row.size_bytes.is_some() && !row.worktree.bare {
            match manager.check_dirty(&row.worktree.path) {
                Ok(dirty) => {
                    row.is_dirty = Some(dirty.is_dirty);
                    row.dirty_files = dirty.files;
                }
                Err(error) => row.error = Some(error.into()),
            }
        }
        check_cancelled(cancelled)?;
        rows.push(row);
        status.completed_worktrees += 1;
        progress(status.clone())?;
    }
    check_cancelled(cancelled)?;
    Ok(rows)
}
