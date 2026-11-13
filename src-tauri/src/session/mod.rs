use crate::ipc::{IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Mirrors `window_registry::PRIMARY_WINDOW_LABEL`; kept local so session persistence stays
/// usable from the daemon build, which does not link the window registry.
pub const PRIMARY_SESSION_WINDOW_LABEL: &str = "main";
/// Mirrors `window_registry::SECONDARY_WINDOW_PREFIX`.
pub const SECONDARY_SESSION_WINDOW_PREFIX: &str = "ws-";

/// Maps a window label to its session file. `primary_path` is today's single-window file, which
/// the primary window keeps unchanged (old files keep loading, no migration). A secondary window
/// `ws-<id>` stores `session-ws-<id>.json` next to it. Labels that are not app windows, or that
/// could escape the session directory, are rejected.
pub fn session_path_for_window(primary_path: &Path, label: &str) -> Result<PathBuf, IpcError> {
    if label == PRIMARY_SESSION_WINDOW_LABEL {
        return Ok(primary_path.to_path_buf());
    }
    let id = label
        .strip_prefix(SECONDARY_SESSION_WINDOW_PREFIX)
        .filter(|id| {
            !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::InvalidArgument,
                format!("Not an app window label: {label:?}"),
            )
        })?;
    let file_name = format!("session-{SECONDARY_SESSION_WINDOW_PREFIX}{id}.json");
    Ok(match primary_path.parent() {
        Some(parent) => parent.join(file_name),
        None => PathBuf::from(file_name),
    })
}

/// Serializes every read/write/remove of one session file, inside THIS process and across every
/// other process that opens the same session (the desktop app and a remote/daemon writer).
///
/// The in-process mutex alone is not enough: load and save both classify the directory entry and
/// then act on it, and two processes doing that concurrently can each act on the other's fresh
/// publish. An advisory lock on a sibling `<name>.json.lock` file closes that window, so no
/// classification can be invalidated before the operation that relies on it finishes.
///
/// This is the ONLY lock gate. The cross-process lock used to be taken separately by each entry
/// point behind its own reentrancy stack, so a nested entry reached with one layer held could take
/// the other layer again; both layers now share one ownership stack, so the pair is reentrant
/// together or not at all.
pub fn try_with_session_path_lock<T>(
    path: &Path,
    operation: impl FnOnce() -> Result<T, IpcError>,
) -> Result<T, IpcError> {
    with_session_gate(path, operation)
}

thread_local! {
    /// Session keys this thread holds, covering BOTH layers: an entry here means the in-process
    /// mutex is held AND the cross-process lock file is locked. One stack, so the two layers can
    /// never disagree about what this thread already owns.
    static HELD: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Pops this key from the thread ownership stack when the admission ends.
struct ReleaseKey(PathBuf);

impl Drop for ReleaseKey {
    fn drop(&mut self) {
        HELD.with(|held| {
            let mut held = held.borrow_mut();
            if let Some(position) = held.iter().rposition(|key| *key == self.0) {
                held.remove(position);
            }
        });
    }
}

/// Takes both layers for one path and runs the operation under them.
///
/// The two guards are locals of this single function on purpose: the in-process MutexGuard borrows
/// the Arc it came from, so neither can be moved into a struct that also owns the other. Keeping
/// them here is also what makes the pair ONE admission - a failure to take the cross-process lock
/// returns before HELD is marked, so nothing is ever recorded as held that is not.
fn with_session_gate<T>(
    path: &Path,
    operation: impl FnOnce() -> Result<T, IpcError>,
) -> Result<T, IpcError> {
    let key = lock_key(path);
    // Reentrant: this thread already holds the entry, so both layers are already ours.
    if HELD.with(|held| held.borrow().contains(&key)) {
        return operation();
    }
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let process_mutex = {
        let mut locks = LOCKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::clone(locks.entry(key.clone()).or_default())
    };
    let _process_guard = process_mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Taken while the in-process mutex is held, and NOT swallowed: a lock that cannot be taken is
    // returned to the caller, which is what makes a failure a typed error instead of a silent
    // unprotected run.
    let _file_lock = FileLock::acquire(&key)?;
    HELD.with(|held| held.borrow_mut().push(key.clone()));
    let _release = ReleaseKey(key);
    operation()
}

/// The identity a session path is locked under.
///
/// `canonicalize` needs the file to exist, and a session file often does not; canonicalizing the
/// PARENT and re-joining the file name resolves a relative spelling or a symlinked directory while
/// staying usable before the first save. When even the parent cannot be resolved the raw path is
/// used, which is still correct for a single spelling.
fn lock_key(path: &Path) -> PathBuf {
    // The key must be stable for the whole life of the entry, because one call chain computes it
    // more than once: BEFORE the first save creates the file, WHILE the path may be a symlink, and
    // AFTER a publish replaces that symlink with a regular file. Resolving the path itself would
    // give those three states three different keys, so a nested caller would miss the outer hold
    // and block on the non-reentrant mutex - and two processes could exclude on different locks.
    //
    // Identity therefore comes from the PARENT only, with the final component kept verbatim: the
    // directory entry being replaced is the shared thing, and resolving the last name would follow
    // a session-file symlink to its TARGET - a different key before and after a save replaces that
    // link, which is precisely the instability this key exists to prevent. The parent may not exist
    // yet on a first run, so the deepest EXISTING ancestor is resolved and the remaining names are
    // re-attached, which also folds a relative spelling into the same key.
    // A path with no final component names no entry, so it keys only itself.
    if path.file_name().is_none() {
        return path.to_path_buf();
    }
    // Order matters: a ".." that follows a symlink must be resolved through that link, because
    // "/base/link/../session.json" with "link" -> "/other/child" names "/other/session.json", not
    // "/base/session.json". So the LONGEST EXISTING prefix is canonicalised first (which follows
    // symlinks), and only the non-existent remainder is folded lexically - pure string work that
    // cannot fail. Folding first would key two spellings of one entry to two different locks.
    // The split stops ONE component short of the end, so the final name is never resolved through
    // a symlink: a save replaces that very name, and the key must survive the replacement.
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(_) => path.to_path_buf(),
        }
    };
    let components: Vec<std::path::Component> = absolute.components().collect();
    let mut split = components.len() - 1;
    let mut key = PathBuf::new();
    while split > 0 {
        let prefix: PathBuf = components[..split].iter().collect();
        match std::fs::canonicalize(&prefix) {
            Ok(resolved) => {
                key = resolved;
                break;
            }
            Err(_) => split -= 1,
        }
    }
    if split == 0 {
        // Nothing on the path could be resolved: keep the absolute spelling, folded.
        key = PathBuf::new();
    }
    for component in &components[split..] {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                // Never fold past the root: "/../s.json" is "/s.json", and popping the root would
                // turn it into the RELATIVE key "s.json", which locks a different file.
                if key.parent().is_some() {
                    key.pop();
                }
            }
            std::path::Component::Normal(name) => key.push(name),
            std::path::Component::RootDir => key.push("/"),
            std::path::Component::Prefix(prefix) => key.push(prefix.as_os_str()),
        }
    }
    key
}

/// An exclusive advisory lock on the session's sibling lock file, released on drop.
struct FileLock {
    file: std::fs::File,
}

impl FileLock {
    // Acquires the cross-process lock, or fails. A lock that cannot be taken must not let the
    // caller proceed unprotected: with a lock file occupied by a directory, two processes could
    // each classify the entry and one could carry off the other process fresh published session.
    fn acquire(path: &Path) -> Result<Self, IpcError> {
        let lock_path = path.with_extension("json.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|error| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!(
                        "Cannot open the session lock file {}: {error}",
                        lock_path.display()
                    ),
                )
            })?;
        file.lock().map_err(|error| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!(
                    "Cannot lock the session lock file {}: {error}",
                    lock_path.display()
                ),
            )
        })?;
        Ok(Self { file })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Best effort: the process is exiting the critical section either way, and a failure to
        // unlock cannot be reported from a destructor.
        let _ = self.file.unlock();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedWorktree {
    pub path: String,
    pub branch: String,
    pub head: String,
    #[serde(default)]
    pub is_main: bool,
    #[serde(default)]
    pub is_locked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTerminalTabState {
    pub primary_session_id: String,
    pub pane_tree: serde_json::Value,
    pub session_ids_by_leaf_id: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_leaf_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_leaf_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedBrowserTabState {
    pub browser_id: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loading: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_go_back: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_go_forward: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "profile")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTabGroup {
    pub id: String,
    pub tab_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTab {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<PersistedTerminalTabState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<PersistedBrowserTabState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,

    // v1 compatibility fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_tree: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ids_by_leaf_id: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_leaf_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_leaf_id: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedLayout {
    pub split_mode: String,
    #[serde(default)]
    pub primary_tab_id: Option<String>,
    #[serde(default)]
    pub secondary_tab_id: Option<String>,
    #[serde(default)]
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub tabs: Vec<PersistedTab>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_groups: Option<Vec<PersistedTabGroup>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_group_layout: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layouts_by_tab_id: Option<serde_json::Value>,
    #[serde(default, flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTerminalSession {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_session_id: Option<String>,
    #[serde(default)]
    pub worktree_path: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_scrollback: Option<String>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default, flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedWorkspace {
    pub workspace_id: String,
    pub repo_root: PathBuf,
    #[serde(default)]
    pub worktrees: Vec<PersistedWorktree>,
    #[serde(default)]
    pub active_worktree_path: Option<String>,
    pub layout: PersistedLayout,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_layouts: Option<HashMap<String, PersistedLayout>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_by_worktree: Option<serde_json::Value>,
    #[serde(default)]
    pub terminal_sessions: HashMap<String, PersistedTerminalSession>,
    #[serde(default, flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedWorkspaceSession {
    pub version: u32,
    pub timestamp: u64,
    pub active_workspace_id: String,
    pub workspaces: HashMap<String, PersistedWorkspace>,
    #[serde(default, flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Unique suffix per write attempt (pid + epoch nanos + counter). A shared fixed
/// temporary filename let concurrent writers (multi-daemon handover, GUI + daemon)
/// interleave create/truncate/write on one inode and publish a mixed document.
fn unique_write_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{nanos}-{counter}", std::process::id())
}

pub fn save_session_to_path(
    path: &Path,
    session: &PersistedWorkspaceSession,
) -> Result<(), IpcError> {
    // The lock is taken HERE, not only at the IPC call sites: the daemon writes session files
    // directly on this same path, and two processes that classify the entry and then act on it
    // can each displace the other's fresh publish. Reentrant, so an IPC caller that already holds
    // it pays nothing.
    // The parent must exist before the LOCK file is created in it: on a first run the session
    // directory does not exist yet, and a lock open would fail with NotFound before the
    // directory-creation inside could ever run, so no session would ever be saved.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to create directory for session state: {}", e),
            )
        })?;
    }
    try_with_session_path_lock(path, || save_session_to_path_locked(path, session))
}

fn save_session_to_path_locked(
    path: &Path,
    session: &PersistedWorkspaceSession,
) -> Result<(), IpcError> {
    // No parent creation here: the wrapper creates it before taking the lock, and the lock file
    // itself lives in that directory, so reaching this point proves the parent exists.
    let write_suffix = unique_write_suffix();

    // `symlink_metadata` rather than `exists`/`try_exists`: both follow symlinks, so a DANGLING
    // symlink reports "absent" and the atomic publish below would silently replace the link itself
    // with no backup. The directory entry is what must be preserved, whether or not its target
    // exists, and a path this process cannot stat at all must also count as present-but-unusable.
    // A symlink is replaced by the publish below (an atomic rename always replaces the link, never
    // its target), so the LINK ITSELF is backed up: recording only the target path as text left
    // the configured name pointing at a regular file afterwards, with the layout reachable only
    // through a note. Recreating the link preserves the entry exactly - a dangling target stays
    // dangling, and a dotfile-managed layout stays a link the user's manager still owns. If the
    // link cannot be backed up, the save stops here and the link is left exactly as it was.
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let target = std::fs::read_link(path).map_err(|error| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Failed to read the session symlink target: {error}"),
                )
            })?;
            let backup = existing_or_new_symlink_backup(path, &write_suffix, &target).map_err(
                |error| {
                    IpcError::new(
                        IpcErrorCode::IoError,
                        format!(
                            "Refusing to replace a session symlink without backing it up: {error}"
                        ),
                    )
                },
            )?;
            tracing::debug!(path = %path.display(), backup = %backup.display(), "Backed up a session symlink before replacing it");
        }
        _ => {}
    }
    // A shape that cannot be hard-linked (a directory, a filesystem without hard links) must be
    // moved, but moving it before the replacement exists leaves the canonical path ABSENT if the
    // publish then fails - a healthy document stranded in a backup. The move is therefore deferred
    // to the instant before the publish, and undone if that publish fails.
    let mut deferred_move: Option<(PathBuf, PathBuf)> = None;
    if std::fs::symlink_metadata(path).is_ok() {
        // Every branch below may exit early through this label once the unreadable or
        // versionless file has been moved aside, so the publish code below still runs.
        'preserved: {
        // A path that cannot be READ at all (a directory left behind by a crash, an ACL that
        // denies reading) would otherwise abort every future save forever. Move it aside and
        // publish, exactly as for unreadable bytes.
        let original = match read_regular_file(path) {
            EntryRead::Bytes(bytes) => bytes,
            // A FIFO, socket or device at this path is not a session and reading it can block
            // forever. It must not stay at the path, but it must also not be DISPLACED: a rename
            // by pathname would move whatever another process published there in between. Linking
            // copies the directory entry and leaves the path intact until this save's own atomic
            // publish replaces it.
            EntryRead::NotRegular => {
                // A FIFO, socket, symlink or device is not a session, and the atomic publish below
                // replaces the DIRECTORY ENTRY itself. Nothing is moved: the path stays occupied
                // until this save's own rename, so no other process's publish can be carried off.
                //
                // It must still be preserved, because that publish destroys the entry. A hard link
                // copies the entry without reading it (so a FIFO cannot block) and is no-replace by
                // definition. Only a shape that cannot be linked - a directory always, and any
                // entry on a filesystem without hard links - is moved, and every move here is
                // immediately followed by this save's own publish under the same lock.
                let backup = path.with_extension(format!("json.corrupted-{write_suffix}"));
                match std::fs::hard_link(path, &backup) {
                    Ok(()) => {
                        // The link shares the entry's inode, so no bytes were copied - but the
                        // backup NAME is not durable until its directory is synced, and the publish
                        // below replaces the canonical entry. A name a crash could drop is not a
                        // backup, so a failure here aborts the save.
                        // The inode itself is deliberately NOT opened: these entry types carry no
                        // session bytes to flush, and opening a FIFO or socket would block.
                        sync_entry_directory(&backup).map_err(|io| {
                            IpcError::new(
                                IpcErrorCode::IoError,
                                format!(
                                    "Cannot replace a non-regular session path; making its backup durable failed: {io}"
                                ),
                            )
                        })?;
                        tracing::warn!(
                            backup = %backup.display(),
                            "Non-regular session path preserved before replacement"
                        );
                    }
                    // Cannot be linked: a directory, or a filesystem without hard links. Defer the
                    // move to just before the publish so the path is never left empty by a save
                    // that then fails.
                    Err(_) => deferred_move = Some((path.to_path_buf(), backup)),
                }
                break 'preserved;
            }
            EntryRead::Failed(e) => {
                let backup = path.with_extension(format!("json.corrupted-{write_suffix}"));
                // Nothing could be read, so no BYTES exist to copy: the directory entry is what is
                // preserved. Link first (no-replace by definition, keeps the canonical name
                // occupied); only a shape that cannot be linked is moved, and that move is
                // deferred to just before the publish so a failed publish cannot leave the path
                // empty.
                match std::fs::hard_link(path, &backup) {
                    Ok(()) => {
                        // A hard link shares the original inode, so nothing had to be copied - but
                        // sharing an inode is not the same as that inode being durable. Its data may
                        // still be dirty in the page cache, and the NAME is not durable until the
                        // directory is synced. Both are established here, before the publish below
                        // replaces the canonical name, and either failure aborts the save: a backup
                        // that a crash could drop is not a backup.
                        // The link shares the original inode, so its data is flushed through the
                        // backup name (permission denials excepted), and then the NAME itself is
                        // made durable before the publish below replaces the canonical entry.
                        sync_preserved_inode(&backup)
                            .and_then(|()| sync_entry_directory(&backup))
                            .map_err(|io| {
                            IpcError::new(
                                IpcErrorCode::IoError,
                                format!(
                                    "Cannot overwrite an unreadable session; making its backup                                      durable failed: {io}"
                                ),
                            )
                        })?;
                        tracing::warn!(
                            backup = %backup.display(),
                            error = %e,
                            "Unreadable session path preserved before replacement"
                        );
                    }
                    Err(_) => deferred_move = Some((path.to_path_buf(), backup)),
                }
                break 'preserved;
            }
        };
        // Only the schema version decides migration; every other field is skipped unparsed.
        #[derive(Deserialize)]
        struct VersionOnly {
            #[serde(default)]
            version: Option<serde_json::Value>,
        }
        match serde_json::from_slice::<VersionOnly>(&original) {
            Ok(previous) => {
                // Well-formed JSON with no usable version is not a session at all: `{}`, `[]` or a
                // foreign document. Treating it as an error would make EVERY later save fail for
                // as long as those bytes stayed at the path, so a recovered-but-unwritable layout
                // would be lost on the next restart. Preserve the bytes and replace them instead.
                let Some(version) = previous.version.as_ref().and_then(serde_json::Value::as_u64)
                else {
                    let backup =
                        preserve_read_bytes(path, Some(&write_suffix), &original).map_err(|io| {
                            IpcError::new(
                                IpcErrorCode::IoError,
                                format!(
                                    "Cannot overwrite a session file with no version; preserving it failed: {io}"
                                ),
                            )
                        })?;
                    tracing::warn!(
                        backup = %backup.display(),
                        "Session state file without a version preserved"
                    );
                    break 'preserved;
                };
                if version > u64::from(session.version) {
                    // A newer version is only a downgrade refusal if the document is actually a
                    // session. `{"version":999}` is not, and refusing forever would leave those
                    // bytes at the path making EVERY later save fail, so a rebuilt layout could
                    // never persist. Preserve it and replace it like any other non-session.
                    if serde_json::from_slice::<PersistedWorkspaceSession>(&original).is_err() {
                        let backup =
                            preserve_read_bytes(path, Some(&write_suffix), &original).map_err(|io| {
                                IpcError::new(
                                    IpcErrorCode::IoError,
                                    format!(
                                        "Cannot overwrite a non-session document claiming version {version}; preserving it failed: {io}"
                                    ),
                                )
                            })?;
                        tracing::warn!(
                            backup = %backup.display(),
                            version,
                            "Non-session document with a future version preserved"
                        );
                        break 'preserved;
                    }
                    return Err(IpcError::new(
                        IpcErrorCode::ParseError,
                        "Refusing session schema downgrade",
                    ));
                }
                // A current-or-older version does not establish a session either. Without this the
                // publish below replaces a damaged document (`{"version":3,"workspaces":"damaged"}`)
                // with no backup of its original bytes. Preservation is skipped when the document
                // already parses as a valid session, which is the only case that needs no backup.
                if serde_json::from_slice::<PersistedWorkspaceSession>(&original).is_err() {
                    let backup =
                        preserve_read_bytes(path, Some(&write_suffix), &original).map_err(|io| {
                            IpcError::new(
                                IpcErrorCode::IoError,
                                format!(
                                    "Cannot overwrite a damaged session; preserving it failed: {io}"
                                ),
                            )
                        })?;
                    tracing::warn!(
                        backup = %backup.display(),
                        version,
                        "Damaged session document preserved before replacement"
                    );
                    break 'preserved;
                }
                if version < 3 && session.version >= 3 {
                    let mut backup = path.with_extension("json.pre-v3");
                    // A DIRECTORY at the backup name is a conflict this save cannot resolve: it
                    // neither holds the document nor can be replaced, and silently taking
                    // `.pre-v3-1` instead would report success while the promised backup name is
                    // unusable. The contract test pins that as an error, so it is one.
                    if std::fs::symlink_metadata(&backup)
                        .map(|metadata| metadata.is_dir())
                        .unwrap_or(false)
                    {
                        return Err(IpcError::new(
                            IpcErrorCode::IoError,
                            format!(
                                "A directory occupies the pre-v3 backup name {}",
                                backup.display()
                            ),
                        ));
                    }
                    // Linking the old inode publishes a complete snapshot without clobbering an
                    // earlier backup. Atomic replacement below never mutates that inode.
                    match fs::hard_link(path, &backup) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                            // An older build can write a legacy file again after an earlier
                            // upgrade left its backup. That backup is kept as it is, and this
                            // legacy file gets its own; comparing it with the old backup would
                            // refuse every later save.
                            if fs::read(&backup).map_or(true, |kept| kept != original) {
                                backup = path.with_extension(format!("json.pre-v3-{write_suffix}"));
                                fs::hard_link(path, &backup).map_err(|e| {
                                    IpcError::new(
                                        IpcErrorCode::IoError,
                                        format!("Failed to back up pre-v3 session: {e}"),
                                    )
                                })?;
                            }
                        }
                        Err(e) => {
                            return Err(IpcError::new(
                                IpcErrorCode::IoError,
                                format!("Failed to back up pre-v3 session: {e}"),
                            ))
                        }
                    }
                    // A DIFFERENT legacy document may arrive later (an older daemon or build still
                    // running, or a restored file). The existing backup then describes a session
                    // that is no longer the one being upgraded, and refusing forever would wedge
                    // every save, so each distinct legacy document gets its own backup name.
                    let backup_already_describes_this_document = matches!(
                        read_regular_file(&backup),
                        EntryRead::Bytes(existing) if existing == original
                    );
                    let backup = if backup_already_describes_this_document {
                        backup
                    } else {
                        // Another process may claim the same free name between the scan and the
                        // link; that is a lost race, not a reason to fail this save, so the next
                        // free slot is tried.
                        // One link per attempt: the link IS the claim, so a successful link
                        // already produced the backup and must not be repeated. Running out of
                        // numbered names must not wedge every later save either — an older
                        // process can keep publishing new legacy documents indefinitely, and
                        // each distinct one deserves a backup — so a unique name is used rather
                        // than failing the save the user asked for.
                        let mut claimed = None;
                        for attempt in 0..9 {
                            let candidate = match first_free_pre_v3_backup(path) {
                                Some(free) if attempt < 8 => free,
                                _ => path.with_extension(format!(
                                    "json.pre-v3-{}",
                                    uuid::Uuid::new_v4()
                                )),
                            };
                            match fs::hard_link(path, &candidate) {
                                Ok(()) => {
                                    claimed = Some(candidate);
                                    break;
                                }
                                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                                Err(e) => {
                                    return Err(IpcError::new(
                                        IpcErrorCode::IoError,
                                        format!("Failed to back up pre-v3 session: {e}"),
                                    ))
                                }
                            }
                        }
                        let Some(claimed) = claimed else {
                            return Err(IpcError::new(
                                IpcErrorCode::IoError,
                                "Failed to back up pre-v3 session",
                            ));
                        };
                        claimed
                    };
                    // The backup name can hold a FIFO or an unreadable entry left by an earlier
                    // run; a plain read would block here forever while the session path lock is
                    // held, so every later load and save would hang too.
                    let verified = match read_regular_file(&backup) {
                        EntryRead::Bytes(bytes) => bytes,
                        EntryRead::NotRegular | EntryRead::Failed(_) => {
                            return Err(IpcError::new(
                                IpcErrorCode::IoError,
                                format!(
                                    "The pre-v3 backup at {} is not a readable regular file",
                                    backup.display()
                                ),
                            ))
                        }
                    };
                    if verified != original {
                        return Err(IpcError::new(
                            IpcErrorCode::IoError,
                            "Pre-v3 backup differs from current legacy session",
                        ));
                    }
                    // FlushFileBuffers (sync_all on Windows) needs a handle with write access; a
                    // read-only open fails with ERROR_ACCESS_DENIED (Windows QA run-q11).
                    fs::OpenOptions::new()
                        .read(true)
                        .write(cfg!(windows))
                        .open(&backup)
                        .and_then(|file| file.sync_all())
                        .map_err(|e| {
                            IpcError::new(
                                IpcErrorCode::IoError,
                                format!("Failed to sync pre-v3 backup: {e}"),
                            )
                        })?;
                    // A directory cannot be opened as a file on Windows (File::open fails), so the
                    // directory-entry fsync is Unix-only, as in session_service.rs's publish; the
                    // backup bytes themselves were synced above on every platform.
                    #[cfg(unix)]
                    {
                        let parent = entry_parent(path);
                        File::open(parent)
                            .and_then(|dir| dir.sync_all())
                            .map_err(|e| {
                                IpcError::new(
                                    IpcErrorCode::IoError,
                                    format!("Failed to sync backup directory: {e}"),
                                )
                            })?;
                    }
                }
            }
            // Well-formed JSON of the wrong shape (no object) is not a session either. It used to
            // be reported as a version error, which left the bytes at the path and silently made
            // every later save fail. Preserve and replace, exactly as for unreadable bytes.
            Err(e) if e.classify() == serde_json::error::Category::Data => {
                let backup =
                    preserve_read_bytes(path, Some(&write_suffix), &original).map_err(|io| {
                        IpcError::new(
                            IpcErrorCode::IoError,
                            format!(
                                "Cannot overwrite a non-session document ({e}); preserving it failed: {io}"
                            ),
                        )
                    })?;
                tracing::warn!(
                    backup = %backup.display(),
                    error = %e,
                    "Non-session document at the session path preserved"
                );
                break 'preserved;
            }
            Err(e) => {
                // Unreadable bytes (mixed/truncated publish, disk fault) must not block
                // every future save: all writers publish complete snapshots, so nothing
                // here can merge with them. Preserve the unreadable content, then replace.
                let backup =
                    preserve_read_bytes(path, Some(&write_suffix), &original).map_err(|io| {
                        IpcError::new(
                            IpcErrorCode::IoError,
                            format!(
                                "Cannot overwrite unreadable session ({e}); preserving it failed: {io}"
                            ),
                        )
                    })?;
                tracing::warn!(
                    backup = %backup.display(),
                    error = %e,
                    "Unreadable session state file preserved"
                );
                break 'preserved;
            }
        }
    }

    }

    let serialized = serde_json::to_string(session).map_err(|e| {
        IpcError::new(
            IpcErrorCode::ParseError,
            format!("Failed to serialize session state: {}", e),
        )
    })?;

    // A deferred move (if any) is performed at the publish step below, not here: the replacement is
    // written and synced first, so the canonical path is only touched once a complete new document
    // exists. If the write fails, `undo_deferred_move` puts the entry back.

    let tmp_path = path.with_extension(format!("json.tmp-{write_suffix}"));

    // Durable atomic write: write -> sync_all -> rename -> parent fsync. A unique
    // temporary inode per attempt keeps concurrent publishers from interleaving.
    {
        let write_result: Result<(), IpcError> = (|| {
            let mut file = File::create(&tmp_path).map_err(|e| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Failed to create temporary session file: {}", e),
                )
            })?;
            file.write_all(serialized.as_bytes()).map_err(|e| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Failed to write session state: {}", e),
                )
            })?;
            file.sync_all().map_err(|e| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Failed to fsync temporary session file: {}", e),
                )
            })?;
            Ok(())
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&tmp_path);
            undo_deferred_move(&mut deferred_move);
            return Err(error);
        }
    }

    // Publish. Moving the occupant aside and THEN renaming the replacement onto it left the
    // canonical path ABSENT between the two calls, so a crash there stranded the document under a
    // backup name with nothing at the real path - and a destructor cannot run after a crash. Where
    // the platform can exchange two entries atomically, the publish IS the swap: the path goes
    // straight from the old entry to the new document, so it is never empty.
    let swapped = match deferred_move.as_ref() {
        Some((_, intended)) => {
            let intended = intended.clone();
            match exchange_paths(&tmp_path, path) {
                Ok(()) => {
                    // The canonical path now names the new document, and the temporary name holds
                    // whatever was there before. That swap is reversible, so the old entry's
                    // durability is established BEFORE the save is allowed to stand: if the
                    // directory cannot be synced, the swap is undone and the previous entry goes
                    // back to the canonical path. Publishing first and syncing afterwards would
                    // leave the old entry findable only by luck.
                    // The entry just swapped into the temporary name may be an UNREADABLE regular
                    // file whose data was never durable, so its bytes are flushed through that name
                    // before the publish is allowed to stand - the name is the only handle left.
                    let durable = sync_preserved_inode(&tmp_path)
                        .and_then(|()| sync_entry_directory(path))
                        .map_err(|io| {
                            IpcError::new(
                                IpcErrorCode::IoError,
                                format!(
                                    "Cannot make the preserved session entry durable; the previous                                      entry was restored at {}: {io}",
                                    path.display()
                                ),
                            )
                        });
                    if let Err(failure) = durable {
                        // Undo: exchange again returns the old entry to the canonical path and
                        // leaves the new document under the temporary name, which is then removed.
                        if exchange_paths(&tmp_path, path).is_ok() {
                            let _ = std::fs::remove_file(&tmp_path);
                            return Err(failure);
                        }
                        // The undo failed too: both entries are on disk, but which name holds which
                        // is no longer certain, so report it rather than guess.
                        return Err(IpcError::new(
                            IpcErrorCode::IoError,
                            format!(
                                "Cannot make the preserved session entry durable and the swap could                                  not be undone; entries remain at {} and {} ({})",
                                path.display(),
                                tmp_path.display(),
                                failure.message
                            ),
                        ));
                    }
                    match rename_no_replace(&tmp_path, &intended) {
                        Ok(()) => {
                            // The name the caller reports must itself survive a crash: the rename
                            // only changes which name the entry has, so the entry is durably on
                            // disk under the TEMPORARY name until this directory sync lands. A
                            // crash before it can recover `tmp_path`, not `intended`, so the sync
                            // has to happen before the backup name is announced.
                            if let Err(error) = sync_entry_directory(&intended) {
                                // The rename already succeeded, so the entry is at `intended` RIGHT
                                // NOW; only its NAME is not yet promised to survive. Reporting the
                                // temporary name here would send the person to a path that no
                                // longer exists, so the durable name is named and the missing
                                // promise is what the message says.
                                tracing::error!(
                                    backup = %intended.display(),
                                    %error,
                                    "Old session entry is at its backup name, but that name is not durable"
                                );
                            } else {
                                tracing::warn!(
                                    backup = %intended.display(),
                                    "Non-regular session path moved aside for the publish"
                                );
                            }
                        }
                        // The old entry is durable on disk under the temporary name: nothing was
                        // destroyed, so say exactly where it stayed instead of implying it reached
                        // the backup name.
                        Err(io) => tracing::error!(
                            kept_at = %tmp_path.display(),
                            intended = %intended.display(),
                            %io,
                            "Old session entry could not be moved to its backup name"
                        ),
                    }
                    deferred_move = None;
                    true
                }
                // No atomic exchange here (another filesystem, or a platform without one): fall
                // back to the two steps, which is the best that can be done there.
                Err(_) => false,
            }
        }
        None => false,
    };

    if !swapped {
        // No atomic exchange here. Moving the occupant aside first and publishing after would leave
        // the canonical path ABSENT for the whole write, so a crash in that window loses the
        // document even though a healthy copy existed. Refuse instead: the previous session stays
        // intact at its original name and the failure is reported rather than hidden.
        if deferred_move.is_some() {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(IpcError::new(
                IpcErrorCode::IoError,
                format!(
                    "Refusing to replace a non-regular entry at {} without an atomic exchange; \
                     the existing entry was left in place",
                    path.display()
                ),
            ));
        }
        fs::rename(&tmp_path, path).map_err(|e| {
            let _ = fs::remove_file(&tmp_path);
            undo_deferred_move(&mut deferred_move);
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to atomically rename session state file: {}", e),
            )
        })?;
    }

    // Parent directory fsync. The new document is already published, so a failure here cannot be
    // turned into an error without misreporting a successful save; it is logged instead.
    if let Err(error) = sync_entry_directory(path) {
        tracing::warn!(%error, path = %path.display(), "Failed to sync the session directory");
    }

    Ok(())
}

pub fn load_session_from_path(path: &Path) -> Result<Option<PersistedWorkspaceSession>, IpcError> {
    Ok(load_session_reporting_corruption(path)?.0)
}

/// Same load, but also answers *why* there is no session when there is none.
///
/// A file that exists and cannot be parsed is quarantined and its backup path is returned, so the
/// caller can tell the person where their layout went instead of silently booting empty - the two
/// outcomes used to be the same `None`.
#[allow(clippy::type_complexity)]
pub fn load_session_reporting_corruption(
    path: &Path,
) -> Result<(Option<PersistedWorkspaceSession>, Option<PathBuf>), IpcError> {
    // A first run has no session directory, so the lock file cannot even be opened. That is the
    // ABSENT case, not a lock failure: there is nothing to read and no publisher to race. This is
    // the only carve-out - an existing directory whose lock cannot be taken still fails closed.
    // The probe FOLLOWS symlinks on purpose. A session directory reached through a symlink is a
    // legitimate layout, and save writes through it; classifying it as "not a directory" here made
    // load report an absent session while save still wrote, so the app booted empty and the next
    // save replaced the person's real layout with the empty snapshot.
    // A bare relative name like "session_state.json" has an EMPTY parent, which means the current
    // directory rather than nowhere; probing the empty path returns NotFound and load would report
    // an absent session for a file save happily writes.
    let parent = entry_parent(path);
    match std::fs::metadata(parent) {
        Ok(meta) if meta.is_dir() => {}
        // An existing parent that is genuinely not a directory can hold no file, so absent is the
        // truth there; save refuses in the same case.
        Ok(_) => return Ok((None, None)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((None, None)),
        Err(_) => {}
    }
    // A lock that cannot be taken is an ERROR on this path too: a lock-free quarantine could carry
    // off a publisher's fresh document, and no in-process probe can prove that a directory has no
    // concurrent writer.
    //
    // The one exception is a directory this process cannot WRITE at all, such as a read-only mount
    // holding the session file. The base code read that file without any lock, so failing the whole
    // load there booted an empty window over a perfectly good layout. The lock-free read may only
    // RETURN a parsed document; anything that would mutate the path still reports the lock error,
    // because that write is exactly what the lock exists to serialize.
    match try_with_session_path_lock(path, || load_session_reporting_corruption_locked(path)) {
        Ok(result) => Ok(result),
        Err(lock_error) if !parent_is_writable(path) => match load_session_read_only(path) {
            Some(session) => Ok((Some(session), None)),
            None => Err(lock_error),
        },
        Err(error) => Err(error),
    }
}

/// Whether a lock file could be created next to `path`.
///
/// This decides the read-only carve-out in `load_session_reporting_corruption` without inspecting
/// an error message: the probe either creates and removes a uniquely named file in the directory or
/// it does not.
fn parent_is_writable(path: &Path) -> bool {
    let parent = entry_parent(path);
    let probe = parent.join(format!(".ferryx-lock-probe-{}", uuid::Uuid::new_v4()));
    match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        // The name is taken, so the directory accepted a create: it is writable.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => true,
        Err(_) => false,
    }
}

/// Reads a session without taking any lock, and without ever mutating the path.
///
/// Used only when the containing directory cannot hold a lock file. Quarantining is not available
/// here: it writes next to the document, which is the operation the lock would have protected.
fn load_session_read_only(path: &Path) -> Option<PersistedWorkspaceSession> {
    match read_regular_file(path) {
        EntryRead::Bytes(bytes) => serde_json::from_slice::<PersistedWorkspaceSession>(&bytes)
            .ok()
            .filter(|session| (1..=3).contains(&session.version)),
        EntryRead::NotRegular | EntryRead::Failed(_) => None,
    }
}

fn load_session_reporting_corruption_locked(
    path: &Path,
) -> Result<(Option<PersistedWorkspaceSession>, Option<PathBuf>), IpcError> {
    // Only a genuinely ABSENT path means "no session". Any other metadata failure (EACCES on a
    // parent directory, ELOOP) means something may be there that this process cannot classify, and
    // reporting that as absent would silently boot an empty window with no backup and no notice.
    match std::fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((None, None)),
        Err(error) => {
            return Err(IpcError::new(
                IpcErrorCode::IoError,
                format!(
                    "The session path {} exists but cannot be inspected: {error}",
                    path.display()
                ),
            ))
        }
    }

    // Read BYTES and parse those bytes: invalid UTF-8 (a damaged document, a torn write) must go
    // through the same preservation and notification path as malformed JSON. `read_to_string` used
    // to fail here and return an error the caller turned into an empty window, so the layout was
    // lost with no backup and no notice. Parsing the raw bytes also means a corrupt byte can never
    // be laundered into a valid document by lossy decoding: `from_slice` rejects invalid UTF-8.
    let content = match read_regular_file(path) {
        EntryRead::Bytes(bytes) => bytes,
        // A FIFO or device here is corruption like any other: preserve and report it rather than
        // blocking on a read that never returns.
        EntryRead::NotRegular => {
            let backup = match preserve_corrupt_session(path)? {
                Preservation::Recovered(session) => return Ok((Some(*session), None)),
                Preservation::Quarantined(backup) => backup,
            };
            eprintln!(
                "Warning: The session path was not a regular file, copied to {}.",
                backup.display()
            );
            return Ok((None, Some(backup)));
        }
        EntryRead::Failed(e) => {
            return Err(IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to read session state file: {}", e),
            ))
        }
    };

    match serde_json::from_slice::<PersistedWorkspaceSession>(&content) {
        Ok(session) => {
            if !(1..=3).contains(&session.version) {
                eprintln!(
                    "Warning: Unsupported session version {}, ignoring.",
                    session.version
                );
                return Ok((None, None));
            }
            Ok((Some(session), None))
        }
        Err(err) => {
            // Never overwrite an earlier quarantine: two corruptions in a row used to
            // replace the first backup, destroying the only recoverable copy of the
            // layout the user still had. A failed preservation is an error, not a silent
            // claim that the file was kept somewhere it is not.
            // Preserve the EXACT bytes that failed to parse. Re-reading the path here would let a
            // concurrent publisher swap in a healthy (or differently corrupt) document between
            // the parse and the copy, so the bytes this call classified would be lost while the
            // backup held something else.
            // A quarantine that cannot be written is an ERROR, not a claim: reporting a backup
            // path that does not exist would send the person looking for a copy that was never
            // made. The original is never moved here, so it is still intact at its own path.
            let backup_path = preserve_read_bytes(path, None, &content).map_err(|io| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Failed to preserve the corrupt session document: {io}"),
                )
            })?;
            eprintln!(
                "Warning: Session state file was corrupted ({}), copied to {}.",
                err,
                backup_path.display()
            );
            Ok((None, Some(backup_path)))
        }
    }
}

/// Every candidate quarantine name for `path`, in preference order: the plain `.corrupted` name,
/// then `.corrupted.1`, `.corrupted.2`, ... A caller must claim one atomically before writing it;
/// a plain rename can replace an existing file, which would destroy the earliest and best copy.
fn quarantine_paths(path: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    quarantine_paths_up_to(path, QUARANTINE_SLOT_LIMIT)
}

/// Upper bound on numbered quarantine slots. Exhausting them is a hard error, never a fallback.
const QUARANTINE_SLOT_LIMIT: u32 = 10_000;

/// The candidate names, bounded so a test can exhaust them without creating ten thousand files.
fn quarantine_paths_up_to(path: &Path, slots: u32) -> impl Iterator<Item = PathBuf> + '_ {
    let base = path.with_extension("json.corrupted");
    let base_name = base.as_os_str().to_os_string();
    std::iter::once(base).chain((1..slots).map(move |index| {
        // Append the suffix to the OsString so a non-Unicode file name is not lossily turned into a
        // different path by display formatting.
        let mut candidate = base_name.clone();
        candidate.push(format!(".{index}"));
        PathBuf::from(candidate)
    }))
}

/// Atomically claims a fresh quarantine name, returning the created placeholder file.
///
/// `create_new` is the reservation: the kernel fails it when the name exists, so two processes (or
/// two threads) quarantining at once cannot both believe they own the same slot, and no existing
/// backup is ever replaced. Checking `exists()` first and renaming afterwards is a check-then-act
/// race in which the other writer's backup, or a session saved in between, is destroyed.
fn reserve_quarantine_path(path: &Path) -> Result<(PathBuf, File), IpcError> {
    reserve_quarantine_path_within(path, QUARANTINE_SLOT_LIMIT)
}

fn reserve_quarantine_path_within(path: &Path, slots: u32) -> Result<(PathBuf, File), IpcError> {
    for candidate in quarantine_paths_up_to(path, slots) {
        match OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(IpcError::new(
                    IpcErrorCode::IoError,
                    format!(
                        "Failed to reserve a backup name at {}: {error}",
                        candidate.display()
                    ),
                ));
            }
        }
    }
    Err(IpcError::new(
        IpcErrorCode::IoError,
        "Refused to quarantine a corrupt session file: every backup name is taken",
    ))
}

/// What reading a session path actually produced.
enum EntryRead {
    /// The bytes of a regular file.
    Bytes(Vec<u8>),
    /// The path holds something that is not a regular file: a directory, a FIFO, a socket or a
    /// device. None of these is a session, and reading one can block with no timeout.
    NotRegular,
    /// The entry could not be opened or read.
    Failed(std::io::Error),
}

/// Reads the session path without ever blocking, and lets the OPENED HANDLE decide the file type.
///
/// A plain `fs::read` is unsafe here for two separate reasons. First, opening a writer-less FIFO
/// blocks indefinitely and never reaches EOF, which hangs startup and holds the session path lock
/// so no later save can ever complete. Second, a pathname check followed by a read is two
/// operations: the entry can be swapped in between, so only metadata taken from the handle that was
/// actually opened tells the truth about what is being read.
#[cfg(unix)]
fn read_regular_file(path: &Path) -> EntryRead {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;

    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return EntryRead::Failed(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the session path contains a NUL byte",
        ));
    };
    // O_NONBLOCK is what makes a FIFO open return immediately; a regular file ignores it.
    let fd = unsafe {
        libc::open(
            c_path.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        // Some entries cannot be opened at all, so the handle never gets a chance to say it is not
        // a regular file: a unix socket returns ENXIO on Linux and EOPNOTSUPP on macOS, and a
        // directory on Windows returns ERROR_ACCESS_DENIED. Without this, LOAD returns an error on
        // every launch and never preserves or reports the entry, and SAVE takes the "unreadable"
        // path. What matters is the directory entry, and metadata answers that without opening
        // anything, so every open failure consults it.
        let non_regular = std::fs::symlink_metadata(path)
            .map(|metadata| !metadata.is_file())
            .unwrap_or(false);
        if non_regular {
            return EntryRead::NotRegular;
        }
        return EntryRead::Failed(error);
    }
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    // A symlink is FOLLOWED for reading: a dotfiles-managed `session_state.json` is a legitimate
    // layout, and reporting a healthy session as corruption would boot an empty workspace. The
    // link itself is not lost either — SAVE records where it pointed before publishing.
    match file.metadata() {
        Ok(metadata) if !metadata.is_file() => EntryRead::NotRegular,
        Ok(_) => {
            // Clear O_NONBLOCK once the handle is known to be a regular file. On a network or FUSE
            // filesystem a non-blocking read of a regular file can return EAGAIN mid-file, which
            // `read_to_end` would surface as an error, or worse, a short buffer that parses as a
            // truncated document. The flag has already done its job: it stopped a FIFO open from
            // blocking.
            #[cfg(unix)]
            if let Err(error) = clear_nonblocking(&file) {
                return EntryRead::Failed(error);
            }
            use std::io::Read;
            let mut file = file;
            let mut bytes = Vec::new();
            // A transient EAGAIN/WouldBlock must not be reported as an unreadable file: the save
            // path preserves and REPLACES anything it cannot read, so a lost race would cost the
            // session. Retrying the read is what keeps a healthy file from being displaced.
            let mut attempts = 0;
            loop {
                match file.read_to_end(&mut bytes) {
                    Ok(_) => break EntryRead::Bytes(bytes),
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock && attempts < 64 =>
                    {
                        attempts += 1;
                        std::thread::yield_now();
                    }
                    Err(error) => break EntryRead::Failed(error),
                }
            }
        }
        Err(error) => EntryRead::Failed(error),
    }
}

/// Removes `O_NONBLOCK` from an already-open regular file so reads run normally to EOF.
///
/// `O_NONBLOCK` is what keeps the OPEN of a FIFO from blocking; once the handle has been proven to
/// be a regular file the flag only hurts, because on macOS and on network/FUSE mounts a read may
/// otherwise fail with EAGAIN part-way through. libc exposes both `fcntl` and the flags on every
/// unix, so this is not Linux-specific.
#[cfg(unix)]
fn clear_nonblocking(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let fd = file.as_raw_fd();
    // SAFETY: fd is a valid open descriptor owned by `file` for the duration of both calls.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if flags & libc::O_NONBLOCK == 0 {
        return Ok(());
    }
    // SAFETY: same descriptor, flag clear only.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Portable fallback: named pipes are not addressable as ordinary paths on Windows, so a plain
/// open cannot block the way a POSIX FIFO does, but the handle still decides the file type.
#[cfg(not(unix))]
fn read_regular_file(path: &Path) -> EntryRead {
    // A symlink is followed for reading, matching the unix branch: a dotfiles-managed session file
    // is a legitimate layout and must not be reported as corruption.
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        // Opening a directory fails with ERROR_ACCESS_DENIED on Windows rather than yielding a
        // handle, so the entry type has to be decided from metadata when the open is refused.
        // Without this a directory at the session path is reported as an I/O failure forever and
        // is never preserved or announced as corruption, even though save already heals it.
        Err(error) => {
            return match std::fs::symlink_metadata(path) {
                Ok(metadata) if !metadata.is_file() => EntryRead::NotRegular,
                _ => EntryRead::Failed(error),
            }
        }
    };
    match file.metadata() {
        Ok(metadata) if !metadata.is_file() => EntryRead::NotRegular,
        Ok(_) => {
            use std::io::Read;
            let mut bytes = Vec::new();
            match (&file).read_to_end(&mut bytes) {
                Ok(_) => EntryRead::Bytes(bytes),
                Err(error) => EntryRead::Failed(error),
            }
        }
        Err(error) => EntryRead::Failed(error),
    }
}

/// Preserves whatever occupies `path` under `backup` without ever leaving a healthy session
/// unreachable and without ever destroying an entry this call did not author.
///
/// `fs::rename` moves whatever is at the SOURCE NAME when it runs. Between the read that
/// classified the entry and the rename, another process can publish a healthy session at the path
/// (writers publish by atomic rename), so a blind rename would move that good session into a
/// backup name and leave startup reporting an empty window. A hard link copies the directory entry
/// instead: the source name keeps referring to the same inode, so nothing is ever removed from the
/// session path and the publish that follows replaces it normally.
///
/// Three entry shapes have to be handled, and the ordering matters:
/// - A regular file is hard-linked, which duplicates it and leaves the canonical name intact.
/// - A directory cannot be linked and cannot be read as a session, and leaving it in place wedges
///   every later save, so it is renamed aside and the renamed entry is verified.
/// - Where hard links are unavailable the entry is renamed aside, which is atomic and lossless.
///
/// First unused `<path>.pre-v3-N` name, so each distinct legacy document keeps its own backup
/// instead of the first one permanently standing in for all later ones.
fn first_free_pre_v3_backup(path: &Path) -> Option<std::path::PathBuf> {
    for index in 1..=64 {
        let mut candidate = path.as_os_str().to_owned();
        candidate.push(format!(".pre-v3-{index}"));
        let candidate = std::path::PathBuf::from(candidate);
        match std::fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(candidate),
            _ => continue,
        }
    }
    None
}

/// Preserves the bytes this call READ, under a name that is claimed exclusively.
///
/// A path-based preserve (hard link or rename) re-resolves the pathname at call time, so if another
/// process publishes a healthy session between the read and the preserve, that healthy session is
/// what gets captured — and the canonical path may even be vacated. Writing the classified bytes
/// cannot do either. Each attempt claims a distinct name, so a lost race costs a name, not a save.
fn preserve_read_bytes(
    path: &Path,
    suffix: Option<&str>,
    bytes: &[u8],
) -> std::io::Result<std::path::PathBuf> {
    let mut last = None;
    // The bound is the DECLARED quarantine capacity, not an arbitrary small number: every distinct
    // corrupt document needs its own name, and stopping at nine made the tenth load fail outright
    // even though `.json.corrupted.9` was free. The caller then saw a bare I/O error, so the person
    // got neither the corruption notice nor a backup of the document that was just replaced.
    for attempt in 0..QUARANTINE_SLOT_LIMIT {
        // The FIRST backup of a corrupt document is exactly "json.corrupted"; only later ones gain
        // a suffix. Emitting "json.corrupted-" for the first would break the recovery contract
        // that both the tests and the person's expectations encode.
        let name = match (attempt, suffix) {
            (0, None) => "json.corrupted".to_string(),
            (0, Some(suffix)) => format!("json.corrupted-{suffix}"),
            (_, None) => format!("json.corrupted.{attempt}"),
            (_, Some(suffix)) => format!("json.corrupted-{suffix}-{attempt}"),
        };
        let backup = path.with_extension(name);
        match preserve_bytes_exclusively(&backup, bytes) {
            Ok(()) => {
                // `preserve_bytes_exclusively` already wrote the bytes, flushed them and synced the
                // directory holding this name, so the backup is durable when it returns.
                return Ok(backup);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // A document that is already preserved byte-for-byte needs no second copy: the
                // caller's guarantee is that THESE bytes survive, not that a new name appears.
                // Without this, a file that stays corrupt across launches burns one name per
                // launch and starts erroring after the ninth.
                // Only a REGULAR file can be compared by reading it: a FIFO at the backup name
                // would block the reader forever while this thread holds the session lock, and a
                // symlink would compare bytes belonging to some other file.
                let readable = std::fs::symlink_metadata(&backup)
                    .map(|meta| meta.file_type().is_file())
                    .unwrap_or(false);
                if readable
                    && std::fs::read(&backup).map(|existing| existing == bytes).unwrap_or(false)
                {
                    // The bytes match, but this name may be inherited from an earlier attempt whose
                    // sync FAILED: `preserve_bytes_exclusively` leaves its file behind when
                    // `sync_all` or the directory sync errors, so a crash could still drop the very
                    // name the caller is about to report as a preserved recovery copy. Reusing it
                    // therefore re-establishes BOTH halves of its durability first - the data still
                    // has to reach the disk, and the NAME still has to survive - and a candidate
                    // that fails either step is not good enough, so a fresh copy is written.
                    if publish_preserved_entry(backup.clone()).is_ok() {
                        return Ok(backup);
                    }
                }
                last = Some(error);            }
            Err(error) => return Err(error),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::from(std::io::ErrorKind::AlreadyExists)))
}

/// Writes the bytes this call actually READ to a fresh backup name.
///
/// This is the only preservation primitive that is correct under concurrency: it copies the
/// document the caller classified rather than re-resolving the path, so a session another process
/// published in the meantime can never be captured as if it were the corrupt one, and the canonical
/// path is never vacated. `create_new` makes the claim exclusive, so two processes cannot write the
/// same backup name.
fn preserve_bytes_exclusively(backup: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(backup)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    sync_entry_directory(backup)
}

/// Recreates a symlink at `backup` pointing at `target`.
///
/// `std::fs::hard_link` cannot stand in for this: POSIX `link(2)` follows the link, so a dangling
/// symlink fails with ENOENT, and Windows has no portable hard link to a symlink either. Recreating
/// the link copies the one thing the entry holds - a name pointing somewhere - without reading
/// through it, so a dangling target stays dangling instead of turning a load into a hard failure.
#[cfg(unix)]
fn recreate_symlink(target: &Path, backup: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, backup)
}

#[cfg(windows)]
fn recreate_symlink(target: &Path, backup: &Path) -> std::io::Result<()> {
    // The recreated entry only has to preserve what was there; a dangling directory-vs-file guess
    // is resolved from the existing link's own type by the caller's metadata, and a file symlink is
    // the portable default. Failure is reported, never silently dropped.
    std::os::windows::fs::symlink_file(target, backup)
}

/// Makes a freshly created directory ENTRY durable, not just its contents.
///
/// A backup whose bytes are on disk but whose NAME is not cannot be found after a crash, so every
/// preserve path syncs the directory that holds the new name BEFORE the canonical entry is
/// replaced. Reported as an error by the preserve paths: a backup that may vanish is not a backup.
/// The directory that holds this path's entry, with an empty parent normalized to the current
/// directory. A bare name like `session_state.json` lives in `.`, not "nowhere": `read_dir("")`
/// fails, so an un-normalized caller would either skip the directory sync that makes a new backup
/// name survive a crash, or never find the backup it already made and mint a fresh name on every
/// launch.
fn entry_parent(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

fn sync_entry_directory(path: &Path) -> std::io::Result<()> {
    // An empty parent is not "nowhere": a bare name like "session_state.json" lives in the current
    // directory, and that directory entry needs the same sync as any other. Skipping it would let a
    // relative session path publish a backup whose NAME a crash could drop.
    let parent = entry_parent(path);
    // Windows cannot open a directory with `File::open`, so an unconditional open would fail every
    // save that preserves an entry there - and the canonical file is corrupt in exactly that case,
    // so the failure repeats forever. NTFS journals the entry itself, so the sync is a no-op there
    // (the same split `daemon::runtime_stage::sync_dir` uses).
    #[cfg(unix)]
    {
        std::fs::File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Ok(())
    }
}

/// Reports a preserved ENTRY only once both halves of its durability hold.
///
/// A backup is named twice over: its bytes must be on disk (`sync_preserved_inode`) and its NAME
/// must survive a crash (`sync_entry_directory`). A caller that reports a backup the person is told
/// to recover from before both have been established is making a promise the filesystem has not
/// made. Byte copies get this from the write chain; a LINKED or RECREATED entry never wrote
/// anything, so it needs the same two steps taken explicitly - and a REUSED name inherits whatever
/// the attempt that created it managed to establish, which may be neither.
///
/// Returns the backup on success; every failure is an error, because a backup that may vanish is
/// not a backup.
fn publish_preserved_entry(backup: PathBuf) -> std::io::Result<PathBuf> {
    sync_preserved_inode_strict(&backup)?;
    sync_entry_directory(&backup)?;
    Ok(backup)
}

/// Flushes a preserved entry's DATA, refusing every failure including a permission denial.
///
/// This is the variant `publish_preserved_entry` uses, because the caller is about to REPORT this
/// name as a recovery copy: if the bytes cannot be flushed, that promise cannot be made. The
/// lenient `sync_preserved_inode` below exists for the SAVE path, where the alternative is wedging
/// every future save on a document this process may not write.
fn sync_preserved_inode_strict(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => return Ok(()),
        Err(error) => return Err(error),
    }
    // Every failure here is refused, INCLUDING a permission denial. A reuse candidate whose bytes
    // cannot be flushed is not a backup this load may report: it may have been left behind by an
    // earlier attempt whose own `sync_all` failed, so a crash could still drop the very bytes the
    // person is told to recover from. Refusing makes the caller fall through to writing a FRESH
    // copy that this process creates and can flush itself, so the retry is bounded - the new copy
    // is writable, so the next load reuses it instead of minting another numbered name.
    open_preserved_for_flush(path).and_then(|file| file.sync_all())
}

/// Opens a preserved entry for flushing.
///
/// `FlushFileBuffers` - what `sync_all` becomes on Windows - needs a handle with WRITE access, so
/// a read-only open fails there with access denied even for a file this process owns. The pre-v3
/// preserve path already opens this way for the same reason; doing it here is what makes the
/// flush actually happen instead of being reported as an unwritable entry.
fn open_preserved_for_flush(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(cfg!(windows))
        .open(path)
}

/// Flushes a preserved entry's DATA, tolerating the one case where it cannot be opened.
///
/// A preserved entry may be a file this process is not allowed to read - mode 000, or an ACL that
/// denies reading - which is exactly the shape the preserve path exists for. Treating that refusal
/// as fatal would wedge every future save on those files, so a permission denial is accepted as
/// "the data is whatever the kernel already holds for an entry we may not touch", while every other
/// I/O failure still aborts the save.
fn sync_preserved_inode(path: &Path) -> std::io::Result<()> {
    // Only a REGULAR file has bytes worth flushing, and only a regular file can be opened without
    // blocking: opening a writer-less FIFO never returns, so a save that preserved one while
    // holding the session lock would hang every later session operation. Directories, sockets and
    // devices carry no session bytes either. The entry's NAME is still synced by the caller.
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => return Ok(()),
        Err(error) => return Err(error),
    }
    match open_preserved_for_flush(path).and_then(|file| file.sync_all()) {
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
        other => other,
    }
}

/// Releases a deferred move when the publish that needed it failed.
///
/// The tuple describes a move that has NOT happened yet, so there is nothing to restore: renaming
/// the backup name onto the canonical path would fail with ENOENT and log a "stranded" error that
/// never occurred - and if such a name ever did exist, that rename would OVERWRITE the live entry.
/// Dropping the marker is the whole operation.
fn undo_deferred_move(deferred: &mut Option<(PathBuf, PathBuf)>) {
    let _ = deferred.take();
}

/// `rename` that refuses to replace an existing destination.
///
/// Atomically EXCHANGES the two directory entries, so a caller that must replace an occupant can
/// do it in one step. Without this the occupant has to be moved aside first, and a crash in between
/// leaves the destination name absent.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn exchange_paths(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let from = CString::new(from.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let to = CString::new(to.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: both pointers are valid NUL-terminated paths that outlive the call.
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_SWAP) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn exchange_paths(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let from = CString::new(from.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let to = CString::new(to.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: both pointers are valid NUL-terminated paths that outlive the call.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Windows has no atomic exchange (ReplaceFileW discards the replacement rather than swapping the
/// two), so the caller keeps its two-step fallback there and the window is a platform limit rather
/// than something this code can close.
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
)))]
fn exchange_paths(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// Plain `rename` overwrites, so it cannot be used to preserve an entry while other processes
/// publish at the same path: it would silently destroy a newer session. This wraps the platform's
/// no-replace variant where one exists.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let from = CString::new(from.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let to = CString::new(to.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: both pointers are valid NUL-terminated paths that outlive the call.
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let from = CString::new(from.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let to = CString::new(to.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: both pointers are valid NUL-terminated paths that outlive the call.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Windows has no no-replace rename in `std`, so the destination is claimed exclusively first and
/// the move is attempted only against that claim: a concurrent publisher makes either the claim or
/// the move fail, and neither path replaces its file.
/// Windows: `MoveFileExW` WITHOUT `MOVEFILE_REPLACE_EXISTING` refuses an existing destination, so
/// the refusal happens inside the kernel rather than as a check-then-act race a concurrent
/// publisher could win. `std::fs::rename` cannot be used: on Windows it replaces.
#[cfg(target_os = "windows")]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

    fn wide(path: &Path) -> std::io::Result<Vec<u16>> {
        let mut buffer: Vec<u16> = path.as_os_str().encode_wide().collect();
        if buffer.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the path contains a NUL character",
            ));
        }
        buffer.push(0);
        Ok(buffer)
    }

    let from = wide(from)?;
    let to = wide(to)?;
    // SAFETY: both buffers are NUL-terminated and outlive the call; the flags are a plain bitmask.
    let moved = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            // No REPLACE_EXISTING: that absence is what makes this no-replace.
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        let error = std::io::Error::last_os_error();
        // ERROR_ALREADY_EXISTS (183) and ERROR_FILE_EXISTS (80) both mean the destination is taken.
        return match error.raw_os_error() {
            Some(183) | Some(80) => Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists)),
            _ => Err(error),
        };
    }
    Ok(())
}

/// Any other target: claim the destination with `create_new` first, then move onto the name we
/// just reserved. The claim is what makes the destination exclusive; the rename itself only ever
/// replaces our own empty file.
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_os = "windows"
)))]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
    {
        Ok(_) => std::fs::rename(from, to),
        Err(error) => Err(error),
    }
}


/// Whether `candidate` and `path` are the same directory entry, judged without opening either.
#[cfg(unix)]
fn same_entry_as_path(candidate: &Path, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (left, right) = match (std::fs::symlink_metadata(candidate), std::fs::symlink_metadata(path)) {
        (Ok(left), Ok(right)) => (left, right),
        _ => return false,
    };
    // A recreated symlink is a DIFFERENT inode that names the same target, so inode identity alone
    // would never match it and every load of a dangling link would mint another backup name. Two
    // links are the same preserved entry when they point at the same target, which is compared
    // from link text only and therefore never reads through the link.
    if left.file_type().is_symlink() && right.file_type().is_symlink() {
        return std::fs::read_link(candidate).ok() == std::fs::read_link(path).ok();
    }
    left.dev() == right.dev() && left.ino() == right.ino()
}

/// Windows has no portable inode pair here; a duplicate backup name is harmless there.
#[cfg(not(unix))]
fn same_entry_as_path(_candidate: &Path, _path: &Path) -> bool {
    false
}

/// Finds an existing backup that holds the SAME directory entry as `path`, if one exists.
///
/// Backups are named two ways: `.corrupted`/`.corrupted.N` for byte copies, and
/// `.corrupted-<suffix>` for a preserved non-regular entry. Enumerating names would only cover
/// one scheme, so the parent directory is scanned and identity decided by device+inode, which
/// never reads and therefore never blocks. A directory holds one entry per name, so this is also
/// the only check that still works when a load and a save used different schemes.
#[cfg(unix)]
fn existing_entry_backup(path: &Path) -> Option<PathBuf> {
    // `read_dir("")` fails, so a relative session path would NEVER find its existing backup and
    // would mint a fresh name on every launch - unbounded growth, the opposite of this scan's job.
    let parent = entry_parent(path);
    let prefix = format!(
        "{}.corrupted",
        path.file_name().map(|name| name.to_string_lossy().into_owned())?
    );
    let entries = std::fs::read_dir(parent).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path();
        if candidate == path {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) {
            continue;
        }
        if same_entry_as_path(&candidate, path) {
            return Some(candidate);
        }
    }
    None
}

/// Windows has no portable inode pair; a duplicate backup name is harmless there.
#[cfg(not(unix))]
fn existing_entry_backup(_path: &Path) -> Option<PathBuf> {
    None
}

/// Backs up the session-file SYMLINK itself, reusing an existing backup when one is already there.
///
/// The publish replaces the directory entry, so the link is the only record of where the layout
/// lived. Writing the target path into a regular file would restore the words but not the wiring:
/// the configured name would end up a plain file and the user's dotfile manager would own nothing.
/// Recreating the link keeps a dangling target dangling and keeps a managed layout managed.
///
/// A link is backed up ONCE per target. Reusing the existing backup is what keeps a session saved
/// on every launch from minting a fresh link file each time; a changed target means the user
/// re-pointed the link, which IS a different entry and needs its own backup.
fn existing_or_new_symlink_backup(
    path: &Path,
    write_suffix: &str,
    target: &Path,
) -> std::io::Result<PathBuf> {
    if let Some(existing) = existing_symlink_backup(path, target) {
        if std::fs::read_link(&existing).is_ok_and(|linked| linked == target) {
            // A reused backup still needs its NAME made durable before the canonical entry is
            // replaced: an earlier attempt may have created the link and then failed its own
            // directory sync, so the name is present but not yet crash-safe. Skipping the barrier
            // here would let a crash publish the replacement while the backup name evaporates.
            sync_entry_directory(&existing)?;
            return Ok(existing);
        }
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "session_state.json".to_string());
    let parent = entry_parent(path);
    // The first free SLOT wins. A single fixed name is not enough: an earlier save can have left
    // that exact name behind (a link for a previous target, or a leftover from an interrupted
    // write), and recreating over it fails with AlreadyExists. Failing there would abort the save
    // forever on the same suffix - a session that can never be persisted again. Slots are bounded
    // like the quarantine chain so a pathological directory cannot spin here.
    let mut last = None;
    for slot in 0..QUARANTINE_SLOT_LIMIT {
        let candidate = if slot == 0 {
            parent.join(format!("{name}.corrupted-{write_suffix}-link"))
        } else {
            parent.join(format!("{name}.corrupted-{write_suffix}-link.{slot}"))
        };
        // An existing link for THIS target is a valid backup already: reuse it rather than minting
        // another one on every save.
        match std::fs::read_link(&candidate) {
            // Same durability rule as the reuse above: a name left by an interrupted attempt is
            // only a backup once its directory is synced.
            Ok(linked) if linked == target => {
                sync_entry_directory(&candidate)?;
                return Ok(candidate);
            }
            Ok(_) => continue,
            Err(_) => {}
        }
        if std::fs::symlink_metadata(&candidate).is_ok() {
            continue;
        }
        match recreate_symlink(target, &candidate) {
            Ok(()) => {
                // A link whose NAME is not durable cannot be found after a crash, so the entry is
                // made durable before the canonical path is replaced. A backup that may vanish is
                // not a backup.
                sync_entry_directory(&candidate)?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                last = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    Err(last.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "no free symlink backup slot",
        )
    }))
}

/// Finds an existing link backup for `path` that preserves `target`, if one is present.
///
/// The target matters. Returning the FIRST `*-link` entry regardless of what it points at made a
/// directory that holds backups for two different targets mint a fresh backup on every save: the
/// caller's target comparison then failed, and the slot names it fell back to carry a per-save
/// suffix, so no name ever repeated and the backups grew without bound.
fn existing_symlink_backup(path: &Path, target: &Path) -> Option<PathBuf> {
    let parent = entry_parent(path);
    let prefix = format!(
        "{}.corrupted",
        path.file_name().map(|name| name.to_string_lossy().into_owned())?
    );
    let entries = std::fs::read_dir(parent).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path();
        if candidate == path {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) || !name.ends_with("-link") {
            continue;
        }
        // `symlink_metadata` keeps this a check of the ENTRY, so a dangling backup still counts.
        if std::fs::symlink_metadata(&candidate).is_ok_and(|meta| meta.file_type().is_symlink())
            && std::fs::read_link(&candidate).is_ok_and(|linked| linked == target)
        {
            return Some(candidate);
        }
    }
    None
}

/// The outcome of preserving an unreadable session file.
enum Preservation {
    /// The entry was copied to this backup. A regular file is left in place for the caller to
    /// replace with its own publish; a directory has to be moved, since it cannot be linked.
    Quarantined(PathBuf),
    /// The path had already become a valid session; it was returned instead of being quarantined.
    Recovered(Box<PersistedWorkspaceSession>),
}

/// Preserves a corrupt session file by COPYING it to a fresh backup, and never unlinks the source.
///
/// There is no safe way to delete the source here. `read` followed by `remove_file` is two
/// filesystem operations: another process can atomically rename a healthy session onto `path`
/// between them, and the removal then destroys that save. Re-reading to compare content does not
/// help, because the comparison and the removal are still separate operations and the replacement
/// can land between them. The same applies to `hard_link` followed by `remove_file`. So this
/// function only ever CREATES: the destination is claimed with `create_new`, the corrupt bytes are
/// written into it, and the source is left exactly where it is. The source is replaced later by
/// the ordinary save path, which publishes through its own atomic rename.
///
/// The file is read ONCE. If those bytes parse as a session, they are returned as-is: re-reading
/// the pathname afterwards could observe a later truncation and report an empty success with no
/// backup, which would silently discard the recovery.
fn preserve_corrupt_session(path: &Path) -> Result<Preservation, IpcError> {
    let corrupt = match read_regular_file(path) {
        EntryRead::Bytes(bytes) => bytes,
        // Nothing can be COPIED out of a FIFO without blocking on a writer that may never come,
        // and its contents are never a session. It must still be recoverable, because the next
        // save replaces the directory entry. Linking copies the entry without reading it, so the
        // FIFO cannot block and the canonical path stays occupied.
        EntryRead::NotRegular => {
            // Reuse the backup that already holds this exact entry so launches do not mint one
            // name per start. Identity is compared from metadata only, which never blocks.
            if let Some(existing) = existing_entry_backup(path) {
                // The name was created by an EARLIER load, and nothing guarantees that attempt
                // finished syncing it: reusing it therefore re-establishes both halves of its
                // durability before the caller is told this is where the entry was preserved.
                // A candidate that cannot be made durable is not reused; a fresh link is made
                // below instead.
                if let Ok(backup) = publish_preserved_entry(existing) {
                    return Ok(Preservation::Quarantined(backup));
                }
            }
            // A non-regular entry holds no session bytes, but it must still be RECOVERABLE: the
            // save that follows replaces the directory entry, so leaving it in place while
            // reporting the canonical path as its own backup would destroy it. A hard link copies
            // the entry without reading it (so a FIFO cannot block) and leaves the canonical path
            // occupied.
            //
            // LOAD NEVER MOVES: a move is only safe when the mover publishes in the same call, and
            // a load does not. A moved load would leave the canonical path absent while a healthy
            // session could exist, and it cannot put back what it never published. So a directory,
            // or anything else that cannot be linked (a filesystem without hard links, a Windows
            // symlink), is reported as an error and left exactly where it is; the next SAVE
            // preserves it as part of its own publish.
            let backup = path.with_extension(format!("json.corrupted-{}", unique_write_suffix()));
            // A backup the person is told to recover from is only a backup once the filesystem has
            // promised it survives a crash, so both a linked and a recreated entry go through the
            // same publish step instead of returning the name the moment it appears.
            let not_durable = |backup: &Path, error: std::io::Error| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!(
                        "A non-regular entry occupies the session path and its backup at {} was not durable ({error}); the next save will preserve it",
                        backup.display()
                    ),
                )
            };
            match std::fs::hard_link(path, &backup) {
                Ok(()) => {
                    return publish_preserved_entry(backup.clone())
                        .map(Preservation::Quarantined)
                        .map_err(|error| not_durable(&backup, error))
                }
                // macOS `link(2)` FOLLOWS a symlink, so a dangling link fails with ENOENT and a
                // bare hard_link error would fail every load while that entry sits there - base
                // booted an empty window instead. Recreating the link preserves exactly what the
                // entry is (a name pointing somewhere) without reading through it, and a link to a
                // missing target holds no session bytes to lose.
                Err(_) if std::fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false) => {
                    match std::fs::read_link(path).and_then(|target| recreate_symlink(&target, &backup)) {
                        Ok(()) => {
                            return publish_preserved_entry(backup.clone())
                                .map(Preservation::Quarantined)
                                .map_err(|error| not_durable(&backup, error))
                        }
                        Err(error) => {
                            return Err(IpcError::new(
                                IpcErrorCode::IoError,
                                format!(
                                    "A symlink occupies the session path and could not be recreated at {} ({error}); the next save will preserve it",
                                    backup.display()
                                ),
                            ))
                        }
                    }
                }
                Err(error) => {
                    return Err(IpcError::new(
                        IpcErrorCode::IoError,
                        format!(
                            "A non-regular entry occupies the session path and cannot be linked to {} ({error}); the next save will preserve it",
                            backup.display()
                        ),
                    ))
                }
            }
        }
        EntryRead::Failed(error) => {
            return Err(IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to read the corrupt session file: {error}"),
            ))
        }
    };

    // A healthy save may have landed between the failed parse and this read. Quarantining it
    // would report a corruption that no longer exists and leave a stray backup of a good session.
    if let Ok(text) = std::str::from_utf8(&corrupt) {
        if let Ok(session) = serde_json::from_str::<PersistedWorkspaceSession>(text) {
            return Ok(Preservation::Recovered(Box::new(session)));
        }
    }

    // A previous launch may already have preserved exactly these bytes. Reusing that backup keeps
    // repeated launches from filling the directory with identical copies. This is a read-only
    // check: racing it can only produce a duplicate backup, never lose anything.
    for candidate in quarantine_paths(path).take(QUARANTINE_SLOT_LIMIT as usize) {
        // The same non-blocking reader as the primary path: a backup name can hold a FIFO or an
        // unreadable entry too, and a plain read of it would hang this load while it holds the
        // session path lock. Anything that is not readable regular bytes simply cannot match.
        if !matches!(read_regular_file(&candidate), EntryRead::Bytes(bytes) if bytes == corrupt) {
            continue;
        }
        // Reuse inherits the backup's durability. A previous attempt may have written these bytes
        // and then failed to sync the data or the name, which leaves a file a crash can drop or
        // truncate; returning that path would report a recovery copy that is not guaranteed to
        // exist. Both halves are re-established here, and an unusable backup is skipped so a
        // fresh copy is written.
        if let Ok(backup) = publish_preserved_entry(candidate) {
            return Ok(Preservation::Quarantined(backup));
        }
    }

    let (backup, mut file) = reserve_quarantine_path(path)?;
    // `sync_all` flushes the backup's CONTENTS; the directory entry that gives them a name is not
    // durable until its directory is synced. Without this, a crash right after load reported the
    // backup path could leave a name the filesystem never committed, so the recovery copy the
    // person was told about would not be there.
    if let Err(error) = file
        .write_all(&corrupt)
        .and_then(|()| file.sync_all())
        .and_then(|()| sync_entry_directory(&backup))
    {
        // The destination was created empty by this call and holds nothing worth keeping.
        let _ = fs::remove_file(&backup);
        return Err(IpcError::new(
            IpcErrorCode::IoError,
            format!(
                "The corrupt session file could not be preserved at {}: {error}",
                backup.display()
            ),
        ));
    }
    drop(file);
    Ok(Preservation::Quarantined(backup))
}



pub fn clear_session_from_path(path: &Path) -> Result<(), IpcError> {
    // A clear on a fresh install has nothing to remove, and taking the lock would first try to
    // create the lock file in a directory that does not exist - turning a successful no-op into an
    // IoError. Nothing to exclude means nothing to lock.
    //
    // Only the PARENT is inspected here: "is there a directory at all" cannot change under a
    // concurrent save in a way that matters, while "is the session file absent" can. Checking the
    // file out here let a save that sits between its temporary write and its publish look like an
    // empty path, so clear returned success and the published session came back on the next load.
    // Every decision about the entry itself is made under the lock.
    let parent = entry_parent(path);
    match std::fs::metadata(parent) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        // An inspection failure is not evidence that there is nothing to remove. Swallowing it
        // reported a successful clear while the saved document was still at its path.
        Err(error) => {
            return Err(IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to inspect session state file: {error}"),
            ))
        }
    }
    try_with_session_path_lock(path, || clear_session_from_path_locked(path))
}

fn clear_session_from_path_locked(path: &Path) -> Result<(), IpcError> {
    // The saved document is a REGULAR file, possibly reached through a symlink: a dotfiles-managed
    // `session_state.json` is a supported layout (load follows it and save writes through it), so a
    // clear must unlink that LINK instead of reporting success and leaving the old layout to be
    // loaded again. A DANGLING link is the same entry - its target is gone, so `metadata` cannot
    // describe it - and it is removed for the same reason. Anything else at the name (a directory,
    // FIFO or socket) is not this app's document, and unlinking it would destroy state this code
    // did not create.
    let entry = match std::fs::symlink_metadata(path) {
        Ok(entry) => entry,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to inspect session state file: {error}"),
            ));
        }
    };
    if !entry.file_type().is_symlink() {
        match std::fs::metadata(path) {
            Ok(meta) if meta.is_file() => {}
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Failed to inspect session state file: {error}"),
                ));
            }
        }
    }
    fs::remove_file(path).map_err(|e| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to remove session state file: {}", e),
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Runs `body` with the process working directory set to `dir`.
    ///
    /// The working directory is process-global, so the swap is serialized and always restored.
    fn with_cwd<T>(dir: &Path, body: impl FnOnce() -> T) -> T {
        static CWD: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = CWD.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir).unwrap();
        let outcome = body();
        std::env::set_current_dir(previous).unwrap();
        outcome
    }

    #[test]
    fn test_session_path_for_main_window_is_unchanged() {
        let primary = PathBuf::from("/data/ferryx/session_state.json");
        assert_eq!(session_path_for_window(&primary, "main").unwrap(), primary);
        let dev = PathBuf::from("/data/ferryx/dev/session_state.json");
        assert_eq!(session_path_for_window(&dev, "main").unwrap(), dev);
    }

    #[test]
    fn test_session_path_for_secondary_window_is_mapped_next_to_primary() {
        let primary = PathBuf::from("/data/ferryx/session_state.json");
        assert_eq!(
            session_path_for_window(&primary, "ws-0f3a-b2_c").unwrap(),
            PathBuf::from("/data/ferryx/session-ws-0f3a-b2_c.json")
        );
        assert_eq!(
            session_path_for_window(&primary, "ws-test").unwrap(),
            PathBuf::from("/data/ferryx/session-ws-test.json")
        );
    }

    #[test]
    fn test_session_path_rejects_non_app_and_unsafe_labels() {
        let primary = PathBuf::from("/data/ferryx/session_state.json");
        for label in [
            "",
            "ws-",
            "settings",
            "Main",
            "browser-1",
            "ws-../evil",
            "ws-a/b",
            "ws-a\\b",
            "ws-..",
            "ws-a.b",
            "ws- a",
        ] {
            let error = session_path_for_window(&primary, label).unwrap_err();
            assert_eq!(error.code, IpcErrorCode::InvalidArgument, "label {label:?}");
        }
    }

    #[test]
    fn test_window_session_files_do_not_overwrite_each_other() {
        let dir = tempdir().unwrap();
        let primary = dir.path().join("session_state.json");
        let secondary = session_path_for_window(&primary, "ws-test").unwrap();

        save_session_to_path(&primary, &minimal_v3_session("main")).unwrap();
        save_session_to_path(&secondary, &minimal_v3_session("secondary")).unwrap();

        let main_loaded = load_session_from_path(&primary).unwrap().unwrap();
        let secondary_loaded = load_session_from_path(&secondary).unwrap().unwrap();
        assert_eq!(main_loaded.extra["testMarker"], "main");
        assert_eq!(secondary_loaded.extra["testMarker"], "secondary");

        clear_session_from_path(&secondary).unwrap();
        assert!(!secondary.exists());
        assert!(primary.exists());
    }

    #[test]
    fn test_session_save_load_clear_cycle() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");

        let mut workspaces = HashMap::new();
        workspaces.insert(
            "default".to_string(),
            PersistedWorkspace {
                workspace_id: "default".to_string(),
                repo_root: PathBuf::from("/tmp/repo"),
                worktrees: vec![PersistedWorktree {
                    path: "/tmp/repo".to_string(),
                    branch: "main".to_string(),
                    head: "abcdef".to_string(),
                    is_main: true,
                    is_locked: false,
                }],
                active_worktree_path: Some("/tmp/repo".to_string()),
                layout: PersistedLayout {
                    split_mode: "none".to_string(),
                    primary_tab_id: Some("tab-1".to_string()),
                    secondary_tab_id: None,
                    active_tab_id: Some("tab-1".to_string()),
                    tabs: vec![PersistedTab {
                        id: "tab-1".to_string(),
                        kind: None,
                        label: "main".to_string(),
                        pinned: None,
                        terminal: None,
                        browser: None,
                        custom_title: None,
                        session_id: Some("term-1".to_string()),
                        worktree_path: Some("/tmp/repo".to_string()),
                        pane_tree: None,
                        session_ids_by_leaf_id: None,
                        active_leaf_id: None,
                        expanded_leaf_id: None,
                        extra: HashMap::new(),
                    }],
                    tab_groups: None,
                    tab_group_layout: None,
                    focused_group_id: None,
                    layouts_by_tab_id: None,
                    extra: HashMap::new(),
                },
                worktree_layouts: None,
                layout_by_worktree: None,
                terminal_sessions: HashMap::new(),
                extra: HashMap::new(),
            },
        );

        let session = PersistedWorkspaceSession {
            version: 1,
            timestamp: 1234567890,
            active_workspace_id: "default".to_string(),
            workspaces,
            extra: HashMap::new(),
        };

        let loaded = load_session_from_path(&session_file).unwrap();
        assert!(loaded.is_none());

        save_session_to_path(&session_file, &session).unwrap();
        assert!(session_file.exists());

        let loaded = load_session_from_path(&session_file).unwrap();
        assert!(loaded.is_some());
        let loaded_session = loaded.unwrap();
        assert_eq!(loaded_session.version, 1);
        assert_eq!(loaded_session.active_workspace_id, "default");
        assert_eq!(
            loaded_session.workspaces["default"].repo_root,
            PathBuf::from("/tmp/repo")
        );

        clear_session_from_path(&session_file).unwrap();
        assert!(!session_file.exists());
        let loaded = load_session_from_path(&session_file).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_corrupted_session_recovery() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");

        fs::write(&session_file, "{ broken json ...").unwrap();

        let loaded = load_session_from_path(&session_file).unwrap();
        assert!(loaded.is_none());
        assert!(dir.path().join("session_state.json.corrupted").exists());
        // The corrupt file is copied, never unlinked: deleting it here would race a concurrent
        // save that renamed a healthy session onto the path. It is replaced by the next save.
        assert_eq!(
            fs::read_to_string(&session_file).unwrap(),
            "{ broken json ..."
        );
    }

    /// The load that reports corruption must name the backup it wrote: without that path the UI
    /// cannot tell a person where their layout went, which is the whole difference between a
    /// silent empty window and a recoverable one.
    #[test]
    fn a_corrupt_load_reports_the_backup_it_wrote() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        fs::write(&session_file, "{ broken json ...").unwrap();

        let (session, quarantined) = load_session_reporting_corruption(&session_file).unwrap();

        let backup = quarantined.expect("a corrupt file must report its quarantine path");
        assert!(session.is_none());
        assert_eq!(backup, dir.path().join("session_state.json.corrupted"));
        assert_eq!(fs::read_to_string(&backup).unwrap(), "{ broken json ...");
    }

    /// An absent file and a healthy file both report no quarantine; otherwise every ordinary start
    /// would raise a corruption warning.
    #[test]
    fn a_missing_or_healthy_file_reports_no_quarantine() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");

        assert_eq!(load_session_reporting_corruption(&session_file).unwrap(), (None, None));

        let session = minimal_v3_session("healthy");
        save_session_to_path(&session_file, &session).unwrap();
        let (loaded, quarantined) = load_session_reporting_corruption(&session_file).unwrap();

        assert!(loaded.is_some());
        assert_eq!(quarantined, None);
    }

    /// Two corruptions in a row used to rename the second file over the first backup, so the
    /// only copy of the layout the user still had was destroyed by the recovery itself.
    #[test]
    fn a_second_corruption_never_overwrites_the_first_backup() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");

        fs::write(&session_file, "{ first corruption").unwrap();
        assert!(load_session_from_path(&session_file).unwrap().is_none());

        fs::write(&session_file, "{ second corruption").unwrap();
        assert!(load_session_from_path(&session_file).unwrap().is_none());

        let first = fs::read_to_string(dir.path().join("session_state.json.corrupted")).unwrap();
        assert_eq!(first, "{ first corruption");
        let second = fs::read_to_string(dir.path().join("session_state.json.corrupted.1")).unwrap();
        assert_eq!(second, "{ second corruption");
    }

    /// The SAVE path itself must preserve unreadable bytes. The helper-level test above cannot fail
    /// if save goes back to renaming the path, so this drives the real entry point: corrupt the
    /// file, save a healthy session, and require the old bytes to survive under some backup name.
    #[test]
    fn a_lock_key_does_not_depend_on_the_final_entry_existing() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("session_state.json");
        let before = super::lock_key(&target);
        fs::write(&target, b"{}").unwrap();
        let after = super::lock_key(&target);
        assert_eq!(
            before, after,
            "the key must be identical before and after the entry is created"
        );
    }

    #[test]
    fn a_relative_spelling_keys_the_same_lock_as_the_absolute_one() {
        let dir = tempdir().unwrap();
        let absolute = dir.path().join("session_state.json");
        let absolute_key = super::lock_key(&absolute);
        let relative = std::path::Path::new("session_state.json");
        let relative_key = with_cwd(dir.path(), || super::lock_key(relative));
        assert_eq!(
            absolute_key, relative_key,
            "a bare relative path must not key a second lock for the same file"
        );
    }

    #[test]
    fn a_relative_path_whose_parent_appears_later_keeps_one_lock_key() {
        let dir = tempdir().unwrap();
        let relative = Path::new("nested/session_state.json");
        let absent_parent_key = with_cwd(dir.path(), || super::lock_key(relative));
        fs::create_dir(dir.path().join("nested")).unwrap();
        let present_parent_key = with_cwd(dir.path(), || super::lock_key(relative));
        assert_eq!(
            absent_parent_key, present_parent_key,
            "creating the parent must not move the lock to a different identity"
        );
    }

    #[test]
    fn a_dotdot_component_keeps_one_lock_key_before_and_after_it_resolves() {
        // "/existing/new/../session_state.json" names the same entry whether or not "new" exists.
        // While "new" is absent the parent cannot be canonicalised and the trailing ".." has no
        // file_name, so without lexical folding the walk gives up and returns the raw spelling -
        // a second key for one unchanged path.
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("existing")).unwrap();
        let via_dotdot = dir.path().join("existing/new/../session_state.json");
        let plain = dir.path().join("existing/session_state.json");
        let before = super::lock_key(&via_dotdot);
        fs::create_dir(dir.path().join("existing/new")).unwrap();
        let after = super::lock_key(&via_dotdot);
        assert_eq!(
            before, after,
            "creating the directory named before the .. must not move the lock"
        );
        assert_eq!(
            before,
            super::lock_key(&plain),
            "both spellings name one entry and must share one key"
        );
    }

    #[test]
    fn a_relative_session_path_still_syncs_the_directory_holding_its_backup() {
        // A bare name has an EMPTY parent, which is the current directory rather than nowhere. If
        // that case skipped the sync, a relative save over a damaged document would publish a
        // backup whose name a crash could drop.
        let dir = tempdir().unwrap();
        let saved = with_cwd(dir.path(), || {
            let path = Path::new("session_state.json");
            fs::write(path, b"{ not json").unwrap();
            let session = PersistedWorkspaceSession {
                version: 3,
                ..Default::default()
            };
            super::save_session_to_path(path, &session)
        });
        assert!(saved.is_ok(), "relative save must succeed: {saved:?}");
        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("corrupted"))
            .collect();
        assert_eq!(
            backups.len(),
            1,
            "the damaged bytes must survive under a backup name: {backups:?}"
        );
        assert_eq!(
            fs::read(dir.path().join(&backups[0])).unwrap(),
            b"{ not json",
            "the backup must hold the bytes that were replaced"
        );
    }

    #[test]
    fn load_reads_through_a_symlinked_session_directory() {
        // A dotfiles-managed session directory is reached through a symlink. Load must follow it and
        // return the stored layout; classifying the linked parent as "not a directory" made load
        // report an absent session while save still wrote there, so the next save would replace the
        // real layout with an empty snapshot.
        let dir = tempdir().unwrap();
        let real = dir.path().join("real-sessions");
        fs::create_dir(&real).unwrap();
        let linked = dir.path().join("linked-sessions");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &linked).unwrap();
        #[cfg(not(unix))]
        {
            // No portable symlink API on Windows without privileges: skip rather than fail.
            return;
        }
        let stored = PersistedWorkspaceSession {
            version: 3,
            ..Default::default()
        };
        save_session_to_path(&linked.join("session_state.json"), &stored).unwrap();

        let loaded = load_session_from_path(&linked.join("session_state.json")).unwrap();
        assert_eq!(
            loaded.map(|s| s.version),
            Some(3),
            "a session behind a symlinked directory must still load"
        );
    }

    #[test]
    fn clear_leaves_a_non_regular_entry_alone() {
        let dir = tempdir().unwrap();
        let occupied = dir.path().join("session_state.json");
        fs::create_dir(&occupied).unwrap();
        super::clear_session_from_path(&occupied).unwrap();
        assert!(
            occupied.exists(),
            "a directory at the session path is not this app's document"
        );
    }

    /// Load follows a symlinked session file and save writes through it, so clear must unlink that
    /// LINK. Treating the link as "something else" reported a successful clear, left the entry at
    /// the path, and the next launch loaded the old layout again.
    #[cfg(unix)]
    #[test]
    fn clear_removes_a_symlinked_session_document() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("dotfiles-session.json");
        fs::write(&target, "{}").unwrap();
        let session_file = dir.path().join("session_state.json");
        std::os::unix::fs::symlink(&target, &session_file).unwrap();

        super::clear_session_from_path(&session_file).unwrap();

        assert!(
            std::fs::symlink_metadata(&session_file).is_err(),
            "clear must remove the session entry, including through a symlink"
        );
        assert!(target.exists(), "the symlink target is not this app's to delete");
    }

    /// A session directory this process cannot write still holds a readable layout. Failing the
    /// load there booted an empty window over a good document, which the base code never did.
    #[cfg(unix)]
    #[test]
    fn a_read_only_directory_still_loads_its_session() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        let session = PersistedWorkspaceSession {
            version: 3,
            ..Default::default()
        };
        save_session_to_path(&session_file, &session).unwrap();

        // The save created the lock file. Leaving it there would let the lock be acquired from a
        // read-only directory, so the test would pass through the normal path and never reach the
        // fallback it exists to cover.
        std::fs::remove_file(session_file.with_extension("json.lock")).unwrap();

        let original = fs::metadata(dir.path()).unwrap().permissions();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let loaded = load_session_from_path(&session_file);
        fs::set_permissions(dir.path(), original).unwrap();

        assert_eq!(
            loaded.unwrap().map(|s| s.version),
            Some(3),
            "a readable session in an unwritable directory must still load"
        );
    }

    /// A dangling link is still the session entry: its target is gone, so `metadata` reports
    /// NotFound, but the link itself must be removed or the next layout stays on disk.
    #[cfg(unix)]
    #[test]
    fn clear_removes_a_dangling_session_symlink() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        std::os::unix::fs::symlink(dir.path().join("gone.json"), &session_file).unwrap();

        super::clear_session_from_path(&session_file).unwrap();

        assert!(
            std::fs::symlink_metadata(&session_file).is_err(),
            "a dangling link is still the session entry clear must remove"
        );
    }

    /// Clear must decide under the lock. Checking the entry before taking it let a save that had
    /// written its temporary file but not yet published look like an absent path, so clear returned
    /// success and the published session reappeared on the next load.
    #[test]
    fn clear_on_a_missing_entry_is_a_no_op() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        super::clear_session_from_path(&session_file).unwrap();
        assert!(
            std::fs::symlink_metadata(&session_file).is_err(),
            "clearing an absent entry leaves nothing behind"
        );
    }

    /// A stale link left at the first backup name must not abort every later save: the slot walk
    /// finds a free name instead of failing with AlreadyExists on the same suffix forever.
    #[cfg(unix)]
    #[test]
    fn a_stale_symlink_backup_name_does_not_block_the_save() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("dotfiles-session.json");
        fs::write(&target, "{}").unwrap();
        let session_file = dir.path().join("session_state.json");
        std::os::unix::fs::symlink(&target, &session_file).unwrap();

        // Occupy the first name with a link to a DIFFERENT target, exactly as an interrupted or
        // re-pointed earlier save would leave it.
        let stale = dir.path().join("session_state.json.corrupted-stale-link");
        std::os::unix::fs::symlink(dir.path().join("elsewhere.json"), &stale).unwrap();

        let backup = super::existing_or_new_symlink_backup(
            &session_file,
            "stale",
            &target,
        )
        .expect("a taken backup name must not block preserving the current link");
        assert_ne!(backup, stale, "the stale entry is not reused for a different target");
        assert_eq!(std::fs::read_link(&backup).unwrap(), target);
        assert_eq!(
            std::fs::read_link(&stale).unwrap(),
            dir.path().join("elsewhere.json"),
            "the stale backup is left untouched"
        );
    }



    /// The publish replaces the LINK, so the link itself is the only record of where the layout
    /// lived. Recording the target path as text left the configured name a plain file and left the
    /// user's dotfile-managed path unowned; recreating the link keeps the wiring.
    #[cfg(unix)]
    #[test]
    fn save_backs_up_the_session_symlink_itself() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("dotfiles-session.json");
        fs::write(&target, "{}").unwrap();
        let session_file = dir.path().join("session_state.json");
        std::os::unix::fs::symlink(&target, &session_file).unwrap();

        let session = PersistedWorkspaceSession { version: 3, ..Default::default() };
        save_session_to_path(&session_file, &session).unwrap();

        let backup = dir
            .path()
            .read_dir()
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.to_string_lossy().ends_with("-link")
                    && std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
            })
            .expect("the replaced symlink must be backed up as a symlink, not as text");
        assert_eq!(std::fs::read_link(&backup).unwrap(), target);
        assert!(
            std::fs::symlink_metadata(&session_file).unwrap().file_type().is_file(),
            "the save publishes a regular document over the link"
        );
    }

    /// Saving repeatedly must not mint a fresh link backup each time: the entry is the same one,
    /// so the existing backup is reused. Otherwise every launch leaks a file.
    #[cfg(unix)]
    #[test]
    fn a_symlink_backup_is_not_duplicated_on_every_save() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("dotfiles-session.json");
        fs::write(&target, "{}").unwrap();
        let session_file = dir.path().join("session_state.json");
        std::os::unix::fs::symlink(&target, &session_file).unwrap();

        let session = PersistedWorkspaceSession { version: 3, ..Default::default() };
        let link_backups = || {
            dir.path()
                .read_dir()
                .unwrap()
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with("-link"))
                .count()
        };
        save_session_to_path(&session_file, &session).unwrap();
        let after_first = link_backups();
        for _ in 0..3 {
            save_session_to_path(&session_file, &session).unwrap();
        }
        assert_eq!(link_backups(), after_first, "the existing link backup is reused");
    }

    #[cfg(unix)]
    #[test]
    fn a_second_target_does_not_mint_a_backup_on_every_save() {
        // Two targets are preserved in the SAME directory. Reuse must match the target, otherwise
        // the scan finds the first `*-link` (target A), the caller's target comparison fails, and
        // the fallback mints a per-save suffix name - so the backup count grows on every save
        // instead of converging on one backup per target.
        let dir = tempdir().unwrap();
        let first = dir.path().join("dotfiles-a.json");
        let second = dir.path().join("dotfiles-b.json");
        fs::write(&first, "{}").unwrap();
        fs::write(&second, "{}").unwrap();
        let session_file = dir.path().join("session_state.json");
        std::os::unix::fs::symlink(&first, &session_file).unwrap();

        let session = PersistedWorkspaceSession { version: 3, ..Default::default() };
        save_session_to_path(&session_file, &session).unwrap();

        // Re-point the canonical entry at the SECOND target and save repeatedly.
        fs::remove_file(&session_file).unwrap();
        std::os::unix::fs::symlink(&second, &session_file).unwrap();
        save_session_to_path(&session_file, &session).unwrap();
        let after_second_target = link_backups_for(dir.path(), &second);
        assert_eq!(
            after_second_target, 1,
            "the second target gets exactly one link backup"
        );
        for _ in 0..3 {
            fs::remove_file(&session_file).unwrap();
            std::os::unix::fs::symlink(&second, &session_file).unwrap();
            save_session_to_path(&session_file, &session).unwrap();
        }
        assert_eq!(
            link_backups_for(dir.path(), &second),
            after_second_target,
            "later saves reuse the second target's backup instead of minting one per save"
        );
        assert_eq!(
            link_backups_for(dir.path(), &first),
            1,
            "the first target's backup is untouched"
        );
    }

    /// Counts `*-link` backups whose target is exactly `target`.
    fn link_backups_for(dir: &Path, target: &Path) -> usize {
        dir.read_dir()
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.contains(".corrupted") && name.contains("-link")
            })
            .filter(|entry| {
                std::fs::read_link(entry.path()).is_ok_and(|linked| linked == target)
            })
            .count()
    }



    #[test]
    fn save_preserves_the_unreadable_bytes_it_replaces() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        fs::write(&session_file, "{ torn write not json").unwrap();

        let session = PersistedWorkspaceSession {
            version: 3,
            ..Default::default()
        };
        save_session_to_path(&session_file, &session).unwrap();

        let saved = load_session_from_path(&session_file).unwrap();
        assert_eq!(saved.map(|s| s.version), Some(3), "the healthy save must be published");

        let backup = dir
            .path()
            .read_dir()
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().contains(".corrupted"))
            .expect("the unreadable document must be preserved under a backup name");
        assert_eq!(fs::read_to_string(backup.path()).unwrap(), "{ torn write not json");
    }

    /// SAVE, unlike LOAD, must preserve a non-regular entry even when nothing was loaded first:
    /// the atomic publish replaces the directory entry, so without preservation a FIFO (or a
    /// device, a socket, a dangling symlink) would vanish with no backup.
    #[cfg(unix)]
    #[test]
    fn save_preserves_a_fifo_at_the_session_path() {
        use std::os::unix::fs::FileTypeExt;

        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        let status = std::process::Command::new("mkfifo")
            .arg(&session_file)
            .status()
            .expect("mkfifo");
        assert!(status.success());

        let session = PersistedWorkspaceSession {
            version: 3,
            ..Default::default()
        };
        save_session_to_path(&session_file, &session).unwrap();

        assert!(
            fs::metadata(&session_file).unwrap().is_file(),
            "the save must publish a real session file"
        );
        let preserved: Vec<_> = dir
            .path()
            .read_dir()
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().map(|t| t.is_fifo()).unwrap_or(false))
            .collect();
        assert_eq!(preserved.len(), 1, "the FIFO must survive under its own name");
        assert_ne!(
            preserved[0].path(),
            session_file,
            "the FIFO must not be reported as the session file itself"
        );
    }

    /// A quarantine that could not happen must be an error, never a path the caller then reports to
    /// the person as the place their layout was kept. An unwritable directory is the realistic
    /// shape of that failure.
    #[cfg(unix)]
    #[test]
    fn a_failed_quarantine_is_an_error_not_a_claim() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        fs::write(&session_file, "{ broken json ...").unwrap();
        // The lock must be creatable, or the failure under test would be the LOCK, not the
        // quarantine: pre-create it while the directory is still writable so this test keeps
        // naming the behaviour it asserts.
        fs::write(session_file.with_extension("json.lock"), "").unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();

        let outcome = load_session_reporting_corruption(&session_file);

        // Restore before asserting so the tempdir can be cleaned up even on failure.
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();

        assert!(
            outcome.is_err(),
            "an impossible quarantine must fail loudly, not report a backup that does not exist"
        );
        assert_eq!(
            fs::read_to_string(&session_file).unwrap(),
            "{ broken json ...",
            "a failed quarantine must leave the only copy in place"
        );
    }

    /// The quarantine path itself must never hand back an occupied name: the old fallback returned
    /// the base name after the numbered slots ran out, which renamed over the first backup.
    #[test]
    fn quarantine_path_never_returns_an_occupied_name() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        let base = dir.path().join("session_state.json.corrupted");
        fs::write(&base, "occupied").unwrap();

        let next = quarantine_paths(&session_file)
            .find(|candidate| OpenOptions::new().write(true).create_new(true).open(candidate).is_ok())
            .expect("a free numbered slot exists");
        assert_ne!(next, base);
        assert_eq!(next, dir.path().join("session_state.json.corrupted.1"));
        // The occupied backup is untouched.
        assert_eq!(fs::read_to_string(&base).unwrap(), "occupied");
    }

    /// Two quarantines must never share a destination: the second reserves the next slot and the
    /// first backup keeps its bytes. The old `exists()`-then-`rename` check-then-act could return
    /// the same free name to both callers and rename over the earlier copy.
    #[test]
    fn a_second_quarantine_never_replaces_the_first_backup() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        let (first, mut first_file) = reserve_quarantine_path(&session_file).unwrap();
        first_file.write_all(b"earliest copy").unwrap();
        drop(first_file);

        let (second, _second_file) = reserve_quarantine_path(&session_file).unwrap();

        assert_ne!(first, second);
        assert_eq!(fs::read_to_string(&first).unwrap(), "earliest copy");
    }

    /// A corrupt file is preserved byte-for-byte at a fresh path, the load reports that backup path
    /// so the person can be told where their layout went, and the source file is NOT deleted.
    #[test]
    fn a_corrupt_file_is_preserved_and_its_backup_path_reported() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        fs::write(&session_file, "{ this is not json").unwrap();

        let (session, backup) = load_session_reporting_corruption(&session_file).unwrap();

        assert!(session.is_none());
        let backup = backup.expect("the quarantine path is reported");
        assert_eq!(fs::read_to_string(&backup).unwrap(), "{ this is not json");
        // The source is never unlinked: removing it would race a concurrent save onto the path.
        assert_eq!(fs::read_to_string(&session_file).unwrap(), "{ this is not json");
    }

    /// Preserving the same corrupt bytes twice reuses the first backup instead of accumulating
    /// identical copies on every launch, and still leaves the source in place.
    #[test]
    fn repeated_quarantine_of_identical_bytes_reuses_the_first_backup() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        fs::write(&session_file, "corrupt").unwrap();

        // Drive the REAL load entry point: the helper-level version of this test passed while the
        // production path burned a fresh name on every launch and began erroring after nine.
        let mut names = Vec::new();
        for _ in 0..12 {
            let (session, backup) = load_session_reporting_corruption(&session_file).unwrap();
            assert!(session.is_none());
            names.push(backup.expect("a corrupt document still reports its backup"));
        }

        let first = names.first().cloned().unwrap();
        assert!(
            names.iter().all(|name| *name == first),
            "identical bytes must reuse one backup, got {names:?}"
        );
        assert_eq!(fs::read_to_string(&session_file).unwrap(), "corrupt");
    }

    /// A bare relative session name resolves to the CURRENT directory, never to an empty path.
    /// `read_dir("")` and `File::open("")` both fail, so an un-normalized caller either skipped
    /// the sync that makes a backup name survive a crash, or never found the backup it already
    /// made and minted a fresh one on every launch.
    #[test]
    fn an_empty_parent_resolves_to_the_current_directory() {
        assert_eq!(entry_parent(Path::new("session_state.json")), Path::new("."));
        assert_eq!(entry_parent(Path::new("./session_state.json")), Path::new("."));
        // A real directory is passed through untouched, including when it is the root.
        assert_eq!(
            entry_parent(Path::new("/tmp/session_state.json")),
            Path::new("/tmp")
        );
        // `.parent()` of a path that IS the current directory is empty too, so this must not
        // recurse into "nowhere" or panic.
        assert_eq!(entry_parent(Path::new(".")), Path::new("."));
    }

    /// Every DISTINCT corrupt document gets its own quarantine name, and the capacity is the
    /// declared slot limit rather than nine. The old bound made the tenth distinct document fail
    /// the load outright with a bare I/O error: no notice, no backup, and the UI started empty.
    #[test]
    fn a_tenth_distinct_corrupt_document_still_gets_a_backup() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");

        for index in 0..12 {
            fs::write(&session_file, format!("{{ corrupt {index}")).unwrap();
            let (session, backup) = load_session_reporting_corruption(&session_file)
                .unwrap_or_else(|error| panic!("load {index} must not fail: {error}"));
            assert!(session.is_none());
            let backup = backup.expect("a corrupt document still reports its backup");
            assert_eq!(
                fs::read_to_string(&backup).unwrap(),
                format!("{{ corrupt {index}"),
                "backup {index} must hold exactly this document's bytes"
            );
        }

        let quarantined = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupted"))
            .count();
        assert_eq!(quarantined, 12, "each distinct document keeps its own name");
    }

    /// A file that is valid JSON but carries no usable version is not a session. Leaving it in
    /// place used to make every later save fail, so a layout the person rebuilt after recovery
    /// could never be persisted. The save path must move it aside and publish.
    #[test]
    fn a_versionless_document_never_blocks_later_saves() {
        for contents in ["{}", "[]", "[1,2,3]"] {
            let dir = tempdir().unwrap();
            let session_file = dir.path().join("session_state.json");
            fs::write(&session_file, contents).unwrap();

            // Loading reports no session and preserves the bytes instead of discarding them.
            let (loaded, backup) = load_session_reporting_corruption(&session_file).unwrap();
            assert!(loaded.is_none(), "{contents} is not a session");
            let backup = backup.expect("the unreadable document is preserved");
            assert_eq!(fs::read_to_string(&backup).unwrap(), contents);

            // The critical property: the next save succeeds and is what the next load sees.
            let session = minimal_v3_session("after-recovery");
            save_session_to_path(&session_file, &session).unwrap();
            let (reloaded, quarantined) = load_session_reporting_corruption(&session_file).unwrap();
            assert!(quarantined.is_none(), "{contents}: the fresh save is not corrupt");
            assert_eq!(
                reloaded.expect("the saved session loads").extra["testMarker"],
                serde_json::json!("after-recovery")
            );
        }
    }

    /// A writer-less FIFO at the session path must not hang the load, and must not stay there
    /// blocking every later save. Reading a FIFO blocks forever with no timeout, which held the
    /// session path lock and left startup waiting on it.
    #[cfg(unix)]
    #[test]
    fn a_fifo_at_the_session_path_is_preserved_under_a_distinct_name_without_blocking() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        // Created through libc rather than the `mkfifo` binary so the test does not depend on a
        // PATH lookup or an external program being installed.
        let c_path = std::ffi::CString::new(session_file.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: c_path is a valid NUL-terminated path that outlives the call.
        let created = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
        assert_eq!(created, 0, "the fixture needs a FIFO at the session path");

        // The load must return promptly. The test harness would hang forever on a blocking read,
        // so reaching the assertions at all is the timing-free proof it did not block.
        let (loaded, backup) = load_session_reporting_corruption(&session_file).unwrap();

        assert!(loaded.is_none(), "a FIFO is not a session");
        let backup = backup.expect("the non-regular entry is preserved, not discarded");
        // The reported backup must be a DISTINCT directory entry. Reporting the canonical path as
        // its own backup would satisfy every existence/type/inode check below by comparing the
        // path with itself, while the next save silently destroys the entry it claims to preserve.
        assert_ne!(
            backup, session_file,
            "the reported backup must not be the canonical path itself"
        );
        // The FIFO is preserved by LINK, not by renaming it away: renaming the pathname would move
        // whatever another process published there in between. The preserved entry is therefore
        // the same directory entry as the one at the session path, and the path is still a FIFO
        // until this save's own atomic publish replaces it.
        assert!(backup.exists(), "the non-regular entry is reachable at the backup name");
        use std::os::unix::fs::FileTypeExt;
        assert!(
            std::fs::symlink_metadata(&backup).unwrap().file_type().is_fifo(),
            "the backup is the FIFO itself, not a copy of something else"
        );
        // The non-displacement property, asserted rather than described: the canonical path must
        // still hold the SAME directory entry after the preserve. A `rename`-based preserve would
        // leave the path empty and would equally satisfy every assertion about the backup alone,
        // which is exactly the two-process defect this must catch.
        use std::os::unix::fs::MetadataExt;
        let at_path = std::fs::symlink_metadata(&session_file)
            .expect("the non-regular entry is still at the session path");
        assert!(
            at_path.file_type().is_fifo(),
            "the session path must not be vacated by preserving the FIFO"
        );
        assert_eq!(
            (at_path.dev(), at_path.ino()),
            {
                let metadata = std::fs::symlink_metadata(&backup).unwrap();
                (metadata.dev(), metadata.ino())
            },
            "the entry at the session path and the backup must be the same inode"
        );

        // A second load must not mint another backup name for the same entry. The two paths name
        // backups differently (load reuses an inode-matching name anywhere in the directory, save
        // appends its write suffix), so this is what proves the identity scan actually covers the
        // scheme the save path produced.
        let (again, second) = load_session_reporting_corruption(&session_file).unwrap();
        assert!(again.is_none(), "a FIFO is still not a session");
        assert_eq!(
            second.expect("the existing backup is reused"),
            backup,
            "repeated loads must reuse the backup that already holds this entry"
        );

        // The critical property: the next save now succeeds and persists, and the FIFO it
        // replaced is still recoverable at the reported backup afterwards. Asserting the backup
        // SURVIVES the save is what makes a missing-backup implementation fail here.
        let session = minimal_v3_session("after-fifo");
        save_session_to_path(&session_file, &session).unwrap();
        let (reloaded, _) = load_session_reporting_corruption(&session_file).unwrap();
        assert_eq!(
            reloaded.expect("the saved session loads").extra["testMarker"],
            serde_json::json!("after-fifo")
        );
        assert!(
            std::fs::symlink_metadata(&backup)
                .map(|metadata| metadata.file_type().is_fifo())
                .unwrap_or(false),
            "the preserved FIFO still exists at {} after the save replaced the path",
            backup.display()
        );
    }

    /// A corrupt byte INSIDE an otherwise valid document must not be laundered into a valid
    /// session by lossy decoding: the raw bytes are parsed, and invalid UTF-8 is rejected.
    #[test]
    fn a_corrupt_byte_inside_a_document_is_never_accepted_as_a_session() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        // A genuinely complete, otherwise-valid session with a raw 0xff byte spliced into one
        // string value. Building it from a real save matters: a document with missing required
        // fields would be rejected by the lossy implementation too, so it could not prove the
        // laundering defect either way.
        let healthy = serde_json::to_string(&minimal_v3_session("aa-bb")).unwrap();
        let marker = "\"aa-bb\"";
        let split = healthy.find(marker).expect("the session id is a string value");
        let mut bytes = healthy.as_bytes()[..split + 1].to_vec();
        bytes.push(0xff);
        bytes.extend_from_slice(&healthy.as_bytes()[split + 1 + 1..]);
        fs::write(&session_file, &bytes).unwrap();

        let (loaded, backup) = load_session_reporting_corruption(&session_file).unwrap();

        assert!(loaded.is_none(), "invalid UTF-8 must never be accepted as a session");
        let backup = backup.expect("the damaged document is preserved");
        assert_eq!(fs::read(&backup).unwrap(), bytes);
    }

    /// Invalid UTF-8 is corruption like any other: it must be preserved and reported, not turned
    /// into an error the caller silently reduces to an empty window.
    #[test]
    fn invalid_utf8_is_preserved_and_reported_not_swallowed() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        fs::write(&session_file, [0x7b, 0xff, 0xfe, 0x7d]).unwrap();

        let (loaded, backup) = load_session_reporting_corruption(&session_file).unwrap();

        assert!(loaded.is_none(), "unreadable bytes are not a session");
        let backup = backup.expect("invalid UTF-8 is preserved, not swallowed");
        assert_eq!(fs::read(&backup).unwrap(), vec![0x7b, 0xff, 0xfe, 0x7d]);
    }

    /// A directory left at the session path (a crashed publisher, an unpacked archive) is not a
    /// session. An EMPTY one is removed and the save proceeds; a NON-EMPTY one is reported rather
    /// than renamed, because a rename by pathname could move a session another process published
    /// in between. Failing loudly beats destroying someone else's file.
    #[test]
    #[cfg(unix)]
    fn a_directory_at_the_session_path_never_blocks_later_saves() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        fs::create_dir(&session_file).unwrap();

        // An empty directory is moved aside like any other directory (a rename cannot replace it),
        // so the save reclaims the path and the entry it displaced is still on disk.
        let session = minimal_v3_session("after-directory");
        save_session_to_path(&session_file, &session).unwrap();
        let (loaded, quarantined) = load_session_reporting_corruption(&session_file).unwrap();
        assert!(quarantined.is_none(), "the fresh save is not corrupt");
        assert_eq!(
            loaded.expect("the saved session loads").extra["testMarker"],
            serde_json::json!("after-directory")
        );

        let empty_was_preserved = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|entry| {
                entry.file_name().to_string_lossy().contains("corrupted")
                    && entry.path().is_dir()
            });
        assert!(
            empty_was_preserved,
            "even an empty directory is preserved under a backup name, never deleted"
        );

        // A NON-EMPTY directory must not wedge the app either. A directory cannot be replaced by a
        // concurrent publish (rename onto a directory fails), so moving it aside is safe, and its
        // contents have to survive at the backup name rather than being deleted.
        fs::remove_file(&session_file).unwrap();
        fs::create_dir(&session_file).unwrap();
        fs::write(session_file.join("stray"), "left behind").unwrap();
        save_session_to_path(&session_file, &minimal_v3_session("after-non-empty"))
            .expect("a non-empty directory is preserved, not a permanent obstruction");

        let preserved: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|candidate| {
                candidate
                    .file_name()
                    .map(|name| name.to_string_lossy().contains("corrupted"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(
            preserved.iter().any(|backup| backup.is_dir()),
            "the directory is preserved under a backup name, not deleted"
        );
        let with_stray: Vec<_> = preserved
            .iter()
            .filter(|backup| backup.join("stray").exists())
            .collect();
        assert_eq!(
            with_stray.len(),
            1,
            "exactly one backup holds the non-empty directory and its contents"
        );
        assert_eq!(
            fs::read_to_string(with_stray[0].join("stray")).unwrap(),
            "left behind",
            "the directory contents survive at the backup name"
        );
        let (reloaded, _) = load_session_reporting_corruption(&session_file).unwrap();
        assert_eq!(
            reloaded.expect("the session saved after the directory").extra["testMarker"],
            serde_json::json!("after-non-empty")
        );
    }

    /// Preserving must read the file once and report what it actually saw. A second read of the
    /// pathname could observe a later truncation and report an empty success with no backup.
    #[test]
    fn preservation_reports_the_healthy_snapshot_it_saw() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");

        // The path already holds a healthy session when preservation runs.
        let session = minimal_v3_session("recovered");
        save_session_to_path(&session_file, &session).unwrap();

        match preserve_corrupt_session(&session_file).unwrap() {
            Preservation::Recovered(recovered) => {
                assert_eq!(recovered.extra["testMarker"], serde_json::json!("recovered"));
            }
            Preservation::Quarantined(_) => panic!("a healthy session must not be quarantined"),
        }
        assert!(session_file.exists(), "preservation never unlinks the source");
    }

    /// A healthy session written onto the path after the corrupt bytes were read must be the file
    /// that ends up at `path`: preservation copies and never unlinks, so a later save is what the
    /// next load actually sees.
    #[test]
    fn a_later_save_of_the_same_path_is_what_the_next_load_sees() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        fs::write(&session_file, "corrupt-on-disk").unwrap();

        let backup = match preserve_corrupt_session(&session_file).unwrap() {
            Preservation::Quarantined(path) => path,
            Preservation::Recovered(_) => panic!("corrupt bytes must not be reported as recovered"),
        };
        assert_eq!(fs::read_to_string(&backup).unwrap(), "corrupt-on-disk");

        // A concurrent save publishes a healthy document, exactly as the real save path does.
        let healthy = minimal_v3_session("after-quarantine");
        save_session_to_path(&session_file, &healthy).unwrap();

        let (loaded, quarantined) = load_session_reporting_corruption(&session_file).unwrap();
        assert!(quarantined.is_none(), "the healthy session is not quarantined");
        let loaded = loaded.expect("the healthy session loads");
        assert_eq!(loaded.extra["testMarker"], serde_json::json!("after-quarantine"));
    }

    /// Preservation copies the bytes the caller READ, so a session published at the path after
    /// the classification can neither be captured as "the corrupt one" nor be moved off the path.
    ///
    /// This is the interleaving every reviewer converged on: the classification reads corrupt
    /// bytes, another process atomically publishes a healthy session, and only then does the
    /// preserve run. A path-based preserve (hard link or rename) resolves the NAME at call time,
    /// so it steals the healthy session into a backup that everyone then treats as corruption.
    #[test]
    fn preservation_copies_the_bytes_it_read_not_whatever_the_path_holds_now() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        let corrupt: &[u8] = b"corrupt-classified-bytes";

        // Step 1: the classification happens here, and these are the bytes that must survive.
        // Step 2: a concurrent save publishes a healthy session before preservation runs.
        let healthy = minimal_v3_session("published-between-read-and-preserve");
        save_session_to_path(&session_file, &healthy).unwrap();

        // Step 3: preservation runs with the already-read bytes.
        let backup = preserve_read_bytes(&session_file, Some("race"), corrupt).unwrap();

        assert_eq!(
            fs::read(&backup).unwrap(),
            corrupt,
            "the backup must hold the classified bytes, not the session published meanwhile"
        );
        // The publisher's session is still exactly where it was published.
        let (loaded, quarantined) =
            load_session_reporting_corruption(&session_file).unwrap();
        assert!(quarantined.is_none(), "the published session must not be quarantined");
        assert_eq!(
            loaded.expect("the published session survives preservation").extra["testMarker"],
            serde_json::json!("published-between-read-and-preserve")
        );
    }

    /// Every quarantine name taken is an explicit error, never a fallback onto an occupied name.
    #[test]
    fn exhaustion_of_every_quarantine_name_is_an_error() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        fs::write(&session_file, "corrupt").unwrap();
        // A small bound keeps this test to a handful of files; the real limit is only larger.
        for candidate in quarantine_paths_up_to(&session_file, 3) {
            fs::write(&candidate, "occupied").unwrap();
        }

        let error = reserve_quarantine_path_within(&session_file, 3).unwrap_err();

        assert!(error.to_string().contains("every backup name is taken"), "{error}");
        // No existing backup was replaced, and the corrupt file is still there to retry on.
        assert_eq!(
            fs::read_to_string(dir.path().join("session_state.json.corrupted")).unwrap(),
            "occupied"
        );
        assert_eq!(fs::read_to_string(&session_file).unwrap(), "corrupt");
    }

    /// A healthy file is still loaded untouched: quarantine must not touch the happy path.
    #[test]
    fn a_valid_session_file_is_not_quarantined() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        let session = minimal_v3_session("healthy");

        save_session_to_path(&session_file, &session).unwrap();
        let loaded = load_session_from_path(&session_file).unwrap().unwrap();

        assert_eq!(loaded.extra["testMarker"], serde_json::json!("healthy"));
        assert!(session_file.exists());
        assert!(!dir.path().join("session_state.json.corrupted").exists());
    }

    fn minimal_v3_session(marker: &str) -> PersistedWorkspaceSession {
        let mut session = PersistedWorkspaceSession { version: 3, ..Default::default() };
        session.extra.insert(
            "testMarker".to_string(),
            serde_json::Value::String(marker.to_string()),
        );
        session
    }

    /// The first v3 save over a v2 file (an upgraded install, or a restore seeded at v2) backs the
    /// old file up and must then publish. Windows cannot open a directory as a file, so a parent
    /// directory fsync there failed this save with IO_ERROR after the backup already existed, and
    /// every later save failed the same way (Windows QA run-q11: the restored pane never attached).
    #[test]
    fn test_first_v3_save_over_v2_session_publishes_and_keeps_backup() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        let legacy = br#"{"version":2,"timestamp":1,"activeWorkspaceId":"w","workspaces":{}}"#.to_vec();
        fs::write(&session_file, &legacy).unwrap();

        save_session_to_path(&session_file, &minimal_v3_session("upgraded")).unwrap();

        let loaded = load_session_from_path(&session_file).unwrap().unwrap();
        assert_eq!(loaded.version, 3);
        assert_eq!(loaded.extra["testMarker"], "upgraded");
        assert_eq!(fs::read(session_file.with_extension("json.pre-v3")).unwrap(), legacy);
    }

    /// A v2 file written again after an earlier upgrade (an older build ran in between) must not
    /// be compared with the earlier backup: that refused every save, so the session was never
    /// written again (daemon-lifetime QA Q14 after Q11, run 20261111t153429z).
    #[test]
    fn test_v3_save_over_rewritten_v2_session_keeps_both_backups() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        let earlier = br#"{"version":2,"timestamp":1,"activeWorkspaceId":"a","workspaces":{}}"#.to_vec();
        let rewritten = br#"{"version":2,"timestamp":2,"activeWorkspaceId":"b","workspaces":{}}"#.to_vec();
        fs::write(&session_file, &earlier).unwrap();
        save_session_to_path(&session_file, &minimal_v3_session("first")).unwrap();
        fs::write(&session_file, &rewritten).unwrap();

        save_session_to_path(&session_file, &minimal_v3_session("second")).unwrap();

        let loaded = load_session_from_path(&session_file).unwrap().unwrap();
        assert_eq!(loaded.extra["testMarker"], "second");
        assert_eq!(fs::read(session_file.with_extension("json.pre-v3")).unwrap(), earlier);
        let newer: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".pre-v3-"))
            .collect();
        assert_eq!(newer.len(), 1);
        assert_eq!(fs::read(newer[0].path()).unwrap(), rewritten);
    }

    #[test]
    fn test_save_self_heals_unreadable_session() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        let unreadable = b"{\"version\":3,}trailing bytes".to_vec();
        fs::write(&session_file, &unreadable).unwrap();

        // A mixed/truncated publish must not block every future save.
        save_session_to_path(&session_file, &minimal_v3_session("healed")).unwrap();

        let loaded = load_session_from_path(&session_file).unwrap().unwrap();
        assert_eq!(loaded.version, 3);
        assert_eq!(loaded.extra["testMarker"], "healed");

        // The unreadable bytes are preserved, not silently destroyed.
        let preserved: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupted"))
            .collect();
        assert_eq!(preserved.len(), 1);
        assert_eq!(fs::read(preserved[0].path()).unwrap(), unreadable);
    }

    #[test]
    fn test_concurrent_saves_never_publish_mixed_documents() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        save_session_to_path(&session_file, &minimal_v3_session("seed")).unwrap();

        let writer_count = 4usize;
        let iterations = 25u32;
        let mut writers = Vec::new();
        for writer in 0..writer_count {
            let path = session_file.clone();
            writers.push(std::thread::spawn(move || {
                for iteration in 0..iterations {
                    // Alternate short and long payloads so a shared temporary inode
                    // would mix documents (the trailing-characters corruption shape).
                    let marker = format!("writer-{writer}-iter-{iteration}-payload ");
                    let filler = marker.repeat(if iteration % 2 == 0 { 96 } else { 24 });
                    let mut session = PersistedWorkspaceSession { version: 3, ..Default::default() };
                    session
                        .extra
                        .insert("marker".into(), serde_json::Value::String(filler));
                    save_session_to_path(&path, &session).unwrap();
                }
            }));
        }

        // A concurrent reader must never observe a mixed document on the published path.
        let reader_path = session_file.clone();
        let reader = std::thread::spawn(move || {
            for _ in 0..4000 {
                if let Ok(bytes) = fs::read(&reader_path) {
                    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_else(|e| {
                        panic!(
                            "mixed session document published: {e} ({} bytes)",
                            bytes.len()
                        )
                    });
                }
            }
        });

        for writer in writers {
            writer.join().unwrap();
        }
        reader.join().unwrap();

        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary files left behind: {leftovers:?}"
        );

        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&session_file).unwrap()).unwrap();
        let marker = value["marker"].as_str().unwrap();
        assert!(marker.starts_with("writer-"));
    }
}
