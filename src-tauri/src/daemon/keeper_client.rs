//! Daemon-side client for this runtime's PTY keeper socket.
//!
//! The keeper wire is defined by `crates/ferryx-ptykeeper/src/protocol.rs` and re-declared here
//! rather than depended on: the keeper's value is that it links almost nothing, and the daemon must
//! understand every keeper generation it may meet. Header, 60 bytes, big endian where numeric,
//! byte for byte the keeper's layout:
//!
//! ```text
//!  0..4   magic "FXKP"          12..48  request id (36 bytes, ASCII UUID)
//!  4..6   framing version u16   48..56  frame sequence (u64)
//!  6..8   kind (u16)            56..58  fd count (u16; this client attaches none)
//!  8..12  payload length (u32)  58..60  flags (u16, reserved, must be 0)
//! ```
//!
//! Additive only within a major version: a new verb is a new kind, a new payload field must be
//! optional on the receiving side, and an unknown kind is a `Protocol` error rather than a guess.
//!
//! Unix only: the keeper transport is a unix domain socket. The gate lives here rather than in
//! `mod.rs` so registering the module stays a one-line change; Windows keeps today's
//! direct-ownership path, matching `ssh/transport_windows.rs`.
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use parking_lot::{Condvar, Mutex};
use serde_json::{json, Map, Value};

use crate::daemon::crash_supervisor::KEEPER_SOCKET_NAME;

pub const KEEPER_MAGIC: [u8; 4] = *b"FXKP";
/// Framing version. Changing it breaks every generation; kinds are the additive axis instead.
pub const KEEPER_FRAMING_VERSION: u16 = 1;
pub const KEEPER_HEADER_BYTES: usize = 60;
/// 1 MiB frame cap, matching `protocol::MAX_FRAME_PAYLOAD_BYTES`.
pub const KEEPER_MAX_PAYLOAD_BYTES: u32 = 1024 * 1024;
/// The 36-dash request id of a frame with no correlation.
pub const KEEPER_NO_REQUEST_ID: &str = "------------------------------------";
/// The protocol major this client speaks; a keeper answering another major is refused, not misread.
pub const KEEPER_PROTOCOL_MAJOR: u16 = 1;

pub const KEEPER_CAPABILITY_EPOCH_FENCED_CHECKPOINT: &str = "epoch-fenced-checkpoint-v1";

pub const KEEPER_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
pub const KEEPER_VERB_TIMEOUT: Duration = Duration::from_secs(30);

/// Wire verbs. Discriminants are pinned by the keeper crate; never renumber one that has shipped.
/// Every request is answered by one reply with the same `request_id` and `frame_sequence`, or an
/// `ERROR` frame with those ids. `REPLAY` is the one verb whose answer is a run of frames;
/// `RESIZE`/`SIGNAL`/`KILL`/`DETACH` have no dedicated ack and echo their own kind instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum KeeperKind {
    Hello = 0x0001,
    HelloAck = 0x0002,
    /// `{sessionId, argv[], env{}, cwd, cols, rows, epoch, requestId}`; idempotent on `requestId`.
    Spawn = 0x0010,
    SpawnAck = 0x0011,
    Write = 0x0020,
    Resize = 0x0030,
    Signal = 0x0031,
    Kill = 0x0040,
    /// `{sessionId, epoch, afterSequence}` -> `ATTACH_ACK` carrying the retained suffix.
    Attach = 0x0050,
    AttachAck = 0x0051,
    /// `{sessionId, epoch, sequenceOffset, data(base64)}`; bytes are opaque, never parsed.
    Output = 0x0052,
    Head = 0x0053,
    HeadAck = 0x0054,
    /// `{sessionId, fromSequence}` -> `REPLAY_CHUNK` frames then `REPLAY_END`, or one gap event.
    Replay = 0x0060,
    ReplayChunk = 0x0061,
    ReplayEnd = 0x0062,
    ReplayGapEvent = 0x0063,
    /// `{sessionId}` -> `OWNERSHIP_LIST`; authoritative discovery, the keeper is the source.
    OwnershipQuery = 0x0070,
    OwnershipList = 0x0071,
    CheckpointInstall = 0x0080,
    CheckpointChunk = 0x0081,
    CheckpointCommit = 0x0082,
    CheckpointAck = 0x0083,
    CheckpointQuery = 0x0084,
    CheckpointFetch = 0x0085,
    /// `{sessionId, checkpointId, sequenceOffset, totalBytes, fromByte, chunkIndex, data(base64)}`.
    CheckpointFetchChunk = 0x0086,
    /// `{sessionId, checkpointId, sequenceOffset, totalBytes, fromByte, nextByte, complete}`.
    CheckpointFetchEnd = 0x0087,
    /// `{sessionId, epoch}` -- releases the attachment, keeps the session and its PTY.
    Detach = 0x0090,
    /// `{code, message, offset?}`.
    Error = 0x00f0,
    Shutdown = 0x00f1,
}

impl KeeperKind {
    /// The kind for a raw discriminant, or `None` when this client does not know it.
    pub fn from_u16(raw: u16) -> Option<Self> {
        use KeeperKind::*;
        Some(match raw {
            0x0001 => Hello, 0x0002 => HelloAck, 0x0010 => Spawn, 0x0011 => SpawnAck,
            0x0020 => Write, 0x0030 => Resize, 0x0031 => Signal, 0x0040 => Kill,
            0x0050 => Attach, 0x0051 => AttachAck, 0x0052 => Output, 0x0053 => Head,
            0x0054 => HeadAck, 0x0060 => Replay, 0x0061 => ReplayChunk, 0x0062 => ReplayEnd,
            0x0063 => ReplayGapEvent, 0x0070 => OwnershipQuery, 0x0071 => OwnershipList,
            0x0080 => CheckpointInstall, 0x0081 => CheckpointChunk, 0x0082 => CheckpointCommit,
            0x0083 => CheckpointAck, 0x0084 => CheckpointQuery, 0x0085 => CheckpointFetch,
            0x0086 => CheckpointFetchChunk, 0x0087 => CheckpointFetchEnd, 0x0090 => Detach,
            0x00f0 => Error, 0x00f1 => Shutdown,
            _ => return None,
        })
    }

    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

/// Everything that can go wrong on the keeper connection.
#[derive(Debug)]
pub enum KeeperClientError {
    Io(std::io::Error),
    /// The keeper's bytes or answers do not match the protocol this client speaks.
    Protocol(String),
    /// The keeper refused the request. `code` is the keeper's wire string verbatim, so a code this
    /// daemon does not know still reaches the log as the keeper wrote it.
    Refused { code: String, message: String },
}

impl std::fmt::Display for KeeperClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "keeper client io error: {error}"),
            Self::Protocol(detail) => write!(f, "keeper protocol error: {detail}"),
            Self::Refused { code, message } => write!(f, "keeper refused: {code}: {message}"),
        }
    }
}

impl std::error::Error for KeeperClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Protocol(_) | Self::Refused { .. } => None,
        }
    }
}

impl From<std::io::Error> for KeeperClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl KeeperClientError {
    /// The keeper's wire code for a refusal, so callers branch on the code, not the message.
    pub fn wire_code(&self) -> Option<&str> {
        match self {
            Self::Refused { code, .. } => Some(code.as_str()),
            Self::Io(_) | Self::Protocol(_) => None,
        }
    }

    pub fn is_epoch_mismatch(&self) -> bool {
        self.wire_code() == Some("EPOCH_MISMATCH")
    }

    pub fn is_stale_checkpoint(&self) -> bool {
        self.wire_code() == Some("STALE_CHECKPOINT")
    }
}

fn protocol_error(detail: impl Into<String>) -> KeeperClientError {
    KeeperClientError::Protocol(detail.into())
}

/// One keeper frame: a 60-byte header plus a JSON payload.
#[derive(Debug, Clone, PartialEq)]
pub struct KeeperFrame {
    pub kind: KeeperKind,
    pub request_id: String,
    pub frame_sequence: u64,
    pub payload: Value,
}

impl KeeperFrame {
    pub fn new(kind: KeeperKind, request_id: impl Into<String>, frame_sequence: u64, payload: Value)
        -> Self {
        Self { kind, request_id: request_id.into(), frame_sequence, payload }
    }

    /// Header plus payload bytes. A request id that does not fit the fixed width becomes the
    /// no-request-id padding (the keeper would refuse it anyway), and an over-cap payload keeps its
    /// true length so the keeper's own cap refuses it by name. A `Value` always serializes, so the
    /// `null` fallback is unreachable rather than a swallowed error.
    pub fn encode(&self) -> Vec<u8> {
        let payload = serde_json::to_vec(&self.payload).unwrap_or_else(|_| b"null".to_vec());
        let length = u32::try_from(payload.len()).unwrap_or(u32::MAX);
        let id = if is_request_id(self.request_id.as_bytes()) {
            self.request_id.clone()
        } else {
            KEEPER_NO_REQUEST_ID.to_string()
        };
        let mut out = vec![0u8; KEEPER_HEADER_BYTES];
        out[0..4].copy_from_slice(&KEEPER_MAGIC);
        out[4..6].copy_from_slice(&KEEPER_FRAMING_VERSION.to_be_bytes());
        out[6..8].copy_from_slice(&self.kind.as_u16().to_be_bytes());
        out[8..12].copy_from_slice(&length.to_be_bytes());
        out[12..48].copy_from_slice(id.as_bytes());
        out[48..56].copy_from_slice(&self.frame_sequence.to_be_bytes());
        // 56..58 fd count stays 0: this client attaches no descriptors, and 58..60 stays 0.
        out.extend_from_slice(&payload);
        out
    }

    /// Parses a complete frame from `bytes`; every refusal is a typed error and a short slice never
    /// indexes past its end.
    pub fn decode(bytes: &[u8]) -> Result<Self, KeeperClientError> {
        if bytes.len() < KEEPER_HEADER_BYTES {
            return Err(protocol_error("keeper frame is shorter than its header"));
        }
        let header = &bytes[..KEEPER_HEADER_BYTES];
        if header[0..4] != KEEPER_MAGIC {
            return Err(protocol_error(format!("bad keeper magic {:02x?}", &header[0..4])));
        }
        let version = u16::from_be_bytes([header[4], header[5]]);
        if version != KEEPER_FRAMING_VERSION {
            return Err(protocol_error(format!("keeper framing version {version} unsupported")));
        }
        let raw_kind = u16::from_be_bytes([header[6], header[7]]);
        let kind = KeeperKind::from_u16(raw_kind)
            .ok_or_else(|| protocol_error(format!("unknown keeper frame kind {raw_kind:#06x}")))?;
        let payload_len = u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
        if payload_len > KEEPER_MAX_PAYLOAD_BYTES as usize {
            return Err(protocol_error(format!("keeper payload {payload_len} is over the cap")));
        }
        if !is_request_id(&header[12..48]) {
            return Err(protocol_error("keeper request id is not 36 printable ASCII bytes"));
        }
        let request_id = String::from_utf8_lossy(&header[12..48]).into_owned();
        let mut sequence = [0u8; 8];
        sequence.copy_from_slice(&header[48..56]);
        let frame_sequence = u64::from_be_bytes(sequence);
        let fd_count = u16::from_be_bytes([header[56], header[57]]);
        if fd_count != 0 {
            return Err(protocol_error(format!("keeper frame declares {fd_count} descriptors")));
        }
        let flags = u16::from_be_bytes([header[58], header[59]]);
        if flags != 0 {
            return Err(protocol_error(format!("keeper reserved flags set: {flags:#06x}")));
        }
        if bytes.len() != KEEPER_HEADER_BYTES + payload_len {
            return Err(protocol_error(format!(
                "keeper frame carries {} payload bytes, header declares {payload_len}",
                bytes.len() - KEEPER_HEADER_BYTES
            )));
        }
        // An empty payload is not JSON; the keeper never sends one, so read it as `null` rather than
        // calling a zero-length frame a decode failure.
        let payload = match payload_len {
            0 => Value::Null,
            _ => serde_json::from_slice(&bytes[KEEPER_HEADER_BYTES..])
                .map_err(|error| protocol_error(format!("keeper payload is not JSON: {error}")))?,
        };
        Ok(Self { kind, request_id, frame_sequence, payload })
    }
}

/// A request id occupies a fixed 36-byte field, so it must be 36 printable ASCII bytes.
fn is_request_id(bytes: &[u8]) -> bool {
    bytes.len() == 36 && bytes.iter().all(|byte| byte.is_ascii_graphic() || *byte == b' ')
}

/// The keeper's base64 alphabet, padded, so opaque bytes travel as text its own decoder accepts.
fn base64_encode(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

/// The inverse of [`base64_encode`]; a field the keeper left out decodes to nothing, not an error,
/// because an absent `data` is how the wire says "no bytes this time".
fn decode_base64_bytes(text: &str) -> Vec<u8> {
    BASE64.decode(text).unwrap_or_default()
}

// Payload readers: a missing or mistyped field reads as the wire's zero value, never a panic. The
// keeper owns payload shape, so a verb that needs a field refuses rather than guessing.
fn str_of(payload: &Value, key: &str) -> String {
    payload.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn opt_str_of(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(str::to_string)
}

fn u64_of(payload: &Value, key: &str) -> u64 {
    payload.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// Narrower integers read as 0 rather than wrapping: the keeper's own types bound these, so an
/// out-of-range value is a malformed frame, not a number to reinterpret.
fn u32_of(payload: &Value, key: &str) -> u32 {
    u32::try_from(u64_of(payload, key)).unwrap_or(0)
}

fn u16_of(payload: &Value, key: &str) -> u16 {
    u16::try_from(u64_of(payload, key)).unwrap_or(0)
}

fn opt_bool_of(payload: &Value, key: &str) -> Option<bool> {
    payload.get(key).and_then(Value::as_bool)
}

fn capabilities_of(payload: &Value) -> Vec<String> {
    payload
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn replay_body(session_id: &str, epoch: Option<u64>, from_sequence: u64) -> Value {
    match epoch {
        Some(epoch) => json!({
            "sessionId": session_id, "epoch": epoch, "fromSequence": from_sequence,
        }),
        None => json!({ "sessionId": session_id, "fromSequence": from_sequence }),
    }
}

fn head_body(session_id: &str, epoch: Option<u64>) -> Value {
    match epoch {
        Some(epoch) => json!({ "sessionId": session_id, "epoch": epoch }),
        None => json!({ "sessionId": session_id }),
    }
}

/// The refusal a keeper `ERROR` frame carries, with the wire code kept verbatim.
fn refusal(payload: &Value) -> KeeperClientError {
    KeeperClientError::Refused {
        code: str_of(payload, "code"),
        message: str_of(payload, "message"),
    }
}

/// A replay gap: the keeper could not answer from the offset the caller asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayGap {
    /// The offset the caller asked to resume after.
    pub requested_after_sequence: u64,
    /// The oldest offset the keeper still retains; resume here.
    pub available_from_sequence: u64,
}

impl ReplayGap {
    /// The gap a `replayGap`/`detail` object describes, or `None` when either offset is absent. The
    /// keeper also sends `retainedFromSequence`; it is additive and not required here.
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            requested_after_sequence: value.get("requestedAfterSequence")?.as_u64()?,
            available_from_sequence: value.get("availableFromSequence")?.as_u64()?,
        })
    }
}

/// `SPAWN_ACK`: the PTY the keeper started, or the existing one a retried `requestId` named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnReply {
    pub session_id: String, pub pid: u32, pub pgid: u32, pub cols: u16, pub rows: u16,
    pub epoch: u64, pub sequence_offset: u64,
    /// The child's kernel start time, which together with `pid` is the identity that a reused pid
    /// cannot forge. Absent only when the keeper could not read it.
    pub process_start_time: Option<u64>,
}
/// `ATTACH_ACK`: one page of the retained suffix plus the session head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachReply {
    pub session_id: String, pub epoch: u64,
    /// The session **head**, not the end of the bytes in this reply: the keeper copies at most one
    /// page, so a longer suffix leaves the head past what a single ack delivered.
    pub sequence_offset: u64,
    pub checkpoint_id: Option<String>,
    /// The output offset the committed checkpoint covers, which is what a fetch must name.
    pub checkpoint_sequence_offset: u64,
    pub checkpoint_stored_bytes: u64,
    /// Base64 of the page of retained output that begins at the request's `afterSequence`.
    /// `sequence_offset` is the session **head**, which a page that stopped at its byte cap has not
    /// reached, so this page is only covered by `afterSequence + data.len()`.
    pub data_base64: String,
    pub gap: Option<ReplayGap>,
    /// The keeper announced the suffix as a following framed run instead of an inline field, so the
    /// caller must read `REPLAY_CHUNK`* until `REPLAY_END` before the connection is a clean tap.
    pub data_chunked: bool,
    /// The keeper armed this connection as the session's output tap. False means no output will
    /// ever be pushed here, so a caller that wants live bytes must not wait on this stream.
    pub live_output: bool,
}

/// The fields every `ATTACH_ACK` carries, whether or not the suffix rides inside it.
fn attach_reply(payload: &Value) -> AttachReply {
    AttachReply {
        session_id: str_of(payload, "sessionId"),
        epoch: u64_of(payload, "epoch"),
        sequence_offset: u64_of(payload, "sequenceOffset"),
        checkpoint_id: opt_str_of(payload, "checkpointId"),
        checkpoint_sequence_offset: u64_of(payload, "checkpointSequenceOffset"),
        checkpoint_stored_bytes: u64_of(payload, "storedBytes"),
        data_base64: str_of(payload, "data"),
        gap: payload.get("replayGap").and_then(ReplayGap::from_value),
        data_chunked: opt_bool_of(payload, "dataChunked").unwrap_or(false),
        live_output: opt_bool_of(payload, "liveOutput").unwrap_or(false),
    }
}
/// The whole `REPLAY` answer: chunks read up to `REPLAY_END`, or a gap instead of any.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplayReply {
    /// Each chunk's `sequenceOffset` and its base64 bytes, in the order the keeper sent them.
    pub chunks: Vec<(u64, String)>,
    /// Where the run's own page ends, which is where the next `REPLAY` resumes. Not the session
    /// head: a page is bounded, so the head can be far past what this run delivered.
    pub end_sequence: u64,
    pub more: bool, pub gap: Option<ReplayGap>,
}

impl ReplayReply {
    /// A gap resets the cursor to what the keeper still retains; otherwise it is the run's own end.
    pub fn resume_offset(&self) -> u64 {
        self.gap.map_or(self.end_sequence, |gap| gap.available_from_sequence)
    }
}
/// One session in `OWNERSHIP_LIST`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipEntry {
    pub session_id: String, pub pid: u32, pub epoch: u64, pub sequence_offset: u64,
    pub cols: u16, pub rows: u16, pub owner: String, pub focused: bool,
    pub generation: String,
}

/// `CHECKPOINT_ACK`: what the keeper stored, and for a chunked upload how far it got.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckpointReply {
    pub session_id: String, pub checkpoint_id: Option<String>, pub sequence_offset: u64,
    pub stored_bytes: u64, pub total_bytes: Option<u64>, pub complete: Option<bool>,
}

/// The fields every `CHECKPOINT_ACK` carries, with the chunked-upload extras left optional.
fn checkpoint_reply(payload: &Value) -> CheckpointReply {
    CheckpointReply {
        session_id: str_of(payload, "sessionId"),
        checkpoint_id: opt_str_of(payload, "checkpointId"),
        sequence_offset: u64_of(payload, "sequenceOffset"),
        stored_bytes: u64_of(payload, "storedBytes"),
        total_bytes: payload.get("totalBytes").and_then(Value::as_u64),
        complete: opt_bool_of(payload, "complete"),
    }
}

fn check_fetched_identity(payload: &Value, checkpoint_id: &str, sequence_offset: u64,
    expected_total: &mut Option<u64>) -> Result<(), KeeperClientError> {
    let fetched_id = str_of(payload, "checkpointId");
    if fetched_id != checkpoint_id {
        return Err(protocol_error(format!(
            "checkpoint fetch asked for {checkpoint_id}, keeper answered {fetched_id}"
        )));
    }
    let fetched_offset = u64_of(payload, "sequenceOffset");
    if fetched_offset != sequence_offset {
        return Err(protocol_error(format!(
            "checkpoint {checkpoint_id} was asked at offset {sequence_offset}, keeper answered \
             {fetched_offset}"
        )));
    }
    let Some(total_bytes) = payload.get("totalBytes").and_then(Value::as_u64) else {
        return Ok(());
    };
    match *expected_total {
        Some(known) if known != total_bytes => Err(protocol_error(format!(
            "checkpoint {checkpoint_id} changed size mid-fetch: {known} then {total_bytes}"
        ))),
        Some(_) => Ok(()),
        None => {
            *expected_total = Some(total_bytes);
            Ok(())
        }
    }
}

/// A blocking connection to this runtime's keeper.
///
/// One connection carries correlated request/reply traffic only. Output is read with `REPLAY` from
/// the offset a reader last delivered, never pushed: a keeper connection has exactly one role, and
/// a pool connection that never attaches can be handed to any verb without being poisoned by
/// another session's stream.
pub struct KeeperClient {
    stream: UnixStream,
    next_sequence: u64,
    capabilities: Vec<String>,
}

/// Bounds `stream`, tolerating the one platform case where the bound is moot.
///
/// Darwin's `sosetopt` answers `EINVAL` for `SO_RCVTIMEO`/`SO_SNDTIMEO` once the peer has both
/// half-closed, where Linux accepts the same call. A hung-up peer cannot block a read or a write, so
/// the missing bound changes nothing and the handshake that follows fails on its own with the true
/// reason. Every other error is returned exactly as it was: only a refusal on a socket the kernel
/// reports as hung up is treated as "no bound needed".
#[cfg(unix)]
fn arm_timeout(stream: &UnixStream, armed: std::io::Result<()>) -> Result<(), KeeperClientError> {
    match armed {
        Ok(()) => Ok(()),
        Err(error) => {
            if crate::daemon::refused_timeout_on_hung_up_peer(stream, &error) {
                Ok(())
            } else {
                Err(KeeperClientError::Io(error))
            }
        }
    }
}

impl KeeperClient {
    /// The keeper socket path for a runtime directory, derived from the runtime directory and never
    /// from `$TMPDIR`, so an isolated runtime never reaches the user's keeper.
    pub fn socket_path(runtime_dir: &Path) -> PathBuf {
        runtime_dir.join(KEEPER_SOCKET_NAME)
    }

    /// Connects and completes the handshake. A keeper answering another protocol major is a
    /// `Protocol` error: reading its payloads with this client's field meanings would be worse than
    /// refusing the connection.
    pub fn connect(runtime_dir: &Path) -> Result<Self, KeeperClientError> {
        let stream = UnixStream::connect(Self::socket_path(runtime_dir))?;
        // Arming is skipped, not failed, when Darwin refuses it because the peer already closed
        // (`EINVAL` once both `SS_CANTRCVMORE` and `SS_CANTSENDMORE` are set). A hung-up peer can
        // never block a read or a write, so the bound is moot there and the handshake below fails
        // on its own with `Closed`/`UnexpectedEof` instead of a spurious timeout-setup error.
        arm_timeout(&stream, stream.set_read_timeout(Some(KEEPER_CONNECT_TIMEOUT)))?;
        arm_timeout(&stream, stream.set_write_timeout(Some(KEEPER_CONNECT_TIMEOUT)))?;
        let mut client = Self {
            stream, next_sequence: 0, capabilities: Vec::new(),
        };
        let hello = json!({"protocolMajor": KEEPER_PROTOCOL_MAJOR, "protocolMinor": 0,
            "keeperVersion": env!("CARGO_PKG_VERSION")});
        let ack = Self::expect_kind(client.call(KeeperKind::Hello, hello)?, KeeperKind::HelloAck)?;
        let major = u64_of(&ack.payload, "protocolMajor");
        if major != u64::from(KEEPER_PROTOCOL_MAJOR) {
            return Err(protocol_error(format!(
                "keeper speaks protocol major {major}, this client speaks {KEEPER_PROTOCOL_MAJOR}"
            )));
        }
        client.capabilities = capabilities_of(&ack.payload);
        // The keeper may be gone again by now (it answers HELLO and then exits), which is the same
        // moot-bound case; the first real read on this connection reports it.
        arm_timeout(&client.stream, client.stream.set_read_timeout(Some(KEEPER_VERB_TIMEOUT)))?;
        arm_timeout(&client.stream, client.stream.set_write_timeout(Some(KEEPER_VERB_TIMEOUT)))?;
        Ok(client)
    }

    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|advertised| advertised.as_str() == capability)
    }

    /// An epoch-fencing keeper rejects an epoch-less verb, so refuse locally with it named.
    fn refuse_unfenced(&self, verb: &str) -> Result<(), KeeperClientError> {
        if self.supports(KEEPER_CAPABILITY_EPOCH_FENCED_CHECKPOINT) {
            return Err(protocol_error(format!(
                "keeper advertises {KEEPER_CAPABILITY_EPOCH_FENCED_CHECKPOINT}: {verb} needs an \
                 epoch, use the fenced variant"
            )));
        }
        Ok(())
    }

    /// Sends one frame and reads its single reply.
    fn call(&mut self, kind: KeeperKind, payload: Value) -> Result<KeeperFrame, KeeperClientError> {
        let request_id = self.send(kind, payload)?;
        self.receive(&request_id)
    }

    /// Writes one frame, returning the request id its reply must carry.
    fn send(&mut self, kind: KeeperKind, payload: Value) -> Result<String, KeeperClientError> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let frame = KeeperFrame::new(kind, request_id.clone(), self.next_sequence, payload);
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.stream.write_all(&frame.encode())?;
        Ok(request_id)
    }

    /// Reads one frame and correlates it with `request_id`, turning an `ERROR` into `Refused`.
    ///
    /// Frames the keeper pushes on its own (`OUTPUT`, `REPLAY_GAP`) carry no request id. Nothing in
    /// this client asks for a live tap, so such a frame can only be stray: it is dropped rather than
    /// buffered (which would let one session's bytes grow a connection's memory) or mistaken for the
    /// outstanding reply.
    fn receive(&mut self, request_id: &str) -> Result<KeeperFrame, KeeperClientError> {
        let reply = loop {
            let frame = self.read_frame()?;
            if Self::is_tap_frame(&frame) {
                continue;
            }
            break frame;
        };
        if reply.kind == KeeperKind::Error {
            return Err(refusal(&reply.payload));
        }
        if reply.request_id != request_id {
            return Err(protocol_error(format!(
                "keeper replied to {:?} while {request_id:?} was outstanding",
                reply.request_id
            )));
        }
        Ok(reply)
    }

    /// Whether a frame is keeper-initiated tap traffic rather than a reply to a verb.
    fn is_tap_frame(frame: &KeeperFrame) -> bool {
        frame.request_id == KEEPER_NO_REQUEST_ID
            && matches!(frame.kind, KeeperKind::Output | KeeperKind::ReplayGapEvent)
    }

    /// Reads exactly one frame, refusing an announced payload before allocating for it.
    fn read_frame(&mut self) -> Result<KeeperFrame, KeeperClientError> {
        let mut header = [0u8; KEEPER_HEADER_BYTES];
        self.stream.read_exact(&mut header)?;
        let payload_len = u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
        if payload_len > KEEPER_MAX_PAYLOAD_BYTES as usize {
            return Err(protocol_error(format!("keeper announced {payload_len} payload bytes")));
        }
        let mut bytes = vec![0u8; KEEPER_HEADER_BYTES + payload_len];
        bytes[..KEEPER_HEADER_BYTES].copy_from_slice(&header);
        if payload_len > 0 {
            self.stream.read_exact(&mut bytes[KEEPER_HEADER_BYTES..])?;
        }
        KeeperFrame::decode(&bytes)
    }

    /// Asserts the reply kind a verb's single answer must have.
    fn expect_kind(reply: KeeperFrame, kind: KeeperKind) -> Result<KeeperFrame, KeeperClientError> {
        if reply.kind == kind {
            return Ok(reply);
        }
        Err(protocol_error(format!("expected {kind:?}, keeper answered {:?}", reply.kind)))
    }

    /// A verb with no dedicated ack: the keeper echoes the request kind with what it applied.
    fn echo(&mut self, kind: KeeperKind, payload: Value) -> Result<(), KeeperClientError> {
        Self::expect_kind(self.call(kind, payload)?, kind).map(|_| ())
    }

    /// Starts a PTY session under a request id this call mints.
    ///
    /// This is for a **new** logical spawn only: the id is fresh on every call, so calling this to
    /// retry a spawn presents an id the keeper has never seen and its request-id tombstone cannot
    /// retire a delayed duplicate. A retry must use
    /// [`spawn_with_request_id`](Self::spawn_with_request_id) with the id of the first attempt.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(&mut self, session_id: &str, argv: &[String], env: &[(String, String)],
        cwd: Option<&Path>, cols: u16, rows: u16, epoch: u64) -> Result<SpawnReply, KeeperClientError> {
        self.spawn_with_request_id(session_id, argv, env, cwd, cols, rows, epoch, None)
    }

    /// Starts a PTY session. Idempotent on `request_id`, so a retry of one logical spawn returns
    /// the existing session instead of a second PTY.
    ///
    /// `request_id` is the caller's, never minted here: a per-attempt id can never match the
    /// keeper's request-id tombstone, so the retry would be indistinguishable from a new spawn once
    /// the live session is gone. `None` mints one and is therefore only correct for a spawn that
    /// cannot be retried.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_with_request_id(&mut self, session_id: &str, argv: &[String],
        env: &[(String, String)], cwd: Option<&Path>, cols: u16, rows: u16, epoch: u64,
        request_id: Option<&str>) -> Result<SpawnReply, KeeperClientError> {
        let request_id = request_id
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut env_object = Map::new();
        for (key, value) in env {
            env_object.insert(key.clone(), Value::String(value.clone()));
        }
        let payload = json!({
            "sessionId": session_id, "argv": argv, "env": env_object,
            "cwd": cwd.map(|path| path.to_string_lossy().into_owned()),
            "cols": cols, "rows": rows, "epoch": epoch,
            "requestId": request_id,
        });
        let reply = Self::expect_kind(self.call(KeeperKind::Spawn, payload)?, KeeperKind::SpawnAck)?;
        Ok(SpawnReply {
            session_id: str_of(&reply.payload, "sessionId"),
            pid: u32_of(&reply.payload, "pid"),
            pgid: u32_of(&reply.payload, "pgid"),
            cols: u16_of(&reply.payload, "cols"),
            rows: u16_of(&reply.payload, "rows"),
            epoch: u64_of(&reply.payload, "epoch"),
            sequence_offset: u64_of(&reply.payload, "sequenceOffset"),
            process_start_time: reply.payload.get("processStartTime").and_then(Value::as_u64),
        })
    }

    /// Writes bytes to the session's PTY, returning how many the keeper accepted.
    pub fn write(&mut self, session_id: &str, epoch: u64, data: &[u8]) -> Result<u64, KeeperClientError> {
        let payload = json!({ "sessionId": session_id, "epoch": epoch, "data": base64_encode(data) });
        let reply = Self::expect_kind(self.call(KeeperKind::Write, payload)?, KeeperKind::Write)?;
        Ok(u64_of(&reply.payload, "written"))
    }

    /// Resizes the session's PTY.
    pub fn resize(&mut self, session_id: &str, epoch: u64, cols: u16, rows: u16)
        -> Result<(), KeeperClientError> {
        let payload = json!({ "sessionId": session_id, "epoch": epoch, "cols": cols, "rows": rows });
        self.echo(KeeperKind::Resize, payload)
    }

    /// Signals the session's process; `signal` is one of `interrupt`, `terminate`, `kill`, `stop`,
    /// `continue`. Anything else is the keeper's `BAD_FRAME` refusal, not a silent no-op.
    pub fn signal(&mut self, session_id: &str, epoch: u64, signal: &str)
        -> Result<(), KeeperClientError> {
        let payload = json!({ "sessionId": session_id, "epoch": epoch, "signal": signal });
        self.echo(KeeperKind::Signal, payload)
    }

    /// Kills the session's child and forgets the session.
    ///
    /// `epoch` is the fence: `KILL` ends a session whose owner may have changed since the caller
    /// last held it, so the keeper acts only when the presented epoch is the newest one.
    pub fn kill(&mut self, session_id: &str, epoch: u64) -> Result<(), KeeperClientError> {
        self.echo(KeeperKind::Kill, json!({ "sessionId": session_id, "epoch": epoch }))
    }

    /// Attaches at `epoch` and takes the retained suffix back in the ack -- one round trip, applied
    /// exactly once because `sequence_offset` already covers those bytes.
    ///
    /// `live_output` asks the keeper to arm this connection as the session's output tap, which is
    /// what makes later output arrive without a poll; `chunked` lets a suffix larger than one frame
    /// arrive as a following run instead of a `FRAME_TOO_LARGE` refusal. Both flags travel with the
    /// request because the keeper decides the tap from them before it fences the epoch.
    pub fn attach(&mut self, session_id: &str, epoch: u64, after_sequence: u64,
        chunked: bool, live_output: bool) -> Result<AttachReply, KeeperClientError> {
        let payload = json!({ "sessionId": session_id, "epoch": epoch,
            "afterSequence": after_sequence, "chunked": chunked, "liveOutput": live_output });
        let reply = Self::expect_kind(self.call(KeeperKind::Attach, payload)?, KeeperKind::AttachAck)?;
        Ok(attach_reply(&reply.payload))
    }

    /// Attaches at `epoch` and takes the recovery answer with it: the ack, the retained bytes from
    /// `after_sequence`, and the offset those bytes reach, so a reader resumes without a second
    /// `ATTACH`. The keeper copies a bounded page, so a long suffix arrives one page at a time: this
    /// returns the first page and the cursor its end names, and later `REPLAY` polls continue from
    /// there.
    ///
    /// One attach matters: the keeper fences the epoch on `ATTACH` and refuses anything at or below
    /// the newest one, so a second attach at the same epoch would be an `EPOCH_MISMATCH` and the
    /// session would keep no reader at all. The returned bytes are the retained suffix from
    /// `after_sequence`; a gap leaves them empty and the caller resumes at the resume offset the
    /// keeper chose.
    ///
    /// `liveOutput` is deliberately **false**: asking for the tap would make the keeper arm this
    /// connection as the session's output stream, which the wire allows exactly one of per
    /// connection, so a daemon serving N sessions would hold N connections and be refused at the
    /// keeper's `MAX_CONNECTIONS`. The connection is released back to the pool as soon as this
    /// returns, and later output is read with [`Self::replay_fenced`] from the offset last
    /// delivered.
    pub fn attach_recovering(&mut self, session_id: &str, epoch: u64, after_sequence: u64)
        -> Result<(AttachReply, Vec<u8>, u64), KeeperClientError> {
        let payload = json!({ "sessionId": session_id, "epoch": epoch,
            "afterSequence": after_sequence, "chunked": true, "liveOutput": false });
        let request_id = self.send(KeeperKind::Attach, payload)?;
        let ack = self.receive(&request_id)?;
        let ack = Self::expect_kind(ack, KeeperKind::AttachAck)?;
        let reply = attach_reply(&ack.payload);
        if let Some(gap) = reply.gap {
            return Ok((reply, Vec::new(), gap.available_from_sequence));
        }
        if !reply.data_chunked {
            let data = decode_base64_bytes(&reply.data_base64);
            // The page begins at the offset the caller asked from, so its own length says where it
            // ends. Taking the ack's `sequenceOffset` would report the session **head**, which is
            // past what this page delivered whenever the suffix exceeded the keeper's page cap: the
            // reader then resumes above bytes it never received, and the rest of the retained
            // history is never polled.
            let offset = after_sequence.saturating_add(data.len() as u64);
            return Ok((reply, data, offset));
        }
        let mut data = Vec::new();
        // A run is one page: `more` says the suffix continues, and the caller pages for the rest with
        // `REPLAY` from the end this run reaches. The end is read off `REPLAY_END` rather than the
        // ack, whose `sequenceOffset` is the session head a paged run has not delivered yet.
        let delivered = loop {
            let frame = self.receive(&request_id)?;
            match frame.kind {
                KeeperKind::ReplayChunk => {
                    data.extend_from_slice(&decode_base64_bytes(&str_of(&frame.payload, "data")));
                }
                KeeperKind::ReplayEnd => break u64_of(&frame.payload, "sequenceOffset"),
                other => {
                    return Err(protocol_error(format!(
                        "expected a replay frame in the attach run, keeper answered {other:?}"
                    )))
                }
            }
        };
        Ok((reply, data, delivered))
    }

    /// Reads the retained log from `from_sequence`: a run of `REPLAY_CHUNK` frames ending in
    /// `REPLAY_END`, or a single gap event when the log no longer reaches that offset. A shortened
    /// answer is never passed off as complete history.
    pub fn replay(&mut self, session_id: &str, from_sequence: u64)
        -> Result<ReplayReply, KeeperClientError> {
        self.refuse_unfenced("REPLAY")?;
        self.replay_with(replay_body(session_id, None, from_sequence), from_sequence)
    }

    pub fn replay_fenced(&mut self, session_id: &str, epoch: u64, from_sequence: u64)
        -> Result<ReplayReply, KeeperClientError> {
        self.replay_with(replay_body(session_id, Some(epoch), from_sequence), from_sequence)
    }

    fn replay_with(&mut self, payload: Value, from_sequence: u64)
        -> Result<ReplayReply, KeeperClientError> {
        let request_id = self.send(KeeperKind::Replay, payload)?;
        let mut chunks = Vec::new();
        loop {
            let reply = self.receive(&request_id)?;
            match reply.kind {
                KeeperKind::ReplayChunk => chunks.push((
                    u64_of(&reply.payload, "sequenceOffset"),
                    str_of(&reply.payload, "data"),
                )),
                KeeperKind::ReplayEnd => {
                    let end_sequence = u64_of(&reply.payload, "sequenceOffset");
                    let more = opt_bool_of(&reply.payload, "more").unwrap_or(false);
                    return Ok(ReplayReply { chunks, end_sequence, more, gap: None });
                }
                KeeperKind::ReplayGapEvent => {
                    let offset = u64_of(&reply.payload, "offset");
                    let detail = reply.payload.get("detail").and_then(ReplayGap::from_value);
                    let gap = detail.unwrap_or(ReplayGap {
                        requested_after_sequence: from_sequence, available_from_sequence: offset,
                    });
                    // The gap is a hole, so partial chunks are dropped rather than passed off as
                    // complete history.
                    chunks.clear();
                    return Ok(ReplayReply {
                        chunks, end_sequence: gap.available_from_sequence, more: false,
                        gap: Some(gap),
                    });
                }
                other => {
                    return Err(protocol_error(format!(
                        "expected a replay frame, keeper answered {other:?}"
                    )))
                }
            }
        }
    }

    /// The session's output head: how many bytes a reader has to catch up with.
    pub fn head(&mut self, session_id: &str) -> Result<u64, KeeperClientError> {
        self.refuse_unfenced("HEAD")?;
        self.head_with(head_body(session_id, None))
    }

    pub fn head_fenced(&mut self, session_id: &str, epoch: u64) -> Result<u64, KeeperClientError> {
        self.head_with(head_body(session_id, Some(epoch)))
    }

    fn head_with(&mut self, payload: Value) -> Result<u64, KeeperClientError> {
        let reply = Self::expect_kind(
            self.call(KeeperKind::Head, payload)?,
            KeeperKind::HeadAck,
        )?;
        Ok(u64_of(&reply.payload, "sequenceOffset"))
    }

    /// Authoritative discovery: every session the keeper holds, read from its registry rather than
    /// inferred from live connections.
    pub fn ownership(&mut self) -> Result<Vec<OwnershipEntry>, KeeperClientError> {
        let reply = Self::expect_kind(
            self.call(KeeperKind::OwnershipQuery, json!({}))?,
            KeeperKind::OwnershipList,
        )?;
        let entry = |entry: &Value| OwnershipEntry {
            session_id: str_of(entry, "id"),
            pid: u32_of(entry, "pid"),
            epoch: u64_of(entry, "epoch"),
            sequence_offset: u64_of(entry, "sequenceOffset"),
            cols: u16_of(entry, "cols"),
            rows: u16_of(entry, "rows"),
            owner: str_of(entry, "owner"),
            focused: opt_bool_of(entry, "focused").unwrap_or(false),
            generation: str_of(entry, "generation"),
        };
        let sessions = reply.payload.get("sessions").and_then(Value::as_array);
        Ok(sessions.map(|entries| entries.iter().map(entry).collect()).unwrap_or_default())
    }

    /// Installs a checkpoint. With `data` present it commits in one frame; without, it opens a
    /// chunked upload under `checkpoint_id` for [`Self::checkpoint_chunk`] to fill.
    pub fn checkpoint_install(&mut self, session_id: &str, epoch: u64, checkpoint_id: &str,
        sequence_offset: u64, total_bytes: u64, data: Option<&[u8]>)
        -> Result<CheckpointReply, KeeperClientError> {
        let mut payload = json!({ "sessionId": session_id, "epoch": epoch,
            "checkpointId": checkpoint_id, "sequenceOffset": sequence_offset,
            "totalBytes": total_bytes });
        if let Some(bytes) = data {
            payload["data"] = Value::String(base64_encode(bytes));
        }
        self.checkpoint_call(KeeperKind::CheckpointInstall, payload)
    }

    /// Sends one chunk of an open checkpoint upload, in `chunk_index` order.
    pub fn checkpoint_chunk(&mut self, session_id: &str, epoch: u64, checkpoint_id: &str,
        chunk_index: u32, data: &[u8]) -> Result<CheckpointReply, KeeperClientError> {
        let payload = json!({ "sessionId": session_id, "epoch": epoch,
            "checkpointId": checkpoint_id, "chunkIndex": chunk_index,
            "data": base64_encode(data) });
        self.checkpoint_call(KeeperKind::CheckpointChunk, payload)
    }

    /// Commits a fully-uploaded checkpoint at `sequence_offset`.
    pub fn checkpoint_commit(&mut self, session_id: &str, epoch: u64, checkpoint_id: &str,
        sequence_offset: u64)
        -> Result<CheckpointReply, KeeperClientError> {
        let payload = json!({ "sessionId": session_id, "epoch": epoch,
            "checkpointId": checkpoint_id, "sequenceOffset": sequence_offset });
        self.checkpoint_call(KeeperKind::CheckpointCommit, payload)
    }

    /// What checkpoint the session holds, if any.
    pub fn checkpoint_query(&mut self, session_id: &str) -> Result<CheckpointReply, KeeperClientError> {
        self.checkpoint_call(KeeperKind::CheckpointQuery, json!({ "sessionId": session_id }))
    }

    /// Reads a committed checkpoint's bytes back in `max_bytes` pages, following `nextByte` until
    /// the keeper reports the read complete.
    ///
    /// `epoch` is the same destructive-verb fence the upload takes, and `sequence_offset` must name
    /// the checkpoint the caller means: two checkpoints of one session differ only by that offset,
    /// so a stale one returns different bytes with no other signal.
    pub fn checkpoint_fetch(&mut self, session_id: &str, epoch: u64, checkpoint_id: &str,
        sequence_offset: u64, max_bytes: u64) -> Result<Vec<u8>, KeeperClientError> {
        let mut bytes = Vec::new();
        let mut total_bytes = None;
        loop {
            let from_byte = bytes.len() as u64;
            let request_id = self.send(
                KeeperKind::CheckpointFetch,
                json!({ "sessionId": session_id, "epoch": epoch,
                    "checkpointId": checkpoint_id, "sequenceOffset": sequence_offset,
                    "fromByte": from_byte, "maxBytes": max_bytes }),
            )?;
            let (next_byte, complete) = loop {
                let frame = self.receive(&request_id)?;
                match frame.kind {
                    KeeperKind::CheckpointFetchChunk => {
                        check_fetched_identity(
                            &frame.payload, checkpoint_id, sequence_offset, &mut total_bytes,
                        )?;
                        bytes.extend_from_slice(
                            &decode_base64_bytes(&str_of(&frame.payload, "data")),
                        );
                    }
                    KeeperKind::CheckpointFetchEnd => {
                        check_fetched_identity(
                            &frame.payload, checkpoint_id, sequence_offset, &mut total_bytes,
                        )?;
                        break (
                            u64_of(&frame.payload, "nextByte"),
                            opt_bool_of(&frame.payload, "complete").unwrap_or(false),
                        );
                    }
                    other => {
                        return Err(protocol_error(format!(
                            "expected a checkpoint fetch frame, keeper answered {other:?}"
                        )))
                    }
                }
            };
            if let Some(declared) = total_bytes {
                if bytes.len() as u64 > declared {
                    return Err(protocol_error(format!(
                        "checkpoint {checkpoint_id} of {session_id} declares {declared} bytes but \
                         the fetch has already read {}",
                        bytes.len()
                    )));
                }
            }
            if complete {
                return Ok(bytes);
            }
            if next_byte <= from_byte {
                return Err(protocol_error(format!(
                    "checkpoint {checkpoint_id} of {session_id} made no progress: the keeper read \
                     from byte {from_byte} and reported {next_byte} next"
                )));
            }
        }
    }

    /// Any checkpoint verb: one request, one `CHECKPOINT_ACK`.
    fn checkpoint_call(&mut self, kind: KeeperKind, payload: Value)
        -> Result<CheckpointReply, KeeperClientError> {
        let reply = self.call(kind, payload)?;
        Ok(checkpoint_reply(&Self::expect_kind(reply, KeeperKind::CheckpointAck)?.payload))
    }

    /// Releases the attachment but keeps the session and its PTY alive.
    pub fn detach(&mut self, session_id: &str, epoch: u64) -> Result<(), KeeperClientError> {
        self.echo(KeeperKind::Detach, json!({ "sessionId": session_id, "epoch": epoch }))
    }
}

/// The daemon-wide ceiling on simultaneous keeper connections.
///
/// The keeper serves at most `protocol::MAX_CONNECTIONS` peers at a time and refuses the rest with
/// `TOO_MANY_CONNECTIONS`; that cap is a deliberate, tested contract and not a limit to raise. Every
/// keeper verb this daemon issues therefore comes out of one small shared pool, so the daemon's
/// keeper footprint is this constant however many sessions it serves. The defect this replaces was
/// one permanent connection per keeper-owned session, which made the ninth spawn fail.
///
/// Four leaves half of the keeper's eight slots free for everything else that may legitimately
/// connect to it: the supervisor's presence probe, the startup occupancy query, and an external
/// harness. Pumps are the traffic this pool exists for, and they poll, so the capacity also sets
/// each poll's latency budget: with `C` permits, `N` sessions and a 100 ms idle interval, a poll
/// waits at most `C * 100 ms / N` behind its peers. Four keeps a hundred idle sessions' empty polls
/// well inside that budget.
pub const KEEPER_POOL_CONNECTIONS: usize = 4;

/// How long a verb waits for a pooled connection before the wait is reported as a transient
/// failure. A verb that cannot get a slot in this long is not stuck behind ordinary traffic.
pub const KEEPER_POOL_WAIT: Duration = Duration::from_secs(30);

/// A bounded pool of keeper connections shared by every keeper verb the daemon issues.
///
/// Two properties make it more than a cache:
///
/// * the number of **live** connections is capped, not just the number kept idle: a caller waits for
///   a permit before it connects, so a burst of sessions cannot open a burst of connections and be
///   refused by the keeper's connection cap;
/// * a client whose verb failed is dropped rather than returned, because the failure may have left
///   the stream mid-run (a paged reply that never finished writing, an `ATTACH` whose framed suffix
///   was cut short). A fresh connection is cheap; reading a desynchronized one is not.
///
/// A closure passed to [`Self::with_client`] must not call [`Self::with_client`] again: with every
/// permit taken, the nested call would wait for a permit its own caller is holding.
pub struct KeeperConnectionPool {
    runtime_dir: PathBuf,
    capacity: usize,
    /// How long a caller waits for a permit before the wait is reported as a transient failure.
    wait: Duration,
    state: Mutex<PoolState>,
    released: Condvar,
}

struct PoolState {
    /// Connections that finished a verb cleanly, newest last, ready to be reused.
    idle: Vec<KeeperClient>,
    /// Permits not currently held by a caller. Live connections are exactly `capacity - permits`.
    permits: usize,
}

impl KeeperConnectionPool {
    pub fn new(runtime_dir: impl Into<PathBuf>) -> Self {
        Self::with_capacity(runtime_dir, KEEPER_POOL_CONNECTIONS)
    }

    pub fn with_capacity(runtime_dir: impl Into<PathBuf>, capacity: usize) -> Self {
        Self::with_capacity_and_wait(runtime_dir, capacity, KEEPER_POOL_WAIT)
    }

    /// The same pool with an explicit admission wait, so a test can assert the refusal without
    /// spending the production wait on it.
    pub fn with_capacity_and_wait(
        runtime_dir: impl Into<PathBuf>,
        capacity: usize,
        wait: Duration,
    ) -> Self {
        let capacity = capacity.max(1);
        Self {
            runtime_dir: runtime_dir.into(),
            capacity,
            wait,
            state: Mutex::new(PoolState { idle: Vec::new(), permits: capacity }),
            released: Condvar::new(),
        }
    }

    /// How many connections this pool may hold open at once.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Runs one verb on a pooled connection.
    ///
    /// The permit is taken before connecting, so this never exceeds the pool's capacity even when
    /// every session in the daemon calls at once.
    pub fn with_client<T>(
        &self,
        verb: impl FnOnce(&mut KeeperClient) -> Result<T, KeeperClientError>,
    ) -> Result<T, KeeperClientError> {
        let held = self.acquire()?;
        let idle = {
            let mut state = self.state.lock();
            state.idle.pop()
        };
        let mut client = match idle {
            Some(client) => client,
            None => match KeeperClient::connect(&self.runtime_dir) {
                Ok(client) => client,
                Err(error) => {
                    held.release(None);
                    return Err(error);
                }
            },
        };
        let outcome = verb(&mut client);
        // A client whose verb failed may be mid-frame, so it is dropped rather than reused; a
        // clean one goes back idle. Either way the permit is returned here.
        held.release(match &outcome {
            Ok(_) => Some(client),
            Err(_) => None,
        });
        outcome
    }

    /// Runs `body` while holding one permit, without connecting.
    ///
    /// This is the admission half of [`Self::with_client`] on its own: the bound the keeper's
    /// connection cap turns into a refusal is a property of admission, and it can be asserted
    /// without a keeper process.
    pub fn with_permit<T>(&self, body: impl FnOnce() -> T) -> Result<T, KeeperClientError> {
        let held = self.acquire()?;
        let value = body();
        held.release(None);
        Ok(value)
    }

    /// Takes one permit, waiting for a release up to the pool's wait.
    fn acquire(&self) -> Result<PoolPermit<'_>, KeeperClientError> {
        let mut state = self.state.lock();
        while state.permits == 0 {
            if self.released.wait_for(&mut state, self.wait).timed_out()
                && state.permits == 0
            {
                return Err(KeeperClientError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "no keeper connection was free within {:?} ({} in use)",
                        self.wait, self.capacity
                    ),
                )));
            }
        }
        state.permits -= 1;
        Ok(PoolPermit { pool: self })
    }

    /// Returns one permit, keeping `client` for reuse when the verb ended cleanly.
    fn release(&self, client: Option<KeeperClient>) {
        let mut state = self.state.lock();
        state.permits += 1;
        if let Some(client) = client {
            if state.idle.len() < self.capacity {
                state.idle.push(client);
            }
        }
        drop(state);
        self.released.notify_one();
    }
}

/// One admitted slot, returned to its pool when dropped or released explicitly.
///
/// Holding the permit is what bounds the footprint: every connection this pool opens is opened while
/// a permit is held, so the number of live connections can never exceed the capacity.
struct PoolPermit<'a> {
    pool: &'a KeeperConnectionPool,
}

impl PoolPermit<'_> {
    /// Returns the permit, keeping `client` idle for reuse when the verb ended cleanly.
    fn release(self, client: Option<KeeperClient>) {
        let pool = self.pool;
        // Forgetting `self` is what stops `Drop` from returning the same permit a second time.
        std::mem::forget(self);
        pool.release(client);
    }
}

impl Drop for PoolPermit<'_> {
    /// A permit dropped by a panicking verb still goes back, or the pool would lose a slot for the
    /// life of the process.
    fn drop(&mut self) {
        self.pool.release(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST_ID: &str = "00000000-0000-4000-8000-000000000000";

    fn frame(kind: KeeperKind, payload: Value) -> KeeperFrame {
        KeeperFrame::new(kind, REQUEST_ID, 9, payload)
    }

    /// The bytes this client writes are the keeper's pinned header, field for field, and they read
    /// back as the frame that wrote them.
    #[test]
    fn header_matches_the_keepers_layout_and_round_trips() {
        let payload = json!({"sessionId": "s-1", "epoch": 2, "sequenceOffset": 128});
        let original = frame(KeeperKind::AttachAck, payload.clone());
        let bytes = original.encode();
        let text = payload.to_string();
        assert_eq!(&bytes[0..4], b"FXKP");
        assert_eq!(&bytes[4..6], &KEEPER_FRAMING_VERSION.to_be_bytes());
        assert_eq!(&bytes[6..8], &0x0051u16.to_be_bytes());
        assert_eq!(&bytes[8..12], &(text.len() as u32).to_be_bytes());
        assert_eq!(&bytes[12..48], REQUEST_ID.as_bytes());
        assert_eq!(&bytes[56..60], &[0u8; 4]);
        assert_eq!(KeeperFrame::decode(&bytes).expect("decode"), original);
    }

    #[test]
    fn unknown_kind_is_a_protocol_error() {
        let mut bytes = frame(KeeperKind::Hello, json!({})).encode();
        bytes[6..8].copy_from_slice(&0x7fffu16.to_be_bytes());
        let message = match KeeperFrame::decode(&bytes).expect_err("unknown kind must be refused") {
            KeeperClientError::Protocol(message) => message,
            other => panic!("expected a protocol error, got {other}"),
        };
        assert!(message.contains("0x7fff"), "{message}");
    }

    #[test]
    fn error_frame_decodes_into_a_refusal_that_keeps_the_wire_code() {
        let payload = json!({"code": "EPOCH_MISMATCH", "message": "epoch 2 is older than 5"});
        let decoded = KeeperFrame::decode(&frame(KeeperKind::Error, payload.clone()).encode());
        assert_eq!(decoded.expect("decode").kind, KeeperKind::Error);
        match refusal(&payload) {
            KeeperClientError::Refused { code, message } => {
                assert_eq!(code, "EPOCH_MISMATCH");
                assert_eq!(message, "epoch 2 is older than 5");
            }
            other => panic!("expected a refusal, got {other}"),
        }
    }

    #[test]
    fn gap_payload_yields_both_offsets() {
        let payload = json!({
            "requestedAfterSequence": 4,
            "availableFromSequence": 40,
            "retainedFromSequence": 40,
        });
        let gap = ReplayGap::from_value(&payload).expect("gap");
        assert_eq!(gap.requested_after_sequence, 4);
        assert_eq!(gap.available_from_sequence, 40);
        assert!(ReplayGap::from_value(&json!({"sessionId": "s-1"})).is_none());
    }

    #[test]
    fn every_kind_maps_to_the_keepers_pinned_discriminant() {
        let table: &[(u16, KeeperKind)] = &[
            (0x0001, KeeperKind::Hello),
            (0x0002, KeeperKind::HelloAck),
            (0x0010, KeeperKind::Spawn),
            (0x0011, KeeperKind::SpawnAck),
            (0x0020, KeeperKind::Write),
            (0x0030, KeeperKind::Resize),
            (0x0031, KeeperKind::Signal),
            (0x0040, KeeperKind::Kill),
            (0x0050, KeeperKind::Attach),
            (0x0051, KeeperKind::AttachAck),
            (0x0052, KeeperKind::Output),
            (0x0053, KeeperKind::Head),
            (0x0054, KeeperKind::HeadAck),
            (0x0060, KeeperKind::Replay),
            (0x0061, KeeperKind::ReplayChunk),
            (0x0062, KeeperKind::ReplayEnd),
            (0x0063, KeeperKind::ReplayGapEvent),
            (0x0070, KeeperKind::OwnershipQuery),
            (0x0071, KeeperKind::OwnershipList),
            (0x0080, KeeperKind::CheckpointInstall),
            (0x0081, KeeperKind::CheckpointChunk),
            (0x0082, KeeperKind::CheckpointCommit),
            (0x0083, KeeperKind::CheckpointAck),
            (0x0084, KeeperKind::CheckpointQuery),
            (0x0085, KeeperKind::CheckpointFetch),
            (0x0086, KeeperKind::CheckpointFetchChunk),
            (0x0087, KeeperKind::CheckpointFetchEnd),
            (0x0090, KeeperKind::Detach),
            (0x00f0, KeeperKind::Error),
            (0x00f1, KeeperKind::Shutdown),
        ];
        for &(raw, kind) in table {
            assert_eq!(kind.as_u16(), raw, "{kind:?}");
            assert_eq!(KeeperKind::from_u16(raw), Some(kind), "{raw:#06x}");
            let decoded = KeeperFrame::decode(&frame(kind, json!({})).encode()).expect("decode");
            assert_eq!(decoded.kind, kind);
        }
        assert_eq!(KeeperKind::from_u16(0x0088), None);
    }

    #[test]
    fn checkpoint_fetch_frames_decode_into_bytes_and_progress() {
        let chunk = frame(
            KeeperKind::CheckpointFetchChunk,
            json!({
                "sessionId": "s-1", "checkpointId": "cp-1", "sequenceOffset": 4096,
                "totalBytes": 8, "fromByte": 0, "chunkIndex": 0,
                "data": base64_encode(&[0x00, 0xff, 0x10, 0x20]),
            }),
        );
        let chunk = KeeperFrame::decode(&chunk.encode()).expect("chunk decodes");
        assert_eq!(chunk.kind, KeeperKind::CheckpointFetchChunk);
        assert_eq!(chunk.request_id, REQUEST_ID);
        assert_eq!(
            decode_base64_bytes(&str_of(&chunk.payload, "data")),
            vec![0x00, 0xff, 0x10, 0x20]
        );
        assert_eq!(u64_of(&chunk.payload, "fromByte"), 0);
        assert_eq!(u32_of(&chunk.payload, "chunkIndex"), 0);
        assert_eq!(u64_of(&chunk.payload, "sequenceOffset"), 4096);

        let end = frame(
            KeeperKind::CheckpointFetchEnd,
            json!({
                "sessionId": "s-1", "checkpointId": "cp-1", "sequenceOffset": 4096,
                "totalBytes": 8, "fromByte": 0, "nextByte": 8, "complete": true,
            }),
        );
        let end = KeeperFrame::decode(&end.encode()).expect("end decodes");
        assert_eq!(end.kind, KeeperKind::CheckpointFetchEnd);
        assert_eq!(u64_of(&end.payload, "nextByte"), 8);
        assert_eq!(opt_bool_of(&end.payload, "complete"), Some(true));

        let short = json!({ "nextByte": 4, "complete": false });
        assert_eq!(opt_bool_of(&short, "complete"), Some(false));
        assert!(u64_of(&short, "nextByte") < u64_of(&end.payload, "totalBytes"));
    }

    #[test]
    fn capabilities_come_from_the_hello_ack_and_absence_negotiates_legacy() {
        let advertised = json!({
            "protocolMajor": KEEPER_PROTOCOL_MAJOR,
            "protocolMinor": 0,
            "capabilities": [KEEPER_CAPABILITY_EPOCH_FENCED_CHECKPOINT, "unknown-additive-v9"],
        });
        assert!(capabilities_of(&advertised)
            .iter()
            .any(|entry| entry.as_str() == KEEPER_CAPABILITY_EPOCH_FENCED_CHECKPOINT));
        let legacy = json!({"protocolMajor": KEEPER_PROTOCOL_MAJOR, "protocolMinor": 0});
        assert!(capabilities_of(&legacy).is_empty());
        assert_eq!(
            capabilities_of(&json!({"capabilities": ["a", 7, null]})),
            vec!["a".to_string()],
        );
    }

    #[test]
    fn fenced_head_and_replay_carry_the_epoch_and_unfenced_ones_do_not() {
        let replay = replay_body("s-1", Some(7), 128);
        assert_eq!(replay["sessionId"], "s-1");
        assert_eq!(replay["epoch"], json!(7));
        assert_eq!(replay["fromSequence"], json!(128));
        assert!(replay_body("s-1", None, 128).get("epoch").is_none());

        let head = head_body("s-1", Some(7));
        assert_eq!(head["sessionId"], "s-1");
        assert_eq!(head["epoch"], json!(7));
        assert!(head_body("s-1", None).get("epoch").is_none());

        let encoded = KeeperFrame::new(KeeperKind::Head, REQUEST_ID, 9, head).encode();
        let decoded = KeeperFrame::decode(&encoded).expect("decode");
        assert_eq!(decoded.kind, KeeperKind::Head);
        assert_eq!(u64_of(&decoded.payload, "epoch"), 7);
    }

    #[test]
    fn a_fetch_page_bound_to_another_checkpoint_is_refused() {
        let mut total = None;
        let good = json!({"checkpointId": "cp-1", "sequenceOffset": 4096, "totalBytes": 8});
        assert!(check_fetched_identity(&good, "cp-1", 4096, &mut total).is_ok());
        assert_eq!(total, Some(8));

        let other_id = json!({"checkpointId": "cp-2", "sequenceOffset": 4096, "totalBytes": 8});
        assert!(check_fetched_identity(&other_id, "cp-1", 4096, &mut total).is_err());

        let other_offset = json!({"checkpointId": "cp-1", "sequenceOffset": 8192, "totalBytes": 8});
        assert!(check_fetched_identity(&other_offset, "cp-1", 4096, &mut total).is_err());

        let replaced = json!({"checkpointId": "cp-1", "sequenceOffset": 4096, "totalBytes": 9});
        assert!(check_fetched_identity(&replaced, "cp-1", 4096, &mut total).is_err());
        assert_eq!(total, Some(8));
    }

    /// The daemon's keeper footprint is bounded by the pool, not by its session count: a pool of
    /// `capacity` admits exactly that many concurrent permits however many sessions ask at once, and
    /// an admission that cannot be granted is a transient failure instead of a connection the keeper
    /// would refuse with `TOO_MANY_CONNECTIONS`.
    ///
    /// Driven on admission alone, with no socket: every connection this pool opens is opened while a
    /// permit is held, so admission is the whole bound and needs no keeper process to observe.
    #[test]
    fn the_pool_admits_only_its_capacity_and_refuses_the_rest_as_transient() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let dir = tempfile::tempdir().expect("temp dir");
        // One permit, and a short admission wait: the refusal is the assertion, not the wait for it.
        let pool = Arc::new(KeeperConnectionPool::with_capacity_and_wait(
            dir.path(),
            1,
            Duration::from_millis(50),
        ));
        assert_eq!(pool.capacity(), 1);
        let (entered_tx, entered_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let live = Arc::new(AtomicUsize::new(0));

        // A holder that keeps its permit until released, as a slow verb does.
        let holder = {
            let pool = Arc::clone(&pool);
            let live = Arc::clone(&live);
            std::thread::spawn(move || {
                pool.with_permit(|| {
                    live.fetch_add(1, Ordering::SeqCst);
                    entered_tx.send(()).expect("report entry");
                    release_rx.recv().expect("wait for release");
                    live.fetch_sub(1, Ordering::SeqCst);
                })
                .expect("the holder's permit");
                // Only after `with_permit` returned is the permit actually back, so this signal is
                // what the reuse assertion below waits on instead of a duration.
                done_tx.send(()).expect("report release");
            })
        };
        entered_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the permit is taken");
        assert_eq!(live.load(Ordering::SeqCst), 1);

        // The pool is at capacity, so the next caller is not admitted: it fails as transient rather
        // than growing the footprint past the capacity, which is the keeper's `TOO_MANY_CONNECTIONS`
        // refusal answered where it can still be answered.
        let refused = pool
            .with_permit(|| ())
            .expect_err("a pool at capacity must not admit another caller");
        assert!(
            matches!(refused, KeeperClientError::Io(ref error) if error.kind() == std::io::ErrorKind::TimedOut),
            "expected a timed-out admission, got {refused:?}"
        );
        assert_eq!(live.load(Ordering::SeqCst), 1, "no second caller was ever admitted");

        // A returned permit is reusable with no further waiting.
        release_tx.send(()).expect("release the holder");
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the holder released its permit");
        pool.with_permit(|| ()).expect("a freed permit is reusable");
        holder.join().expect("holder thread");
        assert_eq!(live.load(Ordering::SeqCst), 0);
    }

    /// One connection carries one verb and goes back to the pool: the attach this daemon issues asks
    /// for **no** live output.
    ///
    /// That flag is the whole difference between a bounded footprint and a socket per session. The
    /// keeper arms a connection as a session's output tap only when `liveOutput` is true, and a tap
    /// holds its connection for the session's life -- so a daemon serving sixty sessions would hold
    /// sixty sockets against a keeper that admits eight. Output is read with `REPLAY` polls instead.
    #[cfg(unix)]
    #[test]
    fn the_daemon_attach_asks_for_no_live_tap() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        let listener = std::os::unix::net::UnixListener::bind(&socket).expect("bind keeper socket");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the client connects");
            let mut seen = Vec::new();
            for _ in 0..2 {
                let mut header = [0u8; KEEPER_HEADER_BYTES];
                stream.read_exact(&mut header).expect("frame header");
                let payload_len =
                    u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
                let mut bytes = vec![0u8; KEEPER_HEADER_BYTES + payload_len];
                bytes[..KEEPER_HEADER_BYTES].copy_from_slice(&header);
                stream
                    .read_exact(&mut bytes[KEEPER_HEADER_BYTES..])
                    .expect("frame payload");
                let request = KeeperFrame::decode(&bytes).expect("the client sends valid frames");
                let reply = match request.kind {
                    KeeperKind::Hello => KeeperFrame::new(
                        KeeperKind::HelloAck,
                        request.request_id.clone(),
                        request.frame_sequence,
                        json!({
                            "protocolMajor": KEEPER_PROTOCOL_MAJOR,
                            "protocolMinor": 0,
                            "keeperVersion": "test",
                            "pid": std::process::id(),
                        }),
                    ),
                    _ => KeeperFrame::new(
                        KeeperKind::AttachAck,
                        request.request_id.clone(),
                        request.frame_sequence,
                        json!({
                            "sessionId": "s-1",
                            "epoch": 2,
                            // An inline ack whose page ends exactly at the head: `afterSequence`
                            // 0 plus these five bytes.
                            "sequenceOffset": 5,
                            "dataChunked": false,
                            "data": base64_encode(b"hello"),
                            "liveOutput": false,
                        }),
                    ),
                };
                stream.write_all(&reply.encode()).expect("reply written");
                seen.push(request);
            }
            seen
        });

        let mut client = KeeperClient::connect(dir.path()).expect("handshake");
        let (reply, replayed, delivered) = client
            .attach_recovering("s-1", 2, 0)
            .expect("attach is answered");
        assert_eq!(reply.session_id, "s-1");
        assert_eq!(replayed.as_slice(), b"hello");
        assert_eq!(delivered, 5, "the page ends where its own bytes end");

        let requests = server.join().expect("fake keeper");
        let attach = requests
            .iter()
            .find(|request| request.kind == KeeperKind::Attach)
            .expect("the client attached");
        assert_eq!(
            attach.payload.get("liveOutput"),
            Some(&Value::Bool(false)),
            "a tap would pin this connection to the session's life: output must be polled"
        );
        assert_eq!(attach.payload.get("chunked"), Some(&Value::Bool(true)));
    }

    /// A page that stops short of the head must report where its own bytes end, not the head.
    ///
    /// The keeper copies at most one page (512 KiB) of the retained suffix, so an `ATTACH_ACK` for a
    /// longer backlog is a prefix of it: `data.len()` is shorter than `sequenceOffset - after`. Taking
    /// the ack's `sequenceOffset` as the delivered offset then skips every byte between the page's end
    /// and the head -- and `start_pump` polls from there, so that range is never read at all. This is
    /// the no-gap case's failure: a 720 KiB history arrived as its first 512 KiB, so `READY:` near the
    /// head never reached the hub.
    #[cfg(unix)]
    #[test]
    fn an_inline_page_shorter_than_the_head_reports_its_own_end() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        let listener = std::os::unix::net::UnixListener::bind(&socket).expect("bind keeper socket");
        let page = b"first page";
        let head = 900u64;
        let server = std::thread::spawn({
            let page = page.to_vec();
            move || {
                let (mut stream, _) = listener.accept().expect("the client connects");
                for _ in 0..2 {
                    let mut header = [0u8; KEEPER_HEADER_BYTES];
                    stream.read_exact(&mut header).expect("frame header");
                    let payload_len =
                        u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
                    let mut bytes = vec![0u8; KEEPER_HEADER_BYTES + payload_len];
                    bytes[..KEEPER_HEADER_BYTES].copy_from_slice(&header);
                    stream
                        .read_exact(&mut bytes[KEEPER_HEADER_BYTES..])
                        .expect("frame payload");
                    let request = KeeperFrame::decode(&bytes).expect("valid frames");
                    let reply = match request.kind {
                        KeeperKind::Hello => KeeperFrame::new(
                            KeeperKind::HelloAck,
                            request.request_id.clone(),
                            request.frame_sequence,
                            json!({
                                "protocolMajor": KEEPER_PROTOCOL_MAJOR,
                                "protocolMinor": 0,
                                "keeperVersion": "test",
                                "pid": std::process::id(),
                            }),
                        ),
                        _ => KeeperFrame::new(
                            KeeperKind::AttachAck,
                            request.request_id.clone(),
                            request.frame_sequence,
                            json!({
                                "sessionId": "s-1",
                                "epoch": 2,
                                // The head is far past this page: the suffix overflowed the page cap.
                                "sequenceOffset": head,
                                "dataChunked": false,
                                "data": base64_encode(&page),
                                "liveOutput": false,
                            }),
                        ),
                    };
                    stream.write_all(&reply.encode()).expect("reply written");
                }
            }
        });

        let mut client = KeeperClient::connect(dir.path()).expect("handshake");
        let (_reply, replayed, delivered) = client
            .attach_recovering("s-1", 2, 100)
            .expect("attach is answered");
        assert_eq!(replayed.as_slice(), &page[..], "the page's bytes are delivered as they arrived");
        assert_eq!(
            delivered,
            100 + page.len() as u64,
            "the cursor resumes at the page's end, so the range up to the head is still polled"
        );
        assert!(
            delivered < head,
            "resuming at the head would skip {head} - {delivered} undelivered bytes"
        );
        server.join().expect("fake keeper");
    }

    #[test]
    fn a_gap_resets_the_resume_cursor_and_a_complete_run_uses_its_head() {
        let complete = ReplayReply {
            chunks: vec![(0, base64_encode(b"abc"))],
            end_sequence: 3,
            more: false,
            gap: None,
        };
        assert_eq!(complete.resume_offset(), 3);

        let gap = ReplayReply {
            chunks: Vec::new(),
            end_sequence: 40,
            more: false,
            gap: Some(ReplayGap {
                requested_after_sequence: 4,
                available_from_sequence: 40,
            }),
        };
        assert_eq!(gap.resume_offset(), 40);
    }

    #[test]
    fn refusals_are_classified_by_the_wire_code() {
        let epoch = KeeperClientError::Refused {
            code: "EPOCH_MISMATCH".to_string(),
            message: "epoch 2 is older than 5".to_string(),
        };
        assert!(epoch.is_epoch_mismatch());
        assert!(!epoch.is_stale_checkpoint());
        assert_eq!(epoch.wire_code(), Some("EPOCH_MISMATCH"));

        let stale = KeeperClientError::Refused {
            code: "STALE_CHECKPOINT".to_string(),
            message: "cp-1 at 4096 is not the committed one".to_string(),
        };
        assert!(stale.is_stale_checkpoint());
        assert!(!stale.is_epoch_mismatch());

        assert_eq!(protocol_error("bad frame").wire_code(), None);
    }

    fn client_with(capabilities: &[&str]) -> KeeperClient {
        let (stream, _peer) = UnixStream::pair().expect("socket pair");
        KeeperClient {
            stream,
            next_sequence: 0,
            capabilities: capabilities.iter().map(|entry| (*entry).to_string()).collect(),
        }
    }

    #[test]
    fn an_epoch_fencing_keeper_refuses_the_epoch_less_verbs_locally() {
        let fenced = client_with(&[KEEPER_CAPABILITY_EPOCH_FENCED_CHECKPOINT]);
        assert!(fenced.supports(KEEPER_CAPABILITY_EPOCH_FENCED_CHECKPOINT));
        for error in [
            fenced.refuse_unfenced("HEAD").expect_err("fenced keeper refuses HEAD"),
            fenced.refuse_unfenced("REPLAY").expect_err("fenced keeper refuses REPLAY"),
        ] {
            assert!(!error.is_epoch_mismatch());
            assert_eq!(error.wire_code(), None);
            match error {
                KeeperClientError::Protocol(detail) => assert!(
                    detail.contains(KEEPER_CAPABILITY_EPOCH_FENCED_CHECKPOINT),
                    "{detail}",
                ),
                other => panic!("expected a protocol error, got {other}"),
            }
        }

        let legacy = client_with(&[]);
        assert!(!legacy.supports(KEEPER_CAPABILITY_EPOCH_FENCED_CHECKPOINT));
        assert!(legacy.refuse_unfenced("HEAD").is_ok());
        assert!(legacy.refuse_unfenced("REPLAY").is_ok());
    }
}
