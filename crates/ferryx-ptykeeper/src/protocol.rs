//! Keeper wire protocol: framing, verbs and descriptor attachment.
//!
//! The framing is the daemon's handover framing (`src-tauri/src/daemon/handover_wire.rs`) with a
//! keeper magic and a keeper max-frame cap. It is duplicated rather than depended on: the daemon
//! is a Tauri library crate, and the keeper's whole value is that it links almost nothing.
//!
//! Header, 60 bytes, big endian where numeric:
//!
//! ```text
//!  0..4   magic "FXKP"
//!  4..6   framing version (u16)  -- must equal FRAMING_VERSION
//!  6..8   kind (u16)             -- see `Kind`
//!  8..12  payload length (u32)   -- JSON bytes, <= MAX_FRAME_PAYLOAD_BYTES
//! 12..48  request id (36 bytes)  -- ASCII, a UUID string; `-` workspace padding for none
//! 48..56  frame sequence (u64)
//! 56..58  fd count (u16)         -- SCM_RIGHTS descriptors attached to this frame
//! 58..60  flags (u16)            -- reserved, must be 0
//! ```
//!
//! Versioning rule from the plan: the protocol is **additive only within a major version**. A new
//! verb is a new `Kind`; a new payload field must be `Option`/defaulted on the receiving side. The
//! daemon is the side that must understand every keeper generation it may meet.
//!
//! ## Framing: one reply, or one framed stream
//!
//! Most verbs are answered by exactly one frame carrying the request's `request_id` and
//! `frame_sequence`: an ack of their own kind, their own kind echoed (`RESIZE`, `SIGNAL`, `KILL`,
//! `DETACH`), or an `ERROR`. Three verbs are answered by a **run** of frames instead, because their
//! answer is not bounded by one frame:
//!
//! * `REPLAY` -> `REPLAY_CHUNK`* then `REPLAY_END`;
//! * `ATTACH` when the retained suffix does not fit an inline field -> `ATTACH_ACK`, then
//!   `REPLAY_CHUNK`* then `REPLAY_END` (the ack says `dataChunked: true`);
//! * `CHECKPOINT_FETCH` -> `CHECKPOINT_FETCH_CHUNK`* then `CHECKPOINT_FETCH_END`.
//!
//! Every frame of a run carries the request's ids, the keeper writes a run without interleaving any
//! other frame (one writer lock per connection), and a reader knows the run ended only at its end
//! frame or at an `ERROR`. A reader must therefore correlate on `request_id`, never on "the next
//! frame".
//!
//! Frames the keeper originates -- `OUTPUT` for live output, `REPLAY_GAP_EVENT` for a lossy reset,
//! `SHUTDOWN` -- carry [`NO_REQUEST_ID`] because no request is outstanding for them. A reader that
//! attached must be prepared for them between its own replies.
//!
//! ## SIGPIPE
//!
//! The keeper must outlive a daemon that dies mid-frame, so every send suppresses `SIGPIPE` **per
//! socket** (`SO_NOSIGPIPE` on Darwin, `MSG_NOSIGNAL` on Linux) and turns a dead peer into an
//! `EPIPE` the connection reports and closes. The process-wide disposition is deliberately left
//! alone: changing it would also change how the keeper's children behave.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use serde_json::Value;

pub const MAGIC: [u8; 4] = *b"FXKP";
pub const FRAMING_VERSION: u16 = 1;
pub const HEADER_BYTES: usize = 60;
/// Plan limit: max frame 1 MiB. The payload is JSON; chunks of a large checkpoint are carried in
/// separate `CHECKPOINT_CHUNK` frames whose payload stays under this.
pub const MAX_FRAME_PAYLOAD_BYTES: u32 = 1024 * 1024;
pub const MAX_FDS: usize = 4;
pub const MAX_CONNECTIONS: usize = 8;
pub const MAX_INFLIGHT_FRAMES: usize = 32;
/// Raw bytes per `CHECKPOINT_FETCH_CHUNK`. Base64 keeps one chunk near 87 KiB, comfortably under
/// the 1 MiB frame cap.
pub const CHECKPOINT_FETCH_CHUNK_BYTES: usize = 64 * 1024;
/// Most raw checkpoint bytes one `CHECKPOINT_FETCH` pages out. A 32 MiB checkpoint therefore takes
/// at least 64 round trips and never arrives as one allocation; 512 KiB of base64 (683 KiB) plus
/// the rest of the payload stays under the 1 MiB frame cap with room to spare.
pub const CHECKPOINT_FETCH_PAGE_BYTES: u64 = 512 * 1024;
/// Largest retained suffix an `ATTACH_ACK` carries inline. A longer suffix is refused inline and
/// served as a framed run (`chunked`) or through `REPLAY`, so no reply can exceed the frame cap.
pub const ATTACH_INLINE_MAX_BYTES: usize = 512 * 1024;

/// Wire verbs. `Kind` values are pinned; never renumber one that has shipped.
///
/// A request is answered by exactly one frame with the same `request_id` and `frame_sequence`, by a
/// framed run as described in the module docs, or by an `ERROR` frame carrying the same correlation
/// ids. The run is what `REPLAY`, a large `ATTACH` and `CHECKPOINT_FETCH` use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Kind {
    // -- handshake ------------------------------------------------------------------------------
    /// `{protocolMajor, protocolMinor, keeperVersion}` -> `HELLO_ACK`. Refuses on an unsupported
    /// major with `BAD_FRAME`.
    Hello = 0x0001,
    HelloAck = 0x0002,

    // -- session lifecycle ----------------------------------------------------------------------
    /// `{sessionId, argv[], env{}, cwd, cols, rows, requestId}` with no descriptors.
    ///
    /// Idempotent on `requestId` in the two cases the keeper can still remember: the session is live
    /// (the reply is that session, not a second PTY), or the session is gone but the id is still held
    /// in the keeper's bounded in-memory table (`SPAWN_REQUEST_RETIRED`, no second PTY). A caller
    /// must therefore *reuse* the id when retrying the same logical spawn and mint a fresh one only
    /// for a genuinely new spawn. Because that table holds the most recent ids only and dies with the
    /// keeper, no reply can prove that an id it does not mention will never fork: the guarantee is
    /// per remembered id, never global.
    Spawn = 0x0010,
    /// `{sessionId}` -> `{pid, pgid, cols, rows, epoch, sequenceOffset}`.
    SpawnAck = 0x0011,
    /// `{sessionId, data(base64)}` -> `{written}`.
    Write = 0x0020,
    /// `{sessionId, cols, rows}`.
    Resize = 0x0030,
    /// `{sessionId, signal}` (`"interrupt"|"terminate"|"kill"|"stop"|"continue"`).
    Signal = 0x0031,
    /// `{sessionId}` -> kills the child and forgets the session.
    Kill = 0x0040,

    // -- attach and replay ----------------------------------------------------------------------
    /// `{sessionId, epoch, afterSequence}` -> `ATTACH_ACK`, then a stream of `REPLAY_CHUNK`.
    /// A stale epoch is refused with `EPOCH_MISMATCH`.
    Attach = 0x0050,
    /// `{sessionId, epoch, checkpointId?, sequenceOffset, replayGap?}`.
    AttachAck = 0x0051,
    /// `{sessionId, epoch, sequenceOffset, data(base64)}`. Bytes are opaque; never parsed.
    Output = 0x0052,
    /// `{sessionId, epoch, sequenceOffset}` -- "no output was produced since `sequenceOffset`",
    /// so a polling consumer can refresh its head without spinning.
    Head = 0x0053,
    /// `{sessionId, sequenceOffset}`.
    HeadAck = 0x0054,
    /// `{sessionId, fromSequence}` -> a run of `REPLAY_CHUNK` frames then `REPLAY_END`.
    Replay = 0x0060,
    /// `{sessionId, sequenceOffset, data(base64)}`.
    ReplayChunk = 0x0061,
    /// `{sessionId, sequenceOffset, more}`.
    ReplayEnd = 0x0062,
    /// `{sessionId, detail{requestedAfterSequence, availableFromSequence}}`. Emitted when the
    /// replay log discarded retained frames past what the consumer asked for.
    ReplayGapEvent = 0x0063,
    /// `{sessionId}` -> `{sessionId, owner, pid, epoch, cols, rows, sequenceOffset}`.
    OwnershipQuery = 0x0070,
    /// `{sessions: [...]}` -- authoritative discovery; the keeper is the source of truth for
    /// which sessions exist.
    OwnershipList = 0x0071,

    // -- checkpoint transport (the keeper never parses; the daemon produces) ---------------------
    /// `{sessionId, checkpointId, sequenceOffset, totalBytes, data(base64)?}`. `data` present and
    /// complete commits in one frame; otherwise the session opens a chunked upload under
    /// `checkpointId` and the daemon follows with `CHECKPOINT_CHUNK` frames.
    CheckpointInstall = 0x0080,
    /// `{sessionId, checkpointId, chunkIndex, data(base64)}`.
    CheckpointChunk = 0x0081,
    /// `{sessionId, checkpointId, sequenceOffset}`.
    CheckpointCommit = 0x0082,
    /// `{sessionId, checkpointId, sequenceOffset, storedBytes}`.
    CheckpointAck = 0x0083,
    /// `{sessionId}` -> `{checkpointId?, sequenceOffset, storedBytes}`.
    CheckpointQuery = 0x0084,
    /// `{sessionId, checkpointId, sequenceOffset, fromByte, maxBytes?}` -> a run of
    /// `CHECKPOINT_FETCH_CHUNK` then `CHECKPOINT_FETCH_END`, or one `ERROR`. Retrieval is paged so
    /// one answer never copies a whole 32 MiB checkpoint: `fromByte` is the absolute byte offset in
    /// the committed checkpoint and `maxBytes` is clamped to the keeper's page cap.
    ///
    /// Every chunk and the end frame repeat `checkpointId`, `sequenceOffset` and `totalBytes`, so a
    /// reader that pages across a replacement can detect it instead of concatenating two commits.
    CheckpointFetch = 0x0085,
    /// `{sessionId, checkpointId, sequenceOffset, totalBytes, fromByte, chunkIndex, data(base64)}`.
    CheckpointFetchChunk = 0x0086,
    /// `{sessionId, checkpointId, sequenceOffset, totalBytes, fromByte, nextByte, complete}`.
    CheckpointFetchEnd = 0x0087,

    // -- keeper administration ------------------------------------------------------------------
    /// `{sessionId, epoch}` -- releases the attachment but keeps the session and its PTY.
    Detach = 0x0090,
    /// `{code, message, offset?}`.
    Error = 0x00f0,
    /// The keeper is about to exit; the peer should reconnect after it is replaced.
    Shutdown = 0x00f1,
}

impl Kind {
    pub fn from_u16(raw: u16) -> Option<Self> {
        use Kind::*;
        Some(match raw {
            0x0001 => Hello,
            0x0002 => HelloAck,
            0x0010 => Spawn,
            0x0011 => SpawnAck,
            0x0020 => Write,
            0x0030 => Resize,
            0x0031 => Signal,
            0x0040 => Kill,
            0x0050 => Attach,
            0x0051 => AttachAck,
            0x0052 => Output,
            0x0053 => Head,
            0x0054 => HeadAck,
            0x0060 => Replay,
            0x0061 => ReplayChunk,
            0x0062 => ReplayEnd,
            0x0063 => ReplayGapEvent,
            0x0070 => OwnershipQuery,
            0x0071 => OwnershipList,
            0x0080 => CheckpointInstall,
            0x0081 => CheckpointChunk,
            0x0082 => CheckpointCommit,
            0x0083 => CheckpointAck,
            0x0084 => CheckpointQuery,
            0x0085 => CheckpointFetch,
            0x0086 => CheckpointFetchChunk,
            0x0087 => CheckpointFetchEnd,
            0x0090 => Detach,
            0x00f0 => Error,
            0x00f1 => Shutdown,
            _ => return None,
        })
    }
}

/// `send`/`sendmsg` flags that turn a closed peer into `EPIPE` rather than `SIGPIPE`.
///
/// Linux carries this on the call (`MSG_NOSIGNAL`); Darwin has no such flag and uses the per-socket
/// `SO_NOSIGPIPE` option instead, which [`suppress_sigpipe`] sets.
#[cfg(any(target_os = "linux", target_os = "android"))]
const SEND_NO_SIGNAL: libc::c_int = libc::MSG_NOSIGNAL;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const SEND_NO_SIGNAL: libc::c_int = 0;

/// Makes writes to this socket report a vanished peer instead of raising `SIGPIPE`.
///
/// A daemon that dies between two frames must not kill the keeper: the default `SIGPIPE`
/// disposition terminates the process, taking every PTY it owns with it. This is done per socket on
/// purpose -- installing a process-wide handler would also change how the keeper's children see the
/// signal, and the keeper only needs its own sockets covered.
///
/// Errors are ignored on purpose: the call is a no-op on platforms without the option, and a
/// non-socket descriptor (a pipe in a test) has no peer to signal. Linux relies on
/// [`SEND_NO_SIGNAL`] on every send instead.
pub fn suppress_sigpipe(fd: RawFd) {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        let on: libc::c_int = 1;
        unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_NOSIGPIPE,
                std::ptr::addr_of!(on).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        let _ = fd;
    }
}

/// The process-wide `SIGPIPE` disposition as a raw `sigaction.sa_sigaction` value, or `usize::MAX`
/// if it cannot be read. Comparable values prove the per-socket suppression changed nothing global.
pub fn sigpipe_disposition() -> usize {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    if unsafe { libc::sigaction(libc::SIGPIPE, std::ptr::null(), &mut action) } != 0 {
        return usize::MAX;
    }
    action.sa_sigaction
}

/// Caps how long a send may block on this socket.
///
/// A peer that stops reading would otherwise pin the connection's thread inside `send` for as long
/// as it stays connected, holding its framed run open. With a timeout the send fails with
/// `EAGAIN`/`EWOULDBLOCK`, the connection closes, and the keeper is free again while the client's
/// session keeps running.
pub fn set_send_timeout(fd: RawFd, timeout: std::time::Duration) -> io::Result<()> {
    let tv = libc::timeval {
        tv_sec: timeout.as_secs() as libc::time_t,
        tv_usec: timeout.subsec_micros() as libc::suseconds_t,
    };
    let result = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDTIMEO,
            std::ptr::addr_of!(tv).cast(),
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[derive(Debug)]
pub enum WireError {
    Io(io::Error),
    Closed,
    BadMagic,
    BadFramingVersion(u16),
    ReservedFlags(u16),
    BadRequestId(String),
    PayloadTooLarge(u32),
    TooManyFds(u16),
    TruncatedControl,
    FdCountMismatch { declared: u16, actual: usize },
    UnknownKind(u16),
    InvalidPayload(serde_json::Error),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "keeper wire io error: {e}"),
            Self::Closed => write!(f, "keeper wire closed by peer"),
            Self::BadMagic => write!(f, "keeper wire bad magic"),
            Self::BadFramingVersion(v) => write!(f, "keeper wire framing version {v} unsupported"),
            Self::ReservedFlags(v) => write!(f, "keeper wire reserved flags set: {v}"),
            Self::BadRequestId(id) => write!(f, "keeper wire bad request id {id:?}"),
            Self::PayloadTooLarge(n) => write!(f, "keeper wire payload too large: {n}"),
            Self::TooManyFds(n) => write!(f, "keeper wire too many fds: {n}"),
            Self::TruncatedControl => write!(f, "keeper wire control data truncated"),
            Self::FdCountMismatch { declared, actual } => {
                write!(f, "keeper wire fd count mismatch: declared {declared}, received {actual}")
            }
            Self::UnknownKind(k) => write!(f, "keeper wire unknown frame kind {k:#x}"),
            Self::InvalidPayload(e) => write!(f, "keeper wire invalid payload: {e}"),
        }
    }
}

impl std::error::Error for WireError {}

impl From<io::Error> for WireError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    pub uid: u32,
    pub gid: u32,
    pub pid: Option<i32>,
}

#[derive(Debug)]
pub struct Frame {
    pub kind: Kind,
    pub request_id: String,
    pub frame_sequence: u64,
    pub payload: Value,
    pub fds: Vec<OwnedFd>,
}

impl Frame {
    pub fn new(kind: Kind, request_id: impl Into<String>, frame_sequence: u64, payload: Value) -> Self {
        Self { kind, request_id: request_id.into(), frame_sequence, payload, fds: Vec::new() }
    }

    /// Attaches descriptors. The only keeper frame that carries any is a future explicit FD
    /// migration; the pattern is the handover one so no second idiom is introduced.
    pub fn with_fds(mut self, fds: Vec<OwnedFd>) -> Self {
        self.fds = fds;
        self
    }
}

/// A request id is a UUID string or, for frames with no correlation (keeper-initiated events),
/// `NONE` padded to 36 bytes. Keeping the field fixed-width mirrors the handover header and lets
/// the decoder reject a misaligned header without heuristics.
pub const NO_REQUEST_ID: &str = "------------------------------------";

fn encode_header(
    kind: Kind,
    request_id: &str,
    frame_sequence: u64,
    fd_count: u16,
    flags: u16,
    payload_len: u32,
) -> Result<[u8; HEADER_BYTES], WireError> {
    let id_bytes = request_id.as_bytes();
    if id_bytes.len() != 36 || !id_bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        return Err(WireError::BadRequestId(request_id.to_string()));
    }
    let mut header = [0u8; HEADER_BYTES];
    header[0..4].copy_from_slice(&MAGIC);
    header[4..6].copy_from_slice(&FRAMING_VERSION.to_be_bytes());
    header[6..8].copy_from_slice(&(kind as u16).to_be_bytes());
    header[8..12].copy_from_slice(&payload_len.to_be_bytes());
    header[12..48].copy_from_slice(id_bytes);
    header[48..56].copy_from_slice(&frame_sequence.to_be_bytes());
    header[56..58].copy_from_slice(&fd_count.to_be_bytes());
    header[58..60].copy_from_slice(&flags.to_be_bytes());
    Ok(header)
}

#[allow(clippy::type_complexity)]
fn decode_header(
    header: &[u8; HEADER_BYTES],
) -> Result<(Kind, String, u64, u16, u32), WireError> {
    if header[0..4] != MAGIC {
        return Err(WireError::BadMagic);
    }
    let version = u16::from_be_bytes([header[4], header[5]]);
    if version != FRAMING_VERSION {
        return Err(WireError::BadFramingVersion(version));
    }
    let raw_kind = u16::from_be_bytes([header[6], header[7]]);
    let kind = Kind::from_u16(raw_kind).ok_or(WireError::UnknownKind(raw_kind))?;
    let payload_len = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
    let id_bytes = &header[12..48];
    if !id_bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        return Err(WireError::BadRequestId(
            String::from_utf8_lossy(id_bytes).into_owned(),
        ));
    }
    let request_id = String::from_utf8_lossy(id_bytes).into_owned();
    let frame_sequence = u64::from_be_bytes(header[48..56].try_into().expect("8 bytes"));
    let fd_count = u16::from_be_bytes([header[56], header[57]]);
    let flags = u16::from_be_bytes([header[58], header[59]]);
    if flags != 0 {
        return Err(WireError::ReservedFlags(flags));
    }
    Ok((kind, request_id, frame_sequence, fd_count, payload_len))
}

/// Sends one frame. Descriptors, when present, ride the same `sendmsg` call as the header so a
/// reader never observes a header whose descriptors are still in flight.
pub fn send_frame(fd: RawFd, frame: &Frame) -> Result<(), WireError> {
    let (header, payload) = encode_parts(frame)?;
    let fd_values: Vec<RawFd> = frame.fds.iter().map(AsRawFd::as_raw_fd).collect();
    send_raw_with_fds(fd, &header, &payload, &fd_values).map_err(WireError::Io)
}

fn encode_parts(frame: &Frame) -> Result<([u8; HEADER_BYTES], Vec<u8>), WireError> {
    let payload = serde_json::to_vec(&frame.payload).map_err(WireError::InvalidPayload)?;
    if payload.len() as u64 > MAX_FRAME_PAYLOAD_BYTES as u64 {
        return Err(WireError::PayloadTooLarge(payload.len() as u32));
    }
    if frame.fds.len() > MAX_FDS {
        return Err(WireError::TooManyFds(frame.fds.len() as u16));
    }
    let header = encode_header(
        frame.kind,
        &frame.request_id,
        frame.frame_sequence,
        frame.fds.len() as u16,
        0,
        payload.len() as u32,
    )?;
    Ok((header, payload))
}

/// A frame as the exact bytes it occupies on the wire. Used by a queue that must account for a
/// reply's real size before it is written, so the charge and the write are the same bytes.
pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, WireError> {
    if !frame.fds.is_empty() {
        return Err(WireError::TooManyFds(frame.fds.len() as u16));
    }
    let (header, payload) = encode_parts(frame)?;
    let mut bytes = Vec::with_capacity(header.len() + payload.len());
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Writes one already-encoded frame. Same SIGPIPE posture as [`send_frame`].
pub fn write_frame_bytes(fd: RawFd, bytes: &[u8]) -> io::Result<()> {
    suppress_sigpipe(fd);
    let mut written = 0usize;
    while written < bytes.len() {
        let chunk = &bytes[written..];
        let n = loop {
            let n = unsafe {
                libc::send(fd, chunk.as_ptr() as *const libc::c_void, chunk.len(), SEND_NO_SIGNAL)
            };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            break n as usize;
        };
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "keeper wire short write"));
        }
        written += n;
    }
    Ok(())
}

pub(crate) fn send_raw_with_fds(
    fd: RawFd,
    header: &[u8; HEADER_BYTES],
    payload: &[u8],
    fds: &[RawFd],
) -> io::Result<()> {
    suppress_sigpipe(fd);
    let mut iov: [libc::iovec; 2] = unsafe { std::mem::zeroed() };
    iov[0].iov_base = header.as_ptr() as *mut libc::c_void;
    iov[0].iov_len = header.len();
    iov[1].iov_base = payload.as_ptr() as *mut libc::c_void;
    iov[1].iov_len = payload.len();

    // Owned here so the kernel-visible control buffer outlives the sendmsg call on every path.
    let mut control_storage: Vec<usize> = Vec::new();
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = iov.as_mut_ptr();
    msg.msg_iovlen = 2;
    if !fds.is_empty() {
        let space = unsafe { libc::CMSG_SPACE(std::mem::size_of_val(fds) as _) } as usize;
        let usize_len = space.div_ceil(std::mem::size_of::<usize>());
        control_storage.resize(usize_len, 0usize);
        msg.msg_control = control_storage.as_mut_ptr() as *mut libc::c_void;
        msg.msg_controllen = space as _;
        let cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
        unsafe {
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(fds) as _) as _;
        }
        let data = unsafe { libc::CMSG_DATA(cmsg) } as *mut RawFd;
        for (index, value) in fds.iter().enumerate() {
            unsafe { data.add(index).write(*value) };
        }
    }

    let total = header.len() + payload.len();
    let n = loop {
        // `SEND_NO_SIGNAL` is `MSG_NOSIGNAL` where the platform has it; Darwin suppresses SIGPIPE
        // through `SO_NOSIGPIPE` instead, set once per socket by `suppress_sigpipe`.
        let n = unsafe { libc::sendmsg(fd, &msg, SEND_NO_SIGNAL) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        break n as usize;
    };
    // The descriptors travel with the first successful sendmsg and are never re-sent, so a short
    // send is finished with plain writes over whatever part of header+payload is still unsent.
    let mut written = n;
    while written < total {
        let chunk = if written < header.len() { &header[written..] } else { &payload[written - header.len()..] };
        let n = loop {
            // `send`, not `write`: the remainder must keep the same no-signal property as the first
            // call, or a peer that dies between the two takes the keeper down with SIGPIPE.
            let n = unsafe {
                libc::send(fd, chunk.as_ptr() as *const libc::c_void, chunk.len(), SEND_NO_SIGNAL)
            };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            break n as usize;
        };
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "keeper wire short write"));
        }
        written += n;
    }
    Ok(())
}

fn recv_bytes(fd: RawFd, buf: &mut [u8]) -> Result<usize, WireError> {
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(WireError::Io(err));
        }
        if n == 0 {
            return Err(WireError::Closed);
        }
        return Ok(n as usize);
    }
}

fn set_cloexec(fd: RawFd) -> Result<(), WireError> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(WireError::Io(io::Error::last_os_error()));
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(WireError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

/// Receives one frame. Control data is collected into raw numbers bounded by the space actually
/// provided, and every descriptor the kernel handed over is adopted into an `OwnedFd` immediately
/// after that collection - before `MSG_CTRUNC` is checked and before the payload is read. Adoption
/// is what makes the rest of this function leak-free: every `?` from here on (a truncated control
/// message, a stalled payload, a failing `FD_CLOEXEC`) drops the `OwnedFd` values, which close
/// them. A `Vec<RawFd>` would own nothing, and a peer that repeated the failure could exhaust
/// `RLIMIT_NOFILE` and take the keeper down with every session it holds. The descriptors that did
/// not fit in the buffer were closed by the kernel, so `MSG_CTRUNC` is still an error - but the
/// ones that did fit are ours to close, and adoption is how that happens on every exit path.
pub fn recv_frame(fd: RawFd) -> Result<Frame, WireError> {
    let mut header = [0u8; HEADER_BYTES];
    let control_bytes = unsafe { libc::CMSG_SPACE((MAX_FDS * std::mem::size_of::<RawFd>()) as _) } as usize;
    let usize_len = control_bytes.max(1).div_ceil(std::mem::size_of::<usize>());
    let mut control = vec![0usize; usize_len];

    let mut iov: [libc::iovec; 1] = unsafe { std::mem::zeroed() };
    iov[0].iov_base = header.as_mut_ptr() as *mut libc::c_void;
    iov[0].iov_len = header.len();
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = iov.as_mut_ptr();
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr() as *mut libc::c_void;
    msg.msg_controllen = control_bytes as _;

    let n = loop {
        let n = unsafe { libc::recvmsg(fd, &mut msg, 0) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(WireError::Io(err));
        }
        break n as usize;
    };
    if n == 0 {
        return Err(WireError::Closed);
    }

    let capacity = control_bytes.saturating_sub(unsafe { libc::CMSG_LEN(0) } as usize)
        / std::mem::size_of::<RawFd>();
    let mut raw_fds: Vec<RawFd> = Vec::new();
    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    while !cmsg.is_null() {
        if unsafe { (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS } {
            let data_start = unsafe { libc::CMSG_DATA(cmsg) } as *const RawFd;
            let header_bytes = unsafe { libc::CMSG_LEN(0) } as usize;
            let data_len = unsafe { (*cmsg).cmsg_len as usize }.saturating_sub(header_bytes);
            let count = (data_len / std::mem::size_of::<RawFd>()).min(capacity.saturating_sub(raw_fds.len()));
            for index in 0..count {
                raw_fds.push(unsafe { data_start.add(index).read() });
            }
        }
        cmsg = unsafe { libc::CMSG_NXTHDR(&msg, cmsg) };
    }
    // Adopt every descriptor the kernel handed over *before* the next fallible step. A `Vec<RawFd>`
    // owns nothing: an early return through `?` below - a truncated header, a stalled payload - or a
    // failing CLOEXEC in the loop at the end would drop bare integers and leak the descriptors. A
    // peer that repeated that could exhaust `RLIMIT_NOFILE` and take the keeper, and every session
    // it owns, down with it. As `OwnedFd` the tail is closed on every exit path, including the
    // unwinding ones.
    let owned_fds: Vec<OwnedFd> = raw_fds
        .drain(..)
        .map(|raw| unsafe { OwnedFd::from_raw_fd(raw) })
        .collect();
    if msg.msg_flags & libc::MSG_CTRUNC != 0 {
        return Err(WireError::TruncatedControl);
    }

    let mut received_bytes = n;
    while received_bytes < HEADER_BYTES {
        received_bytes += recv_bytes(fd, &mut header[received_bytes..HEADER_BYTES])?;
    }

    let (kind, request_id, frame_sequence, fd_count, payload_len) = decode_header(&header)?;
    if fd_count as usize > MAX_FDS {
        return Err(WireError::TooManyFds(fd_count));
    }
    if owned_fds.len() != fd_count as usize {
        let actual = owned_fds.len();
        return Err(WireError::FdCountMismatch { declared: fd_count, actual });
    }
    if payload_len > MAX_FRAME_PAYLOAD_BYTES {
        return Err(WireError::PayloadTooLarge(payload_len));
    }

    let mut payload = vec![0u8; payload_len as usize];
    let mut filled = 0usize;
    while filled < payload.len() {
        filled += recv_bytes(fd, &mut payload[filled..])?;
    }
    let value: Value = serde_json::from_slice(&payload).map_err(WireError::InvalidPayload)?;
    // Iterated by reference: the vector stays the owner, so a CLOEXEC failure on one descriptor
    // still closes that one and every descriptor after it when this function returns.
    for owned in &owned_fds {
        set_cloexec(owned.as_raw_fd())?;
    }
    Ok(Frame { kind, request_id, frame_sequence, payload: value, fds: owned_fds })
}

/// Peer uid/gid (and pid where the platform offers it) of a connected unix socket.
pub fn peer_credentials(fd: RawFd) -> Result<PeerCredentials, WireError> {
    #[cfg(target_os = "macos")]
    {
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;
        if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
            return Err(WireError::Io(io::Error::last_os_error()));
        }
        Ok(PeerCredentials { uid, gid, pid: None })
    }
    #[cfg(target_os = "linux")]
    {
        let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut libc::c_void,
                &mut len,
            )
        } != 0
        {
            return Err(WireError::Io(io::Error::last_os_error()));
        }
        Ok(PeerCredentials { uid: cred.uid, gid: cred.gid, pid: Some(cred.pid) })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = fd;
        Err(WireError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "peer credentials unsupported on this platform",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::os::unix::net::UnixStream;

    fn pair() -> (UnixStream, UnixStream) {
        UnixStream::pair().expect("socketpair")
    }

    fn request_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    #[test]
    fn frame_round_trips_without_descriptors() {
        let (sender, receiver) = pair();
        let frame = Frame::new(Kind::Hello, request_id(), 7, serde_json::json!({"protocolMajor": 1}));
        send_frame(sender.as_raw_fd(), &frame).expect("send");
        let got = recv_frame(receiver.as_raw_fd()).expect("recv");
        assert_eq!(got.kind, Kind::Hello);
        assert_eq!(got.request_id, frame.request_id);
        assert_eq!(got.frame_sequence, 7);
        assert_eq!(got.payload, frame.payload);
        assert!(got.fds.is_empty());
    }

    #[test]
    fn frame_round_trips_with_descriptors_and_sets_cloexec() {
        let (sender, receiver) = pair();
        let fd: OwnedFd = File::open("/dev/null").expect("open").into();
        let frame = Frame::new(Kind::Attach, request_id(), 1, serde_json::json!({"sessionId": "s"}))
            .with_fds(vec![fd]);
        send_frame(sender.as_raw_fd(), &frame).expect("send");
        let got = recv_frame(receiver.as_raw_fd()).expect("recv");
        assert_eq!(got.fds.len(), 1);
        let flags = unsafe { libc::fcntl(got.fds[0].as_raw_fd(), libc::F_GETFD) };
        assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0, "received fd must be close-on-exec");
    }

    #[test]
    fn oversize_payload_is_refused_before_sending() {
        let (sender, _receiver) = pair();
        let big = "x".repeat(MAX_FRAME_PAYLOAD_BYTES as usize + 16);
        let frame = Frame::new(Kind::Output, request_id(), 1, serde_json::json!({ "data": big }));
        let error = send_frame(sender.as_raw_fd(), &frame).expect_err("oversize must be refused");
        assert!(matches!(error, WireError::PayloadTooLarge(_)), "got {error:?}");
    }

    #[test]
    fn reserved_flags_are_rejected_on_decode() {
        let payload = serde_json::to_vec(&serde_json::json!({"x": 1})).expect("payload");
        let mut header =
            encode_header(Kind::Hello, &request_id(), 0, 0, 0, payload.len() as u32).expect("header");
        header[58..60].copy_from_slice(&1u16.to_be_bytes());
        let error = decode_header(&header).expect_err("flags must be rejected");
        assert!(matches!(error, WireError::ReservedFlags(1)));
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let payload = serde_json::to_vec(&serde_json::json!({"x": 1})).expect("payload");
        let mut header =
            encode_header(Kind::Hello, &request_id(), 0, 0, 0, payload.len() as u32).expect("header");
        header[6..8].copy_from_slice(&0x7fffu16.to_be_bytes());
        let error = decode_header(&header).expect_err("unknown kind must be rejected");
        assert!(matches!(error, WireError::UnknownKind(0x7fff)));
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut header = [0u8; HEADER_BYTES];
        header[0..4].copy_from_slice(b"XXXX");
        assert!(matches!(decode_header(&header), Err(WireError::BadMagic)));
    }

    #[test]
    fn bad_request_id_length_is_rejected() {
        assert!(matches!(
            encode_header(Kind::Hello, "short", 0, 0, 0, 0),
            Err(WireError::BadRequestId(_))
        ));
    }

    #[test]
    fn closed_peer_reports_closed() {
        let (sender, receiver) = pair();
        drop(sender);
        assert!(matches!(recv_frame(receiver.as_raw_fd()), Err(WireError::Closed)));
    }

    #[test]
    fn peer_credentials_report_this_process() {
        let (sender, _receiver) = pair();
        let creds = peer_credentials(sender.as_raw_fd()).expect("credentials");
        assert_eq!(creds.uid, unsafe { libc::getuid() });
        assert_eq!(creds.gid, unsafe { libc::getgid() });
    }
}
