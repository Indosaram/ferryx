//! On-demand apparent-size measurement. Never follows links or publishes partial sizes.
use crate::ipc::{IpcError, IpcErrorCode};
use crate::worktree::{git, DirtyFile, Worktree, WorktreeManager};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(100);

struct ProgressThrottle {
    interval: Duration,
    last_emitted: Option<Instant>,
}

impl ProgressThrottle {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_emitted: None,
        }
    }

    fn should_emit(&mut self, now: Instant, force: bool) -> bool {
        if force
            || self
                .last_emitted
                .map_or(true, |last| now.saturating_duration_since(last) >= self.interval)
        {
            self.last_emitted = Some(now);
            true
        } else {
            false
        }
    }
}

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

// Windows sets FILE_ATTRIBUTE_REPARSE_POINT on every entry that carries a reparse point, and
// that set is wider than "link": OneDrive "Files On-Demand" cloud placeholders and AppExecLink
// execution aliases carry it too. Only the tags that name another path -- symlinks and
// junctions/mount points -- are links; every other tagged entry is an ordinary file or directory
// whose size must still be counted. The portable fallback uses symlink_metadata's file type.
#[cfg(any(windows, test))]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
#[cfg(any(windows, test))]
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
#[cfg(any(windows, test))]
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

/// Pure reparse classification, kept platform independent so it is testable everywhere.
#[cfg(any(windows, test))]
fn is_link_reparse_point(attributes: u32, reparse_tag: u32) -> bool {
    attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        && (reparse_tag == IO_REPARSE_TAG_SYMLINK || reparse_tag == IO_REPARSE_TAG_MOUNT_POINT)
}

mod platform {
    #[cfg(windows)]
    pub fn is_link(path: &std::path::Path, metadata: &std::fs::Metadata) -> bool {
        use std::os::windows::fs::MetadataExt;
        let attributes = metadata.file_attributes();
        if attributes & super::FILE_ATTRIBUTE_REPARSE_POINT == 0 {
            return false;
        }
        match reparse_tag(path) {
            Some(tag) => super::is_link_reparse_point(attributes, tag),
            // The tag query failed. symlink_metadata resolved the same
            // FILE_ATTRIBUTE_TAG_INFO, so trust its file type instead of guessing.
            None => metadata.file_type().is_symlink(),
        }
    }

    /// Reparse tag of `path` itself, never of its target: FILE_FLAG_OPEN_REPARSE_POINT keeps the
    /// handle on the link and FILE_FLAG_BACKUP_SEMANTICS allows directories to be opened.
    #[cfg(windows)]
    fn reparse_tag(path: &std::path::Path) -> Option<u32> {
        use std::mem::{size_of, MaybeUninit};
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FileAttributeTagInfo, GetFileInformationByHandleEx,
        };

        let file = std::fs::OpenOptions::new()
            .access_mode(0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .ok()?;
        let mut info = MaybeUninit::<FILE_ATTRIBUTE_TAG_INFO>::zeroed();
        // SAFETY: the handle is open and owned for this call, and the buffer is a correctly
        // sized FILE_ATTRIBUTE_TAG_INFO for FileAttributeTagInfo.
        let queried = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileAttributeTagInfo,
                info.as_mut_ptr().cast(),
                size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        };
        if queried == 0 {
            return None;
        }
        // SAFETY: a non-zero result means the buffer was filled.
        Some(unsafe { info.assume_init() }.ReparseTag)
    }

    #[cfg(not(windows))]
    pub fn is_link(_path: &std::path::Path, metadata: &std::fs::Metadata) -> bool {
        metadata.file_type().is_symlink()
    }

    #[cfg(unix)]
    pub type DirIdentity = (u64, u64);

    #[cfg(unix)]
    pub fn dir_identity(
        _path: &std::path::Path,
        metadata: &std::fs::Metadata,
    ) -> Result<DirIdentity, crate::ipc::IpcError> {
        use std::os::unix::fs::MetadataExt;
        Ok((metadata.dev(), metadata.ino()))
    }

    #[cfg(not(unix))]
    pub type DirIdentity = std::path::PathBuf;

    #[cfg(not(unix))]
    pub fn dir_identity(
        path: &std::path::Path,
        _metadata: &std::fs::Metadata,
    ) -> Result<DirIdentity, crate::ipc::IpcError> {
        std::fs::canonicalize(path).map_err(|error| super::io_error(path, error))
    }
}

/// Iterative depth-first walk keeps directory handles proportional to depth;
/// a visited-directory set (`(dev, ino)` on Unix, canonical path on Windows)
/// guards against cycles during tree changes without paying `canonicalize` per directory on Unix.
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
    let mut seen: HashSet<platform::DirIdentity> = HashSet::new();
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
        if !platform::is_link(&path, &metadata) {
            if metadata.is_dir() {
                check_cancelled(cancelled)?;
                let identity = platform::dir_identity(&path, &metadata)?;
                if seen.insert(identity) {
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
    let mut throttle = ProgressThrottle::new(PROGRESS_EMIT_INTERVAL);
    let mut rows = Vec::new();
    for worktree in worktrees {
        check_cancelled(cancelled)?;
        status.current_path = Some(worktree.path.clone());
        if throttle.should_emit(Instant::now(), false) {
            progress(status.clone())?;
        }
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
            if walk.entries % 256 == 0 && throttle.should_emit(Instant::now(), false) {
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
        let is_final = status.completed_worktrees == status.total_worktrees;
        if throttle.should_emit(Instant::now(), is_final) {
            progress(status.clone())?;
        }
    }
    check_cancelled(cancelled)?;
    Ok(rows)
}

#[cfg(test)]
mod reparse_point_tests {
    use super::*;

    #[test]
    fn test_reparse_point_link_classification() {
        // Real links: symlinks and junctions/mount points, always with the attribute set.
        assert!(is_link_reparse_point(
            FILE_ATTRIBUTE_REPARSE_POINT,
            IO_REPARSE_TAG_SYMLINK
        ));
        assert!(is_link_reparse_point(
            FILE_ATTRIBUTE_REPARSE_POINT,
            IO_REPARSE_TAG_MOUNT_POINT
        ));

        // OneDrive "Files On-Demand" placeholders and AppExecLink carry the same attribute but
        // name no other path, so they stay ordinary entries.
        assert!(!is_link_reparse_point(
            FILE_ATTRIBUTE_REPARSE_POINT,
            0x9000_001A // IO_REPARSE_TAG_CLOUD_6, a dehydrated OneDrive placeholder
        ));
        assert!(!is_link_reparse_point(
            FILE_ATTRIBUTE_REPARSE_POINT,
            0x8000_001B // IO_REPARSE_TAG_APPEXECLINK
        ));

        // The attribute bit still gates the tag: a tag without it is not a reparse point.
        assert!(!is_link_reparse_point(0, IO_REPARSE_TAG_SYMLINK));
        assert!(!is_link_reparse_point(0, 0));
    }

    #[test]
    fn progress_throttle_enforces_interval_and_always_allows_final() {
        let mut throttle = ProgressThrottle::new(PROGRESS_EMIT_INTERVAL);
        let t0 = Instant::now();
        assert!(throttle.should_emit(t0, false), "first progress emits immediately");
        assert!(
            !throttle.should_emit(t0 + Duration::from_millis(50), false),
            "progress inside 100ms window is throttled"
        );
        assert!(
            throttle.should_emit(t0 + PROGRESS_EMIT_INTERVAL, false),
            "progress at 100ms boundary emits"
        );
        assert!(
            !throttle.should_emit(t0 + PROGRESS_EMIT_INTERVAL + Duration::from_millis(10), false),
            "non-final progress shortly after emit is throttled"
        );
        assert!(
            throttle.should_emit(t0 + PROGRESS_EMIT_INTERVAL + Duration::from_millis(10), true),
            "final progress is always emitted regardless of interval"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_dir_identity_uses_dev_ino_from_metadata() {
        let dir = tempfile::TempDir::new().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        let root_meta = fs::symlink_metadata(dir.path()).unwrap();
        let sub_meta = fs::symlink_metadata(&sub).unwrap();
        let root_id = platform::dir_identity(dir.path(), &root_meta).unwrap();
        let sub_id = platform::dir_identity(&sub, &sub_meta).unwrap();
        assert_ne!(root_id, sub_id);
        let dot_meta = fs::symlink_metadata(dir.path().join(".")).unwrap();
        assert_eq!(
            root_id,
            platform::dir_identity(&dir.path().join("."), &dot_meta).unwrap()
        );
    }
}
