//! Existing remote terminal binary-message codec (not native IPC framing).
//!
//! One WebSocket binary message is one frame. There is no payload length: never
//! scan terminal bytes for a second prefix. WebSocket implementations normally
//! reassemble transport fragments; `decode_fragments` supports other callers.
//! Text messages are lifecycle/control JSON and must never enter this codec.
use serde::{Deserialize, Serialize};

pub const METADATA_PREFIX: &[u8] = b"\x1b]777;ferryx;";
pub const HARD_RESET: &[u8] = b"\x1bc";
pub const MAX_METADATA_BYTES: usize = 16 * 1024;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayGap {
    pub requested_after_sequence: u64,
    pub available_from_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metadata {
    Output {
        sequence: u64,
        gap: Option<ReplayGap>,
    },
    Replay {
        start: Option<u64>,
        end: Option<u64>,
        gap: Option<ReplayGap>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    InvalidPrefix,
    TruncatedMetadata,
    MetadataTooLarge,
    FrameTooLarge,
    InvalidJson,
    InvalidSequence(&'static str),
    InvalidMetadata,
    MissingGapReset,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "remote terminal frame: {self:?}")
    }
}
impl std::error::Error for ParseError {}

#[derive(Debug, PartialEq, Eq)]
pub struct DecodedFrame<'a> {
    pub metadata: Metadata,
    /// Includes ESC c when present. Publish these bytes, not the OSC metadata.
    /// Do not remove ESC c: a PTY may itself emit an indistinguishable reset.
    pub terminal_bytes: &'a [u8],
}

#[derive(Debug, PartialEq, Eq)]
pub struct OwnedFrame {
    pub metadata: Metadata,
    pub terminal_bytes: Vec<u8>,
}

// Field order and omission match server.rs's existing serializer exactly.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireMetadata {
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requested_after_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    available_from_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_sequence: Option<String>,
}

fn sequence(value: Option<String>, field: &'static str) -> Result<Option<u64>, ParseError> {
    value
        .map(|s| {
            if s.is_empty()
                || (s.len() > 1 && s.starts_with('0'))
                || !s.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(ParseError::InvalidSequence(field));
            }
            s.parse().map_err(|_| ParseError::InvalidSequence(field))
        })
        .transpose()
}

fn parse_metadata(w: WireMetadata) -> Result<Metadata, ParseError> {
    let seq = sequence(w.sequence, "sequence")?;
    let start = sequence(w.start_sequence, "startSequence")?;
    let end = sequence(w.end_sequence, "endSequence")?;
    let requested = sequence(w.requested_after_sequence, "requestedAfterSequence")?;
    let available = sequence(w.available_from_sequence, "availableFromSequence")?;
    let gap = match (requested, available) {
        (None, None) => None,
        (Some(requested_after_sequence), Some(available_from_sequence)) => Some(ReplayGap {
            requested_after_sequence,
            available_from_sequence,
        }),
        _ => return Err(ParseError::InvalidMetadata),
    };
    if (w.kind == "replayGap") != gap.is_some() {
        return Err(ParseError::InvalidMetadata);
    }
    match w.kind.as_str() {
        "output" | "replayGap" if start.is_none() && end.is_none() && seq.is_some() => {
            Ok(Metadata::Output {
                sequence: seq.unwrap(),
                gap,
            })
        }
        "replay" | "replayGap"
            if start.is_some() == end.is_some() && seq == end && start <= end =>
        {
            Ok(Metadata::Replay { start, end, gap })
        }
        _ => Err(ParseError::InvalidMetadata),
    }
}

/// Encode the same metadata and raw bytes as the legacy server. A gap always
/// inserts a reset; a forced replay boundary inserts one even without a gap.
pub fn encode_frame(
    metadata: Metadata,
    payload: &[u8],
    force_reset: bool,
) -> Result<Vec<u8>, ParseError> {
    let (kind, seq, start, end, gap) = match metadata {
        Metadata::Output { sequence, gap } => ("output", Some(sequence), None, None, gap),
        Metadata::Replay { start, end, gap } => ("replay", end, start, end, gap),
    };
    let wire = WireMetadata {
        kind: if gap.is_some() { "replayGap" } else { kind }.into(),
        sequence: seq.map(|s| s.to_string()),
        requested_after_sequence: gap.map(|g| g.requested_after_sequence.to_string()),
        available_from_sequence: gap.map(|g| g.available_from_sequence.to_string()),
        start_sequence: start.map(|s| s.to_string()),
        end_sequence: end.map(|s| s.to_string()),
    };
    let json = serde_json::to_vec(&wire).expect("string-only metadata serializes");
    parse_metadata(wire)?;
    let reset = force_reset || gap.is_some();
    let header_len =
        METADATA_PREFIX.len() + json.len() + 1 + if reset { HARD_RESET.len() } else { 0 };
    if payload.len() > MAX_FRAME_BYTES - header_len {
        return Err(ParseError::FrameTooLarge);
    }
    let mut frame = Vec::with_capacity(header_len + payload.len());
    frame.extend_from_slice(METADATA_PREFIX);
    frame.extend_from_slice(&json);
    frame.push(7);
    if reset {
        frame.extend_from_slice(HARD_RESET);
    }
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Decode a complete binary message. No UTF-8 decoding is applied to PTY bytes.
/// Payload truncation cannot be detected by this lengthless format; the caller
/// must supply a complete WebSocket message or report transport failure.
pub fn decode_frame(frame: &[u8]) -> Result<DecodedFrame<'_>, ParseError> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(ParseError::FrameTooLarge);
    }
    if frame.len() < METADATA_PREFIX.len() && METADATA_PREFIX.starts_with(frame) {
        return Err(ParseError::TruncatedMetadata);
    }
    let body = frame
        .strip_prefix(METADATA_PREFIX)
        .ok_or(ParseError::InvalidPrefix)?;
    let Some(end) = body.iter().position(|b| *b == 7) else {
        return Err(if body.len() > MAX_METADATA_BYTES {
            ParseError::MetadataTooLarge
        } else {
            ParseError::TruncatedMetadata
        });
    };
    if end > MAX_METADATA_BYTES {
        return Err(ParseError::MetadataTooLarge);
    }
    let wire = serde_json::from_slice(&body[..end]).map_err(|_| ParseError::InvalidJson)?;
    let metadata = parse_metadata(wire)?;
    let terminal_bytes = &body[end + 1..];
    let gap = match metadata {
        Metadata::Output { gap, .. } | Metadata::Replay { gap, .. } => gap,
    };
    if gap.is_some() && !terminal_bytes.starts_with(HARD_RESET) {
        return Err(ParseError::MissingGapReset);
    }
    Ok(DecodedFrame {
        metadata,
        terminal_bytes,
    })
}

/// Fragments of exactly ONE complete binary message, in order. Bounded assembly;
/// iterator completion must mean the transport's message-end, not a read pause.
pub fn decode_fragments<'a>(
    fragments: impl IntoIterator<Item = &'a [u8]>,
) -> Result<OwnedFrame, ParseError> {
    let mut message = Vec::new();
    for fragment in fragments {
        if fragment.len() > MAX_FRAME_BYTES - message.len() {
            return Err(ParseError::FrameTooLarge);
        }
        message.extend_from_slice(fragment);
    }
    let decoded = decode_frame(&message)?;
    Ok(OwnedFrame {
        metadata: decoded.metadata,
        terminal_bytes: decoded.terminal_bytes.to_vec(),
    })
}
