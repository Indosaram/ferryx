use crate::{
    codec::{push_f32, push_u16, push_u32, Decoder},
    CodecError, WireCodec,
};

pub const MAX_CHUNKS_PER_FRAME: u16 = 8192;
pub const MAX_FRAME_BYTES: u32 = 32 * 1024 * 1024;
pub const MAX_VIDEO_CHUNK_BYTES: usize = 1382;
pub const MAX_AUDIO_FRAGMENT_BYTES: usize = 1380;

pub const TIMESTAMP_STATS_MAGIC: &[u8; 6] = b"ERDTS1";

/// Legacy per-frame host timestamps, in host session-relative microseconds.
///
/// The 42-byte ERDTS1 payload uses little-endian fields. Decoding checks only
/// length and magic; timestamp ordering is a consumer concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimestampStats {
    pub frame_id: u32,
    pub capture_us: u64,
    pub encode_start_us: u64,
    pub encode_end_us: u64,
    pub send_us: u64,
}

impl TimestampStats {
    pub const SIZE: usize = 6 + 4 + 8 * 4;

    pub fn encode(self) -> Vec<u8> {
        let mut output = Vec::with_capacity(Self::SIZE);
        output.extend_from_slice(TIMESTAMP_STATS_MAGIC);
        output.extend_from_slice(&self.frame_id.to_le_bytes());
        output.extend_from_slice(&self.capture_us.to_le_bytes());
        output.extend_from_slice(&self.encode_start_us.to_le_bytes());
        output.extend_from_slice(&self.encode_end_us.to_le_bytes());
        output.extend_from_slice(&self.send_us.to_le_bytes());
        output
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::SIZE || &bytes[..6] != TIMESTAMP_STATS_MAGIC {
            return None;
        }
        Some(Self {
            frame_id: u32::from_le_bytes(bytes[6..10].try_into().ok()?),
            capture_us: u64::from_le_bytes(bytes[10..18].try_into().ok()?),
            encode_start_us: u64::from_le_bytes(bytes[18..26].try_into().ok()?),
            encode_end_us: u64::from_le_bytes(bytes[26..34].try_into().ok()?),
            send_us: u64::from_le_bytes(bytes[34..42].try_into().ok()?),
        })
    }
}

/// Negotiated per-frame input echo. Fields use the timestamp side channel's
/// little-endian byte order and host session-relative microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputEcho {
    pub frame_id: u32,
    pub last_applied_input_seq: u64,
    pub injection_timestamp_us: u64,
}

impl InputEcho {
    pub const SIZE: usize = 26;

    pub fn encode(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::SIZE);
        bytes.extend_from_slice(b"ERDIE1");
        bytes.extend_from_slice(&self.frame_id.to_le_bytes());
        bytes.extend_from_slice(&self.last_applied_input_seq.to_le_bytes());
        bytes.extend_from_slice(&self.injection_timestamp_us.to_le_bytes());
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::SIZE || &bytes[..6] != b"ERDIE1" {
            return None;
        }
        Some(Self {
            frame_id: u32::from_le_bytes(bytes[6..10].try_into().ok()?),
            last_applied_input_seq: u64::from_le_bytes(bytes[10..18].try_into().ok()?),
            injection_timestamp_us: u64::from_le_bytes(bytes[18..26].try_into().ok()?),
        })
    }
}

#[cfg(test)]
mod input_echo_tests {
    use super::*;

    #[test]
    fn round_trip_and_reject_other_metadata() {
        let echo = InputEcho {
            frame_id: 0x12345678,
            last_applied_input_seq: u64::MAX,
            injection_timestamp_us: 987654,
        };
        let encoded = echo.encode();
        assert_eq!(&encoded[6..10], &echo.frame_id.to_le_bytes());
        assert_eq!(InputEcho::decode(&encoded), Some(echo));
        assert_eq!(InputEcho::decode(&encoded[..25]), None);
        let mut invalid = encoded.clone();
        invalid[0] = 0;
        assert_eq!(InputEcho::decode(&invalid), None);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    pub frame_id: u32,
    pub width: u16,
    pub height: u16,
    pub is_key_frame: bool,
    pub total_chunks: u16,
    pub total_size: u32,
}

impl FrameHeader {
    pub const SIZE: usize = 16;

    fn validate(&self) -> Result<(), CodecError> {
        if self.total_chunks > MAX_CHUNKS_PER_FRAME {
            return Err(CodecError::LengthLimit {
                field: "frame chunks",
                actual: self.total_chunks as usize,
                max: MAX_CHUNKS_PER_FRAME as usize,
            });
        }
        if self.total_size > MAX_FRAME_BYTES {
            return Err(CodecError::LengthLimit {
                field: "frame bytes",
                actual: self.total_size as usize,
                max: MAX_FRAME_BYTES as usize,
            });
        }
        // A header must be completable by its own chunks: the chunk count is
        // already capped above, so this product fits in u32 (8192 * 1382).
        let chunk_capacity = u32::from(self.total_chunks) * MAX_VIDEO_CHUNK_BYTES as u32;
        if self.total_size > chunk_capacity {
            return Err(CodecError::LengthLimit {
                field: "frame bytes for chunk count",
                actual: self.total_size as usize,
                max: chunk_capacity as usize,
            });
        }
        Ok(())
    }
}

impl WireCodec for FrameHeader {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.validate()?;
        let mut output = Vec::with_capacity(Self::SIZE);
        push_u32(&mut output, self.frame_id);
        push_u16(&mut output, self.width);
        push_u16(&mut output, self.height);
        output.push(u8::from(self.is_key_frame));
        push_u16(&mut output, self.total_chunks);
        output.push(0);
        push_u32(&mut output, self.total_size);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            frame_id: decoder.u32("frame ID")?,
            width: decoder.u16("frame width")?,
            height: decoder.u16("frame height")?,
            is_key_frame: decoder.u8("frame key flag")? != 0,
            total_chunks: decoder.u16("frame chunk count")?,
            total_size: {
                let _padding = decoder.u8("frame padding")?;
                decoder.u32("frame total size")?
            },
        };
        decoder.finish("frame header")?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameChunk {
    pub frame_id: u32,
    pub chunk_index: u16,
    pub data: Vec<u8>,
}

impl FrameChunk {
    pub const HEADER_SIZE: usize = 6;

    fn validate(&self) -> Result<(), CodecError> {
        if self.chunk_index >= MAX_CHUNKS_PER_FRAME {
            return Err(CodecError::InvalidValue {
                field: "frame chunk index",
                value: u64::from(self.chunk_index),
            });
        }
        if self.data.len() > MAX_VIDEO_CHUNK_BYTES {
            return Err(CodecError::LengthLimit {
                field: "video chunk data",
                actual: self.data.len(),
                max: MAX_VIDEO_CHUNK_BYTES,
            });
        }
        Ok(())
    }
}

impl WireCodec for FrameChunk {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.validate()?;
        let mut output = Vec::with_capacity(Self::HEADER_SIZE + self.data.len());
        push_u32(&mut output, self.frame_id);
        push_u16(&mut output, self.chunk_index);
        output.extend_from_slice(&self.data);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let frame_id = decoder.u32("frame chunk ID")?;
        let chunk_index = decoder.u16("frame chunk index")?;
        if chunk_index >= MAX_CHUNKS_PER_FRAME {
            return Err(CodecError::InvalidValue {
                field: "frame chunk index",
                value: u64::from(chunk_index),
            });
        }
        let data = decoder.take_remaining();
        if data.len() > MAX_VIDEO_CHUNK_BYTES {
            return Err(CodecError::LengthLimit {
                field: "video chunk data",
                actual: data.len(),
                max: MAX_VIDEO_CHUNK_BYTES,
            });
        }
        Ok(Self {
            frame_id,
            chunk_index,
            data: data.to_vec(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorUpdate {
    pub x: f32,
    pub y: f32,
    pub cursor_type: u8,
}

impl CursorUpdate {
    pub const SIZE: usize = 9;
}

impl WireCodec for CursorUpdate {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        push_f32(&mut output, self.x);
        push_f32(&mut output, self.y);
        output.push(self.cursor_type);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let x = decoder.f32("cursor x")?;
        let y = decoder.f32("cursor y")?;
        let cursor_type = decoder.u8("cursor type")?;
        decoder.finish("cursor update")?;
        Ok(Self { x, y, cursor_type })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFragmentHeader {
    pub frame_id: u32,
    pub fragment_index: u16,
    pub fragment_count: u16,
}

impl AudioFragmentHeader {
    pub const SIZE: usize = 8;

    pub fn to_bytes(&self) -> Result<[u8; Self::SIZE], CodecError> {
        self.validate()?;
        let mut output = [0; Self::SIZE];
        output[..4].copy_from_slice(&self.frame_id.to_le_bytes());
        output[4..6].copy_from_slice(&self.fragment_index.to_le_bytes());
        output[6..8].copy_from_slice(&self.fragment_count.to_le_bytes());
        Ok(output)
    }

    fn validate(&self) -> Result<(), CodecError> {
        if self.fragment_count == 0 {
            return Err(CodecError::InvalidValue {
                field: "audio fragment count",
                value: 0,
            });
        }
        if self.fragment_index >= self.fragment_count {
            return Err(CodecError::InvalidValue {
                field: "audio fragment index",
                value: self.fragment_index as u64,
            });
        }
        Ok(())
    }
}

impl WireCodec for AudioFragmentHeader {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        Ok(self.to_bytes()?.to_vec())
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            frame_id: decoder.u32("audio frame ID")?,
            fragment_index: decoder.u16("audio fragment index")?,
            fragment_count: decoder.u16("audio fragment count")?,
        };
        decoder.finish("audio fragment header")?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFragment {
    pub header: AudioFragmentHeader,
    pub data: Vec<u8>,
}

impl AudioFragment {
    fn validate(&self) -> Result<(), CodecError> {
        self.header.validate()?;
        if self.data.len() > MAX_AUDIO_FRAGMENT_BYTES {
            return Err(CodecError::LengthLimit {
                field: "audio fragment data",
                actual: self.data.len(),
                max: MAX_AUDIO_FRAGMENT_BYTES,
            });
        }
        Ok(())
    }
}

impl WireCodec for AudioFragment {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.validate()?;
        let mut output = self.header.encode()?;
        output.reserve(self.data.len());
        output.extend_from_slice(&self.data);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let header = AudioFragmentHeader {
            frame_id: decoder.u32("audio frame ID")?,
            fragment_index: decoder.u16("audio fragment index")?,
            fragment_count: decoder.u16("audio fragment count")?,
        };
        header.validate()?;
        let data = decoder.take_remaining();
        if data.len() > MAX_AUDIO_FRAGMENT_BYTES {
            return Err(CodecError::LengthLimit {
                field: "audio fragment data",
                actual: data.len(),
                max: MAX_AUDIO_FRAGMENT_BYTES,
            });
        }
        Ok(Self {
            header,
            data: data.to_vec(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ColorRange {
    #[default]
    Limited = 0,
    Full = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ColorMatrix {
    #[default]
    Bt709 = 0,
    Bt601 = 1,
    Bt2020 = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ChromaSubsampling {
    #[default]
    Yuv420 = 0,
    Yuv444 = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ColorMetadata {
    pub range: ColorRange,
    pub matrix: ColorMatrix,
    pub chroma: ChromaSubsampling,
}

impl ColorMetadata {
    pub const SIZE: usize = 3;

    pub fn encode_into(&self, output: &mut Vec<u8>) {
        output.push(self.range as u8);
        output.push(self.matrix as u8);
        output.push(self.chroma as u8);
    }

    pub(crate) fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self, CodecError> {
        let range = match decoder.u8("color range")? {
            0 => ColorRange::Limited,
            1 => ColorRange::Full,
            unknown => return Err(CodecError::UnknownColorRange(unknown)),
        };
        let matrix = match decoder.u8("color matrix")? {
            0 => ColorMatrix::Bt709,
            1 => ColorMatrix::Bt601,
            2 => ColorMatrix::Bt2020,
            unknown => return Err(CodecError::UnknownColorMatrix(unknown)),
        };
        let chroma = match decoder.u8("chroma subsampling")? {
            0 => ChromaSubsampling::Yuv420,
            1 => ChromaSubsampling::Yuv444,
            unknown => return Err(CodecError::UnknownChromaSubsampling(unknown)),
        };
        Ok(Self {
            range,
            matrix,
            chroma,
        })
    }
}

impl WireCodec for ColorMetadata {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        self.encode_into(&mut output);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish("color metadata")?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_chunk_decode_rejects_index_at_global_limit() {
        // frame_id=1, chunk_index=8192: outside every legal zero-based index.
        let malformed = [0x01, 0x00, 0x00, 0x00, 0x00, 0x20, 0xaa];
        assert!(matches!(
            FrameChunk::decode(&malformed),
            Err(CodecError::InvalidValue { .. })
        ));
    }

    #[test]
    fn frame_chunk_encode_rejects_index_at_global_limit() {
        let chunk = FrameChunk {
            frame_id: 1,
            chunk_index: MAX_CHUNKS_PER_FRAME,
            data: vec![0xaa],
        };
        assert!(matches!(
            chunk.encode(),
            Err(CodecError::InvalidValue { .. })
        ));
    }

    #[test]
    fn frame_chunk_accepts_last_legal_index() {
        let bytes = [0u8, 0, 0, 0, 0xff, 0x1f, 0xaa];
        let chunk = FrameChunk::decode(&bytes).unwrap();
        assert_eq!(chunk.chunk_index, MAX_CHUNKS_PER_FRAME - 1);
        assert_eq!(chunk.data, vec![0xaa]);
    }

    #[test]
    fn frame_header_rejects_size_beyond_chunk_capacity() {
        // One chunk can carry at most MAX_VIDEO_CHUNK_BYTES.
        let header = FrameHeader {
            frame_id: 1,
            width: 1920,
            height: 1080,
            is_key_frame: true,
            total_chunks: 1,
            total_size: MAX_VIDEO_CHUNK_BYTES as u32 + 1,
        };
        assert!(matches!(
            header.encode(),
            Err(CodecError::LengthLimit { .. })
        ));
    }

    #[test]
    fn frame_header_rejects_bytes_with_no_chunks() {
        let header = FrameHeader {
            frame_id: 1,
            width: 1920,
            height: 1080,
            is_key_frame: true,
            total_chunks: 0,
            total_size: 1,
        };
        assert!(matches!(
            header.encode(),
            Err(CodecError::LengthLimit { .. })
        ));
    }

    #[test]
    fn frame_header_accepts_exact_chunk_capacity() {
        let header = FrameHeader {
            frame_id: 1,
            width: 1920,
            height: 1080,
            is_key_frame: true,
            total_chunks: 1,
            total_size: MAX_VIDEO_CHUNK_BYTES as u32,
        };
        let bytes = header.encode().unwrap();
        assert_eq!(FrameHeader::decode(&bytes).unwrap(), header);
    }
}
