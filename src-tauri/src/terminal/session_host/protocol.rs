//! Session-host wire protocol v1 (design rev3 section 2).
//!
//! Platform-neutral: the Windows named-pipe transport feeds raw bytes through [FrameDecoder] and
//! writes [encode_frame] output. Frame layout is u32 LE payload length, one kind byte, then the
//! payload; the length counts payload bytes only (not the kind byte).

use std::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::terminal::output_hub::{
    OutputHubSnapshotError, ReplayGap, ResizePoint, SessionHubSnapshot,
};

/// Host wire protocol version; session_host/mod.rs re-exports it.
pub const HOST_PROTOCOL_VERSION: u32 = 1;
pub const FRAME_HEADER_LEN: usize = 5;
/// Payload cap for every kind except Snapshot.
pub const MAX_FRAME_PAYLOAD: usize = 1 << 20;
/// Payload cap for kind 0x04 (Snapshot).
pub const MAX_SNAPSHOT_PAYLOAD: usize = 64 << 20;
/// Largest input accepted by one [FrameDecoder::push]; transports read at most this much per
/// call so decoder memory stays bounded (see [FrameDecoder]).
pub const MAX_DECODER_PUSH: usize = 64 * 1024;
const SEQUENCE_PREFIX_LEN: usize = 8;

/// Controller epoch. Orders controllers; never grants authority on its own.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Epoch(pub u64);

impl Epoch {
    pub const ZERO: Epoch = Epoch(0);

    pub fn checked_next(self) -> Option<Epoch> {
        self.0.checked_add(1).map(Epoch)
    }

    pub fn saturating_next(self) -> Epoch {
        Epoch(self.0.saturating_add(1))
    }
}

impl fmt::Display for Epoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// 32-byte secret (host token or transfer grant nonce), 64 hex chars on the wire.
/// Equality is constant time and Debug never prints the value.
#[derive(Clone, Copy)]
pub struct Secret32([u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SecretParseError {
    #[error("secret must be 64 hex characters, got {0}")]
    WrongLength(usize),
    #[error("secret has a non-hex character at index {0}")]
    NotHex(usize),
}

impl Secret32 {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Fresh secret from the OS CSPRNG.
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub fn parse_hex(text: &str) -> Result<Self, SecretParseError> {
        let raw = text.as_bytes();
        if raw.len() != 64 {
            return Err(SecretParseError::WrongLength(raw.len()));
        }
        let mut bytes = [0u8; 32];
        for (index, (slot, pair)) in bytes.iter_mut().zip(raw.chunks_exact(2)).enumerate() {
            let [hi, lo] = pair else {
                return Err(SecretParseError::WrongLength(raw.len()));
            };
            let hi = hex_value(*hi).ok_or(SecretParseError::NotHex(index * 2))?;
            let lo = hex_value(*lo).ok_or(SecretParseError::NotHex(index * 2 + 1))?;
            *slot = (hi << 4) | lo;
        }
        Ok(Self(bytes))
    }

    pub fn to_hex(&self) -> String {
        encode_hex(&self.0)
    }

    pub fn sha256(&self) -> [u8; 32] {
        Sha256::digest(self.0).into()
    }

    pub fn ct_eq(&self, other: &Self) -> bool {
        ct_eq_32(&self.0, &other.0)
    }
}

impl PartialEq for Secret32 {
    fn eq(&self, other: &Self) -> bool {
        self.ct_eq(other)
    }
}

impl Eq for Secret32 {}

impl fmt::Debug for Secret32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret32(<redacted>)")
    }
}

impl Serialize for Secret32 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Secret32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Secret32::parse_hex(&text).map_err(serde::de::Error::custom)
    }
}

/// Equality for 32-byte values (secrets and their hashes) through `subtle::ConstantTimeEq`,
/// reached via digest's `CtOutput` (pbkdf2 -> hmac -> digest re-exports, already in the build).
/// This is the best-effort constant time `subtle` provides, not a compiler-level guarantee.
pub fn ct_eq_32(a: &[u8; 32], b: &[u8; 32]) -> bool {
    use pbkdf2::hmac::digest::CtOutput;
    CtOutput::<Sha256>::new((*a).into()) == CtOutput::<Sha256>::new((*b).into())
}

pub(crate) fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Claim,
    Standby,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RejectCode {
    StaleEpoch,
    BadToken,
    ProtocolUnsupported,
    Busy,
    NotStandby,
    ActivePresent,
    NoGrant,
    GrantConsumed,
    GrantExpired,
    EpochBurned,
    TransferInProgress,
    Closing,
    NotActive,
    SuccessorMismatch,
    PredecessorAlive,
    TransferUnverifiable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloFrame {
    pub token: Secret32,
    pub host_protocol: u32,
    pub controller_epoch: Epoch,
    pub controller_pid: u32,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_nonce: Option<Secret32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizeTransferFrame {
    pub controller_epoch: Epoch,
    pub to_epoch: Epoch,
    pub grant_nonce: Secret32,
    pub successor_pid: u32,
    pub successor_creation_time: u64,
}

/// Controller to host control messages (frame kind 0x01).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ControlToHost {
    Hello(HelloFrame),
    AuthorizeTransfer(AuthorizeTransferFrame),
    RevokeTransfer {
        controller_epoch: Epoch,
        to_epoch: Epoch,
    },
    Activate {
        controller_epoch: Epoch,
        grant_nonce: Secret32,
    },
    Resize {
        controller_epoch: Epoch,
        cols: u16,
        rows: u16,
        generation: u64,
    },
    Close {
        controller_epoch: Epoch,
        grace_ms: u32,
    },
    Release {
        controller_epoch: Epoch,
    },
    SnapshotRequest {
        controller_epoch: Epoch,
    },
    Ping {
        nonce: u64,
    },
}

/// Host to controller control messages (frame kind 0x01).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HostToControl {
    /// Always followed by one Snapshot frame.
    Welcome {
        host_protocol: u32,
        session_id: String,
        shell_pid: u32,
        host_pid: u32,
        host_creation_time: u64,
        cols: u16,
        rows: u16,
        current_epoch: Epoch,
        role: Role,
        exited: Option<i32>,
    },
    TransferAuthorized {
        to_epoch: Epoch,
        expires_in_ms: u32,
    },
    TransferRevoked {
        to_epoch: Epoch,
    },
    GapChunk {
        sequence: u64,
        gap: ReplayGap,
    },
    ResizeOk {
        generation: u64,
        point: ResizePoint,
    },
    Rejected {
        code: RejectCode,
        message: String,
    },
    Fenced {
        new_epoch: Epoch,
    },
    Exited {
        code: Option<i32>,
        final_sequence: u64,
    },
    CloseOk {
        code: Option<i32>,
    },
    AgentReport {
        line: String,
    },
    Pong {
        nonce: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    Control = 0x01,
    Output = 0x02,
    Input = 0x03,
    Snapshot = 0x04,
}

impl FrameKind {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x01 => Some(Self::Control),
            0x02 => Some(Self::Output),
            0x03 => Some(Self::Input),
            0x04 => Some(Self::Snapshot),
            _ => None,
        }
    }

    pub const fn payload_cap(self) -> usize {
        match self {
            Self::Snapshot => MAX_SNAPSHOT_PAYLOAD,
            Self::Control | Self::Output | Self::Input => MAX_FRAME_PAYLOAD,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// JSON [ControlToHost] or [HostToControl].
    Control(Vec<u8>),
    /// Host to controller output; gap chunks travel as HostToControl::GapChunk instead.
    Output { sequence: u64, bytes: Vec<u8> },
    /// Controller to host input.
    Input {
        controller_epoch: Epoch,
        bytes: Vec<u8>,
    },
    /// JSON [SessionHubSnapshot], including retained_state.
    Snapshot(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FrameError {
    #[error("unknown frame kind 0x{0:02x}")]
    UnknownKind(u8),
    #[error("frame kind {kind:?} payload of {len} bytes exceeds cap {cap}")]
    PayloadTooLarge {
        kind: FrameKind,
        len: usize,
        cap: usize,
    },
    #[error("frame kind {kind:?} payload of {len} bytes is shorter than its 8-byte prefix")]
    MissingSequencePrefix { kind: FrameKind, len: usize },
    #[error("decoder push of {len} bytes exceeds per-read limit {max}")]
    PushTooLarge { len: usize, max: usize },
    #[error("decoder push while a complete frame is still buffered; drain next_frame first")]
    UndrainedFrame,
    #[error("frame json: {0}")]
    Json(String),
    #[error("invalid snapshot: {0}")]
    InvalidSnapshot(OutputHubSnapshotError),
}

impl Frame {
    pub fn control<T: Serialize>(message: &T) -> Result<Self, FrameError> {
        serde_json::to_vec(message)
            .map(Self::Control)
            .map_err(|e| FrameError::Json(e.to_string()))
    }

    pub fn snapshot(snapshot: &SessionHubSnapshot) -> Result<Self, FrameError> {
        serde_json::to_vec(snapshot)
            .map(Self::Snapshot)
            .map_err(|e| FrameError::Json(e.to_string()))
    }

    pub const fn kind(&self) -> FrameKind {
        match self {
            Self::Control(_) => FrameKind::Control,
            Self::Output { .. } => FrameKind::Output,
            Self::Input { .. } => FrameKind::Input,
            Self::Snapshot(_) => FrameKind::Snapshot,
        }
    }

    fn payload_len(&self) -> usize {
        match self {
            Self::Control(json) | Self::Snapshot(json) => json.len(),
            Self::Output { bytes, .. } | Self::Input { bytes, .. } => {
                SEQUENCE_PREFIX_LEN + bytes.len()
            }
        }
    }
}

/// Parse a Control frame payload into a typed message.
pub fn parse_control<T: DeserializeOwned>(json: &[u8]) -> Result<T, FrameError> {
    serde_json::from_slice(json).map_err(|e| FrameError::Json(e.to_string()))
}

/// Parse and validate a Snapshot frame payload.
pub fn parse_snapshot(json: &[u8]) -> Result<SessionHubSnapshot, FrameError> {
    let snapshot: SessionHubSnapshot = parse_control(json)?;
    snapshot.validate().map_err(FrameError::InvalidSnapshot)?;
    Ok(snapshot)
}

pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, FrameError> {
    let kind = frame.kind();
    let len = frame.payload_len();
    let cap = kind.payload_cap();
    let wire_len = u32::try_from(len)
        .ok()
        .filter(|_| len <= cap)
        .ok_or(FrameError::PayloadTooLarge { kind, len, cap })?;
    let mut out = Vec::with_capacity(FRAME_HEADER_LEN + len);
    out.extend_from_slice(&wire_len.to_le_bytes());
    out.push(kind as u8);
    match frame {
        Frame::Control(json) | Frame::Snapshot(json) => out.extend_from_slice(json),
        Frame::Output { sequence, bytes } => {
            out.extend_from_slice(&sequence.to_le_bytes());
            out.extend_from_slice(bytes);
        }
        Frame::Input {
            controller_epoch,
            bytes,
        } => {
            out.extend_from_slice(&controller_epoch.0.to_le_bytes());
            out.extend_from_slice(bytes);
        }
    }
    Ok(out)
}

fn parse_header(header: [u8; FRAME_HEADER_LEN]) -> Result<(FrameKind, usize), FrameError> {
    let [l0, l1, l2, l3, kind_byte] = header;
    let kind = FrameKind::from_byte(kind_byte).ok_or(FrameError::UnknownKind(kind_byte))?;
    let len = usize::try_from(u32::from_le_bytes([l0, l1, l2, l3])).unwrap_or(usize::MAX);
    let cap = kind.payload_cap();
    if len > cap {
        return Err(FrameError::PayloadTooLarge { kind, len, cap });
    }
    if matches!(kind, FrameKind::Output | FrameKind::Input) && len < SEQUENCE_PREFIX_LEN {
        return Err(FrameError::MissingSequencePrefix { kind, len });
    }
    Ok((kind, len))
}

/// Incremental decoder with enforced memory bound.
///
/// Integration contract: read at most [MAX_DECODER_PUSH] bytes per [Self::push], then call
/// [Self::next_frame] until it returns `Ok(None)`. `push` rejects larger inputs before copying
/// them and rejects a push while a complete frame is still buffered, so the buffer never holds
/// more than one partial frame (header plus its kind cap) plus one push. Every frame header in
/// the pushed bytes is validated during that push; a bad header discards the buffer. The
/// bytes of the offending push are still copied once before its headers are checked. After any
/// error the decoder stays failed and the caller must drop the connection.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
    failed: Option<FrameError>,
    /// Offset in `buf` of the next frame header not yet validated.
    checked: usize,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<(), FrameError> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        if bytes.len() > MAX_DECODER_PUSH {
            return Err(self.fail(FrameError::PushTooLarge {
                len: bytes.len(),
                max: MAX_DECODER_PUSH,
            }));
        }
        if self.holds_complete_frame() {
            return Err(self.fail(FrameError::UndrainedFrame));
        }
        self.buf.extend_from_slice(bytes);
        while let Some(header) = self
            .buf
            .get(self.checked..)
            .and_then(|rest| rest.first_chunk::<FRAME_HEADER_LEN>())
        {
            match parse_header(*header) {
                Ok((_, len)) => self.checked += FRAME_HEADER_LEN + len,
                Err(error) => return Err(self.fail(error)),
            }
        }
        Ok(())
    }

    fn holds_complete_frame(&self) -> bool {
        self.buf
            .first_chunk::<FRAME_HEADER_LEN>()
            .and_then(|header| parse_header(*header).ok())
            .is_some_and(|(_, len)| self.buf.len() >= FRAME_HEADER_LEN + len)
    }

    fn fail(&mut self, error: FrameError) -> FrameError {
        self.buf = Vec::new();
        self.checked = 0;
        self.failed = Some(error.clone());
        error
    }

    /// True when bytes of an incomplete frame are buffered (EOF here is a truncated stream).
    pub fn has_partial_frame(&self) -> bool {
        !self.buf.is_empty()
    }

    pub fn next_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        match self.try_next() {
            Ok(frame) => Ok(frame),
            Err(error) => Err(self.fail(error)),
        }
    }

    fn try_next(&mut self) -> Result<Option<Frame>, FrameError> {
        let Some(header) = self.buf.first_chunk::<FRAME_HEADER_LEN>() else {
            return Ok(None);
        };
        let (kind, len) = parse_header(*header)?;
        let end = FRAME_HEADER_LEN + len;
        if self.buf.len() < end {
            return Ok(None);
        }
        let mut payload: Vec<u8> = self.buf.drain(..end).skip(FRAME_HEADER_LEN).collect();
        self.checked = self.checked.saturating_sub(end);
        let frame = match kind {
            FrameKind::Control => Frame::Control(payload),
            FrameKind::Snapshot => Frame::Snapshot(payload),
            FrameKind::Output | FrameKind::Input => {
                let bytes = payload.split_off(SEQUENCE_PREFIX_LEN);
                let prefix: [u8; SEQUENCE_PREFIX_LEN] = payload
                    .try_into()
                    .map_err(|_| FrameError::MissingSequencePrefix { kind, len })?;
                let value = u64::from_le_bytes(prefix);
                match kind {
                    FrameKind::Output => Frame::Output {
                        sequence: value,
                        bytes,
                    },
                    _ => Frame::Input {
                        controller_epoch: Epoch(value),
                        bytes,
                    },
                }
            }
        };
        Ok(Some(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::output_hub::TerminalOutputHub;

    fn secret(byte: u8) -> Secret32 {
        Secret32::from_bytes([byte; 32])
    }

    fn header(len: u32, kind: u8) -> Vec<u8> {
        let mut out = len.to_le_bytes().to_vec();
        out.push(kind);
        out
    }

    #[test]
    fn frames_round_trip_when_fed_one_byte_at_a_time() {
        let frames = vec![
            Frame::control(&ControlToHost::Ping { nonce: 7 }).unwrap(),
            Frame::Output {
                sequence: u64::MAX - 1,
                bytes: b"out".to_vec(),
            },
            Frame::Input {
                controller_epoch: Epoch(42),
                bytes: Vec::new(),
            },
            Frame::Snapshot(b"{}".to_vec()),
        ];
        let wire: Vec<u8> = frames
            .iter()
            .flat_map(|f| encode_frame(f).unwrap())
            .collect();
        let mut decoder = FrameDecoder::new();
        let mut decoded = Vec::new();
        for byte in wire {
            decoder.push(&[byte]).unwrap();
            while let Some(frame) = decoder.next_frame().unwrap() {
                decoded.push(frame);
            }
        }
        assert_eq!(decoded, frames);
        assert!(!decoder.has_partial_frame());
    }

    #[test]
    fn oversized_control_header_is_rejected_before_payload_arrives() {
        let mut decoder = FrameDecoder::new();
        assert!(matches!(
            decoder.push(&header(u32::try_from(MAX_FRAME_PAYLOAD + 1).unwrap(), 0x01)),
            Err(FrameError::PayloadTooLarge {
                kind: FrameKind::Control,
                ..
            })
        ));
        assert!(matches!(
            decoder.next_frame(),
            Err(FrameError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn snapshot_kind_waits_for_payload_above_control_cap() {
        let mut decoder = FrameDecoder::new();
        decoder
            .push(&header(u32::try_from(MAX_FRAME_PAYLOAD + 1).unwrap(), 0x04))
            .unwrap();
        assert_eq!(decoder.next_frame(), Ok(None));
    }

    #[test]
    fn snapshot_header_above_snapshot_cap_is_rejected() {
        let mut decoder = FrameDecoder::new();
        assert!(matches!(
            decoder.push(&header(
                u32::try_from(MAX_SNAPSHOT_PAYLOAD + 1).unwrap(),
                0x04,
            )),
            Err(FrameError::PayloadTooLarge {
                kind: FrameKind::Snapshot,
                ..
            })
        ));
    }

    #[test]
    fn push_over_read_limit_is_rejected_without_buffering() {
        let mut decoder = FrameDecoder::new();
        assert_eq!(
            decoder.push(&vec![0u8; MAX_DECODER_PUSH + 1]),
            Err(FrameError::PushTooLarge {
                len: MAX_DECODER_PUSH + 1,
                max: MAX_DECODER_PUSH
            })
        );
        assert!(!decoder.has_partial_frame());
    }

    #[test]
    fn bad_header_discards_trailing_bytes_of_same_push() {
        let mut decoder = FrameDecoder::new();
        let mut input = header(u32::try_from(MAX_FRAME_PAYLOAD + 1).unwrap(), 0x01);
        input.extend_from_slice(&[0u8; 1024]);
        assert!(decoder.push(&input).is_err());
        assert!(!decoder.has_partial_frame());
    }

    #[test]
    fn every_header_in_one_push_is_validated() {
        let mut decoder = FrameDecoder::new();
        let mut input = encode_frame(&Frame::Control(b"{}".to_vec())).unwrap();
        input.extend_from_slice(&header(0, 0x09));
        assert_eq!(decoder.push(&input), Err(FrameError::UnknownKind(0x09)));
        assert!(!decoder.has_partial_frame());
    }

    #[test]
    fn push_before_draining_complete_frame_is_rejected() {
        let mut decoder = FrameDecoder::new();
        let wire = encode_frame(&Frame::Control(b"{}".to_vec())).unwrap();
        decoder.push(&wire).unwrap();
        assert_eq!(decoder.push(&wire), Err(FrameError::UndrainedFrame));
        assert_eq!(decoder.next_frame(), Err(FrameError::UndrainedFrame));
    }

    #[test]
    fn ct_eq_32_distinguishes_last_byte() {
        let a = [7u8; 32];
        let mut b = a;
        assert!(ct_eq_32(&a, &b));
        b[31] ^= 1;
        assert!(!ct_eq_32(&a, &b));
    }

    #[test]
    fn encode_rejects_payload_over_kind_cap() {
        let frame = Frame::Output {
            sequence: 1,
            bytes: vec![0; MAX_FRAME_PAYLOAD],
        };
        assert!(matches!(
            encode_frame(&frame),
            Err(FrameError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn unknown_kind_fails_decoder_permanently() {
        let mut decoder = FrameDecoder::new();
        assert_eq!(
            decoder.push(&header(0, 0x09)),
            Err(FrameError::UnknownKind(0x09))
        );
        assert_eq!(decoder.next_frame(), Err(FrameError::UnknownKind(0x09)));
        assert_eq!(
            decoder.push(&encode_frame(&Frame::Control(b"{}".to_vec())).unwrap()),
            Err(FrameError::UnknownKind(0x09))
        );
        assert_eq!(decoder.next_frame(), Err(FrameError::UnknownKind(0x09)));
    }

    #[test]
    fn output_payload_shorter_than_sequence_prefix_is_rejected() {
        let mut decoder = FrameDecoder::new();
        assert!(matches!(
            decoder.push(&header(7, 0x02)),
            Err(FrameError::MissingSequencePrefix { len: 7, .. })
        ));
    }

    #[test]
    fn hello_wire_shape_is_tagged_camel_case_with_hex_secrets() {
        let hello = ControlToHost::Hello(HelloFrame {
            token: secret(0xab),
            host_protocol: 1,
            controller_epoch: Epoch(9),
            controller_pid: 55,
            role: Role::Standby,
            grant_nonce: Some(secret(0x01)),
        });
        let value = serde_json::to_value(&hello).unwrap();
        assert_eq!(value["type"], "hello");
        assert_eq!(value["hostProtocol"], 1);
        assert_eq!(value["controllerEpoch"], 9);
        assert_eq!(value["role"], "standby");
        assert_eq!(value["token"], "ab".repeat(32));
        assert_eq!(value["grantNonce"], "01".repeat(32));
        assert_eq!(
            serde_json::from_value::<ControlToHost>(value).unwrap(),
            hello
        );
    }

    #[test]
    fn rejected_reply_parses_from_wire_json() {
        let json = br#"{"type":"rejected","code":"predecessorAlive","message":"m"}"#;
        let parsed: HostToControl = parse_control(json).unwrap();
        assert_eq!(
            parsed,
            HostToControl::Rejected {
                code: RejectCode::PredecessorAlive,
                message: "m".into()
            }
        );
    }

    #[test]
    fn secret_parse_rejects_wrong_length_and_non_hex() {
        assert_eq!(
            Secret32::parse_hex(&"a".repeat(63)),
            Err(SecretParseError::WrongLength(63))
        );
        let mut bad = "a".repeat(64);
        bad.replace_range(10..11, "g");
        assert_eq!(Secret32::parse_hex(&bad), Err(SecretParseError::NotHex(10)));
        assert!(Secret32::parse_hex(&"AbCd".repeat(16)).is_ok());
    }

    #[test]
    fn secret_debug_is_redacted() {
        let rendered = format!("{:?}", secret(0xcd));
        assert!(!rendered.contains("cdcd"));
    }

    #[test]
    fn snapshot_at_maximum_retained_history_fits_snapshot_cap() {
        // Production capacity and retention budget (DEFAULT_BUFFER_CAPACITY, 512 KiB).
        let hub = TerminalOutputHub::default();
        hub.register_session("s");
        // UTF-8 charset, every tracked private mode and a long SGR group, so evicted records
        // populate retained_state; 0xff is the worst JSON expansion per byte ("255,").
        let mut prelude = b"\x1b%G".to_vec();
        for mode in [1u16, 7, 25, 1000, 1002, 1003, 1004, 1005, 1006, 1049, 2004] {
            prelude.extend_from_slice(format!("\x1b[?{mode}h").as_bytes());
        }
        prelude.extend_from_slice(b"\x1b[1;3;4;5;7;9;38;2;255;255;255;48;2;255;255;255m");
        let chunk_len = prelude.len() + 64 * 1024;
        for _ in 0..32 {
            let mut chunk = prelude.clone();
            chunk.resize(chunk_len, 0xff);
            assert!(hub.publish("s", chunk).is_some());
        }
        // Overfill the resize ledger past its cap with widest-digit geometry.
        for _ in 0..5000 {
            assert!(hub.record_resize("s", u16::MAX, u16::MAX).is_some());
        }
        let snapshot = hub.export_session_state("s").unwrap();
        assert!(
            !snapshot.retained_state.as_ref().unwrap().is_empty(),
            "eviction must populate retained_state"
        );
        let chunk_bytes: usize = snapshot.chunks.iter().map(|c| c.bytes.len()).sum();
        assert!(chunk_bytes <= snapshot.capacity);
        assert!(chunk_bytes + chunk_len > snapshot.capacity, "ring must be full");
        assert_eq!(snapshot.resize_ledger.len(), 4096);
        let encoded = encode_frame(&Frame::snapshot(&snapshot).unwrap()).unwrap();
        let payload = encoded.len() - FRAME_HEADER_LEN;
        assert!(payload > MAX_FRAME_PAYLOAD, "must need the snapshot cap: {payload}");
        assert!(payload <= MAX_SNAPSHOT_PAYLOAD);
        assert_eq!(parse_snapshot(&encoded[FRAME_HEADER_LEN..]).unwrap(), snapshot);
    }

    #[test]
    fn parse_snapshot_rejects_invalid_next_sequence() {
        let json = br#"{"chunks":[],"next_sequence":0,"bracketed_paste_enabled":false,"resize_ledger":[],"replay_gap":null}"#;
        assert!(matches!(
            parse_snapshot(json),
            Err(FrameError::InvalidSnapshot(
                OutputHubSnapshotError::InvalidNextSequence(0)
            ))
        ));
    }
}
