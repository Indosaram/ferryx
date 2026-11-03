//! Media Foundation video encoding for the Windows host.
//!
//! The primary backend is a synchronous Media Foundation Transform (MFT). It
//! selects synchronous software transforms, negotiates HEVC first with H.264 fallback,
//! accepts NV12 frames, disables B-frames where the codec exposes `ICodecAPI`,
//! and applies supported dynamic ABR bitrate changes through
//! `CODECAPI_AVEncCommonMeanBitRate`.
//!
//! [`NvencAvailability`] is the alternative NVIDIA path: the `nvenc` crate
//! dynamically loads `nvEncodeAPI64.dll`, so deployments can select an NVENC
//! implementation without shipping or statically linking the SDK. The MFT path
//! remains the compatibility default and may itself resolve to NVIDIA's MFT.
//!
//! Output is normalized to MahoRD's AVCC contract: every NAL unit has a
//! four-byte big-endian length prefix, and keyframes include cached VPS/SPS/PPS
//! (or SPS/PPS for H.264) before the access unit.
//!
//! CI validates all Windows bindings. Runtime QA still requires real Intel,
//! AMD, and NVIDIA systems to exercise driver selection, format negotiation,
//! bitrate reconfiguration, keyframe requests, and sustained encode load.

use std::{marker::PhantomData, mem::ManuallyDrop, ptr, rc::Rc, slice, time::Instant};

use synchronous::{InputMetadata, PendingInputs, SynchronousTransform};

use crate::mf_encoder_policy::{cabac_setting, negotiate_h264_profile, H264Profile};
use crate::session::host_trace;
use thiserror::Error;
use windows::{
    core::{Interface, GUID},
    Win32::{
        Media::MediaFoundation::{
            eAVEncCommonRateControlMode_CBR, CODECAPI_AVEncCommonLowLatency,
            CODECAPI_AVEncCommonMeanBitRate, CODECAPI_AVEncCommonRateControlMode,
            CODECAPI_AVEncH264CABACEnable, CODECAPI_AVEncMPVDefaultBPictureCount,
            CODECAPI_AVEncMPVGOPSize, CODECAPI_AVEncNumWorkerThreads,
            CODECAPI_AVEncVideoForceKeyFrame, ICodecAPI, IMFActivate, IMFMediaBuffer, IMFMediaType,
            IMFSample, IMFTransform, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
            MFMediaType_Video, MFSampleExtension_CleanPoint, MFShutdown, MFStartup, MFTEnumEx,
            MFT_TRANSFORM_CLSID_Attribute, MFVideoFormat_H264, MFVideoFormat_HEVC,
            MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFSTARTUP_FULL,
            MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_SORTANDFILTER,
            MFT_ENUM_FLAG_SYNCMFT, MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_COMMAND_FLUSH,
            MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
            MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
            MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
            MFT_REGISTER_TYPE_INFO, MF_E_NOTACCEPTING, MF_E_TRANSFORM_NEED_MORE_INPUT,
            MF_E_TRANSFORM_STREAM_CHANGE, MF_LOW_LATENCY, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE,
            MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE,
            MF_MT_MPEG_SEQUENCE_HEADER, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_VERSION,
        },
        System::{
            Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED},
            Variant::VARIANT,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    Hevc,
    H264,
}

impl VideoCodec {
    pub(crate) fn media_subtype(self) -> GUID {
        match self {
            Self::Hevc => MFVideoFormat_HEVC,
            Self::H264 => MFVideoFormat_H264,
        }
    }

    fn parameter_set_types(self) -> &'static [u8] {
        match self {
            Self::Hevc => &[32, 33, 34],
            Self::H264 => &[7, 8],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderBackend {
    MediaFoundationHardware,
    MediaFoundationSoftware,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub bitrate: u32,
    pub fps: u32,
    pub keyframe_interval: u32,
    pub preferred_codec: VideoCodec,
}

impl EncoderConfig {
    pub(crate) fn validate(self) -> Result<Self, EncodeError> {
        if self.width == 0
            || self.height == 0
            || self.width % 2 != 0
            || self.height % 2 != 0
            || self.bitrate == 0
            || self.fps == 0
            || self.fps > 10_000_000
        {
            return Err(EncodeError::InvalidConfiguration);
        }
        Ok(self)
    }

    fn frame_duration_hns(self) -> i64 {
        10_000_000_i64 / i64::from(self.fps)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub is_key_frame: bool,
    pub timestamp_hns: i64,
    pub codec: VideoCodec,
    pub capture_at: Instant,
    pub encode_started_at: Instant,
    pub encode_completed_at: Instant,
}

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error(
        "encoder dimensions must be non-zero, even NV12 sizes, bitrate positive, and fps in 1..=10000000"
    )]
    InvalidConfiguration,
    #[error("input NV12 frame has the wrong length: expected {expected}, got {actual}")]
    InvalidFrameLength { expected: usize, actual: usize },
    #[error("no Media Foundation {0:?} encoder transform is available")]
    TransformUnavailable(VideoCodec),
    #[error("Media Foundation call failed: {0}")]
    MediaFoundation(#[from] windows::core::Error),
    #[error("Media Foundation returned an output sample without a buffer")]
    MissingOutput,
    #[error("encoded access unit is malformed: {0}")]
    MalformedBitstream(&'static str),
    #[error("output timestamp {0} does not identify an accepted input")]
    UnknownOutputTimestamp(i64),
    #[error("encoder input timestamp overflow")]
    TimestampOverflow,
    #[error("encoder does not expose dynamic bitrate control")]
    BitrateControlUnavailable,
    #[error("encoder drain ended with {0} inputs still pending")]
    IncompleteDrain(usize),
}

pub(crate) struct MediaFoundationRuntime {
    com_initialized: bool,
    // COM initialization/uninitialization must remain on the creating thread.
    _thread_affinity: PhantomData<Rc<()>>,
}

impl MediaFoundationRuntime {
    pub(crate) fn start() -> Result<Self, EncodeError> {
        let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let com_initialized = status.is_ok();
        // RPC_E_CHANGED_MODE means COM was initialized differently on this
        // thread. Media Foundation remains usable, but we must not uninitialize.
        if status.is_err() && status.0 != 0x8001_0106_u32 as i32 {
            return Err(windows::core::Error::from(status).into());
        }
        if let Err(error) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) } {
            if com_initialized {
                // SAFETY: this thread successfully initialized COM above.
                unsafe { CoUninitialize() };
            }
            return Err(error.into());
        }
        Ok(Self {
            com_initialized,
            _thread_affinity: PhantomData,
        })
    }
}

impl Drop for MediaFoundationRuntime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            if self.com_initialized {
                CoUninitialize();
            }
        }
    }
}

/// Availability probe for the optional dynamically loaded NVENC backend.
pub struct NvencAvailability;

impl NvencAvailability {
    pub fn probe() -> Result<u32, String> {
        let library = nvenc::nvenc_init().map_err(|error| error.to_string())?;
        library
            .get_max_version()
            .map_err(|error| format!("{error:?}"))
    }
}

pub struct MediaFoundationEncoder {
    transform: IMFTransform,
    output_type: IMFMediaType,
    codec_api: Option<ICodecAPI>,
    config: EncoderConfig,
    backend: EncoderBackend,
    codec: VideoCodec,
    frame_index: u64,
    force_keyframe: bool,
    first_keyframe_emitted: bool,
    parameter_sets: Vec<Vec<u8>>,
    pending_inputs: PendingInputs,
    // Rust drops fields in declaration order: all COM objects precede runtime.
    _runtime: MediaFoundationRuntime,
}

struct PreparedInput {
    sample: IMFSample,
    timestamp_hns: i64,
    metadata: InputMetadata,
    force_keyframe: bool,
}

impl MediaFoundationEncoder {
    pub fn new(config: EncoderConfig) -> Result<Self, EncodeError> {
        let config = config.validate()?;
        let runtime = MediaFoundationRuntime::start()?;
        let codecs = match config.preferred_codec {
            VideoCodec::Hevc => [VideoCodec::Hevc, VideoCodec::H264],
            VideoCodec::H264 => [VideoCodec::H264, VideoCodec::Hevc],
        };

        let mut last_error = None;
        for codec in codecs {
            match create_transform(codec) {
                Ok((transform, backend, clsid_id)) => {
                    return Self::configure(runtime, transform, backend, codec, clsid_id, config);
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or(EncodeError::TransformUnavailable(config.preferred_codec)))
    }

    fn configure(
        runtime: MediaFoundationRuntime,
        transform: IMFTransform,
        backend: EncoderBackend,
        codec: VideoCodec,
        clsid_id: u64,
        config: EncoderConfig,
    ) -> Result<Self, EncodeError> {
        if let Some(trace) = host_trace::enabled() {
            let backend_code = match backend {
                EncoderBackend::MediaFoundationSoftware => 1,
                EncoderBackend::MediaFoundationHardware => 2,
            };
            let codec_code = match codec {
                VideoCodec::Hevc => 1,
                VideoCodec::H264 => 2,
            };
            trace.record(host_trace::Record {
                event: 39,
                kind: backend_code,
                size: codec_code,
                value: clsid_id,
                ..Default::default()
            });
        }
        // Per Microsoft Media Foundation specification ("H.264 Video Encoder", MSDN):
        // "Before setting the media types on the encoder, configure the encoder properties
        // by using the ICodecAPI interface."
        let codec_api = transform.cast::<ICodecAPI>().ok();
        if let Some(api) = &codec_api {
            apply_codec_properties(api, config);
        }

        if let Ok(attributes) = unsafe { transform.GetAttributes() } {
            unsafe {
                let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
            }
        }

        // Encoders generally require output first so the desired profile is
        // known while enumerating supported input formats.
        let output_type = negotiate_output_type(&transform, codec_api.as_ref(), codec, config)?;
        let input_type = video_type(MFVideoFormat_NV12, config)?;
        unsafe {
            transform.SetInputType(0, &input_type, 0)?;
        }

        unsafe {
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        }
        let parameter_sets = media_type_parameter_sets(&output_type, codec);
        Ok(Self {
            _runtime: runtime,
            transform,
            output_type,
            codec_api,
            config,
            backend,
            codec,
            frame_index: 0,
            force_keyframe: true,
            first_keyframe_emitted: false,
            parameter_sets,
            pending_inputs: PendingInputs::default(),
        })
    }

    pub fn backend(&self) -> EncoderBackend {
        self.backend
    }

    pub fn codec(&self) -> VideoCodec {
        self.codec
    }

    /// Packed NV12 byte length this encoder accepts.
    pub fn frame_bytes(&self) -> usize {
        nv12_len(self.config.width, self.config.height)
            .expect("encoder config validated at construction")
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Rebuilds the transform when the desktop resolution changes, so a
    /// resized capture keeps the session alive instead of failing every
    /// subsequent frame. The fresh transform forces a keyframe so the client
    /// receives parameter sets for the new size.
    pub fn reconfigure(&mut self, width: u32, height: u32) -> Result<(), EncodeError> {
        if width == self.config.width && height == self.config.height {
            return Ok(());
        }
        let mut config = self.config;
        config.width = width;
        config.height = height;
        *self = MediaFoundationEncoder::new(config)?;
        Ok(())
    }

    /// Input is tightly packed NV12: Y plane followed by interleaved UV.
    pub fn encode_nv12(
        &mut self,
        nv12: &[u8],
        captured_at: Instant,
    ) -> Result<Vec<EncodedFrame>, EncodeError> {
        let encode_started_at = Instant::now();
        let expected = nv12_len(self.config.width, self.config.height)?;
        if nv12.len() != expected {
            return Err(EncodeError::InvalidFrameLength {
                expected,
                actual: nv12.len(),
            });
        }
        let timestamp = i64::try_from(self.frame_index)
            .ok()
            .and_then(|index| index.checked_mul(self.config.frame_duration_hns()))
            .ok_or(EncodeError::TimestampOverflow)?;
        let sample = {
            let trace = host_trace::enabled();
            let started = trace.map(|_| Instant::now());
            if let Some(trace) = trace {
                trace.record(host_trace::Record {
                    event: 25,
                    frame: self.frame_index,
                    size: nv12.len(),
                    ..Default::default()
                });
            }
            let sample_res = sample_from_bytes(nv12, timestamp, self.config.frame_duration_hns());
            if let (Some(trace), Some(started)) = (trace, started) {
                trace.record(host_trace::Record {
                    event: 26,
                    frame: self.frame_index,
                    value: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
                    size: nv12.len(),
                    repeat: sample_res.is_err(),
                    ..Default::default()
                });
            }
            sample_res?
        };
        let input = PreparedInput {
            sample,
            timestamp_hns: timestamp,
            metadata: InputMetadata {
                capture_at: captured_at,
                encode_started_at,
                force_keyframe: self.force_keyframe,
            },
            force_keyframe: self.force_keyframe,
        };
        synchronous::submit_and_drain(self, &input)
    }

    pub fn force_key_frame(&mut self) {
        // Reactive requests are limited once, in the session shared budget.
        self.force_keyframe = true;
    }

    /// Applies protocol ABR messages immediately when the selected MFT exposes
    /// dynamic bitrate control. MFTs without runtime control are rebuilt at the
    /// new target; the fresh encoder forces a keyframe so the client stays
    /// decodable across the switch.
    pub fn update_bitrate(&mut self, bitrate: u32) -> Result<(), EncodeError> {
        if bitrate == 0 {
            return Err(EncodeError::InvalidConfiguration);
        }
        if bitrate == self.config.bitrate {
            return Ok(());
        }
        if let Some(api) = &self.codec_api {
            let trace = host_trace::enabled();
            let started = trace.map(|_| Instant::now());
            if let Some(trace) = trace {
                trace.record(host_trace::Record {
                    event: 35,
                    value: u64::from(bitrate),
                    ..Default::default()
                });
            }
            // SAFETY: the API and UI4 variant live through the synchronous call.
            let res = apply_bitrate_properties(api, bitrate);
            if let (Some(trace), Some(started)) = (trace, started) {
                trace.record(host_trace::Record {
                    event: 36,
                    value: u64::from(bitrate),
                    size: usize::try_from(started.elapsed().as_micros()).unwrap_or(usize::MAX),
                    repeat: res.is_err(),
                    ..Default::default()
                });
            }
            match res {
                Ok(()) => {
                    self.config.bitrate = bitrate;
                    return Ok(());
                }
                Err(error) => {
                    tracing::warn!(%error, bitrate, "runtime bitrate control rejected; rebuilding encoder");
                }
            }
        }
        let trace = host_trace::enabled();
        let started = trace.map(|_| Instant::now());
        if let Some(trace) = trace {
            trace.record(host_trace::Record {
                event: 37,
                value: u64::from(bitrate),
                ..Default::default()
            });
        }
        let mut config = self.config;
        config.bitrate = bitrate;
        *self = MediaFoundationEncoder::new(config)?;
        if let (Some(trace), Some(started)) = (trace, started) {
            trace.record(host_trace::Record {
                event: 38,
                value: u64::from(bitrate),
                size: usize::try_from(started.elapsed().as_micros()).unwrap_or(usize::MAX),
                ..Default::default()
            });
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<Vec<EncodedFrame>, EncodeError> {
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)?;
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)?;
        }
        let mut frames = Vec::new();
        while let Some(frame) = self.take_output()? {
            frames.push(frame);
        }
        if !self.pending_inputs.is_empty() {
            return Err(EncodeError::IncompleteDrain(self.pending_inputs.len()));
        }
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)?
        };
        Ok(frames)
    }

    fn take_output(&mut self) -> Result<Option<EncodedFrame>, EncodeError> {
        let trace = host_trace::enabled();
        let started = trace.map(|_| Instant::now());
        if let Some(trace) = trace {
            trace.record(host_trace::Record {
                event: 33,
                frame: self.frame_index,
                ..Default::default()
            });
        }
        let stream_info = unsafe { self.transform.GetOutputStreamInfo(0)? };
        let transform_provides_sample = stream_info.dwFlags
            & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32)
            != 0;
        let sample = if transform_provides_sample {
            None
        } else {
            let sample = unsafe { MFCreateSample()? };
            let capacity = stream_info.cbSize.max(1);
            let buffer = unsafe { MFCreateMemoryBuffer(capacity)? };
            unsafe { sample.AddBuffer(&buffer)? };
            Some(sample)
        };
        let mut output = [MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        }];
        let mut status = 0;
        let result = unsafe { self.transform.ProcessOutput(0, &mut output, &mut status) };
        let output_sample = unsafe { ManuallyDrop::take(&mut output[0].pSample) };
        let _events = unsafe { ManuallyDrop::take(&mut output[0].pEvents) };
        match result {
            Ok(()) => {}
            Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                if let (Some(trace), Some(started)) = (trace, started) {
                    trace.record(host_trace::Record {
                        event: 34,
                        frame: self.frame_index,
                        value: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
                        size: 0,
                        ..Default::default()
                    });
                }
                return Ok(None);
            }
            Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                if let (Some(trace), Some(started)) = (trace, started) {
                    trace.record(host_trace::Record {
                        event: 34,
                        frame: self.frame_index,
                        value: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
                        size: 0,
                        repeat: true,
                        ..Default::default()
                    });
                }
                let new_type = unsafe { self.transform.GetOutputAvailableType(0, 0)? };
                // Media Foundation requires ICodecAPI properties to be applied
                // before the media type is set; afterwards it silently drops
                // them and the encoder reverts to buffered, B-frame defaults.
                if let Some(api) = &self.codec_api {
                    apply_codec_properties(api, self.config);
                }
                unsafe { self.transform.SetOutputType(0, &new_type, 0)? };
                self.output_type = new_type;
                self.parameter_sets = media_type_parameter_sets(&self.output_type, self.codec);
                return self.take_output();
            }
            Err(error) => {
                if let (Some(trace), Some(started)) = (trace, started) {
                    trace.record(host_trace::Record {
                        event: 34,
                        frame: self.frame_index,
                        value: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
                        size: 0,
                        repeat: true,
                        ..Default::default()
                    });
                }
                return Err(error.into());
            }
        }

        let sample = output_sample.ok_or(EncodeError::MissingOutput)?;
        // SAFETY: sample is an owned, successful ProcessOutput result.
        let timestamp_hns = unsafe { sample.GetSampleTime()? };
        let metadata = self
            .pending_inputs
            .take(timestamp_hns)
            .ok_or(EncodeError::UnknownOutputTimestamp(timestamp_hns))?;
        let sample_clean_point = unsafe {
            sample
                .GetUINT32(&MFSampleExtension_CleanPoint)
                .unwrap_or_default()
                != 0
        };
        // Keyframe status belongs to the input that produced this sample. A
        // backpressure drain can emit buffered inputs before the forced
        // keyframe, so a global flag alone would let the real keyframe lose
        // its parameter sets when the MFT never sets CleanPoint.
        let is_key_frame =
            sample_clean_point || metadata.force_keyframe || !self.first_keyframe_emitted;
        let bytes = sample_bytes(&sample)?;
        let mut nalus = parse_access_unit(&bytes)?;
        if is_key_frame {
            self.force_keyframe = false;
            self.first_keyframe_emitted = true;
            let fresh = parameter_sets(&nalus, self.codec);
            if !fresh.is_empty() {
                self.parameter_sets = fresh;
            }
            if !self.parameter_sets.is_empty() {
                let present_types: Vec<u8> = nalus
                    .iter()
                    .filter_map(|nalu| nalu_type(nalu, self.codec))
                    .collect();
                let mut prefixed = Vec::new();
                for parameter_set in &self.parameter_sets {
                    let Some(kind) = nalu_type(parameter_set, self.codec) else {
                        continue;
                    };
                    if !present_types.contains(&kind) {
                        prefixed.push(parameter_set.as_slice());
                    }
                }
                prefixed.append(&mut nalus);
                nalus = prefixed;
            }
        }

        let data = write_avcc(&nalus)?;
        if let (Some(trace), Some(started)) = (trace, started) {
            trace.record(host_trace::Record {
                event: 34,
                frame: self.frame_index,
                value: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
                size: data.len(),
                keyframe: is_key_frame,
                ..Default::default()
            });
        }

        Ok(Some(EncodedFrame {
            data,
            is_key_frame,
            timestamp_hns,
            codec: self.codec,
            capture_at: metadata.capture_at,
            encode_started_at: metadata.encode_started_at,
            encode_completed_at: Instant::now(),
        }))
    }
}

impl SynchronousTransform for MediaFoundationEncoder {
    type Input = PreparedInput;
    type Output = EncodedFrame;
    type Error = EncodeError;

    fn process_input(&mut self, input: &PreparedInput) -> Result<(), EncodeError> {
        let is_key = input.force_keyframe;
        let trace = host_trace::enabled();
        let started = trace.map(|_| Instant::now());
        if let Some(trace) = trace {
            trace.record(host_trace::Record {
                event: 27,
                frame: self.frame_index,
                keyframe: is_key,
                ..Default::default()
            });
        }
        if is_key {
            if let Some(api) = &self.codec_api {
                let value = VARIANT::from(1_u32);
                // SAFETY: keyframe is VT_UI4; reapply on the SAME rejected input.
                unsafe { api.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &value)? };
            }
        }
        // SAFETY: input owns a timestamped NV12 sample matching the negotiated type.
        let result = unsafe { self.transform.ProcessInput(0, &input.sample, 0) };
        if let (Some(trace), Some(started)) = (trace, started) {
            trace.record(host_trace::Record {
                event: 28,
                frame: self.frame_index,
                value: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
                keyframe: is_key,
                repeat: result.is_err(),
                ..Default::default()
            });
        }
        result?;
        if is_key {
            self.force_keyframe = false;
        }
        self.pending_inputs
            .accept(input.timestamp_hns, input.metadata);
        self.frame_index += 1;
        Ok(())
    }

    fn is_not_accepting(error: &EncodeError) -> bool {
        matches!(error, EncodeError::MediaFoundation(error) if error.code() == MF_E_NOTACCEPTING)
    }

    fn process_output(&mut self) -> Result<Option<EncodedFrame>, EncodeError> {
        self.take_output()
    }
}

impl Drop for MediaFoundationEncoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
        }
    }
}

fn create_transform(codec: VideoCodec) -> Result<(IMFTransform, EncoderBackend, u64), EncodeError> {
    // Synchronous MFTs first: the Microsoft Software H.264/HEVC encoders work
    // with the synchronous processInput/processMessage pipeline below. Hardware
    // MFTs (NVIDIA etc.) are asynchronous and require the event-driven
    // IMFMediaEventGenerator pipeline, which is not implemented yet — they are
    // deliberately skipped until that lands.
    let sync_flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);
    if let Some((transform, clsid_id)) = enumerate_transform(codec, sync_flags)? {
        return Ok((transform, EncoderBackend::MediaFoundationSoftware, clsid_id));
    }
    // Do NOT fall back to MFT_ENUM_FLAG_ALL: it surfaces asynchronous hardware
    // MFTs (NVIDIA etc.), which hang the synchronous ProcessInput/ProcessOutput
    // pipeline below and stall the whole media pipeline silently.
    Err(EncodeError::TransformUnavailable(codec))
}

fn enumerate_transform(
    codec: VideoCodec,
    flags: MFT_ENUM_FLAG,
) -> Result<Option<(IMFTransform, u64)>, EncodeError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: codec.media_subtype(),
    };
    let mut activations: *mut Option<IMFActivate> = ptr::null_mut();
    let mut count = 0;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            Some(&input),
            Some(&output),
            &mut activations,
            &mut count,
        )?;
    }
    if count == 0 || activations.is_null() {
        return Ok(None);
    }

    let mut selected = None;
    // Take ownership of every entry and release the CoTaskMemAlloc'd array
    // before doing anything fallible: an early return past `CoTaskMemFree`
    // leaks the array and keeps the remaining activation objects (and their
    // transform DLLs) referenced for the life of the process.
    let activation_objects: Vec<IMFActivate> = unsafe {
        let entries = slice::from_raw_parts_mut(activations, count as usize);
        let owned = entries
            .iter_mut()
            .filter_map(|entry| entry.take())
            .collect();
        CoTaskMemFree(Some(activations.cast()));
        owned
    };
    for activation in activation_objects {
        if selected.is_none() {
            let clsid_id = unsafe { activation.GetGUID(&MFT_TRANSFORM_CLSID_Attribute) }
                .map(|guid| {
                    let bytes = guid.to_u128().to_le_bytes();
                    u64::from_le_bytes(bytes[0..8].try_into().unwrap())
                })
                .unwrap_or(0);
            selected = Some((
                unsafe { activation.ActivateObject::<IMFTransform>() }?,
                clsid_id,
            ));
        }
    }
    Ok(selected)
}

pub(crate) fn video_type(
    subtype: GUID,
    config: EncoderConfig,
) -> Result<IMFMediaType, EncodeError> {
    let media_type = unsafe { MFCreateMediaType()? };
    unsafe {
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, &subtype)?;
        media_type.SetUINT64(
            &MF_MT_FRAME_SIZE,
            (u64::from(config.width) << 32) | u64::from(config.height),
        )?;
        media_type.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(config.fps) << 32) | 1)?;
        media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1_u64 << 32) | 1)?;
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        media_type.SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate)?;
        media_type.SetUINT32(&windows::Win32::Media::MediaFoundation::MF_MT_YUV_MATRIX, 2)?;
        media_type.SetUINT32(
            &windows::Win32::Media::MediaFoundation::MF_MT_VIDEO_NOMINAL_RANGE,
            1,
        )?;
    }
    Ok(media_type)
}

/// H.264 output type carrying an explicit `MF_MT_MPEG2_PROFILE`.
pub(crate) fn h264_output_type(
    config: EncoderConfig,
    profile: H264Profile,
) -> Result<IMFMediaType, EncodeError> {
    let media_type = video_type(MFVideoFormat_H264, config)?;
    // SAFETY: setting a scalar attribute on a media type this function owns.
    unsafe { media_type.SetUINT32(&MF_MT_MPEG2_PROFILE, profile.mf_value())? };
    Ok(media_type)
}

/// Sets the encoder output type. H.264 tries High then Main; before each
/// attempt CABAC is enabled when the encoder exposes the property (both
/// profiles allow it). HEVC uses the encoder's default (Main) profile.
pub(crate) fn negotiate_output_type(
    transform: &IMFTransform,
    codec_api: Option<&ICodecAPI>,
    codec: VideoCodec,
    config: EncoderConfig,
) -> Result<IMFMediaType, EncodeError> {
    if codec != VideoCodec::H264 {
        let output_type = video_type(codec.media_subtype(), config)?;
        // SAFETY: SetOutputType configures stream 0 before streaming starts.
        unsafe { transform.SetOutputType(0, &output_type, 0)? };
        return Ok(output_type);
    }
    // SAFETY: IsSupported only queries the property.
    let cabac_supported = codec_api
        .is_some_and(|api| unsafe { api.IsSupported(&CODECAPI_AVEncH264CABACEnable) }.is_ok());
    let negotiated = negotiate_h264_profile(|profile| {
        if let (Some(api), Some(enable)) = (codec_api, cabac_setting(cabac_supported, profile)) {
            set_codec_bool(api, &CODECAPI_AVEncH264CABACEnable, enable);
        }
        let output_type = h264_output_type(config, profile)?;
        // SAFETY: SetOutputType configures stream 0 before streaming starts.
        if let Err(error) = unsafe { transform.SetOutputType(0, &output_type, 0) } {
            tracing::debug!(?profile, %error, "H.264 profile rejected by encoder");
            return Err(EncodeError::from(error));
        }
        Ok(output_type)
    });
    match negotiated {
        Ok((profile, output_type)) => {
            tracing::info!(?profile, cabac_supported, "negotiated H.264 output profile");
            Ok(output_type)
        }
        Err(error) => {
            // Before explicit profiles were applied every MFT ran its default
            // profile; keep that working rather than losing H.264 entirely.
            tracing::warn!(%error, "High and Main rejected; using encoder default H.264 profile");
            let output_type = video_type(MFVideoFormat_H264, config)?;
            // SAFETY: SetOutputType configures stream 0 before streaming starts.
            unsafe { transform.SetOutputType(0, &output_type, 0)? };
            Ok(output_type)
        }
    }
}

pub(crate) fn apply_codec_properties(api: &ICodecAPI, config: EncoderConfig) {
    set_codec_bool(api, &CODECAPI_AVEncCommonLowLatency, true);
    set_codec_u32(
        api,
        &CODECAPI_AVEncCommonRateControlMode,
        eAVEncCommonRateControlMode_CBR.0 as u32,
    );
    if let Err(error) = apply_bitrate_properties(api, config.bitrate) {
        tracing::warn!(%error, "encoder bitrate configuration failed");
    }
    set_codec_u32(api, &CODECAPI_AVEncMPVDefaultBPictureCount, 0);
    set_codec_u32(
        api,
        &CODECAPI_AVEncMPVGOPSize,
        crate::windows_logic::encoder_gop_size(config.keyframe_interval),
    );
    set_codec_u32(api, &CODECAPI_AVEncNumWorkerThreads, 1);
}

/// Mean rate is mandatory; peak and VBV are optional driver capabilities.
/// VBV is specified in bits: a half-second budget bounds scene-change bursts.
pub(crate) fn apply_bitrate_properties(api: &ICodecAPI, bitrate: u32) -> Result<(), EncodeError> {
    use windows::Win32::Media::MediaFoundation::{
        CODECAPI_AVEncCommonBufferSize, CODECAPI_AVEncCommonMaxBitRate,
    };
    for (key, value) in [
        (&CODECAPI_AVEncCommonMaxBitRate, bitrate),
        (&CODECAPI_AVEncCommonBufferSize, (bitrate / 2).max(1)),
    ] {
        // SAFETY: querying and setting scalar codec properties on the owning thread.
        if unsafe { api.IsSupported(key) }.is_ok() {
            unsafe {
                api.SetValue(key, &VARIANT::from(value))?;
            }
        }
    }
    unsafe {
        api.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &VARIANT::from(bitrate))?;
    }
    Ok(())
}

fn set_codec_u32(api: &ICodecAPI, key: &GUID, value: u32) {
    let value = VARIANT::from(value);
    unsafe {
        if let Err(error) = api.SetValue(key, &value) {
            tracing::debug!(%error, ?key, "optional encoder property rejected");
        }
    }
}

fn set_codec_bool(api: &ICodecAPI, key: &GUID, value: bool) {
    let value = VARIANT::from(value);
    unsafe {
        if let Err(error) = api.SetValue(key, &value) {
            tracing::debug!(%error, ?key, "optional encoder property rejected");
        }
    }
}

pub(crate) fn nv12_len(width: u32, height: u32) -> Result<usize, EncodeError> {
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or(EncodeError::InvalidConfiguration)?;
    pixels
        .checked_add(pixels / 2)
        .ok_or(EncodeError::InvalidConfiguration)
}

fn sample_from_bytes(
    bytes: &[u8],
    timestamp: i64,
    duration: i64,
) -> Result<IMFSample, EncodeError> {
    let length = u32::try_from(bytes.len()).map_err(|_| EncodeError::InvalidConfiguration)?;
    let buffer = unsafe { MFCreateMemoryBuffer(length)? };
    let mut destination = ptr::null_mut();
    unsafe {
        buffer.Lock(&mut destination, None, None)?;
        ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
        if let Err(error) = buffer.Unlock() {
            return Err(error.into());
        }
        buffer.SetCurrentLength(length)?;
        let sample = MFCreateSample()?;
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(timestamp)?;
        sample.SetSampleDuration(duration)?;
        Ok(sample)
    }
}

pub(crate) fn sample_bytes(sample: &IMFSample) -> Result<Vec<u8>, EncodeError> {
    let buffer = unsafe { sample.ConvertToContiguousBuffer()? };
    copy_media_buffer(&buffer)
}

fn copy_media_buffer(buffer: &IMFMediaBuffer) -> Result<Vec<u8>, EncodeError> {
    let length = unsafe { buffer.GetCurrentLength()? } as usize;
    let mut pointer = ptr::null_mut();
    unsafe {
        buffer.Lock(&mut pointer, None, None)?;
        let bytes = slice::from_raw_parts(pointer, length).to_vec();
        buffer.Unlock()?;
        Ok(bytes)
    }
}

pub(crate) fn media_type_parameter_sets(
    media_type: &IMFMediaType,
    codec: VideoCodec,
) -> Vec<Vec<u8>> {
    let Ok(size) = (unsafe { media_type.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) }) else {
        return Vec::new();
    };
    if size == 0 {
        return Vec::new();
    }
    let mut bytes = vec![0_u8; size as usize];
    if unsafe { media_type.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut bytes, None) }.is_err() {
        return Vec::new();
    }
    parse_access_unit(&bytes)
        .map(|nalus| parameter_sets(&nalus, codec))
        .unwrap_or_default()
}

/// Accept either Annex B or four-byte length-prefixed MFT/NVENC output.
pub fn parse_access_unit(bytes: &[u8]) -> Result<Vec<&[u8]>, EncodeError> {
    if bytes.is_empty() {
        return Err(EncodeError::MalformedBitstream("empty access unit"));
    }
    if is_start_code_at(bytes, 0).is_some() {
        parse_annex_b(bytes)
    } else {
        parse_avcc(bytes)
    }
}

fn parse_avcc(bytes: &[u8]) -> Result<Vec<&[u8]>, EncodeError> {
    let mut offset = 0;
    let mut nalus = Vec::new();
    while offset < bytes.len() {
        if bytes.len() - offset < 4 {
            return Err(EncodeError::MalformedBitstream("truncated AVCC NAL length"));
        }
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if length == 0 || length > bytes.len() - offset {
            return Err(EncodeError::MalformedBitstream("invalid AVCC NAL length"));
        }
        nalus.push(&bytes[offset..offset + length]);
        offset += length;
    }
    Ok(nalus)
}

fn parse_annex_b(bytes: &[u8]) -> Result<Vec<&[u8]>, EncodeError> {
    let mut nalus = Vec::new();
    let mut cursor = 0;
    while let Some((start, prefix)) = find_start_code(bytes, cursor) {
        let nalu_start = start + prefix;
        let next = find_start_code(bytes, nalu_start).map_or(bytes.len(), |(index, _)| index);
        if next > nalu_start {
            nalus.push(&bytes[nalu_start..next]);
        }
        cursor = next;
        if cursor >= bytes.len() {
            break;
        }
    }
    if nalus.is_empty() {
        Err(EncodeError::MalformedBitstream("no Annex B NAL units"))
    } else {
        Ok(nalus)
    }
}

fn find_start_code(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    (from..bytes.len()).find_map(|index| is_start_code_at(bytes, index).map(|len| (index, len)))
}

fn is_start_code_at(bytes: &[u8], index: usize) -> Option<usize> {
    if bytes.get(index..index + 4) == Some(&[0, 0, 0, 1]) {
        Some(4)
    } else if bytes.get(index..index + 3) == Some(&[0, 0, 1]) {
        Some(3)
    } else {
        None
    }
}

pub(crate) fn write_avcc(nalus: &[impl AsRef<[u8]>]) -> Result<Vec<u8>, EncodeError> {
    let capacity = nalus
        .iter()
        .try_fold(0_usize, |size, nalu| {
            size.checked_add(4)?.checked_add(nalu.as_ref().len())
        })
        .ok_or(EncodeError::MalformedBitstream(
            "AVCC output length overflow",
        ))?;
    let mut output = Vec::with_capacity(capacity);
    for nalu in nalus {
        let nalu = nalu.as_ref();
        let length = u32::try_from(nalu.len())
            .map_err(|_| EncodeError::MalformedBitstream("NAL unit exceeds u32"))?;
        output.extend_from_slice(&length.to_be_bytes());
        output.extend_from_slice(nalu);
    }
    Ok(output)
}

pub(crate) fn parameter_sets(nalus: &[impl AsRef<[u8]>], codec: VideoCodec) -> Vec<Vec<u8>> {
    nalus
        .iter()
        .filter(|nalu| {
            nalu_type(nalu.as_ref(), codec)
                .is_some_and(|kind| codec.parameter_set_types().contains(&kind))
        })
        // Only long-lived parameter sets need ownership; access-unit payloads borrow.
        .map(|nalu| nalu.as_ref().to_vec())
        .collect()
}

pub(crate) fn nalu_type(nalu: &[u8], codec: VideoCodec) -> Option<u8> {
    let first = *nalu.first()?;
    Some(match codec {
        VideoCodec::Hevc => (first >> 1) & 0x3f,
        VideoCodec::H264 => first & 0x1f,
    })
}

// Kept platform-independent so the exact production control flow can be tested
// without COM. The adapter above owns sample allocation and HRESULT translation.
mod synchronous {
    use std::{collections::BTreeMap, time::Instant};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct InputMetadata {
        pub capture_at: Instant,
        pub encode_started_at: Instant,
        pub force_keyframe: bool,
    }

    #[derive(Default)]
    pub(super) struct PendingInputs(BTreeMap<i64, InputMetadata>);

    impl PendingInputs {
        pub fn accept(&mut self, timestamp: i64, metadata: InputMetadata) {
            self.0.insert(timestamp, metadata);
        }

        pub fn take(&mut self, timestamp: i64) -> Option<InputMetadata> {
            self.0.remove(&timestamp)
        }

        pub fn is_empty(&self) -> bool {
            self.0.is_empty()
        }

        pub fn len(&self) -> usize {
            self.0.len()
        }
    }

    pub(super) trait SynchronousTransform {
        type Input;
        type Output;
        type Error;

        fn process_input(&mut self, input: &Self::Input) -> Result<(), Self::Error>;
        fn is_not_accepting(error: &Self::Error) -> bool;
        fn process_output(&mut self) -> Result<Option<Self::Output>, Self::Error>;
    }

    pub(super) fn submit_and_drain<T: SynchronousTransform>(
        transform: &mut T,
        input: &T::Input,
    ) -> Result<Vec<T::Output>, T::Error> {
        let mut output = Vec::new();
        match transform.process_input(input) {
            Ok(()) => {}
            Err(error) if T::is_not_accepting(&error) => {
                while let Some(frame) = transform.process_output()? {
                    output.push(frame);
                }
                // A synchronous MFT must accept input after draining to
                // NEED_MORE_INPUT. A repeated rejection is an error, not a spin.
                transform.process_input(input)?;
            }
            Err(error) => return Err(error),
        }
        while let Some(frame) = transform.process_output()? {
            output.push(frame);
        }
        Ok(output)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{collections::VecDeque, time::Duration};

        #[derive(Debug, PartialEq, Eq)]
        enum Fault {
            NotAccepting,
            Input,
            Output,
        }

        struct Input {
            timestamp: i64,
            keyframe: bool,
            metadata: InputMetadata,
        }

        struct Script {
            inputs: VecDeque<Result<(), Fault>>,
            outputs: VecDeque<Result<Option<i64>, Fault>>,
            attempts: Vec<(usize, i64, bool)>,
            pending: PendingInputs,
        }

        impl SynchronousTransform for Script {
            type Input = Input;
            type Output = (i64, InputMetadata);
            type Error = Fault;

            fn process_input(&mut self, input: &Input) -> Result<(), Fault> {
                self.attempts.push((
                    std::ptr::from_ref(input).addr(),
                    input.timestamp,
                    input.keyframe,
                ));
                self.inputs.pop_front().expect("unexpected ProcessInput")?;
                self.pending.accept(input.timestamp, input.metadata);
                Ok(())
            }

            fn is_not_accepting(error: &Fault) -> bool {
                *error == Fault::NotAccepting
            }

            fn process_output(&mut self) -> Result<Option<Self::Output>, Fault> {
                self.outputs
                    .pop_front()
                    .expect("unexpected ProcessOutput")?
                    .map(|timestamp| {
                        self.pending
                            .take(timestamp)
                            .map(|metadata| (timestamp, metadata))
                            .ok_or(Fault::Output)
                    })
                    .transpose()
            }
        }

        fn input(timestamp: i64, origin: Instant) -> Input {
            Input {
                timestamp,
                keyframe: true,
                metadata: InputMetadata {
                    capture_at: origin + Duration::from_secs(timestamp as u64),
                    encode_started_at: origin
                        + Duration::from_secs(timestamp as u64)
                        + Duration::from_millis(1),
                    force_keyframe: true,
                },
            }
        }

        #[test]
        fn rejected_b_drains_a_retries_same_b_with_keyframe_and_drains_b() {
            // Given A is accepted but buffered; B will be rejected once.
            let origin = Instant::now();
            let a = input(0, origin);
            let b = input(1, origin);
            let mut script = Script {
                inputs: VecDeque::from([Ok(()), Err(Fault::NotAccepting), Ok(())]),
                outputs: VecDeque::from([Ok(None), Ok(Some(0)), Ok(None), Ok(Some(1)), Ok(None)]),
                attempts: Vec::new(),
                pending: PendingInputs::default(),
            };
            assert!(submit_and_drain(&mut script, &a).unwrap().is_empty());
            // When B is submitted.
            let output = submit_and_drain(&mut script, &b).unwrap();
            // Then neither sample nor its identity/keyframe intent is lost.
            assert_eq!(output, [(0, a.metadata), (1, b.metadata)]);
            assert_eq!(script.attempts.len(), 3);
            assert_eq!(script.attempts[1], script.attempts[2]);
            assert!(script.attempts[2].2);
            assert!(script.pending.is_empty());
            assert!(script.outputs.is_empty());
            assert!(script.inputs.is_empty());
        }

        #[test]
        fn output_matches_timestamp_not_fifo_and_unknown_identity_is_absent() {
            // Given distinct metadata for inputs A and B.
            let origin = Instant::now();
            let a = input(0, origin);
            let b = input(1, origin);
            let mut pending = PendingInputs::default();
            pending.accept(0, a.metadata);
            pending.accept(1, b.metadata);
            // When B is returned first, then only its actual identity is removed.
            assert_eq!(pending.take(1), Some(b.metadata));
            assert_eq!(pending.len(), 1);
            assert_eq!(pending.take(99), None);
            assert_eq!(pending.take(0), Some(a.metadata));
            assert_eq!(pending.take(1), None);
        }

        #[test]
        fn permanent_input_error_does_not_drain_or_accept() {
            // Given an input fault rather than backpressure.
            let mut script = Script {
                inputs: VecDeque::from([Err(Fault::Input)]),
                outputs: VecDeque::new(),
                attempts: Vec::new(),
                pending: PendingInputs::default(),
            };
            // When submitted, then propagate without attempting output.
            assert_eq!(
                submit_and_drain(&mut script, &input(0, Instant::now())),
                Err(Fault::Input)
            );
            assert!(script.pending.is_empty());
        }

        #[test]
        fn repeated_rejection_after_drain_errors_instead_of_spinning() {
            // Given an MFT violating the synchronous progress contract.
            let mut script = Script {
                inputs: VecDeque::from([Err(Fault::NotAccepting), Err(Fault::NotAccepting)]),
                outputs: VecDeque::from([Ok(None)]),
                attempts: Vec::new(),
                pending: PendingInputs::default(),
            };
            // When retried once, then report the HRESULT without an unbounded loop.
            assert_eq!(
                submit_and_drain(&mut script, &input(0, Instant::now())),
                Err(Fault::NotAccepting)
            );
            assert_eq!(script.attempts.len(), 2);
            assert!(script.pending.is_empty());
        }

        #[test]
        fn drain_error_propagates_without_retrying_input() {
            // Given a failure draining after rejection.
            let mut script = Script {
                inputs: VecDeque::from([Err(Fault::NotAccepting)]),
                outputs: VecDeque::from([Err(Fault::Output)]),
                attempts: Vec::new(),
                pending: PendingInputs::default(),
            };
            // When submitted, then the output error remains visible.
            assert_eq!(
                submit_and_drain(&mut script, &input(0, Instant::now())),
                Err(Fault::Output)
            );
            assert_eq!(script.attempts.len(), 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "windows")]
    fn native_software_encode_and_flush_preserve_capture_identity() {
        // Given the real synchronous software H.264 MFT (no GPU or desktop needed).
        let config = EncoderConfig {
            width: 64,
            height: 64,
            bitrate: 500_000,
            fps: 30,
            keyframe_interval: 30,
            preferred_codec: VideoCodec::H264,
        };
        let mut encoder = MediaFoundationEncoder::new(config).unwrap();
        assert_eq!(encoder.backend(), EncoderBackend::MediaFoundationSoftware);
        let origin = Instant::now();
        let pixels = vec![128; 64 * 64 * 3 / 2];
        let mut frames = Vec::new();
        // When three samples are submitted and the real MFT is drained.
        for index in 0..3 {
            let captured_at = origin - std::time::Duration::from_secs(3 - index);
            frames.extend(encoder.encode_nv12(&pixels, captured_at).unwrap());
        }
        frames.extend(encoder.flush().unwrap());
        // Then all accepted samples carry their own capture identity.
        assert_eq!(frames.len(), 3);
        for frame in frames {
            let index = frame.timestamp_hns / config.frame_duration_hns();
            assert!((0..3).contains(&index));
            assert_eq!(
                frame.capture_at,
                origin - std::time::Duration::from_secs(3 - index as u64)
            );
            assert!(frame.encode_started_at >= origin);
            assert!(frame.encode_completed_at >= frame.encode_started_at);
            assert!(!frame.data.is_empty());
        }
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn reconfigure_adopts_new_resolution_without_dropping_the_session() {
        // Given a live encoder at 128x128 and a desktop resized to 64x64.
        let config = EncoderConfig {
            width: 128,
            height: 128,
            bitrate: 500_000,
            fps: 30,
            keyframe_interval: 30,
            preferred_codec: VideoCodec::H264,
        };
        let mut encoder = MediaFoundationEncoder::new(config).unwrap();
        let origin = Instant::now();
        encoder
            .encode_nv12(&vec![128; encoder.frame_bytes()], origin)
            .unwrap();
        // When the encoder is reconfigured for the new resolution.
        encoder.reconfigure(64, 64).unwrap();
        assert_eq!(encoder.dimensions(), (64, 64));
        // Then the next frame at the new size encodes instead of failing with
        // InvalidFrameLength, and is a keyframe carrying parameter sets.
        let mut frames = encoder
            .encode_nv12(&vec![128; encoder.frame_bytes()], origin)
            .unwrap();
        frames.extend(encoder.flush().unwrap());
        assert!(frames.iter().any(|frame| frame.is_key_frame));
        assert!(frames.iter().all(|frame| !frame.data.is_empty()));
    }

    #[test]
    fn parsed_payloads_borrow_the_access_unit() {
        // Given three realistic-size AVCC slices (192 KiB total payload).
        let payloads = vec![vec![0x65; 65536], vec![0x61; 65536], vec![0x61; 65536]];
        let bytes = write_avcc(&payloads).unwrap();
        // When normalizing the encoded access unit.
        let nalus = parse_access_unit(&bytes).unwrap();
        let start = bytes.as_ptr().addr();
        let end = start + bytes.len();
        let copied_payloads: usize = nalus
            .iter()
            .filter(|nalu| {
                let ptr = nalu.as_ptr().addr();
                ptr < start || ptr + nalu.len() > end
            })
            .count();
        let copied_bytes: usize = nalus
            .iter()
            .filter(|nalu| {
                let ptr = nalu.as_ptr().addr();
                ptr < start || ptr + nalu.len() > end
            })
            .map(|nalu| nalu.len())
            .sum();
        println!("NAL_PAYLOAD_ALLOCATIONS={copied_payloads}; NAL_PAYLOAD_COPY_BYTES={copied_bytes}; OUTPUT_BYTES={}", bytes.len());
        // Then parsing allocates only spans, never another copy of each payload.
        assert_eq!(copied_payloads, 0);
        assert_eq!(write_avcc(&nalus).unwrap(), bytes);
    }

    #[test]
    fn annex_b_is_normalized_to_four_byte_avcc() {
        let annex_b = [0, 0, 0, 1, 0x40, 1, 2, 0, 0, 1, 0x26, 3];
        let nalus = parse_access_unit(&annex_b).unwrap();
        assert_eq!(nalus, vec![vec![0x40, 1, 2], vec![0x26, 3]]);
        assert_eq!(
            write_avcc(&nalus).unwrap(),
            [0, 0, 0, 3, 0x40, 1, 2, 0, 0, 0, 2, 0x26, 3]
        );
    }

    #[test]
    fn avcc_round_trips_and_rejects_truncation() {
        let avcc = [0, 0, 0, 2, 0x42, 1, 0, 0, 0, 1, 0x44];
        assert_eq!(
            write_avcc(&parse_access_unit(&avcc).unwrap()).unwrap(),
            avcc
        );
        assert!(parse_access_unit(&avcc[..avcc.len() - 1]).is_err());
    }

    #[test]
    fn extracts_hevc_and_h264_parameter_sets() {
        let hevc = vec![vec![32 << 1], vec![33 << 1], vec![34 << 1], vec![19 << 1]];
        assert_eq!(parameter_sets(&hevc, VideoCodec::Hevc).len(), 3);
        let h264 = vec![vec![0x67], vec![0x68], vec![0x65]];
        assert_eq!(parameter_sets(&h264, VideoCodec::H264).len(), 2);
    }
}

#[cfg(test)]
mod stream_configuration_tests {
    use super::*;

    #[test]
    #[cfg(target_os = "windows")]
    fn software_h264_negotiates_high_or_main_with_cabac_when_supported() {
        // Given the real synchronous Microsoft software H.264 MFT.
        let _runtime = MediaFoundationRuntime::start().unwrap();
        let config = EncoderConfig {
            width: 64,
            height: 64,
            bitrate: 500_000,
            fps: 30,
            keyframe_interval: 30,
            preferred_codec: VideoCodec::H264,
        };
        let (transform, _, _) = create_transform(VideoCodec::H264).unwrap();
        let codec_api = transform.cast::<ICodecAPI>().ok();
        // When the output type is negotiated through the High -> Main ladder.
        let output_type =
            negotiate_output_type(&transform, codec_api.as_ref(), VideoCodec::H264, config)
                .unwrap();
        // Then an explicit High or Main profile is set on the accepted type...
        // SAFETY: reading attributes from the negotiated media type and encoder.
        unsafe {
            let profile = output_type.GetUINT32(&MF_MT_MPEG2_PROFILE).unwrap();
            assert!(
                profile == H264Profile::High.mf_value() || profile == H264Profile::Main.mf_value(),
                "unexpected profile {profile}"
            );
            let current = transform.GetOutputCurrentType(0).unwrap();
            assert_eq!(current.GetUINT32(&MF_MT_MPEG2_PROFILE).unwrap(), profile);
            // ...and CABAC is enabled whenever the encoder exposes the property.
            if let Some(api) = &codec_api {
                if api.IsSupported(&CODECAPI_AVEncH264CABACEnable).is_ok() {
                    let cabac = api.GetValue(&CODECAPI_AVEncH264CABACEnable).unwrap();
                    assert_eq!(bool::try_from(&cabac).ok(), Some(true));
                }
            }
        }
    }

    #[test]
    fn h264_media_type_uses_high_profile_and_full_range_bt601() {
        let _runtime = MediaFoundationRuntime::start().unwrap();
        let config = EncoderConfig {
            width: 1280,
            height: 720,
            bitrate: 4_000_000,
            fps: 60,
            keyframe_interval: 0,
            preferred_codec: VideoCodec::H264,
        };
        let media_type = h264_output_type(config, H264Profile::High).unwrap();
        let main = h264_output_type(config, H264Profile::Main).unwrap();
        // SAFETY: reading attributes from the newly configured media type.
        unsafe {
            assert_eq!(media_type.GetUINT32(&MF_MT_MPEG2_PROFILE).unwrap(), 100);
            assert_eq!(main.GetUINT32(&MF_MT_MPEG2_PROFILE).unwrap(), 77);
            assert_eq!(
                media_type
                    .GetUINT32(&windows::Win32::Media::MediaFoundation::MF_MT_YUV_MATRIX)
                    .unwrap(),
                2
            );
            assert_eq!(
                media_type
                    .GetUINT32(&windows::Win32::Media::MediaFoundation::MF_MT_VIDEO_NOMINAL_RANGE)
                    .unwrap(),
                1
            );
            assert_eq!(
                media_type.GetUINT64(&MF_MT_FRAME_SIZE).unwrap(),
                (1280u64 << 32) | 720
            );
        }
    }
}
