use crate::browser::model::LogicalRect;
use serde::{Deserialize, Serialize};

pub const FRAME_ENVELOPE_KIND: u8 = 0x62;
pub const FRAME_ENVELOPE_VERSION: u8 = 1;
pub const FRAME_OPCODE_FRAME: u8 = 1;
pub const FRAME_FORMAT_JPEG: u8 = 1;
pub const FRAME_FORMAT_PNG: u8 = 2;
pub const HEADER_BYTE_LENGTH: usize = 16;
pub const MAX_METADATA_BYTES: usize = 4096; // 4 KiB
pub const MAX_FRAME_PAYLOAD_BYTES: usize = 2 * 1024 * 1024; // 2 MiB
pub const MAX_JSON_PAYLOAD_BYTES: usize = 512 * 1024; // 512 KiB
pub const MAX_IMAGE_EDGE: u32 = 2048;
pub const MAX_IMAGE_PIXELS: u64 = 4 * 1024 * 1024; // 4 MP

pub const IPC_CONTENT_TYPE_JSON: u8 = 0x01;
pub const IPC_CONTENT_TYPE_IMAGE: u8 = 0x02;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteProtocolError {
    HeaderTooShort { actual: usize },
    InvalidKind { actual: u8 },
    InvalidVersion { actual: u8 },
    InvalidOpcode { actual: u8 },
    InvalidFormat { actual: u8 },
    ReservedNonZero { actual: u32 },
    MetadataTooLarge { actual: usize, max: usize },
    FrameTooLarge { actual: usize, max: usize },
    InvalidJsonMetadata(String),
    MissingRequiredExtension(&'static str),
    NonFiniteCoordinate(&'static str),
    InvalidCaptureRect,
    EmptyImagePayload,
    ImageDimensionsTooLarge { width: u32, height: u32 },
    ImagePixelsTooLarge { pixels: u64, max: u64 },
    IpcPayloadTooLarge { actual: usize, max: usize },
    IpcFrameIncomplete { expected: usize, available: usize },
    InvalidIpcContentType(u8),
}

impl std::fmt::Display for RemoteProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderTooShort { actual } => write!(f, "header too short: {} bytes (min 16)", actual),
            Self::InvalidKind { actual } => write!(f, "invalid kind: 0x{:02x}, expected 0x62", actual),
            Self::InvalidVersion { actual } => write!(f, "invalid version: {}, expected 1", actual),
            Self::InvalidOpcode { actual } => write!(f, "invalid opcode: {}, expected 1", actual),
            Self::InvalidFormat { actual } => write!(f, "invalid format: {}, expected 1 (jpeg) or 2 (png)", actual),
            Self::ReservedNonZero { actual } => write!(f, "reserved field is non-zero: {}", actual),
            Self::MetadataTooLarge { actual, max } => write!(f, "metadata too large: {} bytes (max {})", actual, max),
            Self::FrameTooLarge { actual, max } => write!(f, "frame too large: {} bytes (max {})", actual, max),
            Self::InvalidJsonMetadata(msg) => write!(f, "invalid JSON metadata: {}", msg),
            Self::MissingRequiredExtension(field) => write!(f, "missing required extension field: {}", field),
            Self::NonFiniteCoordinate(field) => write!(f, "non-finite coordinate in: {}", field),
            Self::InvalidCaptureRect => write!(f, "invalid capture_rect"),
            Self::EmptyImagePayload => write!(f, "empty image payload"),
            Self::ImageDimensionsTooLarge { width, height } => {
                write!(f, "image dimensions exceed {}px: {}x{}", MAX_IMAGE_EDGE, width, height)
            }
            Self::ImagePixelsTooLarge { pixels, max } => write!(f, "image total pixels {} exceed max {}", pixels, max),
            Self::IpcPayloadTooLarge { actual, max } => write!(f, "IPC payload too large: {} bytes (max {})", actual, max),
            Self::IpcFrameIncomplete { expected, available } => {
                write!(f, "IPC frame incomplete: need {} bytes, available {}", expected, available)
            }
            Self::InvalidIpcContentType(ct) => write!(f, "invalid IPC content type: 0x{:02x}", ct),
        }
    }
}

impl std::error::Error for RemoteProtocolError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    pub kind: u8,
    pub version: u8,
    pub opcode: u8,
    pub format: u8,
    pub seq: u32,
    pub metadata_byte_length: u32,
    pub reserved: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFrameMetadata {
    pub offset_top: f64,
    pub page_scale_factor: f64,
    pub device_width: f64,
    pub device_height: f64,
    pub image_width: u32,
    pub image_height: u32,
    pub scroll_offset_x: f64,
    pub scroll_offset_y: f64,
    pub timestamp: f64,
    // Ferryx required extensions
    pub stream_id: u64,
    pub browser_instance_id: String,
    pub browser_service_epoch: String,
    pub desktop_epoch: String,
    pub document_generation: String,
    pub viewport_revision: u64,
    pub capture_rect: LogicalRect,
    pub geometry_source: String,
}

impl RemoteFrameMetadata {
    pub fn validate(&self) -> Result<(), RemoteProtocolError> {
        if !self.offset_top.is_finite() {
            return Err(RemoteProtocolError::NonFiniteCoordinate("offsetTop"));
        }
        if !self.page_scale_factor.is_finite() || self.page_scale_factor <= 0.0 {
            return Err(RemoteProtocolError::NonFiniteCoordinate("pageScaleFactor"));
        }
        if !self.device_width.is_finite() || self.device_width < 0.0 {
            return Err(RemoteProtocolError::NonFiniteCoordinate("deviceWidth"));
        }
        if !self.device_height.is_finite() || self.device_height < 0.0 {
            return Err(RemoteProtocolError::NonFiniteCoordinate("deviceHeight"));
        }
        if !self.scroll_offset_x.is_finite() {
            return Err(RemoteProtocolError::NonFiniteCoordinate("scrollOffsetX"));
        }
        if !self.scroll_offset_y.is_finite() {
            return Err(RemoteProtocolError::NonFiniteCoordinate("scrollOffsetY"));
        }
        if !self.timestamp.is_finite() || self.timestamp < 0.0 {
            return Err(RemoteProtocolError::NonFiniteCoordinate("timestamp"));
        }

        if self.image_width == 0 || self.image_height == 0 {
            return Err(RemoteProtocolError::EmptyImagePayload);
        }
        if self.image_width > MAX_IMAGE_EDGE || self.image_height > MAX_IMAGE_EDGE {
            return Err(RemoteProtocolError::ImageDimensionsTooLarge {
                width: self.image_width,
                height: self.image_height,
            });
        }
        let pixels = (self.image_width as u64) * (self.image_height as u64);
        if pixels > MAX_IMAGE_PIXELS {
            return Err(RemoteProtocolError::ImagePixelsTooLarge {
                pixels,
                max: MAX_IMAGE_PIXELS,
            });
        }

        if self.browser_instance_id.trim().is_empty() {
            return Err(RemoteProtocolError::MissingRequiredExtension("browserInstanceId"));
        }
        if self.browser_service_epoch.trim().is_empty() {
            return Err(RemoteProtocolError::MissingRequiredExtension("browserServiceEpoch"));
        }
        if self.desktop_epoch.trim().is_empty() {
            return Err(RemoteProtocolError::MissingRequiredExtension("desktopEpoch"));
        }
        if self.document_generation.trim().is_empty() {
            return Err(RemoteProtocolError::MissingRequiredExtension("documentGeneration"));
        }
        if self.geometry_source.trim().is_empty() {
            return Err(RemoteProtocolError::MissingRequiredExtension("geometrySource"));
        }
        if !self.capture_rect.is_valid() {
            return Err(RemoteProtocolError::InvalidCaptureRect);
        }

        Ok(())
    }
}

pub fn encode_frame(
    format: u8,
    seq: u32,
    metadata: &RemoteFrameMetadata,
    image_bytes: &[u8],
) -> Result<Vec<u8>, RemoteProtocolError> {
    if format != FRAME_FORMAT_JPEG && format != FRAME_FORMAT_PNG {
        return Err(RemoteProtocolError::InvalidFormat { actual: format });
    }
    if image_bytes.is_empty() {
        return Err(RemoteProtocolError::EmptyImagePayload);
    }
    metadata.validate()?;

    let metadata_bytes = serde_json::to_vec(metadata)
        .map_err(|e| RemoteProtocolError::InvalidJsonMetadata(e.to_string()))?;

    if metadata_bytes.len() > MAX_METADATA_BYTES {
        return Err(RemoteProtocolError::MetadataTooLarge {
            actual: metadata_bytes.len(),
            max: MAX_METADATA_BYTES,
        });
    }

    let total_len = HEADER_BYTE_LENGTH + metadata_bytes.len() + image_bytes.len();
    if total_len > MAX_FRAME_PAYLOAD_BYTES {
        return Err(RemoteProtocolError::FrameTooLarge {
            actual: total_len,
            max: MAX_FRAME_PAYLOAD_BYTES,
        });
    }

    let mut out = Vec::with_capacity(total_len);
    // Header (16 bytes)
    out.push(FRAME_ENVELOPE_KIND);
    out.push(FRAME_ENVELOPE_VERSION);
    out.push(FRAME_OPCODE_FRAME);
    out.push(format);
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&(metadata_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved = 0

    // Metadata payload
    out.extend_from_slice(&metadata_bytes);

    // Image payload
    out.extend_from_slice(image_bytes);

    Ok(out)
}

pub fn decode_frame(
    bytes: &[u8],
) -> Result<(FrameHeader, RemoteFrameMetadata, Vec<u8>), RemoteProtocolError> {
    if bytes.len() < HEADER_BYTE_LENGTH {
        return Err(RemoteProtocolError::HeaderTooShort { actual: bytes.len() });
    }
    if bytes.len() > MAX_FRAME_PAYLOAD_BYTES {
        return Err(RemoteProtocolError::FrameTooLarge {
            actual: bytes.len(),
            max: MAX_FRAME_PAYLOAD_BYTES,
        });
    }

    let kind = bytes[0];
    if kind != FRAME_ENVELOPE_KIND {
        return Err(RemoteProtocolError::InvalidKind { actual: kind });
    }

    let version = bytes[1];
    if version != FRAME_ENVELOPE_VERSION {
        return Err(RemoteProtocolError::InvalidVersion { actual: version });
    }

    let opcode = bytes[2];
    if opcode != FRAME_OPCODE_FRAME {
        return Err(RemoteProtocolError::InvalidOpcode { actual: opcode });
    }

    let format = bytes[3];
    if format != FRAME_FORMAT_JPEG && format != FRAME_FORMAT_PNG {
        return Err(RemoteProtocolError::InvalidFormat { actual: format });
    }

    let seq = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let metadata_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let reserved = u32::from_le_bytes(bytes[12..16].try_into().unwrap());

    if reserved != 0 {
        return Err(RemoteProtocolError::ReservedNonZero { actual: reserved });
    }

    if metadata_len > MAX_METADATA_BYTES {
        return Err(RemoteProtocolError::MetadataTooLarge {
            actual: metadata_len,
            max: MAX_METADATA_BYTES,
        });
    }

    if bytes.len() < HEADER_BYTE_LENGTH + metadata_len {
        return Err(RemoteProtocolError::IpcFrameIncomplete {
            expected: HEADER_BYTE_LENGTH + metadata_len,
            available: bytes.len(),
        });
    }

    let metadata_bytes = &bytes[HEADER_BYTE_LENGTH..HEADER_BYTE_LENGTH + metadata_len];
    let metadata: RemoteFrameMetadata = serde_json::from_slice(metadata_bytes)
        .map_err(|e| RemoteProtocolError::InvalidJsonMetadata(e.to_string()))?;
    metadata.validate()?;

    let image_bytes = bytes[HEADER_BYTE_LENGTH + metadata_len..].to_vec();
    if image_bytes.is_empty() {
        return Err(RemoteProtocolError::EmptyImagePayload);
    }

    let header = FrameHeader {
        kind,
        version,
        opcode,
        format,
        seq,
        metadata_byte_length: metadata_len as u32,
        reserved,
    };

    Ok((header, metadata, image_bytes))
}

// Framed IPC wire: `u32LE payloadLength + 1B contentType + payload`
pub fn encode_ipc_frame(content_type: u8, payload: &[u8]) -> Result<Vec<u8>, RemoteProtocolError> {
    if content_type != IPC_CONTENT_TYPE_JSON && content_type != IPC_CONTENT_TYPE_IMAGE {
        return Err(RemoteProtocolError::InvalidIpcContentType(content_type));
    }

    let max_len = if content_type == IPC_CONTENT_TYPE_JSON {
        MAX_JSON_PAYLOAD_BYTES
    } else {
        MAX_FRAME_PAYLOAD_BYTES
    };

    if payload.len() > max_len {
        return Err(RemoteProtocolError::IpcPayloadTooLarge {
            actual: payload.len(),
            max: max_len,
        });
    }

    let mut out = Vec::with_capacity(5 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.push(content_type);
    out.extend_from_slice(payload);
    Ok(out)
}

pub fn decode_ipc_frame(cursor: &[u8]) -> Result<Option<(u8, Vec<u8>, usize)>, RemoteProtocolError> {
    if cursor.len() < 5 {
        return Ok(None);
    }

    let payload_len = u32::from_le_bytes(cursor[0..4].try_into().unwrap()) as usize;
    let content_type = cursor[4];

    if content_type != IPC_CONTENT_TYPE_JSON && content_type != IPC_CONTENT_TYPE_IMAGE {
        return Err(RemoteProtocolError::InvalidIpcContentType(content_type));
    }

    let max_len = if content_type == IPC_CONTENT_TYPE_JSON {
        MAX_JSON_PAYLOAD_BYTES
    } else {
        MAX_FRAME_PAYLOAD_BYTES
    };

    if payload_len > max_len {
        return Err(RemoteProtocolError::IpcPayloadTooLarge {
            actual: payload_len,
            max: max_len,
        });
    }

    let total_needed = 5 + payload_len;
    if cursor.len() < total_needed {
        return Ok(None);
    }

    let payload = cursor[5..total_needed].to_vec();
    Ok(Some((content_type, payload, total_needed)))
}

// JSON Protocol Types (CamelCase)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamOptions {
    pub format: String, // "jpeg" or "png"
    pub quality: Option<u8>,
    pub interval_ms: Option<u64>,
    pub max_edge: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHello {
    pub r#type: String, // "browserHello"
    pub protocol_version: u32,
    pub browser_id: String,
    pub browser_instance_id: String,
    pub desktop_epoch: String,
    pub browser_service_epoch: String,
    pub supported_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSubscribe {
    pub r#type: String, // "browserSubscribe"
    pub request_id: String,
    pub viewer_instance_id: String,
    pub options: Option<StreamOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSubscribed {
    pub r#type: String, // "browserSubscribed"
    pub request_id: String,
    pub subscription_id: String,
    pub stream_id: u64,
    pub browser_id: String,
    pub browser_instance_id: String,
    pub browser_service_epoch: String,
    pub desktop_epoch: String,
    pub document_generation: String,
    pub options: StreamOptions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserFrameAck {
    pub r#type: String, // "browserFrameAck"
    pub subscription_id: String,
    pub stream_id: u64,
    pub seq: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHeartbeat {
    pub r#type: String, // "browserHeartbeat"
    pub subscription_id: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPong {
    pub r#type: String, // "browserPong"
    pub subscription_id: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDriverClaim {
    pub r#type: String, // "browserDriverClaim"
    pub subscription_id: String,
    pub browser_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDriverClaimed {
    pub r#type: String, // "browserDriverClaimed"
    pub lease_epoch: u64,
    pub browser_id: String,
    pub device_id: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDriverRelease {
    pub r#type: String, // "browserDriverRelease"
    pub subscription_id: String,
    pub lease_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDriverReleased {
    pub r#type: String, // "browserDriverReleased"
    pub lease_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCommand {
    pub r#type: String, // "browserCommand"
    pub request_id: String,
    pub request_seq: String,
    pub browser_id: String,
    pub lease_epoch: u64,
    pub browser_instance_id: String,
    pub desktop_epoch: String,
    pub document_generation: String,
    pub command: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserResult {
    pub r#type: String, // "browserResult"
    pub request_id: String,
    pub request_seq: String,
    pub result: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserErrorPayload {
    pub r#type: String, // "browserError"
    pub request_id: String,
    pub request_seq: Option<String>,
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_metadata() -> RemoteFrameMetadata {
        RemoteFrameMetadata {
            offset_top: 0.0,
            page_scale_factor: 2.0,
            device_width: 800.0,
            device_height: 600.0,
            image_width: 1600,
            image_height: 1200,
            scroll_offset_x: 0.0,
            scroll_offset_y: 100.0,
            timestamp: 1726567200.0,
            stream_id: 42,
            browser_instance_id: "inst-123".to_string(),
            browser_service_epoch: "3".to_string(),
            desktop_epoch: "7".to_string(),
            document_generation: "12".to_string(),
            viewport_revision: 5,
            capture_rect: LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            geometry_source: "wkSnapshot".to_string(),
        }
    }

    #[test]
    fn test_16b_binary_envelope_codec() {
        let meta = sample_metadata();
        let dummy_jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46];

        let encoded = encode_frame(FRAME_FORMAT_JPEG, 101, &meta, &dummy_jpeg).expect("encode frame");
        assert!(encoded.len() > HEADER_BYTE_LENGTH);

        // Verify header layout
        assert_eq!(encoded[0], 0x62);
        assert_eq!(encoded[1], 1);
        assert_eq!(encoded[2], 1);
        assert_eq!(encoded[3], 1); // jpeg
        let seq = u32::from_le_bytes(encoded[4..8].try_into().unwrap());
        assert_eq!(seq, 101);
        let meta_len = u32::from_le_bytes(encoded[8..12].try_into().unwrap()) as usize;
        let reserved = u32::from_le_bytes(encoded[12..16].try_into().unwrap());
        assert_eq!(reserved, 0);

        let (header, decoded_meta, decoded_img) = decode_frame(&encoded).expect("decode frame");
        assert_eq!(header.kind, 0x62);
        assert_eq!(header.version, 1);
        assert_eq!(header.opcode, 1);
        assert_eq!(header.format, FRAME_FORMAT_JPEG);
        assert_eq!(header.seq, 101);
        assert_eq!(header.metadata_byte_length as usize, meta_len);
        assert_eq!(header.reserved, 0);

        assert_eq!(decoded_meta, meta);
        assert_eq!(decoded_img, dummy_jpeg);
    }

    #[test]
    fn test_binary_envelope_validation_rejects_corrupt() {
        let meta = sample_metadata();
        let dummy_jpeg = vec![1, 2, 3, 4];
        let encoded = encode_frame(FRAME_FORMAT_JPEG, 1, &meta, &dummy_jpeg).unwrap();

        // 1. Header too short
        assert!(matches!(
            decode_frame(&encoded[..15]),
            Err(RemoteProtocolError::HeaderTooShort { actual: 15 })
        ));

        // 2. Invalid kind
        let mut bad_kind = encoded.clone();
        bad_kind[0] = 0x63;
        assert!(matches!(
            decode_frame(&bad_kind),
            Err(RemoteProtocolError::InvalidKind { actual: 0x63 })
        ));

        // 3. Invalid version
        let mut bad_ver = encoded.clone();
        bad_ver[1] = 2;
        assert!(matches!(
            decode_frame(&bad_ver),
            Err(RemoteProtocolError::InvalidVersion { actual: 2 })
        ));

        // 4. Invalid opcode
        let mut bad_op = encoded.clone();
        bad_op[2] = 2;
        assert!(matches!(
            decode_frame(&bad_op),
            Err(RemoteProtocolError::InvalidOpcode { actual: 2 })
        ));

        // 5. Invalid format
        let mut bad_fmt = encoded.clone();
        bad_fmt[3] = 3;
        assert!(matches!(
            decode_frame(&bad_fmt),
            Err(RemoteProtocolError::InvalidFormat { actual: 3 })
        ));

        // 6. Non-zero reserved
        let mut bad_res = encoded.clone();
        bad_res[12] = 1;
        assert!(matches!(
            decode_frame(&bad_res),
            Err(RemoteProtocolError::ReservedNonZero { actual: 1 })
        ));

        // 7. Empty image
        assert!(matches!(
            encode_frame(FRAME_FORMAT_JPEG, 1, &meta, &[]),
            Err(RemoteProtocolError::EmptyImagePayload)
        ));

        // 8. Image dimensions too large
        let mut huge_meta = meta.clone();
        huge_meta.image_width = 2049;
        assert!(matches!(
            encode_frame(FRAME_FORMAT_JPEG, 1, &huge_meta, &dummy_jpeg),
            Err(RemoteProtocolError::ImageDimensionsTooLarge { width: 2049, .. })
        ));

        // 9. Non-finite coordinate
        let mut nan_meta = meta.clone();
        nan_meta.scroll_offset_x = f64::NAN;
        assert!(matches!(
            encode_frame(FRAME_FORMAT_JPEG, 1, &nan_meta, &dummy_jpeg),
            Err(RemoteProtocolError::NonFiniteCoordinate("scrollOffsetX"))
        ));

        // 10. Missing extension field
        let mut missing_meta = meta.clone();
        missing_meta.browser_instance_id = "".to_string();
        assert!(matches!(
            encode_frame(FRAME_FORMAT_JPEG, 1, &missing_meta, &dummy_jpeg),
            Err(RemoteProtocolError::MissingRequiredExtension("browserInstanceId"))
        ));
    }

    #[test]
    fn test_framed_ipc_codec() {
        let json_payload = b"{\"type\":\"browserHeartbeat\",\"subscriptionId\":\"s1\",\"timestamp\":1000}";
        let framed = encode_ipc_frame(IPC_CONTENT_TYPE_JSON, json_payload).expect("encode ipc");
        assert_eq!(framed.len(), 5 + json_payload.len());

        let (ct, payload, consumed) = decode_ipc_frame(&framed)
            .expect("decode ipc")
            .expect("some frame");
        assert_eq!(ct, IPC_CONTENT_TYPE_JSON);
        assert_eq!(payload, json_payload);
        assert_eq!(consumed, framed.len());

        // Incomplete buffer returns None
        let partial = decode_ipc_frame(&framed[..framed.len() - 1]).expect("partial");
        assert_eq!(partial, None);

        // JSON payload over 512 KiB rejected
        let huge_json = vec![b'a'; 512 * 1024 + 1];
        assert!(matches!(
            encode_ipc_frame(IPC_CONTENT_TYPE_JSON, &huge_json),
            Err(RemoteProtocolError::IpcPayloadTooLarge { .. })
        ));
    }
}
