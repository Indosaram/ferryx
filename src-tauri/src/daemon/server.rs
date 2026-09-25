// allow: SIZE_OK — daemon IPC server implementation with routing, session persistence offloading, remote control, and streaming
#[path = "machine_gateway.rs"]
mod machine_gateway;
use super::session_service::*;
use crate::daemon::agent_state::{AgentState, AgentStateHub, AgentStateSubscription};
#[cfg(test)]
use crate::daemon::protocol::AgentProviderSessionKey;
use crate::daemon::protocol::{
    AgentStateReport, DaemonRemoteEvent, DaemonRemoteStatus, DaemonRequest, DaemonResponse,
    DaemonStreamMessage, HistorySegmentWire, TerminalStartup, DAEMON_PROTOCOL_VERSION,
};
use crate::remote::auth::DevicePermission;
use crate::remote::server::{start_remote_server, RemoteServerHandle};
use crate::remote::state::{
    RemoteGatewayConfig, RemoteGatewayState, RemoteNetworkMode, REMOTE_GATEWAY_PORT,
};
use crate::session::{clear_session_from_path, load_session_from_path, save_session_to_path};
use crate::terminal::{PtyManager, TerminalOutputHub, TerminalService};
use crate::worktree::{WorkspaceRegistry, WorktreeIdentity};
use super::session_service::*;
use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
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
    if let Some(dir) = std::env::var_os("FERRYX_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
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
    if let Some(dir) = std::env::var_os("FERRYX_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
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

/// Resolve the executable a daemon upgrade or handover should launch.
///
/// A daemon that survived an in-place application replacement keeps executing from an
/// unlinked inode, so `current_exe()` can name a path that no longer exists. Launching that
/// path fails, and it fails *after* the canonical listener has already been handed over,
/// which would leave no daemon serving either socket. Only ever resolve to a file that
/// exists right now; the caller must treat `None` as "upgrade unavailable".
pub fn resolve_upgrade_target_exe_with<FExe>(
    explicit: Option<&str>,
    get_exe: FExe,
) -> Option<PathBuf>
where
    FExe: Fn() -> std::io::Result<PathBuf>,
{
    if let Some(path) = explicit.map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }
    match get_exe() {
        Ok(exe) if exe.is_file() => Some(exe),
        _ => None,
    }
}

pub fn resolve_upgrade_target_exe(explicit: Option<&str>) -> Option<PathBuf> {
    resolve_upgrade_target_exe_with(explicit, std::env::current_exe)
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
pub fn perform_daemon_exec_with_path(exe: &std::path::Path) -> Result<(), std::io::Error> {
    use std::os::unix::process::CommandExt;

    tracing::info!("Re-executing daemon with binary at: {}", exe.display());

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    let err = cmd.exec();
    tracing::error!("Daemon exec failed: {err}");
    Err(err)
}

#[cfg(unix)]
pub fn perform_daemon_exec() -> Result<(), std::io::Error> {
    let exe = std::env::current_exe()?;
    perform_daemon_exec_with_path(&exe)
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

    /// Detaches the underlying file without invoking `LOCK_UN`, preserving the lock
    /// across ownership transfer to the successor process.
    pub(crate) fn detach_without_unlock(mut self) -> File {
        // SAFETY: `self._file` is a valid File. We read it out and forget `self`
        // so that the `Drop` implementation (which calls `LOCK_UN`) does not execute.
        let file = unsafe { std::ptr::read(&self._file) };
        std::mem::forget(self);
        file
    }

    /// Constructs a `DaemonLockFile` from an already-locked file descriptor received
    /// during handover transfer.
    pub(crate) fn from_locked_file(file: File) -> Self {
        Self { _file: file }
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

    /// Detaches the underlying file without invoking `UnlockFileEx`, preserving the lock
    /// across ownership transfer to the successor process.
    pub(crate) fn detach_without_unlock(self) -> File {
        let file = unsafe { std::ptr::read(&self.file) };
        std::mem::forget(self);
        file
    }

    /// Constructs a `DaemonLockFile` from an already-locked file handle received
    /// during handover transfer.
    pub(crate) fn from_locked_file(file: File) -> Self {
        Self { file }
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

    pub(crate) fn detach_without_unlock(self) -> File {
        let file = unsafe { std::ptr::read(&self._file) };
        std::mem::forget(self);
        file
    }

    pub(crate) fn from_locked_file(file: File) -> Self {
        Self { _file: file }
    }
}

#[derive(Debug)]
pub(crate) struct DaemonLockFiles {
    _persistent: Option<DaemonLockFile>,
    _legacy: DaemonLockFile,
}

impl DaemonLockFiles {
    /// Detaches both lock files without invoking `LOCK_UN`, preserving held locks
    /// across handover ownership transfer.
    pub(crate) fn detach_without_unlock(self) -> (Option<File>, File) {
        let mut me = std::mem::ManuallyDrop::new(self);
        let persistent = me._persistent.take().map(|p| p.detach_without_unlock());
        let legacy = unsafe { std::ptr::read(&me._legacy) }.detach_without_unlock();
        (persistent, legacy)
    }

    /// Adopts pre-locked files received via handover descriptor transfer.
    pub(crate) fn from_locked_files(persistent: Option<File>, legacy: File) -> Self {
        Self {
            _persistent: persistent.map(DaemonLockFile::from_locked_file),
            _legacy: DaemonLockFile::from_locked_file(legacy),
        }
    }
}

fn try_lock_file(file: File) -> Result<DaemonLockFile, String> {
    DaemonLockFile::try_lock(file)
}

/// Takes one lock file, optionally outlasting a predecessor that is still shutting down. The
/// wait is only ever armed for a declared successor, so ordinary startup keeps failing fast.
/// The deadline is checked before every attempt, so a closed window can never be reopened by a
/// lock that happens to free up after it.
fn lock_file_with_optional_wait(
    path: &Path,
    deadline: Option<std::time::Instant>,
) -> Result<DaemonLockFile, String> {
    lock_file_with_optional_wait_internal(path, deadline, || {})
}

fn lock_file_with_optional_wait_internal<F: FnMut()>(
    path: &Path,
    deadline: Option<std::time::Instant>,
    mut on_pre_attempt: F,
) -> Result<DaemonLockFile, String> {
    let mut last_error: Option<String> = None;
    loop {
        let Some(deadline) = deadline else {
            let file = open_secure_lock_file(path)?;
            return try_lock_file(file);
        };
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(last_error.unwrap_or_else(|| {
                format!(
                    "Timed out waiting for the daemon instance lock at {}",
                    path.display()
                )
            }));
        }
        on_pre_attempt();
        let file = open_secure_lock_file(path)?;
        match try_lock_file(file) {
            Ok(locked) => {
                if std::time::Instant::now() >= deadline {
                    drop(locked);
                    return Err(format!(
                        "Acquired instance lock at {} after the wait window expired",
                        path.display()
                    ));
                }
                return Ok(locked);
            }
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(remaining.min(std::time::Duration::from_millis(200)));
            }
        }
    }
}

pub(crate) fn acquire_daemon_locks(
    persistent_path: Option<&Path>,
    legacy_path: &Path,
) -> Result<DaemonLockFiles, String> {
    let deadline = crate::daemon::handover::successor_wait_from_env()
        .map(|wait| std::time::Instant::now() + wait);
    let persistent = if let Some(path) = persistent_path {
        let parent = path
            .parent()
            .ok_or_else(|| format!("Persistent lock path has no parent: {}", path.display()))?;
        ensure_runtime_directory(parent)?;
        let locked = lock_file_with_optional_wait(path, deadline)?;
        Some(locked)
    } else {
        None
    };

    let locked = lock_file_with_optional_wait(legacy_path, deadline)?;
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

/// Runtime file carrying this boot's daemon transport token, beside the `daemon.port` file the
/// loopback listener publishes. A loopback TCP port has no filesystem ownership check, so the
/// published port alone must not be enough to drive the daemon.
#[cfg(not(unix))]
pub fn get_transport_token_path() -> PathBuf {
    get_runtime_dir().join("daemon.token")
}

/// Length of every generated transport token.
#[cfg(not(unix))]
const TRANSPORT_TOKEN_LEN: usize = 48;

/// One per-boot bearer token, drawn from the crate's CSPRNG.
#[cfg(not(unix))]
fn generate_transport_token() -> String {
    use rand::distributions::Alphanumeric;
    use rand::Rng;

    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(TRANSPORT_TOKEN_LEN)
        .map(char::from)
        .collect()
}

/// Publishes this boot's loopback rendezvous pair in the order a reader depends on: the previous
/// boot's token is dropped first, this boot's token is written second, and the port is published
/// last. The ordering guarantees that this boot's port is never on disk before this boot's token.
/// It does not extend to a port a predecessor left behind: while this boot is republishing, a
/// reader can still find that port with no token beside it, an interval that is harmless because
/// the daemon rejects a credential it did not mint and `DaemonClient` re-reads both halves once.
///
/// The stale port is dropped by the caller, not here: `remove_stale_socket_after_lock` removes it
/// under the instance lock before the listener binds. The ordering does not make the pair atomic,
/// so a reader can still assemble this boot's token beside a predecessor's port; that pair is made
/// harmless by the daemon rejecting a credential it did not mint and by `DaemonClient` re-reading
/// both halves once.
///
/// `on_token_published` observes the instant between the two writes, which is the only instant
/// that decides the contract; the ordering test in `agent_state_transport_tests` pins it without
/// a daemon by reading both files there.
#[cfg(any(not(unix), test))]
fn publish_transport_rendezvous_internal<F: FnMut()>(
    port_path: &Path,
    token_path: &Path,
    port: u16,
    token: &str,
    mut on_token_published: F,
) -> std::io::Result<()> {
    // The previous boot's token goes first: while it is gone and the port has not been rewritten,
    // a reader's pair is absent rather than mismatched, so it re-reads both halves instead of
    // presenting a credential that cannot belong to the port it read.
    match fs::remove_file(token_path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(%error, "Failed to remove stale daemon transport token"),
    }
    fs::write(token_path, token)?;
    on_token_published();
    fs::write(port_path, port.to_string())
}

/// Publishes the pair with no observer attached.
#[cfg(not(unix))]
fn publish_transport_rendezvous(
    port_path: &Path,
    token_path: &Path,
    port: u16,
    token: &str,
) -> std::io::Result<()> {
    publish_transport_rendezvous_internal(port_path, token_path, port, token, || {})
}

/// Constant-time comparison of a presented token against the expected one. An absent or empty
/// token never matches, so a client that omits the credential is rejected exactly like one that
/// guesses wrong. Compiled for the loopback transport and for the tests that pin it.
#[cfg(any(not(unix), test))]
fn transport_token_matches(expected: &str, presented: Option<&str>) -> bool {
    let Some(presented) = presented.filter(|token| !token.is_empty()) else {
        return false;
    };
    if presented.len() != expected.len() {
        return false;
    }
    presented
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |acc, (presented, expected)| {
            acc | (presented ^ expected)
        })
        == 0
}

/// Reads the bearer token out of one raw protocol line. The token travels beside the payload as
/// `token`, the shape the agent extension's TCP mode already writes for remote sessions, so no
/// wire struct has to carry it.
#[cfg(any(not(unix), test))]
fn transport_token_from_line(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    value.get("token")?.as_str().map(str::to_owned)
}

pub fn get_agent_state_socket_path() -> PathBuf {
    get_runtime_dir().join("agent-state.sock")
}

pub fn agent_state_socket_path() -> String {
    get_agent_state_socket_path().to_string_lossy().into_owned()
}

/// Name of the rendezvous record the non-unix ingress publishes beside its socket: the loopback
/// port on the first line, the bearer token a pane must present on the second.
const AGENT_STATE_RENDEZVOUS_FILE: &str = "agent-state.rendezvous";

/// Runtime record carrying the non-unix ingress endpoint. `apply_agent_state_tcp_env` reads
/// exactly this path as a pane spawns.
pub fn get_agent_state_rendezvous_path() -> PathBuf {
    get_runtime_dir().join(AGENT_STATE_RENDEZVOUS_FILE)
}

/// Publishes the ingress endpoint as one record renamed into place: a reader can never observe
/// this boot's port beside a previous boot's token, or the reverse.
///
/// `fs::rename` replaces an existing destination on every target: Unix `rename(2)`, and Windows
/// `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` (see `library/std/src/sys/fs/windows.rs`),
/// matching the `std::fs::rename` documentation's "replacing the original file if `to` already
/// exists". A Windows reader does not block the replace: std opens files with a default share
/// mode of `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`.
#[cfg(any(not(unix), test))]
fn publish_agent_state_rendezvous(
    runtime_dir: &Path,
    port: u16,
    token: &str,
) -> std::io::Result<()> {
    let staged = runtime_dir.join(format!("{AGENT_STATE_RENDEZVOUS_FILE}.tmp"));
    fs::write(&staged, format!("{port}\n{token}\n"))?;
    fs::rename(&staged, runtime_dir.join(AGENT_STATE_RENDEZVOUS_FILE))
}

/// Publishes the rendezvous record and only then records the endpoint the accessor reports, so an
/// endpoint is observable exactly when a pane can discover the pair from disk. A publish that
/// fails leaves the endpoint untouched rather than reporting a pair that was never made
/// discoverable.
#[cfg(any(not(unix), test))]
fn publish_agent_state_endpoint(
    endpoint: &Mutex<Option<(u16, String)>>,
    runtime_dir: &Path,
    port: u16,
    token: &str,
) -> std::io::Result<()> {
    publish_agent_state_rendezvous(runtime_dir, port, token)?;
    *endpoint.lock() = Some((port, token.to_owned()));
    Ok(())
}

/// Drops a previous boot's rendezvous record, including the two-file pair older builds published.
/// A listener that fails to bind or publish must not leave a stale pair looking valid.
#[cfg(any(not(unix), test))]
fn clear_stale_agent_state_rendezvous(runtime_dir: &Path) {
    let staged = format!("{AGENT_STATE_RENDEZVOUS_FILE}.tmp");
    for name in [
        AGENT_STATE_RENDEZVOUS_FILE,
        staged.as_str(),
        "agent-state.port",
        "agent-state.token",
    ] {
        match fs::remove_file(runtime_dir.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                %error,
                file = name,
                "Failed to remove stale agent state rendezvous"
            ),
        }
    }
}

pub(crate) use super::session_service::normalize_process_cwd;

fn daemon_ssh_store_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("FERRYX_DATA_DIR") {
        return PathBuf::from(dir).join("ssh_hosts.json");
    }
    let base = dirs_next()
        .unwrap_or_else(get_runtime_dir)
        .join("com.ferryx.app");
    if is_dev_runtime() {
        base.join("dev/ssh_hosts.json")
    } else {
        base.join("ssh_hosts.json")
    }
}

pub struct DaemonServer {
    paired_hosts: crate::paired_host::service::PairedHostService,
    pub session_router: Arc<crate::daemon::proxy::SessionRouter>,
    pub handover_manager: Arc<crate::daemon::handover::HandoverManager>,
    terminal_service: Arc<TerminalService>,
    workspace_registry: WorkspaceRegistry,
    remote_state: Arc<RemoteGatewayState>,
    remote_server_handle: Arc<Mutex<Option<RemoteServerHandle>>>,
    epoch: u64,
    binary_path: Option<String>,
    binary_mtime_ms: Option<u64>,
    pub(crate) session_service: Arc<DaemonSessionService>,
    /// Loopback TCP endpoint (`port`, bearer `token`) of the agent-state ingress, recorded once
    /// that ingress published its rendezvous record. `None` on unix, whose ingress is the socket
    /// path [`agent_state_socket_path`] returns, and `None` on every platform until then: every
    /// failure to bind or publish leaves it `None`, so it is `Some` only while a pane can discover
    /// the pair from disk.
    agent_state_endpoint: Mutex<Option<(u16, String)>>,
    /// Set once this boot's agent-state ingress was observed to fail. It is the only thing that
    /// separates "not published because binding or publishing FAILED" from "not published yet":
    /// the endpoint above is `None` in both cases, so a caller reporting an absent endpoint needs
    /// this flag to know whether it is looking at a degradation.
    agent_state_ingress_unavailable: std::sync::atomic::AtomicBool,
    /// Bearer token every connection must present before a request is dispatched. Windows has no
    /// filesystem ownership boundary on the transport, so the published port alone cannot
    /// authenticate a client.
    #[cfg(not(unix))]
    transport_token: String,
    #[cfg(test)]
    helper_home: Option<String>,
    remote_event_tx: broadcast::Sender<DaemonRemoteEvent>,
    #[cfg(test)]
    _catalog_fixture: Option<tempfile::TempDir>,
}

#[cfg(test)]
mod agent_state_transport_tests {
    use super::*;

    #[test]
    fn transport_token_matches_only_the_exact_credential() {
        let token = "9Zq3r8Tf2kLp";
        assert!(transport_token_matches(token, Some(token)));
        assert!(!transport_token_matches(token, None));
        assert!(!transport_token_matches(token, Some("")));
        assert!(!transport_token_matches(token, Some("9Zq3r8Tf2kL")));
        assert!(!transport_token_matches(token, Some("9Zq3r8Tf2kLq")));
        assert!(!transport_token_matches(token, Some("9Zq3r8Tf2kLx")));
    }

    #[test]
    fn transport_token_is_read_from_the_request_line() {
        assert_eq!(
            transport_token_from_line(r#"{"type":"handshake","version":5,"token":"abc"}"#)
                .as_deref(),
            Some("abc")
        );
        assert_eq!(
            transport_token_from_line(
                r#"{"type":"agentState","sessionId":"s","state":"idle","token":"abc"}"#
            )
            .as_deref(),
            Some("abc")
        );
        assert_eq!(
            transport_token_from_line(r#"{"type":"handshake","version":5}"#),
            None
        );
        assert_eq!(
            transport_token_from_line(r#"{"type":"handshake","token":42}"#),
            None
        );
        assert_eq!(transport_token_from_line("not json"), None);
    }

    #[test]
    fn the_port_is_published_only_after_the_token_that_authenticates_it() {
        let dir = tempfile::tempdir().unwrap();
        let port_path = dir.path().join("daemon.port");
        let token_path = dir.path().join("daemon.token");
        // The pair a previous boot left on disk, as a reader finds it mid-restart.
        std::fs::write(&port_path, "41111").unwrap();
        std::fs::write(&token_path, "previous-boot-token").unwrap();

        // Observed between the two writes: a new port visible here would mean a reader can pair a
        // new port with the token the previous boot published beside the old one.
        let observed = std::cell::RefCell::new(None);
        publish_transport_rendezvous_internal(
            &port_path,
            &token_path,
            52222,
            "this-boot-token",
            || {
                *observed.borrow_mut() = Some((
                    std::fs::read_to_string(&port_path).unwrap(),
                    std::fs::read_to_string(&token_path).unwrap(),
                ));
            },
        )
        .unwrap();

        let (port_between_writes, token_between_writes) = observed.into_inner().unwrap();
        assert_eq!(
            port_between_writes, "41111",
            "the port must still be the predecessor's when this boot's token becomes readable, \
             otherwise a reader finds a port whose credential is not yet on disk"
        );
        assert_eq!(token_between_writes, "this-boot-token");
        assert_eq!(
            std::fs::read_to_string(&token_path).unwrap(),
            "this-boot-token"
        );
        assert_eq!(std::fs::read_to_string(&port_path).unwrap(), "52222");
    }

    #[test]
    fn agent_state_endpoint_is_absent_until_the_ingress_binds() {
        let server = DaemonServer::new_with_paths(None, None);
        assert_eq!(server.agent_state_endpoint(), None);
    }

    #[test]
    fn a_failed_rendezvous_publish_leaves_no_endpoint_to_report() {
        let runtime_dir = std::env::temp_dir().join(format!(
            "ferryx-endpoint-unpublished-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&runtime_dir).ok();

        // An absent runtime directory is the publish failure the ingress reports as unavailable.
        // The endpoint must stay absent, or a caller observes an unavailable ingress beside an
        // endpoint that no pane can discover.
        let endpoint = Mutex::new(None);
        assert!(
            publish_agent_state_endpoint(&endpoint, &runtime_dir, 41_234, "token-abc").is_err(),
            "publishing into an absent runtime directory must fail"
        );
        assert_eq!(
            *endpoint.lock(),
            None,
            "an unpublished rendezvous must never be reported as an endpoint"
        );
        assert!(!runtime_dir.exists());

        // The endpoint becomes observable only with the record it names, and it names the pair
        // that record carries.
        fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        publish_agent_state_endpoint(&endpoint, &runtime_dir, 41_234, "token-abc")
            .expect("publish endpoint");
        assert_eq!(*endpoint.lock(), Some((41_234, "token-abc".to_string())));
        assert_eq!(
            fs::read_to_string(runtime_dir.join(AGENT_STATE_RENDEZVOUS_FILE))
                .expect("read published record"),
            "41234\ntoken-abc\n"
        );

        fs::remove_dir_all(&runtime_dir).ok();
    }

    #[test]
    fn readiness_is_released_only_after_the_ingress_failure_is_recorded() {
        let server = DaemonServer::new_with_paths(None, None);
        let (ready_tx, mut ready_rx) = tokio::sync::oneshot::channel();
        // Read from inside the readiness signal itself, so the assertion is on the ordering and
        // not on the final state: a boot that signals first and records afterwards observes
        // `false` here and fails.
        let recorded_before_signal = std::cell::Cell::new(None);
        let probe = &server;
        probe.settle_agent_state_ingress(None, || {
            recorded_before_signal.set(Some(probe.agent_state_ingress_unavailable()));
            let _ = ready_tx.send(());
        });
        assert_eq!(
            recorded_before_signal.get(),
            Some(true),
            "the failed ingress must be recorded before readiness is released"
        );
        assert!(
            server.agent_state_ingress_unavailable(),
            "a boot without a usable ingress must record it rather than leave callers to infer it"
        );
        assert!(
            ready_rx.try_recv().is_ok(),
            "readiness must still be signalled: terminals keep working without the ingress"
        );
    }

    #[tokio::test]
    async fn a_bound_ingress_is_not_recorded_as_unavailable() {
        let server = DaemonServer::new_with_paths(None, None);
        let (ready_tx, mut ready_rx) = tokio::sync::oneshot::channel();
        // A live handle stands in for an ingress that bound and published its endpoint.
        let ingress = Some(tokio::spawn(async {}));
        let probe = &server;
        probe.settle_agent_state_ingress(ingress, || {
            let _ = ready_tx.send(());
        });
        assert!(
            !server.agent_state_ingress_unavailable(),
            "an ingress that bound must never be reported as a failure"
        );
        assert!(
            ready_rx.try_recv().is_ok(),
            "readiness must still be signalled when the ingress bound"
        );
    }

    #[test]
    fn agent_state_rendezvous_record_sits_beside_the_socket() {
        // The non-unix ingress publishes its loopback port and bearer token to this one record,
        // and `apply_agent_state_tcp_env` reads exactly it before a pane spawns.
        let socket = get_agent_state_socket_path();
        let runtime_dir = socket.parent().expect("agent state socket has a parent");
        assert_eq!(
            get_agent_state_rendezvous_path(),
            runtime_dir.join(AGENT_STATE_RENDEZVOUS_FILE)
        );
    }

    #[test]
    fn rendezvous_publish_writes_the_port_and_the_token_as_one_file() {
        let runtime_dir =
            std::env::temp_dir().join(format!("ferryx-rendezvous-{}", std::process::id()));
        fs::create_dir_all(&runtime_dir).expect("create rendezvous dir");
        clear_stale_agent_state_rendezvous(&runtime_dir);

        publish_agent_state_rendezvous(&runtime_dir, 41_234, "token-abc")
            .expect("publish rendezvous");
        let record = runtime_dir.join(AGENT_STATE_RENDEZVOUS_FILE);
        assert_eq!(
            fs::read_to_string(&record).expect("read published rendezvous"),
            "41234\ntoken-abc\n",
            "one record must carry both the port and the token"
        );
        // The pair exists under a single name, and the staged file the rename consumed is gone.
        assert!(!runtime_dir.join("agent-state.port").exists());
        assert!(!runtime_dir.join("agent-state.token").exists());
        assert!(!runtime_dir.join(format!("{AGENT_STATE_RENDEZVOUS_FILE}.tmp")).exists());

        // A later boot replaces the record in place rather than leaving two halves behind.
        publish_agent_state_rendezvous(&runtime_dir, 41_235, "token-def")
            .expect("republish rendezvous");
        assert_eq!(
            fs::read_to_string(&record).expect("read republished rendezvous"),
            "41235\ntoken-def\n"
        );

        // Clearing drops the record and the two-file pair older builds published.
        fs::write(runtime_dir.join("agent-state.port"), "41234").expect("stage legacy port");
        fs::write(runtime_dir.join("agent-state.token"), "token-abc")
            .expect("stage legacy token");
        clear_stale_agent_state_rendezvous(&runtime_dir);
        assert!(!record.exists());
        assert!(!runtime_dir.join("agent-state.port").exists());
        assert!(!runtime_dir.join("agent-state.token").exists());

        fs::remove_dir_all(&runtime_dir).ok();
    }
}

#[cfg(test)]
mod a05_compatibility_tests {
    use super::*;

    #[test]
    fn catalog_constructor_child() {
        let Some(root) = std::env::var_os("A05_CONSTRUCTOR_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let sentinel = root.join("data/remote/machine-workspaces.v1.json");
        let before = fs::read(&sentinel).unwrap();
        let first = DaemonServer::new();
        let second = DaemonServer::new_with_paths(None, None);
        assert!(
            first.session_service.workspace_service.catalog().is_ok(),
            "constructor read canonical sentinel"
        );
        let first_dir = first
            .session_service
            .remote_sessions_path
            .parent()
            .unwrap()
            .to_owned();
        let second_dir = second
            .session_service
            .remote_sessions_path
            .parent()
            .unwrap()
            .to_owned();
        assert_ne!(first_dir, second_dir);
        for (server, name) in [(&first, "one"), (&second, "two")] {
            let plain = root.join(name);
            fs::create_dir(&plain).unwrap();
            server
                .handle_register_workspace("same-id", plain.to_str().unwrap())
                .unwrap();
            assert_eq!(
                server.workspace_registry.repo_root("same-id").unwrap(),
                fs::canonicalize(plain).unwrap()
            );
        }
        assert_eq!(fs::read(&sentinel).unwrap(), before);
        drop(first);
        drop(second);
        assert!(!first_dir.exists());
        assert!(!second_dir.exists());
        eprintln!(
            "CONSTRUCTOR cleanup first={} second={} absent=true sentinel_unchanged=true",
            first_dir.display(),
            second_dir.display()
        );
    }

    #[tokio::test]
    async fn catalog_constructor_isolation() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("data/remote")).unwrap();
        fs::write(
            root.path().join("data/remote/machine-workspaces.v1.json"),
            b"private canonical sentinel",
        )
        .unwrap();
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "daemon::server::a05_compatibility_tests::catalog_constructor_child",
                "--nocapture",
            ])
            .env("A05_CONSTRUCTOR_ROOT", root.path())
            .env("FERRYX_DATA_DIR", root.path().join("data"))
            .env("FERRYX_RUNTIME_DIR", root.path().join("runtime"))
            .env("HOME", root.path())
            .env("TMPDIR", root.path())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let status = match tokio::time::timeout(Duration::from_secs(20), child.wait()).await {
            Ok(status) => status.unwrap(),
            Err(_) => {
                child.start_kill().unwrap();
                child.wait().await.unwrap()
            }
        };
        let receipt = root.path().to_owned();
        root.close().unwrap();
        eprintln!(
            "CONSTRUCTOR owner pid={pid} reaped=true root={} absent={}",
            receipt.display(),
            !receipt.exists()
        );
        assert!(status.success());
    }

    #[tokio::test]
    async fn catalog_ssh_unregister_compatibility() {
        let root = tempfile::tempdir().unwrap();
        let server = DaemonServer::new_with_paths(
            Some(root.path().join("data/config")),
            Some(root.path().join("data/auth")),
        );
        let id = "ssh:isolated-fixture";
        let metadata = serde_json::from_value(serde_json::json!({
            "client_request_id": "fixture", "workspace_id": id, "worktree": null,
            "cwd": root.path(), "provider_claim": null,
            "spawn_fingerprint": {"workspace_id": id, "worktree": null, "cwd": null,
                "cols": 80, "rows": 24, "shell": null, "provider_claim": null, "startup": null}
        }))
        .unwrap();
        server
            .session_metadata
            .write()
            .insert("expired-ssh-session".into(), metadata);
        let result = server.handle_unregister_workspace(id).await;
        let cleaned = !server
            .session_metadata
            .read()
            .contains_key("expired-ssh-session");
        assert!(server
            .handle_unregister_workspace("daemon:desktop")
            .await
            .is_err());
        assert!(!root.path().join("data/machine-workspaces.v1.json").exists());
        drop(server);
        let receipt = root.path().to_owned();
        root.close().unwrap();
        eprintln!(
            "SSH cleanup root={} absent={} no_hosts_contacted=true ownership_released={cleaned}",
            receipt.display(),
            !receipt.exists()
        );
        assert!(result.is_ok(), "SSH unregister rejected: {result:?}");
        assert!(cleaned);
    }
}

#[cfg(all(test, unix))]
#[path = "../paired_host/process_tests.rs"]
mod paired_host_process_tests;

#[cfg(all(test, unix))]
#[path = "../paired_host/native_operation_tests.rs"]
mod paired_host_operation_tests;

#[cfg(all(test, unix))]
mod paired_host_native_tests {
    use super::*;
    use crate::paired_host::{
        inventory::{AuthStatus, MigrationReceipt},
        service::{PairRequest, Secret},
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_daemon_relay_inventory_roundtrip() {
        let root = tempfile::tempdir().unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(60), exercise(root.path())).await;
        let path = root.path().to_owned();
        root.close().unwrap();
        eprintln!(
            "A13 cleanup private_root={} absent={}",
            path.display(),
            !path.exists()
        );
        outcome.unwrap().unwrap();
    }
    struct GatewayGuard(Arc<DaemonServer>);
    impl Drop for GatewayGuard {
        fn drop(&mut self) {
            if let Some(handle) = self.0.remote_server_handle.lock().take() {
                handle.stop();
            }
        }
    }
    async fn exercise(root: &Path) -> anyhow::Result<()> {
        use anyhow::{ensure, Context};
        let data = root.join("data");
        fs::create_dir_all(&data)?;
        let mut daemon =
            DaemonServer::new_with_paths(Some(data.join("config")), Some(data.join("auth")));
        daemon.paired_hosts =
            crate::paired_host::service::PairedHostService::open_test_loopback(data.clone());
        let daemon = Arc::new(daemon);
        let _gateway_guard = GatewayGuard(Arc::clone(&daemon));
        let socket = root.join("native.sock");
        let listener = tokio::net::UnixListener::bind(&socket)?;
        let owner = Arc::clone(&daemon);
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let owner = Arc::clone(&owner);
                        clients.spawn(async move { owner.handle_client(stream).await });
                    }
                    Err(error) => return Err(error),
                }
            }
        });
        let relay_state = crate::remote::relay_server::RelayState::new_with_key_store(
            vec![],
            root.join("relay-keys.json"),
        )
        .map_err(anyhow::Error::msg)?;
        let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", relay_listener.local_addr()?);
        tasks.spawn(async move {
            axum::serve(
                relay_listener,
                crate::remote::relay_server::relay_router(relay_state)
                    .into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
        });
        let result = async {
            daemon.configure_gateway(RemoteGatewayConfig { mode: RemoteNetworkMode::Relay, port: 0,
                relay_url: Some(origin.clone()), ..Default::default() }).await.map_err(anyhow::Error::msg)?;
            let client = crate::daemon::client::DaemonClient::new_with_socket(socket.clone());
            ensure!(client.paired_host_list().await.map_err(|e| anyhow::anyhow!(e.code))?.is_empty(), "new inventory not empty");
            let coordinator = daemon.remote_state.relay_pairing.read().as_ref()
                .map(|p| p.coordinator.clone()).context("published relay coordinator")?;
            let issue = |scope| {
                let coordinator = coordinator.clone();
                async move {
                    // The existing issuer does not receive relay-consumed notifications.
                    // Explicitly expire its completed PIN before the next issuance.
                    if coordinator.state() == crate::remote::protocol::PairingState::Ready {
                        coordinator.transition(crate::remote::protocol::PairingState::Expired)?;
                    }
                    coordinator.generate_scoped_pairing(Duration::from_secs(60), DevicePermission::Control, scope).await
                }
            };
            let first = issue(crate::remote::auth::DeviceAccessScope::Machine).await.map_err(anyhow::Error::msg)?;
            let host = client.paired_host_pair(PairRequest { relay_origin: origin.clone(), pin: Secret(first.pin), display_label: "Fixture host".into() })
                .await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(host.generation.0 == 1 && host.auth_status == AuthStatus::Paired, "first adoption mismatch");
            // An independent gateway on the same relay establishes real credential isolation.
            let other_data = root.join("other");
            fs::create_dir_all(&other_data)?;
            let other = Arc::new(DaemonServer::new_with_paths(Some(other_data.join("config")), Some(other_data.join("auth"))));
            let _other_guard = GatewayGuard(Arc::clone(&other));
            other.configure_gateway(RemoteGatewayConfig { mode: RemoteNetworkMode::Relay, port: 0,
                relay_url: Some(origin.clone()), ..Default::default() }).await.map_err(anyhow::Error::msg)?;
            let other_coordinator = other.remote_state.relay_pairing.read().as_ref().map(|p| p.coordinator.clone()).context("other coordinator")?;
            let other_pin = other_coordinator.generate_scoped_pairing(Duration::from_secs(60), DevicePermission::Control,
                crate::remote::auth::DeviceAccessScope::Machine).await.map_err(anyhow::Error::msg)?;
            let other_host = client.paired_host_pair(PairRequest { relay_origin: origin.clone(), pin: Secret(other_pin.pin), display_label: "Other host".into() })
                .await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(other_host.host_id != host.host_id, "gateway identities aliased");
            let other_lease = daemon.paired_hosts.test_capture(other_host.host_id.clone(), other_host.generation).await.map_err(|e| anyhow::anyhow!(e.code))?;
            client.paired_host_forget(other_host.host_id.clone(), other_host.generation).await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(other_lease.token().is_err(), "other forget did not cancel its lease");
            ensure!(client.paired_host_read(MigrationReceipt { host_id: host.host_id.clone(), generation: host.generation }).await.map_err(|e| anyhow::anyhow!(e.code))? == host, "cross-host forget mutated first host");
            ensure!(!other.remote_state.auth_manager.list_devices().is_empty(), "other forget revoked remote grant");
            let lease = daemon.paired_hosts.test_capture(host.host_id.clone(), host.generation).await.map_err(|e| anyhow::anyhow!(e.code))?;
            let mut cancelled = lease.cancellation();
            let token = lease.token()?.to_owned();
            let migrated = client.paired_host_migrate_legacy(crate::paired_host::service::MigrationRequest {
                relay_origin: origin.clone(), machine_id: host.machine_id.clone(), display_label: host.display_label.clone(), device_token: Secret(token.clone()),
            }).await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(migrated.generation == host.generation && migrated.host_id == host.host_id, "migration retry changed authority");
            ensure!(client.paired_host_read(migrated).await.map_err(|e| anyhow::anyhow!(e.code))? == host, "migration durable receipt failed");
            let serialized = serde_json::to_string(&host)?;
            ensure!(!serialized.contains(&token) && !serialized.contains("deviceToken"), "native projection leaked credential");
            use std::os::unix::fs::PermissionsExt;
            ensure!(fs::metadata(data.join("paired-hosts.v1.json"))?.permissions().mode() & 0o777 == 0o600, "credential file not private");
            ensure!(fs::metadata(&data)?.permissions().mode() & 0o777 == 0o700, "credential directory not private");
            let second = issue(crate::remote::auth::DeviceAccessScope::Machine).await.map_err(anyhow::Error::msg)?;
            let repaired = client.paired_host_pair(PairRequest { relay_origin: origin.clone(), pin: Secret(second.pin), display_label: "Fixture host".into() })
                .await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(repaired.generation.0 == 2 && repaired.host_id == host.host_id, "re-pair identity/generation mismatch");
            cancelled.changed().await?;
            ensure!(*cancelled.borrow() && lease.token().is_err(), "lease not cancelled");
            ensure!(client.paired_host_forget(host.host_id.clone(), host.generation).await.unwrap_err().code == "PAIRED_HOST_STALE_GENERATION", "stale forget admitted");
            let receipt = MigrationReceipt { host_id: repaired.host_id.clone(), generation: repaired.generation };
            ensure!(client.paired_host_read(receipt).await.map_err(|e| anyhow::anyhow!(e.code))? == repaired, "durable readback mismatch");
            let restart = crate::paired_host::service::PairedHostService::open_test_loopback(data.clone());
            ensure!(restart.list().await.map_err(|e| anyhow::anyhow!(e.code))? == vec![repaired.clone()], "restart persistence mismatch");
            drop(restart);
            let migration = crate::paired_host::service::MigrationRequest { relay_origin: origin.clone(), machine_id: repaired.machine_id.clone(), display_label: repaired.display_label.clone(), device_token: Secret(token.clone()) };
            ensure!(client.paired_host_migrate_legacy(migration).await.unwrap_err().code == "PAIRED_HOST_MIGRATION_PENDING", "old credential replaced re-pair");
            ensure!(client.paired_host_list().await.map_err(|e| anyhow::anyhow!(e.code))? == vec![repaired.clone()], "failed migration damaged inventory");
            let mirror = issue(crate::remote::auth::DeviceAccessScope::Mirror).await.map_err(anyhow::Error::msg)?;
            let mirrored = client.paired_host_pair(PairRequest { relay_origin: origin.clone(), pin: Secret(mirror.pin), display_label: "Fixture host".into() })
                .await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(mirrored.generation.0 == 3 && mirrored.auth_status == AuthStatus::NeedsMachineGrant, "mirror elevated");
            client.paired_host_forget(mirrored.host_id, mirrored.generation).await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(client.paired_host_list().await.map_err(|e| anyhow::anyhow!(e.code))?.is_empty(), "forget retained local host");
            ensure!(!daemon.remote_state.auth_manager.list_devices().is_empty(), "forget revoked remote devices");
            eprintln!("A13 real_uds=true relay_pair_exchange=true scope_negotiation=true re_pair=1,2,3 stale_forget=rejected lease_cancelled=true durable_readback=true restart=true failed_migration_preserved=true local_forget=true cross_host_isolation=true modes=700,600");
            Ok::<_, anyhow::Error>(())
        }.await;
        if let Some(handle) = daemon.remote_server_handle.lock().take() {
            handle.stop();
        }
        tasks.shutdown().await;
        drop(daemon);
        fs::remove_file(socket)?;
        result
    }
}

#[cfg(all(test, unix))]
#[path = "a03_owner_cli_fixture.rs"]
mod a03_owner_cli_fixture;

#[cfg(all(test, unix))]
#[path = "a04_shared_services_tests.rs"]
mod a04_shared_services_tests;

impl std::ops::Deref for DaemonServer {
    type Target = DaemonSessionService;
    fn deref(&self) -> &Self::Target {
        &self.session_service
    }
}

impl Default for DaemonServer {
    fn default() -> Self {
        Self::new()
    }
}

fn daemon_error(message: impl ToString) -> DaemonResponse {
    DaemonResponse::Error {
        message: message.to_string(),
        code: None,
        details: None,
    }
}

fn daemon_session_not_found(session_id: &str, source: &'static str) -> DaemonResponse {
    DaemonResponse::Error {
        message: format!("Session '{session_id}' not found"),
        code: Some("SESSION_NOT_FOUND".to_string()),
        details: Some(serde_json::json!({
            "source": source,
            "kind": "session_not_found",
            "sessionId": session_id,
        })),
    }
}

impl DaemonServer {
    pub fn new() -> Self {
        // Headless CLI constructs synchronously inside its multi-thread runtime.
        // Yield that executor worker while startup waits for catalog restoration.
        if tokio::runtime::Handle::try_current().is_ok_and(|handle| {
            handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread
        }) {
            return tokio::task::block_in_place(|| Self::new_with_paths(None, None));
        }
        Self::new_with_paths(None, None)
    }

    pub fn new_with_paths(config_path: Option<PathBuf>, auth_path: Option<PathBuf>) -> Self {
        // No-path unit constructors must never share or inspect owner state.
        // Explicit paths keep their persistent restart semantics.
        #[cfg(test)]
        let catalog_fixture = config_path
            .is_none()
            .then(|| tempfile::tempdir().expect("private test catalog"));
        let isolated_dir = config_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf);
        #[cfg(test)]
        let isolated_dir =
            isolated_dir.or_else(|| catalog_fixture.as_ref().map(|dir| dir.path().to_owned()));
        let paired_hosts = crate::paired_host::service::PairedHostService::open(
            isolated_dir.clone().unwrap_or_else(|| {
                crate::remote::auth::canonical_identity_dir()
                    .expect("daemon requires a private data directory")
            }),
        );
        let pty_manager = Arc::new(PtyManager::new());
        let output_hub = Arc::new(TerminalOutputHub::default());
        let terminal_service = Arc::new(TerminalService::new(
            Arc::clone(&pty_manager),
            Arc::clone(&output_hub),
        ));
        let session_router = Arc::new(crate::daemon::proxy::SessionRouter::new(Arc::clone(
            &terminal_service,
        )));
        let workspace_registry = WorkspaceRegistry::new();
        let handover_manager = Arc::new(crate::daemon::handover::HandoverManager::new(
            get_socket_path(),
        ));
        let remote_event_tx = broadcast::channel::<DaemonRemoteEvent>(64).0;
        let catalog_path = isolated_dir
            .clone()
            .unwrap_or_else(|| {
                crate::remote::auth::canonical_identity_dir()
                    .expect("daemon requires a private data directory")
            })
            .join("machine-workspaces.v1.json");
        // Keep restore filesystem/Git work off executor threads, and join before
        // building the gateway or advertising readiness.
        let workspace_service = std::thread::scope(|scope| {
            let registry = workspace_registry.clone();
            scope
                .spawn(move || {
                    Arc::new(super::workspace_service::DaemonWorkspaceService::new(
                        registry,
                        catalog_path,
                    ))
                })
                .join()
                .expect("workspace catalog initialization panicked")
        });
        let session_service = Arc::new(DaemonSessionService {
            workspace_service,
            terminal_service: Arc::clone(&terminal_service),
            session_router: Arc::clone(&session_router),
            handover_manager: Arc::downgrade(&handover_manager),
            remote_event_tx: remote_event_tx.clone(),
            agent_states: Arc::new(AgentStateHub::default()),
            spawn_idempotency_cache: Arc::new(Mutex::new(HashMap::new())),
            spawn_lock: Arc::new(tokio::sync::Mutex::new(())),
            machine_controllers: tokio::sync::Mutex::new(HashMap::new()),
            machine_lifecycles: Arc::new(Mutex::new(HashMap::new())),
            remote_persistence_lock: Arc::new(tokio::sync::Mutex::new(())),
            // The runtime dir (/tmp/rorca-{uid}) is wiped on reboot, which erased every
            // persisted remote descriptor even though the host-side helper daemons survived.
            // Durable remote identity belongs next to the machine identity.
            remote_sessions_path: isolated_dir
                .clone()
                .or_else(session_dir_override)
                .unwrap_or_else(|| {
                    crate::remote::auth::canonical_identity_dir()
                        .expect("daemon requires a private data directory")
                })
                .join("remote_sessions.json"),
            ssh_store_path: isolated_dir
                .clone()
                .map(|p| p.join("ssh_hosts.json"))
                .unwrap_or_else(daemon_ssh_store_path),
            session_metadata: Arc::new(RwLock::new(HashMap::new())),
            provider_session_claims: Arc::new(Mutex::new(HashMap::new())),
        });
        #[cfg(test)]
        let remote_state = Arc::new(RemoteGatewayState::new_with_paths_and_service(
            Arc::clone(&session_router) as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
            Arc::clone(&terminal_service),
            workspace_registry.clone(),
            config_path,
            auth_path,
        ));
        #[cfg(not(test))]
        let remote_state = if config_path.is_some() || auth_path.is_some() {
            Arc::new(RemoteGatewayState::new_with_paths_and_service(
                Arc::clone(&session_router)
                    as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
                Arc::clone(&terminal_service),
                workspace_registry.clone(),
                config_path,
                auth_path,
            ))
        } else {
            Arc::new(RemoteGatewayState::new_persistent_with_service(
                Arc::clone(&session_router)
                    as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
                Arc::clone(&terminal_service),
                workspace_registry.clone(),
            ))
        };

        let remote_state = Arc::new(
            Arc::try_unwrap(remote_state)
                .unwrap_or_else(|_| unreachable!("gateway not published yet"))
                .with_machine_services(Arc::clone(&session_service)),
        );

        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(1);

        let (binary_path, binary_mtime_ms) = resolve_binary_identity();
        remote_state
            .daemon_epoch
            .store(epoch, std::sync::atomic::Ordering::Release);

        // The gateway runs inside this process, so desktop-directed events must
        // be relayed to the GUI over the socket; without this sink they are
        // dropped and remote-issued selections never reach the desktop.
        let sink_tx = remote_event_tx.clone();
        remote_state.set_desktop_event_sink(Arc::new(move |event, payload| {
            let _ = sink_tx.send(DaemonRemoteEvent {
                event: event.to_string(),
                payload,
            });
        }));

        // P14: inventory transitions that originate inside the daemon-owned
        // PairedHostService (not just renderer-driven pair/migrate/forget)
        // must reach the desktop. The sink feeds the daemon's remote-event
        // broadcast; the GUI's remote event bridge (lib.rs
        // start_remote_event_bridge) re-emits it under the same
        // paired_host_inventory_changed Tauri event name the renderer already
        // listens for.
        let mut paired_hosts = paired_hosts;
        {
            let inventory_sink_tx = remote_event_tx.clone();
            paired_hosts.set_event_sink(Arc::new(move |event| {
                let payload = serde_json::to_value(&event).unwrap_or(serde_json::Value::Null);
                let _ = inventory_sink_tx.send(DaemonRemoteEvent {
                    event: crate::ipc::paired_host::PAIRED_HOST_INVENTORY_CHANGED_EVENT.to_string(),
                    payload,
                });
            }));
        }

        let remote_handle_for_handover: Arc<
            Mutex<Option<crate::remote::server::RemoteServerHandle>>,
        > = Arc::new(Mutex::new(None));
        {
            let remote_state_cb = Arc::clone(&remote_state);
            let remote_handle_cb = Arc::clone(&remote_handle_for_handover);
            handover_manager.on_commit(move || {
                tracing::info!("Handover committed: tearing down legacy Remote Gateway listener and clearing active selection");
                let prev_handle = remote_handle_cb.lock().take();
                if let Some(handle) = prev_handle {
                    handle.stop();
                }
                remote_state_cb.clear_active_selection();
                *remote_state_cb.is_running.write() = false;
                *remote_state_cb.bound_address.write() = None;
            });
        }

        Self {
            paired_hosts,
            session_router,
            handover_manager,
            terminal_service,
            workspace_registry,
            remote_state,
            remote_event_tx,
            remote_server_handle: remote_handle_for_handover,
            epoch,
            binary_path,
            binary_mtime_ms,
            session_service,
            agent_state_endpoint: Mutex::new(None),
            agent_state_ingress_unavailable: std::sync::atomic::AtomicBool::new(false),
            #[cfg(not(unix))]
            transport_token: generate_transport_token(),
            #[cfg(test)]
            helper_home: isolated_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            #[cfg(test)]
            _catalog_fixture: catalog_fixture,
        }
    }

    /// Installs a loopback-permitting paired inventory for isolated fixtures.
    /// Production pairing always uses the HTTPS-only policy.
    #[cfg(test)]
    pub fn set_paired_hosts_for_test(
        &mut self,
        service: crate::paired_host::service::PairedHostService,
    ) {
        self.paired_hosts = service;
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
        self.session_service
            .handle_spawn(
                client_request_id,
                workspace_id,
                worktree,
                cwd,
                cols,
                rows,
                shell,
                startup,
                #[cfg(test)]
                self.helper_home.clone(),
            )
            .await
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

    /// The loopback TCP endpoint (`port`, bearer `token`) the agent-state ingress bound, or
    /// `None` while it has not bound. Unix exposes the socket path through
    /// [`agent_state_socket_path`] instead.
    pub fn agent_state_endpoint(&self) -> Option<(u16, String)> {
        self.agent_state_endpoint.lock().clone()
    }

    /// True once this boot's agent-state ingress was observed to fail, so an absent endpoint is
    /// reported as a failure instead of being inferred from a `None` that also means "not yet".
    pub fn agent_state_ingress_unavailable(&self) -> bool {
        self.agent_state_ingress_unavailable
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Observes the agent-state ingress outcome, records it, and only then releases the readiness
    /// signal. That order is the contract: a client released by the signal can spawn a pane
    /// immediately, and the pane reads its agent-state endpoint as it spawns, so recording the
    /// outcome afterwards would announce a boot whose panes already lost the ingress.
    ///
    /// A missing ingress is recorded and warned about, never fatal. Terminals are the product and
    /// must keep working; agent-state reporting is only an observability ingress, so the
    /// degradation has to be visible and reportable rather than turned into a refused boot.
    fn settle_agent_state_ingress<S: FnOnce()>(
        &self,
        ingress: Option<tokio::task::JoinHandle<()>>,
        signal_ready: S,
    ) {
        // The handle is deliberately dropped rather than awaited: the ingress task, when there is
        // one, serves this boot for as long as it runs.
        if ingress.is_none() {
            self.agent_state_ingress_unavailable
                .store(true, std::sync::atomic::Ordering::Release);
            tracing::warn!(
                "Agent-state ingress unavailable for this boot: no agent-state endpoint was \
                 bound or published, so panes will not receive one and agent-state reporting is \
                 unavailable for their lifetime; terminals are unaffected"
            );
        }
        signal_ready();
    }

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
    pub fn spawn_agent_state_listener(self: &Arc<Self>) -> Option<tokio::task::JoinHandle<()>> {
        let path = get_agent_state_socket_path();
        let _ = fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "Failed to bind agent state socket");
                return None;
            }
        };
        if let Err(error) = fs::set_permissions(&path, fs::Permissions::from_mode(0o600)) {
            tracing::warn!(%error, "Failed to secure agent state socket");
        }

        let states = Arc::clone(&self.agent_states);
        let sessions = self.session_service.clone();
        let epoch = crate::scoped_contracts::Epoch(self.epoch);
        Some(tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let states = Arc::clone(&states);
                let sessions = sessions.clone();
                while clients.try_join_next().is_some() {}
                clients.spawn(async move {
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                        if let Some(report) = Self::parse_agent_state_report(&line) {
                            match sessions.machine_detail_routed(&report.0, epoch).await {
                                Ok(crate::remote::machine_protocol::SessionDetail::Running {
                                    session,
                                }) => {
                                    let hint = AgentStateReport {
                                        session_id: report.0.clone(),
                                        state: report.1.clone(),
                                        agent: report.2.clone(),
                                        provider_session: report.3.clone(),
                                    };
                                    if let Err(error) = sessions
                                        .validate_machine_agent_report(session.target, hint)
                                        .await
                                    {
                                        tracing::debug!(%error, "Machine agent metadata rejected");
                                        line.clear();
                                        continue;
                                    }
                                }
                                Ok(_) => {
                                    line.clear();
                                    continue;
                                }
                                Err(error) if error == "SESSION_NOT_FOUND" => {}
                                Err(error) => {
                                    tracing::debug!(%error, "Machine agent owner unavailable");
                                    line.clear();
                                    continue;
                                }
                            }
                            states.publish_canonical(AgentState {
                                session_id: report.0,
                                state: report.1,
                                agent: report.2,
                                provider_session: report.3,
                                origin: crate::daemon::protocol::AgentStateOrigin::Agent,
                            });
                        }
                        line.clear();
                    }
                });
            }
        }))
    }

    /// Windows/Linux counterpart of the unix socket ingress: a loopback TCP listener the bundled
    /// extension reaches through `FERRYX_AGENT_STATE_PORT`/`FERRYX_AGENT_STATE_TOKEN`, the same
    /// TCP mode remote sessions already use. Any local process can connect to a loopback port,
    /// so a report is accepted only when it carries this boot's token.
    #[cfg(not(unix))]
    pub fn spawn_agent_state_listener(self: &Arc<Self>) -> Option<tokio::task::JoinHandle<()>> {
        // The previous boot's record is dropped before this boot publishes: a listener that fails
        // to bind below must not leave the old boot's endpoint looking valid. The reported
        // endpoint is dropped with it, so no failure path below can leave this boot reporting an
        // endpoint while nothing is discoverable on disk.
        clear_stale_agent_state_rendezvous(&get_runtime_dir());
        *self.agent_state_endpoint.lock() = None;
        let bound = match std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)) {
            Ok(bound) => bound,
            Err(error) => {
                tracing::warn!(%error, "Failed to bind agent state listener");
                return None;
            }
        };
        if let Err(error) = bound.set_nonblocking(true) {
            tracing::warn!(%error, "Failed to configure agent state listener");
            return None;
        }
        let port = match bound.local_addr() {
            Ok(address) => address.port(),
            Err(error) => {
                tracing::warn!(%error, "Failed to read agent state listener address");
                return None;
            }
        };
        let listener = match TcpListener::from_std(bound) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "Failed to register agent state listener");
                return None;
            }
        };
        let token = generate_transport_token();
        // A pane reads the endpoint out of this record as it spawns, so it must be on disk before
        // the accept loop admits a connection. Port and token travel as one renamed file, so a
        // reader can never observe one half of a pair, and the endpoint is recorded only once that
        // file exists.
        if let Err(error) = publish_agent_state_endpoint(
            &self.agent_state_endpoint,
            &get_runtime_dir(),
            port,
            &token,
        ) {
            tracing::warn!(%error, "Failed to publish agent state rendezvous");
            return None;
        }
        tracing::info!(port, "Agent state ingress listening on loopback");

        let states = Arc::clone(&self.agent_states);
        let sessions = self.session_service.clone();
        let epoch = crate::scoped_contracts::Epoch(self.epoch);
        Some(tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let states = Arc::clone(&states);
                let sessions = sessions.clone();
                let token = token.clone();
                while clients.try_join_next().is_some() {}
                clients.spawn(async move {
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                        let presented = transport_token_from_line(&line);
                        if !transport_token_matches(&token, presented.as_deref()) {
                            tracing::debug!(
                                "Agent state report rejected: token missing or invalid"
                            );
                            line.clear();
                            continue;
                        }
                        if let Some(report) = Self::parse_agent_state_report(&line) {
                            match sessions.machine_detail_routed(&report.0, epoch).await {
                                Ok(crate::remote::machine_protocol::SessionDetail::Running {
                                    session,
                                }) => {
                                    let hint = AgentStateReport {
                                        session_id: report.0.clone(),
                                        state: report.1.clone(),
                                        agent: report.2.clone(),
                                        provider_session: report.3.clone(),
                                    };
                                    if let Err(error) = sessions
                                        .validate_machine_agent_report(session.target, hint)
                                        .await
                                    {
                                        tracing::debug!(%error, "Machine agent metadata rejected");
                                        line.clear();
                                        continue;
                                    }
                                }
                                Ok(_) => {
                                    line.clear();
                                    continue;
                                }
                                Err(error) if error == "SESSION_NOT_FOUND" => {}
                                Err(error) => {
                                    tracing::debug!(%error, "Machine agent owner unavailable");
                                    line.clear();
                                    continue;
                                }
                            }
                            states.publish_canonical(AgentState {
                                session_id: report.0,
                                state: report.1,
                                agent: report.2,
                                provider_session: report.3,
                                origin: crate::daemon::protocol::AgentStateOrigin::Agent,
                            });
                        }
                        line.clear();
                    }
                });
            }
        }))
    }

    fn spawn_foreground_observer(self: &Arc<Self>) {
        let server = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut transitions =
                HashMap::<String, crate::terminal::foreground::ProcessTransition>::new();
            // Inspect even without output. This cadence only collects positive process
            // evidence; neither elapsed time nor silence can release activity.
            let mut sample = tokio::time::interval(std::time::Duration::from_millis(250));
            sample.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                sample.tick().await;
                let Some(server) = server.upgrade() else {
                    break;
                };
                let sessions: Vec<_> = server
                    .terminal_service
                    .list_sessions()
                    .into_iter()
                    .filter_map(|id| {
                        server
                            .terminal_service
                            .get_session(&id)
                            .map(|session| (id, session))
                    })
                    .collect();
                transitions.retain(|id, _| sessions.iter().any(|(live, _)| live == id));
                let observations = crate::ipc::run_blocking(move || {
                    // One process listing per tick. On Windows that listing is a full
                    // PowerShell enumeration, so capturing it per session multiplied the
                    // cost by the pane count; every session in this tick is classified
                    // against the same sample. A failed capture is unknown ownership for
                    // all of them, which is the same evidence a failed per-session
                    // inspection already produced.
                    let snapshot = crate::terminal::foreground::capture_process_snapshot();
                    Ok(sessions
                        .into_iter()
                        .map(|(id, session)| {
                            let observation = match &snapshot {
                                Ok(snapshot) => {
                                    crate::terminal::foreground::inspect_with_snapshot(
                                        &session, snapshot,
                                    )
                                }
                                Err(error) => Err(std::io::Error::new(
                                    error.kind(),
                                    error.to_string(),
                                )),
                            };
                            (id, observation)
                        })
                        .collect::<Vec<_>>())
                })
                .await;
                match observations {
                    Ok(observations) => {
                        for (id, observation) in observations {
                            match observation {
                                Ok(observation) => {
                                    let edge = transitions
                                        .entry(id.clone())
                                        .or_default()
                                        .observe(observation);
                                    if server.terminal_service.get_session(&id).is_some() {
                                        match edge {
                                        Some(crate::terminal::foreground::AgentProcessEdge::Released) => {
                                            server.agent_states.release_foreground(&id);
                                        }
                                        Some(crate::terminal::foreground::AgentProcessEdge::Observed) => {
                                            server.agent_states.observe_foreground_agent(&id);
                                        }
                                        None => {}
                                    }
                                    }
                                }
                                Err(error) => {
                                    tracing::debug!(session_id = id, %error, "foreground inspection unavailable; holding state")
                                }
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "foreground observer failed; holding state")
                    }
                }
            }
        });
    }

    pub async fn run_server(self: Arc<Self>) -> Result<(), String> {
        self.run_server_with_handover_and_readiness(None, None)
            .await
    }

    pub async fn run_server_with_handover(
        self: Arc<Self>,
        handover_from: Option<PathBuf>,
    ) -> Result<(), String> {
        self.run_server_with_handover_and_readiness(handover_from, None)
            .await
    }

    pub async fn run_server_with_handover_and_readiness(
        self: Arc<Self>,
        handover_from: Option<PathBuf>,
        ready_tx: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> Result<(), String> {
        let runtime_dir = get_runtime_dir();
        ensure_runtime_directory(&runtime_dir)?;

        let socket_path = get_socket_path();
        let lock_path = get_lock_path();

        if let Some(ref legacy_path) = handover_from {
            tracing::info!(
                "Starting daemon with handover from {}",
                legacy_path.display()
            );
            // Strictly validate that legacy socket is in the expected runtime directory,
            // owned by current user, mode 0700 parent directory, and not a symlink.
            validate_runtime_socket_path(legacy_path)?;
            // Only unix can be handed sessions: `spawn_legacy_handover_daemon` and the
            // `--handover-from` producer are unix-only, so the peer has no other consumer.
            #[cfg(unix)]
            let legacy_peer = Arc::new(crate::daemon::proxy::LegacyPeer::new(
                legacy_path.clone(),
                Vec::new(),
            ));

            #[cfg(unix)]
            if crate::daemon::handover::is_v5_ownership_transfer_enabled() {
                let transfer_id = uuid::Uuid::new_v4().to_string();
                let handover_socket_path = crate::daemon::handover_socket::get_handover_socket_path(&transfer_id);
                let listener = crate::daemon::handover_socket::HandoverSocketListener::bind(&handover_socket_path)
                    .map_err(|e| format!("Failed to bind handover socket: {e}"))?;

                let accept_handle = tokio::task::spawn_blocking(move || {
                    let (stream, _creds) = listener.accept()?;
                    let mut exports = Vec::new();
                    while let Ok(Some(export)) = crate::daemon::handover_socket::recv_session(&stream) {
                        exports.push(export);
                    }
                    Ok::<_, crate::daemon::handover_socket::HandoverSocketError>(exports)
                });

                let transfer_resp = legacy_peer
                    .send_request(&DaemonRequest::TransferSessions {
                        handover_socket_path: handover_socket_path.to_string_lossy().into_owned(),
                    })
                    .await
                    .map_err(|e| format!("TransferSessions request failed: {e}"))?;

                if !matches!(transfer_resp, DaemonResponse::TransferSessionsOk { .. }) {
                    return Err(format!("TransferSessions failed: {transfer_resp:?}"));
                }

                let exports = accept_handle
                    .await
                    .map_err(|e| format!("Handover worker panicked: {e}"))?
                    .map_err(|e| format!("Handover socket receive error: {e}"))?;

                for export in exports {
                    tracing::info!(session_id = %export.session_id, "Adopting transferred session from predecessor");
                    let (master, snapshot) = export.into_parts();
                    self.terminal_service
                        .pty_manager()
                        .adopt_transferred_session(master, snapshot)
                        .map_err(|e| format!("Failed to adopt transferred session: {e}"))?;
                }

                let commit_resp = legacy_peer
                    .send_request(&DaemonRequest::CommitHandover {
                        legacy_socket_path: None,
                    })
                    .await?;
                if !matches!(commit_resp, DaemonResponse::CommitHandoverOk) {
                    return Err(format!("CommitHandover failed: {commit_resp:?}"));
                }
            } else {
                let sessions = legacy_peer.list_sessions().await?;
                let route = crate::daemon::manifest::HandoverRoute {
                    legacy_socket_path: legacy_path.clone(),
                    sessions,
                };
                crate::ipc::run_blocking(move || {
                    crate::daemon::manifest::HandoverManifest::update_at_path(
                        &crate::daemon::manifest::get_manifest_path(),
                        |manifest| manifest.add_or_update_route(route),
                    )
                    .map_err(|error| {
                        crate::ipc::IpcError::internal(format!(
                            "Failed to persist predecessor route before handover: {error}"
                        ))
                    })?;
                    Ok(())
                })
                .await
                .map_err(|error| error.to_string())?;

                let commit_resp = legacy_peer
                    .send_request(&DaemonRequest::CommitHandover {
                        legacy_socket_path: None,
                    })
                    .await?;
                if !matches!(commit_resp, DaemonResponse::CommitHandoverOk) {
                    return Err(format!("CommitHandover failed: {commit_resp:?}"));
                }
                self.session_router.add_legacy_peer(legacy_peer);
            }
        }

        let lock_files = acquire_daemon_locks(get_persistent_lock_path().as_deref(), &lock_path)?;
        self.handover_manager.set_lock_files(lock_files);

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
            // Publish the pair as one unit, in the order the reader depends on: the token is this
            // boot's only client credential and is on disk before the accept loop admits a
            // connection, and the port is the marker a client keys on, so it is published last.
            publish_transport_rendezvous(
                &socket_path,
                &get_transport_token_path(),
                port,
                &self.transport_token,
            )
            .map_err(|e| format!("Failed to publish daemon port and token: {e}"))?;
        }

        // Ensure 0600 mode
        validate_safe_ownership_and_type(&socket_path, RuntimeNodeKind::Socket)?;
        #[cfg(unix)]
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Failed to secure daemon socket: {error}"))?;

        tracing::info!("rorca daemon listening on {}", socket_path.display());

        #[cfg(unix)]
        if !crate::daemon::handover::is_v5_ownership_transfer_enabled() {
            self.session_router.adopt_routes_from_manifest().await?;
        }
        #[cfg(not(unix))]
        self.session_router.adopt_routes_from_manifest().await?;

        if handover_from.is_none() {
            self.restore_remote_sessions_at(self.remote_sessions_path.clone())
                .await?;
        }

        // Both platform listeners share this call site: unix binds the socket path, every other
        // platform binds a loopback TCP port. This runs before the readiness signal below: a
        // client released by that signal can spawn a pane immediately, and the pane reads its
        // agent-state endpoint from disk as it spawns, so a listener started afterwards leaves
        // that pane with no endpoint for its whole lifetime.
        let agent_state_ingress = self.spawn_agent_state_listener();
        self.spawn_foreground_observer();
        crate::daemon::agent_extension::install_agent_state_extension();

        // The ingress outcome is observed and recorded before readiness is released, so a boot
        // that announces itself ready without a usable ingress is reported rather than inferred.
        self.settle_agent_state_ingress(agent_state_ingress, || {
            if let Some(tx) = ready_tx {
                let _ = tx.send(());
            }
        });

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
        let mut abort_rx = self.handover_manager.subscribe_client_abort();
        // Set once the connection has presented this boot's transport token. Unix inherits the
        // socket's ownership boundary and never needs the check.
        #[cfg(not(unix))]
        let mut transport_authenticated = false;
        #[cfg(not(unix))]
        const TRANSPORT_REJECTION: &str =
            "Daemon connection rejected: transport token missing or invalid";

        loop {
            tokio::select! {
                _ = abort_rx.recv() => {
                    break;
                }
                read_res = reader.read_line(&mut line) => {
                    match read_res {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let req: Result<DaemonRequest, _> = serde_json::from_str(line.trim());
                    // The loopback transport has no filesystem ownership boundary, so the first
                    // frame must carry this boot's token before any request is dispatched. A
                    // rejected connection is closed rather than answered, so a local process
                    // that can only read `daemon.port` cannot drive the daemon.
                    #[cfg(not(unix))]
                    if !transport_authenticated {
                        let presented = transport_token_from_line(&line);
                        if !transport_token_matches(&self.transport_token, presented.as_deref()) {
                            tracing::warn!("{}", TRANSPORT_REJECTION);
                            let mut rejection = serde_json::to_string(&DaemonResponse::Error {
                                message: TRANSPORT_REJECTION.to_string(),
                                code: Some("TRANSPORT_UNAUTHORIZED".to_string()),
                                details: None,
                            })
                            .unwrap_or_else(|_| "{\"type\":\"error\"}".to_string());
                            rejection.push('\n');
                            let _ = write_half.write_all(rejection.as_bytes()).await;
                            let _ = write_half.flush().await;
                            break;
                        }
                        transport_authenticated = true;
                    }
                    line.clear();
                    let should_check_retirement = matches!(
                        req.as_ref(),
                        Ok(DaemonRequest::Close { .. }) | Ok(DaemonRequest::Hibernate { .. }) | Ok(DaemonRequest::CommitHandover { .. })
                    );

                    let resp = match req {
                Ok(DaemonRequest::Handshake { version, .. }) => {
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
                            daemon_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                        }
                    }
                }
                Ok(DaemonRequest::Ping) => DaemonResponse::Pong,
                Ok(DaemonRequest::SshPassword { host, password }) => {
                    let result = crate::ipc::run_blocking(move || match password {
                        Some(password) => crate::ssh::password::set(&host, password),
                        None => crate::ssh::password::clear(&host),
                    }).await;
                    match result {
                        Ok(()) => DaemonResponse::Pong,
                        Err(error) => DaemonResponse::WorktreeError { error },
                    }
                }
                Ok(DaemonRequest::RemoteSessionDetails { session_id }) => {
                    let local_details = self.terminal_service.remote().details(&session_id);
                    // A rolling handover leaves live remote sessions with the draining
                    // predecessor (restore_remote_sessions_at skips them on purpose), so the
                    // local runtime legitimately has no entry. Answering `details: null` would
                    // make the GUI treat the session as missing and respawn it; route the query
                    // to the daemon that still owns the controller instead.
                    let routed = match local_details.is_none()
                        .then(|| self.session_service.router().find_legacy_peer_for_session(&session_id))
                        .flatten()
                    {
                        Some(peer) => match peer.send_request(&DaemonRequest::RemoteSessionDetails { session_id: session_id.clone() }).await {
                            Ok(response) => match &response {
                                // The peer does not know this session either: fall back to the
                                // local answer (including the legacy direct-SSH classification).
                                // Only the structured discriminator counts; message text must
                                // never reclassify an unrelated peer error as "session absent".
                                DaemonResponse::Error { code, .. }
                                    if code.as_deref() == Some("SESSION_NOT_FOUND") => None,
                                _ => Some(response),
                            },
                            // A transport failure is not evidence of absence; never degrade it
                            // into `details: null`.
                            Err(error) => Some(daemon_error(format!("Legacy peer remote session details failed: {error}"))),
                        },
                        None => None,
                    };
                    match routed {
                        Some(response) => response,
                        None => {
                            let legacy_direct_ssh = local_details.is_none() && self.session_metadata.read().get(&session_id).is_some_and(|m| crate::ssh::projects::is_remote(&m.workspace_id));
                            DaemonResponse::RemoteSessionDetailsOk { details: local_details, legacy_direct_ssh }
                        }
                    }
                }
                Ok(DaemonRequest::MachineGateway) => {
                    if let Err(error) = write_half.write_all(b"{\"type\":\"machineGatewayOk\"}\n").await {
                        tracing::warn!(%error, "Legacy machine gateway admission failed");
                        return;
                    }
                    Box::pin(machine_gateway::serve(tokio::io::join(reader, write_half), self.remote_state().clone())).await;
                    return;
                }
                Ok(DaemonRequest::MachineMetadataSubscribe { target }) => {
                    if let Err(error) = self.session_service.serve_metadata(tokio::io::join(reader, write_half), target).await {
                        tracing::debug!(%error, "Owner metadata subscription ended");
                    }
                    return;
                }
                Ok(DaemonRequest::MachineSessionMetadata { target, report }) => {
                    match Box::pin(self.session_service.validate_machine_agent_report(target, report)).await {
                        Ok(session) => DaemonResponse::MachineSessionDetailOk { detail: crate::remote::machine_protocol::SessionDetail::Running { session } },
                        Err(message) => daemon_error(message),
                    }
                }
                Ok(DaemonRequest::MachineSessionDetail { session_id }) => {
                    match Box::pin(self.session_service.machine_detail_routed(&session_id, crate::scoped_contracts::Epoch(self.epoch))).await {
                        Ok(detail) => DaemonResponse::MachineSessionDetailOk { detail },
                        Err(message) => daemon_error(message),
                    }
                }
                Ok(DaemonRequest::RetryRemoteSession { session_id }) => {
                    // Retry must reach the daemon that owns the controller: during a rolling
                    // handover that is the draining predecessor, not this runtime.
                    match (!self.terminal_service.remote().contains(&session_id))
                        .then(|| self.session_service.router().find_legacy_peer_for_session(&session_id))
                        .flatten()
                    {
                        Some(peer) => match peer.send_request(&DaemonRequest::RetryRemoteSession { session_id: session_id.clone() }).await {
                            Ok(response) => response,
                            Err(error) => daemon_error(format!("Legacy peer remote session retry failed: {error}")),
                        },
                        None => match self.validate_session_ssh_target(&session_id).await {
                            Err(e) => daemon_error(e.to_string()),
                            Ok(()) => match self.terminal_service.remote().retry(&session_id) {
                                Ok(()) => DaemonResponse::RetryRemoteSessionOk,
                                Err(failure) => DaemonResponse::RemoteSessionError { failure },
                            }
                        }
                    }
                },
                Ok(DaemonRequest::RemoteWrite { session_id, generation, data }) if crate::terminal::paired_runtime::Runtime::owns(&session_id) => {
                    match self.terminal_service.write_input_operation(&session_id, generation, data) {
                        Ok(op) => match op.await { Ok(()) => DaemonResponse::WriteOk, Err(e) => daemon_error(e.to_string()) },
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::RemoteResize { session_id, generation, cols, rows }) if crate::terminal::paired_runtime::Runtime::owns(&session_id) => {
                    match self.terminal_service.resize_operation(&session_id, generation, cols, rows) {
                        Ok(op) => match op.await { Ok(()) => DaemonResponse::ResizeOk, Err(e) => daemon_error(e.to_string()) },
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::RemoteWrite { session_id, generation, data }) => {
                    match self.validate_session_ssh_target(&session_id).await {
                        Err(e) => daemon_error(e.to_string()),
                        Ok(()) => match self.terminal_service.remote().write(&session_id, generation, data) {
                        Ok(op) => match op.await { Ok(()) => DaemonResponse::WriteOk, Err(failure) => DaemonResponse::RemoteSessionError { failure } },
                        Err(failure) => DaemonResponse::RemoteSessionError { failure },
                        }
                    }
                }
                Ok(DaemonRequest::RemoteResize { session_id, generation, cols, rows }) => {
                    match self.validate_session_ssh_target(&session_id).await {
                        Err(e) => daemon_error(e.to_string()),
                        Ok(()) => match self.terminal_service.remote().resize(&session_id, generation, cols, rows) {
                        Ok(op) => match op.await { Ok(()) => DaemonResponse::ResizeOk, Err(failure) => DaemonResponse::RemoteSessionError { failure } },
                        Err(failure) => DaemonResponse::RemoteSessionError { failure },
                        }
                    }
                },
                Ok(DaemonRequest::CreateWorktree { workspace_id, worktree, base_ref }) => {
                    match crate::remote::workspace_api::worktrees::owner_mutation(self.remote_state().clone(), workspace_id, worktree, base_ref, None).await {
                        Ok(response) => response,
                        Err(message) => daemon_error(message),
                    }
                }
                Ok(DaemonRequest::DeleteWorktree { workspace_id, worktree, delete_branch, destructive }) => {
                    match crate::remote::workspace_api::worktrees::owner_mutation(self.remote_state().clone(), workspace_id, worktree, None, Some((delete_branch, destructive))).await {
                        Ok(response) => response,
                        Err(message) => daemon_error(message),
                    }
                }
                Ok(DaemonRequest::RegisterWorkspace {
                    workspace_id,
                    repo_root,
                }) => match {
                    let service = Arc::clone(&self.session_service.workspace_service);
                    crate::ipc::run_blocking(move || service.register(&workspace_id, &repo_root)
                        .map_err(crate::ipc::IpcError::internal)).await
                } {
                    Ok(()) => DaemonResponse::RegisterWorkspaceOk,
                    Err(e) => daemon_error(e.to_string()),
                },
                Ok(DaemonRequest::UnregisterWorkspace { workspace_id }) => {
                    match self.handle_unregister_workspace(&workspace_id).await {
                        Ok(()) => DaemonResponse::UnregisterWorkspaceOk,
                        Err(e) => daemon_error(e),
                    }
                }
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
                            DaemonResponse::Error { message, .. } => daemon_error(message),
                            _ => daemon_error("Failed to describe spawned session".to_string(),),
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
                        Err(e) => daemon_error(e.to_string(),),
                    }
                }
                Ok(DaemonRequest::DescribeSession { session_id }) => {
                    if self.session_router.is_local_session(&session_id) {
                        let mut response = self.handle_describe_session(&session_id);
                        if let Some(pid) = self.terminal_service.get_session(&session_id).and_then(|session| session.pid()) {
                            let cwd = crate::ipc::run_blocking::<Option<PathBuf>, _>(move || {
                                Ok(crate::ipc::terminal::process_cwd(pid))
                            }).await;
                            if let DaemonResponse::DescribeSessionOk { session } = &mut response {
                                session.cwd = cwd.ok().flatten().map(|path| path.to_string_lossy().into_owned());
                            }
                        }
                        response
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        match peer.describe_session(&session_id).await {
                            Ok(session) => DaemonResponse::DescribeSessionOk { session },
                            Err(message) => daemon_error(message),
                        }
                    } else {
                        daemon_session_not_found(&session_id, "daemon_describe")
                    }
                }
                Ok(DaemonRequest::DiscoverAgentSession {
                    session_id,
                    agent_type,
                }) => {
                    if self.session_router.is_local_session(&session_id) {
                        let maybe_pid = self
                            .terminal_service
                            .get_session(&session_id)
                            .and_then(|session| session.pid());
                        let provider_session_id = match maybe_pid {
                            Some(pid) => {
                                let agent_type = agent_type.clone();
                                tokio::task::spawn_blocking(move || {
                                    crate::ipc::agents::discover_agent_session_id(pid, &agent_type)
                                })
                                .await
                                .ok()
                                .flatten()
                            }
                            None => None,
                        };
                        DaemonResponse::DiscoverAgentSessionOk {
                            provider_session_id,
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        match peer.discover_agent_session(&session_id, &agent_type).await {
                            Ok(provider_session_id) => DaemonResponse::DiscoverAgentSessionOk { provider_session_id },
                            Err(message) => daemon_error(message),
                        }
                    } else {
                        daemon_session_not_found(&session_id, "daemon_discover_agent")
                    }
                }
                Ok(DaemonRequest::ResetAgentState { session_id }) => {
                    if self.session_router.is_local_session(&session_id) || self.agent_states.current(&session_id).is_some() {
                        self.agent_states.release_manual(&session_id);
                        DaemonResponse::ResetAgentStateOk
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        let peer_call = tokio::time::timeout(
                            std::time::Duration::from_secs(2),
                            peer.reset_agent_state(&session_id),
                        )
                        .await;
                        match peer_call {
                            Ok(Ok(())) => {
                                self.agent_states.release_manual(&session_id);
                                DaemonResponse::ResetAgentStateOk
                            }
                            Ok(Err(message)) => daemon_error(message),
                            Err(_) => {
                                tracing::warn!(session_id, "peer reset_agent_state timed out; releasing locally");
                                self.agent_states.release_manual(&session_id);
                                DaemonResponse::ResetAgentStateOk
                            }
                        }
                    } else {
                        self.agent_states.release_manual(&session_id);
                        DaemonResponse::ResetAgentStateOk
                    }
                }
                Ok(DaemonRequest::Write { session_id, data }) if crate::terminal::paired_runtime::Runtime::owns(&session_id) => {
                    match self.terminal_service.write_input_operation(&session_id, 0, data) {
                        Ok(pending) => match pending.await {
                            Ok(()) => DaemonResponse::WriteOk,
                            Err(e) => daemon_error(e.to_string()),
                        },
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::Resize { session_id, cols, rows }) if crate::terminal::paired_runtime::Runtime::owns(&session_id) => {
                    match self.terminal_service.resize_operation(&session_id, 0, cols, rows) {
                        Ok(pending) => match pending.await {
                            Ok(()) => DaemonResponse::ResizeOk,
                            Err(e) => daemon_error(e.to_string()),
                        },
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::Write { session_id, data }) => {
                    if self.session_router.is_local_session(&session_id) {
                        match self.write_session_input(&session_id, data).await {
                            Ok(()) => DaemonResponse::WriteOk,
                            Err(e) => daemon_error(e.to_string(),),
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        match peer.write_input(&session_id, &data).await {
                            Ok(()) => DaemonResponse::WriteOk,
                            Err(message) => daemon_error(message),
                        }
                    } else {
                        daemon_session_not_found(&session_id, "daemon_write")
                    }
                }
                Ok(DaemonRequest::Resize {
                    session_id,
                    cols,
                    rows,
                }) => {
                    if self.session_router.is_local_session(&session_id) {
                        match self.resize_session(&session_id, cols, rows).await {
                            Ok(()) => DaemonResponse::ResizeOk,
                            Err(e) => daemon_error(e.to_string(),),
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        match peer.resize(&session_id, cols, rows).await {
                            Ok(()) => DaemonResponse::ResizeOk,
                            Err(message) => daemon_error(message),
                        }
                    } else {
                        daemon_session_not_found(&session_id, "daemon_resize")
                    }
                }
                Ok(DaemonRequest::Signal { session_id, signal }) => {
                    if self.session_router.is_local_session(&session_id) {
                        match self.terminal_service.signal(&session_id, signal) {
                            Ok(()) => DaemonResponse::SignalOk,
                            Err(e) => daemon_error(e.to_string(),),
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        match peer.signal(&session_id, signal).await {
                            Ok(()) => DaemonResponse::SignalOk,
                            Err(message) => daemon_error(message),
                        }
                    } else {
                        daemon_session_not_found(&session_id, "daemon_signal")
                    }
                }
                Ok(DaemonRequest::Close { session_id }) => {
                    if self.session_router.is_local_session(&session_id) {
                        match self.handle_close(&session_id).await {
                            Ok(()) => DaemonResponse::CloseOk,
                            Err(crate::terminal::PtyError::SessionNotFound(_)) => daemon_session_not_found(&session_id, "daemon_close"),
                            Err(e) => daemon_error(e),
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        match peer.close(&session_id).await {
                            Ok(()) => {
                                self.agent_states.remove(&session_id);
                                DaemonResponse::CloseOk
                            }
                            Err(message) => daemon_error(message),
                        }
                    } else {
                        // Idempotent close: closing an already closed or non-existent session is a success.
                        DaemonResponse::CloseOk
                    }
                }
                Ok(DaemonRequest::Hibernate { session_id }) => {
                    if self.session_router.is_local_session(&session_id) {
                        match self.handle_hibernate(&session_id).await {
                            Ok(()) => DaemonResponse::HibernateOk,
                            Err(e) => daemon_error(e.to_string(),),
                        }
                    } else {
                        // Idempotent hibernate: a process that is already absent needs no work.
                        DaemonResponse::HibernateOk
                    }
                }
                Ok(DaemonRequest::Suspend { session_id }) => {
                    if self.session_router.is_local_session(&session_id) {
                        match self.handle_suspend(&session_id).await {
                            Ok(()) => DaemonResponse::SuspendOk,
                            Err(e) => daemon_error(e.to_string(),),
                        }
                    } else if self.session_router.find_legacy_peer_for_session(&session_id).is_some() {
                        daemon_error("Session suspend is not supported for peer sessions".to_string(),)
                    } else {
                        daemon_session_not_found(&session_id, "daemon_suspend")
                    }
                }
                Ok(DaemonRequest::Resume { session_id }) => {
                    if self.session_router.is_local_session(&session_id) {
                        match self.handle_resume(&session_id).await {
                            Ok(()) => DaemonResponse::ResumeOk,
                            Err(e) => daemon_error(e.to_string(),),
                        }
                    } else if self.session_router.find_legacy_peer_for_session(&session_id).is_some() {
                        daemon_error("Session resume is not supported for peer sessions".to_string(),)
                    } else {
                        daemon_session_not_found(&session_id, "daemon_resume")
                    }
                }
                Ok(DaemonRequest::ListSessions) => {
                    let sessions = self.session_router.list_sessions().await;
                    DaemonResponse::ListSessionsOk {
                        epoch: self.epoch,
                        sessions,
                    }
                }
                Ok(DaemonRequest::Attach {
                    session_id,
                    after_sequence,
                }) => {
                    let agent_subscription = self.agent_states.subscribe(&session_id);
                    if let Err(error) = self.validate_session_ssh_target(&session_id).await {
                        daemon_error(error.to_string())
                    } else if self.session_router.is_local_session(&session_id) {
                        let is_remote = self.terminal_service.remote().contains(&session_id);
                        match self
                            .terminal_service
                            .attach_remote_with_sequence_ranges(&session_id, if is_remote { None } else { after_sequence })
                        {
                            Ok((mut attachment, history_ranges, remote_generation)) => {
                                if is_remote {
                                    attachment.snapshot.gap = Some(crate::terminal::output_hub::ReplayGap {
                                        requested_after_sequence: after_sequence.unwrap_or(0),
                                        available_from_sequence: attachment.snapshot.history_start_sequence.unwrap_or(1),
                                    });
                                }
                                let (pty_cols, pty_rows) = self
                                    .terminal_service
                                    .get_session(&session_id)
                                    .map(|s| s.get_size())
                                    .or_else(|| self.terminal_service.remote().details(&session_id).map(|d| (d.descriptor.cols, d.descriptor.rows)))
                                    .map(|(c, r)| (Some(c), Some(r)))
                                    .unwrap_or((None, None));
                                let hub = Arc::clone(self.terminal_service.output_hub());
                                let history_flat = Bytes::from(std::mem::take(&mut attachment.snapshot.history));
                                let history_segments = history_ranges
                                    .iter()
                                    .map(|r| HistorySegmentWire {
                                        cols: r.cols,
                                        rows: r.rows,
                                        bytes: history_flat.slice(r.start..r.end),
                                    })
                                    .collect();
                                let resp = DaemonResponse::AttachOk {
                                    epoch: self.epoch,
                                    session_id: session_id.clone(),
                                    start_sequence: attachment.snapshot.history_start_sequence,
                                    end_sequence: attachment.snapshot.history_end_sequence,
                                    gap: attachment.snapshot.gap,
                                    history: history_flat,
                                    pty_cols,
                                    pty_rows,
                                    history_segments,
                                    remote_generation,
                                };
                                let mut resp_json = serde_json::to_string(&resp).unwrap();
                                resp_json.push('\n');
                                let _ = write_half.write_all(resp_json.as_bytes()).await;
                                let _ = write_half.flush().await;

                                self.pump_session_stream(
                                    session_id,
                                    attachment.receiver,
                                    hub,
                                    write_half,
                                    Some(agent_subscription),
                                    Some(&mut reader),
                                )
                                .await;
                                return;
                            }
                            Err(crate::terminal::PtyError::SessionNotFound(_)) => daemon_session_not_found(&session_id, "daemon_attach"),
                            Err(e) => daemon_error(e),
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        if let Err(e) = peer.attach_and_stream(
                            &session_id,
                            after_sequence,
                            &mut write_half,
                            &mut reader,
                            agent_subscription,
                            Arc::clone(&self.agent_states),
                        ).await {
                            daemon_error(e)
                        } else {
                            return;
                        }
                    } else {
                        daemon_session_not_found(&session_id, "daemon_attach")
                    }
                }
                Ok(DaemonRequest::SaveSession { session }) => {
                    let res = crate::ipc::run_blocking(move || save_session_to_path(&get_default_session_path(), &session)).await;
                    match res {
                        Ok(()) => DaemonResponse::SaveSessionOk,
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::LoadSession) => {
                    match crate::ipc::run_blocking(move || load_session_from_path(&get_default_session_path())).await {
                        Ok(session) => DaemonResponse::LoadSessionOk { session },
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::ClearSession) => {
                    match crate::ipc::run_blocking(move || clear_session_from_path(&get_default_session_path())).await {
                        Ok(()) => DaemonResponse::ClearSessionOk,
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::RemoteGetStatus) => {
                    let config = self.remote_state.config.read().clone();
                    let is_running = *self.remote_state.is_running.read();
                    let bound_address = self.remote_state.bound_address.read().clone();
                    let identity = crate::remote::auth::canonical_identity_dir()
                        .ok()
                        .and_then(|dir| crate::remote::auth::load_or_generate_machine_identity(&dir).ok());
                    let machine_id = identity.map(|id| id.machine_id);
                    let relay_pairing = self.remote_state.relay_pairing.read();
                    let (relay_connected, control_channel_connected) = if relay_pairing.is_some() {
                        (Some(true), Some(true))
                    } else {
                        (Some(false), Some(false))
                    };
                    let (gate_status, gate_reason) = {
                        let handle_guard = self.remote_server_handle.lock();
                        if let Some(h) = handle_guard.as_ref() {
                            let gs = h.gate_status();
                            let reason = match &gs {
                                crate::remote::server::DirectGatewayGateStatus::InsecureLanGated { reason, .. } => Some(reason.clone()),
                                _ => None,
                            };
                            (Some(gs), reason)
                        } else {
                            (None, None)
                        }
                    };
                    DaemonResponse::RemoteStatusOk {
                        status: DaemonRemoteStatus {
                            mode: config.mode,
                            port: config.port,
                            allow_control: config.allow_control,
                            is_running,
                            bound_address,
                            relay_url: config.relay_url,
                            machine_id,
                            relay_connected,
                            control_channel_connected,
                            gate_status,
                            gate_reason,
                        },
                    }
                }
                Ok(DaemonRequest::RemoteConfigure { config }) => {
                    match self.handle_remote_configure(config).await {
                        Ok(()) => DaemonResponse::RemoteConfigureOk,
                        Err(e) => daemon_error(e),
                    }
                }
                Ok(DaemonRequest::PairedTerminalReattach { descriptor }) => {
                    let result = async {
                        let mut proxy = crate::terminal::paired_daemon::Proxy::new(descriptor, self.terminal_service.output_hub().clone())?;
                        proxy.reattach(&crate::paired_host::client::MachineClient::new(), &self.paired_hosts).await?;
                        let generation = proxy.controller().ok_or_else(|| crate::paired_host::client::ClientError::local("PAIRED_PROXY_UNAVAILABLE"))?;
                        let session_id = self.terminal_service.paired().install(proxy).map_err(|_| crate::paired_host::client::ClientError::local("CONTROL_CONFLICT"))?;
                        Ok::<_, crate::paired_host::client::ClientError>((session_id, generation))
                    }.await;
                    match result {
                        Ok((session_id, generation)) => DaemonResponse::PairedTerminalReattachOk { session_id, generation },
                        Err(error) => DaemonResponse::PairedHostOperationError { error },
                    }
                }
                Ok(DaemonRequest::PairedTerminalDetach { session_id }) => match self.terminal_service.paired().detach(&session_id).await {
                    Ok(()) => DaemonResponse::CloseOk,
                    Err(message) => daemon_error(message),
                },
                Ok(DaemonRequest::PairedTerminalDescriptor { session_id }) => {
                    let descriptor = self.terminal_service.paired().descriptor(&session_id);
                    DaemonResponse::PairedTerminalDescriptorOk { descriptor }
                }
                Ok(DaemonRequest::PairedHostOperation { request }) => match crate::paired_host::client::MachineClient::new().execute(&self.paired_hosts, request).await {
                    Ok(response) => DaemonResponse::PairedHostOperationOk { response },
                    Err(error) => DaemonResponse::PairedHostOperationError { error },
                },
                Ok(DaemonRequest::PairedHostRead { request }) => match self.paired_hosts.read(request).await {
                    Ok(host) => DaemonResponse::PairedHostReadOk { host },
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostList) => match self.paired_hosts.list().await {
                    Ok(hosts) => DaemonResponse::PairedHostListOk { hosts },
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostPair { request }) => match self.paired_hosts.pair(request).await {
                    Ok(host) => DaemonResponse::PairedHostPairOk { host },
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostMigrateLegacy { request }) => match self.paired_hosts.migrate_legacy(request).await {
                    Ok(receipt) => DaemonResponse::PairedHostMigrateLegacyOk { receipt },
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostForget { host_id, expected_generation }) => match self.paired_hosts.forget(host_id, expected_generation).await {
                    Ok(()) => DaemonResponse::PairedHostForgetOk,
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostRevoke { host_id, generation }) => {
                    self.paired_hosts.revoke_on_auth_failure(host_id, generation).await;
                    DaemonResponse::PairedHostForgetOk
                }
                Ok(DaemonRequest::GetCapabilities) => {
                    let mut capabilities = vec!["machinePairingV1".into(), "sshPasswordV1".into(), "dagStreamingV1".into()];
                    if self.paired_hosts.available().await { capabilities.push("pairedHostInventoryV1".into()); }
                    #[cfg(unix)]
                    capabilities.push("sessionOwnershipTransferV1".into());
                    DaemonResponse::CapabilitiesOk { capabilities }
                },
                Ok(DaemonRequest::RemoteAllocateAttachSession) => {
                    let client = self.remote_state.relay_client.read().clone();
                    let relay_origin = self
                        .remote_state
                        .config
                        .read()
                        .relay_url
                        .clone()
                        .unwrap_or_default();
                    match client {
                        None => DaemonResponse::Error {
                            message: "relay mode is not running".into(),
                            code: Some("RELAY_OFF".into()),
                            details: None,
                        },
                        Some(client) => match client.allocate_opaque_session().await {
                        Ok(session_id) => {
                            let identity = crate::remote::auth::canonical_identity_dir()
                                .ok()
                                .and_then(|dir| {
                                    crate::remote::auth::load_or_generate_machine_identity(&dir).ok()
                                });
                            let attach = crate::remote::attach_identity::
                                load_or_generate_canonical_attach_identity();
                            match (identity, attach) {
                                (Some(identity), Ok(attach)) => {
                                    DaemonResponse::RemoteAttachSessionOk {
                                        session_id,
                                        machine_id: identity.machine_id,
                                        machine_attach_public_key: attach.public_key,
                                        enrollment_epoch: crate::account::enroll_client::
                                            load_enrollment_record()
                                            .map(|record| record.enrollment_epoch)
                                            .unwrap_or_default(),
                                        relay_origin,
                                    }
                                }
                                (None, _) => DaemonResponse::Error {
                                    message: "machine identity unavailable".into(),
                                    code: Some("IDENTITY_UNAVAILABLE".into()),
                                    details: None,
                                },
                                (_, Err(error)) => DaemonResponse::Error {
                                    message: error,
                                    code: Some("ATTACH_IDENTITY_UNAVAILABLE".into()),
                                    details: None,
                                },
                            }
                        }
                            Err(error) => DaemonResponse::Error {
                                message: error.to_string(),
                                code: Some("RELAY_ALLOCATE_FAILED".into()),
                                details: None,
                            },
                        },
                    }
                }
                Ok(request @ (DaemonRequest::RemoteCreatePairingCode { .. }
                    | DaemonRequest::RemoteCreateMachinePairingCode)) => {
                    let (permission, scope) = match request {
                        DaemonRequest::RemoteCreatePairingCode { permission } =>
                            (permission, crate::remote::auth::DeviceAccessScope::Mirror),
                        DaemonRequest::RemoteCreateMachinePairingCode =>
                            (Some(DevicePermission::Control), crate::remote::auth::DeviceAccessScope::Machine),
                        _ => unreachable!("pairing request pattern"),
                    };
                    let perm = permission.unwrap_or(DevicePermission::Control);
                    let mut auto_configured = false;
                    let mut config_error = None;
                    if self.remote_state.relay_pairing.read().is_none() {
                        let mode = self.remote_state.config.read().mode;
                        if mode == RemoteNetworkMode::Off {
                            let relay_url = std::env::var("FERRYX_RELAY_URL")
                                .ok()
                                .filter(|s| !s.trim().is_empty())
                                .or_else(|| self.remote_state.config.read().relay_url.clone())
                                .or_else(|| Some(crate::remote::relay_server::DEFAULT_RELAY_URL.to_string()));
                            if let Some(relay_url) = relay_url {
                                let mut config = self.remote_state.config.read().clone();
                                config.mode = RemoteNetworkMode::Relay;
                                config.relay_url = Some(relay_url);
                                if let Err(e) = self.handle_remote_configure(config).await {
                                    config_error = Some(e);
                                } else {
                                    auto_configured = true;
                                }
                            }
                        }
                    }

                    if let Some(e) = config_error {
                        daemon_error(format!("Relay is unreachable or registration failed: {e}"),)
                    } else {
                        let is_relay_mode = auto_configured
                            || self.remote_state.config.read().mode == RemoteNetworkMode::Relay;

                        // In relay mode a code is only redeemable remotely if its PIN was
                        // registered with the relay, so go through the gateway's single
                        // pairing coordinator rather than minting a local-only code.
                        let coordinator = self
                            .remote_state
                            .relay_pairing
                            .read()
                            .as_ref()
                            .map(|published| published.coordinator.clone());
                        match coordinator {
                            Some(coordinator) => {
                                match coordinator
                                    .generate_scoped_pairing(
                                        std::time::Duration::from_secs(if scope == crate::remote::auth::DeviceAccessScope::Machine { 600 } else { 60 }),
                                        perm,
                                        scope,
                                    )
                                    .await
                                {
                                    Ok(info) => {
                                        let effective_relay_url = self
                                            .remote_state
                                            .config
                                            .read()
                                            .relay_url
                                            .clone()
                                            .or_else(|| {
                                                std::env::var("FERRYX_RELAY_URL")
                                                    .ok()
                                                    .filter(|s| !s.trim().is_empty())
                                            })
                                            .or_else(|| {
                                                Some(crate::remote::relay_server::DEFAULT_RELAY_URL.to_string())
                                            });
                                        DaemonResponse::RemotePairingCodeOk {
                                            code: info.pin,
                                            pairing_token: Some(info.pairing_token),
                                            machine_id: Some(info.machine_id),
                                            relay_url: effective_relay_url,
                                        }
                                    }
                                    Err(message) => {
                                        let message = if message.starts_with("Relay is unreachable or registration failed:") {
                                            message
                                        } else {
                                            format!("Relay is unreachable or registration failed: {message}")
                                        };
                                        daemon_error(message)
                                    }
                                }
                            }
                            None if is_relay_mode => daemon_error("Relay is unreachable or registration failed: relay client is not connected".to_string(),),
                            None => {
                                match self.remote_state.auth_manager.create_scoped_pairing_code(perm, scope) {
                                    Ok(code) => {
                                        let machine_id = if scope == crate::remote::auth::DeviceAccessScope::Machine {
                                            crate::remote::auth::canonical_identity_dir()
                                                .ok()
                                                .and_then(|dir| crate::remote::auth::load_or_generate_machine_identity(&dir).ok())
                                                .map(|id| id.machine_id)
                                        } else {
                                            None
                                        };
                                        DaemonResponse::RemotePairingCodeOk {
                                            code,
                                            pairing_token: None,
                                            machine_id,
                                            relay_url: None,
                                        }
                                    }
                                    Err(error) => daemon_error(error.to_string()),
                                }
                            }
                        }
                    }
                }
                Ok(DaemonRequest::RemoteListDevices) => {
                    let devices = self.remote_state.auth_manager.list_devices();
                    DaemonResponse::RemoteListDevicesOk { devices }
                }
                Ok(DaemonRequest::RemoteRevokeDevice { device_id }) => {
                    match self.remote_state.auth_manager.revoke_device(&device_id) {
                        Ok(true) => DaemonResponse::RemoteRevokeDeviceOk,
                        Ok(false) => daemon_error(format!("Device '{device_id}' not found"),),
                        Err(err) => daemon_error(format!("Failed to persist revocation: {err}"),),
                    }
                }
                Ok(DaemonRequest::RemoteSetActiveSelection { selection, ssh_store_path }) => {
                    if let Some(path) = ssh_store_path {
                        *self.remote_state.ssh_store_path.write() = Some(path);
                    }
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
                Ok(DaemonRequest::UpgradeBinary { new_binary_path }) => {
                    self.handle_upgrade_binary(new_binary_path).await
                }
                Ok(DaemonRequest::PrepareHandover) => {
                    #[cfg(unix)]
                    {
                        match self.handover_manager.prepare_handover(&self.terminal_service) {
                            Ok((legacy_path, active_sessions, listener)) => {
                                let legacy_path_str = legacy_path.to_string_lossy().into_owned();
                                Arc::clone(&self).spawn_legacy_listener(listener);
                                DaemonResponse::PrepareHandoverOk {
                                    legacy_socket_path: legacy_path_str,
                                    active_sessions,
                                }
                            }
                            Err(e) => daemon_error(e),
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        DaemonResponse::HandoverRejected {
                            reason: "Handover is only supported on Unix platforms".to_string(),
                        }
                    }
                }
                Ok(DaemonRequest::TransferSessions { handover_socket_path }) => {
                    #[cfg(unix)]
                    {
                        let path = std::path::PathBuf::from(handover_socket_path);
                        match crate::daemon::handover_socket::connect_handover_socket(&path) {
                            Ok((stream, _creds)) => {
                                let sessions = self.terminal_service.list_sessions();
                                let mut count = 0;
                                let transfer_id = uuid::Uuid::new_v4().to_string();
                                let mut seq = 1;
                                for session_id in sessions {
                                    if let Ok(export) = self.terminal_service.pty_manager().export_session(&session_id) {
                                        if crate::daemon::handover_socket::send_session(&stream, &transfer_id, seq, export).is_ok() {
                                            count += 1;
                                            seq += 1;
                                        }
                                    }
                                }
                                let _ = crate::daemon::handover_socket::send_transfer_done(&stream, &transfer_id, seq);
                                DaemonResponse::TransferSessionsOk {
                                    transferred_count: count,
                                }
                            }
                            Err(e) => daemon_error(format!("Failed to connect to handover socket: {e}")),
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        DaemonResponse::HandoverRejected {
                            reason: "Session transfer unsupported on non-Unix".to_string(),
                        }
                    }
                }
                Ok(DaemonRequest::CommitHandover { .. }) => {
                    let manager = Arc::clone(&self.handover_manager);
                    let service = Arc::clone(&self.terminal_service);
                    match crate::ipc::run_blocking(move || {
                        manager.commit_handover(&service).map_err(crate::ipc::IpcError::internal)
                    }).await {
                        Ok(()) => DaemonResponse::CommitHandoverOk,
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::AbortHandover) => {
                    match self.handover_manager.abort_handover() {
                        Ok(()) => DaemonResponse::AbortHandoverOk,
                        Err(e) => daemon_error(e),
                    }
                }
                Ok(DaemonRequest::UploadClipboardImage { file_name, data }) => {
                    match crate::clipboard_image::save_paste_file(&file_name, &data) {
                        Ok(path) => DaemonResponse::UploadClipboardImageOk {
                            remote_path: path.to_string_lossy().into_owned(),
                            byte_length: data.len(),
                        },
                        Err(e) => daemon_error(e.message),
                    }
                }
                Ok(DaemonRequest::SubscribeDag { workspace_id, project_path, paired }) => {
                    let mut resp_json = serde_json::to_string(&DaemonResponse::SubscribeDagOk).unwrap();
                    resp_json.push('\n');
                    if write_half.write_all(resp_json.as_bytes()).await.is_err()
                        || write_half.flush().await.is_err()
                    {
                        return;
                    }
                    match paired {
                        // A paired workspace's journal lives on the remote machine.
                        // Scanning project_path here would read an unrelated local
                        // directory that happens to share the name, or nothing at all.
                        Some(binding) => {
                            self.serve_paired_dag_stream(&mut reader, &mut write_half, workspace_id, binding).await;
                        }
                        None => {
                            self.serve_dag_stream(&mut reader, &mut write_half, workspace_id, project_path).await;
                        }
                    }
                    return;
                }
                Ok(DaemonRequest::UnsubscribeDag { .. }) => {
                    DaemonResponse::UnsubscribeDagOk
                }
                Ok(DaemonRequest::Shutdown) => {
                    match self.persist_remote_sessions_at(self.remote_sessions_path.clone()).await {
                        Ok(()) => std::process::exit(0),
                        Err(message) => daemon_error(message),
                    }
                }
                Err(e) => daemon_error(format!("Malformed request: {e}"),),
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
            if should_check_retirement {
                self.handover_manager
                    .check_retirement_if_empty(&self.terminal_service);
            }
                }
            }
        }
    }

    #[cfg(unix)]
    pub fn spawn_legacy_listener(self: Arc<Self>, listener: UnixListener) {
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let s = Arc::clone(&self);
                        tokio::spawn(async move {
                            s.handle_client(stream).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });
    }

    /// Streams a paired workspace's DAG from the authenticated remote host.
    ///
    /// The desktop daemon owns no copy of that journal, so it forwards frames the
    /// host pushes and translates them into the same stream messages a local
    /// subscription produces. `project_path` on every forwarded frame is the root
    /// the *remote* resolved, never a local path.
    async fn serve_paired_dag_stream<R, W>(
        &self,
        reader: &mut R,
        write_half: &mut W,
        workspace_id: String,
        binding: crate::daemon::protocol::PairedDagBinding,
    ) where
        R: tokio::io::AsyncBufReadExt + Unpin,
        W: tokio::io::AsyncWriteExt + Unpin,
    {
        use futures_util::StreamExt;

        let stream = crate::paired_host::client::MachineClient::new()
            .attach_dag(
                &self.paired_hosts,
                binding.host_id.clone(),
                binding.generation,
                &binding.remote_workspace_id,
            )
            .await;
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!(code = %error.code, workspace_id = %workspace_id, "Paired DAG attach refused");
                let message = DaemonResponse::Error {
                    message: error.code.clone(),
                    code: Some(error.code),
                    details: None,
                };
                if let Ok(mut json) = serde_json::to_string(&message) {
                    json.push('\n');
                    let _ = write_half.write_all(json.as_bytes()).await;
                    let _ = write_half.flush().await;
                }
                return;
            }
        };

        let mut client_lines = reader.lines();
        loop {
            tokio::select! {
                frame = stream.socket.next() => {
                    let Some(Ok(frame)) = frame else { break };
                    let text = match frame {
                        tokio_tungstenite::tungstenite::Message::Text(text) => text,
                        tokio_tungstenite::tungstenite::Message::Close(_) => break,
                        _ => continue,
                    };
                    let Ok(parsed) = serde_json::from_str::<crate::remote::dag_api::DagFrame>(&text) else {
                        continue;
                    };
                    let message = match parsed {
                        crate::remote::dag_api::DagFrame::DagInventory { project_path, runs, .. } => {
                            DaemonStreamMessage::DagInventory {
                                workspace_id: std::borrow::Cow::Borrowed(&workspace_id),
                                project_path: std::borrow::Cow::Owned(project_path),
                                runs,
                            }
                        }
                        crate::remote::dag_api::DagFrame::DagRunUpdated { project_path, snapshot, .. } => {
                            DaemonStreamMessage::DagRunUpdated {
                                workspace_id: std::borrow::Cow::Borrowed(&workspace_id),
                                project_path: std::borrow::Cow::Owned(project_path),
                                snapshot,
                            }
                        }
                        crate::remote::dag_api::DagFrame::DagError { code } => {
                            tracing::warn!(%code, workspace_id = %workspace_id, "Paired DAG stream reported an error");
                            break;
                        }
                    };
                    if let Ok(mut json) = serde_json::to_string(&message) {
                        json.push('\n');
                        if write_half.write_all(json.as_bytes()).await.is_err()
                            || write_half.flush().await.is_err()
                        {
                            break;
                        }
                    }
                }
                line = client_lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            if let Ok(DaemonRequest::UnsubscribeDag { .. }) = serde_json::from_str::<DaemonRequest>(&line) {
                                let mut unsub_ok = serde_json::to_string(&DaemonResponse::UnsubscribeDagOk).unwrap();
                                unsub_ok.push('\n');
                                let _ = write_half.write_all(unsub_ok.as_bytes()).await;
                                let _ = write_half.flush().await;
                                break;
                            }
                        }
                        _ => break,
                    }
                }
            }
        }
        // Closing the socket is what stops the remote watcher; dropping alone would
        // leave the host streaming until its own transfer timeout.
        let _ = stream.socket.close(None).await;
    }

    async fn serve_dag_stream<R, W>(
        &self,
        reader: &mut R,
        write_half: &mut W,
        workspace_id: String,
        project_path: String,
    ) where
        R: tokio::io::AsyncBufReadExt + Unpin,
        W: tokio::io::AsyncWriteExt + Unpin,
    {
        let ppath = project_path.clone();
        let runs = crate::ipc::run_blocking(move || {
            Ok(crate::daemon::dag_service::scan_project_inventory(
                std::path::Path::new(&ppath),
            ))
        })
        .await
        .unwrap_or_default();

        let inv_msg = DaemonStreamMessage::DagInventory {
            workspace_id: std::borrow::Cow::Borrowed(&workspace_id),
            project_path: std::borrow::Cow::Borrowed(&project_path),
            runs,
        };
        if let Ok(mut inv_json) = serde_json::to_string(&inv_msg) {
            inv_json.push('\n');
            if write_half.write_all(inv_json.as_bytes()).await.is_err()
                || write_half.flush().await.is_err()
            {
                return;
            }
        }

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(100);
        let watcher_handle = crate::dag::watcher::spawn_dag_watcher(
            std::path::PathBuf::from(&project_path),
            event_tx,
        );

        let mut client_lines = reader.lines();
        loop {
            tokio::select! {
                maybe_event = event_rx.recv() => {
                    match maybe_event {
                        Some((_, snapshot)) => {
                            let update_msg = DaemonStreamMessage::DagRunUpdated {
                                workspace_id: std::borrow::Cow::Borrowed(&workspace_id),
                                project_path: std::borrow::Cow::Borrowed(&project_path),
                                snapshot: Box::new(snapshot),
                            };
                            if let Ok(mut update_json) = serde_json::to_string(&update_msg) {
                                update_json.push('\n');
                                if write_half.write_all(update_json.as_bytes()).await.is_err()
                                    || write_half.flush().await.is_err()
                                {
                                    break;
                                }
                            }
                        }
                        None => break,
                    }
                }
                res = client_lines.next_line() => {
                    match res {
                        Ok(Some(line)) => {
                            if let Ok(DaemonRequest::UnsubscribeDag { .. }) = serde_json::from_str::<DaemonRequest>(&line) {
                                let mut unsub_ok = serde_json::to_string(&DaemonResponse::UnsubscribeDagOk).unwrap();
                                unsub_ok.push('\n');
                                let _ = write_half.write_all(unsub_ok.as_bytes()).await;
                                let _ = write_half.flush().await;
                                break;
                            }
                        }
                        _ => break,
                    }
                }
            }
        }
        watcher_handle.abort();
    }

    #[cfg(unix)]
    pub fn spawn_legacy_handover_daemon(
        self: Arc<Self>,
        legacy_path: PathBuf,
        listener: UnixListener,
        exe: PathBuf,
    ) {
        tokio::spawn(async move {
            let mut cmd = std::process::Command::new(exe);
            cmd.arg("--daemon")
                .arg("--handover-from")
                .arg(&legacy_path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null());
            if let Err(e) = cmd.spawn() {
                // The canonical listener is already gone by this point. Abandoning the loop
                // here would strand every live session with no reachable socket, so keep
                // serving the legacy peer instead of returning.
                tracing::error!(
                    "Failed to spawn new daemon for handover: {e}; continuing to serve legacy socket"
                );
            }

            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let s = Arc::clone(&self);
                        tokio::spawn(async move {
                            s.handle_client(stream).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });
    }

    #[cfg(unix)]
    async fn handle_upgrade_binary(
        self: &Arc<Self>,
        new_binary_path: Option<String>,
    ) -> DaemonResponse {
        let Some(target_exe) = resolve_upgrade_target_exe(new_binary_path.as_deref()) else {
            tracing::error!(
                "Daemon upgrade aborted: no launchable executable on disk (requested {:?}, current_exe {:?})",
                new_binary_path,
                std::env::current_exe().ok()
            );
            return daemon_error(
                "Daemon upgrade unavailable: the running daemon's executable no longer \
                          exists on disk and no valid newBinaryPath was supplied",
            );
        };

        // If no explicit new binary path was provided by the GUI, check if upgrade is needed
        if new_binary_path.is_none() {
            let booted_mtime = self.binary_mtime_ms;
            let target_mtime = tokio::task::spawn_blocking({
                let exe = target_exe.clone();
                move || get_file_mtime_ms(&exe)
            })
            .await
            .unwrap_or(None);

            if !crate::daemon::client::should_request_upgrade(
                None,
                env!("CARGO_PKG_VERSION"),
                booted_mtime,
                target_mtime,
            ) {
                return DaemonResponse::UpgradeNotNeeded;
            }
        }

        let active_sessions = self.terminal_service.list_sessions();
        if active_sessions.is_empty() {
            let exe = target_exe.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                if let Err(e) = perform_daemon_exec_with_path(&exe) {
                    tracing::error!("Failed to re-exec daemon during upgrade: {e}");
                }
            });
            return DaemonResponse::UpgradeScheduled;
        }

        let (legacy_path, _, listener) = match self
            .handover_manager
            .prepare_handover(&self.terminal_service)
        {
            Ok(res) => res,
            Err(e) => return daemon_error(format!("Failed to prepare handover: {e}")),
        };

        Arc::clone(self).spawn_legacy_handover_daemon(legacy_path, listener, target_exe);

        DaemonResponse::UpgradeScheduled
    }

    #[cfg(not(unix))]
    async fn handle_upgrade_binary(
        self: &Arc<Self>,
        _new_binary_path: Option<String>,
    ) -> DaemonResponse {
        let live_sessions = self.terminal_service.list_sessions().len();
        match crate::daemon::handover::upgrade_action(live_sessions, false) {
            crate::daemon::handover::UpgradeAction::Proceed => {
                if !crate::daemon::handover::idle_upgrade_requested() {
                    tracing::warn!(
                        live_sessions,
                        "Daemon upgrade requested on a platform without session transfer; \
                         Ferryx must be restarted to finish updating (set \
                         FERRYX_DAEMON_IDLE_UPGRADE=1 to allow an idle daemon restart)"
                    );
                    return DaemonResponse::UpgradeUnsupported;
                }
                let exe = match std::env::current_exe() {
                    Ok(exe) => exe,
                    Err(error) => {
                        tracing::warn!(%error, "Daemon upgrade aborted: current executable unknown");
                        return DaemonResponse::UpgradeUnsupported;
                    }
                };
                // The successor waits for our instance lock, so it must be running before we
                // release it. No session is live here, so the exit cannot cost a terminal.
                let spawned = std::process::Command::new(&exe)
                    .arg("--daemon")
                    .env("FERRYX_DAEMON_SUCCESSOR", "1")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                match spawned {
                    Ok(child) => {
                        tracing::warn!(
                            pid = child.id(),
                            live_sessions,
                            "Restarting idle daemon: successor spawned, releasing locks and exiting"
                        );
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            std::process::exit(0);
                        });
                        DaemonResponse::UpgradeScheduled
                    }
                    Err(error) => {
                        tracing::warn!(%error, live_sessions, "Idle daemon restart failed to spawn a successor");
                        DaemonResponse::UpgradeUnsupported
                    }
                }
            }
            crate::daemon::handover::UpgradeAction::Refuse { live_sessions } => {
                // `UpgradeUnsupported` carries no field, so the restart instruction has to
                // travel in the log: this is the only place the user learns why the update
                // did not finish.
                tracing::warn!(
                    live_sessions,
                    "Daemon upgrade refused: this platform cannot transfer live sessions; \
                     restart Ferryx to finish updating"
                );
                DaemonResponse::UpgradeUnsupported
            }
        }
    }

    pub fn handle_register_workspace(
        &self,
        workspace_id: &str,
        repo_root: &str,
    ) -> Result<(), String> {
        self.session_service
            .workspace_service
            .register(workspace_id, repo_root)
    }

    pub async fn handle_remote_configure(
        &self,
        mut config: RemoteGatewayConfig,
    ) -> Result<(), String> {
        config.port = REMOTE_GATEWAY_PORT;
        self.configure_gateway(config).await
    }

    pub(crate) async fn configure_gateway(
        &self,
        config: RemoteGatewayConfig,
    ) -> Result<(), String> {
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

    pub async fn pump_sequenced_stream<W>(
        session_id: String,
        rx: broadcast::Receiver<crate::terminal::output_hub::OutputChunk>,
        hub: Arc<TerminalOutputHub>,
        writer: W,
    ) where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        Self::pump_sequenced_stream_with_agent_state(
            session_id,
            rx,
            hub,
            writer,
            None,
            None::<tokio::io::Empty>,
        )
        .await
    }

    async fn pump_session_stream<W, R>(
        &self,
        session_id: String,
        rx: broadcast::Receiver<crate::terminal::output_hub::OutputChunk>,
        hub: Arc<TerminalOutputHub>,
        mut writer: W,
        agent_rx: Option<AgentStateSubscription>,
        client_reader: Option<R>,
    ) where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
        R: tokio::io::AsyncRead + Unpin + Send,
    {
        let Ok(mut updates) = self.terminal_service.remote().subscribe(&session_id) else {
            Self::pump_sequenced_stream_with_agent_state(
                session_id,
                rx,
                hub,
                writer,
                agent_rx,
                client_reader,
            )
            .await;
            return;
        };
        let (input, output) = tokio::io::duplex(64 * 1024);
        let pump = Self::pump_sequenced_stream_with_agent_state(
            session_id.clone(),
            rx,
            hub,
            output,
            agent_rx,
            client_reader,
        );
        tokio::pin!(pump);
        let mut lines = BufReader::new(input).lines();
        loop {
            let details = updates.borrow_and_update().clone();
            let status = DaemonStreamMessage::RemoteStatus {
                session_id: Cow::Borrowed(&session_id),
                state: details.state,
                generation: details.generation,
                failure: details.failure,
                replay_gap: details.replay_gap,
            };
            let Ok(frame) = crate::daemon::protocol::encode_daemon_stream_frame(&status) else {
                break;
            };
            if writer.write_all(frame.as_bytes()).await.is_err() || writer.flush().await.is_err() {
                break;
            }
            loop {
                tokio::select! {
                    _ = &mut pump => return,
                    changed = updates.changed() => { if changed.is_err() { return; } break; },
                    line = lines.next_line() => {
                        let Ok(Some(mut line)) = line else { return; };
                        line.push('\n');
                        if writer.write_all(line.as_bytes()).await.is_err() || writer.flush().await.is_err() { return; }
                    }
                }
            }
        }
    }

    pub(crate) async fn pump_sequenced_stream_with_agent_state<W, R>(
        session_id: String,
        mut rx: broadcast::Receiver<crate::terminal::output_hub::OutputChunk>,
        hub: Arc<TerminalOutputHub>,
        writer: W,
        mut agent_state_rx: Option<AgentStateSubscription>,
        mut client_reader: Option<R>,
    ) where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
        R: tokio::io::AsyncRead + Unpin + Send,
    {
        let mut writer = BufWriter::new(writer);
        if let Some(snapshot) = agent_state_rx
            .as_ref()
            .and_then(|subscription| subscription.snapshot.as_ref())
        {
            let msg = DaemonStreamMessage::AgentState {
                session_id: Cow::Borrowed(&snapshot.session_id),
                state: Cow::Borrowed(&snapshot.state),
                agent: snapshot.agent.as_deref().map(Cow::Borrowed),
                provider_session: snapshot.provider_session.clone(),
                is_snapshot: true,
                origin: snapshot.origin,
            };
            let Ok(frame) = crate::daemon::protocol::encode_daemon_stream_frame(&msg) else {
                return;
            };
            if writer.write_all(frame.as_bytes()).await.is_err() || writer.flush().await.is_err() {
                return;
            }
        }
        let mut last_seen_sequence: Option<u64> = None;
        // Reused serialization buffer: one allocation serves the whole stream instead of a
        // fresh String per frame (audit H2: optimized payload encoding).
        let mut frame_buf = Vec::with_capacity(8 * 1024);
        // A signal drained from `rx` (lag or close) that must be processed by the main
        // loop on the next iteration instead of inside the batch drain.
        let mut pending: Option<
            Result<crate::terminal::output_hub::OutputChunk, broadcast::error::RecvError>,
        > = None;

        let mut disconnect_buf = [0u8; 1];

        loop {
            let received = match pending.take() {
                Some(received) => received,
                None => {
                    tokio::select! {
                        biased;
                        disconnect = async {
                            match client_reader.as_mut() {
                                Some(r) => r.read(&mut disconnect_buf).await,
                                None => std::future::pending().await,
                            }
                        } => {
                            match disconnect {
                                Ok(0) | Err(_) => {
                                    tracing::debug!(session_id = %session_id, "Client disconnected from attach stream");
                                    break;
                                }
                                Ok(_) => {
                                    // Unexpected client data on output stream; terminate stream
                                    tracing::debug!(session_id = %session_id, "Unexpected client data on attach stream");
                                    break;
                                }
                            }
                        }
                        report = async {
                            match agent_state_rx.as_mut() {
                                Some(subscription) => subscription.receiver.recv().await,
                                None => std::future::pending().await,
                            }
                        } => {
                            match report {
                                Ok(report) if report.state.session_id == session_id => {
                                    let msg = DaemonStreamMessage::AgentState {
                                        session_id: Cow::Borrowed(&session_id),
                                        state: Cow::Borrowed(&report.state.state),
                                        agent: report.state.agent.as_deref().map(Cow::Borrowed),
                                        provider_session: report.state.provider_session,
                                        is_snapshot: report.is_snapshot,
                                        origin: report.state.origin,
                                    };
                                    frame_buf.clear();
                                    if serde_json::to_writer(&mut frame_buf, &msg).is_err() {
                                        break;
                                    }
                                    frame_buf.push(b'\n');
                                    if writer.write_all(&frame_buf).await.is_err() {
                                        break;
                                    }
                                    if writer.flush().await.is_err() {
                                        break;
                                    }
                                }
                                Ok(_) => {}
                                Err(broadcast::error::RecvError::Lagged(_)) => {
                                    if let Some(subscription) = agent_state_rx.as_mut() {
                                        if let Some(current) = subscription.resynchronize(&session_id) {
                                            let msg = DaemonStreamMessage::AgentState {
                                                session_id: Cow::Borrowed(&session_id),
                                                state: Cow::Borrowed(&current.state),
                                                agent: current.agent.as_deref().map(Cow::Borrowed),
                                                provider_session: current.provider_session,
                                                is_snapshot: true,
                                                origin: current.origin,
                                            };
                                            frame_buf.clear();
                                            if serde_json::to_writer(&mut frame_buf, &msg).is_err() { break; }
                                            frame_buf.push(b'\n');
                                            if writer.write_all(&frame_buf).await.is_err() || writer.flush().await.is_err() { break; }
                                        }
                                    }
                                }
                                Err(broadcast::error::RecvError::Closed) => break,
                            }
                            continue;
                        }
                        output = rx.recv() => output,
                    }
                }
            };

            match received {
                Ok(chunk) => {
                    if let Some(gap) = &chunk.replay_gap {
                        let msg = DaemonStreamMessage::Gap {
                            session_id: Cow::Borrowed(&session_id),
                            requested_after_sequence: gap.requested_after_sequence,
                            available_from_sequence: gap.available_from_sequence,
                        };
                        let Ok(frame) = crate::daemon::protocol::encode_daemon_stream_frame(&msg)
                        else {
                            break;
                        };
                        if writer.write_all(frame.as_bytes()).await.is_err()
                            || writer.flush().await.is_err()
                        {
                            break;
                        }
                        last_seen_sequence = Some(chunk.sequence);
                        continue;
                    }
                    if last_seen_sequence.is_some_and(|last| chunk.sequence <= last) {
                        continue;
                    }
                    last_seen_sequence = Some(chunk.sequence);
                    {
                        let msg = DaemonStreamMessage::Output {
                            session_id: Cow::Borrowed(&session_id),
                            sequence: chunk.sequence,
                            data: Cow::Borrowed(&chunk.bytes),
                            metrics_read_unix_micros: chunk.metrics_read_unix_micros,
                        };
                        frame_buf.clear();
                        if serde_json::to_writer(&mut frame_buf, &msg).is_err() {
                            break;
                        }
                        frame_buf.push(b'\n');
                        if writer.write_all(&frame_buf).await.is_err() {
                            break;
                        }
                    }

                    // Drain-then-flush (audit M5): absorb any immediately-available output
                    // chunks into the buffered writer before flushing, so a burst costs one
                    // flush per batch instead of one per chunk. Flushing when the queue is
                    // empty keeps interactive latency at a single batch interval.
                    const BATCH_FLUSH_BUDGET_BYTES: usize = 64 * 1024;
                    let mut batched_bytes = frame_buf.len();
                    let mut write_failed = false;
                    while batched_bytes < BATCH_FLUSH_BUDGET_BYTES {
                        match rx.try_recv() {
                            Ok(next) => {
                                if next.replay_gap.is_some() {
                                    pending = Some(Ok(next));
                                    break;
                                }
                                if last_seen_sequence.is_some_and(|last| next.sequence <= last) {
                                    continue;
                                }
                                last_seen_sequence = Some(next.sequence);
                                let msg = DaemonStreamMessage::Output {
                                    session_id: Cow::Borrowed(&session_id),
                                    sequence: next.sequence,
                                    data: Cow::Borrowed(&next.bytes),
                                    metrics_read_unix_micros: next.metrics_read_unix_micros,
                                };
                                frame_buf.clear();
                                if serde_json::to_writer(&mut frame_buf, &msg).is_err() {
                                    write_failed = true;
                                    break;
                                }
                                frame_buf.push(b'\n');
                                batched_bytes += frame_buf.len();
                                if writer.write_all(&frame_buf).await.is_err() {
                                    write_failed = true;
                                    break;
                                }
                            }
                            Err(broadcast::error::TryRecvError::Empty) => break,
                            Err(broadcast::error::TryRecvError::Closed) => {
                                pending = Some(Err(broadcast::error::RecvError::Closed));
                                break;
                            }
                            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                                pending = Some(Err(broadcast::error::RecvError::Lagged(n)));
                                break;
                            }
                        }
                    }
                    if write_failed {
                        break;
                    }
                    if writer.flush().await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Re-subscribe with sequence after last_seen_sequence to recover replay gap
                    if let Some((mut att, history_ranges)) =
                        hub.subscribe_with_sequence_ranges(&session_id, last_seen_sequence)
                    {
                        rx = att.receiver;
                        // Captured before `last_seen_sequence` is overwritten below: a delta claim
                        // is only meaningful relative to what this subscriber actually asked for.
                        // A `None` request is a full-history replay, never a delta, no matter what
                        // the snapshot's gap field says.
                        let requested_cursor = last_seen_sequence;
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
                        let lag_history = Bytes::from(std::mem::take(&mut att.snapshot.history));
                        let segments = history_ranges
                            .iter()
                            .map(|r| HistorySegmentWire {
                                cols: r.cols,
                                rows: r.rows,
                                bytes: lag_history.slice(r.start..r.end),
                            })
                            .collect();
                        let msg = DaemonStreamMessage::Lagged {
                            session_id: Cow::Borrowed(&session_id),
                            requested_after_sequence,
                            available_from_sequence,
                            start_sequence: att.snapshot.history_start_sequence,
                            end_sequence: att.snapshot.history_end_sequence,
                            history: lag_history,
                            segments,
                            replay_is_delta: Some(
                                requested_cursor.is_some() && att.snapshot.gap.is_none(),
                            ),
                        };
                        frame_buf.clear();
                        if serde_json::to_writer(&mut frame_buf, &msg).is_err() {
                            break;
                        }
                        frame_buf.push(b'\n');
                        if writer.write_all(&frame_buf).await.is_err() {
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
                    frame_buf.clear();
                    if serde_json::to_writer(&mut frame_buf, &msg).is_ok() {
                        frame_buf.push(b'\n');
                        let _ = writer.write_all(&frame_buf).await;
                        let _ = writer.flush().await;
                    }
                    break;
                }
            }
        }
    }
}

#[cfg(all(test, unix))]
#[path = "ssh_survival_tests.rs"]
mod ssh_survival_tests;

#[cfg(all(test, unix))]
#[path = "remote_ssh_tests.rs"]
mod remote_ssh_tests;

fn remote_spawn_relative_path(repo_root: &str, root: &str) -> String {
    let norm_repo = repo_root.replace('\\', "/");
    let norm_root = root.replace('\\', "/");

    let repo_parts: Vec<&str> = norm_repo.split('/').filter(|s| !s.is_empty()).collect();
    let root_parts: Vec<&str> = norm_root.split('/').filter(|s| !s.is_empty()).collect();

    if root_parts.len() < repo_parts.len() {
        return String::new();
    }

    let is_windows = (norm_repo.len() >= 2 && norm_repo.as_bytes()[1] == b':')
        || norm_repo.starts_with("//")
        || (norm_root.len() >= 2 && norm_root.as_bytes()[1] == b':')
        || norm_root.starts_with("//");

    for (repo_part, root_part) in repo_parts.iter().zip(root_parts.iter()) {
        if is_windows {
            if repo_part.to_lowercase() != root_part.to_lowercase() {
                return String::new();
            }
        } else if repo_part != root_part {
            return String::new();
        }
    }

    root_parts[repo_parts.len()..].join("/")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::terminal::output_hub::OutputChunk;
    use tempfile::tempdir;

    #[test]
    fn p08_remote_cwd_aliases_preserve_relative_path() {
        for (repo, cwd, expected) in [
            (r"C:\Repo", r"c:\repo\src", "src"),
            (r"C:\Repo", "C:/Repo/src", "src"),
            (r"C:\Repo", r"c:\repo\deep\nested", "deep/nested"),
            (r"C:\ẞẞ", r"c:\ßß\src", "src"),
            (r"\\host\share\Repo", "//HOST/share/repo/src", "src"),
            ("/repo", "/repo/src", "src"),
            ("/repo", "/repo", ""),
        ] {
            assert_eq!(
                remote_spawn_relative_path(repo, cwd),
                expected,
                "remote cwd {cwd} under {repo} must not silently select repository root"
            );
        }
    }

    #[test]
    fn resolve_upgrade_target_exe_only_resolves_paths_that_exist() {
        let live = std::env::current_exe().expect("test binary path");
        let deleted = PathBuf::from("/nonexistent/Ferryx.app.bak-deleted/Contents/MacOS/ferryx");

        let explicit_wins = {
            let live = live.clone();
            resolve_upgrade_target_exe_with(live.to_str(), || {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "current_exe unavailable",
                ))
            })
        };
        assert_eq!(explicit_wins.as_deref(), Some(live.as_path()));

        let falls_back = {
            let live = live.clone();
            resolve_upgrade_target_exe_with(deleted.to_str(), move || Ok(live.clone()))
        };
        assert_eq!(falls_back.as_deref(), Some(live.as_path()));

        // An orphaned daemon: current_exe names an unlinked image and the caller supplied
        // nothing usable. Resolving here would exec a path that cannot be launched.
        let orphaned = {
            let deleted = deleted.clone();
            resolve_upgrade_target_exe_with(None, move || Ok(deleted.clone()))
        };
        assert!(
            orphaned.is_none(),
            "must not resolve to an executable that no longer exists"
        );
    }

    struct AbortTask(tokio::task::JoinHandle<()>);

    impl Drop for AbortTask {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    fn init_test_git_repo() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output();
        dir
    }

    #[tokio::test]
    async fn test_serve_dag_stream_inventory_and_unsubscribe() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let temp = tempdir().expect("tempdir");
        let project_dir = temp.path().to_path_buf();
        let runs_dir = project_dir.join(".omo/senpi-task/dag/runs");
        std::fs::create_dir_all(&runs_dir).expect("create runs dir");

        let run_file = runs_dir.join("run-1.json");
        let run_content =
            include_str!("../dag/testdata/dag_081e597f-0aa8-4a20-a826-4e3d045aacef.json");
        std::fs::write(&run_file, run_content).expect("write run");

        let (mut client_duplex, server_duplex) = tokio::io::duplex(4096);
        let (read_half, mut write_half) = tokio::io::split(server_duplex);
        let mut reader = tokio::io::BufReader::new(read_half);

        let server = Arc::new(DaemonServer::new_with_paths(None, None));
        let ppath = project_dir.to_string_lossy().to_string();
        let server_task = {
            let server = Arc::clone(&server);
            let ppath = ppath.clone();
            tokio::spawn(async move {
                server
                    .serve_dag_stream(&mut reader, &mut write_half, "ws-test".to_string(), ppath)
                    .await;
            })
        };

        let mut client_reader = tokio::io::BufReader::new(&mut client_duplex);
        let mut inv_line = String::new();
        tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            client_reader.read_line(&mut inv_line),
        )
        .await
        .expect("read inventory timeout")
        .expect("read inventory line");

        let inv_msg: DaemonStreamMessage =
            serde_json::from_str(&inv_line).expect("parse inventory");
        match inv_msg {
            DaemonStreamMessage::DagInventory {
                workspace_id,
                project_path,
                runs,
            } => {
                assert_eq!(workspace_id, "ws-test");
                assert_eq!(project_path, ppath);
                assert_eq!(runs.len(), 1);
                assert_eq!(runs[0].run_id, "dag_081e597f-0aa8-4a20-a826-4e3d045aacef");
            }
            _ => panic!("Expected DagInventory message, got: {:?}", inv_msg),
        }

        let unsub_req = DaemonRequest::UnsubscribeDag {
            workspace_id: "ws-test".to_string(),
            project_path: ppath,
        };
        let mut unsub_line = serde_json::to_string(&unsub_req).unwrap();
        unsub_line.push('\n');
        client_reader
            .get_mut()
            .write_all(unsub_line.as_bytes())
            .await
            .expect("write unsub");
        client_reader.get_mut().flush().await.expect("flush unsub");

        tokio::time::timeout(tokio::time::Duration::from_secs(3), async {
            let mut lines = client_reader.lines();
            loop {
                let line = lines.next_line().await.expect("read unsubscribe frame")
                    .expect("stream closed before unsubscribe acknowledgement");
                if matches!(serde_json::from_str::<DaemonResponse>(&line),
                    Ok(DaemonResponse::UnsubscribeDagOk)) {
                    break;
                }
                let frame: DaemonStreamMessage = serde_json::from_str(&line)
                    .expect("expected DAG update before unsubscribe acknowledgement");
                assert!(matches!(frame, DaemonStreamMessage::DagRunUpdated { .. }));
            }
        }).await.expect("unsubscribe acknowledgement timeout");

        tokio::time::timeout(tokio::time::Duration::from_secs(3), server_task)
            .await
            .expect("server task exit timeout")
            .expect("server task join");
    }

    #[tokio::test]
    async fn test_p14_daemon_wires_paired_host_inventory_event_sink() {
        // P14: inventory transitions that originate inside the daemon-owned
        // PairedHostService must reach the desktop through the remote-event
        // broadcast (lib.rs's bridge re-emits paired_host_inventory_changed).
        // The constructor must therefore install the sink in production.
        let server = DaemonServer::new_with_paths(None, None);
        assert!(
            server.paired_hosts.has_event_sink(),
            "daemon-owned PairedHostService must have a production event sink wired"
        );
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
            bytes: b"hello pty stream\n".to_vec().into(),
            metrics_read_unix_micros: None,
            replay_gap: None,
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
    async fn test_server_unregister_workspace_is_idempotent_and_revokes_registration() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        let repo_path = repo.path().to_str().unwrap();

        assert!(server
            .handle_register_workspace("ws-revoke", repo_path)
            .is_ok());
        assert!(server.workspace_registry.contains("ws-revoke"));

        // Revocation removes the binding so the remote gateway (which resolves
        // against this registry) can no longer address the workspace.
        server
            .handle_unregister_workspace("ws-revoke")
            .await
            .expect("unregister of a registered workspace succeeds");
        assert!(!server.workspace_registry.contains("ws-revoke"));

        // Idempotent: unregistering an unknown workspace is a success no-op
        // (e.g. after a daemon restart dropped its registry).
        server
            .handle_unregister_workspace("ws-revoke")
            .await
            .expect("unregister of an unknown workspace is a no-op success");
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
    async fn test_omo_resume_cwd_cannot_escape_workspace() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        let outside = tempdir().unwrap();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();
        let transcript = outside.path().join("session.jsonl");
        fs::write(
            &transcript,
            format!(
                "{}\n",
                serde_json::json!({
                    "type": "session", "id": "provider-session", "cwd": outside.path(),
                })
            ),
        )
        .unwrap();
        let result = server
            .handle_spawn(
                "omo-outside-workspace",
                "default",
                None,
                Some(repo.path().to_string_lossy().into_owned()),
                80,
                24,
                None,
                Some(TerminalStartup::AgentResume {
                    agent_type: "omo".to_string(),
                    provider_session: crate::daemon::protocol::AgentProviderSession {
                        key: AgentProviderSessionKey::SessionId,
                        id: "provider-session".to_string(),
                        transcript_path: Some(transcript.to_string_lossy().into_owned()),
                    },
                }),
            )
            .await;
        assert!(matches!(result, Err(SpawnError::Other(_))));
        assert!(server.terminal_service().list_sessions().is_empty());
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
        struct RestoreRuntimeDir(Option<std::ffi::OsString>);
        impl Drop for RestoreRuntimeDir {
            fn drop(&mut self) {
                match &self.0 {
                    Some(value) => std::env::set_var("FERRYX_RUNTIME_DIR", value),
                    None => std::env::remove_var("FERRYX_RUNTIME_DIR"),
                }
            }
        }
        // This Unix-only test reads no other environment overrides. Restore even
        // on assertion failure; environment-mutating tests run serially.
        let _restore = RestoreRuntimeDir(std::env::var_os("FERRYX_RUNTIME_DIR"));
        std::env::remove_var("FERRYX_RUNTIME_DIR");
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
            relay_url: None,
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
        // The bearer travels in the Authorization header. A device token in the URL
        // is refused, because it would persist in access logs and browser history.
        let request = format!(
            "POST /api/v1/workspace/select HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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

        // 2. Configure to LocalNetwork and create pairing code
        line.clear();
        let conf_req = DaemonRequest::RemoteConfigure {
            config: crate::remote::state::RemoteGatewayConfig {
                mode: RemoteNetworkMode::LocalNetwork,
                port: 0,
                allow_control: true,
                relay_url: None,
            },
        };
        let mut json = serde_json::to_string(&conf_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::RemoteConfigureOk));

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
            DaemonResponse::RemotePairingCodeOk { code, .. } => {
                assert_eq!(code.len(), 6);
            }
            other => panic!("Expected RemotePairingCodeOk, got {other:?}"),
        }

        // 3. Active selection set & get
        line.clear();
        let sel_req = DaemonRequest::RemoteSetActiveSelection {
            ssh_store_path: None,
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
    async fn test_remote_create_pairing_code_relay_unreachable_returns_error() {
        let server = Arc::new(DaemonServer::new());

        // Set mode to Relay with an unreachable relay URL
        *server.remote_state.config.write() = RemoteGatewayConfig {
            mode: RemoteNetworkMode::Relay,
            port: 0,
            relay_url: Some("http://127.0.0.1:1".to_string()),
            ..Default::default()
        };

        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let pair_req = DaemonRequest::RemoteCreatePairingCode {
            permission: Some(DevicePermission::Control),
        };
        let mut json = serde_json::to_string(&pair_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::Error { message, .. } => {
                assert!(
                    message.contains("Relay is unreachable")
                        || message.contains("registration failed"),
                    "unexpected error message: {message}"
                );
            }
            other => panic!("Expected Error, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_remote_create_pairing_code_fresh_off_auto_configures_relay() {
        use crate::remote::auth::DeviceAccessScope;

        let temp_dir = tempfile::tempdir().unwrap();
        let relay_state = crate::remote::relay_server::RelayState::new_with_key_store(
            vec![],
            temp_dir.path().join("keys.json"),
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_url = format!("http://{}", listener.local_addr().unwrap());
        let relay_task = tokio::spawn(async move {
            axum::serve(
                listener,
                crate::remote::relay_server::relay_router(relay_state)
                    .into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .expect("serve relay");
        });

        for (pair_req, scope) in [
            (
                DaemonRequest::RemoteCreatePairingCode {
                    permission: Some(DevicePermission::Control),
                },
                DeviceAccessScope::Mirror,
            ),
            (
                DaemonRequest::RemoteCreateMachinePairingCode,
                DeviceAccessScope::Machine,
            ),
        ] {
            let server = Arc::new(DaemonServer::new());
            assert_eq!(
                server.remote_state.config.read().mode,
                RemoteNetworkMode::Off
            );
            // Only substitute the relay endpoint; pairing must start the gateway.
            server.remote_state.config.write().relay_url = Some(relay_url.clone());
            server.remote_state.config.write().port = 0;

            let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
            let server_clone = Arc::clone(&server);
            let server_task = tokio::spawn(async move {
                server_clone.handle_client(server_stream).await;
            });

            let (read_half, mut write_half) = client_stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            let mut json = serde_json::to_string(&pair_req).unwrap();
            json.push('\n');
            write_half.write_all(json.as_bytes()).await.unwrap();

            reader.read_line(&mut line).await.unwrap();
            let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();

            assert_eq!(
                server.remote_state.config.read().mode,
                RemoteNetworkMode::Relay
            );
            assert_eq!(
                server.remote_state.config.read().relay_url,
                Some(relay_url.clone())
            );

            match resp {
                DaemonResponse::RemotePairingCodeOk {
                    code,
                    pairing_token,
                    machine_id,
                    relay_url: effective_url,
                } => {
                    assert!(pairing_token.is_some(), "must not be a local-only PIN");
                    assert!(machine_id.is_some());
                    assert_eq!(effective_url, Some(relay_url.clone()));
                    let (_, device) = server
                        .remote_state
                        .auth_manager
                        .exchange_pairing_code(&code, "Paired device")
                        .expect("pair");
                    assert_eq!(device.access_scope, scope);
                }
                other => panic!("Expected RemotePairingCodeOk, got {other:?}"),
            }

            drop(write_half);
            server_task.await.expect("client task");
        }
        relay_task.abort();
    }

    #[tokio::test]
    async fn test_remote_create_pairing_code_local_mode_creates_local_pin() {
        for mode in [
            RemoteNetworkMode::LocalNetwork,
            RemoteNetworkMode::Tailscale,
        ] {
            let server = Arc::new(DaemonServer::new());
            *server.remote_state.config.write() = RemoteGatewayConfig {
                mode,
                port: 0,
                ..Default::default()
            };

            let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
            let server_clone = Arc::clone(&server);
            let server_task = tokio::spawn(async move {
                server_clone.handle_client(server_stream).await;
            });

            let (read_half, mut write_half) = client_stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            let pair_req = DaemonRequest::RemoteCreatePairingCode {
                permission: Some(DevicePermission::Control),
            };
            let mut json = serde_json::to_string(&pair_req).unwrap();
            json.push('\n');
            write_half.write_all(json.as_bytes()).await.unwrap();

            reader.read_line(&mut line).await.unwrap();
            let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
            match resp {
                DaemonResponse::RemotePairingCodeOk {
                    code,
                    pairing_token,
                    machine_id,
                    relay_url,
                } => {
                    assert_eq!(code.len(), 6);
                    assert_eq!(pairing_token, None);
                    assert_eq!(machine_id, None);
                    assert_eq!(relay_url, None);
                }
                other => panic!("Expected RemotePairingCodeOk for mode {mode:?}, got {other:?}"),
            }
            assert_eq!(server.remote_state.config.read().mode, mode);
            assert_eq!(server.remote_state.config.read().relay_url, None);

            drop(write_half);
            let _ = server_task.await;
        }
    }

    #[tokio::test]
    async fn test_remote_create_pairing_code_with_relay_returns_effective_relay_url() {
        let temp_dir = tempfile::tempdir().unwrap();
        let relay_state = crate::remote::relay_server::RelayState::new_with_key_store(
            vec![],
            temp_dir.path().join("keys.json"),
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_relay_url = format!("http://{}", listener.local_addr().unwrap());
        let relay_task = tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                crate::remote::relay_server::relay_router(relay_state)
                    .into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await;
        });

        let server = Arc::new(DaemonServer::new());
        // Configure Relay with the mock relay URL
        let config = RemoteGatewayConfig {
            mode: RemoteNetworkMode::Relay,
            port: 0,
            relay_url: Some(mock_relay_url.clone()),
            ..Default::default()
        };
        server
            .handle_remote_configure(config)
            .await
            .expect("configure gateway");

        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let pair_req = DaemonRequest::RemoteCreatePairingCode {
            permission: Some(DevicePermission::Control),
        };
        let mut json = serde_json::to_string(&pair_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemotePairingCodeOk {
                code,
                pairing_token,
                machine_id,
                relay_url,
            } => {
                assert_eq!(code.len(), 6);
                assert!(pairing_token.is_some());
                assert!(machine_id.is_some());
                assert_eq!(relay_url, Some(mock_relay_url));
            }
            DaemonResponse::Error { message, .. } => {
                panic!("Expected RemotePairingCodeOk, got Error: {message}");
            }
            other => panic!("Expected RemotePairingCodeOk, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
        relay_task.abort();
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
            token: None,
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
                daemon_version,
            } => {
                assert_eq!(version, DAEMON_PROTOCOL_VERSION);
                assert_eq!(pid, std::process::id());
                assert_eq!(epoch, server.epoch());
                assert!(binary_path.is_some());
                assert!(binary_mtime_ms.is_some());
                assert!(daemon_version.is_some());
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

        let hs = DaemonRequest::Handshake {
            version: 9999,
            token: None,
        };
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
            DaemonResponse::Error { message, .. } => {
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
    async fn test_server_reset_agent_state_request() {
        let server = Arc::new(DaemonServer::new());
        let session_id = "test-reset-session".to_string();
        server.agent_states.publish_canonical(AgentState {
            session_id: session_id.clone(),
            state: "working".to_string(),
            agent: Some("omo".to_string()),
            provider_session: None,
            origin: crate::daemon::protocol::AgentStateOrigin::Agent,
        });
        assert_eq!(
            server.agent_states.current(&session_id).unwrap().state,
            "working"
        );

        // Subscribe to exact state change BEFORE triggering reset
        let mut sub = server.agent_states.subscribe(&session_id);

        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = tokio::io::split(client_stream);
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let req = DaemonRequest::ResetAgentState {
            session_id: session_id.clone(),
        };
        let mut req_json = serde_json::to_string(&req).unwrap();
        req_json.push('\n');

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            write_half.write_all(req_json.as_bytes()).await.unwrap();
            write_half.flush().await.unwrap();
            reader.read_line(&mut line).await.unwrap();
        })
        .await
        .expect("reset request/response within timeout");

        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::ResetAgentStateOk));

        // Await the exact broadcast update with bounded timeout
        let update = tokio::time::timeout(std::time::Duration::from_secs(2), sub.receiver.recv())
            .await
            .expect("update signal within timeout")
            .expect("valid update");
        assert_eq!(update.state.state, "idle");
        assert_eq!(
            server.agent_states.current(&session_id).unwrap().state,
            "idle"
        );

        server_task.abort();
    }

    #[tokio::test]
    async fn agent_state_report_reaches_only_its_own_session_stream() {
        let server = Arc::new(DaemonServer::new());
        let (client, mut server_side) = tokio::io::duplex(4096);
        let session_id = "session-under-test".to_string();
        let other_session = "unrelated-session".to_string();

        let hub = Arc::clone(server.terminal_service().output_hub());
        let (_raw_rx, rx) = hub.register_session_channels(&session_id);
        let agent_rx = server.agent_states.subscribe(&session_id);

        let pump = tokio::spawn(DaemonServer::pump_sequenced_stream_with_agent_state(
            session_id.clone(),
            rx,
            hub,
            client,
            Some(agent_rx),
            None::<tokio::io::Empty>,
        ));

        // Send the foreign report FIRST, then this session's own report. The pump processes the
        // broadcast in order, so the first frame that arrives is decisive: with a correct filter it
        // is this session's report, and a leak shows up as the foreign one arriving ahead of it.
        // This orders the assertion on the channel itself instead of on elapsed time.
        server.agent_states.publish_canonical(AgentState {
            session_id: other_session.clone(),
            state: "working".to_string(),
            agent: Some("codex".to_string()),
            provider_session: None,
            origin: crate::daemon::protocol::AgentStateOrigin::Agent,
        });
        server.agent_states.publish_canonical(AgentState {
            session_id: session_id.clone(),
            state: "blocked".to_string(),
            agent: Some("omo".to_string()),
            provider_session: None,
            origin: crate::daemon::protocol::AgentStateOrigin::Agent,
        });

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

    #[tokio::test]
    async fn attach_resynchronizes_agent_state_reported_before_subscription() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();
        let session_id = server
            .handle_spawn(
                "req-agent-snapshot",
                "default",
                None,
                None,
                80,
                24,
                None,
                None,
            )
            .await
            .expect("spawn test session");

        let result: Result<serde_json::Value, String> = async {
            // This is the same hub publication used by the production agent-state socket handler.
            // The report deliberately arrives before handle_client subscribes for Attach.
            server.agent_states.publish_canonical(AgentState {
                session_id: session_id.clone(),
                state: "working".to_string(),
                agent: Some("omo".to_string()),
                provider_session: None,
                origin: crate::daemon::protocol::AgentStateOrigin::Agent,
            });

            let (client_stream, server_stream) = UnixStream::pair().map_err(|e| e.to_string())?;
            let server_task = AbortTask(tokio::spawn(
                Arc::clone(&server).handle_client(server_stream),
            ));
            let (read_half, mut write_half) = client_stream.into_split();
            let mut reader = BufReader::new(read_half);

            for request in [
                DaemonRequest::Handshake {
                    version: DAEMON_PROTOCOL_VERSION,
                    token: None,
                },
                DaemonRequest::Attach {
                    session_id: session_id.clone(),
                    after_sequence: None,
                },
            ] {
                let mut frame = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
                frame.push(b'\n');
                write_half
                    .write_all(&frame)
                    .await
                    .map_err(|e| e.to_string())?;
                write_half.flush().await.map_err(|e| e.to_string())?;

                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .await
                    .map_err(|e| e.to_string())?;
                let response: DaemonResponse =
                    serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
                if !matches!(
                    (request, response),
                    (
                        DaemonRequest::Handshake { .. },
                        DaemonResponse::HandshakeOk { .. }
                    ) | (
                        DaemonRequest::Attach { .. },
                        DaemonResponse::AttachOk { .. }
                    )
                ) {
                    return Err("unexpected attach response".to_string());
                }
            }

            let snapshot = tokio::time::timeout(tokio::time::Duration::from_secs(10), async {
                loop {
                    let mut line = String::new();
                    let read = reader
                        .read_line(&mut line)
                        .await
                        .map_err(|e| e.to_string())?;
                    if read == 0 {
                        return Err("attach stream closed before agent snapshot".to_string());
                    }
                    let value: serde_json::Value =
                        serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
                    if value["type"] == "agentState" {
                        return Ok(value);
                    }
                }
            })
            .await
            .map_err(|_| {
                "attach must receive retained agent state without another report".to_string()
            })??;

            drop(write_half);
            drop(server_task);
            Ok(snapshot)
        }
        .await;

        // Cleanup runs before any assertion, including timeout and malformed-frame failures.
        server
            .handle_close(&session_id)
            .await
            .expect("close test session");

        let snapshot = result.expect("attach state snapshot");
        assert_eq!(snapshot["sessionId"], session_id);
        assert_eq!(snapshot["state"], "working");
        assert_eq!(snapshot["isSnapshot"], true);
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
        let mock_exe_err = || {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "not found",
            ))
        };
        let (path_err, mtime_err) = resolve_binary_identity_with(mock_exe_err, mock_mtime);
        assert_eq!(path_err, None);
        assert_eq!(mtime_err, None);

        // Injection test: mtime failure
        let mock_mtime_none = |_path: &Path| None;
        let (path_no_mtime, mtime_none) = resolve_binary_identity_with(mock_exe, mock_mtime_none);
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
        let resp = server.handle_upgrade_binary(None).await;
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
        let set_cloexec_ret = unsafe { libc::fcntl(read_fd, libc::F_SETFD, libc::FD_CLOEXEC) };
        assert_eq!(set_cloexec_ret, 0);

        let flags = unsafe { libc::fcntl(read_fd, libc::F_GETFD) };
        assert_ne!(flags & libc::FD_CLOEXEC, 0, "FD_CLOEXEC must be set");

        // 3. Clear FD_CLOEXEC using clear_cloexec
        clear_cloexec(read_fd).expect("clear_cloexec must succeed");

        let flags_cleared = unsafe { libc::fcntl(read_fd, libc::F_GETFD) };
        assert_eq!(
            flags_cleared & libc::FD_CLOEXEC,
            0,
            "FD_CLOEXEC must be cleared"
        );

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

        unsafe {
            libc::close(read_fd);
        }

        assert!(output.status.success());
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout_str.trim(), "ferryx-inherited-fd-payload");
    }

    #[tokio::test]
    async fn test_daemon_production_configure_gated_status_projection() {
        let _guard = crate::remote::server::DIRECT_GATE_TEST_MUTEX.lock().await;
        use crate::remote::server::set_allow_insecure_direct;
        set_allow_insecure_direct(false);

        let server = Arc::new(DaemonServer::new());
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        // Configure to LocalNetwork without insecure opt-in
        let conf_req = DaemonRequest::RemoteConfigure {
            config: crate::remote::state::RemoteGatewayConfig {
                mode: RemoteNetworkMode::LocalNetwork,
                port: 0,
                allow_control: true,
                relay_url: None,
            },
        };
        let mut json = serde_json::to_string(&conf_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::RemoteConfigureOk));

        // Now query RemoteGetStatus: must expose gate status + reason
        line.clear();
        let req = DaemonRequest::RemoteGetStatus;
        let mut json = serde_json::to_string(&req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemoteStatusOk { status } => {
                assert_eq!(status.mode, RemoteNetworkMode::LocalNetwork);
                assert!(status.is_running);
                assert!(
                    matches!(
                        status.gate_status,
                        Some(
                            crate::remote::server::DirectGatewayGateStatus::InsecureLanGated { .. }
                        )
                    ),
                    "expected InsecureLanGated in status response, got {:?}",
                    status.gate_status
                );
                assert!(
                    status
                        .gate_reason
                        .as_deref()
                        .unwrap_or_default()
                        .contains("gated"),
                    "expected gating reason in status response, got {:?}",
                    status.gate_reason
                );
            }
            other => panic!("Expected RemoteStatusOk, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_idle_stream_client_disconnect_terminates_pump() {
        let (client_read_side, server_write_side) = tokio::io::duplex(4096);
        let (server_read_side, client_write_side) = tokio::io::duplex(4096);

        let (_tx, rx) = broadcast::channel(16);
        let hub = Arc::new(TerminalOutputHub::default());
        let session_id = "test-idle-session".to_string();

        let pump_handle = tokio::spawn(DaemonServer::pump_sequenced_stream_with_agent_state(
            session_id,
            rx,
            hub,
            server_write_side,
            None,
            Some(server_read_side),
        ));

        // Terminal is completely idle: no output sent.
        // Client drops write and read ends of socket (e.g. GUI tab close or abort).
        drop(client_write_side);
        drop(client_read_side);

        // Assert that the pump terminates within 2 seconds instead of hanging forever.
        let result = tokio::time::timeout(Duration::from_secs(2), pump_handle).await;
        assert!(
            result.is_ok(),
            "pump must terminate promptly when client disconnects on idle stream"
        );
    }

    #[tokio::test]
    async fn test_idle_stream_handle_client_attach_disconnect_terminates() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        let session_id = server
            .handle_spawn("req-idle-1", "default", None, None, 80, 24, None, None)
            .await
            .unwrap();

        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = tokio::io::split(client_stream);
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
            token: None,
        };
        let mut hs_json = serde_json::to_string(&hs).unwrap();
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await.unwrap();
        write_half.flush().await.unwrap();
        reader.read_line(&mut line).await.unwrap();

        let attach = DaemonRequest::Attach {
            session_id: session_id.clone(),
            after_sequence: None,
        };
        let mut attach_json = serde_json::to_string(&attach).unwrap();
        attach_json.push('\n');
        write_half.write_all(attach_json.as_bytes()).await.unwrap();
        write_half.flush().await.unwrap();
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::AttachOk { .. }));

        // Now session is idle (no output produced). Drop client stream.
        drop(reader);
        drop(write_half);

        let result = tokio::time::timeout(Duration::from_secs(2), server_task).await;
        assert!(
            result.is_ok(),
            "handle_client must terminate promptly when client disconnects from idle attach"
        );
    }

    #[tokio::test]
    async fn test_handle_client_upload_clipboard_image() {
        let server = Arc::new(DaemonServer::new_with_paths(None, None));
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let _server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let req = DaemonRequest::UploadClipboardImage {
            file_name: "test-client-paste.png".to_string(),
            data: b"fake-daemon-paste-bytes".to_vec(),
        };
        let mut json = serde_json::to_string(&req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::UploadClipboardImageOk {
                remote_path,
                byte_length,
            } => {
                assert!(remote_path.ends_with("test-client-paste.png"));
                assert_eq!(byte_length, 23);
                let content = std::fs::read(&remote_path).expect("read saved paste file");
                assert_eq!(content, b"fake-daemon-paste-bytes");
                let _ = std::fs::remove_file(remote_path);
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_legacy_peer_attach_and_stream_client_eof_drops_legacy_connection() {
        use std::os::unix::fs::PermissionsExt;
        use tokio::net::UnixListener;

        let socket_dir = tempfile::Builder::new()
            .prefix("fx-legacy")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(socket_dir.path(), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        let socket_path = socket_dir.path().join("legacy.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        let (legacy_closed_tx, legacy_closed_rx) = tokio::sync::oneshot::channel();
        let legacy_daemon = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut lines = BufReader::new(read).lines();

            // Handshake
            let line = lines.next_line().await.unwrap().unwrap();
            assert!(line.contains("handshake"));
            write
                .write_all(b"{\"type\":\"handshakeOk\",\"version\":3,\"pid\":1,\"epoch\":1}\n")
                .await
                .unwrap();

            // Attach
            let line = lines.next_line().await.unwrap().unwrap();
            assert!(line.contains("attach"));
            write.write_all(b"{\"type\":\"attachOk\",\"epoch\":1,\"sessionId\":\"legacy-1\",\"startSequence\":null,\"endSequence\":null,\"gap\":null,\"history\":\"\"}\n").await.unwrap();

            // The legacy daemon is now idle, waiting for commands or connection close.
            // When LegacyPeer drops its socket, next_line() returns None (EOF).
            let eof = lines.next_line().await.unwrap();
            assert!(
                eof.is_none(),
                "legacy daemon must observe EOF when client disconnects"
            );
            let _ = legacy_closed_tx.send(());
        });

        let peer = Arc::new(crate::daemon::proxy::LegacyPeer::new(
            socket_path,
            vec!["legacy-1".into()],
        ));
        let agent_hub = Arc::new(AgentStateHub::default());
        let agent_sub = agent_hub.subscribe("legacy-1");

        let (client_writer_read, mut client_writer) = tokio::io::duplex(4096);
        let (client_reader_write, mut client_reader) = tokio::io::duplex(4096);

        let peer_clone = Arc::clone(&peer);
        let stream_task = tokio::spawn(async move {
            peer_clone
                .attach_and_stream(
                    "legacy-1",
                    None,
                    &mut client_writer,
                    &mut client_reader,
                    agent_sub,
                    agent_hub,
                )
                .await
        });

        // Client reads the forwarded AttachOk frame
        let mut client_lines = BufReader::new(client_writer_read).lines();
        let first_line = client_lines.next_line().await.unwrap().unwrap();
        assert!(first_line.contains("attachOk"));

        // Now client disconnects (drops write end, sending EOF to client_reader)
        drop(client_reader_write);

        // attach_and_stream must terminate promptly
        let stream_res = tokio::time::timeout(Duration::from_secs(2), stream_task).await;
        assert!(
            stream_res.is_ok(),
            "attach_and_stream must terminate within timeout"
        );
        assert!(stream_res.unwrap().unwrap().is_ok());

        // Legacy daemon must have observed EOF and closed its socket within timeout
        let legacy_res = tokio::time::timeout(Duration::from_secs(2), legacy_closed_rx).await;
        assert!(
            legacy_res.is_ok(),
            "upstream connection to legacy daemon must be closed promptly"
        );

        legacy_daemon.await.unwrap();
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_legacy_peer_attach_session_receiver_drop_terminates_reader() {
        use std::os::unix::fs::PermissionsExt;
        use tokio::net::UnixListener;

        let socket_dir = tempfile::Builder::new()
            .prefix("fx-legacy")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(socket_dir.path(), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        let socket_path = socket_dir.path().join("legacy_session.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        let (legacy_closed_tx, legacy_closed_rx) = tokio::sync::oneshot::channel();
        let legacy_daemon = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut lines = BufReader::new(read).lines();

            let line = lines.next_line().await.unwrap().unwrap();
            assert!(line.contains("handshake"));
            write
                .write_all(b"{\"type\":\"handshakeOk\",\"version\":3,\"pid\":1,\"epoch\":1}\n")
                .await
                .unwrap();

            let line = lines.next_line().await.unwrap().unwrap();
            assert!(line.contains("attach"));
            write.write_all(b"{\"type\":\"attachOk\",\"epoch\":1,\"sessionId\":\"legacy-session-1\",\"startSequence\":1,\"endSequence\":1,\"gap\":null,\"history\":\"\"}\n").await.unwrap();

            // Send output periodically until EOF
            let eof = loop {
                if write.write_all(b"{\"type\":\"output\",\"sessionId\":\"legacy-session-1\",\"sequence\":2,\"data\":\"aGVsbG8=\"}\n").await.is_err() {
                    break true;
                }
                match lines.next_line().await {
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => break true,
                }
            };
            assert!(eof);
            let _ = legacy_closed_tx.send(());
        });

        let peer = Arc::new(crate::daemon::proxy::LegacyPeer::new(
            socket_path,
            vec!["legacy-session-1".into()],
        ));
        let attachment = peer.attach_session("legacy-session-1", None).await.unwrap();

        // Drop the receiver, making receiver_count() == 0
        drop(attachment.receiver);

        // Within 2 seconds, the legacy connection should be closed because reader task breaks
        let legacy_res = tokio::time::timeout(Duration::from_secs(2), legacy_closed_rx).await;
        assert!(
            legacy_res.is_ok(),
            "dropping receiver must cause legacy peer reader task to drop connection"
        );

        legacy_daemon.await.unwrap();
    }
}

#[cfg(test)]
mod instance_lock_deadline_tests {
    use super::*;

    #[test]
    fn expired_wait_window_never_attempts_or_acquires_instance_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("instance.lock");

        // 1. Even if the lock file is completely free, calling lock_file_with_optional_wait
        // with an already-expired deadline must return Err immediately without acquiring.
        // If expiry were checked after attempt (the bug), try_lock_file would succeed and return Ok.
        let expired_deadline = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(10))
            .unwrap();
        let outcome = lock_file_with_optional_wait(&path, Some(expired_deadline));
        assert!(
            outcome.is_err(),
            "an expired wait window must never acquire the lock, even when free"
        );

        // 2. Predecessor holds lock past the successor's deadline:
        // Successor must wait the full window and timeout with Err.
        let held = try_lock_file(open_secure_lock_file(&path).unwrap()).unwrap();
        let started = std::time::Instant::now();
        let outcome = lock_file_with_optional_wait(
            &path,
            Some(std::time::Instant::now() + std::time::Duration::from_millis(200)),
        );
        assert!(
            outcome.is_err(),
            "successor must timeout when predecessor holds past deadline"
        );
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(180),
            "wait must last until deadline, took {:?}",
            started.elapsed()
        );
        drop(held);

        // 3. Without a declared successor deadline, plain startup fails fast on a held lock.
        let held_again = try_lock_file(open_secure_lock_file(&path).unwrap()).unwrap();
        let fast = std::time::Instant::now();
        let immediate = lock_file_with_optional_wait(&path, None);
        assert!(immediate.is_err(), "plain startup must fail on held lock");
        assert!(
            fast.elapsed() < std::time::Duration::from_millis(100),
            "plain startup must fail fast, took {:?}",
            fast.elapsed()
        );
        drop(held_again);

        // 4. Predecessor releases WITHIN deadline: successor acquires successfully.
        let held_third = try_lock_file(open_secure_lock_file(&path).unwrap()).unwrap();
        let release_thread = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            drop(held_third);
        });
        let acquired = lock_file_with_optional_wait(
            &path,
            Some(std::time::Instant::now() + std::time::Duration::from_millis(400)),
        );
        assert!(
            acquired.is_ok(),
            "successor must acquire lock when predecessor releases within deadline"
        );
        release_thread.join().unwrap();
        drop(acquired);

        // 5. Pre-check passes with remaining window, but delay before lock crosses deadline:
        // Post-acquisition check deterministically detects late lock, drops it immediately, and returns Err.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(20);
        let late_outcome = lock_file_with_optional_wait_internal(
            &path,
            Some(deadline),
            || std::thread::sleep(std::time::Duration::from_millis(30)),
        );
        assert!(
            late_outcome.is_err(),
            "lock acquired past deadline must be dropped and rejected"
        );
        let err_msg = late_outcome.unwrap_err();
        assert!(
            err_msg.contains("after the wait window expired"),
            "must be rejected specifically by the post-acquisition check, got: {err_msg}"
        );
        // Lock must have been dropped, so an immediate unarmed call succeeds.
        assert!(lock_file_with_optional_wait(&path, None).is_ok());
    }
}
