use crate::{
    codec::{checked_bytes, push_i32, push_u16, push_u32, usize_to_u16, Decoder},
    Capabilities, ClipboardImageChunk, CodecError, GuestPermissions, HostStatus, PairingTrustGrant,
    PairingTrustRequest, RttProbe, SecureAttentionRequest, SecureAttentionResponse, SessionEnded,
    WireCodec,
};

pub const MAX_CLIPBOARD_TEXT_BYTES: usize = 4096;
pub const MAX_HOST_DISPLAYS: usize = 16;
pub const MAX_HOST_DISPLAY_NAME_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ControlMessageType {
    RequestKeyFrame = 0,
    StartStream = 1,
    StopStream = 2,
    Disconnect = 3,
    Ping = 4,
    Pong = 5,
    BitrateAdjust = 6,
    StreamConfigRequest = 7,
    StreamConfigResponse = 8,
    StreamConfigReject = 9,
    StreamConfigError = 10,
    ClipboardSyncRequest = 11,
    ClipboardSyncUpdate = 12,
    ClipboardSyncError = 13,
    InputAck = 14,
    StreamOptionsRequest = 15,
    StreamOptionsActive = 16,
    HostDisplayList = 17,
    HostPolicy = 18,
    SecureAttentionRequest = 19,
    SecureAttentionResponse = 20,
    HostStatus = 21,
    SessionEnded = 22,
    GuestPermissions = 23,
    PairingTrustRequest = 24,
    PairingTrustGrant = 25,
    ClipboardImageChunk = 26,
    RttPing = 27,
    RttPong = 28,
    CursorShape = 29, // c3-cursor
    ClockPing = 30,
    ClockPong = 31,
}

impl ControlMessageType {
    pub const ALL: [Self; 32] = [
        Self::RequestKeyFrame,
        Self::StartStream,
        Self::StopStream,
        Self::Disconnect,
        Self::Ping,
        Self::Pong,
        Self::BitrateAdjust,
        Self::StreamConfigRequest,
        Self::StreamConfigResponse,
        Self::StreamConfigReject,
        Self::StreamConfigError,
        Self::ClipboardSyncRequest,
        Self::ClipboardSyncUpdate,
        Self::ClipboardSyncError,
        Self::InputAck,
        Self::StreamOptionsRequest,
        Self::StreamOptionsActive,
        Self::HostDisplayList,
        Self::HostPolicy,
        Self::SecureAttentionRequest,
        Self::SecureAttentionResponse,
        Self::HostStatus,
        Self::SessionEnded,
        Self::GuestPermissions,
        Self::PairingTrustRequest,
        Self::PairingTrustGrant,
        Self::ClipboardImageChunk,
        Self::RttPing,
        Self::RttPong,
        Self::CursorShape,
        Self::ClockPing,
        Self::ClockPong,
    ];

    /// Additional capability required on both peers for parity extensions.
    /// Legacy message negotiation remains governed by its existing contract.
    pub const fn required_capability(self) -> Option<Capabilities> {
        match self {
            Self::SecureAttentionRequest | Self::SecureAttentionResponse => {
                Some(Capabilities::SECURE_ATTENTION)
            }
            Self::HostStatus => Some(Capabilities::HOST_STATUS),
            Self::SessionEnded => Some(Capabilities::SESSION_ENDED),
            Self::GuestPermissions => Some(Capabilities::GUEST_PERMISSIONS),
            Self::PairingTrustRequest | Self::PairingTrustGrant => {
                Some(Capabilities::PAIRING_TRUST)
            }
            Self::ClipboardImageChunk => Some(Capabilities::CLIPBOARD_IMAGE),
            Self::CursorShape => Some(Capabilities::CURSOR_SHAPE),
            Self::ClockPing | Self::ClockPong => Some(Capabilities::CLOCK_CORRELATION),
            Self::RttPing | Self::RttPong => Some(Capabilities::RTT_PROBE),
            _ => None,
        }
    }

    /// Check before sending or accepting any new control message.
    pub const fn supported_by(self, local: Capabilities, peer: Capabilities) -> bool {
        match self.required_capability() {
            Some(required) => local.contains(required) && peer.contains(required),
            None => true,
        }
    }
}

impl TryFrom<u8> for ControlMessageType {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::RequestKeyFrame),
            1 => Ok(Self::StartStream),
            2 => Ok(Self::StopStream),
            3 => Ok(Self::Disconnect),
            4 => Ok(Self::Ping),
            5 => Ok(Self::Pong),
            6 => Ok(Self::BitrateAdjust),
            7 => Ok(Self::StreamConfigRequest),
            8 => Ok(Self::StreamConfigResponse),
            9 => Ok(Self::StreamConfigReject),
            10 => Ok(Self::StreamConfigError),
            11 => Ok(Self::ClipboardSyncRequest),
            12 => Ok(Self::ClipboardSyncUpdate),
            13 => Ok(Self::ClipboardSyncError),
            14 => Ok(Self::InputAck),
            15 => Ok(Self::StreamOptionsRequest),
            16 => Ok(Self::StreamOptionsActive),
            17 => Ok(Self::HostDisplayList),
            18 => Ok(Self::HostPolicy),
            19 => Ok(Self::SecureAttentionRequest),
            20 => Ok(Self::SecureAttentionResponse),
            21 => Ok(Self::HostStatus),
            22 => Ok(Self::SessionEnded),
            23 => Ok(Self::GuestPermissions),
            24 => Ok(Self::PairingTrustRequest),
            25 => Ok(Self::PairingTrustGrant),
            26 => Ok(Self::ClipboardImageChunk),
            27 => Ok(Self::RttPing),
            28 => Ok(Self::RttPong),
            29 => Ok(Self::CursorShape),
            30 => Ok(Self::ClockPing),
            31 => Ok(Self::ClockPong),
            unknown => Err(CodecError::UnknownControlMessageType(unknown)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitrateAdjust {
    pub target_bitrate: i32,
}

impl BitrateAdjust {
    pub const SIZE: usize = 4;
}

impl WireCodec for BitrateAdjust {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        push_i32(&mut output, self.target_bitrate);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let target_bitrate = decoder.i32("target bitrate")?;
        decoder.finish("bitrate adjustment")?;
        Ok(Self { target_bitrate })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputAckMessage {
    pub sequence: u32,
    pub success: bool,
    pub error_code: u8,
}

impl InputAckMessage {
    pub const SIZE: usize = 6;
}

impl WireCodec for InputAckMessage {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        push_u32(&mut output, self.sequence);
        output.push(self.success as u8);
        output.push(self.error_code);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let sequence = decoder.u32("input ack sequence")?;
        let success = decoder.u8("input ack success")? != 0;
        let error_code = decoder.u8("input ack error code")?;
        decoder.finish("input ack message")?;
        Ok(Self {
            sequence,
            success,
            error_code,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamConfiguration {
    pub width: u32,
    pub height: u32,
    pub bitrate: u32,
    pub frames_per_second: u16,
}

impl StreamConfiguration {
    pub const SIZE: usize = 14;

    fn encode_into(&self, output: &mut Vec<u8>) {
        push_u32(output, self.width);
        push_u32(output, self.height);
        push_u32(output, self.bitrate);
        push_u16(output, self.frames_per_second);
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            width: decoder.u32("stream width")?,
            height: decoder.u32("stream height")?,
            bitrate: decoder.u32("stream bitrate")?,
            frames_per_second: decoder.u16("stream frames per second")?,
        })
    }
}

impl WireCodec for StreamConfiguration {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        self.encode_into(&mut output);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish("stream configuration")?;
        Ok(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamConfigurationRequest {
    pub request_id: u32,
    pub desired: StreamConfiguration,
}

impl StreamConfigurationRequest {
    pub const SIZE: usize = 18;
}

impl WireCodec for StreamConfigurationRequest {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        push_u32(&mut output, self.request_id);
        self.desired.encode_into(&mut output);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let request_id = decoder.u32("stream request ID")?;
        let desired = StreamConfiguration::decode_from(&mut decoder)?;
        decoder.finish("stream configuration request")?;
        Ok(Self {
            request_id,
            desired,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamConfigurationResponse {
    pub request_id: u32,
    pub active: StreamConfiguration,
}

impl StreamConfigurationResponse {
    pub const SIZE: usize = 18;
}

impl WireCodec for StreamConfigurationResponse {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        push_u32(&mut output, self.request_id);
        self.active.encode_into(&mut output);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let request_id = decoder.u32("stream response ID")?;
        let active = StreamConfiguration::decode_from(&mut decoder)?;
        decoder.finish("stream configuration response")?;
        Ok(Self { request_id, active })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StreamConfigurationErrorCode {
    InvalidRequest = 0,
    UnsupportedDimensions = 1,
    UnsupportedBitrate = 2,
    UnsupportedFps = 3,
    RejectedByPeer = 4,
}

impl TryFrom<u8> for StreamConfigurationErrorCode {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::InvalidRequest),
            1 => Ok(Self::UnsupportedDimensions),
            2 => Ok(Self::UnsupportedBitrate),
            3 => Ok(Self::UnsupportedFps),
            4 => Ok(Self::RejectedByPeer),
            unknown => Err(CodecError::UnknownStreamConfigurationErrorCode(unknown)),
        }
    }
}

fn encode_stream_problem(
    request_id: u32,
    code: StreamConfigurationErrorCode,
    message: &str,
) -> Result<Vec<u8>, CodecError> {
    let message = checked_bytes(message, "stream configuration message", u16::MAX as usize)?;
    let mut output = Vec::with_capacity(7 + message.len());
    push_u32(&mut output, request_id);
    output.push(code as u8);
    push_u16(
        &mut output,
        usize_to_u16(message.len(), "stream configuration message")?,
    );
    output.extend_from_slice(message);
    Ok(output)
}

fn decode_stream_problem(
    input: &[u8],
    field: &'static str,
) -> Result<(u32, StreamConfigurationErrorCode, String), CodecError> {
    let mut decoder = Decoder::new(input);
    let request_id = decoder.u32("stream problem request ID")?;
    let code = StreamConfigurationErrorCode::try_from(decoder.u8("stream problem code")?)?;
    let message_length = decoder.u16("stream problem message length")? as usize;
    let message = decoder.utf8(message_length, "stream configuration message")?;
    decoder.finish(field)?;
    Ok((request_id, code, message))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamConfigurationReject {
    pub request_id: u32,
    pub reason: StreamConfigurationErrorCode,
    pub message: String,
}

impl WireCodec for StreamConfigurationReject {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        encode_stream_problem(self.request_id, self.reason, &self.message)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let (request_id, reason, message) =
            decode_stream_problem(input, "stream configuration reject")?;
        Ok(Self {
            request_id,
            reason,
            message,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamConfigurationError {
    pub request_id: u32,
    pub error_code: StreamConfigurationErrorCode,
    pub message: String,
}

impl WireCodec for StreamConfigurationError {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        encode_stream_problem(self.request_id, self.error_code, &self.message)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let (request_id, error_code, message) =
            decode_stream_problem(input, "stream configuration error")?;
        Ok(Self {
            request_id,
            error_code,
            message,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClipboardSyncDirection {
    HostToClient = 0,
    ClientToHost = 1,
    Bidirectional = 2,
}

impl TryFrom<u8> for ClipboardSyncDirection {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::HostToClient),
            1 => Ok(Self::ClientToHost),
            2 => Ok(Self::Bidirectional),
            unknown => Err(CodecError::UnknownClipboardDirection(unknown)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClipboardSyncOrigin {
    LocalPasteboard = 0,
    RemotePasteboard = 1,
    SyncedFromPeer = 2,
}

impl TryFrom<u8> for ClipboardSyncOrigin {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::LocalPasteboard),
            1 => Ok(Self::RemotePasteboard),
            2 => Ok(Self::SyncedFromPeer),
            unknown => Err(CodecError::UnknownClipboardOrigin(unknown)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardSyncRequest {
    pub request_id: u32,
    pub direction: ClipboardSyncDirection,
    pub origin: ClipboardSyncOrigin,
}

impl ClipboardSyncRequest {
    pub const SIZE: usize = 6;
}

impl WireCodec for ClipboardSyncRequest {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        push_u32(&mut output, self.request_id);
        output.push(self.direction as u8);
        output.push(self.origin as u8);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let request_id = decoder.u32("clipboard request ID")?;
        let direction = ClipboardSyncDirection::try_from(decoder.u8("clipboard direction")?)?;
        let origin = ClipboardSyncOrigin::try_from(decoder.u8("clipboard origin")?)?;
        decoder.finish("clipboard sync request")?;
        Ok(Self {
            request_id,
            direction,
            origin,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardSyncUpdate {
    pub request_id: u32,
    pub direction: ClipboardSyncDirection,
    pub origin: ClipboardSyncOrigin,
    pub text: String,
}

impl WireCodec for ClipboardSyncUpdate {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let text = checked_bytes(&self.text, "clipboard text", MAX_CLIPBOARD_TEXT_BYTES)?;
        let mut output = Vec::with_capacity(8 + text.len());
        push_u32(&mut output, self.request_id);
        output.push(self.direction as u8);
        output.push(self.origin as u8);
        push_u16(&mut output, usize_to_u16(text.len(), "clipboard text")?);
        output.extend_from_slice(text);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let request_id = decoder.u32("clipboard update ID")?;
        let direction = ClipboardSyncDirection::try_from(decoder.u8("clipboard direction")?)?;
        let origin = ClipboardSyncOrigin::try_from(decoder.u8("clipboard origin")?)?;
        let text_length = decoder.u16("clipboard text length")? as usize;
        if text_length > MAX_CLIPBOARD_TEXT_BYTES {
            return Err(CodecError::LengthLimit {
                field: "clipboard text",
                actual: text_length,
                max: MAX_CLIPBOARD_TEXT_BYTES,
            });
        }
        let text = decoder.utf8(text_length, "clipboard text")?;
        decoder.finish("clipboard sync update")?;
        Ok(Self {
            request_id,
            direction,
            origin,
            text,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardSyncError {
    pub request_id: u32,
    pub direction: ClipboardSyncDirection,
    pub origin: ClipboardSyncOrigin,
    pub error_code: u8,
    pub message: String,
}

impl WireCodec for ClipboardSyncError {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let message = checked_bytes(&self.message, "clipboard error message", u16::MAX as usize)?;
        let mut output = Vec::with_capacity(9 + message.len());
        push_u32(&mut output, self.request_id);
        output.push(self.direction as u8);
        output.push(self.origin as u8);
        output.push(self.error_code);
        push_u16(
            &mut output,
            usize_to_u16(message.len(), "clipboard error message")?,
        );
        output.extend_from_slice(message);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let request_id = decoder.u32("clipboard error ID")?;
        let direction = ClipboardSyncDirection::try_from(decoder.u8("clipboard direction")?)?;
        let origin = ClipboardSyncOrigin::try_from(decoder.u8("clipboard origin")?)?;
        let error_code = decoder.u8("clipboard error code")?;
        let message_length = decoder.u16("clipboard error message length")? as usize;
        let message = decoder.utf8(message_length, "clipboard error message")?;
        decoder.finish("clipboard sync error")?;
        Ok(Self {
            request_id,
            direction,
            origin,
            error_code,
            message,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum StreamCodecPreference {
    #[default]
    Auto = 0,
    H264 = 1,
    Hevc = 2,
}

impl TryFrom<u8> for StreamCodecPreference {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Auto),
            1 => Ok(Self::H264),
            2 => Ok(Self::Hevc),
            unknown => Err(CodecError::InvalidValue {
                field: "stream codec",
                value: unknown as u64,
            }),
        }
    }
}

fn decode_bool(decoder: &mut Decoder<'_>, field: &'static str) -> Result<bool, CodecError> {
    match decoder.u8(field)? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(CodecError::InvalidValue {
            field,
            value: value as u64,
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamOptionsRequest {
    pub request_id: u32,
    pub scale_permille: u16,
    pub fps: u16,
    pub codec: StreamCodecPreference,
    pub display_index: u8,
    pub bitrate: u32,
}

impl StreamOptionsRequest {
    pub const SIZE: usize = 14;
}

impl WireCodec for StreamOptionsRequest {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        push_u32(&mut output, self.request_id);
        push_u16(&mut output, self.scale_permille);
        push_u16(&mut output, self.fps);
        output.push(self.codec as u8);
        output.push(self.display_index);
        push_u32(&mut output, self.bitrate);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let request_id = decoder.u32("stream options request id")?;
        let scale_permille = decoder.u16("stream options scale permille")?;
        let fps = decoder.u16("stream options fps")?;
        let codec = StreamCodecPreference::try_from(decoder.u8("stream codec")?)?;
        let display_index = decoder.u8("stream options display index")?;
        let bitrate = decoder.u32("stream options bitrate")?;
        decoder.finish("stream options request")?;
        Ok(Self {
            request_id,
            scale_permille,
            fps,
            codec,
            display_index,
            bitrate,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamOptionsActive {
    pub request_id: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u16,
    pub codec: StreamCodecPreference,
    pub display_index: u8,
    pub bitrate: u32,
}

impl StreamOptionsActive {
    pub const SIZE: usize = 20;
}

impl WireCodec for StreamOptionsActive {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        push_u32(&mut output, self.request_id);
        push_u32(&mut output, self.width);
        push_u32(&mut output, self.height);
        push_u16(&mut output, self.fps);
        output.push(self.codec as u8);
        output.push(self.display_index);
        push_u32(&mut output, self.bitrate);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let request_id = decoder.u32("stream options active request id")?;
        let width = decoder.u32("stream options active width")?;
        let height = decoder.u32("stream options active height")?;
        let fps = decoder.u16("stream options active fps")?;
        let codec = StreamCodecPreference::try_from(decoder.u8("stream codec")?)?;
        let display_index = decoder.u8("stream options active display index")?;
        let bitrate = decoder.u32("stream options active bitrate")?;
        decoder.finish("stream options active")?;
        Ok(Self {
            request_id,
            width,
            height,
            fps,
            codec,
            display_index,
            bitrate,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostDisplay {
    pub index: u8,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostDisplayList {
    pub displays: Vec<HostDisplay>,
}

impl WireCodec for HostDisplayList {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        if self.displays.len() > MAX_HOST_DISPLAYS {
            return Err(CodecError::LengthLimit {
                field: "host display count",
                actual: self.displays.len(),
                max: MAX_HOST_DISPLAYS,
            });
        }
        let mut total_size = 1;
        for display in &self.displays {
            let name_bytes = display.name.as_bytes();
            if name_bytes.len() > MAX_HOST_DISPLAY_NAME_BYTES {
                return Err(CodecError::LengthLimit {
                    field: "host display name",
                    actual: name_bytes.len(),
                    max: MAX_HOST_DISPLAY_NAME_BYTES,
                });
            }
            total_size += 1 + 4 + 4 + 1 + 1 + name_bytes.len();
        }

        let mut output = Vec::with_capacity(total_size);
        output.push(self.displays.len() as u8);
        for display in &self.displays {
            let name_bytes = display.name.as_bytes();
            output.push(display.index);
            push_u32(&mut output, display.width);
            push_u32(&mut output, display.height);
            output.push(display.primary as u8);
            output.push(name_bytes.len() as u8);
            output.extend_from_slice(name_bytes);
        }
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let count = decoder.u8("host display count")? as usize;
        if count > MAX_HOST_DISPLAYS {
            return Err(CodecError::LengthLimit {
                field: "host display count",
                actual: count,
                max: MAX_HOST_DISPLAYS,
            });
        }

        let mut displays = Vec::with_capacity(count);
        for _ in 0..count {
            let index = decoder.u8("host display index")?;
            let width = decoder.u32("host display width")?;
            let height = decoder.u32("host display height")?;
            let primary = decode_bool(&mut decoder, "host display primary")?;
            let name_len = decoder.u8("host display name length")? as usize;
            if name_len > MAX_HOST_DISPLAY_NAME_BYTES {
                return Err(CodecError::LengthLimit {
                    field: "host display name",
                    actual: name_len,
                    max: MAX_HOST_DISPLAY_NAME_BYTES,
                });
            }
            let name = decoder.utf8(name_len, "host display name")?;
            displays.push(HostDisplay {
                index,
                width,
                height,
                primary,
                name,
            });
        }
        decoder.finish("host display list")?;
        Ok(Self { displays })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostPolicy {
    pub allow_input: bool,
    pub allow_clipboard: bool,
}

impl HostPolicy {
    pub const SIZE: usize = 2;
}

impl WireCodec for HostPolicy {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        output.push(self.allow_input as u8);
        output.push(self.allow_clipboard as u8);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let allow_input = decode_bool(&mut decoder, "host policy allow_input")?;
        let allow_clipboard = decode_bool(&mut decoder, "host policy allow_clipboard")?;
        decoder.finish("host policy")?;
        Ok(Self {
            allow_input,
            allow_clipboard,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlMessage {
    RequestKeyFrame,
    StartStream,
    StopStream,
    Disconnect,
    Ping,
    Pong,
    BitrateAdjust(BitrateAdjust),
    StreamConfigRequest(StreamConfigurationRequest),
    StreamConfigResponse(StreamConfigurationResponse),
    StreamConfigReject(StreamConfigurationReject),
    StreamConfigError(StreamConfigurationError),
    ClipboardSyncRequest(ClipboardSyncRequest),
    ClipboardSyncUpdate(ClipboardSyncUpdate),
    ClipboardSyncError(ClipboardSyncError),
    InputAck(InputAckMessage),
    StreamOptionsRequest(StreamOptionsRequest),
    StreamOptionsActive(StreamOptionsActive),
    HostDisplayList(HostDisplayList),
    HostPolicy(HostPolicy),
    SecureAttentionRequest(SecureAttentionRequest),
    SecureAttentionResponse(SecureAttentionResponse),
    HostStatus(HostStatus),
    SessionEnded(SessionEnded),
    GuestPermissions(GuestPermissions),
    PairingTrustRequest(PairingTrustRequest),
    PairingTrustGrant(PairingTrustGrant),
    ClipboardImageChunk(ClipboardImageChunk),
    RttPing(RttProbe),
    RttPong(RttProbe),
    CursorShape(crate::CursorShape),
    ClockPing(crate::ClockProbe),
    ClockPong(crate::ClockProbe),
}

impl ControlMessage {
    pub const fn message_type(&self) -> ControlMessageType {
        match self {
            Self::RequestKeyFrame => ControlMessageType::RequestKeyFrame,
            Self::StartStream => ControlMessageType::StartStream,
            Self::StopStream => ControlMessageType::StopStream,
            Self::Disconnect => ControlMessageType::Disconnect,
            Self::Ping => ControlMessageType::Ping,
            Self::Pong => ControlMessageType::Pong,
            Self::BitrateAdjust(_) => ControlMessageType::BitrateAdjust,
            Self::StreamConfigRequest(_) => ControlMessageType::StreamConfigRequest,
            Self::StreamConfigResponse(_) => ControlMessageType::StreamConfigResponse,
            Self::StreamConfigReject(_) => ControlMessageType::StreamConfigReject,
            Self::StreamConfigError(_) => ControlMessageType::StreamConfigError,
            Self::ClipboardSyncRequest(_) => ControlMessageType::ClipboardSyncRequest,
            Self::ClipboardSyncUpdate(_) => ControlMessageType::ClipboardSyncUpdate,
            Self::ClipboardSyncError(_) => ControlMessageType::ClipboardSyncError,
            Self::InputAck(_) => ControlMessageType::InputAck,
            Self::StreamOptionsRequest(_) => ControlMessageType::StreamOptionsRequest,
            Self::StreamOptionsActive(_) => ControlMessageType::StreamOptionsActive,
            Self::HostDisplayList(_) => ControlMessageType::HostDisplayList,
            Self::HostPolicy(_) => ControlMessageType::HostPolicy,
            Self::SecureAttentionRequest(_) => ControlMessageType::SecureAttentionRequest,
            Self::SecureAttentionResponse(_) => ControlMessageType::SecureAttentionResponse,
            Self::HostStatus(_) => ControlMessageType::HostStatus,
            Self::SessionEnded(_) => ControlMessageType::SessionEnded,
            Self::GuestPermissions(_) => ControlMessageType::GuestPermissions,
            Self::PairingTrustRequest(_) => ControlMessageType::PairingTrustRequest,
            Self::PairingTrustGrant(_) => ControlMessageType::PairingTrustGrant,
            Self::ClipboardImageChunk(_) => ControlMessageType::ClipboardImageChunk,
            Self::RttPing(_) => ControlMessageType::RttPing,
            Self::CursorShape(_) => ControlMessageType::CursorShape,
            Self::ClockPing(_) => ControlMessageType::ClockPing,
            Self::ClockPong(_) => ControlMessageType::ClockPong,
            Self::RttPong(_) => ControlMessageType::RttPong,
        }
    }
}

impl WireCodec for ControlMessage {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = vec![self.message_type() as u8];
        let body = match self {
            Self::RequestKeyFrame
            | Self::StartStream
            | Self::StopStream
            | Self::Disconnect
            | Self::Ping
            | Self::Pong => return Ok(output),
            Self::BitrateAdjust(value) => value.encode()?,
            Self::StreamConfigRequest(value) => value.encode()?,
            Self::StreamConfigResponse(value) => value.encode()?,
            Self::StreamConfigReject(value) => value.encode()?,
            Self::StreamConfigError(value) => value.encode()?,
            Self::ClipboardSyncRequest(value) => value.encode()?,
            Self::ClipboardSyncUpdate(value) => value.encode()?,
            Self::ClipboardSyncError(value) => value.encode()?,
            Self::InputAck(value) => value.encode()?,
            Self::StreamOptionsRequest(value) => value.encode()?,
            Self::StreamOptionsActive(value) => value.encode()?,
            Self::HostDisplayList(value) => value.encode()?,
            Self::HostPolicy(value) => value.encode()?,
            Self::SecureAttentionRequest(value) => value.encode()?,
            Self::SecureAttentionResponse(value) => value.encode()?,
            Self::HostStatus(value) => value.encode()?,
            Self::SessionEnded(value) => value.encode()?,
            Self::GuestPermissions(value) => value.encode()?,
            Self::PairingTrustRequest(value) => value.encode()?,
            Self::PairingTrustGrant(value) => value.encode()?,
            Self::ClipboardImageChunk(value) => value.encode()?,
            Self::CursorShape(value) => value.encode()?,
            Self::ClockPing(value) | Self::ClockPong(value) => value.encode()?,
            Self::RttPing(value) | Self::RttPong(value) => value.encode()?,
        };
        output.extend_from_slice(&body);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let message_type = ControlMessageType::try_from(decoder.u8("control message type")?)?;
        let body = decoder.take_remaining();
        match message_type {
            ControlMessageType::RequestKeyFrame => {
                Decoder::new(body).finish("control message")?;
                Ok(Self::RequestKeyFrame)
            }
            ControlMessageType::StartStream => {
                Decoder::new(body).finish("control message")?;
                Ok(Self::StartStream)
            }
            ControlMessageType::StopStream => {
                Decoder::new(body).finish("control message")?;
                Ok(Self::StopStream)
            }
            ControlMessageType::Disconnect => {
                Decoder::new(body).finish("control message")?;
                Ok(Self::Disconnect)
            }
            ControlMessageType::Ping => {
                Decoder::new(body).finish("control message")?;
                Ok(Self::Ping)
            }
            ControlMessageType::Pong => {
                Decoder::new(body).finish("control message")?;
                Ok(Self::Pong)
            }
            ControlMessageType::BitrateAdjust => {
                Ok(Self::BitrateAdjust(BitrateAdjust::decode(body)?))
            }
            ControlMessageType::StreamConfigRequest => Ok(Self::StreamConfigRequest(
                StreamConfigurationRequest::decode(body)?,
            )),
            ControlMessageType::StreamConfigResponse => Ok(Self::StreamConfigResponse(
                StreamConfigurationResponse::decode(body)?,
            )),
            ControlMessageType::StreamConfigReject => Ok(Self::StreamConfigReject(
                StreamConfigurationReject::decode(body)?,
            )),
            ControlMessageType::StreamConfigError => Ok(Self::StreamConfigError(
                StreamConfigurationError::decode(body)?,
            )),
            ControlMessageType::ClipboardSyncRequest => Ok(Self::ClipboardSyncRequest(
                ClipboardSyncRequest::decode(body)?,
            )),
            ControlMessageType::ClipboardSyncUpdate => Ok(Self::ClipboardSyncUpdate(
                ClipboardSyncUpdate::decode(body)?,
            )),
            ControlMessageType::ClipboardSyncError => {
                Ok(Self::ClipboardSyncError(ClipboardSyncError::decode(body)?))
            }
            ControlMessageType::InputAck => Ok(Self::InputAck(InputAckMessage::decode(body)?)),
            ControlMessageType::StreamOptionsRequest => Ok(Self::StreamOptionsRequest(
                StreamOptionsRequest::decode(body)?,
            )),
            ControlMessageType::StreamOptionsActive => Ok(Self::StreamOptionsActive(
                StreamOptionsActive::decode(body)?,
            )),
            ControlMessageType::HostDisplayList => {
                Ok(Self::HostDisplayList(HostDisplayList::decode(body)?))
            }
            ControlMessageType::HostPolicy => Ok(Self::HostPolicy(HostPolicy::decode(body)?)),
            ControlMessageType::SecureAttentionRequest => Ok(Self::SecureAttentionRequest(
                SecureAttentionRequest::decode(body)?,
            )),
            ControlMessageType::SecureAttentionResponse => Ok(Self::SecureAttentionResponse(
                SecureAttentionResponse::decode(body)?,
            )),
            ControlMessageType::HostStatus => Ok(Self::HostStatus(HostStatus::decode(body)?)),
            ControlMessageType::SessionEnded => Ok(Self::SessionEnded(SessionEnded::decode(body)?)),
            ControlMessageType::GuestPermissions => {
                Ok(Self::GuestPermissions(GuestPermissions::decode(body)?))
            }
            ControlMessageType::PairingTrustRequest => Ok(Self::PairingTrustRequest(
                PairingTrustRequest::decode(body)?,
            )),
            ControlMessageType::PairingTrustGrant => {
                Ok(Self::PairingTrustGrant(PairingTrustGrant::decode(body)?))
            }
            ControlMessageType::ClipboardImageChunk => Ok(Self::ClipboardImageChunk(
                ClipboardImageChunk::decode(body)?,
            )),
            ControlMessageType::ClockPing => Ok(Self::ClockPing(crate::ClockProbe::decode(body)?)),
            ControlMessageType::ClockPong => Ok(Self::ClockPong(crate::ClockProbe::decode(body)?)),
            ControlMessageType::RttPing => Ok(Self::RttPing(RttProbe::decode(body)?)),
            ControlMessageType::CursorShape => {
                Ok(Self::CursorShape(crate::CursorShape::decode(body)?))
            }
            ControlMessageType::RttPong => Ok(Self::RttPong(RttProbe::decode(body)?)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_ack_message_roundtrip() {
        let original = InputAckMessage {
            sequence: 42,
            success: true,
            error_code: 0,
        };
        let encoded = original.encode().unwrap();
        assert_eq!(encoded.len(), InputAckMessage::SIZE);
        let decoded = InputAckMessage::decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn input_ack_message_with_error() {
        let original = InputAckMessage {
            sequence: 100,
            success: false,
            error_code: 5,
        };
        let encoded = original.encode().unwrap();
        assert_eq!(encoded.len(), InputAckMessage::SIZE);
        let decoded = InputAckMessage::decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn bodyless_control_message_rejects_trailing_bytes() {
        // A bodyless control value followed by an extra byte must not decode;
        // previously the suffix was silently discarded.
        let malformed = [ControlMessageType::StartStream as u8, 0x03];
        assert!(matches!(
            ControlMessage::decode(&malformed),
            Err(CodecError::TrailingBytes { .. })
        ));
        let ping_suffix = [ControlMessageType::Ping as u8, 0xff, 0x00];
        assert!(matches!(
            ControlMessage::decode(&ping_suffix),
            Err(CodecError::TrailingBytes { .. })
        ));
        // Exact-length bodyless messages still decode.
        assert_eq!(
            ControlMessage::decode(&[ControlMessageType::StartStream as u8]).unwrap(),
            ControlMessage::StartStream
        );
    }

    #[test]
    fn control_message_input_ack_roundtrip() {
        let ack = InputAckMessage {
            sequence: 123,
            success: true,
            error_code: 0,
        };
        let original = ControlMessage::InputAck(ack);
        let encoded = original.encode().unwrap();
        assert!(!encoded.is_empty());
        assert_eq!(encoded[0], ControlMessageType::InputAck as u8);
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn stream_options_request_roundtrip() {
        let msg = StreamOptionsRequest {
            request_id: 101,
            scale_permille: 750,
            fps: 60,
            codec: StreamCodecPreference::Hevc,
            display_index: 2,
            bitrate: 25_000_000,
        };
        let control = ControlMessage::StreamOptionsRequest(msg);
        let encoded = control.encode().unwrap();
        assert_eq!(encoded[0], ControlMessageType::StreamOptionsRequest as u8);
        assert_eq!(encoded.len(), 1 + StreamOptionsRequest::SIZE);
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(decoded, control);
    }

    #[test]
    fn stream_options_active_roundtrip() {
        let msg = StreamOptionsActive {
            request_id: 102,
            width: 3840,
            height: 1600,
            fps: 120,
            codec: StreamCodecPreference::H264,
            display_index: 1,
            bitrate: 50_000_000,
        };
        let control = ControlMessage::StreamOptionsActive(msg);
        let encoded = control.encode().unwrap();
        assert_eq!(encoded[0], ControlMessageType::StreamOptionsActive as u8);
        assert_eq!(encoded.len(), 1 + StreamOptionsActive::SIZE);
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(decoded, control);
    }

    #[test]
    fn host_display_list_roundtrip() {
        let msg = HostDisplayList {
            displays: vec![
                HostDisplay {
                    index: 0,
                    width: 3840,
                    height: 1600,
                    primary: true,
                    name: "Built-in Display".to_string(),
                },
                HostDisplay {
                    index: 1,
                    width: 1920,
                    height: 1080,
                    primary: false,
                    name: "Side Monitor".to_string(),
                },
            ],
        };
        let control = ControlMessage::HostDisplayList(msg);
        let encoded = control.encode().unwrap();
        assert_eq!(encoded[0], ControlMessageType::HostDisplayList as u8);
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(decoded, control);
    }

    #[test]
    fn host_policy_roundtrip() {
        let msg = HostPolicy {
            allow_input: true,
            allow_clipboard: false,
        };
        let control = ControlMessage::HostPolicy(msg);
        let encoded = control.encode().unwrap();
        assert_eq!(encoded[0], ControlMessageType::HostPolicy as u8);
        assert_eq!(encoded.len(), 1 + HostPolicy::SIZE);
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(decoded, control);
    }

    #[test]
    fn malformed_codec_byte_rejects_with_invalid_value() {
        // StreamOptionsRequest payload: request_id(4), scale(2), fps(2), codec(1), display_index(1), bitrate(4)
        let mut payload = vec![ControlMessageType::StreamOptionsRequest as u8];
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(&1000_u16.to_le_bytes());
        payload.extend_from_slice(&60_u16.to_le_bytes());
        payload.push(7); // Invalid codec
        payload.push(0);
        payload.extend_from_slice(&0_u32.to_le_bytes());

        let result = ControlMessage::decode(&payload);
        assert_eq!(
            result,
            Err(CodecError::InvalidValue {
                field: "stream codec",
                value: 7,
            })
        );
    }

    #[test]
    fn malformed_bool_byte_rejects_with_invalid_value() {
        // HostPolicy payload: allow_input(1), allow_clipboard(1)
        let payload = [
            ControlMessageType::HostPolicy as u8,
            2, // Invalid bool
            0,
        ];
        let result = ControlMessage::decode(&payload);
        assert_eq!(
            result,
            Err(CodecError::InvalidValue {
                field: "host policy allow_input",
                value: 2,
            })
        );

        // Also test display primary bool invalid value
        let mut display_payload = vec![ControlMessageType::HostDisplayList as u8, 1]; // count 1
        display_payload.push(0); // index
        display_payload.extend_from_slice(&1920_u32.to_le_bytes()); // width
        display_payload.extend_from_slice(&1080_u32.to_le_bytes()); // height
        display_payload.push(2); // primary bool = 2
        display_payload.push(4); // name_len
        display_payload.extend_from_slice(b"test");

        let display_result = ControlMessage::decode(&display_payload);
        assert_eq!(
            display_result,
            Err(CodecError::InvalidValue {
                field: "host display primary",
                value: 2,
            })
        );
    }

    #[test]
    fn malformed_display_count_rejects_with_length_limit() {
        let payload = [
            ControlMessageType::HostDisplayList as u8,
            17, // count 17 > MAX_HOST_DISPLAYS (16)
        ];
        let result = ControlMessage::decode(&payload);
        assert_eq!(
            result,
            Err(CodecError::LengthLimit {
                field: "host display count",
                actual: 17,
                max: 16,
            })
        );

        // Also test encoder rejection
        let too_many_displays = HostDisplayList {
            displays: (0..17)
                .map(|i| HostDisplay {
                    index: i,
                    width: 1920,
                    height: 1080,
                    primary: i == 0,
                    name: format!("Display {i}"),
                })
                .collect(),
        };
        assert_eq!(
            too_many_displays.encode(),
            Err(CodecError::LengthLimit {
                field: "host display count",
                actual: 17,
                max: 16,
            })
        );
    }

    #[test]
    fn malformed_name_len_rejects_with_length_limit() {
        let mut payload = vec![ControlMessageType::HostDisplayList as u8, 1];
        payload.push(0); // index
        payload.extend_from_slice(&1920_u32.to_le_bytes()); // width
        payload.extend_from_slice(&1080_u32.to_le_bytes()); // height
        payload.push(1); // primary
        payload.push(65); // name_len 65 > MAX_HOST_DISPLAY_NAME_BYTES (64)
        payload.extend_from_slice(&[b'a'; 65]);

        let result = ControlMessage::decode(&payload);
        assert_eq!(
            result,
            Err(CodecError::LengthLimit {
                field: "host display name",
                actual: 65,
                max: 64,
            })
        );

        // Also test encoder rejection
        let oversized_name = HostDisplayList {
            displays: vec![HostDisplay {
                index: 0,
                width: 1920,
                height: 1080,
                primary: true,
                name: "a".repeat(65),
            }],
        };
        assert_eq!(
            oversized_name.encode(),
            Err(CodecError::LengthLimit {
                field: "host display name",
                actual: 65,
                max: 64,
            })
        );
    }

    #[test]
    fn truncated_payload_rejects_with_truncated_error() {
        let mut short_req = vec![ControlMessageType::StreamOptionsRequest as u8];
        short_req.extend_from_slice(&1_u32.to_le_bytes()); // only 4 bytes instead of 14
        assert!(matches!(
            ControlMessage::decode(&short_req),
            Err(CodecError::Truncated { .. })
        ));

        let mut short_active = vec![ControlMessageType::StreamOptionsActive as u8];
        short_active.extend_from_slice(&1_u32.to_le_bytes());
        assert!(matches!(
            ControlMessage::decode(&short_active),
            Err(CodecError::Truncated { .. })
        ));

        let short_policy = [ControlMessageType::HostPolicy as u8, 1]; // only 1 byte instead of 2
        assert!(matches!(
            ControlMessage::decode(&short_policy),
            Err(CodecError::Truncated { .. })
        ));
    }

    #[test]
    fn trailing_byte_rejects_with_trailing_bytes_error() {
        let req = StreamOptionsRequest {
            request_id: 1,
            scale_permille: 1000,
            fps: 60,
            codec: StreamCodecPreference::Auto,
            display_index: 0,
            bitrate: 0,
        };
        let mut req_bytes = ControlMessage::StreamOptionsRequest(req).encode().unwrap();
        req_bytes.push(0xaa);
        assert_eq!(
            ControlMessage::decode(&req_bytes),
            Err(CodecError::TrailingBytes {
                field: "stream options request",
                remaining: 1,
            })
        );

        let active = StreamOptionsActive {
            request_id: 1,
            width: 1920,
            height: 1080,
            fps: 60,
            codec: StreamCodecPreference::Auto,
            display_index: 0,
            bitrate: 0,
        };
        let mut active_bytes = ControlMessage::StreamOptionsActive(active)
            .encode()
            .unwrap();
        active_bytes.push(0xbb);
        assert_eq!(
            ControlMessage::decode(&active_bytes),
            Err(CodecError::TrailingBytes {
                field: "stream options active",
                remaining: 1,
            })
        );

        let policy = HostPolicy {
            allow_input: true,
            allow_clipboard: true,
        };
        let mut policy_bytes = ControlMessage::HostPolicy(policy).encode().unwrap();
        policy_bytes.push(0xcc);
        assert_eq!(
            ControlMessage::decode(&policy_bytes),
            Err(CodecError::TrailingBytes {
                field: "host policy",
                remaining: 1,
            })
        );

        let list = HostDisplayList { displays: vec![] };
        let mut list_bytes = ControlMessage::HostDisplayList(list).encode().unwrap();
        list_bytes.push(0xdd);
        assert_eq!(
            ControlMessage::decode(&list_bytes),
            Err(CodecError::TrailingBytes {
                field: "host display list",
                remaining: 1,
            })
        );
    }
}
