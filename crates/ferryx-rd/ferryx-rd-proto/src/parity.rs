use crate::{
    codec::{checked_bytes, push_u16, push_u32, push_u64, usize_to_u16, Decoder},
    CodecError, WireCodec,
};

pub const MAX_ACTIVE_GUESTS: usize = 64;
pub const MAX_GUEST_NAME_BYTES: usize = 1024;
pub const MAX_CLIPBOARD_IMAGE_BYTES: u32 = 8 * 1024 * 1024;
pub const MAX_CLIPBOARD_IMAGE_CHUNK_BYTES: usize = 64 * 1024;
pub const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecureAttentionRequest {
    pub request_id: u32,
}

impl WireCodec for SecureAttentionRequest {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(4);
        push_u32(&mut output, self.request_id);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let request_id = decoder.u32("secure attention request ID")?;
        decoder.finish("secure attention request")?;
        Ok(Self { request_id })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SecureAttentionResult {
    Accepted = 0,
    PermissionDenied = 1,
    Unsupported = 2,
    Failed = 3,
}

impl TryFrom<u8> for SecureAttentionResult {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Accepted),
            1 => Ok(Self::PermissionDenied),
            2 => Ok(Self::Unsupported),
            3 => Ok(Self::Failed),
            value => Err(CodecError::InvalidValue {
                field: "secure attention result",
                value: value.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecureAttentionResponse {
    pub request_id: u32,
    pub result: SecureAttentionResult,
}

impl WireCodec for SecureAttentionResponse {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(5);
        push_u32(&mut output, self.request_id);
        output.push(self.result as u8);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let request_id = decoder.u32("secure attention request ID")?;
        let result = SecureAttentionResult::try_from(decoder.u8("secure attention result")?)?;
        decoder.finish("secure attention response")?;
        Ok(Self { request_id, result })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostStatus {
    pub guest_names: Vec<String>,
}

impl HostStatus {
    pub fn active_guest_count(&self) -> usize {
        self.guest_names.len()
    }
}

impl WireCodec for HostStatus {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        if self.guest_names.len() > MAX_ACTIVE_GUESTS {
            return Err(CodecError::LengthLimit {
                field: "active guests",
                actual: self.guest_names.len(),
                max: MAX_ACTIVE_GUESTS,
            });
        }
        let mut output = Vec::new();
        push_u16(
            &mut output,
            usize_to_u16(self.guest_names.len(), "active guests")?,
        );
        for name in &self.guest_names {
            let bytes = checked_bytes(name, "guest name", MAX_GUEST_NAME_BYTES)?;
            push_u16(&mut output, usize_to_u16(bytes.len(), "guest name")?);
            output.extend_from_slice(bytes);
        }
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let count = usize::from(decoder.u16("active guest count")?);
        if count > MAX_ACTIVE_GUESTS {
            return Err(CodecError::LengthLimit {
                field: "active guests",
                actual: count,
                max: MAX_ACTIVE_GUESTS,
            });
        }
        let mut guest_names = Vec::with_capacity(count);
        for _ in 0..count {
            let length = usize::from(decoder.u16("guest name length")?);
            if length > MAX_GUEST_NAME_BYTES {
                return Err(CodecError::LengthLimit {
                    field: "guest name",
                    actual: length,
                    max: MAX_GUEST_NAME_BYTES,
                });
            }
            guest_names.push(decoder.utf8(length, "guest name")?);
        }
        decoder.finish("host status")?;
        Ok(Self { guest_names })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionEndReason {
    Kicked = 0,
    HostStopped = 1,
    PermissionRevoked = 2,
    GuestExpired = 3,
    HostShutdown = 4,
}

impl TryFrom<u8> for SessionEndReason {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Kicked),
            1 => Ok(Self::HostStopped),
            2 => Ok(Self::PermissionRevoked),
            3 => Ok(Self::GuestExpired),
            4 => Ok(Self::HostShutdown),
            value => Err(CodecError::InvalidValue {
                field: "session end reason",
                value: value.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionEnded {
    pub reason: SessionEndReason,
}

impl WireCodec for SessionEnded {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        Ok(vec![self.reason as u8])
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let reason = SessionEndReason::try_from(decoder.u8("session end reason")?)?;
        decoder.finish("session ended")?;
        Ok(Self { reason })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestPermissions {
    pub keyboard: bool,
    pub mouse: bool,
    pub gamepad: bool,
    pub clipboard: bool,
    pub audio: bool,
}

impl WireCodec for GuestPermissions {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        Ok(vec![
            self.keyboard as u8,
            self.mouse as u8,
            self.gamepad as u8,
            self.clipboard as u8,
            self.audio as u8,
        ])
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let mut read_bool = |field| match decoder.u8(field)? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(CodecError::InvalidValue {
                field,
                value: value.into(),
            }),
        };
        let keyboard = read_bool("guest keyboard")?;
        let mouse = read_bool("guest mouse")?;
        let gamepad = read_bool("guest gamepad")?;
        let clipboard = read_bool("guest clipboard")?;
        let audio = read_bool("guest audio")?;
        decoder.finish("guest permissions")?;
        Ok(Self {
            keyboard,
            mouse,
            gamepad,
            clipboard,
            audio,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PairingTrust {
    #[default]
    Owner,
    GuestOnce,
    GuestExpiring {
        expires_at_unix_ms: u64,
    },
}

impl WireCodec for PairingTrust {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let (tag, expiry) = match *self {
            Self::Owner => (0, 0),
            Self::GuestOnce => (1, 0),
            Self::GuestExpiring {
                expires_at_unix_ms: 0,
            } => {
                return Err(CodecError::InvalidValue {
                    field: "guest expiry",
                    value: 0,
                })
            }
            Self::GuestExpiring { expires_at_unix_ms } => (2, expires_at_unix_ms),
        };
        let mut output = Vec::with_capacity(9);
        output.push(tag);
        push_u64(&mut output, expiry);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let tag = decoder.u8("pairing trust")?;
        let expiry = decoder.u64("guest expiry")?;
        decoder.finish("pairing trust")?;
        match (tag, expiry) {
            (0, 0) => Ok(Self::Owner),
            (1, 0) => Ok(Self::GuestOnce),
            (2, 1..=u64::MAX) => Ok(Self::GuestExpiring {
                expires_at_unix_ms: expiry,
            }),
            (0..=2, value) => Err(CodecError::InvalidValue {
                field: "guest expiry",
                value,
            }),
            (value, _) => Err(CodecError::InvalidValue {
                field: "pairing trust",
                value: value.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairingTrustRequest {
    pub trust: PairingTrust,
}

impl WireCodec for PairingTrustRequest {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.trust.encode()
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            trust: PairingTrust::decode(input)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairingTrustGrant {
    pub trust: PairingTrust,
}

impl WireCodec for PairingTrustGrant {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.trust.encode()
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            trust: PairingTrust::decode(input)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardImageChunk {
    pub transfer_id: u32,
    pub total_bytes: u32,
    pub offset: u32,
    pub png_bytes: Vec<u8>,
}

impl ClipboardImageChunk {
    fn validate(&self) -> Result<(), CodecError> {
        if self.total_bytes < PNG_SIGNATURE.len() as u32
            || self.total_bytes > MAX_CLIPBOARD_IMAGE_BYTES
        {
            return Err(CodecError::InvalidValue {
                field: "clipboard image total bytes",
                value: self.total_bytes.into(),
            });
        }
        if self.png_bytes.is_empty() || self.png_bytes.len() > MAX_CLIPBOARD_IMAGE_CHUNK_BYTES {
            return Err(CodecError::LengthLimit {
                field: "clipboard image chunk",
                actual: self.png_bytes.len(),
                max: MAX_CLIPBOARD_IMAGE_CHUNK_BYTES,
            });
        }
        if u64::from(self.offset) + self.png_bytes.len() as u64 > u64::from(self.total_bytes) {
            return Err(CodecError::InvalidValue {
                field: "clipboard image offset",
                value: self.offset.into(),
            });
        }
        if self.offset == 0 && !self.png_bytes.starts_with(PNG_SIGNATURE) {
            return Err(CodecError::InvalidValue {
                field: "PNG signature",
                value: 0,
            });
        }
        Ok(())
    }
}

impl WireCodec for ClipboardImageChunk {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.validate()?;
        let mut output = Vec::with_capacity(12 + self.png_bytes.len());
        push_u32(&mut output, self.transfer_id);
        push_u32(&mut output, self.total_bytes);
        push_u32(&mut output, self.offset);
        output.extend_from_slice(&self.png_bytes);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let transfer_id = decoder.u32("clipboard image transfer ID")?;
        let total_bytes = decoder.u32("clipboard image total bytes")?;
        let offset = decoder.u32("clipboard image offset")?;
        if decoder.remaining() > MAX_CLIPBOARD_IMAGE_CHUNK_BYTES {
            return Err(CodecError::LengthLimit {
                field: "clipboard image chunk",
                actual: decoder.remaining(),
                max: MAX_CLIPBOARD_IMAGE_CHUNK_BYTES,
            });
        }
        let value = Self {
            transfer_id,
            total_bytes,
            offset,
            png_bytes: decoder.take_remaining().to_vec(),
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RttProbe {
    pub timestamp_us: u64,
}

impl WireCodec for RttProbe {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(8);
        push_u64(&mut output, self.timestamp_us);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let timestamp_us = decoder.u64("RTT timestamp")?;
        decoder.finish("RTT probe")?;
        Ok(Self { timestamp_us })
    }
}

/// NTP four-timestamp exchange; all times are microseconds in each session epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockProbe {
    pub client_send_us: u64,
    pub host_receive_us: u64,
    pub host_send_us: u64,
}

impl WireCodec for ClockProbe {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(24);
        push_u64(&mut output, self.client_send_us);
        push_u64(&mut output, self.host_receive_us);
        push_u64(&mut output, self.host_send_us);
        Ok(output)
    }
    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let probe = Self {
            client_send_us: decoder.u64("client send")?,
            host_receive_us: decoder.u64("host receive")?,
            host_send_us: decoder.u64("host send")?,
        };
        decoder.finish("clock probe")?;
        Ok(probe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Capabilities, ControlMessage, ControlMessageType, TcpFrameEvent, TcpFrameReader,
        TcpFrameWriter,
    };

    macro_rules! control_case {
        ($name:ident, $value:expr, $tag:expr) => {
            #[test]
            fn $name() {
                let value = $value;
                let bytes = value.encode().unwrap();
                assert_eq!(bytes[0], $tag);
                assert_eq!(ControlMessage::decode(&bytes).unwrap(), value);
                for end in 0..bytes.len() {
                    assert!(
                        ControlMessage::decode(&bytes[..end]).is_err(),
                        "prefix {end}"
                    );
                }
                let mut trailing = bytes;
                trailing.push(0xff);
                assert!(ControlMessage::decode(&trailing).is_err());
            }
        };
    }

    control_case!(
        secure_attention_request_roundtrip_and_malformed,
        ControlMessage::SecureAttentionRequest(SecureAttentionRequest {
            request_id: 0x01020304
        }),
        19
    );
    control_case!(
        secure_attention_response_roundtrip_and_malformed,
        ControlMessage::SecureAttentionResponse(SecureAttentionResponse {
            request_id: 7,
            result: SecureAttentionResult::PermissionDenied
        }),
        20
    );
    control_case!(
        host_status_roundtrip_and_malformed,
        ControlMessage::HostStatus(HostStatus {
            guest_names: vec!["guest".into(), "owner".into()]
        }),
        21
    );
    control_case!(
        session_ended_roundtrip_and_malformed,
        ControlMessage::SessionEnded(SessionEnded {
            reason: SessionEndReason::Kicked
        }),
        22
    );
    control_case!(
        guest_permissions_roundtrip_and_malformed,
        ControlMessage::GuestPermissions(GuestPermissions {
            keyboard: true,
            mouse: false,
            gamepad: true,
            clipboard: false,
            audio: true
        }),
        23
    );
    control_case!(
        pairing_trust_request_roundtrip_and_malformed,
        ControlMessage::PairingTrustRequest(PairingTrustRequest {
            trust: PairingTrust::GuestOnce
        }),
        24
    );
    control_case!(
        pairing_trust_grant_roundtrip_and_malformed,
        ControlMessage::PairingTrustGrant(PairingTrustGrant {
            trust: PairingTrust::GuestExpiring {
                expires_at_unix_ms: 1234
            }
        }),
        25
    );
    control_case!(
        rtt_ping_roundtrip_and_malformed,
        ControlMessage::RttPing(RttProbe {
            timestamp_us: 0x0102030405060708
        }),
        27
    );
    control_case!(
        rtt_pong_roundtrip_and_malformed,
        ControlMessage::RttPong(RttProbe { timestamp_us: 123 }),
        28
    );

    control_case!(
        clock_ping_roundtrip_and_malformed,
        ControlMessage::ClockPing(ClockProbe {
            client_send_us: 100,
            host_receive_us: 0,
            host_send_us: 0
        }),
        30
    );
    control_case!(
        clock_pong_roundtrip_and_malformed,
        ControlMessage::ClockPong(ClockProbe {
            client_send_us: 100,
            host_receive_us: 200,
            host_send_us: 210
        }),
        31
    );

    #[test]
    fn clock_messages_are_not_sent_to_old_peers() {
        for tag in [ControlMessageType::ClockPing, ControlMessageType::ClockPong] {
            assert!(!tag.supported_by(Capabilities::all(), Capabilities::RTT_PROBE));
            assert!(!tag.supported_by(Capabilities::RTT_PROBE, Capabilities::all()));
            assert!(tag.supported_by(Capabilities::all(), Capabilities::all()));
        }
    }

    #[test]
    fn new_control_messages_require_both_peers_capability() {
        let bits = [9, 9, 10, 11, 12, 13, 13, 14, 15, 15];
        for (tag, bit) in (19..=28).zip(bits) {
            let message_type = ControlMessageType::try_from(tag).unwrap();
            let required = Capabilities::from_bits_retain(1 << bit);
            assert_eq!(message_type.required_capability(), Some(required));
            assert!(message_type.supported_by(required, required));
            assert!(!message_type.supported_by(required, Capabilities::empty()));
            assert!(!message_type.supported_by(Capabilities::empty(), required));
            assert!(Capabilities::all().contains(required));
        }
    }

    #[test]
    fn typed_results_reject_unknown_values() {
        assert!(SecureAttentionResponse::decode(&[1, 0, 0, 0, 4]).is_err());
        assert!(SessionEnded::decode(&[5]).is_err());
        for result in [
            SecureAttentionResult::Accepted,
            SecureAttentionResult::PermissionDenied,
            SecureAttentionResult::Unsupported,
            SecureAttentionResult::Failed,
        ] {
            let value = SecureAttentionResponse {
                request_id: 1,
                result,
            };
            assert_eq!(
                SecureAttentionResponse::decode(&value.encode().unwrap()).unwrap(),
                value
            );
        }
        for reason in [
            SessionEndReason::Kicked,
            SessionEndReason::HostStopped,
            SessionEndReason::PermissionRevoked,
            SessionEndReason::GuestExpired,
            SessionEndReason::HostShutdown,
        ] {
            let value = SessionEnded { reason };
            assert_eq!(
                SessionEnded::decode(&value.encode().unwrap()).unwrap(),
                value
            );
        }
    }

    #[test]
    fn guest_permissions_reject_non_boolean_fields() {
        for position in 0..5 {
            let mut bytes = [0; 5];
            bytes[position] = 2;
            assert!(GuestPermissions::decode(&bytes).is_err());
        }
    }

    #[test]
    fn host_status_enforces_count_name_and_utf8_bounds() {
        assert!(HostStatus::decode(&[65, 0]).is_err());
        assert!(HostStatus::decode(&[1, 0, 1, 4]).is_err());
        assert!(HostStatus::decode(&[1, 0, 1, 0, 0xff]).is_err());
        assert!(HostStatus {
            guest_names: vec![String::new(); 65]
        }
        .encode()
        .is_err());
        assert!(HostStatus {
            guest_names: vec!["x".repeat(1025)]
        }
        .encode()
        .is_err());
        let boundary = HostStatus {
            guest_names: vec!["x".repeat(1024); 64],
        };
        assert_eq!(
            HostStatus::decode(&boundary.encode().unwrap()).unwrap(),
            boundary
        );
        let empty = HostStatus {
            guest_names: Vec::new(),
        };
        assert_eq!(empty.active_guest_count(), 0);
        assert_eq!(HostStatus::decode(&empty.encode().unwrap()).unwrap(), empty);
    }

    #[test]
    fn pairing_trust_preserves_owner_default_and_canonical_expiry() {
        assert_eq!(PairingTrust::default(), PairingTrust::Owner);
        for trust in [
            PairingTrust::Owner,
            PairingTrust::GuestOnce,
            PairingTrust::GuestExpiring {
                expires_at_unix_ms: u64::MAX,
            },
        ] {
            assert_eq!(
                PairingTrust::decode(&trust.encode().unwrap()).unwrap(),
                trust
            );
        }
        assert!(PairingTrust::GuestExpiring {
            expires_at_unix_ms: 0
        }
        .encode()
        .is_err());
        for tag in [0, 1, 3] {
            let mut bytes = [0; 9];
            bytes[0] = tag;
            bytes[1] = 1;
            assert!(PairingTrustRequest::decode(&bytes).is_err());
            assert!(PairingTrustGrant::decode(&bytes).is_err());
        }
        assert!(PairingTrustRequest::decode(&[2, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(PairingTrustGrant::decode(&[2, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
    }

    #[test]
    fn clipboard_image_chunk_roundtrip_through_fragmented_framing() {
        let mut png_bytes = vec![0; MAX_CLIPBOARD_IMAGE_CHUNK_BYTES];
        png_bytes[..8].copy_from_slice(PNG_SIGNATURE);
        let value = ControlMessage::ClipboardImageChunk(ClipboardImageChunk {
            transfer_id: 42,
            total_bytes: MAX_CLIPBOARD_IMAGE_BYTES,
            offset: 0,
            png_bytes,
        });
        let bytes = value.encode().unwrap();
        assert_eq!(bytes[0], 26);
        assert!(bytes.len() + crate::PacketHeader::SIZE < crate::RELAY_MAX_PAYLOAD_LEN);
        let framed = TcpFrameWriter::encode(&bytes).unwrap();
        let mut reader = TcpFrameReader::new();
        let mut events = Vec::new();
        for fragment in framed.chunks(137) {
            events.extend(reader.push(fragment));
        }
        assert_eq!(events, vec![TcpFrameEvent::Frame(bytes.clone())]);
        assert_eq!(ControlMessage::decode(&bytes).unwrap(), value);
        let final_chunk = ClipboardImageChunk {
            transfer_id: 42,
            total_bytes: MAX_CLIPBOARD_IMAGE_BYTES,
            offset: MAX_CLIPBOARD_IMAGE_BYTES - 1,
            png_bytes: vec![0],
        };
        assert_eq!(
            ClipboardImageChunk::decode(&final_chunk.encode().unwrap()).unwrap(),
            final_chunk
        );
    }

    #[test]
    fn clipboard_image_chunk_rejects_malformed_bounds_and_signature() {
        let valid = ClipboardImageChunk {
            transfer_id: 1,
            total_bytes: 8,
            offset: 0,
            png_bytes: PNG_SIGNATURE.to_vec(),
        };
        let bytes = valid.encode().unwrap();
        for end in 0..bytes.len() {
            assert!(ClipboardImageChunk::decode(&bytes[..end]).is_err());
        }
        for invalid in [
            ClipboardImageChunk {
                total_bytes: MAX_CLIPBOARD_IMAGE_BYTES + 1,
                ..valid.clone()
            },
            ClipboardImageChunk {
                total_bytes: 7,
                ..valid.clone()
            },
            ClipboardImageChunk {
                offset: u32::MAX,
                ..valid.clone()
            },
            ClipboardImageChunk {
                png_bytes: Vec::new(),
                ..valid.clone()
            },
            ClipboardImageChunk {
                png_bytes: vec![0; 8],
                ..valid.clone()
            },
            ClipboardImageChunk {
                png_bytes: vec![0; MAX_CLIPBOARD_IMAGE_CHUNK_BYTES + 1],
                ..valid.clone()
            },
        ] {
            assert!(invalid.encode().is_err());
        }
        let mut overflow = bytes.clone();
        overflow[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(ClipboardImageChunk::decode(&overflow).is_err());
        let mut total = bytes.clone();
        total[4..8].copy_from_slice(&(MAX_CLIPBOARD_IMAGE_BYTES + 1).to_le_bytes());
        assert!(ClipboardImageChunk::decode(&total).is_err());
        let mut bad_signature = bytes.clone();
        bad_signature[12] = 0;
        assert!(ClipboardImageChunk::decode(&bad_signature).is_err());
        let mut oversized = bytes;
        oversized.resize(12 + MAX_CLIPBOARD_IMAGE_CHUNK_BYTES + 1, 0);
        assert!(ClipboardImageChunk::decode(&oversized).is_err());
    }

    #[test]
    fn new_integer_fields_use_existing_little_endian_helpers() {
        assert_eq!(
            SecureAttentionRequest {
                request_id: 0x01020304
            }
            .encode()
            .unwrap(),
            [4, 3, 2, 1]
        );
        assert_eq!(
            RttProbe {
                timestamp_us: 0x0102030405060708
            }
            .encode()
            .unwrap(),
            [8, 7, 6, 5, 4, 3, 2, 1]
        );
    }
}
