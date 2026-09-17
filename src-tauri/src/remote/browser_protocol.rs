//! Remote Browser Screencast Protocol - Public Wire Codec & DTOs
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§1.3, §4.1, §4.2)

use serde::{Deserialize, Serialize};

pub const PROTOCOL_KIND: u8 = 0x62; // 'b' in ASCII
pub const PROTOCOL_VERSION: u8 = 1;
pub const OPCODE_FRAME: u8 = 1;
pub const FORMAT_JPEG: u8 = 1;
pub const FORMAT_PNG: u8 = 2;

pub const HEADER_BYTE_LENGTH: usize = 16;
pub const MAX_METADATA_BYTES: usize = 4096; // 4 KiB
pub const MAX_FRAME_PAYLOAD_BYTES: usize = 2 * 1024 * 1024; // 2 MiB
pub const MAX_IMAGE_EDGE: u32 = 2048;
pub const MAX_IMAGE_PIXELS: u64 = 4_000_000; // 4 MP cap

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserImageFormat {
    Jpeg,
    Png,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserCaptureRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserFrameMetadata {
    pub offset_top: f64,
    pub page_scale_factor: f64,
    pub device_width: f64,
    pub device_height: f64,
    pub image_width: u32,
    pub image_height: u32,
    pub scroll_offset_x: f64,
    pub scroll_offset_y: f64,
    pub timestamp: f64,
    // Ferryx required extension fields (§4.2)
    pub stream_id: u32,
    pub browser_instance_id: String,
    pub browser_service_epoch: String, // u64 decimal string
    pub desktop_epoch: String,         // u64 decimal string
    pub document_generation: String,   // u64 decimal string
    pub viewport_revision: String,     // u64 decimal string
    pub capture_rect: BrowserCaptureRect,
    pub geometry_source: String, // Must be "wkSnapshot"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryFrameHeader {
    pub kind: u8,
    pub version: u8,
    pub opcode: u8,
    pub format: u8,
    pub seq: u32,
    pub metadata_byte_length: u32,
    pub reserved: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedBrowserFrame {
    pub header: BinaryFrameHeader,
    pub format: BrowserImageFormat,
    pub seq: u32,
    pub metadata: BrowserFrameMetadata,
    pub image_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolCodecError {
    #[error("Frame buffer too short: expected at least {expected} bytes, got {actual}")]
    BufferTooShort { expected: usize, actual: usize },
    #[error("Frame buffer exceeds limit: got {actual} bytes, max {max}")]
    BufferTooLarge { actual: usize, max: usize },
    #[error("Invalid protocol kind: expected 0x62, got {0:#x}")]
    InvalidKind(u8),
    #[error("Invalid protocol version: expected 1, got {0}")]
    InvalidVersion(u8),
    #[error("Invalid opcode: expected 1 (Frame), got {0}")]
    InvalidOpcode(u8),
    #[error("Invalid format code: expected 1 (jpeg) or 2 (png), got {0}")]
    InvalidFormat(u8),
    #[error("Reserved field must be 0, got {0}")]
    ReservedNonZero(u32),
    #[error("Metadata length exceeds 4KiB: got {actual} bytes, max {max}")]
    MetadataTooLarge { actual: usize, max: usize },
    #[error("Truncated frame: expected at least {expected} bytes, got {actual}")]
    TruncatedFrame { expected: usize, actual: usize },
    #[error("Empty image payload")]
    EmptyImagePayload,
    #[error("Invalid metadata JSON: {0}")]
    InvalidMetadataJson(String),
    #[error("Non-finite numeric value in metadata: {0}")]
    NonFiniteNumeric(&'static str),
    #[error("Image edge dimension exceeds 2048px: {width}x{height}")]
    ImageEdgeExceeded { width: u32, height: u32 },
    #[error("Image pixels exceed 4MP: {pixels} pixels")]
    ImagePixelsExceeded { pixels: u64 },
    #[error("Invalid decimal string for {0}: must be unsigned 64-bit integer")]
    InvalidDecimalString(&'static str),
    #[error("Geometry source must be 'wkSnapshot', got '{0}'")]
    InvalidGeometrySource(String),
    #[error("Invalid image header: {0}")]
    InvalidImageHeader(String),
    #[error("Image dimension mismatch: header has {header_w}x{header_h}, metadata has {meta_w}x{meta_h}")]
    DimensionMismatch {
        header_w: u32,
        header_h: u32,
        meta_w: u32,
        meta_h: u32,
    },
    #[error("Client to server binary frames are rejected")]
    ClientBinaryRejected,
}

fn parse_jpeg_dimensions(bytes: &[u8]) -> Result<(u32, u32), ProtocolCodecError> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return Err(ProtocolCodecError::InvalidImageHeader(
            "Missing JPEG SOI marker (0xFF 0xD8)".into(),
        ));
    }
    let mut offset = 2;
    while offset < bytes.len() {
        if bytes[offset] != 0xff {
            offset += 1;
            continue;
        }
        while offset < bytes.len() && bytes[offset] == 0xff {
            offset += 1;
        }
        if offset >= bytes.len() {
            break;
        }
        let marker = bytes[offset];
        offset += 1;

        if marker == 0xd8 || marker == 0xd9 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if marker == 0xda {
            // Start of Scan
            break;
        }
        if offset + 2 > bytes.len() {
            break;
        }
        let len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        let is_sof = matches!(
            marker,
            0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc5 | 0xc6 | 0xc7 | 0xc9 | 0xca | 0xcb | 0xcd | 0xce | 0xcf
        );
        if is_sof {
            if offset + len <= bytes.len() && len >= 7 {
                let height = u16::from_be_bytes([bytes[offset + 3], bytes[offset + 4]]) as u32;
                let width = u16::from_be_bytes([bytes[offset + 5], bytes[offset + 6]]) as u32;
                return Ok((width, height));
            }
            break;
        }
        offset += len;
    }
    Err(ProtocolCodecError::InvalidImageHeader(
        "Could not locate JPEG SOF marker for dimensions".into(),
    ))
}

fn parse_png_dimensions(bytes: &[u8]) -> Result<(u32, u32), ProtocolCodecError> {
    const PNG_SIG: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 {
        return Err(ProtocolCodecError::InvalidImageHeader(
            "PNG buffer too short for IHDR".into(),
        ));
    }
    if &bytes[0..8] != &PNG_SIG {
        return Err(ProtocolCodecError::InvalidImageHeader(
            "Invalid PNG signature".into(),
        ));
    }
    if &bytes[12..16] != b"IHDR" {
        return Err(ProtocolCodecError::InvalidImageHeader(
            "First PNG chunk is not IHDR".into(),
        ));
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Ok((width, height))
}

pub fn is_decimal_u64_string(val: &str) -> bool {
    !val.is_empty() && val.chars().all(|c| c.is_ascii_digit()) && val.parse::<u64>().is_ok()
}

pub fn validate_metadata(meta: &BrowserFrameMetadata) -> Result<(), ProtocolCodecError> {
    if !meta.offset_top.is_finite() {
        return Err(ProtocolCodecError::NonFiniteNumeric("offset_top"));
    }
    if !meta.page_scale_factor.is_finite() || meta.page_scale_factor <= 0.0 {
        return Err(ProtocolCodecError::NonFiniteNumeric("page_scale_factor"));
    }
    if !meta.device_width.is_finite() || meta.device_width <= 0.0 {
        return Err(ProtocolCodecError::NonFiniteNumeric("device_width"));
    }
    if !meta.device_height.is_finite() || meta.device_height <= 0.0 {
        return Err(ProtocolCodecError::NonFiniteNumeric("device_height"));
    }
    if !meta.scroll_offset_x.is_finite() {
        return Err(ProtocolCodecError::NonFiniteNumeric("scroll_offset_x"));
    }
    if !meta.scroll_offset_y.is_finite() {
        return Err(ProtocolCodecError::NonFiniteNumeric("scroll_offset_y"));
    }
    if !meta.timestamp.is_finite() {
        return Err(ProtocolCodecError::NonFiniteNumeric("timestamp"));
    }
    if !meta.capture_rect.x.is_finite()
        || !meta.capture_rect.y.is_finite()
        || !meta.capture_rect.width.is_finite()
        || !meta.capture_rect.height.is_finite()
    {
        return Err(ProtocolCodecError::NonFiniteNumeric("capture_rect"));
    }
    if meta.image_width == 0 || meta.image_height == 0 || meta.image_width > MAX_IMAGE_EDGE || meta.image_height > MAX_IMAGE_EDGE {
        return Err(ProtocolCodecError::ImageEdgeExceeded {
            width: meta.image_width,
            height: meta.image_height,
        });
    }
    let pixels = (meta.image_width as u64) * (meta.image_height as u64);
    if pixels > MAX_IMAGE_PIXELS {
        return Err(ProtocolCodecError::ImagePixelsExceeded { pixels });
    }
    if meta.stream_id == 0 {
        return Err(ProtocolCodecError::NonFiniteNumeric("stream_id"));
    }
    if meta.browser_instance_id.is_empty() {
        return Err(ProtocolCodecError::InvalidDecimalString("browser_instance_id"));
    }
    if !is_decimal_u64_string(&meta.browser_service_epoch) {
        return Err(ProtocolCodecError::InvalidDecimalString("browser_service_epoch"));
    }
    if !is_decimal_u64_string(&meta.desktop_epoch) {
        return Err(ProtocolCodecError::InvalidDecimalString("desktop_epoch"));
    }
    if !is_decimal_u64_string(&meta.document_generation) {
        return Err(ProtocolCodecError::InvalidDecimalString("document_generation"));
    }
    if !is_decimal_u64_string(&meta.viewport_revision) {
        return Err(ProtocolCodecError::InvalidDecimalString("viewport_revision"));
    }
    if meta.geometry_source != "wkSnapshot" {
        return Err(ProtocolCodecError::InvalidGeometrySource(
            meta.geometry_source.clone(),
        ));
    }
    Ok(())
}

pub fn encode_binary_frame(
    format: BrowserImageFormat,
    seq: u32,
    metadata: &BrowserFrameMetadata,
    image_bytes: &[u8],
) -> Result<Vec<u8>, ProtocolCodecError> {
    validate_metadata(metadata)?;

    if image_bytes.is_empty() {
        return Err(ProtocolCodecError::EmptyImagePayload);
    }

    let meta_json = serde_json::to_vec(metadata)
        .map_err(|e| ProtocolCodecError::InvalidMetadataJson(e.to_string()))?;

    if meta_json.len() > MAX_METADATA_BYTES {
        return Err(ProtocolCodecError::MetadataTooLarge {
            actual: meta_json.len(),
            max: MAX_METADATA_BYTES,
        });
    }

    let total_len = HEADER_BYTE_LENGTH + meta_json.len() + image_bytes.len();
    if total_len > MAX_FRAME_PAYLOAD_BYTES {
        return Err(ProtocolCodecError::BufferTooLarge {
            actual: total_len,
            max: MAX_FRAME_PAYLOAD_BYTES,
        });
    }

    let format_code = match format {
        BrowserImageFormat::Jpeg => FORMAT_JPEG,
        BrowserImageFormat::Png => FORMAT_PNG,
    };

    let mut out = Vec::with_capacity(total_len);
    out.push(PROTOCOL_KIND);
    out.push(PROTOCOL_VERSION);
    out.push(OPCODE_FRAME);
    out.push(format_code);
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&(meta_json.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // Reserved = 0

    out.extend_from_slice(&meta_json);
    out.extend_from_slice(image_bytes);

    Ok(out)
}

pub fn decode_binary_frame(bytes: &[u8]) -> Result<DecodedBrowserFrame, ProtocolCodecError> {
    if bytes.len() < HEADER_BYTE_LENGTH {
        return Err(ProtocolCodecError::BufferTooShort {
            expected: HEADER_BYTE_LENGTH,
            actual: bytes.len(),
        });
    }
    if bytes.len() > MAX_FRAME_PAYLOAD_BYTES {
        return Err(ProtocolCodecError::BufferTooLarge {
            actual: bytes.len(),
            max: MAX_FRAME_PAYLOAD_BYTES,
        });
    }

    let kind = bytes[0];
    let version = bytes[1];
    let opcode = bytes[2];
    let format_code = bytes[3];
    let seq = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let metadata_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let reserved = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);

    if kind != PROTOCOL_KIND {
        return Err(ProtocolCodecError::InvalidKind(kind));
    }
    if version != PROTOCOL_VERSION {
        return Err(ProtocolCodecError::InvalidVersion(version));
    }
    if opcode != OPCODE_FRAME {
        return Err(ProtocolCodecError::InvalidOpcode(opcode));
    }
    let format = match format_code {
        FORMAT_JPEG => BrowserImageFormat::Jpeg,
        FORMAT_PNG => BrowserImageFormat::Png,
        other => return Err(ProtocolCodecError::InvalidFormat(other)),
    };
    if reserved != 0 {
        return Err(ProtocolCodecError::ReservedNonZero(reserved));
    }
    if metadata_len > MAX_METADATA_BYTES {
        return Err(ProtocolCodecError::MetadataTooLarge {
            actual: metadata_len,
            max: MAX_METADATA_BYTES,
        });
    }

    let min_total = HEADER_BYTE_LENGTH + metadata_len;
    if bytes.len() < min_total {
        return Err(ProtocolCodecError::TruncatedFrame {
            expected: min_total,
            actual: bytes.len(),
        });
    }

    let meta_bytes = &bytes[HEADER_BYTE_LENGTH..min_total];
    let image_bytes = &bytes[min_total..];

    if image_bytes.is_empty() {
        return Err(ProtocolCodecError::EmptyImagePayload);
    }

    let metadata: BrowserFrameMetadata = serde_json::from_slice(meta_bytes)
        .map_err(|e| ProtocolCodecError::InvalidMetadataJson(e.to_string()))?;

    validate_metadata(&metadata)?;

    // Validate dimensions in image header against metadata
    match format {
        BrowserImageFormat::Jpeg => {
            let (w, h) = parse_jpeg_dimensions(image_bytes)?;
            if w != metadata.image_width || h != metadata.image_height {
                return Err(ProtocolCodecError::DimensionMismatch {
                    header_w: w,
                    header_h: h,
                    meta_w: metadata.image_width,
                    meta_h: metadata.image_height,
                });
            }
        }
        BrowserImageFormat::Png => {
            let (w, h) = parse_png_dimensions(image_bytes)?;
            if w != metadata.image_width || h != metadata.image_height {
                return Err(ProtocolCodecError::DimensionMismatch {
                    header_w: w,
                    header_h: h,
                    meta_w: metadata.image_width,
                    meta_h: metadata.image_height,
                });
            }
        }
    }

    let header = BinaryFrameHeader {
        kind,
        version,
        opcode,
        format: format_code,
        seq,
        metadata_byte_length: metadata_len as u32,
        reserved,
    };

    Ok(DecodedBrowserFrame {
        header,
        format,
        seq,
        metadata,
        image_bytes: image_bytes.to_vec(),
    })
}

// ---------------------------------------------------------------------------
// JSON Messages (§1.3, §4.1)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserSubscribeOptions {
    pub format: BrowserImageFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_edge: Option<u32>,
}

impl Default for BrowserSubscribeOptions {
    fn default() -> Self {
        Self {
            format: BrowserImageFormat::Jpeg,
            quality: Some(70),
            interval_ms: Some(250),
            max_edge: Some(1280),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum ClientMessage {
    #[serde(rename_all = "camelCase")]
    BrowserSubscribe {
        request_id: String,
        viewer_instance_id: String,
        options: BrowserSubscribeOptions,
    },
    #[serde(rename_all = "camelCase")]
    BrowserFrameAck { stream_id: u32, seq: u32 },
    #[serde(rename_all = "camelCase")]
    BrowserHeartbeat {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lease_epoch: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subscription_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    BrowserDriverClaim {
        request_id: String,
        subscription_id: String,
        browser_id: String,
    },
    #[serde(rename_all = "camelCase")]
    BrowserDriverRelease {
        request_id: String,
        lease_epoch: String,
    },
    #[serde(rename_all = "camelCase")]
    BrowserCommand {
        request_id: String,
        request_seq: String,
        browser_id: String,
        lease_epoch: String,
        browser_instance_id: String,
        desktop_epoch: String,
        document_generation: String,
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        params: Option<serde_json::Value>,
    },
    #[serde(rename_all = "camelCase")]
    BrowserUnsubscribe {
        request_id: String,
        subscription_id: String,
    },
    /// §4.1 R4-9: View-accessible snapshot request. The response is
    /// `ServerMessage::BrowserSnapshot` carrying a public reference catalogue.
    #[serde(rename_all = "camelCase")]
    BrowserSnapshot {
        request_id: String,
        browser_id: String,
    },
    /// §4.3 R4-10: pause/resume are stream-scoped and carry the same identity
    /// pair the client sends (`browserId` / `streamId`).
    #[serde(rename_all = "camelCase")]
    BrowserPause { browser_id: String, stream_id: u32 },
    #[serde(rename_all = "camelCase")]
    BrowserResume { browser_id: String, stream_id: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum ServerMessage {
    #[serde(rename_all = "camelCase")]
    BrowserHello {
        browser_id: String,
        browser_instance_id: String,
        browser_service_epoch: String,
        desktop_epoch: String,
        protocol_version: u32,
        supported_commands: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capabilities: Option<serde_json::Value>,
    },
    #[serde(rename_all = "camelCase")]
    BrowserSubscribed {
        request_id: String,
        subscription_id: String,
        stream_id: u32,
        browser_id: String,
        browser_instance_id: String,
        browser_service_epoch: String,
        desktop_epoch: String,
        document_generation: String,
        options: BrowserSubscribeOptions,
    },
    #[serde(rename_all = "camelCase")]
    BrowserPong {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timestamp: Option<f64>,
    },
    #[serde(rename_all = "camelCase")]
    BrowserDriverClaimed {
        request_id: String,
        lease_epoch: String,
        expires_at: f64,
    },
    #[serde(rename_all = "camelCase")]
    BrowserDriverChanged {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lease_epoch: Option<String>,
        is_driver: bool,
    },
    #[serde(rename_all = "camelCase")]
    BrowserDriverReleased {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lease_epoch: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    BrowserDriverRevoked {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lease_epoch: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    BrowserResult {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
    },
    #[serde(rename_all = "camelCase")]
    BrowserError {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        code: String,
        message: String,
        retryable: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_ms: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    BrowserState {
        browser_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        document_generation: String,
        viewport_revision: String,
        loading: bool,
        paused: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pause_reason: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    BrowserUnsubscribed {
        request_id: String,
        subscription_id: String,
    },
    /// §4.1 R4-9: snapshot response. `map_revision` is a u64 decimal string and
    /// `elements` is the public reference catalogue (never a bare count).
    #[serde(rename_all = "camelCase")]
    BrowserSnapshot {
        request_id: String,
        snapshot_id: String,
        map_revision: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        root: Option<serde_json::Value>,
        #[serde(default)]
        elements: Vec<serde_json::Value>,
    },
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn sample_metadata() -> BrowserFrameMetadata {
        BrowserFrameMetadata {
            offset_top: 0.0,
            page_scale_factor: 1.0,
            device_width: 800.0,
            device_height: 600.0,
            image_width: 1,
            image_height: 1,
            scroll_offset_x: 0.0,
            scroll_offset_y: 0.0,
            timestamp: 1726560000.0,
            stream_id: 42,
            browser_instance_id: "inst-1".into(),
            browser_service_epoch: "100".into(),
            desktop_epoch: "200".into(),
            document_generation: "300".into(),
            viewport_revision: "400".into(),
            capture_rect: BrowserCaptureRect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            geometry_source: "wkSnapshot".into(),
        }
    }

    // Minimal valid 1x1 PNG bytes
    pub fn sample_1x1_png() -> &'static [u8] {
        &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG magic
            0x00, 0x00, 0x00, 0x0D, // IHDR length = 13
            0x49, 0x48, 0x44, 0x52, // "IHDR"
            0x00, 0x00, 0x00, 0x01, // width = 1
            0x00, 0x00, 0x00, 0x01, // height = 1
            0x08, 0x06, 0x00, 0x00, 0x00, // bit depth 8, truecolor+alpha
            0x1F, 0x15, 0xC4, 0x89, // CRC
            0x00, 0x00, 0x00, 0x0A, // IDAT length = 10
            0x49, 0x44, 0x41, 0x54, // "IDAT"
            0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, // zlib stream
            0x0D, 0x0A, 0x2D, 0xB4, // CRC
            0x00, 0x00, 0x00, 0x00, // IEND length = 0
            0x49, 0x45, 0x4E, 0x44, // "IEND"
            0xAE, 0x42, 0x60, 0x82, // CRC
        ]
    }

    #[test]
    fn test_golden_16b_binary_envelope_roundtrip() {
        let meta = sample_metadata();
        let png = sample_1x1_png();
        let encoded = encode_binary_frame(BrowserImageFormat::Png, 7, &meta, png).unwrap();

        // Exact 16B envelope check
        assert_eq!(encoded[0], 0x62, "kind offset 0");
        assert_eq!(encoded[1], 1, "version offset 1");
        assert_eq!(encoded[2], 1, "opcode offset 2");
        assert_eq!(encoded[3], 2, "format PNG offset 3");
        assert_eq!(
            u32::from_le_bytes(encoded[4..8].try_into().unwrap()),
            7,
            "seq offset 4"
        );
        let _meta_len = u32::from_le_bytes(encoded[8..12].try_into().unwrap()) as usize;
        assert_eq!(
            u32::from_le_bytes(encoded[12..16].try_into().unwrap()),
            0,
            "reserved offset 12"
        );

        let decoded = decode_binary_frame(&encoded).unwrap();
        assert_eq!(decoded.seq, 7);
        assert_eq!(decoded.format, BrowserImageFormat::Png);
        assert_eq!(decoded.metadata.stream_id, 42);
        assert_eq!(decoded.image_bytes, png);
    }

    #[test]
    fn test_binary_envelope_validation_rejects_corrupted_header() {
        let bytes = vec![0u8; 15]; // Too short
        assert!(decode_binary_frame(&bytes).is_err());

        // Header with bad kind
        let mut header = [0u8; 16];
        header[0] = 0x63; // Bad kind != 0x62
        header[1] = 1;
        header[2] = 1;
        header[3] = 1;
        assert!(decode_binary_frame(&header).is_err());

        // Header with bad version
        header[0] = 0x62;
        header[1] = 2; // Bad version != 1
        assert!(decode_binary_frame(&header).is_err());

        // Header with bad opcode
        header[1] = 1;
        header[2] = 2; // Bad opcode != 1
        assert!(decode_binary_frame(&header).is_err());

        // Header with bad format
        header[2] = 1;
        header[3] = 3; // Bad format != 1 or 2
        assert!(decode_binary_frame(&header).is_err());

        // Reserved != 0
        header[3] = 1;
        header[12] = 1; // Reserved non-zero
        assert!(decode_binary_frame(&header).is_err());
    }

    #[test]
    fn test_metadata_dimension_and_edge_limits() {
        let mut meta = sample_metadata();
        let png = sample_1x1_png();

        // Edge > 2048px rejected
        meta.image_width = 2049;
        assert!(matches!(
            encode_binary_frame(BrowserImageFormat::Png, 1, &meta, png),
            Err(ProtocolCodecError::ImageEdgeExceeded { width: 2049, .. })
        ));

        // Pixels > 4MP rejected
        meta.image_width = 2000;
        meta.image_height = 2001; // 4,002,000 > 4,000,000
        assert!(matches!(
            encode_binary_frame(BrowserImageFormat::Png, 1, &meta, png),
            Err(ProtocolCodecError::ImagePixelsExceeded { .. })
        ));

        // Non-finite values rejected
        meta.image_width = 1;
        meta.image_height = 1;
        meta.offset_top = f64::NAN;
        assert!(matches!(
            encode_binary_frame(BrowserImageFormat::Png, 1, &meta, png),
            Err(ProtocolCodecError::NonFiniteNumeric("offset_top"))
        ));

        meta.offset_top = 0.0;
        meta.geometry_source = "invalid".into();
        assert!(matches!(
            encode_binary_frame(BrowserImageFormat::Png, 1, &meta, png),
            Err(ProtocolCodecError::InvalidGeometrySource(_))
        ));
    }

    #[test]
    fn test_image_header_dimension_mismatch() {
        let mut meta = sample_metadata();
        let png = sample_1x1_png();
        // metadata claims 2x2, but png header is 1x1
        meta.image_width = 2;
        meta.image_height = 2;

        let meta_json = serde_json::to_vec(&meta).unwrap();
        let mut raw = Vec::new();
        raw.push(PROTOCOL_KIND);
        raw.push(PROTOCOL_VERSION);
        raw.push(OPCODE_FRAME);
        raw.push(FORMAT_PNG);
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.extend_from_slice(&(meta_json.len() as u32).to_le_bytes());
        raw.extend_from_slice(&0u32.to_le_bytes());
        raw.extend_from_slice(&meta_json);
        raw.extend_from_slice(png);

        assert!(matches!(
            decode_binary_frame(&raw),
            Err(ProtocolCodecError::DimensionMismatch {
                header_w: 1,
                header_h: 1,
                meta_w: 2,
                meta_h: 2,
            })
        ));
    }

    #[test]
    fn test_json_messages_camel_case_and_deny_unknown_fields() {
        let client_json = r#"{"type":"browserSubscribe","requestId":"r1","viewerInstanceId":"v1","options":{"format":"jpeg","quality":70}}"#;
        let parsed: ClientMessage = serde_json::from_str(client_json).unwrap();
        match parsed {
            ClientMessage::BrowserSubscribe {
                request_id,
                viewer_instance_id,
                options,
            } => {
                assert_eq!(request_id, "r1");
                assert_eq!(viewer_instance_id, "v1");
                assert_eq!(options.format, BrowserImageFormat::Jpeg);
                assert_eq!(options.quality, Some(70));
            }
            _ => panic!("Expected BrowserSubscribe"),
        }

        // Unknown field must be denied
        let client_unknown_field = r#"{"type":"browserSubscribe","requestId":"r1","viewerInstanceId":"v1","options":{"format":"jpeg"},"unknownField":"bad"}"#;
        assert!(serde_json::from_str::<ClientMessage>(client_unknown_field).is_err());

        // Server message test
        let server_json = r#"{"type":"browserHello","browserId":"b1","browserInstanceId":"bi1","browserServiceEpoch":"1","desktopEpoch":"2","protocolVersion":1,"supportedCommands":["click","fill"]}"#;
        let server_parsed: ServerMessage = serde_json::from_str(server_json).unwrap();
        match server_parsed {
            ServerMessage::BrowserHello {
                browser_id,
                supported_commands,
                ..
            } => {
                assert_eq!(browser_id, "b1");
                assert_eq!(supported_commands, vec!["click", "fill"]);
            }
            _ => panic!("Expected BrowserHello"),
        }
    }

    // R4-10: pause/resume wire schema must match the client (browserId/streamId).
    #[test]
    fn test_r4_10_pause_resume_wire_schema_matches_client() {
        let pause: ClientMessage =
            serde_json::from_str(r#"{"type":"browserPause","browserId":"b1","streamId":7}"#)
                .expect("browserPause with browserId/streamId must parse");
        assert_eq!(
            serde_json::to_value(&pause).unwrap(),
            serde_json::json!({ "type": "browserPause", "browserId": "b1", "streamId": 7 })
        );

        let resume: ClientMessage =
            serde_json::from_str(r#"{"type":"browserResume","browserId":"b1","streamId":7}"#)
                .expect("browserResume with browserId/streamId must parse");
        assert_eq!(
            serde_json::to_value(&resume).unwrap(),
            serde_json::json!({ "type": "browserResume", "browserId": "b1", "streamId": 7 })
        );

        // Legacy requestId/subscriptionId shape is no longer part of the contract
        assert!(serde_json::from_str::<ClientMessage>(
            r#"{"type":"browserPause","requestId":"p1","subscriptionId":"sub1"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<ClientMessage>(
            r#"{"type":"browserResume","browserId":"b1","streamId":7,"extra":1}"#
        )
        .is_err());
    }

    // R4-9: browserSnapshot must exist on both directions of the Rust wire contract.
    #[test]
    fn test_r4_9_browser_snapshot_wire_variants() {
        let client: ClientMessage = serde_json::from_str(
            r#"{"type":"browserSnapshot","requestId":"r1","browserId":"b1"}"#,
        )
        .expect("browserSnapshot client message must parse");
        assert_eq!(
            serde_json::to_value(&client).unwrap(),
            serde_json::json!({ "type": "browserSnapshot", "requestId": "r1", "browserId": "b1" })
        );

        let server: ServerMessage = serde_json::from_str(
            r#"{"type":"browserSnapshot","requestId":"r1","snapshotId":"snap-1","mapRevision":"12","elements":[{"ref":"e1"}]}"#,
        )
        .expect("browserSnapshot server message must parse");
        assert_eq!(
            serde_json::to_value(&server).unwrap(),
            serde_json::json!({
                "type": "browserSnapshot",
                "requestId": "r1",
                "snapshotId": "snap-1",
                "mapRevision": "12",
                "elements": [{ "ref": "e1" }]
            })
        );
    }
}
