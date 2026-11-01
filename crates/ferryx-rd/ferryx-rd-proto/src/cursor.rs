//! Bounded cursor images, delivered reliably on the TCP control stream.
use crate::{
    codec::{push_u16, push_u32, Decoder},
    CodecError, CursorUpdate, WireCodec,
};

pub const MAX_CURSOR_DIMENSION: u16 = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorShape {
    pub shape_id: u32,
    pub width: u16,
    pub height: u16,
    pub hotspot_x: u16,
    pub hotspot_y: u16,
    pub rgba: Vec<u8>,
}

impl CursorShape {
    fn validate(&self) -> Result<(), CodecError> {
        if self.shape_id == 0
            || self.width == 0
            || self.height == 0
            || self.width > MAX_CURSOR_DIMENSION
            || self.height > MAX_CURSOR_DIMENSION
            || self.hotspot_x >= self.width
            || self.hotspot_y >= self.height
        {
            return Err(CodecError::InvalidValue {
                field: "cursor shape geometry",
                value: self.shape_id.into(),
            });
        }
        let expected = usize::from(self.width) * usize::from(self.height) * 4;
        if self.rgba.len() != expected {
            return Err(CodecError::LengthLimit {
                field: "cursor RGBA bytes",
                actual: self.rgba.len(),
                max: expected,
            });
        }
        Ok(())
    }
}

impl WireCodec for CursorShape {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.validate()?;
        let mut out = Vec::with_capacity(12 + self.rgba.len());
        push_u32(&mut out, self.shape_id);
        for value in [self.width, self.height, self.hotspot_x, self.hotspot_y] {
            push_u16(&mut out, value);
        }
        out.extend_from_slice(&self.rgba);
        Ok(out)
    }
    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut d = Decoder::new(input);
        let shape_id = d.u32("cursor shape id")?;
        let width = d.u16("cursor width")?;
        let height = d.u16("cursor height")?;
        let hotspot_x = d.u16("cursor hotspot x")?;
        let hotspot_y = d.u16("cursor hotspot y")?;
        // Validate before allocating a peer-controlled image.
        let mut shape = Self {
            shape_id,
            width,
            height,
            hotspot_x,
            hotspot_y,
            rgba: Vec::new(),
        };
        let data = d.take_remaining();
        if shape_id == 0
            || hotspot_x >= width
            || hotspot_y >= height
            || width == 0
            || height == 0
            || width > MAX_CURSOR_DIMENSION
            || height > MAX_CURSOR_DIMENSION
            || data.len() != usize::from(width) * usize::from(height) * 4
        {
            return Err(CodecError::InvalidValue {
                field: "cursor shape bytes",
                value: data.len() as u64,
            });
        }
        shape.rgba = data.to_vec();
        shape.validate()?;
        Ok(shape)
    }
}

impl CursorUpdate {
    /// A missing capability keeps the byte-exact nine-byte legacy payload.
    pub fn encode_with_shape(&self, shape_id: Option<u32>) -> Result<Vec<u8>, CodecError> {
        let mut bytes = self.encode()?;
        if let Some(id) = shape_id {
            push_u32(&mut bytes, id);
        }
        Ok(bytes)
    }
    pub fn decode_with_shape(
        input: &[u8],
        negotiated: bool,
    ) -> Result<(Self, Option<u32>), CodecError> {
        if !negotiated {
            return Ok((Self::decode(input)?, None));
        }
        let mut d = Decoder::new(input);
        let cursor = Self::decode(d.take(Self::SIZE, "cursor position")?)?;
        let id = d.u32("cursor shape id")?;
        d.finish("shaped cursor update")?;
        Ok((cursor, Some(id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_shape_roundtrip_and_control_negotiation() {
        let shape = CursorShape {
            shape_id: 7,
            width: 2,
            height: 1,
            hotspot_x: 1,
            hotspot_y: 0,
            rgba: vec![255; 8],
        };
        assert_eq!(
            CursorShape::decode(&shape.encode().unwrap()).unwrap(),
            shape
        );
        let message = crate::ControlMessage::CursorShape(shape);
        assert_eq!(
            crate::ControlMessage::decode(&message.encode().unwrap()).unwrap(),
            message
        );
        assert_eq!(crate::Capabilities::CURSOR_SHAPE.bits(), 1 << 18);
        assert_eq!(crate::PacketType::CursorShape as u8, 15);
        let kind = message.message_type();
        assert!(!kind.supported_by(crate::Capabilities::all(), crate::Capabilities::empty()));
        assert!(!kind.supported_by(crate::Capabilities::empty(), crate::Capabilities::all()));
        assert!(kind.supported_by(crate::Capabilities::all(), crate::Capabilities::all()));
    }
    #[test]
    fn cursor_shape_rejects_oversize_invalid_hotspot_and_truncated_pixels() {
        let mut shape = CursorShape {
            shape_id: 1,
            width: 1,
            height: 1,
            hotspot_x: 0,
            hotspot_y: 0,
            rgba: vec![0; 4],
        };
        let bytes = shape.encode().unwrap();
        let mut invalid_id = bytes.clone();
        invalid_id[..4].fill(0);
        assert!(CursorShape::decode(&invalid_id).is_err());
        let mut oversized = bytes.clone();
        oversized[4..6].copy_from_slice(&129_u16.to_le_bytes());
        assert!(CursorShape::decode(&oversized).is_err());
        let mut invalid_hotspot = bytes.clone();
        invalid_hotspot[8..10].copy_from_slice(&1_u16.to_le_bytes());
        assert!(CursorShape::decode(&invalid_hotspot).is_err());
        assert!(CursorShape::decode(&bytes[..bytes.len() - 1]).is_err());
        shape.hotspot_x = 1;
        assert!(shape.encode().is_err());
        shape.hotspot_x = 0;
        shape.width = 129;
        assert!(shape.encode().is_err());
    }
    #[test]
    fn maximum_cursor_shape_survives_fragmented_reliable_control_framing() {
        let shape = CursorShape {
            shape_id: u32::MAX,
            width: MAX_CURSOR_DIMENSION,
            height: MAX_CURSOR_DIMENSION,
            hotspot_x: 127,
            hotspot_y: 127,
            rgba: vec![123; 128 * 128 * 4],
        };
        let message = crate::ControlMessage::CursorShape(shape);
        let payload = message.encode().unwrap();
        let framed = crate::TcpFrameWriter::encode(&payload).unwrap();
        let mut reader = crate::TcpFrameReader::new();
        let mut events = Vec::new();
        for chunk in framed.chunks(1031) {
            events.extend(reader.push(chunk));
        }
        assert_eq!(events, vec![crate::TcpFrameEvent::Frame(payload.clone())]);
        assert_eq!(crate::ControlMessage::decode(&payload).unwrap(), message);
        assert_eq!(reader.buffered_len(), 0);
    }

    #[test]
    fn cursor_update_old_peer_is_byte_exact_and_new_peer_requires_shape_id() {
        let cursor = CursorUpdate {
            x: 0.25,
            y: 0.5,
            cursor_type: 1,
        };
        let old = cursor.encode_with_shape(None).unwrap();
        assert_eq!(old, cursor.encode().unwrap());
        assert_eq!(old.len(), 9);
        assert_eq!(
            CursorUpdate::decode_with_shape(&old, false).unwrap(),
            (cursor, None)
        );
        let new = cursor.encode_with_shape(Some(42)).unwrap();
        assert_eq!(new.len(), 13);
        assert_eq!(
            CursorUpdate::decode_with_shape(&new, true).unwrap(),
            (cursor, Some(42))
        );
        assert!(CursorUpdate::decode_with_shape(&new, false).is_err());
        assert!(CursorUpdate::decode_with_shape(&old, true).is_err());
    }
}
