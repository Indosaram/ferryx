use crate::messages::{Message, PayloadError};
use crate::types::Uuid;
use crate::wire::Put;

pub const MAGIC: u32 = 0x4658_5348;
pub const MAJOR: u16 = 1;
pub const MINOR: u16 = 0;
pub const HEADER_LEN: usize = 40;
pub const MAX_PAYLOAD: u32 = 16 * 1024 * 1024;

pub const FLAG_RESPONSE: u16 = 1 << 0;
pub const FLAG_ERROR: u16 = 1 << 1;
pub const FLAG_EVENT: u16 = 1 << 2;
const FLAG_DEFINED: u16 = FLAG_RESPONSE | FLAG_ERROR | FLAG_EVENT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub major: u16,
    pub minor: u16,
    pub command: u16,
    pub flags: u16,
    pub session_id: Uuid,
    pub request_id: u64,
    pub payload_len: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatalFrameError {
    BadMagic,
    PayloadTooLarge(u32),
    UndefinedFlags(u16),
    Truncated,
}

impl Header {
    pub fn encode(&self, w: &mut Vec<u8>) {
        w.put_u32(MAGIC);
        w.put_u16(self.major);
        w.put_u16(self.minor);
        w.put_u16(self.command);
        w.put_u16(self.flags);
        w.put_raw(&self.session_id.0);
        w.put_u64(self.request_id);
        w.put_u32(self.payload_len);
    }

    pub fn decode(b: &[u8; HEADER_LEN]) -> Result<Header, FatalFrameError> {
        let u16_at = |o: usize| u16::from_be_bytes([b[o], b[o + 1]]);
        let u32_at = |o: usize| u32::from_be_bytes(b[o..o + 4].try_into().expect("4-byte slice"));
        if u32_at(0) != MAGIC {
            return Err(FatalFrameError::BadMagic);
        }
        let flags = u16_at(10);
        if flags & !FLAG_DEFINED != 0 {
            return Err(FatalFrameError::UndefinedFlags(flags));
        }
        let payload_len = u32_at(36);
        if payload_len > MAX_PAYLOAD {
            return Err(FatalFrameError::PayloadTooLarge(payload_len));
        }
        Ok(Header {
            major: u16_at(4),
            minor: u16_at(6),
            command: u16_at(8),
            flags,
            session_id: Uuid(b[12..28].try_into().expect("16-byte slice")),
            request_id: u64::from_be_bytes(b[28..36].try_into().expect("8-byte slice")),
            payload_len,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub session_id: Uuid,
    pub request_id: u64,
    pub flags: u16,
    pub message: Message,
}

impl Frame {
    pub fn encode(&self) -> Vec<u8> {
        let mut payload = Vec::new();
        self.message.encode_payload(&mut payload);
        let payload_len = u32::try_from(payload.len()).expect("FXSH payload exceeds u32");
        assert!(payload_len <= MAX_PAYLOAD, "FXSH payload exceeds 16 MiB");
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        Header {
            major: MAJOR,
            minor: MINOR,
            command: self.message.command(),
            flags: self.flags,
            session_id: self.session_id,
            request_id: self.request_id,
            payload_len,
        }
        .encode(&mut out);
        out.extend_from_slice(&payload);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoded {
    NeedMore,
    Frame { consumed: usize, header: Header, frame: Frame },
    Rejected { consumed: usize, header: Header, error: PayloadError },
}

pub fn decode_frame(buf: &[u8]) -> Result<Decoded, FatalFrameError> {
    if buf.len() < HEADER_LEN {
        return Ok(Decoded::NeedMore);
    }
    let header = Header::decode(buf[..HEADER_LEN].try_into().expect("header slice"))?;
    let total = HEADER_LEN + header.payload_len as usize;
    if buf.len() < total {
        return Ok(Decoded::NeedMore);
    }
    let payload = &buf[HEADER_LEN..total];
    Ok(match Message::decode_payload(header.command, payload) {
        Ok(message) => Decoded::Frame {
            consumed: total,
            header,
            frame: Frame { session_id: header.session_id, request_id: header.request_id, flags: header.flags, message },
        },
        Err(error) => Decoded::Rejected { consumed: total, header, error },
    })
}

pub fn decode_complete_frame(buf: &[u8]) -> Result<Decoded, FatalFrameError> {
    match decode_frame(buf)? {
        Decoded::NeedMore => Err(FatalFrameError::Truncated),
        d => Ok(d),
    }
}
