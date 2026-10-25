use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::handover_wire::{
    peer_credentials, recv_frame, send_frame, HandoverFrame, HandoverWireError, PeerCredentials,
};
use super::server::DaemonLockFiles;

pub const KIND_SESSION: u16 = 0x0001;
pub const KIND_SESSION_ACK: u16 = 0x0002;
pub const KIND_TRANSFER_DONE: u16 = 0x0003;
pub const KIND_AUTHORITY: u16 = 0x0010;
pub const KIND_AUTHORITY_ACK: u16 = 0x0011;
/// Additive (todo 8): the candidate asks the predecessor for its authority descriptors over the
/// handover socket; the predecessor answers with one KIND_AUTHORITY frame.
pub const KIND_AUTHORITY_REQUEST: u16 = 0x0012;

#[derive(Debug)]
pub enum HandoverSocketError {
    Wire(HandoverWireError),
    Io(io::Error),
    PeerUidMismatch { expected: u32, actual: u32 },
    UnexpectedMessageKind(u16),
    MissingRequiredDescriptor(&'static str),
    AdoptionFailed(String),
}

impl std::fmt::Display for HandoverSocketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wire(e) => write!(f, "handover wire error: {e}"),
            Self::Io(e) => write!(f, "handover socket io error: {e}"),
            Self::PeerUidMismatch { expected, actual } => {
                write!(f, "handover peer uid mismatch: expected {expected}, actual {actual}")
            }
            Self::UnexpectedMessageKind(k) => write!(f, "unexpected handover message kind: {k:#x}"),
            Self::MissingRequiredDescriptor(role) => {
                write!(f, "missing required descriptor for role: {role}")
            }
            Self::AdoptionFailed(reason) => write!(f, "descriptor adoption failed: {reason}"),
        }
    }
}

impl std::error::Error for HandoverSocketError {}

impl From<HandoverWireError> for HandoverSocketError {
    fn from(err: HandoverWireError) -> Self {
        Self::Wire(err)
    }
}

impl From<io::Error> for HandoverSocketError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityMetadata {
    pub has_canonical_listener: bool,
    pub has_persistent_lock: bool,
    pub fd_roles: Vec<String>,
    /// Appended after the base fds (todo 8). Omitted when false so a handover-v1 base frame
    /// round-trips value for value.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_control_listener: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_agent_state_listener: bool,
}

pub struct TransferredAuthority {
    pub canonical_listener: Option<OwnedFd>,
    pub legacy_lock: OwnedFd,
    pub persistent_lock: Option<OwnedFd>,
    pub control_listener: Option<OwnedFd>,
    pub agent_state_listener: Option<OwnedFd>,
}

/// Descriptors a predecessor hands to its successor, already duplicated (F_DUPFD_CLOEXEC) so
/// they share the predecessor's open file descriptions: the flock locks stay held by both
/// processes and the listening sockets keep their bound paths for the whole transfer.
pub struct AuthoritySources {
    pub canonical_listener: Option<OwnedFd>,
    pub legacy_lock: OwnedFd,
    pub persistent_lock: Option<OwnedFd>,
    pub control_listener: Option<OwnedFd>,
    pub agent_state_listener: Option<OwnedFd>,
}

/// What the candidate requires before it may commit: the runtime paths each received listener
/// must be bound to, and the lock files each received lock fd must hold.
#[derive(Debug, Clone)]
pub struct ExpectedAuthority {
    pub canonical_socket: PathBuf,
    pub legacy_lock: PathBuf,
    pub persistent_lock: Option<PathBuf>,
    pub control_socket: Option<PathBuf>,
    pub agent_state_socket: Option<PathBuf>,
}

/// The adopted form of a validated authority. Built only after the predecessor's decision reads
/// Retired: until then the descriptors stay plain OwnedFd values, whose drop closes this
/// process's copy without LOCK_UN (DaemonLockFiles would unlock the shared open file description).
pub(crate) struct AdoptedAuthority {
    pub canonical_listener: StdUnixListener,
    pub control_listener: Option<StdUnixListener>,
    pub agent_state_listener: Option<StdUnixListener>,
    pub lock_files: DaemonLockFiles,
}

pub fn get_handover_socket_path(transfer_id: &str) -> PathBuf {
    crate::daemon::server::get_runtime_dir().join(format!("handover-{transfer_id}.sock"))
}

pub struct HandoverSocketListener {
    listener: StdUnixListener,
    path: PathBuf,
}

impl HandoverSocketListener {
    pub fn bind(path: &Path) -> Result<Self, io::Error> {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        let listener = StdUnixListener::bind(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
        }
        Ok(Self {
            listener,
            path: path.to_path_buf(),
        })
    }

    pub fn accept(&self) -> Result<(StdUnixStream, PeerCredentials), HandoverSocketError> {
        let (stream, _) = self.listener.accept()?;
        let creds = peer_credentials(stream.as_raw_fd())?;
        let expected_uid = unsafe { libc::getuid() };
        if creds.uid != expected_uid {
            return Err(HandoverSocketError::PeerUidMismatch {
                expected: expected_uid,
                actual: creds.uid,
            });
        }
        Ok((stream, creds))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for HandoverSocketListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn connect_handover_socket(
    path: &Path,
) -> Result<(StdUnixStream, PeerCredentials), HandoverSocketError> {
    let stream = StdUnixStream::connect(path)?;
    let creds = peer_credentials(stream.as_raw_fd())?;
    let expected_uid = unsafe { libc::getuid() };
    if creds.uid != expected_uid {
        return Err(HandoverSocketError::PeerUidMismatch {
            expected: expected_uid,
            actual: creds.uid,
        });
    }
    Ok((stream, creds))
}

fn dup_cloexec(fd: RawFd) -> Result<OwnedFd, HandoverSocketError> {
    let duped = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duped < 0 {
        return Err(HandoverSocketError::Wire(HandoverWireError::Io(std::io::Error::last_os_error())));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(duped) })
}

/// Duplicates one descriptor this process keeps for a later authority transfer.
pub fn dup_for_authority(fd: RawFd) -> Result<OwnedFd, HandoverSocketError> {
    dup_cloexec(fd)
}

pub fn send_authority(
    stream: &StdUnixStream,
    transfer_id: &str,
    frame_sequence: u64,
    canonical_listener: Option<RawFd>,
    legacy_lock: RawFd,
    persistent_lock: Option<RawFd>,
) -> Result<(), HandoverSocketError> {
    let sources = AuthoritySources {
        canonical_listener: canonical_listener.map(dup_cloexec).transpose()?,
        legacy_lock: dup_cloexec(legacy_lock)?,
        persistent_lock: persistent_lock.map(dup_cloexec).transpose()?,
        control_listener: None,
        agent_state_listener: None,
    };
    send_authority_sources(stream, transfer_id, frame_sequence, sources)
}

/// Sends every authority descriptor in the frozen order: the handover-v1 base fds
/// (canonical_listener, legacy_lock, persistent_lock) and then, appended, control_listener and
/// agent_state_listener.
pub fn send_authority_sources(
    stream: &StdUnixStream,
    transfer_id: &str,
    frame_sequence: u64,
    sources: AuthoritySources,
) -> Result<(), HandoverSocketError> {
    let mut fds = Vec::new();
    let mut roles = Vec::new();
    let has_canonical_listener = sources.canonical_listener.is_some();
    let has_persistent_lock = sources.persistent_lock.is_some();
    let has_control_listener = sources.control_listener.is_some();
    let has_agent_state_listener = sources.agent_state_listener.is_some();

    if let Some(fd) = sources.canonical_listener {
        fds.push(fd);
        roles.push("canonical_listener".to_string());
    }

    fds.push(sources.legacy_lock);
    roles.push("legacy_lock".to_string());

    if let Some(fd) = sources.persistent_lock {
        fds.push(fd);
        roles.push("persistent_lock".to_string());
    }
    if let Some(fd) = sources.control_listener {
        fds.push(fd);
        roles.push("control_listener".to_string());
    }
    if let Some(fd) = sources.agent_state_listener {
        fds.push(fd);
        roles.push("agent_state_listener".to_string());
    }

    let meta = AuthorityMetadata {
        has_canonical_listener,
        has_persistent_lock,
        fd_roles: roles,
        has_control_listener,
        has_agent_state_listener,
    };

    let payload = serde_json::to_value(&meta)
        .map_err(HandoverWireError::InvalidPayload)?;

    let frame = HandoverFrame::new(KIND_AUTHORITY, transfer_id.to_string(), frame_sequence, payload)
        .with_fds(fds);

    send_frame(stream.as_raw_fd(), &frame)?;
    Ok(())
}

pub fn recv_authority(stream: &StdUnixStream) -> Result<TransferredAuthority, HandoverSocketError> {
    let frame = recv_frame(stream.as_raw_fd())?;
    if frame.kind != KIND_AUTHORITY {
        return Err(HandoverSocketError::UnexpectedMessageKind(frame.kind));
    }

    let meta: AuthorityMetadata = serde_json::from_value(frame.payload)
        .map_err(HandoverWireError::InvalidPayload)?;

    let mut fd_iter = frame.fds.into_iter();

    let canonical_listener = if meta.has_canonical_listener {
        Some(fd_iter.next().ok_or(HandoverSocketError::MissingRequiredDescriptor(
            "canonical_listener",
        ))?)
    } else {
        None
    };

    let legacy_lock = fd_iter
        .next()
        .ok_or(HandoverSocketError::MissingRequiredDescriptor("legacy_lock"))?;

    let persistent_lock = if meta.has_persistent_lock {
        Some(fd_iter.next().ok_or(HandoverSocketError::MissingRequiredDescriptor(
            "persistent_lock",
        ))?)
    } else {
        None
    };

    let control_listener = if meta.has_control_listener {
        Some(fd_iter.next().ok_or(HandoverSocketError::MissingRequiredDescriptor(
            "control_listener",
        ))?)
    } else {
        None
    };

    let agent_state_listener = if meta.has_agent_state_listener {
        Some(fd_iter.next().ok_or(HandoverSocketError::MissingRequiredDescriptor(
            "agent_state_listener",
        ))?)
    } else {
        None
    };

    Ok(TransferredAuthority {
        canonical_listener,
        legacy_lock,
        persistent_lock,
        control_listener,
        agent_state_listener,
    })
}

/// The path a listening unix socket is bound to, read with getsockname.
pub fn bound_socket_path(fd: RawFd) -> io::Result<PathBuf> {
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    // SAFETY: address is a properly sized, writable sockaddr_un and length holds its size.
    let ret = unsafe {
        libc::getsockname(fd, (&mut address as *mut libc::sockaddr_un).cast(), &mut length)
    };
    if ret != 0 {
        return Err(io::Error::last_os_error());
    }
    if i32::from(address.sun_family) != libc::AF_UNIX {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "descriptor is not a unix socket"));
    }
    let bytes: Vec<u8> = address
        .sun_path
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

fn same_path(left: &Path, right: &Path) -> bool {
    left == right
        || matches!(
            (std::fs::canonicalize(left), std::fs::canonicalize(right)),
            (Ok(a), Ok(b)) if a == b
        )
}

fn validate_listener(
    role: &'static str,
    fd: Option<&OwnedFd>,
    expected: Option<&Path>,
) -> Result<(), HandoverSocketError> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let fd = fd.ok_or(HandoverSocketError::MissingRequiredDescriptor(role))?;
    let bound = bound_socket_path(fd.as_raw_fd())?;
    if !same_path(&bound, expected) {
        return Err(HandoverSocketError::AdoptionFailed(format!(
            "{role} is bound to {} instead of {}",
            bound.display(),
            expected.display()
        )));
    }
    Ok(())
}

/// True when some open file description holds an exclusive flock on path: a fresh open of the
/// file (a new open file description, exactly what another process would get) cannot take it.
pub fn lock_is_held_elsewhere(path: &Path) -> io::Result<bool> {
    let probe = std::fs::OpenOptions::new().read(true).open(path)?;
    // SAFETY: probe is an open file for the duration of the call.
    let ret = unsafe { libc::flock(probe.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret == 0 {
        // The probe took a free lock; closing its only fd releases it again.
        drop(probe);
        return Ok(false);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(true)
    } else {
        Err(error)
    }
}

fn validate_lock(role: &'static str, fd: &OwnedFd, path: &Path) -> Result<(), HandoverSocketError> {
    use std::os::unix::fs::MetadataExt;
    // The fd must name the lock file at path (same device and inode) ...
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fd is open and stat is a writable libc::stat.
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    let on_disk = std::fs::metadata(path)?;
    #[cfg(target_os = "macos")]
    let same_device = stat.st_dev as u64 == on_disk.dev();
    #[cfg(not(target_os = "macos"))]
    let same_device = stat.st_dev == on_disk.dev();
    if !same_device || stat.st_ino != on_disk.ino() {
        return Err(HandoverSocketError::AdoptionFailed(format!(
            "{role} does not refer to {}",
            path.display()
        )));
    }
    // ... the lock must be held (a fresh open file description cannot take it) ...
    if !lock_is_held_elsewhere(path)? {
        return Err(HandoverSocketError::AdoptionFailed(format!(
            "{role} at {} is not locked",
            path.display()
        )));
    }
    // ... and held by this very open file description: re-locking it succeeds only when the
    // exclusive lock is ours (a lock held by another description refuses it).
    // SAFETY: fd is open for the duration of the call.
    if unsafe { libc::flock(fd.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(HandoverSocketError::AdoptionFailed(format!(
            "{role} does not hold the lock on {}",
            path.display()
        )));
    }
    Ok(())
}

/// Candidate-side validation of a received authority. Accepts on no listener.
pub fn validate_authority(
    authority: &TransferredAuthority,
    expected: &ExpectedAuthority,
) -> Result<(), HandoverSocketError> {
    validate_listener(
        "canonical_listener",
        authority.canonical_listener.as_ref(),
        Some(&expected.canonical_socket),
    )?;
    validate_lock("legacy_lock", &authority.legacy_lock, &expected.legacy_lock)?;
    if let Some(path) = expected.persistent_lock.as_deref() {
        let fd = authority
            .persistent_lock
            .as_ref()
            .ok_or(HandoverSocketError::MissingRequiredDescriptor("persistent_lock"))?;
        validate_lock("persistent_lock", fd, path)?;
    }
    validate_listener(
        "control_listener",
        authority.control_listener.as_ref(),
        expected.control_socket.as_deref(),
    )?;
    validate_listener(
        "agent_state_listener",
        authority.agent_state_listener.as_ref(),
        expected.agent_state_socket.as_deref(),
    )?;
    Ok(())
}

/// Turns a validated authority into listeners and lock files. Call only after the predecessor's
/// recorded decision is Retired.
pub(crate) fn adopt_validated_authority(
    authority: TransferredAuthority,
) -> Result<AdoptedAuthority, HandoverSocketError> {
    let canonical = authority
        .canonical_listener
        .ok_or(HandoverSocketError::MissingRequiredDescriptor("canonical_listener"))?;
    let listener = |fd: OwnedFd| -> Result<StdUnixListener, HandoverSocketError> {
        let listener = StdUnixListener::from(fd);
        listener.set_nonblocking(true)?;
        Ok(listener)
    };
    let canonical_listener = listener(canonical)?;
    let control_listener = authority.control_listener.map(listener).transpose()?;
    let agent_state_listener = authority.agent_state_listener.map(listener).transpose()?;
    let legacy_file = unsafe { File::from_raw_fd(authority.legacy_lock.into_raw_fd()) };
    let persistent_file = authority
        .persistent_lock
        .map(|fd| unsafe { File::from_raw_fd(fd.into_raw_fd()) });
    Ok(AdoptedAuthority {
        canonical_listener,
        control_listener,
        agent_state_listener,
        lock_files: DaemonLockFiles::from_locked_files(persistent_file, legacy_file),
    })
}

/// Candidate side: asks for the predecessor's authority on an accepted handover-socket stream
/// and receives it. Accepts on none of the received listeners.
pub fn request_authority(
    stream: &StdUnixStream,
    transfer_id: &str,
) -> Result<TransferredAuthority, HandoverSocketError> {
    let request = HandoverFrame::new(
        KIND_AUTHORITY_REQUEST,
        transfer_id.to_string(),
        1,
        serde_json::Value::Null,
    );
    send_frame(stream.as_raw_fd(), &request)?;
    recv_authority(stream)
}

/// Whether a `request_authority` failure means the predecessor hung up without answering.
///
/// A pre-plan predecessor (one that predates the authority transfer, e.g. 2026.1018.x) never
/// reads KIND_AUTHORITY_REQUEST: it closes its end of the handover stream once the sessions are
/// sent. Depending on timing the candidate then sees EOF on the reply (`Closed`) or fails the
/// request write itself (`EPIPE`, `ECONNRESET`). Every one of these means the same thing: no
/// authority is coming, so the candidate falls back to taking the locks after commit.
pub fn predecessor_closed_without_authority(error: &HandoverSocketError) -> bool {
    let io_error = match error {
        HandoverSocketError::Wire(HandoverWireError::Closed) => return true,
        HandoverSocketError::Wire(HandoverWireError::Io(error)) | HandoverSocketError::Io(error) => error,
        _ => return false,
    };
    matches!(
        io_error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::UnexpectedEof
    )
}

/// Predecessor side: reads the candidate's KIND_AUTHORITY_REQUEST and answers it with the
/// duplicated authority descriptors.
pub fn serve_authority_request(
    stream: &StdUnixStream,
    sources: AuthoritySources,
) -> Result<(), HandoverSocketError> {
    let request = recv_frame(stream.as_raw_fd())?;
    if request.kind != KIND_AUTHORITY_REQUEST {
        return Err(HandoverSocketError::UnexpectedMessageKind(request.kind));
    }
    send_authority_sources(
        stream,
        &request.transfer_id,
        request.frame_sequence.saturating_add(1),
        sources,
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTransferMetadata {
    pub session_id: String,
    pub session_snapshot: crate::terminal::session::PtySessionSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hub_snapshot: Option<crate::terminal::output_hub::SessionHubSnapshot>,
    /// The exporting daemon's authoritative spawn record for this session, serialized verbatim
    /// (`daemon::session_service::StoredSessionMeta`). The export snapshot carries PTY-level
    /// identity only, so this record is the sole carrier of the workspace/worktree ownership and
    /// of the client-request identity a transferred session keeps. `None` when the predecessor
    /// holds no record for the session, which its successor refuses instead of inventing one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<serde_json::Value>,
}

pub fn send_session(
    stream: &StdUnixStream,
    transfer_id: &str,
    frame_sequence: u64,
    export: crate::terminal::session::PtySessionExport,
    owner: Option<serde_json::Value>,
) -> Result<(), HandoverSocketError> {
    let session_id = export.session_id.clone();
    let snapshot = export.snapshot();
    let meta = SessionTransferMetadata {
        session_id,
        hub_snapshot: None,
        session_snapshot: snapshot,
        owner,
    };
    let payload = serde_json::to_value(&meta)
        .map_err(HandoverWireError::InvalidPayload)?;
    let frame = HandoverFrame::new(KIND_SESSION, transfer_id.to_string(), frame_sequence, payload)
        .with_fds(vec![export.master_fd]);
    send_frame(stream.as_raw_fd(), &frame)?;
    Ok(())
}

pub fn send_transfer_done(
    stream: &StdUnixStream,
    transfer_id: &str,
    frame_sequence: u64,
) -> Result<(), HandoverSocketError> {
    let frame = HandoverFrame::new(
        KIND_TRANSFER_DONE,
        transfer_id.to_string(),
        frame_sequence,
        serde_json::Value::Null,
    );
    send_frame(stream.as_raw_fd(), &frame)?;
    Ok(())
}

pub fn recv_session(
    stream: &StdUnixStream,
) -> Result<Option<crate::terminal::session::PtySessionExport>, HandoverSocketError> {
    Ok(recv_session_with_owner(stream)?.map(|(export, _owner)| export))
}

/// Receives one exported session together with the owner record the predecessor sent for it.
///
/// The record is returned beside the export rather than merged into it: the adopting daemon must
/// check it against the predecessor's own describe answer before it takes ownership, and must be
/// able to refuse the session when the two disagree.
pub fn recv_session_with_owner(
    stream: &StdUnixStream,
) -> Result<
    Option<(crate::terminal::session::PtySessionExport, Option<serde_json::Value>)>,
    HandoverSocketError,
> {
    let frame = recv_frame(stream.as_raw_fd())?;
    if frame.kind == KIND_TRANSFER_DONE {
        return Ok(None);
    }
    if frame.kind != KIND_SESSION {
        return Err(HandoverSocketError::UnexpectedMessageKind(frame.kind));
    }
    let mut meta: SessionTransferMetadata = serde_json::from_value(frame.payload)
        .map_err(HandoverWireError::InvalidPayload)?;
    if meta.session_snapshot.hub_snapshot.is_none() {
        meta.session_snapshot.hub_snapshot = meta.hub_snapshot.take();
    }
    let master_fd = frame.fds.into_iter().next().ok_or(
        HandoverSocketError::MissingRequiredDescriptor("pty_master_fd"),
    )?;
    Ok(Some((
        crate::terminal::session::PtySessionExport::from_parts(meta.session_snapshot, master_fd),
        meta.owner,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn test_authority_frame_roundtrip() {
        let (sender, receiver) = UnixStream::pair().expect("socketpair");
        let temp_dir = tempfile::tempdir().expect("tempdir");

        let listener_path = temp_dir.path().join("canonical.sock");
        let listener = StdUnixListener::bind(&listener_path).expect("bind listener");

        let legacy_lock_path = temp_dir.path().join("daemon.lock");
        let legacy_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&legacy_lock_path)
            .expect("create legacy lock");
        let ret = unsafe { libc::flock(legacy_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        assert_eq!(ret, 0, "flock legacy");

        let persistent_lock_path = temp_dir.path().join("persistent.lock");
        let persistent_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&persistent_lock_path)
            .expect("create persistent lock");
        let ret = unsafe { libc::flock(persistent_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        assert_eq!(ret, 0, "flock persistent");

        let transfer_id = uuid::Uuid::new_v4().to_string();
        send_authority(
            &sender,
            &transfer_id,
            1,
            Some(listener.as_raw_fd()),
            legacy_file.as_raw_fd(),
            Some(persistent_file.as_raw_fd()),
        )
        .expect("send authority");

        let received = recv_authority(&receiver).expect("recv authority");
        assert!(received.canonical_listener.is_some());
        assert!(received.persistent_lock.is_some());

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .expect("tokio rt");

        rt.block_on(async move {
            let adopted = adopt_validated_authority(received).expect("adopt authority");
            let tokio_listener = tokio::net::UnixListener::from_std(adopted.canonical_listener)
                .expect("listener adopted");

            let client = StdUnixStream::connect(&listener_path).expect("connect to adopted listener");
            let (server_stream, _) = tokio_listener.accept().await.expect("accept on adopted listener");
            drop(client);
            drop(server_stream);
        });
    }

    #[test]
    fn test_handover_listener_client_roundtrip() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        handover_listener_client_roundtrip_in(temp_dir.path());
    }

    /// A predecessor-shaped authority: five bound/locked runtime objects in one directory.
    struct AuthorityFixture {
        dir: tempfile::TempDir,
        canonical: StdUnixListener,
        control: StdUnixListener,
        agent_state: StdUnixListener,
        legacy_lock: File,
        persistent_lock: File,
    }

    impl AuthorityFixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let lock = |name: &str| {
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(dir.path().join(name))
                    .expect("lock file");
                assert_eq!(
                    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                    0
                );
                file
            };
            Self {
                canonical: StdUnixListener::bind(dir.path().join("daemon.sock")).unwrap(),
                control: StdUnixListener::bind(dir.path().join("control.sock")).unwrap(),
                agent_state: StdUnixListener::bind(dir.path().join("agent-state.sock")).unwrap(),
                legacy_lock: lock("daemon.lock"),
                persistent_lock: lock("persistent.lock"),
                dir,
            }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        fn expected(&self) -> ExpectedAuthority {
            ExpectedAuthority {
                canonical_socket: self.path("daemon.sock"),
                legacy_lock: self.path("daemon.lock"),
                persistent_lock: Some(self.path("persistent.lock")),
                control_socket: Some(self.path("control.sock")),
                agent_state_socket: Some(self.path("agent-state.sock")),
            }
        }

        fn sources(&self) -> AuthoritySources {
            AuthoritySources {
                canonical_listener: Some(dup_for_authority(self.canonical.as_raw_fd()).unwrap()),
                legacy_lock: dup_for_authority(self.legacy_lock.as_raw_fd()).unwrap(),
                persistent_lock: Some(dup_for_authority(self.persistent_lock.as_raw_fd()).unwrap()),
                control_listener: Some(dup_for_authority(self.control.as_raw_fd()).unwrap()),
                agent_state_listener: Some(dup_for_authority(self.agent_state.as_raw_fd()).unwrap()),
            }
        }

        /// The probe-flock invariant: another process's non-blocking flock on either lock file
        /// fails, and the public socket path exists.
        fn assert_authority_held(&self, transition: &str) {
            for name in ["daemon.lock", "persistent.lock"] {
                assert!(
                    lock_is_held_elsewhere(&self.path(name)).unwrap(),
                    "{name} was free at {transition}"
                );
            }
            assert!(self.path("daemon.sock").exists(), "public socket vanished at {transition}");
        }
    }

    /// Requests the authority over a real socketpair, with the predecessor side on a thread.
    fn transfer(fixture: &AuthorityFixture) -> TransferredAuthority {
        let (candidate, predecessor) = UnixStream::pair().expect("socketpair");
        let sources = fixture.sources();
        let serving = std::thread::spawn(move || serve_authority_request(&predecessor, sources));
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let authority = request_authority(&candidate, &transfer_id).expect("authority received");
        serving.join().unwrap().expect("authority served");
        authority
    }

    /// A pre-plan predecessor closes the handover stream before the candidate writes its
    /// authority request: the write fails with EPIPE, which must read as "no authority" (the
    /// legacy fallback), not as a failed transfer that aborts the handover.
    #[test]
    fn request_authority_to_a_predecessor_that_already_hung_up_falls_back() {
        let (candidate, predecessor) = UnixStream::pair().expect("socketpair");
        drop(predecessor);
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let Err(error) = request_authority(&candidate, &transfer_id) else {
            panic!("a closed peer sent authority");
        };
        assert!(
            matches!(&error, HandoverSocketError::Wire(HandoverWireError::Io(err)) if err.kind() == io::ErrorKind::BrokenPipe),
            "expected EPIPE on the request write, got {error}"
        );
        assert!(predecessor_closed_without_authority(&error), "{error}");
    }

    /// The other timing: the predecessor reads the request, then closes without answering.
    #[test]
    fn request_authority_to_a_predecessor_that_closes_after_reading_falls_back() {
        let (candidate, predecessor) = UnixStream::pair().expect("socketpair");
        let peer = std::thread::spawn(move || {
            let _ = recv_frame(predecessor.as_raw_fd());
            drop(predecessor);
        });
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let Err(error) = request_authority(&candidate, &transfer_id) else {
            panic!("a closed peer sent authority");
        };
        peer.join().unwrap();
        assert!(predecessor_closed_without_authority(&error), "{error}");
    }

    #[test]
    fn hang_up_kinds_fall_back_and_protocol_errors_do_not() {
        for kind in [
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::ConnectionAborted,
            io::ErrorKind::UnexpectedEof,
        ] {
            assert!(predecessor_closed_without_authority(&HandoverSocketError::Wire(HandoverWireError::Io(
                io::Error::from(kind)
            ))));
            assert!(predecessor_closed_without_authority(&HandoverSocketError::Io(io::Error::from(kind))));
        }
        assert!(predecessor_closed_without_authority(&HandoverSocketError::Wire(HandoverWireError::Closed)));
        assert!(!predecessor_closed_without_authority(&HandoverSocketError::Wire(HandoverWireError::BadMagic)));
        assert!(!predecessor_closed_without_authority(&HandoverSocketError::Wire(HandoverWireError::Io(
            io::Error::from(io::ErrorKind::PermissionDenied)
        ))));
        assert!(!predecessor_closed_without_authority(&HandoverSocketError::UnexpectedMessageKind(0x99)));
    }

    #[test]
    fn authority_transfer_carries_control_and_agent_state_listeners_after_the_base_fds() {
        let fixture = AuthorityFixture::new();
        let (sender, receiver) = UnixStream::pair().unwrap();
        let transfer_id = uuid::Uuid::new_v4().to_string();
        send_authority_sources(&sender, &transfer_id, 2, fixture.sources()).unwrap();
        let frame = recv_frame(receiver.as_raw_fd()).unwrap();
        let meta: AuthorityMetadata = serde_json::from_value(frame.payload).unwrap();
        assert_eq!(
            meta.fd_roles,
            ["canonical_listener", "legacy_lock", "persistent_lock", "control_listener", "agent_state_listener"]
        );
        assert!(meta.has_control_listener && meta.has_agent_state_listener);
        assert_eq!(frame.fds.len(), 5);
    }

    #[test]
    fn base_authority_metadata_serializes_without_the_appended_fields() {
        let base = AuthorityMetadata {
            has_canonical_listener: true,
            has_persistent_lock: true,
            fd_roles: vec!["canonical_listener".into(), "legacy_lock".into(), "persistent_lock".into()],
            has_control_listener: false,
            has_agent_state_listener: false,
        };
        let value = serde_json::to_value(&base).unwrap();
        assert!(value.get("has_control_listener").is_none());
        assert!(value.get("has_agent_state_listener").is_none());
        let back: AuthorityMetadata = serde_json::from_value(value).unwrap();
        assert!(!back.has_control_listener && !back.has_agent_state_listener);
    }

    #[test]
    fn valid_authority_validates_and_adopts_without_any_lock_or_path_gap() {
        let fixture = AuthorityFixture::new();
        fixture.assert_authority_held("before transfer");
        let authority = transfer(&fixture);
        fixture.assert_authority_held("after the authority frame");
        validate_authority(&authority, &fixture.expected()).expect("valid authority");
        fixture.assert_authority_held("after validation");

        // Commit: the predecessor closes its copies without LOCK_UN.
        let AuthorityFixture { dir, canonical, control, agent_state, legacy_lock, persistent_lock } = fixture;
        drop((canonical, control, agent_state, legacy_lock, persistent_lock));
        for name in ["daemon.lock", "persistent.lock"] {
            assert!(lock_is_held_elsewhere(&dir.path().join(name)).unwrap(), "{name} free after predecessor close");
        }
        assert!(dir.path().join("daemon.sock").exists());

        let adopted = adopt_validated_authority(authority).expect("adopt");
        for name in ["daemon.lock", "persistent.lock"] {
            assert!(lock_is_held_elsewhere(&dir.path().join(name)).unwrap(), "{name} free after adoption");
        }
        // The successor serves the very socket clients already know.
        let client = StdUnixStream::connect(dir.path().join("daemon.sock")).expect("connect");
        adopted.canonical_listener.set_nonblocking(false).unwrap();
        adopted.canonical_listener.accept().expect("inherited listener accepts");
        drop(client);
        let control = adopted.control_listener.expect("control listener");
        assert_eq!(bound_socket_path(control.as_raw_fd()).unwrap(), dir.path().join("control.sock"));
        drop(adopted.lock_files);
        assert!(!lock_is_held_elsewhere(&dir.path().join("daemon.lock")).unwrap());
    }

    /// Sends an authority frame whose metadata declares every fd but which carries only
    /// keep of them.
    fn truncated_authority(fixture: &AuthorityFixture, keep: usize) -> Result<TransferredAuthority, HandoverSocketError> {
        let (sender, receiver) = UnixStream::pair().unwrap();
        let sources = fixture.sources();
        let fds: Vec<OwnedFd> = [
            sources.canonical_listener,
            Some(sources.legacy_lock),
            sources.persistent_lock,
            sources.control_listener,
            sources.agent_state_listener,
        ]
        .into_iter()
        .flatten()
        .take(keep)
        .collect();
        let meta = AuthorityMetadata {
            has_canonical_listener: true,
            has_persistent_lock: true,
            fd_roles: Vec::new(),
            has_control_listener: true,
            has_agent_state_listener: true,
        };
        let frame = HandoverFrame::new(
            KIND_AUTHORITY,
            uuid::Uuid::new_v4().to_string(),
            2,
            serde_json::to_value(meta).unwrap(),
        )
        .with_fds(fds);
        send_frame(sender.as_raw_fd(), &frame).unwrap();
        recv_authority(&receiver)
    }

    #[test]
    fn authority_frame_missing_each_fd_kind_is_refused_and_the_predecessor_stays_owner() {
        let fixture = AuthorityFixture::new();
        for (keep, role) in [
            (0, "canonical_listener"),
            (1, "legacy_lock"),
            (2, "persistent_lock"),
            (3, "control_listener"),
            (4, "agent_state_listener"),
        ] {
            match truncated_authority(&fixture, keep) {
                Err(HandoverSocketError::MissingRequiredDescriptor(missing)) => assert_eq!(missing, role),
                Err(other) => panic!("{role}: unexpected error {other}"),
                Ok(_) => panic!("{role}: a frame without it was accepted"),
            }
            fixture.assert_authority_held(role);
        }
    }

    #[test]
    fn listener_bound_to_another_path_is_refused() {
        let fixture = AuthorityFixture::new();
        let stray = StdUnixListener::bind(fixture.path("stray.sock")).unwrap();
        let mut authority = transfer(&fixture);
        authority.control_listener = Some(dup_for_authority(stray.as_raw_fd()).unwrap());
        let error = validate_authority(&authority, &fixture.expected()).unwrap_err();
        assert!(error.to_string().contains("control_listener"), "{error}");
        drop(authority);
        fixture.assert_authority_held("after the refused listener");
        assert!(StdUnixStream::connect(fixture.path("control.sock")).is_ok());
    }

    #[test]
    fn lock_fd_that_holds_no_lock_is_refused() {
        let fixture = AuthorityFixture::new();
        let mut authority = transfer(&fixture);
        // A second open of the same file: right inode, but not the open file description
        // holding the lock.
        let unlocked: OwnedFd = std::fs::OpenOptions::new()
            .read(true)
            .open(fixture.path("daemon.lock"))
            .unwrap()
            .into();
        authority.legacy_lock = unlocked;
        let error = validate_authority(&authority, &fixture.expected()).unwrap_err();
        assert!(error.to_string().contains("legacy_lock"), "{error}");
        drop(authority);
        fixture.assert_authority_held("after the refused lock");
    }

    #[test]
    fn lock_fd_for_an_unlocked_file_is_refused() {
        let fixture = AuthorityFixture::new();
        let mut authority = transfer(&fixture);
        let other = fixture.path("other.lock");
        std::fs::write(&other, b"").unwrap();
        authority.persistent_lock = Some(std::fs::File::open(&other).unwrap().into());
        assert!(validate_authority(&authority, &fixture.expected()).is_err());
        fixture.assert_authority_held("after the refused persistent lock");
    }

    fn handover_listener_client_roundtrip_in(dir: &Path) {
        let sock_path = dir.join("handover.sock");

        let listener = HandoverSocketListener::bind(&sock_path).expect("bind listener");
        let (client_stream, client_creds) =
            connect_handover_socket(&sock_path).expect("client connect");

        let (server_stream, server_creds) = listener.accept().expect("server accept");

        assert_eq!(client_creds.uid, unsafe { libc::getuid() });
        assert_eq!(server_creds.uid, unsafe { libc::getuid() });

        drop(client_stream);
        drop(server_stream);
        drop(listener);
        assert!(!sock_path.exists(), "socket file unlinked on drop");
    }

    /// The owner record must survive the frame round-trip exactly as the owner serialized it: it
    /// is the only carrier of a transferred session's workspace ownership, and a successor refuses
    /// a session whose record it cannot read.
    #[test]
    fn test_session_frame_carries_the_owner_record() {
        let (sender, receiver) = UnixStream::pair().expect("socketpair");
        let master: OwnedFd = File::open("/dev/null").expect("open /dev/null").into();
        let snapshot = crate::terminal::session::PtySessionSnapshot {
            session_id: "owner-record-session".into(),
            incarnation: Some("owner-record-incarnation".into()),
            pid: Some(4242),
            pgid: Some(4242),
            cols: 80,
            rows: 24,
            worktree_path: None,
            state: crate::terminal::PtySessionState::Running,
            hub_snapshot: None,
            suspension_receipt: None,
        };
        let owner = serde_json::json!({
            "clientRequestId": "req-owner-record",
            "workspaceId": "ws-owner-record",
        });
        let transfer_id = uuid::Uuid::new_v4().to_string();
        send_session(
            &sender,
            &transfer_id,
            1,
            crate::terminal::session::PtySessionExport::from_parts(snapshot, master),
            Some(owner.clone()),
        )
        .expect("send session frame");

        let (export, received) = recv_session_with_owner(&receiver)
            .expect("recv session frame")
            .expect("a session frame");
        assert_eq!(export.session_id, "owner-record-session");
        assert_eq!(
            export.incarnation.as_deref(),
            Some("owner-record-incarnation")
        );
        assert_eq!(received, Some(owner));
    }

    /// Regression test for handover-4: A full 512 KiB terminal output hub ring buffer must not be
    /// serialized twice, which would push the JSON frame past HANDOVER_MAX_PAYLOAD_BYTES (4 MiB)
    /// and fail with PayloadTooLarge.
    #[test]
    fn test_send_session_large_hub_snapshot_does_not_exceed_payload_limit() {
        use std::sync::Arc;
        use crate::terminal::output_hub::{OutputChunk, SessionHubSnapshot};

        let (sender, receiver) = UnixStream::pair().expect("socketpair");
        let master: OwnedFd = File::open("/dev/null").expect("open /dev/null").into();

        let chunk_size = 64 * 1024;
        let mut chunks = Vec::new();
        let chunk_data: Vec<u8> = (0..chunk_size).map(|i| (i % 256) as u8).collect();
        let chunk_arc: Arc<[u8]> = chunk_data.into();
        for i in 1..=12 {
            chunks.push(OutputChunk {
                sequence: i,
                bytes: chunk_arc.clone(),
                metrics_read_unix_micros: None,
                replay_gap: None,
            });
        }

        let hub_snapshot = SessionHubSnapshot {
            capacity: 768 * 1024,
            chunks,
            next_sequence: 13,
            bracketed_paste_enabled: false,
            resize_ledger: Vec::new(),
            replay_gap: None,
            transport_owner: false,
            retained_state: None,
            screen_seed: None,
            history_incomplete: false,
        };

        let snapshot = crate::terminal::session::PtySessionSnapshot {
            session_id: "large-hub-session".into(),
            incarnation: Some("large-hub-incarnation".into()),
            pid: Some(5432),
            pgid: Some(5432),
            cols: 80,
            rows: 24,
            worktree_path: None,
            state: crate::terminal::PtySessionState::Running,
            hub_snapshot: Some(hub_snapshot),
            suspension_receipt: None,
        };

        let transfer_id = uuid::Uuid::new_v4().to_string();
        let export = crate::terminal::session::PtySessionExport::from_parts(snapshot, master);
        let sending = std::thread::spawn(move || {
            send_session(&sender, &transfer_id, 1, export, None)
                .expect("send_session with 768 KiB hub snapshot must succeed");
        });

        let (received_export, _) = recv_session_with_owner(&receiver)
            .expect("recv_session_with_owner")
            .expect("a session");
        sending.join().expect("sender thread");
        assert_eq!(received_export.session_id, "large-hub-session");
        let received_hub = received_export
            .snapshot()
            .hub_snapshot
            .expect("hub snapshot must be preserved");
        assert_eq!(received_hub.chunks.len(), 12);
        let total_bytes: usize = received_hub.chunks.iter().map(|c| c.bytes.len()).sum();
        assert_eq!(total_bytes, 768 * 1024);
    }
}
