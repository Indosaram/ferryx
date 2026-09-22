#![cfg(unix)]

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
}

pub struct TransferredAuthority {
    pub canonical_listener: Option<OwnedFd>,
    pub legacy_lock: OwnedFd,
    pub persistent_lock: Option<OwnedFd>,
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

pub fn send_authority(
    stream: &StdUnixStream,
    transfer_id: &str,
    frame_sequence: u64,
    canonical_listener: Option<RawFd>,
    legacy_lock: RawFd,
    persistent_lock: Option<RawFd>,
) -> Result<(), HandoverSocketError> {
    let mut fds = Vec::new();
    let mut roles = Vec::new();

    if let Some(listener_fd) = canonical_listener {
        fds.push(dup_cloexec(listener_fd)?);
        roles.push("canonical_listener".to_string());
    }

    fds.push(dup_cloexec(legacy_lock)?);
    roles.push("legacy_lock".to_string());

    if let Some(persistent_fd) = persistent_lock {
        fds.push(dup_cloexec(persistent_fd)?);
        roles.push("persistent_lock".to_string());
    }

    let meta = AuthorityMetadata {
        has_canonical_listener: canonical_listener.is_some(),
        has_persistent_lock: persistent_lock.is_some(),
        fd_roles: roles,
    };

    let payload = serde_json::to_value(&meta)
        .map_err(|e| HandoverWireError::InvalidPayload(e))?;

    let frame = HandoverFrame::new(KIND_AUTHORITY, transfer_id.to_string(), frame_sequence, payload)
        .with_fds(fds);

    send_frame(stream.as_raw_fd(), &frame)?;
    Ok(())
}

pub fn recv_authority(stream: &StdUnixStream) -> Result<TransferredAuthority, HandoverSocketError> {
    let mut frame = recv_frame(stream.as_raw_fd())?;
    if frame.kind != KIND_AUTHORITY {
        return Err(HandoverSocketError::UnexpectedMessageKind(frame.kind));
    }

    let meta: AuthorityMetadata = serde_json::from_value(frame.payload)
        .map_err(|e| HandoverWireError::InvalidPayload(e))?;

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

    Ok(TransferredAuthority {
        canonical_listener,
        legacy_lock,
        persistent_lock,
    })
}

pub fn adopt_authority(
    authority: TransferredAuthority,
) -> Result<(Option<tokio::net::UnixListener>, DaemonLockFiles), HandoverSocketError> {
    let tokio_listener = if let Some(fd) = authority.canonical_listener {
        let std_listener = StdUnixListener::from(fd);
        std_listener.set_nonblocking(true)?;
        let listener = tokio::net::UnixListener::from_std(std_listener)?;
        Some(listener)
    } else {
        None
    };

    let legacy_file = unsafe { File::from_raw_fd(authority.legacy_lock.into_raw_fd()) };
    let persistent_file = authority
        .persistent_lock
        .map(|fd| unsafe { File::from_raw_fd(fd.into_raw_fd()) });

    let lock_files = DaemonLockFiles::from_locked_files(persistent_file, legacy_file);

    Ok((tokio_listener, lock_files))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTransferMetadata {
    pub session_id: String,
    pub session_snapshot: crate::terminal::session::PtySessionSnapshot,
    pub hub_snapshot: Option<crate::terminal::output_hub::SessionHubSnapshot>,
}

pub fn send_session(
    stream: &StdUnixStream,
    transfer_id: &str,
    frame_sequence: u64,
    export: crate::terminal::session::PtySessionExport,
) -> Result<(), HandoverSocketError> {
    let session_id = export.session_id.clone();
    let snapshot = export.snapshot();
    let meta = SessionTransferMetadata {
        session_id,
        hub_snapshot: snapshot.hub_snapshot.clone(),
        session_snapshot: snapshot,
    };
    let payload = serde_json::to_value(&meta)
        .map_err(|e| HandoverWireError::InvalidPayload(e))?;
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
    let mut frame = recv_frame(stream.as_raw_fd())?;
    if frame.kind == KIND_TRANSFER_DONE {
        return Ok(None);
    }
    if frame.kind != KIND_SESSION {
        return Err(HandoverSocketError::UnexpectedMessageKind(frame.kind));
    }
    let meta: SessionTransferMetadata = serde_json::from_value(frame.payload)
        .map_err(|e| HandoverWireError::InvalidPayload(e))?;
    let master_fd = frame.fds.into_iter().next().ok_or(
        HandoverSocketError::MissingRequiredDescriptor("pty_master_fd"),
    )?;
    Ok(Some(crate::terminal::session::PtySessionExport::from_parts(
        meta.session_snapshot,
        master_fd,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
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
            .open(&legacy_lock_path)
            .expect("create legacy lock");
        let ret = unsafe { libc::flock(legacy_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        assert_eq!(ret, 0, "flock legacy");

        let persistent_lock_path = temp_dir.path().join("persistent.lock");
        let persistent_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
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
            let (tokio_listener, _locks) =
                adopt_authority(received).expect("adopt authority");
            let tokio_listener = tokio_listener.expect("listener adopted");

            let client = StdUnixStream::connect(&listener_path).expect("connect to adopted listener");
            let (server_stream, _) = tokio_listener.accept().await.expect("accept on adopted listener");
            drop(client);
            drop(server_stream);
        });
    }

    #[test]
    fn test_handover_listener_client_roundtrip() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let sock_path = temp_dir.path().join("handover.sock");

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
}
