//! FFmpeg video encoding for the Linux host.
//!
//! The preferred encoders are `hevc_vaapi` and `h264_vaapi`. They use libva
//! through FFmpeg's VAAPI hardware-device and hardware-frame contexts. BGRA
//! capture frames are converted to NV12 and uploaded to VAAPI surfaces. If no
//! VAAPI encoder/device can be opened, Cisco's `libopenh264` is opened in
//! bitrate rate-control mode with no frame skipping, no B-frames, and a
//! one-thread policy. OpenH264 keeps the FFmpeg build LGPL; libx264 would
//! make it GPL and is not used.
//!
//! Output is always AVCC-style: every NAL unit has a four-byte big-endian
//! length prefix. FFmpeg's Annex-B output is converted when necessary. On
//! keyframes the cached VPS/SPS/PPS (HEVC) or SPS/PPS (H.264) are prepended,
//! matching the macOS VideoToolbox contract.
//!
//! Runtime QA on the deployment host must exercise the actual GPU. For Arch,
//! install `ffmpeg`, `libva`, and the vendor VA driver (`libva-mesa-driver` or
//! `intel-media-driver`), then confirm `vainfo` and `ffmpeg -encoders` expose
//! the selected codec.

use std::{ffi::CString, ptr};

use ffmpeg_next as ffmpeg;
use thiserror::Error;

use ffmpeg::{
    codec,
    format::Pixel,
    frame,
    software::scaling::{context::Context as ScaleContext, flag::Flags as ScaleFlags},
    Dictionary, Packet,
};

#[cfg(test)]
thread_local! {
    static SOFTWARE_FRAME_ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn allocate_software_frame(format: Pixel, width: u32, height: u32) -> frame::Video {
    let frame = frame::Video::new(format, width, height);
    #[cfg(test)]
    SOFTWARE_FRAME_ALLOCATIONS.with(|count| count.set(count.get() + 1));
    frame
}

fn make_frame_writable(frame: &mut frame::Video) -> Result<(), EncodeError> {
    // SAFETY: Video owns a live AVFrame. The exclusive borrow prevents Rust aliases;
    // FFmpeg detaches shared AVBufferRefs before the caller mutates retained pixels.
    let status = unsafe { ffmpeg::ffi::av_frame_make_writable(frame.as_mut_ptr()) };
    if status < 0 {
        return Err(EncodeError::Ffmpeg(ffmpeg::Error::from(status)));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    Hevc,
    H264,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderBackend {
    Nvenc,
    Vaapi,
    OpenH264,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub bitrate: usize,
    pub fps: u32,
    pub keyframe_interval: u32,
    pub preferred_codec: VideoCodec,
}

impl EncoderConfig {
    pub fn validate(self) -> Result<Self, EncodeError> {
        if self.width == 0 || self.height == 0 || self.fps == 0 || self.bitrate == 0 {
            return Err(EncodeError::InvalidConfig);
        }
        // The NV12 conversion reads and writes pixel pairs per 2x2 block.
        if self.width % 2 != 0 || self.height % 2 != 0 {
            return Err(EncodeError::InvalidConfig);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub is_key_frame: bool,
    pub codec: VideoCodec,
    pub pts: i64,
}

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("encoder dimensions must be non-zero and even; FPS and bitrate must be non-zero")]
    InvalidConfig,
    #[error("BGRA frame length/stride does not match the configured dimensions")]
    InvalidFrame,
    #[error("no VAAPI encoder or libopenh264 fallback is available")]
    EncoderUnavailable,
    #[error("FFmpeg error: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    #[error("encoded packet is neither valid Annex-B nor AVCC")]
    InvalidBitstream,
    #[error("encoded packet is missing its input presentation timestamp")]
    MissingOutputTimestamp,
}

pub struct HwBufferRef(*mut ffmpeg::ffi::AVBufferRef);
impl HwBufferRef {
    /// # Safety
    /// raw must point to a live AVBufferRef for the duration of this call.
    pub unsafe fn retain(raw: *mut ffmpeg::ffi::AVBufferRef) -> Result<Self, EncodeError> {
        let retained = unsafe { ffmpeg::ffi::av_buffer_ref(raw) };
        if retained.is_null() {
            return Err(EncodeError::EncoderUnavailable);
        }
        Ok(Self(retained))
    }
    pub fn as_ptr(&self) -> *mut ffmpeg::ffi::AVBufferRef {
        self.0
    }
}
impl Drop for HwBufferRef {
    fn drop(&mut self) {
        unsafe { ffmpeg::ffi::av_buffer_unref(&mut self.0) };
    }
}

struct VaapiResources {
    device: *mut ffmpeg::ffi::AVBufferRef,
    frames: *mut ffmpeg::ffi::AVBufferRef,
}

impl Drop for VaapiResources {
    fn drop(&mut self) {
        unsafe {
            ffmpeg::ffi::av_buffer_unref(&mut self.frames);
            ffmpeg::ffi::av_buffer_unref(&mut self.device);
        }
    }
}

struct OpenEncoder {
    encoder: ffmpeg::codec::encoder::video::Encoder,
    backend: EncoderBackend,
    codec: VideoCodec,
    scaler: ScaleContext,
    software: frame::Video,
    source: frame::Video,
    vaapi: Option<VaapiResources>,
}

pub struct LinuxVideoEncoder {
    config: EncoderConfig,
    open: OpenEncoder,
    next_pts: i64,
    force_keyframe: bool,
    parameter_sets: Vec<Vec<u8>>,
    pending_bitrate: Option<usize>,
    last_bitrate_pts: Option<i64>,
}

fn bitrate_reopen_interval(config: EncoderConfig) -> u32 {
    if config.keyframe_interval == 0 {
        config.fps.max(1)
    } else {
        config.keyframe_interval
    }
}

impl LinuxVideoEncoder {
    pub fn new_cuda(
        config: EncoderConfig,
        frames: *mut ffmpeg::ffi::AVBufferRef,
    ) -> Result<Self, EncodeError> {
        let config = config.validate()?;
        ffmpeg::init()?;
        let open = open_nvenc_frames(config, config.preferred_codec, Some(frames))
            .or_else(|_| open_nvenc_frames(config, VideoCodec::H264, Some(frames)))?;
        Ok(Self {
            config,
            open,
            next_pts: 0,
            force_keyframe: true,
            parameter_sets: Vec::new(),
            pending_bitrate: None,
            last_bitrate_pts: None,
        })
    }

    fn reopen_nvenc(&self, config: EncoderConfig) -> Result<OpenEncoder, EncodeError> {
        let frames = unsafe { (*self.open.encoder.as_ptr()).hw_frames_ctx };
        open_nvenc_frames(
            config,
            self.open.codec,
            (!frames.is_null()).then_some(frames),
        )
    }
    pub fn new_vaapi(config: EncoderConfig) -> Result<Self, EncodeError> {
        let config = config.validate()?;
        ffmpeg::init()?;
        let open = open_vaapi(config, config.preferred_codec)
            .or_else(|_| open_vaapi(config, VideoCodec::H264))?;
        Ok(Self {
            config,
            open,
            next_pts: 0,
            force_keyframe: true,
            parameter_sets: Vec::new(),
            pending_bitrate: None,
            last_bitrate_pts: None,
        })
    }

    pub fn new(config: EncoderConfig) -> Result<Self, EncodeError> {
        let config = config.validate()?;
        ffmpeg::init()?;

        let attempts = match config.preferred_codec {
            VideoCodec::Hevc => [VideoCodec::Hevc, VideoCodec::H264],
            VideoCodec::H264 => [VideoCodec::H264, VideoCodec::Hevc],
        };
        let mut last_error = None;
        // Priority 1: Hardware NVENC on NVIDIA
        for codec in attempts {
            match open_nvenc(config, codec) {
                Ok(open) => {
                    tracing::info!(backend = ?open.backend, codec = ?open.codec, "Initialized Linux NVENC encoder");
                    return Ok(Self {
                        config,
                        open,
                        next_pts: 0,
                        force_keyframe: true,
                        parameter_sets: Vec::new(),
                        pending_bitrate: None,
                        last_bitrate_pts: None,
                    });
                }
                Err(error) => last_error = Some(error),
            }
        }
        // Priority 2: VAAPI
        for codec in attempts {
            match open_vaapi(config, codec) {
                Ok(open) => {
                    return Ok(Self {
                        config,
                        open,
                        next_pts: 0,
                        force_keyframe: true,
                        parameter_sets: Vec::new(),
                        pending_bitrate: None,
                        last_bitrate_pts: None,
                    });
                }
                Err(error) => last_error = Some(error),
            }
        }
        match open_openh264(config) {
            Ok(open) => Ok(Self {
                config,
                open,
                next_pts: 0,
                force_keyframe: true,
                parameter_sets: Vec::new(),
                pending_bitrate: None,
                last_bitrate_pts: None,
            }),
            Err(_) => Err(last_error.unwrap_or(EncodeError::EncoderUnavailable)),
        }
    }

    pub fn backend(&self) -> EncoderBackend {
        self.open.backend
    }

    pub fn codec(&self) -> VideoCodec {
        self.open.codec
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Rebuilds the transform when the capture resolution changes, returning
    /// packets drained from the previous encoder so their PTS stay resolvable.
    /// The fresh transform forces a keyframe so the client receives parameter
    /// sets for the new size.
    pub fn reconfigure(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<Vec<EncodedFrame>, EncodeError> {
        if width == self.config.width && height == self.config.height {
            return Ok(Vec::new());
        }
        let config = EncoderConfig {
            width,
            height,
            ..self.config
        }
        .validate()?;
        let replacement = match self.open.backend {
            EncoderBackend::Nvenc => self.reopen_nvenc(config)?,
            EncoderBackend::Vaapi => open_vaapi(config, self.open.codec)?,
            EncoderBackend::OpenH264 => open_openh264(config)?,
        };
        let drained = self.drain()?;
        self.open = replacement;
        self.config = config;
        self.parameter_sets.clear();
        self.force_keyframe = true;
        Ok(drained)
    }

    pub fn force_key_frame(&mut self) {
        self.force_keyframe = true;
    }

    pub fn current_pts(&self) -> i64 {
        self.next_pts
    }

    pub fn update_bitrate(&mut self, bitrate: usize) -> Result<Vec<EncodedFrame>, EncodeError> {
        let config = EncoderConfig {
            bitrate,
            ..self.config
        }
        .validate()?;
        if bitrate == self.config.bitrate {
            self.pending_bitrate = None;
            return Ok(Vec::new());
        }
        self.pending_bitrate = Some(bitrate);
        // Bound reopen frequency to one GOP and ignore small feedback noise.
        if self.last_bitrate_pts.is_some_and(|pts| {
            self.next_pts - pts < i64::from(bitrate_reopen_interval(self.config))
        }) || (self.config.keyframe_interval != 0
            && self.next_pts % i64::from(bitrate_reopen_interval(self.config)) != 0)
            || bitrate.abs_diff(self.config.bitrate) < self.config.bitrate / 10
        {
            return Ok(Vec::new());
        }
        // Reopen before draining: post-open setters are not honored by all codecs.
        self.last_bitrate_pts = Some(self.next_pts);
        let replacement = match self.open.backend {
            EncoderBackend::Nvenc => self.reopen_nvenc(config),
            EncoderBackend::Vaapi => open_vaapi(config, self.open.codec),
            EncoderBackend::OpenH264 => open_openh264(config),
        };
        let replacement = match replacement {
            Ok(replacement) => replacement,
            Err(error) => {
                tracing::warn!(%error, "bitrate reopen rejected; retaining active encoder");
                return Ok(Vec::new());
            }
        };
        self.pending_bitrate = None;
        let pending = self.drain()?;
        self.open = replacement;
        self.config = config;
        self.parameter_sets.clear();
        self.force_keyframe = true;
        Ok(pending)
    }

    pub fn apply_pending_bitrate(&mut self) -> Result<Vec<EncodedFrame>, EncodeError> {
        match self.pending_bitrate {
            Some(bitrate) => self.update_bitrate(bitrate),
            None => Ok(Vec::new()),
        }
    }

    /// Encodes one tightly packed or padded BGRA frame.
    pub fn encode_bgra(
        &mut self,
        bgra: &[u8],
        stride: usize,
    ) -> Result<Vec<EncodedFrame>, EncodeError> {
        let mut bitrate_packets = if let Some(bitrate) = self.pending_bitrate {
            self.update_bitrate(bitrate)?
        } else {
            Vec::new()
        };
        let row_bytes = self.config.width as usize * 4;
        let required = stride
            .checked_mul(self.config.height as usize)
            .ok_or(EncodeError::InvalidFrame)?;
        if stride < row_bytes || bgra.len() < required {
            return Err(EncodeError::InvalidFrame);
        }

        let pts = self.next_pts;
        self.next_pts += 1;
        let software = &mut self.open.software;
        make_frame_writable(software)?;

        if self.open.backend == EncoderBackend::Nvenc || self.open.backend == EncoderBackend::Vaapi
        {
            let width = self.config.width as usize;
            let height = self.config.height as usize;
            let y_stride = software.stride(0);
            let uv_stride = software.stride(1);
            let dst_y = software.data_mut(0);

            for y in 0..height {
                let s_row = &bgra[y * stride..y * stride + width * 4];
                let dy_row = &mut dst_y[y * y_stride..y * y_stride + width];
                for (x, dst_pixel) in dy_row.iter_mut().enumerate() {
                    let p = x * 4;
                    let b = s_row[p] as i32;
                    let g = s_row[p + 1] as i32;
                    let r = s_row[p + 2] as i32;
                    *dst_pixel = ((77 * r + 150 * g + 29 * b + 128) >> 8).clamp(0, 255) as u8;
                }
            }

            let dst_uv = software.data_mut(1);
            let uv_height = height / 2;
            for uv_y in 0..uv_height {
                let y = uv_y * 2;
                let s_row = &bgra[y * stride..y * stride + width * 4];
                let next_row = &bgra[(y + 1) * stride..(y + 1) * stride + width * 4];
                let duv_row = &mut dst_uv[uv_y * uv_stride..uv_y * uv_stride + width];
                for x in (0..width).step_by(2) {
                    let p0 = x * 4;
                    let p1 = (x + 1) * 4;
                    let [u, v] = chroma_2x2(&s_row[p0..p1 + 4], &next_row[p0..p1 + 4]);
                    duv_row[x] = u;
                    duv_row[x + 1] = v;
                }
            }
        } else {
            // For the OpenH264 software fallback (YUV420P)
            let source = &mut self.open.source;
            let source_stride = source.stride(0);
            for row in 0..self.config.height as usize {
                let input_start = row * stride;
                let output_start = row * source_stride;
                source.data_mut(0)[output_start..output_start + row_bytes]
                    .copy_from_slice(&bgra[input_start..input_start + row_bytes]);
            }
            self.open.scaler.run(source, software)?;
        }
        software.set_pts(Some(pts));
        if self.force_keyframe {
            software.set_kind(ffmpeg::picture::Type::I);
        } else {
            software.set_kind(ffmpeg::picture::Type::None);
        }

        let hardware = if let Some(vaapi) = &self.open.vaapi {
            let mut hardware = frame::Video::empty();
            unsafe {
                let status =
                    ffmpeg::ffi::av_hwframe_get_buffer(vaapi.frames, hardware.as_mut_ptr(), 0);
                if status < 0 {
                    return Err(EncodeError::Ffmpeg(ffmpeg::Error::from(status)));
                }
                let status = ffmpeg::ffi::av_hwframe_transfer_data(
                    hardware.as_mut_ptr(),
                    software.as_ptr(),
                    0,
                );
                if status < 0 {
                    return Err(EncodeError::Ffmpeg(ffmpeg::Error::from(status)));
                }
            }
            hardware.set_pts(Some(pts));
            hardware.set_kind(software.kind());
            Some(hardware)
        } else {
            None
        };

        // EAGAIN from avcodec_send_frame means the output queue is full: drain
        // it and resubmit the same frame instead of failing the pipeline.
        let mut output = Vec::new();
        match self.submit_frame(hardware.as_ref()) {
            Ok(()) => {}
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::ffi::EAGAIN => {
                output = self.receive_packets()?;
                self.submit_frame(hardware.as_ref())?;
            }
            Err(error) => return Err(EncodeError::Ffmpeg(error)),
        }

        self.force_keyframe = false;
        output.extend(self.receive_packets()?);
        bitrate_packets.extend(output);
        Ok(bitrate_packets)
    }

    /// Encodes a hardware VAAPI surface frame (e.g. from VPP or external HW frame context).
    pub fn encode_vaapi_frame(
        &mut self,
        mut frame: frame::Video,
    ) -> Result<Vec<EncodedFrame>, EncodeError> {
        let pts = self.next_pts;
        self.next_pts += 1;
        frame.set_pts(Some(pts));
        if self.force_keyframe {
            frame.set_kind(ffmpeg::picture::Type::I);
        } else {
            frame.set_kind(ffmpeg::picture::Type::None);
        }

        let mut output = Vec::new();
        match self.submit_frame(Some(&frame)) {
            Ok(()) => {}
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::ffi::EAGAIN => {
                output = self.receive_packets()?;
                self.submit_frame(Some(&frame))?;
            }
            Err(error) => return Err(EncodeError::Ffmpeg(error)),
        }

        self.force_keyframe = false;
        output.extend(self.receive_packets()?);
        Ok(output)
    }

    /// Exposes the underlying VAAPI device and frames AVBufferRef pointers if VAAPI is active.
    pub fn vaapi_hwctx(
        &self,
    ) -> Option<(*mut ffmpeg::ffi::AVBufferRef, *mut ffmpeg::ffi::AVBufferRef)> {
        self.open.vaapi.as_ref().map(|v| (v.device, v.frames))
    }

    /// Submits either the VAAPI surface or the software conversion frame.
    fn submit_frame(&mut self, hardware: Option<&frame::Video>) -> Result<(), ffmpeg::Error> {
        match hardware {
            Some(hardware) => self.open.encoder.send_frame(hardware),
            None => self.open.encoder.send_frame(&self.open.software),
        }
    }

    pub fn drain(&mut self) -> Result<Vec<EncodedFrame>, EncodeError> {
        self.open.encoder.send_eof()?;
        self.receive_packets()
    }

    fn receive_packets(&mut self) -> Result<Vec<EncodedFrame>, EncodeError> {
        let mut output = Vec::new();
        loop {
            let mut packet = Packet::empty();
            match self.open.encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    let bytes = packet.data().ok_or(EncodeError::InvalidBitstream)?;
                    let nal_units = parse_nal_units(bytes)?;
                    let is_key = packet.is_key();
                    if is_key {
                        let discovered = extract_parameter_sets(self.open.codec, &nal_units);
                        if !discovered.is_empty() {
                            self.parameter_sets = discovered;
                        }
                    }
                    let mut avcc = Vec::new();
                    if is_key {
                        append_unique_parameter_sets(&mut avcc, &self.parameter_sets, &nal_units)?;
                    }
                    for nalu in &nal_units {
                        append_avcc_nalu(&mut avcc, nalu)?;
                    }
                    output.push(EncodedFrame {
                        data: avcc,
                        is_key_frame: is_key,
                        codec: self.open.codec,
                        pts: packet_timestamp(&packet)?,
                    });
                }
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::ffi::EAGAIN => break,
                Err(ffmpeg::Error::Eof) => break,
                Err(error) => return Err(EncodeError::Ffmpeg(error)),
            }
        }
        Ok(output)
    }
}

fn chroma_2x2(top: &[u8], bottom: &[u8]) -> [u8; 2] {
    let average = |channel| {
        (i32::from(top[channel])
            + i32::from(top[channel + 4])
            + i32::from(bottom[channel])
            + i32::from(bottom[channel + 4])
            + 2)
            >> 2
    };
    let (r, g, b) = (average(2), average(1), average(0));
    [
        (128 + ((-43 * r - 85 * g + 128 * b + 128) >> 8)).clamp(0, 255) as u8,
        (128 + ((128 * r - 107 * g - 21 * b + 128) >> 8)).clamp(0, 255) as u8,
    ]
}

fn packet_timestamp(packet: &Packet) -> Result<i64, EncodeError> {
    packet.pts().ok_or(EncodeError::MissingOutputTimestamp)
}

fn base_video_context(
    config: EncoderConfig,
    codec: ffmpeg::Codec,
    pixel_format: Pixel,
) -> Result<ffmpeg::codec::encoder::video::Video, EncodeError> {
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()?;
    encoder.set_width(config.width);
    encoder.set_height(config.height);
    encoder.set_format(pixel_format);
    encoder.set_time_base((1, config.fps as i32));
    encoder.set_frame_rate(Some((config.fps as i32, 1)));
    encoder.set_bit_rate(config.bitrate);
    encoder.set_max_bit_rate(config.bitrate);
    encoder.set_gop(if config.keyframe_interval == 0 {
        i32::MAX as u32
    } else {
        config.keyframe_interval
    });
    encoder.set_max_b_frames(0);
    encoder.set_threading(codec::threading::Config::count(1));
    // SAFETY: encoder owns a valid live AVCodecContext pointer; color properties are populated prior to open.
    unsafe {
        let ptr = encoder.as_mut_ptr();
        (*ptr).color_range = ffmpeg::ffi::AVColorRange::AVCOL_RANGE_JPEG;
        (*ptr).colorspace = ffmpeg::ffi::AVColorSpace::AVCOL_SPC_SMPTE170M;
        (*ptr).color_primaries = ffmpeg::ffi::AVColorPrimaries::AVCOL_PRI_SMPTE170M;
        (*ptr).color_trc = ffmpeg::ffi::AVColorTransferCharacteristic::AVCOL_TRC_SMPTE170M;
    }
    Ok(encoder)
}

fn open_nvenc(config: EncoderConfig, video_codec: VideoCodec) -> Result<OpenEncoder, EncodeError> {
    open_nvenc_frames(config, video_codec, None)
}

fn open_nvenc_frames(
    config: EncoderConfig,
    video_codec: VideoCodec,
    frames: Option<*mut ffmpeg::ffi::AVBufferRef>,
) -> Result<OpenEncoder, EncodeError> {
    let name = match video_codec {
        VideoCodec::Hevc => "hevc_nvenc",
        VideoCodec::H264 => "h264_nvenc",
    };
    let codec = codec::encoder::find_by_name(name).ok_or(EncodeError::EncoderUnavailable)?;
    let mut encoder = base_video_context(
        config,
        codec,
        if frames.is_some() {
            Pixel::CUDA
        } else {
            Pixel::NV12
        },
    )?;
    if let Some(frames) = frames {
        unsafe {
            (*encoder.as_mut_ptr()).hw_frames_ctx = ffmpeg::ffi::av_buffer_ref(frames);
        }
    }
    let mut options = Dictionary::new();
    options.set("preset", "p1");
    options.set("tune", "ull");
    options.set("zerolatency", "1");
    options.set("forced-idr", "1");
    options.set("repeat_headers", "1");

    options.set("bf", "0");
    let encoder = encoder.open_as_with(codec, options)?;
    let scaler = ScaleContext::get(
        Pixel::BGRA,
        config.width,
        config.height,
        Pixel::NV12,
        config.width,
        config.height,
        ScaleFlags::FAST_BILINEAR,
    )?;
    Ok(OpenEncoder {
        encoder,
        backend: EncoderBackend::Nvenc,
        codec: video_codec,
        scaler,
        software: allocate_software_frame(Pixel::NV12, config.width, config.height),
        source: frame::Video::empty(),
        vaapi: None,
    })
}

fn open_vaapi(config: EncoderConfig, video_codec: VideoCodec) -> Result<OpenEncoder, EncodeError> {
    let name = match video_codec {
        VideoCodec::Hevc => "hevc_vaapi",
        VideoCodec::H264 => "h264_vaapi",
    };
    let codec = codec::encoder::find_by_name(name).ok_or(EncodeError::EncoderUnavailable)?;
    let mut encoder = base_video_context(config, codec, Pixel::VAAPI)?;

    let mut device = ptr::null_mut();
    let device_path = std::env::var("MAHO_VAAPI_DEVICE")
        .ok()
        .map(CString::new)
        .transpose()
        .map_err(|_| EncodeError::InvalidConfig)?;
    let status = unsafe {
        ffmpeg::ffi::av_hwdevice_ctx_create(
            &mut device,
            ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
            device_path
                .as_ref()
                .map_or(ptr::null(), |path| path.as_ptr()),
            ptr::null_mut(),
            0,
        )
    };
    if status < 0 {
        return Err(EncodeError::Ffmpeg(ffmpeg::Error::from(status)));
    }

    let frames = unsafe { ffmpeg::ffi::av_hwframe_ctx_alloc(device) };
    if frames.is_null() {
        unsafe { ffmpeg::ffi::av_buffer_unref(&mut device) };
        return Err(EncodeError::EncoderUnavailable);
    }
    unsafe {
        let frames_context = (*frames).data.cast::<ffmpeg::ffi::AVHWFramesContext>();
        (*frames_context).format = Pixel::VAAPI.into();
        (*frames_context).sw_format = Pixel::NV12.into();
        (*frames_context).width = config.width as i32;
        (*frames_context).height = config.height as i32;
        (*frames_context).initial_pool_size = 16;
        let status = ffmpeg::ffi::av_hwframe_ctx_init(frames);
        if status < 0 {
            let mut frames = frames;
            ffmpeg::ffi::av_buffer_unref(&mut frames);
            ffmpeg::ffi::av_buffer_unref(&mut device);
            return Err(EncodeError::Ffmpeg(ffmpeg::Error::from(status)));
        }
        (*encoder.as_mut_ptr()).hw_frames_ctx = ffmpeg::ffi::av_buffer_ref(frames);
    }

    let mut options = Dictionary::new();
    options.set("rc_mode", "CBR");
    options.set("idr_interval", "0");
    options.set("bf", "0");
    let encoder = match encoder.open_as_with(codec, options) {
        Ok(encoder) => encoder,
        Err(error) => {
            let resources = VaapiResources { device, frames };
            drop(resources);
            return Err(EncodeError::Ffmpeg(error));
        }
    };
    let scaler = ScaleContext::get(
        Pixel::BGRA,
        config.width,
        config.height,
        Pixel::NV12,
        config.width,
        config.height,
        ScaleFlags::FAST_BILINEAR,
    )?;
    Ok(OpenEncoder {
        encoder,
        backend: EncoderBackend::Vaapi,
        codec: video_codec,
        scaler,
        software: allocate_software_frame(Pixel::NV12, config.width, config.height),
        source: frame::Video::empty(),
        vaapi: Some(VaapiResources { device, frames }),
    })
}

fn open_openh264(config: EncoderConfig) -> Result<OpenEncoder, EncodeError> {
    let codec =
        codec::encoder::find_by_name("libopenh264").ok_or(EncodeError::EncoderUnavailable)?;
    let encoder = base_video_context(config, codec, Pixel::YUV420P)?;
    let mut options = Dictionary::new();
    options.set("rc_mode", "bitrate");
    options.set("allow_skip_frames", "0");
    let encoder = encoder.open_as_with(codec, options)?;
    let scaler = ScaleContext::get(
        Pixel::BGRA,
        config.width,
        config.height,
        Pixel::YUV420P,
        config.width,
        config.height,
        ScaleFlags::FAST_BILINEAR,
    )?;
    Ok(OpenEncoder {
        encoder,
        backend: EncoderBackend::OpenH264,
        codec: VideoCodec::H264,
        scaler,
        software: allocate_software_frame(Pixel::YUV420P, config.width, config.height),
        source: allocate_software_frame(Pixel::BGRA, config.width, config.height),
        vaapi: None,
    })
}

fn append_avcc_nalu(output: &mut Vec<u8>, nalu: &[u8]) -> Result<(), EncodeError> {
    let length = u32::try_from(nalu.len()).map_err(|_| EncodeError::InvalidBitstream)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(nalu);
    Ok(())
}

fn append_unique_parameter_sets(
    output: &mut Vec<u8>,
    parameter_sets: &[Vec<u8>],
    frame_nalus: &[&[u8]],
) -> Result<(), EncodeError> {
    for parameter_set in parameter_sets {
        if !frame_nalus.contains(&parameter_set.as_slice()) {
            append_avcc_nalu(output, parameter_set)?;
        }
    }
    Ok(())
}

fn parse_nal_units(data: &[u8]) -> Result<Vec<&[u8]>, EncodeError> {
    // A four-byte AVCC length of one is byte-identical to an Annex-B start
    // code. Parse a complete AVCC packet first, then fall back to Annex-B.
    parse_avcc(data).or_else(|_| parse_annex_b(data))
}

fn parse_annex_b(data: &[u8]) -> Result<Vec<&[u8]>, EncodeError> {
    let mut starts = Vec::new();
    let mut index = 0;
    while index + 3 <= data.len() {
        let prefix = if data[index..].starts_with(&[0, 0, 0, 1]) {
            Some(4)
        } else if data[index..].starts_with(&[0, 0, 1]) {
            Some(3)
        } else {
            None
        };
        if let Some(length) = prefix {
            starts.push((index, index + length));
            index += length;
        } else {
            index += 1;
        }
    }
    let mut nalus = Vec::new();
    for (position, (_, payload_start)) in starts.iter().enumerate() {
        let payload_end = starts
            .get(position + 1)
            .map_or(data.len(), |(next_start, _)| *next_start);
        let mut trimmed_end = payload_end;
        while trimmed_end > *payload_start && data[trimmed_end - 1] == 0 {
            trimmed_end -= 1;
        }
        if trimmed_end > *payload_start {
            nalus.push(&data[*payload_start..trimmed_end]);
        }
    }
    (!nalus.is_empty())
        .then_some(nalus)
        .ok_or(EncodeError::InvalidBitstream)
}

fn parse_avcc(data: &[u8]) -> Result<Vec<&[u8]>, EncodeError> {
    let mut nalus = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        if offset + 4 > data.len() {
            return Err(EncodeError::InvalidBitstream);
        }
        let length =
            u32::from_be_bytes(data[offset..offset + 4].try_into().expect("four bytes")) as usize;
        offset += 4;
        if length == 0 || offset + length > data.len() {
            return Err(EncodeError::InvalidBitstream);
        }
        nalus.push(&data[offset..offset + length]);
        offset += length;
    }
    (!nalus.is_empty())
        .then_some(nalus)
        .ok_or(EncodeError::InvalidBitstream)
}

fn extract_parameter_sets(codec: VideoCodec, nalus: &[&[u8]]) -> Vec<Vec<u8>> {
    nalus
        .iter()
        .filter(|nalu| match codec {
            VideoCodec::H264 => matches!(nalu.first().map(|byte| byte & 0x1f), Some(7 | 8)),
            VideoCodec::Hevc => {
                nalu.len() >= 2
                    && matches!(nalu.first().map(|byte| (byte >> 1) & 0x3f), Some(32..=34))
            }
        })
        .map(|nalu| nalu.to_vec())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chroma_averages_both_rows_with_full_range_matrix() {
        let red = [0, 0, 255, 255, 0, 0, 255, 255];
        let blue = [255, 0, 0, 255, 255, 0, 0, 255];
        assert_eq!(chroma_2x2(&red, &blue), chroma_2x2(&blue, &red));
        assert_eq!(chroma_2x2(&red, &blue), [171, 182]);
        assert_eq!(chroma_2x2(&[255; 8], &[255; 8]), [128, 128]);
    }

    #[test]
    fn retained_hw_buffer_outlives_original_owner() {
        unsafe {
            let mut original = ffmpeg::ffi::av_buffer_alloc(8);
            assert!(!original.is_null());
            *(*original).data = 73;
            let retained = HwBufferRef::retain(original).unwrap();
            ffmpeg::ffi::av_buffer_unref(&mut original);
            assert_eq!(*(*retained.as_ptr()).data, 73);
            drop(retained);
        }
    }

    #[test]
    fn reconfigure_adopts_new_resolution_and_preserves_pts() {
        ffmpeg::init().unwrap();
        let config = EncoderConfig {
            width: 32,
            height: 32,
            bitrate: 100_000,
            fps: 30,
            keyframe_interval: 300,
            preferred_codec: VideoCodec::H264,
        };
        let mut encoder = LinuxVideoEncoder {
            config,
            open: open_openh264(config).unwrap(),
            next_pts: 0,
            force_keyframe: true,
            parameter_sets: Vec::new(),
            pending_bitrate: None,
            last_bitrate_pts: None,
        };
        encoder.encode_bgra(&vec![128; 32 * 32 * 4], 128).unwrap();
        // When the capture resolution changes.
        let drained = encoder.reconfigure(32, 16).unwrap();
        assert_eq!(encoder.dimensions(), (32, 16));
        // Drained packets belong to inputs accepted before the resize.
        assert!(drained.iter().all(|frame| frame.pts == 0));
        // Then the next frame at the new size encodes with the next PTS and a
        // fresh keyframe, instead of failing the pipeline with InvalidFrame.
        let output = encoder.encode_bgra(&vec![128; 32 * 16 * 4], 128).unwrap();
        assert!(output.iter().any(|frame| frame.pts == 1));
        assert!(output.iter().any(|frame| frame.is_key_frame));
    }

    #[test]
    fn conversion_frames_are_reused_after_warmup() {
        let config = EncoderConfig {
            width: 32,
            height: 32,
            bitrate: 100_000,
            fps: 30,
            keyframe_interval: 300,
            preferred_codec: VideoCodec::H264,
        };
        let mut encoder = LinuxVideoEncoder {
            config,
            open: open_openh264(config).unwrap(),
            next_pts: 0,
            force_keyframe: true,
            parameter_sets: Vec::new(),
            pending_bitrate: None,
            last_bitrate_pts: None,
        };
        let mut pixels = vec![128; 32 * 32 * 4];
        encoder.encode_bgra(&pixels, 128).unwrap();
        SOFTWARE_FRAME_ALLOCATIONS.with(|count| count.set(0));
        for index in 1..=16 {
            pixels.fill(index * 8);
            let output = encoder.encode_bgra(&pixels, 128).unwrap();
            assert_eq!(output.len(), 1);
            assert_eq!(output[0].pts, i64::from(index));
        }
        SOFTWARE_FRAME_ALLOCATIONS.with(|count| {
            assert_eq!(
                count.get(),
                0,
                "conversion buffers must survive encode calls"
            )
        });
    }

    #[test]
    fn reused_frame_detaches_from_retained_native_reference() {
        ffmpeg::init().unwrap();
        let mut frame = allocate_software_frame(Pixel::NV12, 32, 32);
        frame.data_mut(0).fill(29);
        frame.data_mut(1).fill(71);
        let mut retained = frame::Video::empty();
        // SAFETY: Both wrappers own live AVFrames. av_frame_ref creates shared
        // native ownership; each wrapper independently unreferences it on drop.
        let status = unsafe { ffmpeg::ffi::av_frame_ref(retained.as_mut_ptr(), frame.as_ptr()) };
        assert_eq!(status, 0);
        let original = frame.data(0).as_ptr();
        assert_eq!(retained.data(0).as_ptr(), original);
        make_frame_writable(&mut frame).unwrap();
        assert_ne!(frame.data(0).as_ptr(), original);
        frame.data_mut(0).fill(200);
        frame.data_mut(1).fill(100);
        assert!(retained.data(0).iter().all(|byte| *byte == 29));
        assert!(retained.data(1).iter().all(|byte| *byte == 71));
        let unique = frame.data(0).as_ptr();
        make_frame_writable(&mut frame).unwrap();
        assert_eq!(frame.data(0).as_ptr(), unique);
    }

    #[test]
    fn packet_timestamp_requires_identity_and_preserves_present_pts() {
        let mut packet = Packet::empty();
        assert!(
            matches!(
                packet_timestamp(&packet),
                Err(EncodeError::MissingOutputTimestamp)
            ),
            "missing output PTS must not invent input zero"
        );
        for pts in [0, 1, -7, i64::MAX] {
            packet.set_pts(Some(pts));
            assert_eq!(packet_timestamp(&packet).unwrap(), pts);
        }
    }

    #[test]
    fn forced_key_frame_is_an_idr() {
        ffmpeg::init().unwrap();
        let config = EncoderConfig {
            width: 32,
            height: 32,
            bitrate: 100_000,
            fps: 30,
            keyframe_interval: 300,
            preferred_codec: VideoCodec::H264,
        };
        let mut encoder = LinuxVideoEncoder {
            config,
            open: open_openh264(config).unwrap(),
            next_pts: 0,
            force_keyframe: true,
            parameter_sets: Vec::new(),
            pending_bitrate: None,
            last_bitrate_pts: None,
        };
        let pixels = vec![128; 32 * 32 * 4];
        encoder.encode_bgra(&pixels, 128).unwrap();
        encoder.encode_bgra(&pixels, 128).unwrap();
        encoder.force_key_frame();
        let output = encoder.encode_bgra(&pixels, 128).unwrap();
        assert!(
            output.iter().any(|frame| parse_nal_units(&frame.data)
                .unwrap()
                .iter()
                .any(|nalu| nalu[0] & 31 == 5)),
            "forced I picture must contain an IDR NAL"
        );
    }

    #[test]
    fn converts_annex_b_to_avcc_without_padding() {
        let annex_b = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3];
        let nalus = parse_nal_units(&annex_b).unwrap();
        let mut output = Vec::new();
        for nalu in nalus {
            append_avcc_nalu(&mut output, nalu).unwrap();
        }
        assert_eq!(output, vec![0, 0, 0, 3, 0x67, 1, 2, 0, 0, 0, 2, 0x68, 3]);
    }

    #[test]
    fn detects_h264_and_hevc_parameter_sets() {
        let h264 = [vec![0x67, 1], vec![0x68, 2], vec![0x65, 3]];
        let h264_refs = h264.iter().map(Vec::as_slice).collect::<Vec<_>>();
        assert_eq!(
            extract_parameter_sets(VideoCodec::H264, &h264_refs).len(),
            2
        );

        let hevc = [
            vec![32 << 1, 1],
            vec![33 << 1, 2],
            vec![34 << 1, 3],
            vec![19 << 1, 4],
        ];
        let hevc_refs = hevc.iter().map(Vec::as_slice).collect::<Vec<_>>();
        assert_eq!(
            extract_parameter_sets(VideoCodec::Hevc, &hevc_refs).len(),
            3
        );
    }

    #[test]
    fn bitrate_reopens_codec_and_preserves_pts() {
        ffmpeg::init().unwrap();
        let config = EncoderConfig {
            width: 32,
            height: 32,
            bitrate: 100_000,
            fps: 30,
            keyframe_interval: 300,
            preferred_codec: VideoCodec::H264,
        };
        let mut encoder = LinuxVideoEncoder {
            config,
            open: open_openh264(config).unwrap(),
            next_pts: 0,
            force_keyframe: true,
            parameter_sets: Vec::new(),
            pending_bitrate: None,
            last_bitrate_pts: None,
        };
        let pixels = vec![128; 32 * 32 * 4];
        encoder.encode_bgra(&pixels, 128).unwrap();
        let old_context = unsafe { encoder.open.encoder.as_ptr() };
        encoder.config.keyframe_interval = 1;
        encoder.update_bitrate(200_000).unwrap();
        assert_ne!(
            unsafe { encoder.open.encoder.as_ptr() },
            old_context,
            "post-open setters do not reconfigure every backend"
        );
        assert_eq!(encoder.config.bitrate, 200_000);
        let output = encoder.encode_bgra(&pixels, 128).unwrap();
        assert_eq!(output[0].pts, 1);
        assert!(output[0].is_key_frame);
        assert!(parse_nal_units(&output[0].data)
            .unwrap()
            .iter()
            .any(|n| n[0] & 31 == 5));
        assert!(encoder.update_bitrate(0).is_err());
        assert_eq!(encoder.config.bitrate, 200_000);
    }

    #[test]
    fn on_demand_gop_reopens_without_boundary_and_emits_idr() {
        ffmpeg::init().unwrap();
        let config = EncoderConfig {
            width: 32,
            height: 32,
            bitrate: 100_000,
            fps: 30,
            keyframe_interval: 0,
            preferred_codec: VideoCodec::H264,
        };
        let mut encoder = LinuxVideoEncoder {
            config,
            open: open_openh264(config).unwrap(),
            next_pts: 0,
            force_keyframe: true,
            parameter_sets: Vec::new(),
            pending_bitrate: None,
            last_bitrate_pts: None,
        };
        let pixels = vec![128; 32 * 32 * 4];
        encoder.encode_bgra(&pixels, 128).unwrap();
        encoder.update_bitrate(200_000).unwrap();
        assert_eq!(encoder.config.bitrate, 200_000);
        let output = encoder.encode_bgra(&pixels, 128).unwrap();
        assert_eq!(output[0].pts, 1);
        assert!(output[0].is_key_frame);
        assert!(parse_nal_units(&output[0].data)
            .unwrap()
            .iter()
            .any(|n| n[0] & 31 == 5));
    }

    #[test]
    fn bitrate_feedback_coalesces_until_gop_boundary() {
        ffmpeg::init().unwrap();
        let config = EncoderConfig {
            width: 32,
            height: 32,
            bitrate: 100_000,
            fps: 30,
            keyframe_interval: 3,
            preferred_codec: VideoCodec::H264,
        };
        let mut encoder = LinuxVideoEncoder {
            config,
            open: open_openh264(config).unwrap(),
            next_pts: 0,
            force_keyframe: true,
            parameter_sets: Vec::new(),
            pending_bitrate: None,
            last_bitrate_pts: None,
        };
        let pixels = vec![128; 32 * 32 * 4];
        encoder.encode_bgra(&pixels, 128).unwrap();
        encoder.update_bitrate(200_000).unwrap();
        encoder.update_bitrate(300_000).unwrap();
        assert_eq!(encoder.config.bitrate, 100_000);
        for pts in [1, 2] {
            let packets = encoder.encode_bgra(&pixels, 128).unwrap();
            assert_eq!(packets[0].pts, pts);
            assert!(!packets[0].is_key_frame);
        }
        let packets = encoder.encode_bgra(&pixels, 128).unwrap();
        assert_eq!(encoder.config.bitrate, 300_000);
        assert_eq!(packets[0].pts, 3);
        assert!(packets[0].is_key_frame);
    }

    #[test]
    fn full_range_bt601_coefficients() {
        // Pure black: R=0, G=0, B=0
        let (r, g, b) = (0, 0, 0);
        let y_black = ((77 * r + 150 * g + 29 * b + 128) >> 8).clamp(0, 255) as u8;
        let u_black = (128 + ((-43 * r - 85 * g + 128 * b + 128) >> 8)).clamp(0, 255) as u8;
        let v_black = (128 + ((128 * r - 107 * g - 21 * b + 128) >> 8)).clamp(0, 255) as u8;
        assert_eq!(y_black, 0, "black must map to Y=0 in full range BT.601");
        assert_eq!(u_black, 128, "black chroma U must be 128");
        assert_eq!(v_black, 128, "black chroma V must be 128");

        // Pure white: R=255, G=255, B=255
        let (r, g, b) = (255, 255, 255);
        let y_white = ((77 * r + 150 * g + 29 * b + 128) >> 8).clamp(0, 255) as u8;
        let u_white = (128 + ((-43 * r - 85 * g + 128 * b + 128) >> 8)).clamp(0, 255) as u8;
        let v_white = (128 + ((128 * r - 107 * g - 21 * b + 128) >> 8)).clamp(0, 255) as u8;
        assert_eq!(y_white, 255, "white must map to Y=255 in full range BT.601");
        assert_eq!(u_white, 128, "white chroma U must be 128");
        assert_eq!(v_white, 128, "white chroma V must be 128");

        // Neutral gray: R=128, G=128, B=128
        let (r, g, b) = (128, 128, 128);
        let y_gray = ((77 * r + 150 * g + 29 * b + 128) >> 8).clamp(0, 255) as u8;
        let u_gray = (128 + ((-43 * r - 85 * g + 128 * b + 128) >> 8)).clamp(0, 255) as u8;
        let v_gray = (128 + ((128 * r - 107 * g - 21 * b + 128) >> 8)).clamp(0, 255) as u8;
        assert_eq!(y_gray, 128);
        assert_eq!(u_gray, 128);
        assert_eq!(v_gray, 128);
    }

    #[test]
    fn accepts_existing_avcc() {
        let avcc = [0, 0, 0, 2, 0x65, 1, 0, 0, 0, 1, 0x41];
        assert_eq!(
            parse_nal_units(&avcc).unwrap(),
            vec![&avcc[4..6], &avcc[10..11]]
        );
    }
}
