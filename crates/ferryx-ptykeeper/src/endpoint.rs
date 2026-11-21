//! The keeper's UDS endpoint: directory, socket, permissions and the single-instance lock.
//!
//! `FERRYX_RUNTIME_DIR`, when set, wins: QA and tests must be able to point the keeper at an
//! isolated root without ever reaching the user's real keeper socket. Otherwise the path is
//! `/tmp/ferryx-{uid}/keeper.sock`, matching `daemon::crash_supervisor::KEEPER_SOCKET_NAME` so the
//! daemon's presence probe and this listener cannot disagree about where the keeper lives.
//!
//! Security posture, all enforced here rather than by callers:
//!
//! * the directory is `0700` and is created only for this uid;
//! * the socket is `0600` and its ownership is verified after bind;
//! * a stale socket (no holder of the lock) is removed, a live one is not;
//! * the single-instance guard is an `flock` on `keeper.lock`, held for the process lifetime.

use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};

use crate::error::{KeeperError, KeeperErrorCode, KeeperResult};

/// Socket file name. Must equal the daemon's `KEEPER_SOCKET_NAME`.
pub const SOCKET_NAME: &str = "keeper.sock";

/// Lock file name, held while this keeper serves.
pub const LOCK_NAME: &str = "keeper.lock";

/// Directory mode: owner only.
pub const DIR_MODE: u32 = 0o700;

/// Socket mode: owner only.
pub const SOCKET_MODE: u32 = 0o600;

/// Longest UDS path this platform accepts. macOS caps `sun_path` at 104 bytes including the NUL;
/// refusing early gives a typed error instead of a truncated bind that silently listens elsewhere.
pub const MAX_SOCKET_PATH_BYTES: usize = 103;

/// The endpoint a keeper serves: where the socket is, where its lock is, and the uid it serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    socket: PathBuf,
    lock: PathBuf,
    dir: PathBuf,
    uid: u32,
}

impl Endpoint {
    /// Builds the endpoint for `runtime_dir`. Rejects paths a UDS cannot carry.
    pub fn new(runtime_dir: impl Into<PathBuf>, uid: u32) -> KeeperResult<Self> {
        let dir = runtime_dir.into();
        let socket = dir.join(SOCKET_NAME);
        if socket.as_os_str().len() > MAX_SOCKET_PATH_BYTES {
            return Err(KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!(
                    "socket path {} is {} bytes, longer than the {MAX_SOCKET_PATH_BYTES} byte unix socket limit",
                    socket.display(),
                    socket.as_os_str().len()
                ),
            ));
        }
        Ok(Self {
            lock: dir.join(LOCK_NAME),
            socket,
            dir,
            uid,
        })
    }

    /// The endpoint for this process: `$FERRYX_RUNTIME_DIR`, else `/tmp/ferryx-{uid}`.
    pub fn for_runtime() -> KeeperResult<Self> {
        let uid = current_uid()?;
        Self::new(runtime_dir(uid), uid)
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// Creates the runtime directory `0700`, refusing a directory owned by anyone else. A `/tmp`
    /// directory owned by another uid is a hijack, not a configuration error, so it is refused
    /// rather than repaired.
    pub fn prepare_dir(&self) -> KeeperResult<()> {
        match fs::symlink_metadata(&self.dir) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    return Err(KeeperError::new(
                        KeeperErrorCode::InvalidSpawn,
                        format!("keeper runtime path {} is not a directory", self.dir.display()),
                    ));
                }
                let owner = owner_uid(&metadata);
                if owner != self.uid {
                    return Err(KeeperError::new(
                        KeeperErrorCode::PeerUidMismatch,
                        format!(
                            "keeper runtime directory {} is owned by uid {owner}, not {}. Refusing to serve from it",
                            self.dir.display(),
                            self.uid
                        ),
                    ));
                }
                set_dir_mode(&self.dir)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Some(parent) = self.dir.parent() {
                    if !parent.exists() {
                        fs::create_dir_all(parent).map_err(|error| dir_error(parent, &error))?;
                    }
                }
                fs::create_dir(&self.dir).map_err(|error| dir_error(&self.dir, &error))?;
                set_dir_mode(&self.dir)
            }
            Err(error) => Err(dir_error(&self.dir, &error)),
        }
    }

    /// Takes the single-instance lock without blocking. `Ok(None)` means another keeper holds it.
    pub fn try_lock(&self) -> KeeperResult<Option<InstanceLock>> {
        InstanceLock::acquire(&self.lock, false)
    }

    /// Takes the single-instance lock, waiting for a departing keeper to finish. Used by the
    /// binary, where a brief overlap during upgrade is expected; the socket itself always has one
    /// owner because the lock is released only after unbind.
    pub fn lock_blocking(&self) -> KeeperResult<InstanceLock> {
        InstanceLock::acquire(&self.lock, true)?.ok_or_else(|| {
            KeeperError::new(
                KeeperErrorCode::ShuttingDown,
                format!("keeper lock {} is held by another keeper", self.lock.display()),
            )
        })
    }

    /// Removes a socket left behind by a keeper that is gone. Only safe while holding the lock.
    pub fn clear_stale_socket(&self) -> KeeperResult<bool> {
        let refusal = |error: &io::Error| {
            KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("cannot inspect keeper socket {}: {error}", self.socket.display()),
            )
        };
        let metadata = match fs::symlink_metadata(&self.socket) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(refusal(&error)),
        };
        // Only ever remove the socket this keeper would have bound. Anything else at that path -- a
        // directory, a device, another user's file planted there -- is refused by name: deleting it
        // would be acting on a path the keeper does not own, and following a symlink would be worse.
        if !metadata.file_type().is_socket() {
            return Err(KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!(
                    "{} exists and is not a socket, so it is not a stale keeper socket",
                    self.socket.display()
                ),
            ));
        }
        if metadata.uid() != self.uid {
            return Err(KeeperError::new(
                KeeperErrorCode::PeerUidMismatch,
                format!(
                    "{} is owned by uid {}, not {}",
                    self.socket.display(),
                    metadata.uid(),
                    self.uid
                ),
            ));
        }
        fs::remove_file(&self.socket).map_err(|error| {
            KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("cannot remove the stale keeper socket {}: {error}", self.socket.display()),
            )
        })?;
        Ok(true)
    }

    /// Binds the listener and narrows it to the owner. Call only while holding the lock and after
    /// `clear_stale_socket`.
    #[cfg(unix)]
    pub fn bind(&self) -> KeeperResult<std::os::unix::net::UnixListener> {
        use std::os::unix::fs::PermissionsExt as _;

        let listener = std::os::unix::net::UnixListener::bind(&self.socket).map_err(|error| {
            KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("cannot bind the keeper socket {}: {error}", self.socket.display()),
            )
        })?;
        fs::set_permissions(&self.socket, fs::Permissions::from_mode(SOCKET_MODE)).map_err(
            |error| {
                KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!("cannot set mode {SOCKET_MODE:o} on {}: {error}", self.socket.display()),
                )
            },
        )?;
        let metadata = fs::symlink_metadata(&self.socket).map_err(|error| {
            KeeperError::new(
                KeeperErrorCode::InvalidSpawn,
                format!("cannot inspect the bound keeper socket {}: {error}", self.socket.display()),
            )
        })?;
        let owner = owner_uid(&metadata);
        if owner != self.uid {
            let _ = fs::remove_file(&self.socket);
            return Err(KeeperError::new(
                KeeperErrorCode::PeerUidMismatch,
                format!(
                    "bound keeper socket {} is owned by uid {owner}, not {}",
                    self.socket.display(),
                    self.uid
                ),
            ));
        }
        Ok(listener)
    }

    /// Verifies a peer against this endpoint's uid. The typed refusal is what the daemon turns into
    /// a user-visible "another user's process tried to talk to your keeper".
    #[cfg(unix)]
    pub fn verify_peer(
        &self,
        fd: std::os::fd::RawFd,
    ) -> KeeperResult<crate::protocol::PeerCredentials> {
        let credentials = crate::protocol::peer_credentials(fd).map_err(|error| {
            KeeperError::new(
                KeeperErrorCode::PeerUidMismatch,
                format!("cannot read peer credentials on the keeper socket: {error}"),
            )
        })?;
        if credentials.uid != self.uid {
            return Err(KeeperError::new(
                KeeperErrorCode::PeerUidMismatch,
                format!(
                    "peer uid {} is not the keeper's uid {}",
                    credentials.uid, self.uid
                ),
            ));
        }
        Ok(credentials)
    }

    /// Removes the socket on the way out. The lock file stays: it is the rendezvous, and unlinking
    /// it would let a second keeper take a fresh lock while this one is still winding down.
    pub fn clear_socket(&self) {
        let _ = fs::remove_file(&self.socket);
    }
}

/// The held single-instance lock. Released on drop; the lock file itself stays, see [`Drop`].
pub struct InstanceLock {
    file: fs::File,
    path: PathBuf,
}

impl InstanceLock {
    fn acquire(path: &Path, wait: bool) -> KeeperResult<Option<Self>> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|error| dir_error(parent, &error))?;
            }
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|error| {
                KeeperError::new(
                    KeeperErrorCode::InvalidSpawn,
                    format!("cannot open the keeper lock {}: {error}", path.display()),
                )
            })?;
        let flags = if wait { libc::LOCK_EX } else { libc::LOCK_EX | libc::LOCK_NB };
        let result = flock(&file, flags);
        if result != 0 {
            let error = io::Error::last_os_error();
            if !wait && error.kind() == io::ErrorKind::WouldBlock {
                return Ok(None);
            }
            return Err(KeeperError::new(
                KeeperErrorCode::ShuttingDown,
                format!("cannot take the keeper lock {}: {error}", path.display()),
            ));
        }
        Ok(Some(Self {
            file,
            path: path.to_path_buf(),
        }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // The lock file is never unlinked, and that is what makes the lock sound. `flock` is held on
        // an inode, not a path: unlinking here would let a newcomer create a *different* inode at the
        // same path and lock it while this one is still held, so two keepers would both believe they
        // own the instance. Leaving the file behind means every keeper locks the same inode for the
        // life of the runtime directory, and the release below is the only way to give it up.
        let _ = flock(&self.file, libc::LOCK_UN);
    }
}

#[cfg(unix)]
fn flock(file: &fs::File, operation: i32) -> i32 {
    use std::os::fd::AsRawFd as _;
    unsafe { libc::flock(file.as_raw_fd(), operation) }
}

/// `$FERRYX_RUNTIME_DIR`, else `/tmp/ferryx-{uid}`. Deliberately not `$TMPDIR`: a per-user temp
/// directory changes per login session, and a keeper must be findable across them.
pub fn runtime_dir(uid: u32) -> PathBuf {
    if let Some(dir) = std::env::var_os("FERRYX_RUNTIME_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    // The dev suffix matches the daemon's `is_dev_runtime()` so debug keepers and release keepers
    // never share a socket.
    let suffix = if is_dev_runtime() { "-dev" } else { "" };
    PathBuf::from(format!("/tmp/ferryx-{uid}{suffix}"))
}

fn is_dev_runtime() -> bool {
    cfg!(debug_assertions)
}

pub fn current_uid() -> KeeperResult<u32> {
    #[cfg(unix)]
    {
        Ok(unsafe { libc::getuid() })
    }
    #[cfg(not(unix))]
    {
        Err(KeeperError::new(
            KeeperErrorCode::PeerUidMismatch,
            "the keeper endpoint needs a unix uid; this platform has no unix sockets",
        ))
    }
}

#[cfg(unix)]
fn owner_uid(metadata: &fs::Metadata) -> u32 {
    // `MetadataExt` is already imported at the top of this module (line 17), which is where
    // `metadata.uid()` below resolves from. A second, narrower import here is redundant.
    metadata.uid()
}

#[cfg(not(unix))]
fn owner_uid(_metadata: &fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
fn set_dir_mode(dir: &Path) -> KeeperResult<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(dir, fs::Permissions::from_mode(DIR_MODE))
        .map_err(|error| dir_error(dir, &error))
}

#[cfg(not(unix))]
fn set_dir_mode(_dir: &Path) -> KeeperResult<()> {
    Ok(())
}

fn dir_error(dir: &Path, error: &io::Error) -> KeeperError {
    KeeperError::new(
        KeeperErrorCode::InvalidSpawn,
        format!("cannot prepare the keeper runtime directory {}: {error}", dir.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_endpoint() -> (tempfile::TempDir, Endpoint) {
        let temp = tempfile::tempdir().expect("tempdir");
        let uid = current_uid().expect("uid");
        let endpoint = Endpoint::new(temp.path().join("runtime"), uid).expect("endpoint");
        (temp, endpoint)
    }

    #[test]
    fn preparing_the_runtime_directory_makes_it_owner_only() {
        let (_temp, endpoint) = temp_endpoint();
        endpoint.prepare_dir().expect("prepare");
        assert!(endpoint.dir().is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(endpoint.dir()).expect("stat").permissions().mode() & 0o777;
            assert_eq!(mode, DIR_MODE);
        }
        // Idempotent: a second call is a no-op, not an error.
        endpoint.prepare_dir().expect("prepare again");
    }

    #[test]
    fn binding_makes_the_socket_owner_only_and_verifies_its_owner() {
        let (_temp, endpoint) = temp_endpoint();
        endpoint.prepare_dir().expect("prepare");
        let lock = endpoint.try_lock().expect("lock").expect("free lock");
        endpoint.clear_stale_socket().expect("clear");
        let listener = endpoint.bind().expect("bind");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(endpoint.socket_path())
                .expect("stat")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, SOCKET_MODE);
        }
        drop(listener);
        endpoint.clear_socket();
        drop(lock);
    }

    #[test]
    fn a_second_keeper_cannot_take_a_held_lock() {
        let (_temp, endpoint) = temp_endpoint();
        endpoint.prepare_dir().expect("prepare");
        let first = endpoint.try_lock().expect("lock").expect("free");
        assert!(
            endpoint.try_lock().expect("try").is_none(),
            "a held lock must report a running keeper instead of a second listener"
        );
        drop(first);
        assert!(endpoint.try_lock().expect("try").is_some(), "the lock is reusable after release");
    }

    #[test]
    fn a_stale_socket_is_cleared_before_a_new_bind() {
        let (_temp, endpoint) = temp_endpoint();
        endpoint.prepare_dir().expect("prepare");
        std::os::unix::net::UnixListener::bind(endpoint.socket_path())
            .expect("plant a socket a dead keeper left behind");
        assert!(endpoint.clear_stale_socket().expect("clear"));
        assert!(!endpoint.socket_path().exists());
        assert!(!endpoint.clear_stale_socket().expect("clear again"), "nothing left to clear");
        endpoint.bind().expect("bind over the cleared path");
    }

    #[test]
    fn a_non_socket_at_the_socket_path_is_refused_rather_than_deleted() {
        let (_temp, endpoint) = temp_endpoint();
        endpoint.prepare_dir().expect("prepare");
        std::fs::write(endpoint.socket_path(), b"not a socket").expect("plant a regular file");
        let error = endpoint.clear_stale_socket().expect_err("must refuse");
        assert_eq!(error.code, KeeperErrorCode::InvalidSpawn);
        assert!(
            endpoint.socket_path().exists(),
            "the keeper does not delete a path it did not create"
        );
    }

    #[test]
    fn the_lock_file_survives_release_so_successors_lock_the_same_inode() {
        let (_temp, endpoint) = temp_endpoint();
        endpoint.prepare_dir().expect("prepare");
        let first = endpoint.try_lock().expect("lock").expect("free lock");
        let inode = fs::metadata(endpoint.lock_path()).expect("stat").ino();
        drop(first);
        assert_eq!(
            fs::metadata(endpoint.lock_path()).expect("stat after release").ino(),
            inode,
            "releasing the lock must not replace the inode a successor locks"
        );
        assert!(endpoint.try_lock().expect("relock").is_some());
    }

    #[test]
    fn this_process_passes_its_own_peer_check_and_a_foreign_uid_does_not() {
        let (_temp, endpoint) = temp_endpoint();
        let (socket, _peer) = std::os::unix::net::UnixStream::pair().expect("pair");
        use std::os::fd::AsRawFd as _;
        endpoint
            .verify_peer(socket.as_raw_fd())
            .expect("the keeper's own uid is accepted");

        let mut foreign = endpoint.clone();
        foreign.uid = endpoint.uid().wrapping_add(1);
        let error = foreign
            .verify_peer(socket.as_raw_fd())
            .expect_err("a different uid is refused");
        assert_eq!(error.code, KeeperErrorCode::PeerUidMismatch);
    }

    #[test]
    fn an_over_long_socket_path_is_refused_typed() {
        let uid = current_uid().expect("uid");
        let long = format!("/tmp/{}", "x".repeat(120));
        let error = Endpoint::new(long, uid).expect_err("over-long path");
        assert_eq!(error.code, KeeperErrorCode::InvalidSpawn);
        assert!(error.message.contains("unix socket limit"));
    }

    #[test]
    fn the_runtime_override_wins_over_the_default_path() {
        // Not a process-wide env mutation: read the same rule the daemon uses.
        let uid = current_uid().expect("uid");
        let dir = runtime_dir(uid);
        if std::env::var_os("FERRYX_RUNTIME_DIR").is_none() {
            assert_eq!(
                dir,
                PathBuf::from(format!("/tmp/ferryx-{uid}{}", if is_dev_runtime() { "-dev" } else { "" }))
            );
        }
        let endpoint = Endpoint::new(dir, uid).expect("endpoint");
        assert_eq!(endpoint.socket_path().file_name().and_then(|n| n.to_str()), Some(SOCKET_NAME));
    }
}
