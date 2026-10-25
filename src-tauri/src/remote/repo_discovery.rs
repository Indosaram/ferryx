//! Bounded, read-only Git repository discovery for machine grants.
//!
//! Breadth-first from a native directory, never following symlinks, stopping at
//! the first directory that carries a `.git` marker. Branch and linked-worktree
//! facts come from parsing `.git`/`HEAD` files only: no Git subprocess per repo.
use super::{
    auth::{DeviceAccessScope, DevicePermission},
    filesystem::{self, BrowseLimits, CancelOnDrop, FsError},
    machine_protocol::{DiscoveredRepository, Repositories, DIRECTORY_JSON_MAX_BYTES},
    state::RemoteGatewayState,
};
use axum::{
    extract::{Extension, RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::{
    collections::{HashMap, VecDeque},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

/// Advertised under the same Machine+Control condition as `directoryBrowseV1`.
pub const REPO_DISCOVERY_CAPABILITY: &str = "repoDiscoveryV1";
pub const DEFAULT_DEPTH: u8 = 3;
pub const MAX_DEPTH: u8 = 4;
/// Directory names never descended into, hidden or not.
const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".cache",
    "vendor",
    "dist",
    "build",
    "Library",
    ".Trash",
];
/// `.git` files and `HEAD` are tiny; anything larger is not trusted.
const MARKER_READ_LIMIT: u64 = 4096;

type DiscoveryResult<T> = Result<T, FsError>;

/// Walk caps. Exceeding any of them sets `truncated`; none of them errors.
#[derive(Debug, Clone, Copy)]
pub(super) struct Caps {
    pub(super) budget: Duration,
    pub(super) entries: usize,
    pub(super) repos: usize,
    pub(super) json_bytes: usize,
}
pub(super) const CAPS: Caps = Caps {
    budget: Duration::from_secs(3),
    entries: 20_000,
    repos: 200,
    // Leave room for the envelope (path, flags) under the shared response bound.
    json_bytes: DIRECTORY_JSON_MAX_BYTES - 8 * 1024,
};

fn fs_response(error: FsError) -> Response {
    let (status, code) = match error {
        FsError::Invalid => (StatusCode::BAD_REQUEST, "INVALID_PATH"),
        FsError::Missing => (StatusCode::NOT_FOUND, "DIRECTORY_NOT_FOUND"),
        FsError::Denied => (StatusCode::FORBIDDEN, "PERMISSION_DENIED"),
        FsError::Unsupported => (StatusCode::UNPROCESSABLE_ENTITY, "UNSUPPORTED_PATH"),
        FsError::Timeout => (StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"),
        FsError::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "HOST_UNAVAILABLE"),
    };
    super::server::machine_error(status, code)
}

fn io_error(error: std::io::Error) -> FsError {
    match error.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory => FsError::Missing,
        std::io::ErrorKind::PermissionDenied => FsError::Denied,
        std::io::ErrorKind::TimedOut => FsError::Timeout,
        _ => FsError::Unavailable,
    }
}

fn decode_component(value: &str) -> DiscoveryResult<String> {
    let mut output = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        output.push(match byte {
            b'+' => b' ',
            b'%' => {
                let hi = bytes
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or(FsError::Invalid)?;
                let lo = bytes
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or(FsError::Invalid)?;
                (hi * 16 + lo) as u8
            }
            byte => byte,
        });
    }
    String::from_utf8(output).map_err(|_| FsError::Invalid)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiscoveryQuery {
    pub(super) path: String,
    pub(super) depth: u8,
    pub(super) include_hidden: bool,
}

/// Accepts exactly `path`, `depth` (1..=4) and `includeHidden`, each at most once.
pub(super) fn query(raw: Option<&str>) -> DiscoveryResult<DiscoveryQuery> {
    let raw = raw.unwrap_or("");
    if raw.len() > 3 * 4096 + 128 {
        return Err(FsError::Invalid);
    }
    let (mut path, mut depth, mut hidden) = (None, None, None);
    for field in raw.split('&').filter(|field| !field.is_empty()) {
        let (key, value) = field.split_once('=').ok_or(FsError::Invalid)?;
        let value = decode_component(value)?;
        match decode_component(key)?.as_str() {
            "path" if path.is_none() => path = Some(value),
            "depth" if depth.is_none() => {
                depth = Some(match value.as_str() {
                    "1" => 1,
                    "2" => 2,
                    "3" => 3,
                    "4" => 4,
                    _ => return Err(FsError::Invalid),
                })
            }
            "includeHidden" if hidden.is_none() => {
                hidden = Some(match value.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err(FsError::Invalid),
                })
            }
            _ => return Err(FsError::Invalid),
        }
    }
    Ok(DiscoveryQuery {
        path: path.unwrap_or_default(),
        depth: depth.unwrap_or(DEFAULT_DEPTH),
        include_hidden: hidden.unwrap_or(false),
    })
}

fn path_text(path: &Path) -> Option<String> {
    path.to_str().map(crate::worktree::strip_verbatim_prefix)
}

/// Canonical text of an existing path, else its literal text.
fn canonical_text(path: &Path) -> Option<String> {
    match std::fs::canonicalize(path) {
        Ok(canonical) => path_text(&canonical),
        Err(_) => path_text(path),
    }
}

/// Reads a small regular file. Never opens FIFOs or devices that could block.
fn read_marker(path: &Path) -> Option<String> {
    if !std::fs::symlink_metadata(path).ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MARKER_READ_LIMIT)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// `ref: refs/heads/<branch>` names a branch; a detached HEAD (or anything else) is none.
pub(super) fn parse_branch(head: &str) -> Option<String> {
    let branch = head
        .lines()
        .next()?
        .trim()
        .strip_prefix("ref:")?
        .trim()
        .strip_prefix("refs/heads/")?;
    (!branch.is_empty() && !branch.chars().any(char::is_control)).then(|| branch.to_owned())
}

fn parse_gitdir(dir: &Path, marker: &str) -> Option<PathBuf> {
    let target = marker.lines().next()?.trim().strip_prefix("gitdir:")?.trim();
    if target.is_empty() || target.chars().any(char::is_control) {
        return None;
    }
    let target = PathBuf::from(target);
    Some(if target.is_absolute() {
        target
    } else {
        dir.join(target)
    })
}

/// Describes `dir` when it carries a `.git` directory or file; `None` when it does not.
/// A `.git` file whose `gitdir:` points into `<common>/worktrees/` is a linked worktree.
pub(super) fn inspect(dir: &Path) -> Option<DiscoveredRepository> {
    let marker = dir.join(".git");
    let metadata = std::fs::symlink_metadata(&marker).ok()?;
    let (git_dir, linked, common) = if metadata.is_dir() {
        (Some(marker.clone()), false, Some(marker))
    } else if metadata.is_file() {
        match read_marker(&marker).and_then(|text| parse_gitdir(dir, &text)) {
            Some(git_dir) => {
                let git_dir = std::fs::canonicalize(&git_dir).unwrap_or(git_dir);
                let parent = git_dir.parent();
                let linked = parent
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "worktrees")
                    && parent.and_then(Path::parent).is_some();
                let common = if linked {
                    parent.and_then(Path::parent).map(Path::to_path_buf)
                } else {
                    Some(git_dir.clone())
                };
                (Some(git_dir), linked, common)
            }
            // An unreadable marker still stops descent; it just carries no facts.
            None => (None, false, None),
        }
    } else {
        return None;
    };
    let branch = git_dir
        .and_then(|git_dir| read_marker(&git_dir.join("HEAD")))
        .and_then(|head| parse_branch(&head));
    Some(DiscoveredRepository {
        repo_root: path_text(dir)?,
        is_linked_worktree: linked,
        git_common_dir: common.as_deref().and_then(canonical_text),
        branch,
        registered_workspace_id: None,
    })
}

fn is_hidden(name: &str, entry: &std::fs::DirEntry) -> bool {
    let hidden = name.starts_with('.');
    #[cfg(windows)]
    let hidden = {
        use std::os::windows::fs::MetadataExt;
        hidden
            || entry
                .metadata()
                .is_ok_and(|metadata| metadata.file_attributes() & 2 != 0)
    };
    #[cfg(not(windows))]
    let _ = entry;
    hidden
}

#[derive(Debug, Default)]
pub(super) struct Walk {
    pub(super) repos: Vec<DiscoveredRepository>,
    pub(super) truncated: bool,
}
impl Walk {
    /// Adds one repository unless a result cap is reached; returns false when full.
    fn admit(&mut self, repo: DiscoveredRepository, size: &mut usize, caps: Caps) -> bool {
        let bytes = serde_json::to_vec(&repo)
            .map(|bytes| bytes.len() + 1)
            .unwrap_or(usize::MAX);
        if self.repos.len() >= caps.repos || size.saturating_add(bytes) > caps.json_bytes {
            self.truncated = true;
            return false;
        }
        *size += bytes;
        self.repos.push(repo);
        true
    }
}

/// Breadth-first discovery below an already-resolved native directory. Call only
/// inside run_blocking. `depth` is clamped to 1..=[MAX_DEPTH]: a repository is found
/// at most `depth` levels below `root`.
pub(super) fn walk(
    root: &Path,
    depth: u8,
    include_hidden: bool,
    caps: Caps,
    cancelled: &AtomicBool,
    elapsed: impl Fn() -> Duration,
) -> DiscoveryResult<Walk> {
    let depth = depth.clamp(1, MAX_DEPTH);
    let mut walk = Walk::default();
    let mut size = 0usize;
    if cancelled.load(Ordering::Acquire) {
        return Err(FsError::Timeout);
    }
    if let Some(repo) = inspect(root) {
        walk.admit(repo, &mut size, caps);
        return Ok(walk);
    }
    let mut queue = VecDeque::from([(root.to_path_buf(), 0u8)]);
    let mut entries = 0usize;
    'walk: while let Some((dir, level)) = queue.pop_front() {
        let children = match std::fs::read_dir(&dir) {
            Ok(children) => children,
            Err(error) if level == 0 => return Err(io_error(error)),
            // Unreadable subdirectories are skipped, not reported as a cap.
            Err(_) => continue,
        };
        for child in children {
            if cancelled.load(Ordering::Acquire) {
                return Err(FsError::Timeout);
            }
            if elapsed() >= caps.budget {
                walk.truncated = true;
                break 'walk;
            }
            entries += 1;
            if entries > caps.entries {
                walk.truncated = true;
                break 'walk;
            }
            let Ok(child) = child else { continue };
            // DirEntry::file_type never traverses a symlink (or Windows junction).
            let Ok(kind) = child.file_type() else { continue };
            if kind.is_symlink() || !kind.is_dir() {
                continue;
            }
            let Ok(name) = child.file_name().into_string() else {
                continue;
            };
            if name == ".git"
                || name.chars().any(char::is_control)
                || SKIPPED_DIRS.contains(&name.as_str())
                || (!include_hidden && is_hidden(&name, &child))
            {
                continue;
            }
            let path = dir.join(&name);
            if let Some(repo) = inspect(&path) {
                if !walk.admit(repo, &mut size, caps) {
                    break 'walk;
                }
            } else if level + 1 < depth {
                queue.push_back((path, level + 1));
            }
        }
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(FsError::Timeout);
    }
    walk.repos.sort_by(|a, b| a.repo_root.cmp(&b.repo_root));
    Ok(walk)
}

/// Joins each discovered root against the machine workspace catalog by canonical root.
pub(super) fn attach_registrations<'a>(
    repos: &mut [DiscoveredRepository],
    rows: impl IntoIterator<Item = (&'a str, &'a Path)>,
) {
    let index: HashMap<String, &str> = rows
        .into_iter()
        .filter_map(|(id, root)| Some((path_text(root)?, id)))
        .collect();
    for repo in repos {
        repo.registered_workspace_id = index
            .get(&crate::worktree::strip_verbatim_prefix(&repo.repo_root))
            .map(|id| (*id).to_owned());
    }
}

fn discover(
    state: &RemoteGatewayState,
    query: &DiscoveryQuery,
    home: &Path,
    cancelled: &AtomicBool,
    started: Instant,
) -> DiscoveryResult<Repositories> {
    let home = filesystem::resolve_directory("", home)?;
    let root = filesystem::resolve_directory(&query.path, &home)?;
    let walk = walk(
        &root,
        query.depth,
        query.include_hidden,
        CAPS,
        cancelled,
        || started.elapsed(),
    )?;
    let mut repos = walk.repos;
    if let Some(services) = &state.machine_services {
        // A catalog read failure leaves rows unregistered; discovery itself still stands.
        if let Ok(catalog) = services.workspaces.catalog() {
            attach_registrations(
                &mut repos,
                catalog
                    .workspaces
                    .iter()
                    .map(|(id, row)| (id.as_str(), row.repo_root.as_path())),
            );
        }
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(FsError::Timeout);
    }
    Ok(Repositories {
        path: path_text(&root).ok_or(FsError::Unsupported)?,
        repos,
        truncated: walk.truncated,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// `GET /api/v1/fs/repositories`. Same admission as `fs/directories`: bounded blocking
/// auth, Machine+Control scope, the shared per-device [BrowseLimits], and a revocation
/// select around the blocking walk.
pub(crate) async fn repositories(
    State(state): State<Arc<RemoteGatewayState>>,
    Extension(limits): Extension<Arc<BrowseLimits>>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Response, Response> {
    // Start the complete read budget before admission or blocking auth I/O.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let cancel = CancelOnDrop(Arc::new(AtomicBool::new(false)));
    let auth_flag = cancel.0.clone();
    let auth_permit = limits
        .auth_slots()
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            super::server::machine_error(StatusCode::TOO_MANY_REQUESTS, "CAPACITY_EXCEEDED")
        })?;
    let auth_state = state.clone();
    let auth_headers = headers.clone();
    let auth_work = crate::ipc::run_blocking(move || {
        let permit = auth_permit;
        let result = if auth_flag.load(Ordering::Acquire) || tokio::time::Instant::now() >= deadline
        {
            Err(fs_response(FsError::Timeout))
        } else {
            super::server::authenticate_machine_request(&auth_state, &auth_headers)
                .map_err(|response| *response)
        };
        drop(permit);
        Ok(result)
    });
    let device = tokio::time::timeout_at(deadline, auth_work)
        .await
        .map_err(|_| fs_response(FsError::Timeout))?
        .map_err(|_| fs_response(FsError::Unavailable))??;
    if device.access_scope != DeviceAccessScope::Machine
        || device.permission != DevicePermission::Control
    {
        return Err(super::server::machine_error(
            StatusCode::FORBIDDEN,
            "MACHINE_ACCESS_REQUIRED",
        ));
    }
    if state.machine_services.is_none() {
        return Err(super::server::machine_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MACHINE_SERVICE_UNAVAILABLE",
        ));
    }
    let mut revoked = state
        .auth_manager
        .device_revocation(&device.id)
        .map_err(|_| super::server::machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;
    let query = query(raw.as_deref()).map_err(fs_response)?;
    if tokio::time::Instant::now() >= deadline {
        return Err(fs_response(FsError::Timeout));
    }
    let permit = limits.acquire(&device.id).map_err(|response| *response)?;
    let flag = cancel.0.clone();
    let started = Instant::now();
    let work = crate::ipc::run_blocking(move || {
        let _permit = permit; // Cancellation cannot free a still-blocked worker slot.
        #[cfg(test)]
        let home = state.browse_home.read().clone();
        #[cfg(not(test))]
        let home: Option<PathBuf> = None;
        let home = home.or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
        });
        Ok(home
            .ok_or(FsError::Unavailable)
            .and_then(|home| discover(&state, &query, &home, &flag, started)))
    });
    let listing = tokio::select! {
        biased;
        _ = revoked.wait_for(|value| *value) => return Err(super::server::machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED")),
        result = tokio::time::timeout_at(deadline, work) => result.map_err(|_| fs_response(FsError::Timeout))?.map_err(|_| fs_response(FsError::Unavailable))?.map_err(fs_response)?,
    };
    if *revoked.borrow() {
        return Err(super::server::machine_error(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
        ));
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(fs_response(FsError::Timeout));
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(listing)).into_response())
}

#[cfg(test)]
#[path = "repo_discovery_tests.rs"]
mod tests;
