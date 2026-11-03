use crate::{codec::Decoder, CodecError, WireCodec};

pub const MAGIC: u16 = 0xec1d;
pub const PROTOCOL_VERSION: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    Handshake = 0,
    HandshakeAck = 1,
    FrameHeader = 2,
    FrameChunk = 3,
    CursorUpdate = 4,
    InputEvent = 5,
    Control = 6,
    Ping = 7,
    AudioFrame = 8,
    PairingRequest = 9,
    PairingGrant = 10,
    PairingReject = 11,
    /// XOR parity for one video frame; requires `Capabilities::FEC_NACK`.
    FrameParity = 12,
    /// Client-to-host chunk retransmit request; requires `Capabilities::FEC_NACK`.
    FrameNack = 13,
    InputBatch = 14,
    CursorShape = 15,
}

impl PacketType {
    pub const ALL: [Self; 16] = [
        Self::Handshake,
        Self::HandshakeAck,
        Self::FrameHeader,
        Self::FrameChunk,
        Self::CursorUpdate,
        Self::InputEvent,
        Self::Control,
        Self::Ping,
        Self::AudioFrame,
        Self::PairingRequest,
        Self::PairingGrant,
        Self::PairingReject,
        Self::FrameParity,
        Self::FrameNack,
        Self::InputBatch,
        Self::CursorShape,
    ];
}

impl TryFrom<u8> for PacketType {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Handshake),
            1 => Ok(Self::HandshakeAck),
            2 => Ok(Self::FrameHeader),
            3 => Ok(Self::FrameChunk),
            4 => Ok(Self::CursorUpdate),
            5 => Ok(Self::InputEvent),
            6 => Ok(Self::Control),
            7 => Ok(Self::Ping),
            8 => Ok(Self::AudioFrame),
            9 => Ok(Self::PairingRequest),
            10 => Ok(Self::PairingGrant),
            11 => Ok(Self::PairingReject),
            12 => Ok(Self::FrameParity),
            13 => Ok(Self::FrameNack),
            14 => Ok(Self::InputBatch),
            15 => Ok(Self::CursorShape),
            unknown => Err(CodecError::UnknownPacketType(unknown)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketHeader {
    pub packet_type: PacketType,
    pub sequence: u32,
    pub timestamp_ms: u32,
    pub flags: u8,
}

impl PacketHeader {
    pub const SIZE: usize = 12;

    pub const fn new(packet_type: PacketType, sequence: u32, timestamp_ms: u32, flags: u8) -> Self {
        Self {
            packet_type,
            sequence,
            timestamp_ms,
            flags,
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut output = [0; Self::SIZE];
        output[..2].copy_from_slice(&MAGIC.to_le_bytes());
        output[2] = self.packet_type as u8;
        output[3..7].copy_from_slice(&self.sequence.to_le_bytes());
        output[7..11].copy_from_slice(&self.timestamp_ms.to_le_bytes());
        output[11] = self.flags;
        output
    }
}

impl WireCodec for PacketHeader {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        Ok(self.to_bytes().to_vec())
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let magic = decoder.u16("packet magic")?;
        if magic != MAGIC {
            return Err(CodecError::InvalidMagic { found: magic });
        }
        let packet_type = PacketType::try_from(decoder.u8("packet type")?)?;
        let sequence = decoder.u32("packet sequence")?;
        let timestamp_ms = decoder.u32("packet timestamp")?;
        let flags = decoder.u8("packet flags")?;
        decoder.finish("packet header")?;
        Ok(Self {
            packet_type,
            sequence,
            timestamp_ms,
            flags,
        })
    }
}
