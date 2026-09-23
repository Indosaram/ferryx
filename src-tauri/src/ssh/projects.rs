//! Durable remote identities, separate from the local workspace/Git registry.
// allow: SIZE_OK — inline memo cache and tests mandated in single-file scope
use super::{direct, SshHost};
use crate::ipc::{ssh::SshHostStore, IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

static MUTATION: Mutex<()> = Mutex::new(());
pub const REMOTE_PREFIX: &str = "ssh:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProject {
    pub workspace_id: String,
    pub host_id: String,
    pub repo_root: String,
    pub git_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_remote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<super::runtime::RemotePlatform>,
}

pub fn is_remote(id: &str) -> bool {
    id.starts_with(REMOTE_PREFIX)
}

pub fn unsupported() -> IpcError {
    IpcError::new(
        IpcErrorCode::Unsupported,
        "This operation is unavailable for direct SSH projects",
    )
}

fn io(error: std::io::Error) -> IpcError {
    IpcError::new(IpcErrorCode::IoError, error.to_string())
}

pub fn store_path(host_store: &Path) -> PathBuf {
    host_store.with_file_name("remote_projects.json")
}

#[derive(Clone)]
enum CachedFile {
    Projects(BTreeMap<String, RemoteProject>),
    Hosts(SshHostStore),
}

impl CachedFile {
    fn as_projects(&self) -> Option<BTreeMap<String, RemoteProject>> {
        match self {
            Self::Projects(projects) => Some(projects.clone()),
            _ => None,
        }
    }

    fn as_hosts(&self) -> Option<SshHostStore> {
        match self {
            Self::Hosts(hosts) => Some(hosts.clone()),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct FileMemo {
    mtime: SystemTime,
    len: u64,
    value: Result<CachedFile, IpcError>,
}

/// The projects file and the hosts file can resolve to the same path for some
/// callers, so each kind keeps its own map and a path never poisons the other.
static PROJECTS_MEMO: OnceLock<Mutex<HashMap<PathBuf, FileMemo>>> = OnceLock::new();
static HOSTS_MEMO: OnceLock<Mutex<HashMap<PathBuf, FileMemo>>> = OnceLock::new();

#[derive(Clone, Copy)]
enum MemoKind {
    Projects,
    Hosts,
}

fn file_memo_cache(kind: MemoKind) -> &'static Mutex<HashMap<PathBuf, FileMemo>> {
    match kind {
        MemoKind::Projects => PROJECTS_MEMO.get_or_init(|| Mutex::new(HashMap::new())),
        MemoKind::Hosts => HOSTS_MEMO.get_or_init(|| Mutex::new(HashMap::new())),
    }
}

static MEMO_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Writers call this after replacing the store file so a same-second rewrite of
/// identical length is never served from the memo.
fn invalidate_file_memo(path: &Path) {
    MEMO_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    for kind in [MemoKind::Projects, MemoKind::Hosts] {
        let mut guard = file_memo_cache(kind)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard.remove(path);
    }
}

/// Invalidate cached store memos for both the host store file and the associated
/// projects store file derived from that root.
pub fn invalidate_store_memo(host_store: &Path) {
    invalidate_file_memo(host_store);
    invalidate_file_memo(&store_path(host_store));
}

fn memoized_read<T: Clone + Default>(
    path: &Path,
    kind: MemoKind,
    extract: fn(&CachedFile) -> Option<T>,
    parse: impl FnOnce(&[u8]) -> Result<CachedFile, IpcError>,
) -> Result<T, IpcError> {
    let gen_before = MEMO_GENERATION.load(std::sync::atomic::Ordering::SeqCst);
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut guard = file_memo_cache(kind)
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            guard.remove(path);
            return Ok(T::default());
        }
        Err(e) => return Err(io(e)),
    };
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let len = metadata.len();

    {
        let guard = file_memo_cache(kind)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.get(path) {
            if entry.mtime == mtime && entry.len == len {
                if let Ok(file) = &entry.value {
                    if let Some(val) = extract(file) {
                        return Ok(val);
                    }
                }
            }
        }
    }

    let parsed = match std::fs::read(path) {
        Ok(bytes) => parse(&bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => Err(io(e)),
    };

    let mut guard = file_memo_cache(kind)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    guard.insert(
        path.to_path_buf(),
        FileMemo {
            mtime,
            len,
            value: parsed.clone(),
        },
    );
    let post_sig = std::fs::metadata(path)
        .ok()
        .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
    if post_sig != Some((mtime, len))
        || MEMO_GENERATION.load(std::sync::atomic::Ordering::SeqCst) != gen_before
    {
        guard.remove(path);
    }
    match parsed {
        Ok(ref file) => extract(file).ok_or_else(|| {
            IpcError::new(IpcErrorCode::ParseError, "Unexpected cached file type")
        }),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
pub(crate) static PROJECTS_PARSE_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

fn parse_projects(bytes: &[u8]) -> Result<BTreeMap<String, RemoteProject>, IpcError> {
    #[cfg(test)]
    PROJECTS_PARSE_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    serde_json::from_slice(bytes)
        .map_err(|e| IpcError::new(IpcErrorCode::ParseError, e.to_string()))
}

fn parse_host_store(bytes: &[u8]) -> Result<SshHostStore, IpcError> {
    serde_json::from_slice(bytes)
        .map_err(|e| IpcError::new(IpcErrorCode::ParseError, e.to_string()))
}

fn read_projects(host_store: &Path) -> Result<BTreeMap<String, RemoteProject>, IpcError> {
    memoized_read(
        &store_path(host_store),
        MemoKind::Projects,
        CachedFile::as_projects,
        |bytes| parse_projects(bytes).map(CachedFile::Projects),
    )
}

fn read_host_store(host_store: &Path) -> Result<SshHostStore, IpcError> {
    memoized_read(
        host_store,
        MemoKind::Hosts,
        CachedFile::as_hosts,
        |bytes| parse_host_store(bytes).map(CachedFile::Hosts),
    )
}

pub fn enabled_host(host_store: &Path, host_id: &str) -> Result<SshHost, IpcError> {
    let store = read_host_store(host_store)?;
    let host = store
        .hosts
        .into_iter()
        .find(|h| h.id == host_id)
        .ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::WorkspaceNotFound,
                "Configured SSH host no longer exists",
            )
            .with_details(serde_json::json!({"hostId": host_id, "reason": "hostMissing"}))
        })?;
    if host.disabled == Some(true) {
        return Err(IpcError::new(
            IpcErrorCode::WorkspaceNotFound,
            "Configured SSH host is disabled",
        )
        .with_details(serde_json::json!({"hostId": host_id, "reason": "hostDisabled"})));
    }
    direct::validate_host(&host)?;
    Ok(host)
}

pub fn identity(host_id: &str, root: &str) -> String {
    let mut hash = Sha256::new();
    hash.update((host_id.len() as u64).to_le_bytes());
    hash.update(host_id.as_bytes());
    hash.update(root.as_bytes());
    format!("{REMOTE_PREFIX}{:x}", hash.finalize())
}

pub fn resolve(
    host_store: &Path,
    workspace_id: &str,
) -> Result<(RemoteProject, SshHost), IpcError> {
    let projects = read_projects(host_store)?;
    let project = projects.get(workspace_id).ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::WorkspaceNotFound,
            "Remote project is not registered",
        )
    })?;
    if project.workspace_id != workspace_id
        || identity(&project.host_id, &project.repo_root) != workspace_id
    {
        return Err(IpcError::new(
            IpcErrorCode::ParseError,
            "Remote project identity does not match its stored location",
        ));
    }
    project
        .platform
        .unwrap_or(super::runtime::RemotePlatform::Posix)
        .validate_path(&project.repo_root)?;
    let host = enabled_host(host_store, &project.host_id)?;
    Ok((project.clone(), host))
}

fn save(host_store: &Path, projects: &BTreeMap<String, RemoteProject>) -> Result<(), IpcError> {
    let path = store_path(host_store);
    let bytes =
        serde_json::to_vec_pretty(projects).map_err(|e| IpcError::internal(e.to_string()))?;
    // The host store's parent already exists. A unique temp file never clobbers another writer.
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, &path)
    })();
    if let Err(error) = result {
        match std::fs::remove_file(&temp) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(io(std::io::Error::other(format!(
                    "{error}; temp cleanup failed: {e}"
                ))))
            }
        }
        return Err(io(error));
    }
    invalidate_store_memo(host_store);
    Ok(())
}

pub fn persist(host_store: &Path, project: RemoteProject) -> Result<RemoteProject, IpcError> {
    let _guard = MUTATION
        .lock()
        .map_err(|e| IpcError::internal(e.to_string()))?;
    // Re-resolve after the network probe; deletion/disable during registration fails closed.
    enabled_host(host_store, &project.host_id)?;
    let mut projects = read_projects(host_store)?;
    if let Some(existing) = projects.get(&project.workspace_id) {
        if existing.host_id != project.host_id || existing.repo_root != project.repo_root {
            return Err(IpcError::new(
                IpcErrorCode::WorkspaceAlreadyRegistered,
                "Remote identity collision",
            ));
        }
    }
    projects.insert(project.workspace_id.clone(), project.clone());
    save(host_store, &projects)?;
    Ok(project)
}

pub fn unregister(host_store: &Path, workspace_id: &str) -> Result<(), IpcError> {
    let _guard = MUTATION
        .lock()
        .map_err(|e| IpcError::internal(e.to_string()))?;
    let mut projects = read_projects(host_store)?;
    if projects.remove(workspace_id).is_some() {
        save(host_store, &projects)?;
    }
    Ok(())
}

/// Provenance comes only from explicit workspace context. A legacy path-only
/// request remains local, regardless of paths saved for remote projects.
pub fn guard_reveal(workspace_id: Option<&str>) -> Result<(), IpcError> {
    if workspace_id.is_some_and(is_remote) {
        return Err(unsupported());
    }
    Ok(())
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;

#[cfg(test)]
mod memo_tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn test_fixture() -> (tempfile::TempDir, PathBuf, SshHost) {
        let dir = tempfile::tempdir().unwrap();
        let host_path = dir.path().join("ssh_hosts.json");
        let host = SshHost {
            id: "h1".into(),
            label: "Host 1".into(),
            hostname: "127.0.0.1".into(),
            username: None,
            port: None,
            identity_file: None,
            jump_host: None,
            source: crate::ssh::SshHostSource::Manual,
            auth_method: crate::ssh::SshAuthMethod::Agent,
            disabled: None,
        };
        let hosts_json = serde_json::to_vec(&SshHostStore {
            hosts: vec![host.clone()],
            tombstones: vec![],
        })
        .unwrap();
        std::fs::write(&host_path, hosts_json).unwrap();
        (dir, host_path, host)
    }

    #[test]
    fn resolve_memoizes_unchanged_projects_file_without_reparsing() {
        let _guard = MUTATION.lock().unwrap_or_else(|e| e.into_inner());
        let (_dir, host_store, host) = test_fixture();
        let proj = RemoteProject {
            workspace_id: identity(&host.id, "/repo"),
            host_id: host.id.clone(),
            repo_root: "/repo".into(),
            git_root: None,
            git_remote: None,
            git_branch: None,
            git_head: None,
            platform: None,
        };
        let mut projects = BTreeMap::new();
        projects.insert(proj.workspace_id.clone(), proj.clone());
        let projects_path = store_path(&host_store);
        std::fs::write(&projects_path, serde_json::to_vec_pretty(&projects).unwrap()).unwrap();

        PROJECTS_PARSE_COUNT.store(0, Ordering::SeqCst);
        let (p1, h1) = resolve(&host_store, &proj.workspace_id).expect("first resolve succeeds");
        let (p2, h2) = resolve(&host_store, &proj.workspace_id).expect("second resolve succeeds");
        assert_eq!(p1, proj);
        assert_eq!(p2, proj);
        assert_eq!(h1.id, host.id);
        assert_eq!(h2.id, host.id);
        assert_eq!(PROJECTS_PARSE_COUNT.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn resolve_observes_new_content_when_file_rewritten_with_different_size() {
        let _guard = MUTATION.lock().unwrap_or_else(|e| e.into_inner());
        let (_dir, host_store, host) = test_fixture();
        let proj = RemoteProject {
            workspace_id: identity(&host.id, "/repo"),
            host_id: host.id.clone(),
            repo_root: "/repo".into(),
            git_root: None,
            git_remote: None,
            git_branch: None,
            git_head: None,
            platform: None,
        };
        let mut projects = BTreeMap::new();
        projects.insert(proj.workspace_id.clone(), proj.clone());
        let projects_path = store_path(&host_store);
        std::fs::write(&projects_path, serde_json::to_vec_pretty(&projects).unwrap()).unwrap();

        PROJECTS_PARSE_COUNT.store(0, Ordering::SeqCst);
        let (initial, _) = resolve(&host_store, &proj.workspace_id).expect("initial resolve");
        assert_eq!(initial.git_branch, None);
        assert_eq!(PROJECTS_PARSE_COUNT.load(Ordering::SeqCst), 1);

        let mut updated_proj = proj.clone();
        updated_proj.git_branch = Some("feature/rewritten-signature-test".into());
        projects.insert(proj.workspace_id.clone(), updated_proj.clone());
        let new_bytes = serde_json::to_vec_pretty(&projects).unwrap();
        assert_ne!(
            new_bytes.len() as u64,
            std::fs::metadata(&projects_path).unwrap().len()
        );
        std::fs::write(&projects_path, &new_bytes).unwrap();

        let (after_rewrite, _) =
            resolve(&host_store, &proj.workspace_id).expect("resolve after rewrite");
        assert_eq!(
            after_rewrite.git_branch.as_deref(),
            Some("feature/rewritten-signature-test")
        );
        assert_eq!(after_rewrite, updated_proj);
        assert_eq!(PROJECTS_PARSE_COUNT.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn enabled_host_observes_new_content_after_same_length_and_mtime_rewrite_via_invalidation() {
        let _guard = MUTATION.lock().unwrap_or_else(|e| e.into_inner());
        let (_dir, host_store, host) = test_fixture();

        // Initial read populates the memo cache
        let initial = enabled_host(&host_store, &host.id).expect("initial host enabled");
        assert_eq!(initial.disabled, None);

        // Verify the memo cache holds an entry for host_store
        assert!(file_memo_cache(MemoKind::Hosts)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&host_store));

        // Prepare an updated host that is disabled. The disabled variant can be longer than
        // the original file, so both writes are made to an equal, explicitly chosen length.
        let mut disabled_host = host.clone();
        disabled_host.disabled = Some(true);
        let updated_store = SshHostStore {
            hosts: vec![disabled_host],
            tombstones: vec![],
        };
        let disabled_bytes = serde_json::to_vec(&updated_store).unwrap();
        let initial_bytes = std::fs::read(&host_store).expect("read initial store");
        let target_len = std::cmp::max(initial_bytes.len(), disabled_bytes.len()) + 8;

        // Pad the initial store (JSON tolerates trailing whitespace), then prime the memo on
        // the padded file so the rewrite below keeps the exact same byte length.
        let mut padded_initial = initial_bytes;
        padded_initial.resize(target_len, b' ');
        std::fs::write(&host_store, &padded_initial).expect("pad initial store");
        let primed = enabled_host(&host_store, &host.id).expect("enabled host on padded store");
        assert_eq!(primed.disabled, None);
        assert!(
            file_memo_cache(MemoKind::Hosts)
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key(&host_store),
            "padded initial read must populate the hosts memo"
        );

        let mut updated_bytes = disabled_bytes;
        updated_bytes.resize(target_len, b' ');
        assert_eq!(
            updated_bytes.len(),
            target_len,
            "simulated rewrite must match identical file length"
        );
        let initial_len = target_len as u64;

        // Write the same-length content to disk
        std::fs::write(&host_store, &updated_bytes).expect("write updated store");

        // Invalidate through the exported entry point
        invalidate_store_memo(&host_store);

        // Assert that the invalidation call cleared the memo entry
        assert!(
            !file_memo_cache(MemoKind::Hosts)
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key(&host_store),
            "invalidate_store_memo must clear the hosts memo entry"
        );

        // enabled_host must observe the new content (host is now disabled)
        let error = enabled_host(&host_store, &host.id).expect_err("disabled host must be rejected");
        assert_eq!(error.code, IpcErrorCode::WorkspaceNotFound);
        assert_eq!(
            error
                .details
                .as_ref()
                .and_then(|d| d.get("reason"))
                .and_then(|r| r.as_str()),
            Some("hostDisabled")
        );
    }

    #[test]
    fn invalidate_store_memo_clears_both_projects_and_hosts_caches() {
        let _guard = MUTATION.lock().unwrap_or_else(|e| e.into_inner());
        let (_dir, host_store, host) = test_fixture();

        let proj = RemoteProject {
            workspace_id: identity(&host.id, "/repo"),
            host_id: host.id.clone(),
            repo_root: "/repo".into(),
            git_root: None,
            git_remote: None,
            git_branch: None,
            git_head: None,
            platform: None,
        };
        let mut projects = BTreeMap::new();
        projects.insert(proj.workspace_id.clone(), proj.clone());
        let projects_path = store_path(&host_store);
        std::fs::write(&projects_path, serde_json::to_vec_pretty(&projects).unwrap()).unwrap();

        // Populate both caches
        let _ = enabled_host(&host_store, &host.id).expect("host read");
        let _ = read_projects(&host_store).expect("projects read");

        assert!(file_memo_cache(MemoKind::Hosts)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&host_store));
        assert!(file_memo_cache(MemoKind::Projects)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&projects_path));

        // Invalidate through the public entry point
        invalidate_store_memo(&host_store);

        assert!(!file_memo_cache(MemoKind::Hosts)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&host_store));
        assert!(!file_memo_cache(MemoKind::Projects)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&projects_path));
    }

    #[test]
    fn memoized_read_race_safe_drops_stale_entry_when_invalidated_during_read() {
        let _guard = MUTATION.lock().unwrap_or_else(|e| e.into_inner());
        let (_dir, host_store, host) = test_fixture();

        let projects_path = store_path(&host_store);
        let mut projects: BTreeMap<String, RemoteProject> = BTreeMap::new();
        projects.insert(
            "p1".into(),
            RemoteProject {
                workspace_id: "p1".into(),
                host_id: host.id.clone(),
                repo_root: "/repo".into(),
                git_root: None,
                git_remote: None,
                git_branch: None,
                git_head: None,
                platform: None,
            },
        );
        std::fs::write(&projects_path, serde_json::to_vec_pretty(&projects).unwrap()).unwrap();

        // Simulate a reader whose read was in flight when an invalidation occurred:
        // Pass a parse closure that triggers an invalidation mid-flight.
        let path_clone = projects_path.clone();
        let res: Result<BTreeMap<String, RemoteProject>, IpcError> = memoized_read(
            &projects_path,
            MemoKind::Projects,
            CachedFile::as_projects,
            move |bytes| {
                invalidate_file_memo(&path_clone);
                parse_projects(bytes).map(CachedFile::Projects)
            },
        );
        assert!(res.is_ok(), "reader itself should receive the parsed data");

        // The cache must NOT retain the entry because it was invalidated mid-flight
        assert!(
            !file_memo_cache(MemoKind::Projects)
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key(&projects_path),
            "memo cache must not retain entry invalidated during read"
        );
    }
}
