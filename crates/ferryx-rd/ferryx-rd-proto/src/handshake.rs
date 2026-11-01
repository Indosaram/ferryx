use std::ops::{BitOr, BitOrAssign};

use crate::{
    codec::{checked_bytes, push_f32, push_u16, push_u64, usize_to_u16, Decoder},
    CodecError, WireCodec, PROTOCOL_VERSION,
};

pub const MAX_HANDSHAKE_NAME_BYTES: usize = 1024;
pub const MAX_PAIRING_ID_BYTES: usize = 256;
pub const SESSION_SALT_SIZE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities(u64);

impl Capabilities {
    pub const STREAM_CONFIGURATION: Self = Self(1 << 0);
    pub const CLIPBOARD_SYNC: Self = Self(1 << 1);
    pub const TEXT_CLIPBOARD_SYNC: Self = Self(1 << 2);
    pub const COLOR_444: Self = Self(1 << 3);
    pub const COLOR_HDR: Self = Self(1 << 4);
    pub const GAMEPAD: Self = Self(1 << 5);
    pub const PEN_INPUT: Self = Self(1 << 6);
    pub const AUTHENTICATED_UDP_REGISTRATION: Self = Self(1 << 7);
    pub const STREAM_OPTIONS: Self = Self(1 << 8);
    pub const SECURE_ATTENTION: Self = Self(1 << 9);
    pub const HOST_STATUS: Self = Self(1 << 10);
    pub const SESSION_ENDED: Self = Self(1 << 11);
    pub const GUEST_PERMISSIONS: Self = Self(1 << 12);
    pub const PAIRING_TRUST: Self = Self(1 << 13);
    pub const CLIPBOARD_IMAGE: Self = Self(1 << 14);
    pub const AUDIO_OPUS: Self = Self(1 << 19);
    pub const AUDIO_SOURCE_TIME: Self = Self(1 << 20);
    pub const INPUT_ECHO: Self = Self(1 << 24);
    pub const CLOCK_CORRELATION: Self = Self(1 << 23); // d3
    pub const RTT_PROBE: Self = Self(1 << 15);
    /// Video XOR parity and GCM UDP NACK retransmission (PERF-TRANSPORT-3).
    pub const FEC_NACK: Self = Self(1 << 16);
    pub const UDP_INPUT: Self = Self(1 << 17);
    pub const CURSOR_SHAPE: Self = Self(1 << 18);
    /// Independent portal cursor position metadata (d1).
    pub const CURSOR_METADATA: Self = Self(1 << 21);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn all() -> Self {
        Self(
            Self::STREAM_CONFIGURATION.0
                | Self::CLIPBOARD_SYNC.0
                | Self::TEXT_CLIPBOARD_SYNC.0
                | Self::COLOR_444.0
                | Self::COLOR_HDR.0
                | Self::GAMEPAD.0
                | Self::PEN_INPUT.0
                | Self::AUTHENTICATED_UDP_REGISTRATION.0
                | Self::STREAM_OPTIONS.0
                | Self::SECURE_ATTENTION.0
                | Self::HOST_STATUS.0
                | Self::SESSION_ENDED.0
                | Self::GUEST_PERMISSIONS.0
                | Self::PAIRING_TRUST.0
                | Self::CLIPBOARD_IMAGE.0
                | Self::RTT_PROBE.0
                | Self::FEC_NACK.0
                | Self::UDP_INPUT.0
                | Self::CURSOR_SHAPE.0
                | Self::AUDIO_OPUS.0
                | Self::AUDIO_SOURCE_TIME.0
                | Self::CLOCK_CORRELATION.0
                | Self::CURSOR_METADATA.0
                | Self::INPUT_ECHO.0,
        )
    }

    pub const fn from_bits_retain(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for Capabilities {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Capabilities {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Handshake {
    pub name: String,
    pub width: u16,
    pub height: u16,
    pub scale: f32,
    pub version: u8,
    pub capabilities: Capabilities,
    pub pairing_id: String,
    pub session_salt: [u8; SESSION_SALT_SIZE],
}

impl WireCodec for Handshake {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        if self.version != PROTOCOL_VERSION {
            return Err(CodecError::UnsupportedVersion(self.version));
        }
        let name = checked_bytes(&self.name, "handshake name", MAX_HANDSHAKE_NAME_BYTES)?;
        let pairing_id = checked_bytes(
            &self.pairing_id,
            "handshake pairing ID",
            MAX_PAIRING_ID_BYTES,
        )?;
        let mut output = Vec::with_capacity(37 + name.len() + pairing_id.len());
        push_u16(&mut output, usize_to_u16(name.len(), "handshake name")?);
        output.extend_from_slice(name);
        push_u16(&mut output, self.width);
        push_u16(&mut output, self.height);
        push_f32(&mut output, self.scale);
        output.push(self.version);
        push_u64(&mut output, self.capabilities.bits());
        push_u16(
            &mut output,
            usize_to_u16(pairing_id.len(), "handshake pairing ID")?,
        );
        output.extend_from_slice(pairing_id);
        output.extend_from_slice(&self.session_salt);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let name_length = decoder.u16("handshake name length")? as usize;
        if name_length > MAX_HANDSHAKE_NAME_BYTES {
            return Err(CodecError::LengthLimit {
                field: "handshake name",
                actual: name_length,
                max: MAX_HANDSHAKE_NAME_BYTES,
            });
        }
        let name = decoder.utf8(name_length, "handshake name")?;
        let width = decoder.u16("handshake width")?;
        let height = decoder.u16("handshake height")?;
        let scale = decoder.f32("handshake scale")?;
        let version = decoder.u8("handshake version")?;
        if version != PROTOCOL_VERSION {
            return Err(CodecError::UnsupportedVersion(version));
        }
        let capabilities = Capabilities::from_bits_retain(decoder.u64("handshake capabilities")?);
        let pairing_id_length = decoder.u16("handshake pairing ID length")? as usize;
        if pairing_id_length > MAX_PAIRING_ID_BYTES {
            return Err(CodecError::LengthLimit {
                field: "handshake pairing ID",
                actual: pairing_id_length,
                max: MAX_PAIRING_ID_BYTES,
            });
        }
        let pairing_id = decoder.utf8(pairing_id_length, "handshake pairing ID")?;
        let salt = decoder.take(SESSION_SALT_SIZE, "handshake session salt")?;
        let mut session_salt = [0_u8; SESSION_SALT_SIZE];
        session_salt.copy_from_slice(salt);
        decoder.finish("handshake")?;
        Ok(Self {
            name,
            width,
            height,
            scale,
            version,
            capabilities,
            pairing_id,
            session_salt,
        })
    }
}

#[cfg(test)]
mod cursor_metadata_tests {
    use super::*;

    #[test]
    fn metadata_requires_explicit_peer_support() {
        assert!(!Capabilities::empty().contains(Capabilities::CURSOR_METADATA));
        assert!(
            !Capabilities::from_bits_retain((1 << 21) - 1).contains(Capabilities::CURSOR_METADATA)
        );
        assert!(Capabilities::all().contains(Capabilities::CURSOR_METADATA));
        assert_eq!(Capabilities::CURSOR_METADATA.bits(), 1 << 21);
    }

    #[test]
    fn metadata_capability_round_trips_without_changing_old_wire_shape() {
        let mut handshake = Handshake {
            name: "portal".into(),
            width: 1920,
            height: 1080,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::empty(),
            pairing_id: String::new(),
            session_salt: [0; SESSION_SALT_SIZE],
        };
        let old = handshake.encode().unwrap();
        assert!(!Handshake::decode(&old)
            .unwrap()
            .capabilities
            .contains(Capabilities::CURSOR_METADATA));
        handshake.capabilities = Capabilities::CURSOR_METADATA;
        let new = handshake.encode().unwrap();
        assert_eq!(old.len(), new.len());
        assert_eq!(Handshake::decode(&new).unwrap(), handshake);
    }
}
