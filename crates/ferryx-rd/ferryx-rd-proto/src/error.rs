use thiserror::Error;

/// Errors produced while encoding or decoding version 3 wire values.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CodecError {
    #[error("truncated {field}: needed {needed} bytes, only {remaining} remain")]
    Truncated {
        field: &'static str,
        needed: usize,
        remaining: usize,
    },
    #[error("invalid packet magic 0x{found:04x}")]
    InvalidMagic { found: u16 },
    #[error("unknown packet type {0}")]
    UnknownPacketType(u8),
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u8),
    #[error("unknown input event type {0}")]
    UnknownInputEventType(u8),
    #[error("unknown control message type {0}")]
    UnknownControlMessageType(u8),
    #[error("unknown pairing rejection reason {0}")]
    UnknownPairingRejectReason(u8),
    #[error("unknown stream configuration error code {0}")]
    UnknownStreamConfigurationErrorCode(u8),
    #[error("unknown clipboard direction {0}")]
    UnknownClipboardDirection(u8),
    #[error("unknown clipboard origin {0}")]
    UnknownClipboardOrigin(u8),
    #[error("unknown color range {0}")]
    UnknownColorRange(u8),
    #[error("unknown color matrix {0}")]
    UnknownColorMatrix(u8),
    #[error("unknown chroma subsampling {0}")]
    UnknownChromaSubsampling(u8),
    #[error("invalid UTF-8 in {field}")]
    InvalidUtf8 { field: &'static str },
    #[error("invalid {field} length {actual}; expected {expected}")]
    InvalidLength {
        field: &'static str,
        actual: usize,
        expected: usize,
    },
    #[error("{field} length {actual} exceeds maximum {max}")]
    LengthLimit {
        field: &'static str,
        actual: usize,
        max: usize,
    },
    #[error("invalid value for {field}: {value}")]
    InvalidValue { field: &'static str, value: u64 },
    #[error("trailing bytes after {field}: {remaining}")]
    TrailingBytes {
        field: &'static str,
        remaining: usize,
    },
    #[error("TCP frame length {0} is invalid")]
    InvalidFrameLength(u32),
    #[error("unknown relay channel {0}")]
    UnknownRelayChannel(u8),
}
