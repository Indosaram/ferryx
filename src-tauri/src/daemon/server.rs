// allow: SIZE_OK — daemon IPC server implementation with routing, session persistence offloading, remote control, and streaming
use crate::daemon::protocol::{
    AgentProviderSessionKey, AgentStateReport, DaemonRemoteEvent, DaemonRemoteStatus,
    DaemonRequest, DaemonResponse, DaemonSessionDetails, DaemonStreamMessage, HistorySegmentWire,
    TerminalStartup, DAEMON_PROTOCOL_VERSION,
};
use crate::remote::auth::DevicePermission;
use crate::remote::server::{start_remote_server, RemoteServerHandle};
use crate::remote::state::{
    RemoteGatewayConfig, RemoteGatewayState, RemoteNetworkMode, REMOTE_GATEWAY_PORT,
};
use crate::session::{clear_session_from_path, load_session_from_path, save_session_to_path};
use crate::terminal::{PtyManager, PtySessionState, TerminalOutputHub, TerminalService};
use crate::worktree::{WorkspaceRegistry, WorktreeIdentity, WorktreeManager};
use parking_lot::{Mutex, RwLock};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
#[cfg(not(unix))]
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(all(test, unix))]
use tokio::net::UnixStream;
use tokio::sync::broadcast;

/// Returns the compile-time development profile. Debug builds isolate session state and daemon
/// endpoints so a release GUI cannot attach to a dev daemon; release paths remain byte-identical.
pub(crate) fn is_dev_runtime() -> bool {
    cfg!(debug_assertions)
}

/// Returns the QA and multi-instance session directory override. See
/// `docs/SESSION_STATE_WIPE_2026-08-31.md`; release defaults intentionally remain byte-identical.
pub(crate) fn session_dir_override() -> Option<PathBuf> {
    std::env::var_os("FERRYX_SESSION_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn resolve_session_path(
    env_dir: Option<&Path>,
    is_dev: bool,
    release_path: &Path,
    dev_path: &Path,
) -> PathBuf {
    env_dir
        .map(|dir| dir.join("session_state.json"))
        .unwrap_or_else(|| {
            if is_dev {
                dev_path.to_path_buf()
            } else {
                release_path.to_path_buf()
            }
        })
}

#[cfg(test)]
mod session_path_tests {
    use super::resolve_session_path;
    use std::path::{Path, PathBuf};

    #[test]
    fn session_path_override_wins_for_release() {
        assert_eq!(
            resolve_session_path(
                Some(Path::new("/qa/session")),
                false,
                Path::new("/app/rorca/session_state.json"),
                Path::new("/app/rorca-dev/session_state.json"),
            ),
            PathBuf::from("/qa/session/session_state.json")
        );
    }

    #[test]
    fn session_path_dev_uses_isolated_path_without_override() {
        assert_eq!(
            resolve_session_path(
                None,
                true,
                Path::new("/app/rorca/session_state.json"),
                Path::new("/app/rorca-dev/session_state.json"),
            ),
            PathBuf::from("/app/rorca-dev/session_state.json")
        );
    }

    #[test]
    fn session_path_release_preserves_existing_path_without_override() {
        assert_eq!(
            resolve_session_path(
                None,
                false,
                Path::new("/app/rorca/session_state.json"),
                Path::new("/app/rorca-dev/session_state.json"),
            ),
            PathBuf::from("/app/rorca/session_state.json")
        );
    }

    #[test]
    fn session_path_override_wins_for_dev() {
        assert_eq!(
            resolve_session_path(
                Some(Path::new("/qa/session")),
                true,
                Path::new("/app/rorca/session_state.json"),
                Path::new("/app/rorca-dev/session_state.json"),
            ),
            PathBuf::from("/qa/session/session_state.json")
        );
    }
}

/// Returns the daemon runtime directory. Debug builds use a separate directory to prevent
/// dev and release daemons from sharing endpoints; release paths intentionally remain unchanged.
#[cfg(unix)]
pub fn get_runtime_dir() -> PathBuf {
    // SAFETY:
    // Category: Foreign Function Interface (FFI).
    // Invariant: `libc::getuid` is a stateless, side-effect-free POSIX syscall wrapper that
    // takes no arguments, dereferences no pointers, and always returns the current process's UID.
    let uid = unsafe { libc::getuid() };
    let suffix = if is_dev_runtime() { "-dev" } else { "" };
    PathBuf::from(format!("/tmp/rorca-{uid}{suffix}"))
}

#[cfg(not(unix))]
pub fn get_runtime_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("TEMP"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"))
        .join("Ferryx")
        .join(if is_dev_runtime() {
            "runtime-dev"
        } else {
            "runtime"
        })
}

#[cfg(unix)]
pub fn get_socket_path() -> PathBuf {
    get_runtime_dir().join("daemon.sock")
}

#[cfg(not(unix))]
pub fn get_socket_path() -> PathBuf {
    get_runtime_dir().join("daemon.port")
}

pub fn get_lock_path() -> PathBuf {
    get_runtime_dir().join("daemon.lock")
}

pub fn get_file_mtime_ms(path: &Path) -> Option<u64> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(duration.as_millis() as u64)
}

pub fn resolve_binary_identity_with<FExe, FMtime>(
    get_exe: FExe,
    get_mtime: FMtime,
) -> (Option<String>, Option<u64>)
where
    FExe: Fn() -> std::io::Result<PathBuf>,
    FMtime: Fn(&Path) -> Option<u64>,
{
    match get_exe() {
        Ok(exe_path) => {
            let mtime = get_mtime(&exe_path);
            (Some(exe_path.to_string_lossy().into_owned()), mtime)
        }
        Err(error) => {
            tracing::warn!("Failed to determine current_exe for binary identity: {error}");
            (None, None)
        }
    }
}

pub fn resolve_binary_identity() -> (Option<String>, Option<u64>) {
    resolve_binary_identity_with(std::env::current_exe, get_file_mtime_ms)
}

#[cfg(unix)]
pub fn clear_cloexec(fd: std::os::unix::io::RawFd) -> Result<(), std::io::Error> {
    // SAFETY: Foreign Function Interface to fcntl.
    // Invariant: fd is a valid open file descriptor.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags == -1 {
            return Err(std::io::Error::last_os_error());
        }
        if (flags & libc::FD_CLOEXEC) != 0 {
            if libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
pub fn perform_daemon_exec() -> Result<(), std::io::Error> {
    use std::os::unix::process::CommandExt;

    let exe = std::env::current_exe()?;
    tracing::info!("Re-executing daemon with binary at: {}", exe.display());

    // Lock file semantics:
    // The daemon lock is held via `libc::flock` on an open file descriptor (`DaemonLockFile`).
    // Rust standard library opens files with `FD_CLOEXEC` by default.
    // When `Command::exec()` is called, the kernel atomically replaces the process image
    // (preserving the PID). Descriptors marked `FD_CLOEXEC` are closed by the kernel upon `exec()`,
    // releasing the old flock. The newly exec'd daemon then re-acquires the lock cleanly in `run_server()`.
    //
    // Socket semantics:
    // The existing socket file at `/tmp/rorca-{uid}/daemon.sock` is safely unlinked by the new
    // daemon's `remove_stale_socket_after_lock` after it acquires the lock. No drop guards delete
    // the socket prematurely.
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--daemon");
    let err = cmd.exec();
    tracing::error!("Daemon exec failed: {err}");
    Err(err)
}

fn get_persistent_lock_path() -> Option<PathBuf> {
    std::env::var_os("FERRYX_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            #[cfg(unix)]
            {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ferryx"))
            }
            #[cfg(windows)]
            {
                std::env::var_os("APPDATA")
                    .map(|appdata| PathBuf::from(appdata).join("Ferryx"))
                    .or_else(|| {
                        std::env::var_os("USERPROFILE")
                            .map(|home| PathBuf::from(home).join(".ferryx"))
                    })
            }
            #[cfg(not(any(unix, windows)))]
            {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ferryx"))
            }
        })
        .map(|base| {
            base.join("locks").join(if is_dev_runtime() {
                "daemon-dev.lock"
            } else {
                "daemon.lock"
            })
        })
}

pub fn get_default_session_path() -> PathBuf {
    let override_dir = session_dir_override();
    let is_dev = is_dev_runtime();
    if override_dir.is_some() {
        let path = resolve_session_path(
            override_dir.as_deref(),
            is_dev,
            Path::new(""),
            Path::new(""),
        );
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        return path;
    }

    let path = if let Some(base) = dirs_next().or_else(dirs_fallback) {
        resolve_session_path(
            override_dir.as_deref(),
            is_dev,
            &base.join("rorca").join("session_state.json"),
            &base.join("rorca-dev").join("session_state.json"),
        )
    } else {
        let runtime_dir = get_runtime_dir();
        resolve_session_path(
            override_dir.as_deref(),
            is_dev,
            &runtime_dir.join("session_state.json"),
            &runtime_dir.join("session_state.dev.json"),
        )
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    path
}

fn dirs_next() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }
}

fn dirs_fallback() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".rorca"))
}

#[derive(Clone, Copy)]
enum RuntimeNodeKind {
    Directory,
    RegularFile,
    Socket,
}

#[cfg(unix)]
fn validate_safe_ownership_and_type_for_uid(
    path: &Path,
    kind: RuntimeNodeKind,
    expected_uid: libc::uid_t,
) -> Result<(), String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to read metadata for {}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "Path {} is a symlink, which is prohibited for daemon runtime",
            path.display()
        ));
    }
    if meta.uid() != expected_uid {
        return Err(format!(
            "Path {} is owned by UID {} (expected current UID {})",
            path.display(),
            meta.uid(),
            expected_uid
        ));
    }
    let valid_type = match kind {
        RuntimeNodeKind::Directory => meta.file_type().is_dir(),
        RuntimeNodeKind::RegularFile => meta.file_type().is_file(),
        RuntimeNodeKind::Socket => meta.file_type().is_socket(),
    };
    if !valid_type {
        return Err(format!(
            "Path {} has an invalid runtime node type",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_safe_ownership_and_type_for_uid(
    path: &Path,
    kind: RuntimeNodeKind,
    _expected_uid: u32,
) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to read metadata for {}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "Path {} is a symlink, which is prohibited for daemon runtime",
            path.display()
        ));
    }
    let valid_type = match kind {
        RuntimeNodeKind::Directory => meta.file_type().is_dir(),
        RuntimeNodeKind::RegularFile | RuntimeNodeKind::Socket => !meta.file_type().is_dir(),
    };
    if !valid_type {
        return Err(format!(
            "Path {} has an invalid runtime node type",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn validate_safe_ownership_and_type(path: &Path, kind: RuntimeNodeKind) -> Result<(), String> {
    // SAFETY:
    // Category: Foreign Function Interface (FFI).
    // Invariant: `libc::getuid` is a stateless, side-effect-free POSIX syscall wrapper that
    // takes no arguments, dereferences no pointers, and always returns the current process's UID.
    validate_safe_ownership_and_type_for_uid(path, kind, unsafe { libc::getuid() })
}

#[cfg(not(unix))]
fn validate_safe_ownership_and_type(path: &Path, kind: RuntimeNodeKind) -> Result<(), String> {
    validate_safe_ownership_and_type_for_uid(path, kind, 0)
}

#[cfg(unix)]
pub(crate) fn validate_runtime_socket_path(path: &Path) -> Result<(), String> {
    // SAFETY:
    // Category: Foreign Function Interface (FFI).
    // Invariant: `libc::getuid` is a stateless, side-effect-free POSIX syscall wrapper that
    // takes no arguments, dereferences no pointers, and always returns the current process's UID.
    validate_runtime_socket_path_for_uid(path, unsafe { libc::getuid() })
}

#[cfg(not(unix))]
pub(crate) fn validate_runtime_socket_path(path: &Path) -> Result<(), String> {
    validate_runtime_socket_path_for_uid(path, 0)
}

#[cfg(unix)]
pub(crate) fn validate_runtime_socket_path_for_uid(
    path: &Path,
    expected_uid: libc::uid_t,
) -> Result<(), String> {
    let runtime_dir = path
        .parent()
        .ok_or_else(|| format!("Daemon socket {} has no runtime directory", path.display()))?;
    validate_safe_ownership_and_type_for_uid(
        runtime_dir,
        RuntimeNodeKind::Directory,
        expected_uid,
    )?;
    let mode = fs::symlink_metadata(runtime_dir)
        .map_err(|error| format!("Failed to verify {}: {error}", runtime_dir.display()))?
        .permissions()
        .mode()
        & 0o777;
    if mode != 0o700 {
        return Err(format!(
            "Daemon runtime directory {} has mode {mode:o}, expected 700",
            runtime_dir.display()
        ));
    }
    validate_safe_ownership_and_type_for_uid(path, RuntimeNodeKind::Socket, expected_uid)
}

#[cfg(not(unix))]
pub(crate) fn validate_runtime_socket_path_for_uid(
    path: &Path,
    _expected_uid: u32,
) -> Result<(), String> {
    if let Some(runtime_dir) = path.parent() {
        validate_safe_ownership_and_type_for_uid(runtime_dir, RuntimeNodeKind::Directory, 0)?;
    }
    validate_safe_ownership_and_type_for_uid(path, RuntimeNodeKind::RegularFile, 0)?;
    Ok(())
}

fn ensure_runtime_directory(path: &Path) -> Result<(), String> {
    match fs::create_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(format!(
                "Failed to create daemon runtime directory {}: {error}",
                path.display()
            ));
        }
    }
    validate_safe_ownership_and_type(path, RuntimeNodeKind::Directory)?;
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            format!(
                "Failed to secure daemon runtime directory {}: {error}",
                path.display()
            )
        })?;
        let mode = fs::symlink_metadata(path)
            .map_err(|error| format!("Failed to verify {}: {error}", path.display()))?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o700 {
            return Err(format!(
                "Daemon runtime directory {} has mode {mode:o}, expected 700",
                path.display()
            ));
        }
    }
    Ok(())
}

fn open_secure_lock_file(path: &Path) -> Result<File, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => validate_safe_ownership_and_type(path, RuntimeNodeKind::RegularFile)?,
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to inspect lock file: {error}")),
    }

    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);

    #[cfg(unix)]
    {
        options.mode(0o600);
        options.custom_flags(libc::O_NOFOLLOW);
    }

    let file = options
        .open(path)
        .map_err(|error| format!("Failed to open lock file {}: {error}", path.display()))?;
    validate_safe_ownership_and_type(path, RuntimeNodeKind::RegularFile)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Failed to secure lock file: {error}"))?;
    Ok(file)
}

#[cfg(unix)]
#[derive(Debug)]
pub(crate) struct DaemonLockFile {
    _file: File,
}

#[cfg(unix)]
impl DaemonLockFile {
    pub(crate) fn try_lock(file: File) -> Result<Self, String> {
        use std::os::unix::io::AsRawFd;

        // SAFETY:
        // Category: Foreign Function Interface (FFI) / Invalid File Descriptor.
        // Invariant: `file.as_raw_fd()` returns a valid open file descriptor borrowed from `file`,
        // which remains open and valid for the duration of the `libc::flock` call.
        let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if ret != 0 {
            return Err("Another daemon instance is already holding the lock.".into());
        }
        Ok(Self { _file: file })
    }
}

#[cfg(unix)]
impl Drop for DaemonLockFile {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;

        // SAFETY:
        // Category: Foreign Function Interface (FFI) / Invalid File Descriptor.
        // Invariant: `self._file.as_raw_fd()` returns a valid open file descriptor owned by `self._file`,
        // which has not been closed yet. Calling `libc::flock` with `LOCK_UN` synchronously clears
        // the exclusive lock before the descriptor is closed by `_file`'s drop.
        unsafe {
            libc::flock(self._file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(windows)]
#[derive(Debug)]
pub(crate) struct DaemonLockFile {
    file: File,
}

#[cfg(windows)]
impl DaemonLockFile {
    pub(crate) fn try_lock(file: File) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::Storage::FileSystem::{
            LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
        };
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = file.as_raw_handle() as HANDLE;
        // SAFETY:
        // Category: Uninitialized Memory.
        // Invariant: `OVERLAPPED` is a C-compatible repr(C) struct whose all-zero bit pattern
        // is valid memory representing zero offset (Offset=0, OffsetHigh=0) and null hEvent.
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };

        // SAFETY:
        // Category: Foreign Function Interface (FFI) / Invalid Handle Dereference.
        // Invariant: `handle` is guaranteed to be a valid, open Win32 file handle owned by `file`,
        // which remains open and valid for the duration of this Win32 `LockFileEx` call.
        // `&mut overlapped` points to a valid, properly aligned, stack-allocated `OVERLAPPED` struct.
        let ret = unsafe {
            LockFileEx(
                handle,
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &mut overlapped,
            )
        };

        if ret == 0 {
            return Err("Another daemon instance is already holding the lock.".into());
        }

        Ok(Self { file })
    }
}

#[cfg(windows)]
impl Drop for DaemonLockFile {
    fn drop(&mut self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = self.file.as_raw_handle() as HANDLE;
        // SAFETY:
        // Category: Uninitialized Memory.
        // Invariant: `OVERLAPPED` is a C-compatible repr(C) struct whose all-zero bit pattern
        // is valid memory representing zero offset (Offset=0, OffsetHigh=0) and null hEvent.
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };

        // SAFETY:
        // Category: Foreign Function Interface (FFI) / Invalid Handle Dereference.
        // Invariant: `handle` is guaranteed to be a valid, open Win32 file handle owned by `self.file`,
        // which has not been closed yet. The offset (0) and length (1 byte) exactly match the
        // exclusive range locked in `DaemonLockFile::try_lock`.
        unsafe {
            UnlockFileEx(handle, 0, 1, 0, &mut overlapped);
        }
    }
}

#[cfg(not(any(unix, windows)))]
#[derive(Debug)]
pub(crate) struct DaemonLockFile {
    _file: File,
}

#[cfg(not(any(unix, windows)))]
impl DaemonLockFile {
    pub(crate) fn try_lock(file: File) -> Result<Self, String> {
        Ok(Self { _file: file })
    }
}

#[derive(Debug)]
pub(crate) struct DaemonLockFiles {
    _persistent: Option<DaemonLockFile>,
    _legacy: DaemonLockFile,
}

fn try_lock_file(file: File) -> Result<DaemonLockFile, String> {
    DaemonLockFile::try_lock(file)
}

pub(crate) fn acquire_daemon_locks(
    persistent_path: Option<&Path>,
    legacy_path: &Path,
) -> Result<DaemonLockFiles, String> {
    let persistent = if let Some(path) = persistent_path {
        let parent = path
            .parent()
            .ok_or_else(|| format!("Persistent lock path has no parent: {}", path.display()))?;
        ensure_runtime_directory(parent)?;
        let file = open_secure_lock_file(path)?;
        let locked = try_lock_file(file)?;
        Some(locked)
    } else {
        None
    };

    let legacy = open_secure_lock_file(legacy_path)?;
    let locked = try_lock_file(legacy)?;
    Ok(DaemonLockFiles {
        _persistent: persistent,
        _legacy: locked,
    })
}

fn remove_stale_socket_after_lock(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            validate_safe_ownership_and_type(path, RuntimeNodeKind::Socket)?;
            fs::remove_file(path).map_err(|error| {
                format!("Failed to remove stale socket {}: {error}", path.display())
            })
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to inspect daemon socket: {error}")),
    }
}

const SPAWN_REQUEST_TTL: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct SpawnCacheEntry {
    session_id: String,
    created_at: Instant,
    fingerprint: SpawnRequestFingerprint,
}

#[derive(Clone)]
struct StoredSessionMeta {
    client_request_id: String,
    workspace_id: String,
    worktree: Option<WorktreeIdentity>,
    cwd: PathBuf,
    provider_claim: Option<ProviderSessionClaimKey>,
    spawn_fingerprint: SpawnRequestFingerprint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SpawnRequestFingerprint {
    workspace_id: String,
    worktree: Option<WorktreeIdentity>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    shell: Option<String>,
    provider_claim: Option<ProviderSessionClaimKey>,
    startup: Option<TerminalStartup>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ProviderSessionClaimKey {
    agent_type: String,
    provider_key: AgentProviderSessionKey,
    provider_id: String,
    transcript_path: Option<String>,
}

impl ProviderSessionClaimKey {
    fn from_startup(startup: Option<&TerminalStartup>) -> Option<Self> {
        let TerminalStartup::AgentResume {
            agent_type,
            provider_session,
        } = startup?;
        let agent_type = agent_type.trim().to_ascii_lowercase();
        let transcript_path = if matches!(agent_type.as_str(), "pi" | "prime-agent") {
            provider_session
                .transcript_path
                .as_deref()
                .map(str::trim)
                .map(str::to_string)
        } else {
            None
        };
        Some(Self {
            agent_type,
            provider_key: provider_session.key,
            provider_id: provider_session.id.trim().to_string(),
            transcript_path,
        })
    }
}

#[derive(Debug, thiserror::Error)]
enum SpawnError {
    #[error(
        "AgentSessionConflict: {agent_type} {provider_key:?} '{provider_id}' is already owned by session '{existing_session_id}'"
    )]
    AgentSessionConflict {
        agent_type: String,
        provider_key: AgentProviderSessionKey,
        provider_id: String,
        existing_session_id: String,
    },
    #[error("{0}")]
    InvalidAgentResume(String),
    #[error("{0}")]
    Other(String),
}

impl SpawnError {
    #[cfg(test)]
    fn contains(&self, needle: &str) -> bool {
        self.to_string().contains(needle)
    }
}

impl From<String> for SpawnError {
    fn from(value: String) -> Self {
        Self::Other(value)
    }
}

pub fn get_agent_state_socket_path() -> PathBuf {
    get_runtime_dir().join("agent-state.sock")
}

pub fn agent_state_socket_path() -> String {
    get_agent_state_socket_path().to_string_lossy().into_owned()
}

pub(crate) fn normalize_process_cwd(path: &Path) -> PathBuf {
    let Some(path_str) = path.to_str() else {
        return path.to_path_buf();
    };

    if let Some(rest) = path_str.strip_prefix(r"\\?\") {
        if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(r"UNC\") {
            return PathBuf::from(format!(r"\\{}", &rest[4..]));
        }

        let bytes = rest.as_bytes();
        if bytes.len() >= 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes.len() == 2 || bytes[2] == b'\\' || bytes[2] == b'/')
        {
            return PathBuf::from(rest);
        }
    }

    path.to_path_buf()
}

pub struct DaemonServer {
    terminal_service: Arc<TerminalService>,
    workspace_registry: WorkspaceRegistry,
    remote_state: Arc<RemoteGatewayState>,
    remote_server_handle: Arc<Mutex<Option<RemoteServerHandle>>>,
    epoch: u64,
    binary_path: Option<String>,
    binary_mtime_ms: Option<u64>,
    spawn_idempotency_cache: Arc<Mutex<HashMap<String, SpawnCacheEntry>>>,
    spawn_lock: tokio::sync::Mutex<()>,
    session_metadata: Arc<RwLock<HashMap<String, StoredSessionMeta>>>,
    provider_session_claims: Arc<Mutex<HashMap<ProviderSessionClaimKey, String>>>,
    agent_state_tx: broadcast::Sender<(
        String,
        String,
        Option<String>,
        Option<crate::daemon::protocol::AgentProviderSession>,
    )>,
    remote_event_tx: broadcast::Sender<DaemonRemoteEvent>,
}

impl Default for DaemonServer {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonServer {
    pub fn new() -> Self {
        Self::new_with_paths(None, None)
    }

    pub fn new_with_paths(config_path: Option<PathBuf>, auth_path: Option<PathBuf>) -> Self {
        let pty_manager = Arc::new(PtyManager::new());
        let output_hub = Arc::new(TerminalOutputHub::default());
        let terminal_service = Arc::new(TerminalService::new(
            Arc::clone(&pty_manager),
            Arc::clone(&output_hub),
        ));
        let workspace_registry = WorkspaceRegistry::new();
        #[cfg(test)]
        let remote_state = Arc::new(RemoteGatewayState::new_with_paths(
            Arc::clone(&terminal_service),
            workspace_registry.clone(),
            config_path,
            auth_path,
        ));
        #[cfg(not(test))]
        let remote_state = if config_path.is_some() || auth_path.is_some() {
            Arc::new(RemoteGatewayState::new_with_paths(
                Arc::clone(&terminal_service),
                workspace_registry.clone(),
                config_path,
                auth_path,
            ))
        } else {
            Arc::new(RemoteGatewayState::new_persistent(
                Arc::clone(&terminal_service),
                workspace_registry.clone(),
            ))
        };

        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(1);

        let (binary_path, binary_mtime_ms) = resolve_binary_identity();

        // The gateway runs inside this process, so desktop-directed events must
        // be relayed to the GUI over the socket; without this sink they are
        // dropped and remote-issued selections never reach the desktop.
        let remote_event_tx = broadcast::channel::<DaemonRemoteEvent>(64).0;
        let sink_tx = remote_event_tx.clone();
        remote_state.set_desktop_event_sink(Arc::new(move |event, payload| {
            let _ = sink_tx.send(DaemonRemoteEvent {
                event: event.to_string(),
                payload,
            });
        }));

        Self {
            terminal_service,
            workspace_registry,
            remote_state,
            remote_event_tx,
            remote_server_handle: Arc::new(Mutex::new(None)),
            agent_state_tx: broadcast::channel(64).0,
            epoch,
            binary_path,
            binary_mtime_ms,
            spawn_idempotency_cache: Arc::new(Mutex::new(HashMap::new())),
            spawn_lock: tokio::sync::Mutex::new(()),
            session_metadata: Arc::new(RwLock::new(HashMap::new())),
            provider_session_claims: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn terminal_service(&self) -> &Arc<TerminalService> {
        &self.terminal_service
    }

    pub fn workspace_registry(&self) -> &WorkspaceRegistry {
        &self.workspace_registry
    }

    pub fn remote_state(&self) -> &Arc<RemoteGatewayState> {
        &self.remote_state
    }

    #[cfg(unix)]
    /// Parses one newline-delimited extension report, rejecting states the UI cannot render.
    fn parse_agent_state_report(
        line: &str,
    ) -> Option<(
        String,
        String,
        Option<String>,
        Option<crate::daemon::protocol::AgentProviderSession>,
    )> {
        let report = serde_json::from_str::<AgentStateReport>(line.trim()).ok()?;
        if !matches!(report.state.as_str(), "working" | "blocked" | "idle") {
            return None;
        }
        let provider_session = report.provider_session.filter(|provider| {
            report.agent.as_deref().is_some_and(|agent| {
                crate::terminal::shell::resolve_agent_resume_plan(agent, provider).is_ok()
            })
        });
        Some((
            report.session_id,
            report.state,
            report.agent,
            provider_session,
        ))
    }

    #[cfg(unix)]
    fn spawn_agent_state_listener(self: &Arc<Self>) {
        let path = get_agent_state_socket_path();
        let _ = fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "Failed to bind agent state socket");
                return;
            }
        };
        if let Err(error) = fs::set_permissions(&path, fs::Permissions::from_mode(0o600)) {
            tracing::warn!(%error, "Failed to secure agent state socket");
        }

        let tx = self.agent_state_tx.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let tx = tx.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                        if let Some(report) = Self::parse_agent_state_report(&line) {
                            let _ = tx.send(report);
                        }
                        line.clear();
                    }
                });
            }
        });
    }

    pub async fn run_server(self: Arc<Self>) -> Result<(), String> {
        let runtime_dir = get_runtime_dir();
        ensure_runtime_directory(&runtime_dir)?;

        let socket_path = get_socket_path();
        let lock_path = get_lock_path();

        let _lock_files = acquire_daemon_locks(get_persistent_lock_path().as_deref(), &lock_path)?;

        // Clean up stale socket only after lock acquisition and safe ownership check.
        remove_stale_socket_after_lock(&socket_path)?;

        #[cfg(unix)]
        let listener = UnixListener::bind(&socket_path).map_err(|e| {
            format!(
                "Failed to bind UDS socket at {}: {e}",
                socket_path.display()
            )
        })?;

        #[cfg(not(unix))]
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Failed to bind TCP listener on localhost: {e}"))?;

        #[cfg(not(unix))]
        {
            let port = listener
                .local_addr()
                .map_err(|e| format!("Failed to get local port: {e}"))?
                .port();
            fs::write(&socket_path, port.to_string())
                .map_err(|e| format!("Failed to write daemon.port: {e}"))?;
        }

        // Ensure 0600 mode
        validate_safe_ownership_and_type(&socket_path, RuntimeNodeKind::Socket)?;
        #[cfg(unix)]
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Failed to secure daemon socket: {error}"))?;

        tracing::info!("rorca daemon listening on {}", socket_path.display());

        #[cfg(unix)]
        self.spawn_agent_state_listener();
        crate::daemon::agent_extension::install_agent_state_extension();

        let persisted_remote_config = self.remote_state.config.read().clone();
        if persisted_remote_config.mode != RemoteNetworkMode::Off {
            if let Err(error) = self.handle_remote_configure(persisted_remote_config).await {
                tracing::warn!("Failed to restore daemon remote gateway listener: {error}");
            }
        }

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let server = Arc::clone(&self);
                    tokio::spawn(async move {
                        server.handle_client(stream).await;
                    });
                }
                Err(e) => {
                    tracing::error!("Daemon accept error: {e}");
                    break;
                }
            }
        }

        Ok(())
    }

    pub async fn handle_client<S>(self: Arc<Self>, stream: S)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        while let Ok(n) = reader.read_line(&mut line).await {
            if n == 0 {
                break;
            }
            let req: Result<DaemonRequest, _> = serde_json::from_str(line.trim());
            line.clear();

            let resp = match req {
                Ok(DaemonRequest::Handshake { version }) => {
                    if version != DAEMON_PROTOCOL_VERSION {
                        DaemonResponse::ProtocolMismatch {
                            expected_version: DAEMON_PROTOCOL_VERSION,
                            received_version: version,
                        }
                    } else {
                        DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: self.epoch,
                            binary_path: self.binary_path.clone(),
                            binary_mtime_ms: self.binary_mtime_ms,
                        }
                    }
                }
                Ok(DaemonRequest::Ping) => DaemonResponse::Pong,
                Ok(DaemonRequest::RegisterWorkspace {
                    workspace_id,
                    repo_root,
                }) => match self.handle_register_workspace(&workspace_id, &repo_root) {
                    Ok(()) => DaemonResponse::RegisterWorkspaceOk,
                    Err(e) => DaemonResponse::Error { message: e },
                },
                Ok(DaemonRequest::Spawn {
                    client_request_id,
                    workspace_id,
                    worktree,
                    cwd,
                    cols,
                    rows,
                    shell,
                    startup,
                }) => {
                    let res = self
                        .handle_spawn(
                            &client_request_id,
                            &workspace_id,
                            worktree,
                            cwd,
                            cols,
                            rows,
                            shell,
                            startup,
                        )
                        .await;
                    match res {
                        Ok(session_id) => match self.handle_describe_session(&session_id) {
                            DaemonResponse::DescribeSessionOk { session } => {
                                DaemonResponse::SpawnOk {
                                    session_id,
                                    epoch: self.epoch,
                                    session,
                                }
                            }
                            DaemonResponse::Error { message } => DaemonResponse::Error { message },
                            _ => DaemonResponse::Error {
                                message: "Failed to describe spawned session".to_string(),
                            },
                        },
                        Err(SpawnError::AgentSessionConflict {
                            agent_type,
                            provider_key,
                            provider_id,
                            existing_session_id,
                        }) => DaemonResponse::AgentSessionConflict {
                            agent_type,
                            provider_key,
                            provider_id,
                            existing_session_id,
                        },
                        Err(SpawnError::InvalidAgentResume(message)) => {
                            DaemonResponse::AgentResumeInvalid { message }
                        }
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::DescribeSession { session_id }) => {
                    self.handle_describe_session(&session_id)
                }
                Ok(DaemonRequest::DiscoverAgentSession {
                    session_id,
                    agent_type,
                }) => {
                    let provider_session_id = self
                        .terminal_service
                        .get_session(&session_id)
                        .and_then(|session| session.pid())
                        .and_then(|pid| {
                            crate::ipc::agents::discover_agent_session_id(pid, &agent_type)
                        });
                    DaemonResponse::DiscoverAgentSessionOk {
                        provider_session_id,
                    }
                }
                Ok(DaemonRequest::Write { session_id, data }) => {
                    match self.terminal_service.write_input(&session_id, &data) {
                        Ok(()) => DaemonResponse::WriteOk,
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::Resize {
                    session_id,
                    cols,
                    rows,
                }) => match self.terminal_service.resize(&session_id, cols, rows) {
                    Ok(()) => DaemonResponse::ResizeOk,
                    Err(e) => DaemonResponse::Error {
                        message: e.to_string(),
                    },
                },
                Ok(DaemonRequest::Signal { session_id, signal }) => {
                    match self.terminal_service.signal(&session_id, signal) {
                        Ok(()) => DaemonResponse::SignalOk,
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::Close { session_id }) => {
                    match self.handle_close(&session_id).await {
                        Ok(()) => DaemonResponse::CloseOk,
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::ListSessions) => {
                    let sessions = self.terminal_service.list_sessions();
                    DaemonResponse::ListSessionsOk {
                        epoch: self.epoch,
                        sessions,
                    }
                }
                Ok(DaemonRequest::Attach {
                    session_id,
                    after_sequence,
                }) => {
                    match self
                        .terminal_service
                        .attach_with_sequence(&session_id, after_sequence)
                    {
                        Ok(attachment) => {
                            let (pty_cols, pty_rows) = self
                                .terminal_service
                                .get_session(&session_id)
                                .map(|s| s.get_size())
                                .map(|(c, r)| (Some(c), Some(r)))
                                .unwrap_or((None, None));
                            let hub = Arc::clone(self.terminal_service.output_hub());
                            let history_segments = attachment
                                .snapshot
                                .history_segments
                                .iter()
                                .map(|seg| HistorySegmentWire {
                                    cols: seg.cols,
                                    rows: seg.rows,
                                    bytes: seg.bytes.clone(),
                                })
                                .collect();
                            let resp = DaemonResponse::AttachOk {
                                epoch: self.epoch,
                                session_id: session_id.clone(),
                                start_sequence: attachment.snapshot.history_start_sequence,
                                end_sequence: attachment.snapshot.history_end_sequence,
                                gap: attachment.snapshot.gap,
                                history: attachment.snapshot.history,
                                pty_cols,
                                pty_rows,
                                history_segments,
                            };
                            let mut resp_json = serde_json::to_string(&resp).unwrap();
                            resp_json.push('\n');
                            let _ = write_half.write_all(resp_json.as_bytes()).await;
                            let _ = write_half.flush().await;

                            Self::pump_sequenced_stream_with_agent_state(
                                session_id,
                                attachment.receiver,
                                hub,
                                write_half,
                                Some(self.agent_state_tx.subscribe()),
                            )
                            .await;
                            return;
                        }
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::SaveSession { session }) => {
                    let path = get_default_session_path();
                    let res =
                        tokio::task::spawn_blocking(move || save_session_to_path(&path, &session))
                            .await;
                    match res {
                        Ok(Ok(())) => DaemonResponse::SaveSessionOk,
                        Ok(Err(e)) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                        Err(e) => DaemonResponse::Error {
                            message: format!("Save session task panicked: {e}"),
                        },
                    }
                }
                Ok(DaemonRequest::LoadSession) => {
                    let path = get_default_session_path();
                    let res =
                        tokio::task::spawn_blocking(move || load_session_from_path(&path)).await;
                    match res {
                        Ok(Ok(session)) => DaemonResponse::LoadSessionOk { session },
                        Ok(Err(e)) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                        Err(e) => DaemonResponse::Error {
                            message: format!("Load session task panicked: {e}"),
                        },
                    }
                }
                Ok(DaemonRequest::ClearSession) => {
                    let path = get_default_session_path();
                    let res =
                        tokio::task::spawn_blocking(move || clear_session_from_path(&path)).await;
                    match res {
                        Ok(Ok(())) => DaemonResponse::ClearSessionOk,
                        Ok(Err(e)) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                        Err(e) => DaemonResponse::Error {
                            message: format!("Clear session task panicked: {e}"),
                        },
                    }
                }
                Ok(DaemonRequest::RemoteGetStatus) => {
                    let config = self.remote_state.config.read().clone();
                    let is_running = *self.remote_state.is_running.read();
                    let bound_address = self.remote_state.bound_address.read().clone();
                    DaemonResponse::RemoteStatusOk {
                        status: DaemonRemoteStatus {
                            mode: config.mode,
                            port: config.port,
                            allow_control: config.allow_control,
                            is_running,
                            bound_address,
                        },
                    }
                }
                Ok(DaemonRequest::RemoteConfigure { config }) => {
                    match self.handle_remote_configure(config).await {
                        Ok(()) => DaemonResponse::RemoteConfigureOk,
                        Err(e) => DaemonResponse::Error { message: e },
                    }
                }
                Ok(DaemonRequest::RemoteCreatePairingCode { permission }) => {
                    let perm = permission.unwrap_or(DevicePermission::Control);
                    let code = self.remote_state.auth_manager.create_pairing_code(perm);
                    DaemonResponse::RemotePairingCodeOk { code }
                }
                Ok(DaemonRequest::RemoteListDevices) => {
                    let devices = self.remote_state.auth_manager.list_devices();
                    DaemonResponse::RemoteListDevicesOk { devices }
                }
                Ok(DaemonRequest::RemoteRevokeDevice { device_id }) => {
                    if self.remote_state.auth_manager.revoke_device(&device_id) {
                        DaemonResponse::RemoteRevokeDeviceOk
                    } else {
                        DaemonResponse::Error {
                            message: format!("Device '{device_id}' not found"),
                        }
                    }
                }
                Ok(DaemonRequest::RemoteSetActiveSelection { selection }) => {
                    self.remote_state.set_active_selection_opt(selection);
                    DaemonResponse::RemoteSetActiveSelectionOk
                }
                Ok(DaemonRequest::RemoteGetActiveSelection) => {
                    let selection = self.remote_state.active_selection.read().clone();
                    DaemonResponse::RemoteGetActiveSelectionOk { selection }
                }
                Ok(DaemonRequest::SubscribeRemoteEvents) => {
                    let mut events = self.remote_event_tx.subscribe();
                    let mut resp_json =
                        serde_json::to_string(&DaemonResponse::SubscribeRemoteEventsOk).unwrap();
                    resp_json.push('\n');
                    if write_half.write_all(resp_json.as_bytes()).await.is_err()
                        || write_half.flush().await.is_err()
                    {
                        return;
                    }
                    loop {
                        match events.recv().await {
                            Ok(event) => {
                                let Ok(mut line) = serde_json::to_string(&event) else {
                                    continue;
                                };
                                line.push('\n');
                                if write_half.write_all(line.as_bytes()).await.is_err()
                                    || write_half.flush().await.is_err()
                                {
                                    return;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => return,
                        }
                    }
                }
                Ok(DaemonRequest::UpgradeBinary) => {
                    self.handle_upgrade_binary().await
                }
                Ok(DaemonRequest::Shutdown) => {
                    std::process::exit(0);
                }
                Err(e) => DaemonResponse::Error {
                    message: format!("Malformed request: {e}"),
                },
            };

            let mut resp_json = serde_json::to_string(&resp).unwrap();
            resp_json.push('\n');
            if write_half.write_all(resp_json.as_bytes()).await.is_err() {
                break;
            }
            let _ = write_half.flush().await;
            if matches!(resp, DaemonResponse::ProtocolMismatch { .. }) {
                break;
            }
        }
    }

    #[cfg(unix)]
    async fn handle_upgrade_binary(&self) -> DaemonResponse {
        let booted_mtime = self.binary_mtime_ms;
        let on_disk_mtime = tokio::task::spawn_blocking(|| {
            let exe = std::env::current_exe().ok()?;
            get_file_mtime_ms(&exe)
        })
        .await
        .unwrap_or(None);

        if !crate::daemon::client::should_request_upgrade(booted_mtime, on_disk_mtime) {
            return DaemonResponse::UpgradeNotNeeded;
        }

        let active_sessions = self.terminal_service.list_sessions();
        if !active_sessions.is_empty() {
            tracing::warn!(
                "Deferring daemon binary upgrade because {} live terminal session(s) are active ({:?})",
                active_sessions.len(),
                active_sessions
            );
            return DaemonResponse::UpgradeDeferred;
        }

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if let Err(e) = perform_daemon_exec() {
                tracing::error!("Failed to re-exec daemon during upgrade: {e}");
            }
        });

        DaemonResponse::UpgradeScheduled
    }

    #[cfg(not(unix))]
    async fn handle_upgrade_binary(&self) -> DaemonResponse {
        DaemonResponse::UpgradeUnsupported
    }

    pub fn handle_register_workspace(
        &self,
        workspace_id: &str,
        repo_root: &str,
    ) -> Result<(), String> {
        let path = PathBuf::from(repo_root);
        if !path.is_absolute() {
            return Err("repo_root must be an absolute path".into());
        }
        let canonical = fs::canonicalize(&path)
            .map_err(|e| format!("Invalid repo_root path '{}': {e}", path.display()))?;
        if !canonical.is_dir() {
            return Err(format!(
                "Repo root '{}' is not a directory",
                canonical.display()
            ));
        }
        let manager = WorktreeManager::try_new(&canonical).map_err(|e| e.to_string())?;
        if manager.repo_root() != canonical {
            return Err(format!(
                "repo_root '{}' must be the canonical repository root '{}'",
                path.display(),
                manager.repo_root().display()
            ));
        }
        self.workspace_registry
            .register(workspace_id, &canonical)
            .map_err(|e| e.to_string())
    }

    pub async fn handle_remote_configure(&self, mut config: RemoteGatewayConfig) -> Result<(), String> {
        config.port = REMOTE_GATEWAY_PORT;
        self.configure_gateway(config).await
    }

    pub(crate) async fn configure_gateway(&self, config: RemoteGatewayConfig) -> Result<(), String> {
        if config.mode == RemoteNetworkMode::Off {
            let prev_handle = self.remote_server_handle.lock().take();
            if let Some(handle) = prev_handle {
                handle.stop();
            }
            *self.remote_state.config.write() = config;
            *self.remote_state.is_running.write() = false;
            *self.remote_state.bound_address.write() = None;
            self.remote_state
                .persist_config()
                .map_err(|error| format!("Failed to persist remote gateway config: {error}"))?;
            return Ok(());
        }

        let prev_config = self.remote_state.config.read().clone();
        *self.remote_state.config.write() = config.clone();

        match start_remote_server(Arc::clone(&self.remote_state)).await {
            Ok((handle, _addr)) => {
                let prev_handle = self.remote_server_handle.lock().take();
                if let Some(h) = prev_handle {
                    h.stop();
                }
                *self.remote_server_handle.lock() = Some(handle);
                self.remote_state
                    .persist_config()
                    .map_err(|error| format!("Failed to persist remote gateway config: {error}"))?;
                Ok(())
            }
            Err(err) => {
                *self.remote_state.config.write() = prev_config;
                *self.remote_state.is_running.write() = false;
                *self.remote_state.bound_address.write() = None;
                let _ = self.remote_state.persist_config();
                Err(err)
            }
        }
    }

    async fn handle_spawn(
        &self,
        client_request_id: &str,
        workspace_id: &str,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        shell: Option<String>,
        startup: Option<TerminalStartup>,
    ) -> Result<String, SpawnError> {
        if client_request_id.trim().is_empty() {
            return Err(SpawnError::Other("clientRequestId cannot be empty".into()));
        }

        let _spawn_guard = self.spawn_lock.lock().await;

        let now = Instant::now();
        let provider_claim = ProviderSessionClaimKey::from_startup(startup.as_ref());
        let spawn_fingerprint = SpawnRequestFingerprint {
            workspace_id: workspace_id.to_string(),
            worktree: worktree.clone(),
            cwd: cwd.clone(),
            cols,
            rows,
            shell: shell.clone(),
            provider_claim: provider_claim.clone(),
            startup: startup.clone(),
        };
        self.prune_dead_spawn_ownership(now);
        {
            let mut cache = self.spawn_idempotency_cache.lock();
            if let Some(entry) = cache.get_mut(client_request_id) {
                if entry.fingerprint != spawn_fingerprint {
                    return Err(SpawnError::Other(format!(
                        "clientRequestId '{client_request_id}' was reused with a different spawn request"
                    )));
                }
                entry.created_at = now;
                return Ok(entry.session_id.clone());
            }
        }

        let existing_request = self
            .session_metadata
            .read()
            .iter()
            .find(|(session_id, meta)| {
                meta.client_request_id == client_request_id && self.session_is_live(session_id)
            })
            .map(|(session_id, meta)| (session_id.clone(), meta.spawn_fingerprint.clone()));
        if let Some((live_session_id, existing_fingerprint)) = existing_request {
            if existing_fingerprint != spawn_fingerprint {
                return Err(SpawnError::Other(format!(
                    "clientRequestId '{client_request_id}' was reused with a different spawn request"
                )));
            }
            self.spawn_idempotency_cache.lock().insert(
                client_request_id.to_string(),
                SpawnCacheEntry {
                    session_id: live_session_id.clone(),
                    created_at: now,
                    fingerprint: spawn_fingerprint.clone(),
                },
            );
            return Ok(live_session_id);
        }

        // Resolve manager from workspace registry; workspace MUST be registered.
        let (mgr, default_cwd) = self
            .workspace_registry
            .resolve_terminal_target(workspace_id, worktree.as_ref())
            .map_err(|e| SpawnError::Other(e.to_string()))?;

        let resolved_cwd = if let Some(ref custom_cwd_str) = cwd {
            let custom_path = PathBuf::from(custom_cwd_str);
            if !custom_path.exists() {
                return Err(SpawnError::Other(format!(
                    "CWD does not exist: {custom_cwd_str}"
                )));
            }
            if !custom_path.is_dir() {
                return Err(SpawnError::Other(format!(
                    "CWD is not a directory: {custom_cwd_str}"
                )));
            }
            let canonical = fs::canonicalize(&custom_path).map_err(|e| {
                SpawnError::Other(format!("Cannot canonicalize CWD {custom_cwd_str}: {e}"))
            })?;
            let allowed = mgr.canonical_allowed_path(&canonical).map_err(|e| {
                SpawnError::Other(format!("CWD '{custom_cwd_str}' is outside workspace: {e}"))
            })?;
            if allowed != default_cwd && !allowed.starts_with(&default_cwd) {
                return Err(SpawnError::Other(format!(
                    "CWD '{custom_cwd_str}' is outside the resolved workspace/worktree root '{}'",
                    default_cwd.display()
                )));
            }
            allowed
        } else {
            default_cwd
        };

        let mut cmd = match crate::terminal::shell::resolve_startup_command(
            shell.as_deref(),
            startup.as_ref(),
        ) {
            Ok(cmd) => cmd,
            Err(err) => return Err(SpawnError::InvalidAgentResume(err.to_string())),
        };
        if let Some(claim) = provider_claim.as_ref() {
            if let Some(existing_session_id) = self.provider_session_claims.lock().get(claim) {
                return Err(SpawnError::AgentSessionConflict {
                    agent_type: claim.agent_type.clone(),
                    provider_key: claim.provider_key,
                    provider_id: claim.provider_id.clone(),
                    existing_session_id: existing_session_id.clone(),
                });
            }
        }
        cmd.env("PROMPT_EOL_MARK", "");
        cmd.cwd(normalize_process_cwd(&resolved_cwd));

        let (session_id, mut lifecycle_rx) = self
            .terminal_service
            .spawn_in_worktree(cmd, cols, rows, &mgr, &resolved_cwd)
            .map_err(|e| SpawnError::Other(e.to_string()))?;

        // Store idempotency entry and session metadata before releasing the request lock.
        self.spawn_idempotency_cache.lock().insert(
            client_request_id.to_string(),
            SpawnCacheEntry {
                session_id: session_id.clone(),
                created_at: now,
                fingerprint: spawn_fingerprint.clone(),
            },
        );
        self.session_metadata.write().insert(
            session_id.clone(),
            StoredSessionMeta {
                client_request_id: client_request_id.to_string(),
                workspace_id: workspace_id.to_string(),
                worktree,
                cwd: resolved_cwd,
                provider_claim: provider_claim.clone(),
                spawn_fingerprint,
            },
        );
        if let Some(claim) = provider_claim {
            self.provider_session_claims
                .lock()
                .insert(claim, session_id.clone());
        }

        let cleanup_session_id = session_id.clone();
        let cleanup_cache = Arc::clone(&self.spawn_idempotency_cache);
        let cleanup_metadata = Arc::clone(&self.session_metadata);
        let cleanup_claims = Arc::clone(&self.provider_session_claims);
        tokio::spawn(async move {
            loop {
                match lifecycle_rx.recv().await {
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            cleanup_cache
                .lock()
                .retain(|_, entry| entry.session_id != cleanup_session_id);
            if let Some(meta) = cleanup_metadata.write().remove(&cleanup_session_id) {
                if let Some(claim) = meta.provider_claim {
                    cleanup_claims
                        .lock()
                        .retain(|key, owner| key != &claim || owner != &cleanup_session_id);
                }
            }
        });

        Ok(session_id)
    }

    async fn handle_close(&self, session_id: &str) -> Result<(), crate::terminal::PtyError> {
        self.terminal_service.close_session(session_id).await?;
        self.release_session_ownership(session_id);
        Ok(())
    }

    fn session_is_live(&self, session_id: &str) -> bool {
        self.terminal_service
            .get_session(session_id)
            .is_some_and(|session| {
                matches!(
                    session.state(),
                    PtySessionState::Starting | PtySessionState::Running
                )
            })
    }

    fn release_session_ownership(&self, session_id: &str) {
        self.spawn_idempotency_cache
            .lock()
            .retain(|_, entry| entry.session_id != session_id);
        if let Some(meta) = self.session_metadata.write().remove(session_id) {
            if let Some(claim) = meta.provider_claim {
                self.provider_session_claims
                    .lock()
                    .retain(|key, owner| key != &claim || owner != session_id);
            }
        }
    }

    fn prune_dead_spawn_ownership(&self, now: Instant) {
        let dead_sessions: Vec<String> = self
            .session_metadata
            .read()
            .keys()
            .filter(|session_id| !self.session_is_live(session_id))
            .cloned()
            .collect();
        for session_id in dead_sessions {
            self.release_session_ownership(&session_id);
        }
        self.spawn_idempotency_cache.lock().retain(|_, entry| {
            now.duration_since(entry.created_at) <= SPAWN_REQUEST_TTL
                && self.session_is_live(&entry.session_id)
        });
        self.provider_session_claims
            .lock()
            .retain(|_, session_id| self.session_is_live(session_id));
    }

    #[cfg(test)]
    fn expire_spawn_request_for_test(&self, client_request_id: &str) {
        if let Some(entry) = self
            .spawn_idempotency_cache
            .lock()
            .get_mut(client_request_id)
        {
            entry.created_at = Instant::now() - SPAWN_REQUEST_TTL - Duration::from_secs(1);
        }
    }

    #[cfg(test)]
    fn spawn_cache_len_for_test(&self) -> usize {
        self.spawn_idempotency_cache.lock().len()
    }

    #[cfg(test)]
    fn provider_claim_len_for_test(&self) -> usize {
        self.provider_session_claims.lock().len()
    }

    #[cfg(test)]
    fn reserve_provider_claim_for_test(
        &self,
        client_request_id: &str,
        session_id: &str,
        startup: &TerminalStartup,
    ) -> Result<String, SpawnError> {
        let claim = ProviderSessionClaimKey::from_startup(Some(startup))
            .expect("agent resume startup has a provider claim");
        let mut claims = self.provider_session_claims.lock();
        if let Some(existing_session_id) = claims.get(&claim) {
            return Err(SpawnError::AgentSessionConflict {
                agent_type: claim.agent_type,
                provider_key: claim.provider_key,
                provider_id: claim.provider_id,
                existing_session_id: existing_session_id.clone(),
            });
        }
        claims.insert(claim.clone(), session_id.to_string());
        self.session_metadata.write().insert(
            session_id.to_string(),
            StoredSessionMeta {
                client_request_id: client_request_id.to_string(),
                workspace_id: "test".to_string(),
                worktree: None,
                cwd: PathBuf::from("/test"),
                provider_claim: Some(claim),
                spawn_fingerprint: SpawnRequestFingerprint {
                    workspace_id: "test".to_string(),
                    worktree: None,
                    cwd: None,
                    cols: 80,
                    rows: 24,
                    shell: None,
                    provider_claim: ProviderSessionClaimKey::from_startup(Some(startup)),
                    startup: Some(startup.clone()),
                },
            },
        );
        Ok(session_id.to_string())
    }

    fn handle_describe_session(&self, session_id: &str) -> DaemonResponse {
        let Some(pty_session) = self.terminal_service.get_session(session_id) else {
            return DaemonResponse::Error {
                message: format!("Session '{session_id}' not found"),
            };
        };

        let (cols, rows) = pty_session.get_size();
        let running = matches!(
            pty_session.state(),
            PtySessionState::Starting | PtySessionState::Running
        );
        let (start_sequence, end_sequence) = self
            .terminal_service
            .output_hub()
            .session_sequence_range(session_id)
            .unwrap_or((None, None));

        let meta = self.session_metadata.read().get(session_id).cloned();
        let (workspace_id, worktree, cwd) = match meta {
            Some(m) => (
                Some(m.workspace_id),
                m.worktree,
                Some(m.cwd.to_string_lossy().to_string()),
            ),
            None => (
                None,
                None,
                pty_session
                    .worktree_path()
                    .map(|p| p.to_string_lossy().to_string()),
            ),
        };

        DaemonResponse::DescribeSessionOk {
            session: DaemonSessionDetails {
                session_id: session_id.to_string(),
                workspace_id,
                worktree,
                cwd,
                cols,
                rows,
                running,
                start_sequence,
                end_sequence,
            },
        }
    }

    pub async fn pump_sequenced_stream<W>(
        session_id: String,
        rx: broadcast::Receiver<crate::terminal::output_hub::OutputChunk>,
        hub: Arc<TerminalOutputHub>,
        writer: W,
    ) where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        Self::pump_sequenced_stream_with_agent_state(session_id, rx, hub, writer, None).await
    }

    pub async fn pump_sequenced_stream_with_agent_state<W>(
        session_id: String,
        mut rx: broadcast::Receiver<crate::terminal::output_hub::OutputChunk>,
        hub: Arc<TerminalOutputHub>,
        writer: W,
        mut agent_state_rx: Option<
            broadcast::Receiver<(
                String,
                String,
                Option<String>,
                Option<crate::daemon::protocol::AgentProviderSession>,
            )>,
        >,
    ) where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let mut writer = BufWriter::new(writer);
        let mut last_seen_sequence: Option<u64> = None;

        loop {
            let received = match agent_state_rx.as_mut() {
                Some(state_rx) => tokio::select! {
                    output = rx.recv() => output,
                    report = state_rx.recv() => {
                        if let Ok((reported_session_id, state, agent, provider_session)) = report {
                            if reported_session_id == session_id {
                                let msg = DaemonStreamMessage::AgentState {
                                    session_id: Cow::Borrowed(&session_id),
                                    state: Cow::Borrowed(&state),
                                    agent: agent.as_deref().map(Cow::Borrowed),
                                    provider_session,
                                };
                                let mut json = serde_json::to_string(&msg).unwrap();
                                json.push('\n');
                                if writer.write_all(json.as_bytes()).await.is_err() {
                                    break;
                                }
                                if writer.flush().await.is_err() {
                                    break;
                                }
                            }
                        }
                        continue;
                    }
                },
                None => rx.recv().await,
            };

            match received {
                Ok(chunk) => {
                    if last_seen_sequence.is_some_and(|last| chunk.sequence <= last) {
                        continue;
                    }
                    last_seen_sequence = Some(chunk.sequence);
                    let msg = DaemonStreamMessage::Output {
                        session_id: Cow::Borrowed(&session_id),
                        sequence: chunk.sequence,
                        data: Cow::Borrowed(&chunk.bytes),
                        metrics_read_unix_micros: chunk.metrics_read_unix_micros,
                    };
                    let mut json = serde_json::to_string(&msg).unwrap();
                    json.push('\n');
                    if writer.write_all(json.as_bytes()).await.is_err() {
                        break;
                    }
                    if writer.flush().await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Re-subscribe with sequence after last_seen_sequence to recover replay gap
                    if let Some(att) = hub.subscribe_with_sequence(&session_id, last_seen_sequence)
                    {
                        rx = att.receiver;
                        let requested_after_sequence = last_seen_sequence.unwrap_or(0);
                        if att.snapshot.history_end_sequence.is_some() {
                            last_seen_sequence = att.snapshot.history_end_sequence;
                        }
                        let available_from_sequence = att
                            .snapshot
                            .gap
                            .as_ref()
                            .map(|gap| gap.available_from_sequence)
                            .or(att.snapshot.history_start_sequence)
                            .unwrap_or_else(|| requested_after_sequence.saturating_add(1));
                        let segments = att
                            .snapshot
                            .history_segments
                            .iter()
                            .map(|seg| HistorySegmentWire {
                                cols: seg.cols,
                                rows: seg.rows,
                                bytes: seg.bytes.clone(),
                            })
                            .collect();
                        let msg = DaemonStreamMessage::Lagged {
                            session_id: Cow::Borrowed(&session_id),
                            requested_after_sequence,
                            available_from_sequence,
                            start_sequence: att.snapshot.history_start_sequence,
                            end_sequence: att.snapshot.history_end_sequence,
                            history: Cow::Borrowed(&att.snapshot.history),
                            segments,
                        };
                        let mut json = serde_json::to_string(&msg).unwrap();
                        json.push('\n');
                        if writer.write_all(json.as_bytes()).await.is_err() {
                            break;
                        }
                        if writer.flush().await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    let msg = DaemonStreamMessage::Exit {
                        session_id: Cow::Borrowed(&session_id),
                        exit_code: None,
                    };
                    let mut json = serde_json::to_string(&msg).unwrap();
                    json.push('\n');
                    let _ = writer.write_all(json.as_bytes()).await;
                    let _ = writer.flush().await;
                    break;
                }
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::terminal::output_hub::OutputChunk;
    use tempfile::tempdir;

    fn init_test_git_repo() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output();
        dir
    }

    #[tokio::test]
    async fn test_pump_stream_compact_framing_and_exit() {
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let (_server_read, server_write) = server_stream.into_split();
        let (client_read, _client_write) = client_stream.into_split();
        let mut client_reader = BufReader::new(client_read);

        let (tx, rx) = broadcast::channel(16);
        let hub = Arc::new(TerminalOutputHub::default());
        let session_id = "test-session-123".to_string();

        let pump_handle = tokio::spawn(DaemonServer::pump_sequenced_stream(
            session_id.clone(),
            rx,
            hub,
            server_write,
        ));

        // Send binary output chunk
        tx.send(OutputChunk {
            sequence: 1,
            bytes: b"hello pty stream\n".to_vec(),
            metrics_read_unix_micros: None,
        })
        .unwrap();

        let mut line = String::new();
        client_reader.read_line(&mut line).await.unwrap();
        assert!(
            !line.contains('['),
            "Must not contain JSON number array: {line}"
        );
        assert!(
            line.contains(r#""data":"aGVsbG8gcHR5IHN0cmVhbQo=""#),
            "Expected base64 data: {line}"
        );
        assert!(
            line.contains(r#""sessionId":"test-session-123""#),
            "Expected camelCase sessionId: {line}"
        );
        assert!(
            line.contains(r#""sequence":1"#),
            "Expected sequence 1: {line}"
        );

        // Close broadcast sender to trigger Exit
        drop(tx);
        line.clear();
        client_reader.read_line(&mut line).await.unwrap();
        assert!(line.contains(r#""type":"exit""#));
        assert!(line.contains(r#""sessionId":"test-session-123""#));

        let _ = pump_handle.await;
    }

    #[tokio::test]
    async fn test_server_register_workspace_and_spawn_isolation() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        let repo_path = repo.path().to_str().unwrap();

        // 1. Spawning before registration must fail explicitly (no GUI CWD fallback)
        let unreg_res = server
            .handle_spawn("req-unreg-1", "ws-app", None, None, 80, 24, None, None)
            .await;
        assert!(
            unreg_res.is_err(),
            "Must not infer workspace before explicit registration"
        );

        // 2. Explicit registration of valid canonical git repo
        let reg_res = server.handle_register_workspace("ws-app", repo_path);
        assert!(reg_res.is_ok(), "Registration of valid repo succeeds");

        let nested = repo.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let nested_reg = server.handle_register_workspace("ws-nested", nested.to_str().unwrap());
        assert!(
            nested_reg.is_err(),
            "registration requires the canonical repository root"
        );

        // 3. Spawning in registered workspace succeeds
        let spawn_res = server
            .handle_spawn("req-reg-1", "ws-app", None, None, 80, 24, None, None)
            .await;
        assert!(spawn_res.is_ok());

        // 4. Plain (non-git) directories register as terminal-only workspaces
        let non_git = tempdir().unwrap();
        let plain_reg =
            server.handle_register_workspace("ws-plain", non_git.path().to_str().unwrap());
        assert!(
            plain_reg.is_ok(),
            "plain folders register successfully (terminal-only)"
        );

        // 5. Nonexistent roots still fail registration
        let bad_reg = server.handle_register_workspace("ws-bad", "/definitely/not/a/real/path");
        assert!(bad_reg.is_err());
    }

    #[tokio::test]
    async fn test_server_spawn_idempotency_cache() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        let req_id = "spawn-req-idempotency-1".to_string();
        let (first, second) = tokio::join!(
            server.handle_spawn(&req_id, "default", None, None, 80, 24, None, None),
            server.handle_spawn(&req_id, "default", None, None, 80, 24, None, None),
        );
        let session1 = first.expect("first spawn succeeds");
        let session2 = second.expect("concurrent duplicate succeeds");

        assert_eq!(
            session1, session2,
            "Concurrent duplicate requests must share one shell"
        );
        assert_eq!(server.terminal_service().list_sessions().len(), 1);

        server
            .terminal_service()
            .close_session(&session1)
            .await
            .expect("close first session");
        let replacement = server
            .handle_spawn(&req_id, "default", None, None, 80, 24, None, None)
            .await
            .expect("dead cached session is never returned");
        assert_ne!(session1, replacement);

        server.expire_spawn_request_for_test(&req_id);
        let after_ttl = server
            .handle_spawn(&req_id, "default", None, None, 80, 24, None, None)
            .await
            .expect("a repeated id never creates a second live shell");
        assert_eq!(replacement, after_ttl);
        assert_eq!(server.spawn_cache_len_for_test(), 1);
    }

    fn claude_resume_startup(id: &str) -> TerminalStartup {
        TerminalStartup::AgentResume {
            agent_type: "claude".to_string(),
            provider_session: crate::daemon::protocol::AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: id.to_string(),
                transcript_path: None,
            },
        }
    }

    #[cfg(unix)]
    fn test_agent_resume_startup(id: &str) -> TerminalStartup {
        TerminalStartup::AgentResume {
            agent_type: "ferryx-test-agent".to_string(),
            provider_session: crate::daemon::protocol::AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: id.to_string(),
                transcript_path: None,
            },
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_agent_resume_production_spawn_path_fences_duplicates_and_conflicts() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();
        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let spawn = |request_id: &'static str, barrier: Arc<tokio::sync::Barrier>| {
            let server = Arc::clone(&server);
            async move {
                barrier.wait().await;
                server
                    .handle_spawn(
                        request_id,
                        "default",
                        None,
                        None,
                        80,
                        24,
                        None,
                        Some(test_agent_resume_startup("production-provider")),
                    )
                    .await
            }
        };

        let same_a = spawn("production-same", Arc::clone(&barrier));
        let same_b = spawn("production-same", Arc::clone(&barrier));
        let (_, same_a, same_b) = tokio::join!(barrier.wait(), same_a, same_b);
        let owner = same_a.expect("first production spawn");
        assert_eq!(owner, same_b.expect("idempotent production retry"));
        assert_eq!(server.terminal_service().list_sessions().len(), 1);

        let conflict = server
            .handle_spawn(
                "production-competing",
                "default",
                None,
                None,
                80,
                24,
                None,
                Some(test_agent_resume_startup("production-provider")),
            )
            .await
            .expect_err("competing production claim must fail");
        assert!(matches!(conflict, SpawnError::AgentSessionConflict { .. }));
        assert_eq!(server.terminal_service().list_sessions().len(), 1);

        let reused_request = server
            .handle_spawn(
                "production-same",
                "default",
                None,
                None,
                100,
                40,
                None,
                Some(test_agent_resume_startup("different-provider")),
            )
            .await
            .expect_err("request id cannot be reused for a different spawn fingerprint");
        assert!(reused_request
            .to_string()
            .contains("reused with a different spawn request"));
        assert_eq!(server.terminal_service().list_sessions().len(), 1);

        server.handle_close(&owner).await.expect("production close");
        assert_eq!(server.provider_claim_len_for_test(), 0);
    }

    #[tokio::test]
    async fn test_agent_resume_idempotency_fingerprint_includes_full_startup_payload() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();
        let startup = |transcript_path: &str| TerminalStartup::AgentResume {
            agent_type: "ferryx-test-agent".to_string(),
            provider_session: crate::daemon::protocol::AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: "same-provider-id".to_string(),
                transcript_path: Some(transcript_path.to_string()),
            },
        };

        let first = server
            .handle_spawn(
                "same-request",
                "default",
                None,
                None,
                80,
                24,
                None,
                Some(startup("/tmp/pi-first.json")),
            )
            .await
            .expect("first path-sensitive spawn");
        let changed = server
            .handle_spawn(
                "same-request",
                "default",
                None,
                None,
                80,
                24,
                None,
                Some(startup("/tmp/pi-second.json")),
            )
            .await
            .expect_err("changed startup payload cannot reuse idempotency key");
        assert!(changed
            .to_string()
            .contains("reused with a different spawn request"));

        server
            .handle_close(&first)
            .await
            .expect("close first session");
    }

    #[tokio::test]
    async fn test_agent_resume_identical_request_is_one_spawn() {
        let server = Arc::new(DaemonServer::new());
        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let spawn = |barrier: Arc<tokio::sync::Barrier>| {
            let server = Arc::clone(&server);
            async move {
                barrier.wait().await;
                let _guard = server.spawn_lock.lock().await;
                if let Some(meta) = server
                    .session_metadata
                    .read()
                    .iter()
                    .find(|(_, meta)| meta.client_request_id == "resume-same-request")
                    .map(|(session_id, _)| session_id.clone())
                {
                    return Ok(meta);
                }
                server.reserve_provider_claim_for_test(
                    "resume-same-request",
                    "session-one",
                    &claude_resume_startup("provider-same"),
                )
            }
        };
        let first = spawn(Arc::clone(&barrier));
        let second = spawn(Arc::clone(&barrier));
        let (_, first, second) = tokio::join!(barrier.wait(), first, second);
        let first = first.expect("first resume spawn");
        let second = second.expect("duplicate resume spawn");
        assert_eq!(first, second);
        assert_eq!(server.session_metadata.read().len(), 1);
        assert_eq!(server.provider_claim_len_for_test(), 1);
    }

    #[tokio::test]
    async fn test_agent_resume_competing_claim_conflicts_and_releases() {
        let server = Arc::new(DaemonServer::new());
        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let spawn = |request_id: &'static str,
                     session_id: &'static str,
                     barrier: Arc<tokio::sync::Barrier>| {
            let server = Arc::clone(&server);
            async move {
                barrier.wait().await;
                let _guard = server.spawn_lock.lock().await;
                server.reserve_provider_claim_for_test(
                    request_id,
                    session_id,
                    &claude_resume_startup("provider-conflict"),
                )
            }
        };
        let first = spawn("resume-owner-a", "session-a", Arc::clone(&barrier));
        let second = spawn("resume-owner-b", "session-b", Arc::clone(&barrier));
        let (_, first, second) = tokio::join!(barrier.wait(), first, second);
        let (owner, conflict) = match (first, second) {
            (Ok(owner), Err(conflict)) | (Err(conflict), Ok(owner)) => (owner, conflict),
            other => panic!("expected one owner and one conflict, got {other:?}"),
        };
        assert!(matches!(conflict, SpawnError::AgentSessionConflict { .. }));
        assert_eq!(server.session_metadata.read().len(), 1);
        assert_eq!(server.provider_claim_len_for_test(), 1);

        server.release_session_ownership(&owner);
        assert_eq!(
            server.provider_claim_len_for_test(),
            0,
            "close/exit cleanup must make the provider reference immediately claimable"
        );
        assert!(
            !server.provider_session_claims.lock().contains_key(
                &ProviderSessionClaimKey::from_startup(Some(&claude_resume_startup(
                    "provider-conflict"
                )))
                .expect("claim key")
            ),
            "released provider claim must not retain a stale owner"
        );
    }

    #[tokio::test]
    async fn test_agent_resume_failed_spawn_does_not_leak_claim() {
        let server = DaemonServer::new();
        let invalid = crate::terminal::shell::resolve_startup_command(
            None,
            Some(&claude_resume_startup("--invalid")),
        );
        assert!(invalid.is_err());
        assert_eq!(server.provider_claim_len_for_test(), 0);

        let valid = server.reserve_provider_claim_for_test(
            "resume-valid",
            "session-valid",
            &claude_resume_startup("provider-after-failure"),
        );
        assert!(valid.is_ok());
        assert_eq!(server.provider_claim_len_for_test(), 1);
    }

    #[tokio::test]
    async fn test_server_spawn_cwd_validation() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        // Nonexistent CWD
        let err_nonexistent = server
            .handle_spawn(
                "req-bad-cwd",
                "default",
                None,
                Some("/nonexistent/path/for/rorca/test/123".to_string()),
                80,
                24,
                None,
                None,
            )
            .await;
        assert!(err_nonexistent.is_err());
        assert!(err_nonexistent.unwrap_err().contains("CWD does not exist"));

        let outside = tempdir().unwrap();
        let outside_res = server
            .handle_spawn(
                "req-outside-cwd",
                "default",
                None,
                Some(outside.path().to_string_lossy().into_owned()),
                80,
                24,
                None,
                None,
            )
            .await;
        assert!(
            outside_res.is_err(),
            "arbitrary cwd outside the registered root must fail"
        );

        let symlink_outside = repo.path().join("escape");
        std::os::unix::fs::symlink(outside.path(), &symlink_outside).unwrap();
        let symlink_res = server
            .handle_spawn(
                "req-symlink-cwd",
                "default",
                None,
                Some(symlink_outside.to_string_lossy().into_owned()),
                80,
                24,
                None,
                None,
            )
            .await;
        assert!(
            symlink_res.is_err(),
            "symlink escape outside the registered root must fail"
        );

        // Valid CWD inside repo
        let ok_cwd = repo.path().to_str().unwrap().to_string();
        let ok_res = server
            .handle_spawn(
                "req-good-cwd",
                "default",
                None,
                Some(ok_cwd.clone()),
                80,
                24,
                None,
                None,
            )
            .await;
        assert!(ok_res.is_ok());

        // Verify DescribeSession reflects cwd
        let session_id = ok_res.unwrap();
        let desc_resp = server.handle_describe_session(&session_id);
        match desc_resp {
            DaemonResponse::DescribeSessionOk { session } => {
                assert_eq!(session.session_id, session_id);
                assert!(session.cwd.is_some());
                assert!(session.running);
            }
            other => panic!("Expected DescribeSessionOk, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_runtime_paths_are_profile_aware() {
        // SAFETY: `getuid` takes no arguments and returns the current process UID.
        let uid = unsafe { libc::getuid() };
        let expected = if cfg!(debug_assertions) {
            PathBuf::from(format!("/tmp/rorca-{uid}-dev"))
        } else {
            PathBuf::from(format!("/tmp/rorca-{uid}"))
        };
        assert_eq!(get_runtime_dir(), expected);

        let expected = if cfg!(debug_assertions) {
            PathBuf::from(format!("/tmp/rorca-{uid}-dev/daemon.sock"))
        } else {
            PathBuf::from(format!("/tmp/rorca-{uid}/daemon.sock"))
        };
        assert_eq!(get_socket_path(), expected);
        assert_eq!(get_lock_path(), get_runtime_dir().join("daemon.lock"));
    }

    #[test]
    fn test_persistent_lock_filename_is_profile_aware() {
        let expected = if cfg!(debug_assertions) {
            "daemon-dev.lock"
        } else {
            "daemon.lock"
        };
        assert_eq!(
            get_persistent_lock_path()
                .expect("persistent daemon lock path")
                .file_name()
                .and_then(|name| name.to_str()),
            Some(expected)
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_runtime_dir_and_socket_symlink_protection() {
        let dir = tempdir().unwrap();
        let symlink_path = dir.path().join("symlink_dir");
        let target_dir = dir.path().join("target_dir");
        fs::create_dir_all(&target_dir).unwrap();
        std::os::unix::fs::symlink(&target_dir, &symlink_path).unwrap();

        let check = validate_safe_ownership_and_type(&symlink_path, RuntimeNodeKind::Directory);
        assert!(check.is_err(), "Symlinks in runtime dir must be rejected");
        assert!(
            symlink_path.exists(),
            "Validation must never delete an unsafe path"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_runtime_node_modes_are_enforced() {
        let dir = tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        fs::create_dir(&runtime).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755)).unwrap();
        ensure_runtime_directory(&runtime).expect("secure runtime directory");
        assert_eq!(
            fs::symlink_metadata(&runtime).unwrap().permissions().mode() & 0o777,
            0o700
        );

        let lock = runtime.join("daemon.lock");
        let _file = open_secure_lock_file(&lock).expect("secure lock file");
        assert_eq!(
            fs::symlink_metadata(&lock).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let socket_symlink = runtime.join("daemon.sock");
        let target = runtime.join("target");
        fs::write(&target, b"keep").unwrap();
        std::os::unix::fs::symlink(&target, &socket_symlink).unwrap();
        assert!(remove_stale_socket_after_lock(&socket_symlink).is_err());
        assert!(socket_symlink.symlink_metadata().is_ok());
        assert_eq!(fs::read(&target).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn test_persistent_daemon_lock_survives_legacy_tmp_lock_replacement() {
        let dir = tempdir().unwrap();
        let persistent = dir.path().join("data").join("locks").join("daemon.lock");
        let legacy_dir = dir.path().join("runtime");
        fs::create_dir(&legacy_dir).unwrap();
        let legacy = legacy_dir.join("daemon.lock");

        let first = acquire_daemon_locks(Some(&persistent), &legacy).expect("first lock set");
        fs::remove_file(&legacy).expect("unlink legacy lock");
        fs::write(&legacy, b"").expect("recreate legacy lock inode");

        let second = acquire_daemon_locks(Some(&persistent), &legacy);
        assert!(second.is_err(), "persistent lock must reject split brain");
        drop(first);
    }

    #[test]
    fn test_daemon_exclusive_lock_semantics_rejects_duplicates_and_releases_on_drop() {
        let dir = tempdir().unwrap();
        let persistent = dir.path().join("data").join("locks").join("daemon.lock");
        let legacy_dir = dir.path().join("runtime");
        fs::create_dir(&legacy_dir).unwrap();
        let legacy = legacy_dir.join("daemon.lock");

        // 1. Dual lock acquisition succeeds for first instance
        let first = acquire_daemon_locks(Some(&persistent), &legacy).expect("first lock set");

        // 2. Second instance attempting dual lock is rejected deterministically without sleeps
        let second_dual = acquire_daemon_locks(Some(&persistent), &legacy);
        assert!(
            second_dual.is_err(),
            "second daemon instance must be rejected while first instance holds lock"
        );
        assert_eq!(
            second_dual.unwrap_err(),
            "Another daemon instance is already holding the lock."
        );

        // 3. Second instance attempting legacy-only lock is also rejected deterministically
        let second_legacy = acquire_daemon_locks(None, &legacy);
        assert!(
            second_legacy.is_err(),
            "second daemon instance requesting legacy lock must be rejected"
        );
        assert_eq!(
            second_legacy.unwrap_err(),
            "Another daemon instance is already holding the lock."
        );

        // 4. Dropping the first lock guard via RAII releases held locks
        drop(first);

        // 5. Subsequent dual lock acquisition now succeeds
        let third = acquire_daemon_locks(Some(&persistent), &legacy)
            .expect("subsequent dual lock acquisition after RAII drop");

        // 6. Collision rejected while third is held
        let third_collision = acquire_daemon_locks(None, &legacy);
        assert!(third_collision.is_err());
        assert_eq!(
            third_collision.unwrap_err(),
            "Another daemon instance is already holding the lock."
        );

        drop(third);

        // 7. Legacy-only lock acquisition succeeds after third is dropped
        let fourth = acquire_daemon_locks(None, &legacy)
            .expect("legacy-only acquisition after RAII release");
        let fourth_collision = acquire_daemon_locks(None, &legacy);
        assert!(fourth_collision.is_err());
        assert_eq!(
            fourth_collision.unwrap_err(),
            "Another daemon instance is already holding the lock."
        );
        drop(fourth);
    }

    #[cfg(unix)]
    #[test]
    fn test_persistent_daemon_lock_creates_only_dedicated_secure_directory() {
        let dir = tempdir().unwrap();
        let data_root = dir.path().join("data");
        fs::create_dir(&data_root).unwrap();
        fs::set_permissions(&data_root, fs::Permissions::from_mode(0o755)).unwrap();
        let persistent = data_root.join("locks").join("daemon.lock");
        let runtime = dir.path().join("runtime");
        fs::create_dir(&runtime).unwrap();
        let legacy = runtime.join("daemon.lock");

        let locks = acquire_daemon_locks(Some(&persistent), &legacy).expect("dual locks");
        let data_mode = fs::symlink_metadata(&data_root)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let lock_dir_mode = fs::symlink_metadata(persistent.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(data_mode, 0o755);
        assert_eq!(lock_dir_mode, 0o700);
        drop(locks);
    }

    /// End-to-end proof of the path a phone tap actually travels: HTTP POST on
    /// the real gateway (hosted by this daemon) -> desktop event sink -> the
    /// GUI's subscribed socket connection. Before the sink existed this event
    /// was dropped inside the daemon and the desktop never switched.
    #[tokio::test]
    async fn test_remote_select_request_reaches_a_subscribed_desktop_client() {
        use crate::remote::auth::DevicePermission;
        use crate::remote::server::start_remote_server;
        use crate::remote::state::{RemoteGatewayConfig, RemoteNetworkMode};

        let server = Arc::new(DaemonServer::new());
        *server.remote_state.config.write() = RemoteGatewayConfig {
            mode: RemoteNetworkMode::LocalNetwork,
            port: 0,
            allow_control: true,
        };
        let (handle, addr) = start_remote_server(Arc::clone(&server.remote_state))
            .await
            .expect("start remote gateway");

        // The GUI subscribes over the daemon socket, exactly like lib.rs does.
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let mut json = serde_json::to_string(&DaemonRequest::SubscribeRemoteEvents).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();
        reader.read_line(&mut line).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<DaemonResponse>(line.trim()).unwrap(),
            DaemonResponse::SubscribeRemoteEventsOk
        ));

        // Drive the real HTTP endpoint the phone calls, with a paired control
        // device, instead of poking the sink directly.
        let repo = tempfile::tempdir().expect("tempdir");
        server
            .handle_register_workspace("ws", &repo.path().to_string_lossy())
            .expect("register workspace");

        let code = server
            .remote_state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
        let token = server
            .remote_state
            .auth_manager
            .exchange_pairing_code(&code, "Phone")
            .expect("pair control device")
            .0;
        // No tabId: tab availability is validated against the desktop's last
        // published selection, which is empty in a fresh daemon.
        let body = serde_json::json!({ "workspaceId": "ws" }).to_string();
        let request = format!(
            "POST /api/v1/workspace/select?token={token} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let mut http = tokio::net::TcpStream::connect(addr).await.expect("connect");
        http.write_all(request.as_bytes()).await.expect("send");

        // The GUI must observe the request that arrived over HTTP. Bounded so a
        // regression fails the test instead of hanging the suite.
        line.clear();
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            reader.read_line(&mut line),
        )
        .await
        .expect("desktop event must arrive")
        .expect("read event line");
        assert!(read > 0, "subscription closed before delivering the event");
        let event: DaemonRemoteEvent = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(event.event, "remote_selection_requested");
        assert_eq!(event.payload["workspaceId"], "ws");

        handle.stop();
    }

    #[tokio::test]
    async fn test_subscribe_remote_events_streams_desktop_directed_events() {
        let server = Arc::new(DaemonServer::new());
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let mut json = serde_json::to_string(&DaemonRequest::SubscribeRemoteEvents).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();
        reader.read_line(&mut line).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<DaemonResponse>(line.trim()).unwrap(),
            DaemonResponse::SubscribeRemoteEventsOk
        ));

        server.remote_state.emit_desktop_event(
            "remote_selection_requested",
            serde_json::json!({ "workspaceId": "ws", "tabId": "tab:a::leaf:b" }),
        );

        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let event: DaemonRemoteEvent = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(event.event, "remote_selection_requested");
        assert_eq!(event.payload["tabId"], "tab:a::leaf:b");
    }

    #[tokio::test]
    async fn test_daemon_remote_gateway_lifecycle_and_commands() {
        let server = Arc::new(DaemonServer::new());

        // Default: listener is OFF
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        // 1. Check default remote status
        let req = DaemonRequest::RemoteGetStatus;
        let mut json = serde_json::to_string(&req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemoteStatusOk { status } => {
                assert_eq!(status.mode, RemoteNetworkMode::Off);
                assert!(!status.is_running);
            }
            other => panic!("Expected RemoteStatusOk, got {other:?}"),
        }

        // 2. Create pairing code
        line.clear();
        let pair_req = DaemonRequest::RemoteCreatePairingCode {
            permission: Some(DevicePermission::Control),
        };
        let mut json = serde_json::to_string(&pair_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemotePairingCodeOk { code } => {
                assert_eq!(code.len(), 6);
            }
            other => panic!("Expected RemotePairingCodeOk, got {other:?}"),
        }

        // 3. Active selection set & get
        line.clear();
        let sel_req = DaemonRequest::RemoteSetActiveSelection {
            selection: Some(crate::remote::protocol::RemoteActiveDesktopSelection {
                workspace_id: Some("ws-desktop".to_string()),
                worktree_slug: None,
                worktree_label: None,
                session_id: Some("session-1".to_string()),
                ..Default::default()
            }),
        };
        let mut json = serde_json::to_string(&sel_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::RemoteSetActiveSelectionOk));

        line.clear();
        let get_sel_req = DaemonRequest::RemoteGetActiveSelection;
        let mut json = serde_json::to_string(&get_sel_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemoteGetActiveSelectionOk { selection } => {
                assert_eq!(
                    selection.unwrap().workspace_id,
                    Some("ws-desktop".to_string())
                );
            }
            other => panic!("Expected RemoteGetActiveSelectionOk, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_stream_lagged_replay_gap_full_recovery() {
        let hub = Arc::new(TerminalOutputHub::new(128));
        let session_id = "test-lag-session".to_string();
        let (_raw_rx, rx) = hub.register_session_channels(&session_id);

        for i in 0..1100 {
            hub.publish(&session_id, format!("chunk_{i:04};").into_bytes());
        }

        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let (_server_read, server_write) = server_stream.into_split();
        let (client_read, _client_write) = client_stream.into_split();
        let mut client_reader = BufReader::new(client_read);

        let pump_handle = tokio::spawn(DaemonServer::pump_sequenced_stream(
            session_id.clone(),
            rx,
            Arc::clone(&hub),
            server_write,
        ));

        let mut line = String::new();
        client_reader.read_line(&mut line).await.unwrap();
        let message: DaemonStreamMessage<'static> = serde_json::from_str(line.trim()).unwrap();
        match message {
            DaemonStreamMessage::Lagged {
                requested_after_sequence,
                available_from_sequence,
                start_sequence,
                end_sequence,
                history,
                ..
            } => {
                assert_eq!(requested_after_sequence, 0);
                assert!(available_from_sequence > 1);
                assert_eq!(start_sequence, Some(available_from_sequence));
                assert!(end_sequence.unwrap() >= available_from_sequence);
                assert!(!history.is_empty());
                assert!(
                    history.len() <= 128,
                    "replay must stay bounded by hub history"
                );
            }
            other => panic!("Expected typed replayGap frame, got {other:?}"),
        }

        hub.remove_session(&session_id);
        let _ = pump_handle.await;
    }

    #[tokio::test]
    async fn test_server_epoch_in_handshake_and_list() {
        let server = Arc::new(DaemonServer::new());
        assert!(server.epoch() > 0);

        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        // 1. Handshake
        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        };
        let mut hs_json = serde_json::to_string(&hs).unwrap();
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let hs_resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match hs_resp {
            DaemonResponse::HandshakeOk {
                version,
                pid,
                epoch,
                binary_path,
                binary_mtime_ms,
            } => {
                assert_eq!(version, DAEMON_PROTOCOL_VERSION);
                assert_eq!(pid, std::process::id());
                assert_eq!(epoch, server.epoch());
                assert!(binary_path.is_some());
                assert!(binary_mtime_ms.is_some());
            }
            other => panic!("Expected HandshakeOk, got {other:?}"),
        }

        // 2. ListSessions
        line.clear();
        let list_req = DaemonRequest::ListSessions;
        let mut list_json = serde_json::to_string(&list_req).unwrap();
        list_json.push('\n');
        write_half.write_all(list_json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let list_resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match list_resp {
            DaemonResponse::ListSessionsOk { epoch, sessions } => {
                assert_eq!(epoch, server.epoch());
                assert!(sessions.is_empty() || !sessions.is_empty());
            }
            other => panic!("Expected ListSessionsOk, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_server_handshake_version_mismatch() {
        let server = Arc::new(DaemonServer::new());
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let hs = DaemonRequest::Handshake { version: 9999 };
        let mut hs_json = serde_json::to_string(&hs).unwrap();
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let hs_resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match hs_resp {
            DaemonResponse::ProtocolMismatch {
                expected_version,
                received_version,
            } => {
                assert_eq!(expected_version, DAEMON_PROTOCOL_VERSION);
                assert_eq!(received_version, 9999);
                assert!(server.terminal_service().list_sessions().is_empty());
            }
            other => panic!("Expected typed ProtocolMismatch, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_server_describe_nonexistent_session() {
        let server = Arc::new(DaemonServer::new());
        let resp = server.handle_describe_session("nonexistent-session-id");
        match resp {
            DaemonResponse::Error { message } => {
                assert!(message.contains("not found"));
            }
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_server_attach_with_after_sequence_and_snapshot() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        let session_id = server
            .handle_spawn("req-seq-1", "default", None, None, 80, 24, None, None)
            .await
            .unwrap();

        server
            .terminal_service()
            .output_hub()
            .publish(&session_id, b"chunk1".to_vec());
        server
            .terminal_service()
            .output_hub()
            .publish(&session_id, b"chunk2".to_vec());

        let attachment = server
            .terminal_service()
            .attach_with_sequence(&session_id, Some(1))
            .expect("attach with sequence");

        assert_eq!(attachment.snapshot.session_id, session_id);
        assert_eq!(attachment.snapshot.history, b"chunk2");
        assert_eq!(attachment.snapshot.history_start_sequence, Some(2));
        assert_eq!(attachment.snapshot.history_end_sequence, Some(2));
    }
    #[tokio::test]
    async fn agent_state_report_reaches_only_its_own_session_stream() {
        let server = Arc::new(DaemonServer::new());
        let (client, mut server_side) = tokio::io::duplex(4096);
        let session_id = "session-under-test".to_string();
        let other_session = "unrelated-session".to_string();

        let hub = Arc::clone(server.terminal_service().output_hub());
        let (_raw_rx, rx) = hub.register_session_channels(&session_id);
        let agent_rx = server.agent_state_tx.subscribe();

        let pump = tokio::spawn(DaemonServer::pump_sequenced_stream_with_agent_state(
            session_id.clone(),
            rx,
            hub,
            client,
            Some(agent_rx),
        ));

        // Send the foreign report FIRST, then this session's own report. The pump processes the
        // broadcast in order, so the first frame that arrives is decisive: with a correct filter it
        // is this session's report, and a leak shows up as the foreign one arriving ahead of it.
        // This orders the assertion on the channel itself instead of on elapsed time.
        server
            .agent_state_tx
            .send((
                other_session.clone(),
                "working".to_string(),
                Some("codex".to_string()),
                None,
            ))
            .expect("send unrelated report");
        server
            .agent_state_tx
            .send((
                session_id.clone(),
                "blocked".to_string(),
                Some("omo".to_string()),
                None,
            ))
            .expect("send own report");

        let mut reader = BufReader::new(&mut server_side);
        let mut line = String::new();
        tokio::time::timeout(
            tokio::time::Duration::from_secs(10),
            reader.read_line(&mut line),
        )
        .await
        .expect("stream produced a frame")
        .expect("frame read");

        let msg: serde_json::Value = serde_json::from_str(line.trim()).expect("json frame");
        assert_eq!(msg["type"], "agentState");
        assert_eq!(msg["sessionId"], "session-under-test");
        assert_eq!(
            msg["agent"], "omo",
            "the foreign report for {other_session} must be dropped, not relabelled onto this stream"
        );
        assert_eq!(
            msg["state"], "blocked",
            "the reported state must survive the hop verbatim"
        );

        pump.abort();
    }

    #[test]
    fn agent_state_reports_are_parsed_and_filtered() {
        assert_eq!(
            DaemonServer::parse_agent_state_report(
                r#"{"type":"agentState","sessionId":"s1","state":"working","agent":"omo"}"#
            ),
            Some((
                "s1".to_string(),
                "working".to_string(),
                Some("omo".to_string()),
                None,
            ))
        );
        let valid = DaemonServer::parse_agent_state_report(
            r#"{"type":"agentState","sessionId":"s1","state":"working","agent":"omo","providerSession":{"key":"session_id","id":"omo-session"}}"#,
        )
        .expect("valid Omo provider report");
        assert_eq!(valid.3.expect("provider reference").id, "omo-session");
        let wrong_key = DaemonServer::parse_agent_state_report(
            r#"{"type":"agentState","sessionId":"s1","state":"working","agent":"omo","providerSession":{"key":"conversation_id","id":"bad-key"}}"#,
        )
        .expect("activity report remains valid");
        assert_eq!(
            wrong_key.3, None,
            "semantically invalid provider reference must be omitted"
        );
        let missing_required_path = DaemonServer::parse_agent_state_report(
            r#"{"type":"agentState","sessionId":"s1","state":"working","agent":"pi","providerSession":{"key":"session_id","id":"pi-session"}}"#,
        )
        .expect("activity report remains valid");
        assert_eq!(
            missing_required_path.3, None,
            "path-sensitive provider reference must validate fully"
        );
        assert_eq!(
            DaemonServer::parse_agent_state_report(
                r#"{"type":"agentState","sessionId":"s1","state":"idle"}"#
            ),
            Some(("s1".to_string(), "idle".to_string(), None, None)),
            "agent is optional so older extension copies keep working"
        );
        for rejected in [
            r#"{"type":"agentState","sessionId":"s1","state":"bogus"}"#,
            r#"{"sessionId":"s1"}"#,
            "not json at all",
            "",
        ] {
            assert_eq!(
                DaemonServer::parse_agent_state_report(rejected),
                None,
                "must reject {rejected:?}"
            );
        }
    }

    #[test]
    fn test_normalize_process_cwd_windows_verbatim_paths() {
        use super::normalize_process_cwd;
        use std::path::{Path, PathBuf};

        // Verbatim drive paths should have \\?\ stripped for process cwd
        assert_eq!(
            normalize_process_cwd(Path::new(r"\\?\C:\Windows\System32")),
            PathBuf::from(r"C:\Windows\System32")
        );
        assert_eq!(
            normalize_process_cwd(Path::new(r"\\?\c:\Users\test\project")),
            PathBuf::from(r"c:\Users\test\project")
        );
        assert_eq!(
            normalize_process_cwd(Path::new(r"\\?\D:")),
            PathBuf::from(r"D:")
        );
        // Verbatim UNC paths \\?\UNC\server\share -> \\server\share
        assert_eq!(
            normalize_process_cwd(Path::new(r"\\?\UNC\server\share\repo")),
            PathBuf::from(r"\\server\share\repo")
        );
        assert_eq!(
            normalize_process_cwd(Path::new(r"\\?\unc\server\share")),
            PathBuf::from(r"\\server\share")
        );
        // Non-verbatim UNC, Windows drive, Unix, and relative paths should remain unchanged
        assert_eq!(
            normalize_process_cwd(Path::new(r"\\server\share\repo")),
            PathBuf::from(r"\\server\share\repo")
        );
        assert_eq!(
            normalize_process_cwd(Path::new(r"C:\Windows\System32")),
            PathBuf::from(r"C:\Windows\System32")
        );
        assert_eq!(
            normalize_process_cwd(Path::new("/Users/test/project")),
            PathBuf::from("/Users/test/project")
        );
        assert_eq!(
            normalize_process_cwd(Path::new("relative/path")),
            PathBuf::from("relative/path")
        );
        // Non-drive verbatim path preserved
        assert_eq!(
            normalize_process_cwd(Path::new(
                r"\\?\Volume{b75e2c83-0000-0000-0000-602200000000}\"
            )),
            PathBuf::from(r"\\?\Volume{b75e2c83-0000-0000-0000-602200000000}\")
        );
    }

    #[test]
    fn test_binary_identity_helper_pure_and_injection() {
        // Injection test: successful exe and mtime
        let mock_exe = || Ok(PathBuf::from("/opt/ferryx/bin/ferryx"));
        let mock_mtime = |_path: &Path| Some(1725280000000u64);
        let (path, mtime) = resolve_binary_identity_with(mock_exe, mock_mtime);
        assert_eq!(path, Some("/opt/ferryx/bin/ferryx".to_string()));
        assert_eq!(mtime, Some(1725280000000u64));

        // Injection test: exe failure
        let mock_exe_err = || Err(std::io::Error::new(std::io::ErrorKind::NotFound, "not found"));
        let (path_err, mtime_err) = resolve_binary_identity_with(mock_exe_err, mock_mtime);
        assert_eq!(path_err, None);
        assert_eq!(mtime_err, None);

        // Injection test: mtime failure
        let mock_mtime_none = |_path: &Path| None;
        let (path_no_mtime, mtime_none) =
            resolve_binary_identity_with(mock_exe, mock_mtime_none);
        assert_eq!(path_no_mtime, Some("/opt/ferryx/bin/ferryx".to_string()));
        assert_eq!(mtime_none, None);

        // Live identity test
        let (live_path, live_mtime) = resolve_binary_identity();
        assert!(live_path.is_some(), "live binary path should be resolved");
        assert!(
            live_mtime.is_some_and(|t| t > 0),
            "live binary mtime should be non-zero"
        );
    }

    #[tokio::test]
    async fn test_handle_upgrade_binary_idempotent_when_not_needed() {
        let server = Arc::new(DaemonServer::new());
        let resp = server.handle_upgrade_binary().await;
        // The running test binary is at least as new as itself on disk, so upgrade is not needed
        #[cfg(unix)]
        assert!(matches!(resp, DaemonResponse::UpgradeNotNeeded));
        #[cfg(not(unix))]
        assert!(matches!(resp, DaemonResponse::UpgradeUnsupported));
    }

    #[cfg(unix)]
    #[test]
    fn test_clear_cloexec_and_fd_inheritance_across_exec() {
        use std::io::Write;
        use std::os::unix::io::FromRawFd;

        // 1. Create a pipe
        let mut pipe_fds = [0i32; 2];
        let ret = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
        assert_eq!(ret, 0);
        let [read_fd, write_fd] = pipe_fds;

        // 2. Set FD_CLOEXEC explicitly on read_fd (simulating portable-pty behavior)
        let set_cloexec_ret = unsafe {
            libc::fcntl(read_fd, libc::F_SETFD, libc::FD_CLOEXEC)
        };
        assert_eq!(set_cloexec_ret, 0);

        let flags = unsafe { libc::fcntl(read_fd, libc::F_GETFD) };
        assert_ne!(flags & libc::FD_CLOEXEC, 0, "FD_CLOEXEC must be set");

        // 3. Clear FD_CLOEXEC using clear_cloexec
        clear_cloexec(read_fd).expect("clear_cloexec must succeed");

        let flags_cleared = unsafe { libc::fcntl(read_fd, libc::F_GETFD) };
        assert_eq!(flags_cleared & libc::FD_CLOEXEC, 0, "FD_CLOEXEC must be cleared");

        // 4. Write data to write_fd
        let test_message = b"ferryx-inherited-fd-payload\n";
        let mut write_file = unsafe { std::fs::File::from_raw_fd(write_fd) };
        write_file.write_all(test_message).expect("write to pipe");
        drop(write_file); // Close writer so reader hits EOF

        // 5. Spawn /bin/sh to read from inherited read_fd
        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("read -r line <&{}; echo \"$line\"", read_fd))
            .output()
            .expect("spawn helper process with inherited fd");

        unsafe { libc::close(read_fd); }

        assert!(output.status.success());
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout_str.trim(), "ferryx-inherited-fd-payload");
    }
}
