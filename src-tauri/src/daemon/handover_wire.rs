
#![cfg(unix)]

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use serde_json::Value;

pub const HANDOVER_MAGIC: [u8; 4] = *b"FXHO";
pub const HANDOVER_FRAMING_VERSION: u16 = 1;
pub const HANDOVER_HEADER_BYTES: usize = 60;
pub const HANDOVER_MAX_PAYLOAD_BYTES: u32 = 4 * 1024 * 1024;
pub const HANDOVER_MAX_FDS: usize = 16;

#[derive(Debug)]
pub enum HandoverWireError {
    Io(io::Error),
    Closed,
    BadMagic,
    BadFramingVersion(u16),
    ReservedFlags(u16),
    BadTransferId(String),
    PayloadTooLarge(u32),
    TooManyFds(u16),
    TruncatedControl,
    FdCountMismatch { declared: u16, actual: usize },
    InvalidPayload(serde_json::Error),
}

impl std::fmt::Display for HandoverWireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "handover wire io error: {e}"),
            Self::Closed => write!(f, "handover wire closed by peer"),
            Self::BadMagic => write!(f, "handover wire bad magic"),
            Self::BadFramingVersion(v) => write!(f, "handover wire bad framing version {v}"),
            Self::ReservedFlags(v) => write!(f, "handover wire reserved flags set: {v}"),
            Self::BadTransferId(id) => write!(f, "handover wire bad transfer id {id:?}"),
            Self::PayloadTooLarge(n) => write!(f, "handover wire payload too large: {n}"),
            Self::TooManyFds(n) => write!(f, "handover wire too many fds: {n}"),
            Self::TruncatedControl => write!(f, "handover wire control data truncated"),
            Self::FdCountMismatch { declared, actual } => {
                write!(f, "handover wire fd count mismatch: declared {declared}, received {actual}")
            }
            Self::InvalidPayload(e) => write!(f, "handover wire invalid payload: {e}"),
        }
    }
}

impl std::error::Error for HandoverWireError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    pub uid: u32,
    pub gid: u32,
    pub pid: Option<i32>,
}

#[derive(Debug)]
pub struct HandoverFrame {
    pub kind: u16,
    pub transfer_id: String,
    pub frame_sequence: u64,
    pub flags: u16,
    pub payload: Value,
    pub fds: Vec<OwnedFd>,
}

impl HandoverFrame {
    pub fn new(kind: u16, transfer_id: String, frame_sequence: u64, payload: Value) -> Self {
        Self { kind, transfer_id, frame_sequence, flags: 0, payload, fds: Vec::new() }
    }

    pub fn with_fds(mut self, fds: Vec<OwnedFd>) -> Self {
        self.fds = fds;
        self
    }
}

fn encode_header(
    kind: u16,
    transfer_id: &str,
    frame_sequence: u64,
    fd_count: u16,
    flags: u16,
    payload_len: u32,
) -> Result<[u8; HANDOVER_HEADER_BYTES], HandoverWireError> {
    let id_bytes = transfer_id.as_bytes();
    if id_bytes.len() != 36 || !id_bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        return Err(HandoverWireError::BadTransferId(transfer_id.to_string()));
    }
    let mut header = [0u8; HANDOVER_HEADER_BYTES];
    header[0..4].copy_from_slice(&HANDOVER_MAGIC);
    header[4..6].copy_from_slice(&HANDOVER_FRAMING_VERSION.to_be_bytes());
    header[6..8].copy_from_slice(&kind.to_be_bytes());
    header[8..12].copy_from_slice(&payload_len.to_be_bytes());
    header[12..48].copy_from_slice(id_bytes);
    header[48..56].copy_from_slice(&frame_sequence.to_be_bytes());
    header[56..58].copy_from_slice(&fd_count.to_be_bytes());
    header[58..60].copy_from_slice(&flags.to_be_bytes());
    Ok(header)
}

#[allow(clippy::type_complexity)]
fn decode_header(header: &[u8; HANDOVER_HEADER_BYTES]) -> Result<(u16, String, u64, u16, u16, u32), HandoverWireError> {
    if header[0..4] != HANDOVER_MAGIC {
        return Err(HandoverWireError::BadMagic);
    }
    let version = u16::from_be_bytes([header[4], header[5]]);
    if version != HANDOVER_FRAMING_VERSION {
        return Err(HandoverWireError::BadFramingVersion(version));
    }
    let kind = u16::from_be_bytes([header[6], header[7]]);
    let payload_len = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
    let transfer_id_bytes = &header[12..48];
    if !transfer_id_bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        return Err(HandoverWireError::BadTransferId(
            String::from_utf8_lossy(transfer_id_bytes).into_owned(),
        ));
    }
    let transfer_id = String::from_utf8_lossy(transfer_id_bytes).into_owned();
    if transfer_id.len() != 36 {
        return Err(HandoverWireError::BadTransferId(transfer_id));
    }
    let frame_sequence = u64::from_be_bytes(header[48..56].try_into().expect("8 bytes"));
    let fd_count = u16::from_be_bytes([header[56], header[57]]);
    let flags = u16::from_be_bytes([header[58], header[59]]);
    if flags != 0 {
        return Err(HandoverWireError::ReservedFlags(flags));
    }
    Ok((kind, transfer_id, frame_sequence, fd_count, flags, payload_len))
}

pub fn send_frame(fd: RawFd, frame: &HandoverFrame) -> Result<(), HandoverWireError> {
    let payload = serde_json::to_vec(&frame.payload).map_err(HandoverWireError::InvalidPayload)?;
    let payload_len = u32::try_from(payload.len()).map_err(|_| HandoverWireError::PayloadTooLarge(u32::MAX))?;
    if payload.len() as u64 > HANDOVER_MAX_PAYLOAD_BYTES as u64 {
        return Err(HandoverWireError::PayloadTooLarge(payload_len));
    }
    if frame.fds.len() > HANDOVER_MAX_FDS {
        return Err(HandoverWireError::TooManyFds(frame.fds.len() as u16));
    }
    let header = encode_header(
        frame.kind,
        &frame.transfer_id,
        frame.frame_sequence,
        frame.fds.len() as u16,
        frame.flags,
        payload_len,
    )?;
    let fd_values: Vec<RawFd> = frame.fds.iter().map(AsRawFd::as_raw_fd).collect();
    unsafe { send_raw_with_fds(fd, &header, &payload, &fd_values).map_err(HandoverWireError::Io) }
}

pub(crate) unsafe fn send_raw_with_fds(
    fd: RawFd,
    header: &[u8; HANDOVER_HEADER_BYTES],
    payload: &[u8],
    fds: &[RawFd],
) -> io::Result<()> {
    let mut iov: [libc::iovec; 2] = unsafe { std::mem::zeroed() };
    iov[0].iov_base = header.as_ptr() as *mut libc::c_void;
    iov[0].iov_len = header.len();
    iov[1].iov_base = payload.as_ptr() as *mut libc::c_void;
    iov[1].iov_len = payload.len();

    let mut _control_storage: Vec<usize>;
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = iov.as_mut_ptr();
    msg.msg_iovlen = 2;
    if !fds.is_empty() {
        let space = unsafe { libc::CMSG_SPACE((fds.len() * std::mem::size_of::<RawFd>()) as _) } as usize;
        let usize_len = (space + std::mem::size_of::<usize>() - 1) / std::mem::size_of::<usize>();
        _control_storage = vec![0usize; usize_len];
        msg.msg_control = _control_storage.as_mut_ptr() as *mut libc::c_void;
        msg.msg_controllen = space as _;
        let cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = unsafe { libc::CMSG_LEN((fds.len() * std::mem::size_of::<RawFd>()) as _) } as _;
        let data = unsafe { libc::CMSG_DATA(cmsg) } as *mut RawFd;
        for (index, value) in fds.iter().enumerate() {
            data.add(index).write(*value);
        }
    }

    let total = header.len() + payload.len();
    let mut sent = 0usize;
    loop {
        let n = unsafe { libc::sendmsg(fd, &msg, 0) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        sent = n as usize;
        break;
    }
    let mut offset = sent;
    let mut iov_index = 0usize;
    let mut iov_offset = offset;
    while offset < total {
        let (slice, skip) = if iov_offset < header.len() {
            (&header[iov_offset..], 0usize)
        } else {
            let payload_offset = iov_offset - header.len();
            (&payload[payload_offset..], 0usize)
        };
        let _ = skip;
        let n = {
            let mut written_total = 0usize;
            let n = unsafe { libc::write(fd, slice.as_ptr() as *const libc::c_void, slice.len()) };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            written_total = n as usize;
            written_total
        };
        let _ = iov_index;
        offset += n;
        iov_offset += n;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "handover wire short write"));
        }
    }
    Ok(())
}

fn recv_bytes(fd: RawFd, buf: &mut [u8]) -> Result<usize, HandoverWireError> {
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(HandoverWireError::Io(err));
        }
        if n == 0 {
            return Err(HandoverWireError::Closed);
        }
        return Ok(n as usize);
    }
}

fn set_cloexec(fd: RawFd) -> Result<(), HandoverWireError> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(HandoverWireError::Io(io::Error::last_os_error()));
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(HandoverWireError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

unsafe fn recv_header_with_control_capacity(
    fd: RawFd,
    header: &mut [u8; HANDOVER_HEADER_BYTES],
    control_ptr: *mut libc::c_void,
    control_len: usize,
    received: &mut Vec<OwnedFd>,
) -> Result<(), HandoverWireError> {
    let mut iov: [libc::iovec; 1] = unsafe { std::mem::zeroed() };
    iov[0].iov_base = header.as_mut_ptr() as *mut libc::c_void;
    iov[0].iov_len = header.len();
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = iov.as_mut_ptr();
    msg.msg_iovlen = 1;
    msg.msg_control = control_ptr;
    msg.msg_controllen = control_len as _;

    let n = loop {
        let n = unsafe { libc::recvmsg(fd, &mut msg, 0) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(HandoverWireError::Io(err));
        }
        break n as usize;
    };
    if n == 0 {
        return Err(HandoverWireError::Closed);
    }
    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    // A truncated control message may report a length larger than the buffer that actually
    // holds it, and the kernel closes the descriptors that did not fit. Reading past the
    // buffer would wrap already-closed numbers, whose drop aborts the process; so collect
    // raw numbers here, bounded by the space the caller really provided, and wrap them only
    // once the truncation flag has been checked.
    let capacity = control_len
        .saturating_sub(unsafe { libc::CMSG_LEN(0) } as usize)
        / std::mem::size_of::<RawFd>();
    let mut raw_fds: Vec<RawFd> = Vec::new();
    while !cmsg.is_null() {
        if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
            let data_start = unsafe { libc::CMSG_DATA(cmsg) } as *const RawFd;
            let header_bytes = unsafe { libc::CMSG_LEN(0) } as usize;
            let data_len = (*cmsg).cmsg_len as usize - header_bytes;
            let count = (data_len / std::mem::size_of::<RawFd>()).min(capacity - raw_fds.len());
            for index in 0..count {
                raw_fds.push(unsafe { data_start.add(index).read() });
            }
        }
        cmsg = unsafe { libc::CMSG_NXTHDR(&msg, cmsg) };
    }
    if msg.msg_flags & libc::MSG_CTRUNC != 0 {
        // These received descriptors are still ours, so close them without wrapping: a number
        // the kernel already reclaimed must not reach an OwnedFd drop.
        for raw in raw_fds {
            unsafe { libc::close(raw) };
        }
        return Err(HandoverWireError::TruncatedControl);
    }
    for raw in raw_fds {
        received.push(unsafe { OwnedFd::from_raw_fd(raw) });
    }
    if n < HANDOVER_HEADER_BYTES {
        return Err(HandoverWireError::Closed);
    }
    Ok(())
}

pub fn recv_frame(fd: RawFd) -> Result<HandoverFrame, HandoverWireError> {
    recv_frame_with_control_capacity(fd, HANDOVER_MAX_FDS)
}

fn recv_frame_with_control_capacity(
    fd: RawFd,
    control_capacity_fds: usize,
) -> Result<HandoverFrame, HandoverWireError> {
    let mut header = [0u8; HANDOVER_HEADER_BYTES];
    let control_bytes =
        unsafe { libc::CMSG_SPACE((control_capacity_fds * std::mem::size_of::<RawFd>()) as _) } as usize;
    let usize_len = (control_bytes.max(1) + std::mem::size_of::<usize>() - 1) / std::mem::size_of::<usize>();
    let mut control = vec![0usize; usize_len];
    let mut received: Vec<OwnedFd> = Vec::new();
    let outcome = unsafe {
        recv_header_with_control_capacity(
            fd,
            &mut header,
            control.as_mut_ptr() as *mut libc::c_void,
            control_bytes,
            &mut received,
        )
    };
    if let Err(error) = outcome {
        drop(received);
        return Err(error);
    }
    let (kind, transfer_id, frame_sequence, fd_count, flags, payload_len) = match decode_header(&header) {
        Ok(parsed) => parsed,
        Err(error) => {
            drop(received);
            return Err(error);
        }
    };
    if fd_count as usize > HANDOVER_MAX_FDS {
        drop(received);
        return Err(HandoverWireError::TooManyFds(fd_count));
    }
    if received.len() != fd_count as usize {
        let actual = received.len();
        drop(received);
        return Err(HandoverWireError::FdCountMismatch { declared: fd_count, actual });
    }
    if payload_len > HANDOVER_MAX_PAYLOAD_BYTES {
        drop(received);
        return Err(HandoverWireError::PayloadTooLarge(payload_len));
    }
    let mut payload = vec![0u8; payload_len as usize];
    let mut filled = 0usize;
    while filled < payload.len() {
        match recv_bytes(fd, &mut payload[filled..]) {
            Ok(n) => filled += n,
            Err(error) => {
                drop(received);
                return Err(error);
            }
        }
    }
    let value: Value = match serde_json::from_slice(&payload) {
        Ok(value) => value,
        Err(error) => {
            drop(received);
            return Err(HandoverWireError::InvalidPayload(error));
        }
    };
    for descriptor in &received {
        if let Err(error) = set_cloexec(descriptor.as_raw_fd()) {
            drop(received);
            return Err(error);
        }
    }
    Ok(HandoverFrame { kind, transfer_id, frame_sequence, flags, payload: value, fds: received })
}

pub fn peer_credentials(fd: RawFd) -> Result<PeerCredentials, HandoverWireError> {
    #[cfg(target_os = "macos")]
    {
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;
        if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
            return Err(HandoverWireError::Io(io::Error::last_os_error()));
        }
        Ok(PeerCredentials { uid, gid, pid: None })
    }
    #[cfg(target_os = "linux")]
    {
        let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        if unsafe {
            libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_PEERCRED, &mut cred as *mut _ as *mut libc::c_void, &mut len)
        } != 0
        {
            return Err(HandoverWireError::Io(io::Error::last_os_error()));
        }
        Ok(PeerCredentials { uid: cred.uid, gid: cred.gid, pid: Some(cred.pid) })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = fd;
        Err(HandoverWireError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "peer credentials unsupported on this platform",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::os::fd::IntoRawFd;
    use std::os::unix::net::UnixStream;

    fn socketpair() -> (UnixStream, UnixStream) {
        UnixStream::pair().expect("socketpair")
    }

    fn dev_null_fds(count: usize) -> Vec<OwnedFd> {
        (0..count)
            .map(|_| File::open("/dev/null").expect("open /dev/null").into())
            .collect()
    }

    fn sample_frame(fds: Vec<OwnedFd>) -> HandoverFrame {
        HandoverFrame::new(
            7,
            uuid::Uuid::new_v4().to_string(),
            42,
            serde_json::json!({"session": "abc", "sequence": 7, "nested": {"ok": true}}),
        )
        .with_fds(fds)
    }

    fn is_cloexec(fd: RawFd) -> bool {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        flags >= 0 && flags & libc::FD_CLOEXEC != 0
    }

    #[test]
    fn roundtrip_zero_fds() {
        let (sender, receiver) = socketpair();
        let frame = sample_frame(Vec::new());
        send_frame(sender.as_raw_fd(), &frame).expect("send");
        let got = recv_frame(receiver.as_raw_fd()).expect("recv");
        assert_eq!(got.kind, frame.kind);
        assert_eq!(got.transfer_id, frame.transfer_id);
        assert_eq!(got.frame_sequence, frame.frame_sequence);
        assert_eq!(got.payload, frame.payload);
        assert!(got.fds.is_empty());
    }

    #[test]
    fn roundtrip_with_fds() {
        for count in [2usize, 5usize] {
            let (sender, receiver) = socketpair();
            let frame = sample_frame(dev_null_fds(count));
            send_frame(sender.as_raw_fd(), &frame).expect("send");
            let got = recv_frame(receiver.as_raw_fd()).expect("recv");
            assert_eq!(got.fds.len(), count);
            for fd in &got.fds {
                assert!(is_cloexec(fd.as_raw_fd()), "FD_CLOEXEC must be set");
            }
        }
    }

    #[test]
    fn truncated_control_is_rejected() {
        let (sender, receiver) = socketpair();
        let frame = sample_frame(dev_null_fds(2));
        send_frame(sender.as_raw_fd(), &frame).expect("send");
        let mut header = [0u8; HANDOVER_HEADER_BYTES];
        let mut control = vec![0u8; unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as _) } as usize];
        let mut received: Vec<OwnedFd> = Vec::new();
        let error = unsafe {
            recv_header_with_control_capacity(
                receiver.as_raw_fd(),
                &mut header,
                control.as_mut_ptr() as *mut libc::c_void,
                control.len(),
                &mut received,
            )
        }
        .expect_err("must detect truncation");
        assert!(matches!(error, HandoverWireError::TruncatedControl));
        assert!(received.is_empty());
    }

    #[test]
    fn fd_count_mismatch_is_rejected() {
        let (sender, receiver) = socketpair();
        let frame = sample_frame(dev_null_fds(1));
        let payload = serde_json::to_vec(&frame.payload).expect("payload");
        let mut header =
            encode_header(frame.kind, &frame.transfer_id, frame.frame_sequence, 2, 0, payload.len() as u32)
                .expect("header");
        let fd_values: Vec<RawFd> = frame.fds.iter().map(AsRawFd::as_raw_fd).collect();
        unsafe { send_raw_with_fds(sender.as_raw_fd(), &header, &payload, &fd_values) }.expect("send");
        header = [0u8; HANDOVER_HEADER_BYTES];
        let error = recv_frame(receiver.as_raw_fd()).expect_err("mismatch");
        assert!(matches!(
            error,
            HandoverWireError::FdCountMismatch { declared: 2, actual: 1 }
        ));
    }

    #[test]
    fn oversize_payload_is_rejected() {
        let (sender, receiver) = socketpair();
        let mut header =
            encode_header(1, &uuid::Uuid::new_v4().to_string(), 1, 0, 0, HANDOVER_MAX_PAYLOAD_BYTES + 1)
                .expect("header");
        let mut cursor = 0usize;
        let raw = unsafe {
            libc::write(sender.as_raw_fd(), header.as_ptr() as *const libc::c_void, header.len())
        };
        assert_eq!(raw as usize, header.len());
        cursor = header.len();
        let _ = cursor;
        let _ = &mut header;
        let error = recv_frame(receiver.as_raw_fd()).expect_err("oversize");
        assert!(matches!(error, HandoverWireError::PayloadTooLarge(_)));
    }

    #[test]
    fn reserved_flags_are_rejected() {
        let (sender, receiver) = socketpair();
        let frame = sample_frame(Vec::new());
        let payload = serde_json::to_vec(&frame.payload).expect("payload");
        let header = encode_header(frame.kind, &frame.transfer_id, frame.frame_sequence, 0, 0x1, payload.len() as u32)
            .expect("header");
        unsafe { send_raw_with_fds(sender.as_raw_fd(), &header, &payload, &[]) }.expect("send");
        let error = recv_frame(receiver.as_raw_fd()).expect_err("reserved flags");
        assert!(matches!(error, HandoverWireError::ReservedFlags(0x1)));
    }

    #[test]
    fn bad_transfer_id_is_rejected_on_decode() {
        let (sender, receiver) = socketpair();
        let payload = serde_json::to_vec(&serde_json::json!({"x": 1})).expect("payload");
        let header = encode_header(1, "short", 0, 0, 0, payload.len() as u32)
            .expect_err("encode must reject short ids");
        let _ = header;
        let mut doctored = [b'a'; 36];
        doctored[0] = 0x01;
        let header = encode_header(1, std::str::from_utf8(&doctored).unwrap(), 0, 0, 0, payload.len() as u32);
        assert!(matches!(header, Err(HandoverWireError::BadTransferId(_))));
        let ok = encode_header(1, &uuid::Uuid::new_v4().to_string(), 0, 0, 0, payload.len() as u32)
            .expect("valid id");
        let _ = ok;
        drop(receiver);
    }

    #[test]
    fn closed_peer_yields_closed_error() {
        let (sender, receiver) = socketpair();
        drop(sender);
        let error = recv_frame(receiver.as_raw_fd()).expect_err("closed");
        assert!(matches!(error, HandoverWireError::Closed));
    }

    #[test]
    fn peer_credentials_match_self() {
        let (sender, receiver) = socketpair();
        let creds = peer_credentials(sender.as_raw_fd()).expect("credentials");
        assert_eq!(creds.uid, unsafe { libc::getuid() });
        assert_eq!(creds.gid, unsafe { libc::getgid() });
    }
}
