//! FFmpeg HEVC decoding for the MahoRD AVCC media stream.

pub mod audio_opus;

use thiserror::Error;

pub const NAL_LENGTH_BYTES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareAcceleration {
    Software,
    #[cfg(any(feature = "videotoolbox", feature = "ios-videotoolbox"))]
    VideoToolbox,
    #[cfg(feature = "d3d11va")]
    D3d11va,
    #[cfg(feature = "vaapi")]
    Vaapi,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nalu<'a> {
    pub nal_type: u8,
    pub data: &'a [u8],
}

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("HEVC access unit is truncated at byte {offset}")]
    TruncatedNalu { offset: usize },
    #[error("HEVC NALU at byte {offset} has zero length")]
    EmptyNalu { offset: usize },
    #[error("HEVC parameter sets (VPS/SPS/PPS) are incomplete")]
    MissingParameterSets,
    #[error("FFmpeg support is disabled")]
    FfmpegDisabled,
    #[error("FFmpeg initialization failed: {0}")]
    FfmpegInit(String),
    #[error("FFmpeg decoder failed: {0}")]
    Ffmpeg(String),
    #[error("decoded frame has unsupported dimensions or format")]
    UnsupportedFrame,
    #[error("VideoToolbox support is disabled")]
    VideoToolboxDisabled,
    #[error("VideoToolbox initialization failed: {0}")]
    VideoToolboxInit(String),
    #[error("VideoToolbox decoder failed: {0}")]
    VideoToolbox(String),
}

/// Parses one access unit containing 4-byte big-endian length-prefixed HEVC NALUs.
pub fn parse_length_prefixed_nalus(access_unit: &[u8]) -> Result<Vec<Nalu<'_>>, DecodeError> {
    let mut offset = 0;
    let mut nalus = Vec::new();
    while offset < access_unit.len() {
        if access_unit.len() - offset < NAL_LENGTH_BYTES {
            return Err(DecodeError::TruncatedNalu { offset });
        }
        let length = u32::from_be_bytes(
            access_unit[offset..offset + NAL_LENGTH_BYTES]
                .try_into()
                .expect("four-byte NAL length"),
        ) as usize;
        let length_offset = offset;
        offset += NAL_LENGTH_BYTES;
        if length < 2 {
            return Err(DecodeError::EmptyNalu {
                offset: length_offset,
            });
        }
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= access_unit.len())
            .ok_or(DecodeError::TruncatedNalu {
                offset: length_offset,
            })?;
        let data = &access_unit[offset..end];
        nalus.push(Nalu {
            nal_type: (data[0] >> 1) & 0x3f,
            data,
        });
        offset = end;
    }
    if nalus.is_empty() {
        return Err(DecodeError::TruncatedNalu { offset: 0 });
    }
    Ok(nalus)
}

/// Converts length-prefixed NALUs to Annex-B (0x00, 0x00, 0x00, 0x01 prefixed).
pub fn to_annex_b(data: &[u8]) -> Vec<u8> {
    prepare_annex_b(data).unwrap_or_else(|_| data.to_vec())
}

fn prepare_annex_b(data: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let nalus = parse_length_prefixed_nalus(data)?;
    // Four-byte start codes replace four-byte lengths without changing size.
    let mut out = Vec::with_capacity(data.len());
    for nalu in nalus {
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nalu.data);
    }
    Ok(out)
}

/// Extracts the direct AVCC-style VPS/SPS/PPS blob expected by the decoder context.
pub fn hevc_parameter_set_blob(access_unit: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let mut parameter_sets: [Option<&[u8]>; 3] = [None, None, None];
    for nalu in parse_length_prefixed_nalus(access_unit)? {
        match nalu.nal_type {
            32 => parameter_sets[0] = Some(nalu.data),
            33 => parameter_sets[1] = Some(nalu.data),
            34 => parameter_sets[2] = Some(nalu.data),
            _ => {}
        }
    }
    if parameter_sets.iter().any(Option::is_none) {
        return Err(DecodeError::MissingParameterSets);
    }
    let mut output = Vec::new();
    for parameter_set in parameter_sets.into_iter().flatten() {
        output.extend_from_slice(&(parameter_set.len() as u32).to_be_bytes());
        output.extend_from_slice(parameter_set);
    }
    Ok(output)
}

/// H.264 analog of [`hevc_parameter_set_blob`]: SPS (NAL type 7) then PPS (8).
pub fn h264_parameter_set_blob(access_unit: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let mut parameter_sets: [Option<&[u8]>; 2] = [None, None];
    for nalu in parse_length_prefixed_nalus(access_unit)? {
        // The shared parser reports HEVC-style nal_type; H.264 lives in the
        // low 5 bits of the header byte instead.
        let h264_type = nalu.data.first().map(|byte| byte & 0x1f).unwrap_or(0);
        match h264_type {
            7 => parameter_sets[0] = Some(nalu.data),
            8 => parameter_sets[1] = Some(nalu.data),
            _ => {}
        }
    }
    if parameter_sets.iter().any(Option::is_none) {
        return Err(DecodeError::MissingParameterSets);
    }
    let mut output = Vec::new();
    for parameter_set in parameter_sets.into_iter().flatten() {
        output.extend_from_slice(&(parameter_set.len() as u32).to_be_bytes());
        output.extend_from_slice(parameter_set);
    }
    Ok(output)
}

/// Which video codec an access unit carries, inferred from its parameter-set
/// NAL units. The wire protocol does not carry a codec field, so the decoder
/// sniffs the first keyframe instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecKind {
    Hevc,
    H264,
}

/// Classifies the first parameter-set NAL of an access unit. H.264 SPS has
/// nal_type 7 (mask 0x1F) while HEVC VPS/SPS/PPS are 32/33/34 after the
/// 1-bit-zero + 6-bit-type shift.
pub fn detect_codec(access_unit: &[u8]) -> Result<CodecKind, DecodeError> {
    for nalu in parse_length_prefixed_nalus(access_unit)? {
        if nalu.data.is_empty() {
            continue;
        }
        let first = nalu.data[0];
        let h264_type = first & 0x1F;
        let hevc_type = (first >> 1) & 0x3F;
        if h264_type == 7 {
            return Ok(CodecKind::H264);
        }
        if hevc_type == 32 || hevc_type == 33 || hevc_type == 34 {
            return Ok(CodecKind::Hevc);
        }
    }
    Err(DecodeError::MissingParameterSets)
}

/// Packed IPC layout: width, height, timestamp (16 bytes), followed by tight NV12.
#[derive(Debug)]
pub struct PackedNv12Frame {
    pub width: u32,
    pub height: u32,
    pub timestamp_ms: i64,
    pub buffer: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nv12Frame {
    pub width: u32,
    pub height: u32,
    pub y_stride: usize,
    pub uv_stride: usize,
    pub y_plane: Vec<u8>,
    pub uv_plane: Vec<u8>,
    pub timestamp_ms: i64,
}

#[cfg(feature = "ffmpeg")]
mod ffmpeg_impl {
    use std::ptr;

    use ffmpeg::{codec, format::Pixel, frame, software::scaling};
    use ffmpeg_next as ffmpeg;

    use super::{
        hevc_parameter_set_blob, parse_length_prefixed_nalus, to_annex_b, DecodeError,
        HardwareAcceleration, Nv12Frame,
    };

    pub struct HevcDecoder {
        decoder: codec::decoder::Video,
        scaler: Option<scaling::Context>,
        scaler_full_range: bool,
        acceleration: HardwareAcceleration,
    }

    unsafe impl Send for HevcDecoder {}

    impl HevcDecoder {
        pub fn new(extradata: &[u8]) -> Result<Self, DecodeError> {
            Self::with_codec(
                extradata,
                codec::Id::HEVC,
                "HEVC decoder is unavailable",
                true,
            )
        }

        /// H.264 variant for hosts whose Media Foundation pipeline emits H.264
        /// (e.g. the Windows software encoder MFT). Parameter sets are SPS/PPS.
        pub fn new_h264(extradata: &[u8]) -> Result<Self, DecodeError> {
            Self::with_codec(
                extradata,
                codec::Id::H264,
                "H264 decoder is unavailable",
                true,
            )
        }

        /// Explicit CPU-only fallback, independent of compiled hardware features.
        pub fn new_software(extradata: &[u8]) -> Result<Self, DecodeError> {
            Self::with_codec(
                extradata,
                codec::Id::HEVC,
                "HEVC decoder is unavailable",
                false,
            )
        }

        fn with_codec(
            extradata: &[u8],
            codec_id: codec::Id,
            missing_message: &str,
            hardware: bool,
        ) -> Result<Self, DecodeError> {
            ffmpeg::init().map_err(|error| DecodeError::FfmpegInit(error.to_string()))?;
            if extradata.is_empty() {
                return Err(DecodeError::MissingParameterSets);
            }
            let codec = codec::decoder::find(codec_id)
                .ok_or_else(|| DecodeError::Ffmpeg(missing_message.to_owned()))?;
            let mut context = codec::Context::new_with_codec(codec);
            unsafe {
                (*context.as_mut_ptr()).err_recognition =
                    ffmpeg::ffi::AV_EF_CRCCHECK | ffmpeg::ffi::AV_EF_BUFFER;
                (*context.as_mut_ptr()).flags |= ffmpeg::ffi::AV_CODEC_FLAG_LOW_DELAY as i32;
                (*context.as_mut_ptr()).flags2 |= ffmpeg::ffi::AV_CODEC_FLAG2_FAST;
                // Disable multi-frame threading (FF_THREAD_FRAME introduces 1-frame latency per thread).
                // Use slice-level threading (FF_THREAD_SLICE = 2) for zero latency.
                (*context.as_mut_ptr()).thread_type = ffmpeg::ffi::FF_THREAD_SLICE;
                (*context.as_mut_ptr()).thread_count = 4;
            }
            let annex_b = to_annex_b(extradata);
            let raw_len = annex_b.len();
            let padded_len = raw_len + ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize;
            let buffer = unsafe { ffmpeg::ffi::av_mallocz(padded_len) as *mut u8 };
            if buffer.is_null() {
                return Err(DecodeError::Ffmpeg(
                    "av_mallocz failed for extradata".to_owned(),
                ));
            }
            unsafe {
                ptr::copy_nonoverlapping(annex_b.as_ptr(), buffer, raw_len);
                (*context.as_mut_ptr()).extradata = buffer;
                (*context.as_mut_ptr()).extradata_size = raw_len as i32;
            }
            if hardware {
                configure_hardware(&mut context);
            }
            let decoder = match context.decoder().video() {
                Ok(decoder) => decoder,
                Err(error) if hardware => {
                    tracing::warn!(%error, "hardware decoder open failed; retrying software");
                    return Self::with_codec(extradata, codec_id, missing_message, false);
                }
                Err(error) => return Err(DecodeError::Ffmpeg(error.to_string())),
            };
            Ok(Self {
                decoder,
                scaler: None,
                scaler_full_range: false,
                acceleration: HardwareAcceleration::Software,
            })
        }

        pub fn from_keyframe(keyframe: &[u8]) -> Result<Self, DecodeError> {
            Self::new(&hevc_parameter_set_blob(keyframe)?)
        }

        /// Builds a decoder from the first keyframe, sniffing H.264 vs HEVC
        /// from its parameter-set NAL units. Windows hosts emit H.264; macOS
        /// hosts emit HEVC.
        pub fn from_keyframe_auto(
            keyframe: &[u8],
        ) -> Result<(super::CodecKind, Self), DecodeError> {
            let kind = super::detect_codec(keyframe)?;
            let decoder = match kind {
                super::CodecKind::Hevc => Self::new(&super::hevc_parameter_set_blob(keyframe)?),
                super::CodecKind::H264 => {
                    Self::new_h264(&super::h264_parameter_set_blob(keyframe)?)
                }
            }?;
            Ok((kind, decoder))
        }

        /// Backend of the most recently received frame (Software until the first frame).
        pub fn acceleration(&self) -> HardwareAcceleration {
            self.acceleration
        }

        /// Feeds one complete 4-byte length-prefixed access unit to FFmpeg.
        pub fn decode(
            &mut self,
            access_unit: &[u8],
            timestamp_ms: i64,
        ) -> Result<Vec<Nv12Frame>, DecodeError> {
            self.decode_into(access_unit, timestamp_ms, copy_nv12)
        }

        /// Decode directly into caller-owned packed storage, without plane vectors.
        pub fn decode_packed(
            &mut self,
            access_unit: &[u8],
            timestamp_ms: i64,
            mut acquire: impl FnMut(usize) -> Vec<u8>,
        ) -> Result<Vec<super::PackedNv12Frame>, DecodeError> {
            self.decode_into(access_unit, timestamp_ms, |source, timestamp| {
                pack_nv12(source, timestamp, &mut acquire)
            })
        }

        fn decode_into<T>(
            &mut self,
            access_unit: &[u8],
            timestamp_ms: i64,
            consume: impl FnMut(&frame::Video, i64) -> Result<T, DecodeError>,
        ) -> Result<Vec<T>, DecodeError> {
            let nalus = parse_length_prefixed_nalus(access_unit)?;
            let mut packet = ffmpeg::Packet::new(access_unit.len());
            let output = packet.data_mut().expect("nonempty packet has storage");
            let mut offset = 0;
            for nalu in nalus {
                output[offset..offset + 4].copy_from_slice(&[0, 0, 0, 1]);
                offset += 4;
                output[offset..offset + nalu.data.len()].copy_from_slice(nalu.data);
                offset += nalu.data.len();
            }
            packet.set_pts(Some(timestamp_ms));
            packet.set_dts(Some(timestamp_ms));
            self.decoder
                .send_packet(&packet)
                .map_err(|error| DecodeError::Ffmpeg(error.to_string()))?;
            self.receive_into(timestamp_ms, consume)
        }

        pub fn flush(&mut self) -> Result<Vec<Nv12Frame>, DecodeError> {
            self.decoder
                .send_eof()
                .map_err(|error| DecodeError::Ffmpeg(error.to_string()))?;
            self.receive_into(0, copy_nv12)
        }

        fn receive_into<T>(
            &mut self,
            fallback_timestamp_ms: i64,
            mut consume: impl FnMut(&frame::Video, i64) -> Result<T, DecodeError>,
        ) -> Result<Vec<T>, DecodeError> {
            let mut frames = Vec::new();
            loop {
                let mut decoded = frame::Video::empty();
                match self.decoder.receive_frame(&mut decoded) {
                    Ok(()) => {
                        let timestamp_ms = decoded
                            .timestamp()
                            .or_else(|| decoded.pts())
                            .unwrap_or(fallback_timestamp_ms);
                        self.acceleration = decoded_backend(decoded.format());
                        if matches!(
                            decoded.format(),
                            Pixel::VIDEOTOOLBOX | Pixel::D3D11 | Pixel::D3D11VA_VLD | Pixel::VAAPI
                        ) {
                            let decoded = transfer_hardware_frame(&decoded)?;
                            frames.push(self.convert_to_nv12(
                                &decoded,
                                timestamp_ms,
                                &mut consume,
                            )?);
                        } else {
                            frames.push(self.convert_to_nv12(
                                &decoded,
                                timestamp_ms,
                                &mut consume,
                            )?);
                        }
                    }
                    Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => break,
                    Err(ffmpeg::Error::Eof) => break,
                    Err(error) => return Err(DecodeError::Ffmpeg(error.to_string())),
                }
            }
            Ok(frames)
        }

        fn convert_to_nv12<T>(
            &mut self,
            decoded: &frame::Video,
            timestamp_ms: i64,
            consume: &mut impl FnMut(&frame::Video, i64) -> Result<T, DecodeError>,
        ) -> Result<T, DecodeError> {
            let converted;
            let source = if decoded.format() == Pixel::NV12 {
                decoded
            } else {
                let full_range = is_full_range(decoded);

                let needs_scaler = self.scaler.as_ref().map_or(true, |scaler| {
                    scaler.input().format != decoded.format()
                        || scaler.input().width != decoded.width()
                        || scaler.input().height != decoded.height()
                        || self.scaler_full_range != full_range
                });
                if needs_scaler {
                    self.scaler = Some(nv12_scaler(
                        decoded.format(),
                        decoded.width(),
                        decoded.height(),
                        full_range,
                    )?);
                    self.scaler_full_range = full_range;
                }
                converted = {
                    let mut converted = frame::Video::empty();
                    self.scaler
                        .as_mut()
                        .expect("scaler initialized above")
                        .run(decoded, &mut converted)
                        .map_err(|error| DecodeError::Ffmpeg(error.to_string()))?;
                    converted
                };
                &converted
            };
            consume(source, timestamp_ms)
        }
    }

    pub(super) fn is_full_range(frame: &frame::Video) -> bool {
        matches!(
            frame.format(),
            Pixel::YUVJ420P | Pixel::YUVJ422P | Pixel::YUVJ444P
        ) || frame.color_range() == ffmpeg::util::color::Range::JPEG
    }

    pub(super) fn nv12_scaler(
        format: Pixel,
        width: u32,
        height: u32,
        full_range: bool,
    ) -> Result<scaling::Context, DecodeError> {
        let mut context = scaling::Context::get(
            format,
            width,
            height,
            Pixel::NV12,
            width,
            height,
            scaling::Flags::FAST_BILINEAR,
        )
        .map_err(|error| DecodeError::Ffmpeg(error.to_string()))?;

        if full_range {
            // SAFETY: `context.as_mut_ptr()` yields the valid, initialized SwsContext.
            // We query the current colorspace details, then re-apply them setting both
            // srcRange and dstRange to 1 (full range) so swscale does not compress luma
            // from 0..255 into 16..235 when converting full-range YUV to NV12.
            unsafe {
                let sws_ctx = context.as_mut_ptr();
                let mut inv_table: *mut std::ffi::c_int = ptr::null_mut();
                let mut table: *mut std::ffi::c_int = ptr::null_mut();
                let mut src_range: std::ffi::c_int = 0;
                let mut dst_range: std::ffi::c_int = 0;
                let mut brightness: std::ffi::c_int = 0;
                let mut contrast: std::ffi::c_int = 0;
                let mut saturation: std::ffi::c_int = 0;

                let ret = ffmpeg::ffi::sws_getColorspaceDetails(
                    sws_ctx,
                    &mut inv_table,
                    &mut src_range,
                    &mut table,
                    &mut dst_range,
                    &mut brightness,
                    &mut contrast,
                    &mut saturation,
                );
                if ret >= 0 {
                    ffmpeg::ffi::sws_setColorspaceDetails(
                        sws_ctx, inv_table, 1, table, 1, brightness, contrast, saturation,
                    );
                }
            }
        }

        Ok(context)
    }

    fn transfer_hardware_frame(decoded: &frame::Video) -> Result<frame::Video, DecodeError> {
        let mut software = frame::Video::empty();
        let result = unsafe {
            ffmpeg::ffi::av_hwframe_transfer_data(software.as_mut_ptr(), decoded.as_ptr(), 0)
        };
        if result < 0 {
            return Err(DecodeError::Ffmpeg(ffmpeg::Error::from(result).to_string()));
        }
        Ok(software)
    }

    fn pack_nv12(
        frame: &frame::Video,
        timestamp_ms: i64,
        acquire: &mut impl FnMut(usize) -> Vec<u8>,
    ) -> Result<super::PackedNv12Frame, DecodeError> {
        if frame.format() != Pixel::NV12 || frame.planes() < 2 {
            return Err(DecodeError::UnsupportedFrame);
        }
        let width = frame.width() as usize;
        let height = frame.height() as usize;
        let size = 16 + width * (height + height.div_ceil(2));
        for (plane, rows) in [(0, height), (1, height.div_ceil(2))] {
            if frame.stride(plane) < width || frame.data(plane).len() < frame.stride(plane) * rows {
                return Err(DecodeError::UnsupportedFrame);
            }
        }
        let mut buffer = acquire(size + 13);
        buffer.clear();
        buffer.extend_from_slice(&frame.width().to_le_bytes());
        buffer.extend_from_slice(&frame.height().to_le_bytes());
        buffer.extend_from_slice(&timestamp_ms.to_le_bytes());
        for (plane, rows) in [(0, height), (1, height.div_ceil(2))] {
            for row in 0..rows {
                let start = row * frame.stride(plane);
                buffer.extend_from_slice(&frame.data(plane)[start..start + width]);
            }
        }
        Ok(super::PackedNv12Frame {
            width: frame.width(),
            height: frame.height(),
            timestamp_ms,
            buffer,
        })
    }

    pub(super) fn copy_nv12(
        frame: &frame::Video,
        timestamp_ms: i64,
    ) -> Result<Nv12Frame, DecodeError> {
        if frame.format() != Pixel::NV12 || frame.planes() < 2 {
            return Err(DecodeError::UnsupportedFrame);
        }
        let width = frame.width() as usize;
        let height = frame.height() as usize;
        let y_stride = frame.stride(0);
        let uv_stride = frame.stride(1);
        if y_stride < width
            || uv_stride < width
            || frame.data(0).len() < y_stride * height
            || frame.data(1).len() < uv_stride * height.div_ceil(2)
        {
            return Err(DecodeError::UnsupportedFrame);
        }
        let mut y_plane = vec![0_u8; width * height];
        let mut uv_plane = vec![0_u8; width * height.div_ceil(2)];
        for row in 0..height {
            let source = &frame.data(0)[row * y_stride..row * y_stride + width];
            y_plane[row * width..(row + 1) * width].copy_from_slice(source);
        }
        for row in 0..height.div_ceil(2) {
            let source = &frame.data(1)[row * uv_stride..row * uv_stride + width];
            uv_plane[row * width..(row + 1) * width].copy_from_slice(source);
        }
        Ok(Nv12Frame {
            width: width as u32,
            height: height as u32,
            y_stride: width,
            uv_stride: width,
            y_plane,
            uv_plane,
            timestamp_ms,
        })
    }

    fn decoded_backend(format: Pixel) -> HardwareAcceleration {
        match format {
            #[cfg(feature = "videotoolbox")]
            Pixel::VIDEOTOOLBOX => HardwareAcceleration::VideoToolbox,
            #[cfg(feature = "d3d11va")]
            Pixel::D3D11 | Pixel::D3D11VA_VLD => HardwareAcceleration::D3d11va,
            #[cfg(feature = "vaapi")]
            Pixel::VAAPI => HardwareAcceleration::Vaapi,
            _ => HardwareAcceleration::Software,
        }
    }

    fn configure_hardware(_context: &mut codec::Context) {
        #[cfg(all(feature = "videotoolbox", target_os = "macos"))]
        create_hw_device(
            _context,
            ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
        );
        #[cfg(all(feature = "d3d11va", target_os = "windows"))]
        create_hw_device(
            _context,
            ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
        );
        #[cfg(all(feature = "vaapi", target_os = "linux"))]
        create_hw_device(
            _context,
            ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
        );
    }

    #[cfg(any(
        all(feature = "videotoolbox", target_os = "macos"),
        all(feature = "d3d11va", target_os = "windows"),
        all(feature = "vaapi", target_os = "linux")
    ))]
    fn create_hw_device(context: &mut codec::Context, device_type: ffmpeg::ffi::AVHWDeviceType) {
        // The codec context owns this reference, including on decoder-open failure.
        // VAAPI uses FFmpeg's default render device selection (DRM/X11).
        unsafe {
            let context = context.as_mut_ptr();
            let result = ffmpeg::ffi::av_hwdevice_ctx_create(
                &mut (*context).hw_device_ctx,
                device_type,
                ptr::null(),
                ptr::null_mut(),
                0,
            );
            if result >= 0 {
                (*context).get_format = Some(select_hardware_format);
            } else {
                tracing::warn!(?device_type, error = %ffmpeg::Error::from(result),
                    "hardware decode device unavailable; using software");
            }
        }
    }

    #[cfg(any(
        all(feature = "videotoolbox", target_os = "macos"),
        all(feature = "d3d11va", target_os = "windows"),
        all(feature = "vaapi", target_os = "linux")
    ))]
    unsafe extern "C" fn select_hardware_format(
        context: *mut ffmpeg::ffi::AVCodecContext,
        formats: *const ffmpeg::ffi::AVPixelFormat,
    ) -> ffmpeg::ffi::AVPixelFormat {
        use ffmpeg::ffi::*;
        // FFmpeg calls again without the failed format when hardware setup fails.
        // Choose only a device-compatible hardware format, otherwise a CPU format.
        let device = (*(*context).hw_device_ctx).data as *const AVHWDeviceContext;
        let preferred = match (*device).type_ {
            AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX => AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX,
            AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA => AVPixelFormat::AV_PIX_FMT_D3D11,
            AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI => AVPixelFormat::AV_PIX_FMT_VAAPI,
            _ => AVPixelFormat::AV_PIX_FMT_NONE,
        };
        let mut current = formats;
        let mut software = AVPixelFormat::AV_PIX_FMT_NONE;
        while *current != AVPixelFormat::AV_PIX_FMT_NONE {
            if *current == preferred {
                return preferred;
            }
            let descriptor = av_pix_fmt_desc_get(*current);
            if software == AVPixelFormat::AV_PIX_FMT_NONE
                && !descriptor.is_null()
                && (*descriptor).flags & AV_PIX_FMT_FLAG_HWACCEL as u64 == 0
            {
                software = *current;
            }
            current = current.add(1);
        }
        software
    }

    #[cfg(all(
        test,
        any(
            all(feature = "videotoolbox", target_os = "macos"),
            all(feature = "d3d11va", target_os = "windows"),
            all(feature = "vaapi", target_os = "linux")
        )
    ))]
    #[test]
    fn hardware_format_negotiation_rejects_other_devices_and_falls_back() {
        use ffmpeg::ffi::*;
        unsafe {
            let mut context = codec::Context::new();
            (*context.as_mut_ptr()).hw_device_ctx =
                av_buffer_allocz(std::mem::size_of::<AVHWDeviceContext>());
            assert!(!(*context.as_ptr()).hw_device_ctx.is_null());
            let device = (*(*context.as_mut_ptr()).hw_device_ctx).data as *mut AVHWDeviceContext;
            (*device).type_ = AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI;
            let offered = [
                AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX,
                AVPixelFormat::AV_PIX_FMT_YUV420P,
                AVPixelFormat::AV_PIX_FMT_VAAPI,
                AVPixelFormat::AV_PIX_FMT_NONE,
            ];
            assert_eq!(
                select_hardware_format(context.as_mut_ptr(), offered.as_ptr()),
                AVPixelFormat::AV_PIX_FMT_VAAPI
            );
            // FFmpeg removes a failed hardware format and invokes get_format again.
            let fallback = [
                AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX,
                AVPixelFormat::AV_PIX_FMT_YUV420P,
                AVPixelFormat::AV_PIX_FMT_NONE,
            ];
            assert_eq!(
                select_hardware_format(context.as_mut_ptr(), fallback.as_ptr()),
                AVPixelFormat::AV_PIX_FMT_YUV420P
            );
            let unsupported = [
                AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX,
                AVPixelFormat::AV_PIX_FMT_NONE,
            ];
            assert_eq!(
                select_hardware_format(context.as_mut_ptr(), unsupported.as_ptr()),
                AVPixelFormat::AV_PIX_FMT_NONE
            );
        }
    }

    pub use HevcDecoder as ExportedHevcDecoder;
}

#[cfg(feature = "ffmpeg")]
pub use ffmpeg_impl::ExportedHevcDecoder as HevcDecoder;

#[cfg(all(
    not(feature = "ffmpeg"),
    feature = "ios-videotoolbox",
    any(target_os = "ios", target_os = "macos")
))]
mod vt;

#[cfg(all(
    not(feature = "ffmpeg"),
    feature = "ios-videotoolbox",
    any(target_os = "ios", target_os = "macos")
))]
pub use vt::HevcDecoder;

#[cfg(all(
    not(feature = "ffmpeg"),
    not(all(
        feature = "ios-videotoolbox",
        any(target_os = "ios", target_os = "macos")
    ))
))]
pub struct HevcDecoder;

#[cfg(all(
    not(feature = "ffmpeg"),
    not(all(
        feature = "ios-videotoolbox",
        any(target_os = "ios", target_os = "macos")
    ))
))]
impl HevcDecoder {
    pub fn new(_extradata: &[u8]) -> Result<Self, DecodeError> {
        Err(DecodeError::FfmpegDisabled)
    }

    pub fn new_h264(_extradata: &[u8]) -> Result<Self, DecodeError> {
        Err(DecodeError::FfmpegDisabled)
    }

    pub fn from_keyframe(_keyframe: &[u8]) -> Result<Self, DecodeError> {
        Err(DecodeError::FfmpegDisabled)
    }

    pub fn from_keyframe_auto(_keyframe: &[u8]) -> Result<(CodecKind, Self), DecodeError> {
        Err(DecodeError::FfmpegDisabled)
    }

    pub fn decode(
        &mut self,
        _access_unit: &[u8],
        _timestamp_ms: i64,
    ) -> Result<Vec<Nv12Frame>, DecodeError> {
        Err(DecodeError::FfmpegDisabled)
    }

    pub fn flush(&mut self) -> Result<Vec<Nv12Frame>, DecodeError> {
        Err(DecodeError::FfmpegDisabled)
    }

    pub fn acceleration(&self) -> HardwareAcceleration {
        HardwareAcceleration::Software
    }
}

#[cfg(all(test, feature = "ffmpeg"))]
mod hardware_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ffmpeg")]
    #[test]
    fn packed_decode_reuses_storage_and_matches_planes() {
        let access_unit: Vec<_> = include_str!("../tests/fixtures/black_16x16.hevc.hex")
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect();
        let mut decoder = HevcDecoder::new_software(&access_unit).unwrap();
        let expected = decoder.decode(&access_unit, 1).unwrap().remove(0);
        let mut storage = Vec::with_capacity(16 + 384 + 13);
        let address = storage.as_ptr();
        let packed = decoder
            .decode_packed(&access_unit, 2, |size| {
                assert!(size <= storage.capacity());
                std::mem::take(&mut storage)
            })
            .unwrap()
            .remove(0);
        assert_eq!(packed.buffer.as_ptr(), address);
        assert_eq!(&packed.buffer[16..272], expected.y_plane);
        assert_eq!(&packed.buffer[272..], expected.uv_plane);
        assert_eq!(packed.timestamp_ms, 2);
    }

    #[cfg(feature = "ffmpeg")]
    #[test]
    fn decoder_packet_avoids_intermediate_payload_copy() {
        let access_unit: Vec<_> = include_str!("../tests/fixtures/black_16x16.hevc.hex")
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect();
        let mut decoder = HevcDecoder::new_software(&access_unit).unwrap();
        assert_eq!(decoder.decode(&access_unit, 1).unwrap().len(), 1);
        let (frames, count) =
            crate::test_alloc::allocations(|| decoder.decode(&access_unit, 2).unwrap());
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        assert_eq!((frame.width, frame.height, frame.timestamp_ms), (16, 16, 2));
        for row in frame.y_plane.chunks(frame.y_stride).take(16) {
            assert_eq!(&row[..16], &[16; 16]);
        }
        for row in frame.uv_plane.chunks(frame.uv_stride).take(8) {
            assert_eq!(&row[..16], &[128; 16]);
        }
        eprintln!("warm decoded 16x16 HEVC Rust allocations: {count}");
        assert!(
            count <= 4,
            "decoded planes, output list and NAL metadata must not include a copied input Vec"
        );
    }

    #[cfg(feature = "ffmpeg")]
    #[test]
    fn copy_nv12_rejects_undersized_chroma_stride() {
        use ffmpeg_next::{format::Pixel, frame};
        // Given an NV12 frame whose chroma rows are narrower than the frame width.
        let mut frame = frame::Video::new(Pixel::NV12, 16, 16);
        assert!(crate::ffmpeg_impl::copy_nv12(&frame, 0).is_ok());
        unsafe {
            (*frame.as_mut_ptr()).linesize[1] = 8;
        }
        // When the FFmpeg path copies it.
        let result = crate::ffmpeg_impl::copy_nv12(&frame, 0);
        // Then the frame is rejected instead of panicking the decode thread.
        assert!(matches!(result, Err(DecodeError::UnsupportedFrame)));
    }

    #[cfg(feature = "ffmpeg")]
    #[test]
    fn full_range_yuvj420p_conversion_preserves_full_luma_range() {
        use ffmpeg_next::{format::Pixel, frame};

        // Given a 16x16 YUVJ420P frame with left half = 0 and right half = 255
        let width = 16;
        let height = 16;
        let mut frame = frame::Video::new(Pixel::YUVJ420P, width, height);

        let y_stride = frame.stride(0);
        let y_data = frame.data_mut(0);
        for y in 0..height as usize {
            let row_start = y * y_stride;
            for x in 0..width as usize {
                y_data[row_start + x] = if x < 8 { 0 } else { 255 };
            }
        }

        // Fill chroma U and V planes with 128 (neutral gray)
        let u_stride = frame.stride(1);
        let u_data = frame.data_mut(1);
        for y in 0..(height / 2) as usize {
            let row_start = y * u_stride;
            for x in 0..(width / 2) as usize {
                u_data[row_start + x] = 128;
            }
        }
        let v_stride = frame.stride(2);
        let v_data = frame.data_mut(2);
        for y in 0..(height / 2) as usize {
            let row_start = y * v_stride;
            for x in 0..(width / 2) as usize {
                v_data[row_start + x] = 128;
            }
        }

        let is_full = crate::ffmpeg_impl::is_full_range(&frame);
        assert!(is_full, "YUVJ420P must be detected as full range");

        let mut scaler =
            crate::ffmpeg_impl::nv12_scaler(frame.format(), width, height, is_full).unwrap();
        let mut converted = frame::Video::empty();
        scaler.run(&frame, &mut converted).unwrap();

        let nv12 = crate::ffmpeg_impl::copy_nv12(&converted, 0).unwrap();
        assert_eq!(nv12.width, width);
        assert_eq!(nv12.height, height);

        for y in 0..height as usize {
            let row = &nv12.y_plane[y * nv12.y_stride..y * nv12.y_stride + width as usize];
            for (x, &val) in row.iter().enumerate() {
                if x < 8 {
                    assert!(
                        val <= 1,
                        "left half luma must stay near 0 (got {val} at x={x}, y={y})"
                    );
                } else {
                    assert!(
                        val >= 254,
                        "right half luma must stay near 255 (got {val} at x={x}, y={y})"
                    );
                }
            }
        }
    }

    #[cfg(feature = "ffmpeg")]
    #[test]
    fn decoder_initialization_with_corrupt_extradata_cleans_up_safely() {
        // Given random non-parameter bytes passed as extradata
        let bogus = [0xde, 0xad, 0xbe, 0xef, 0x00, 0x11, 0x22, 0x33];
        // When HevcDecoder is initialized
        let decoder = HevcDecoder::new(&bogus);
        // It cleans up safely without allocator mismatch or double free
        drop(decoder);
    }

    #[test]
    fn annex_b_preserves_payloads_when_replacing_length_prefixes() {
        // Given multiple NALs, including an emulation-prevention sequence.
        let data = [0, 0, 0, 5, 64, 1, 0, 0, 3, 0, 0, 0, 2, 103, 2];
        let expected = [0, 0, 0, 1, 64, 1, 0, 0, 3, 0, 0, 0, 1, 103, 2];
        // When strict NAL normalization converts the access unit.
        let prepared = prepare_annex_b(&data).unwrap();
        // Then only the four-byte prefixes change; public normalization agrees.
        assert_eq!(prepared, expected);
        assert_eq!(prepared.capacity(), data.len());
        assert_eq!(to_annex_b(&data), expected);
    }

    #[test]
    fn parser_borrows_payloads_when_access_unit_has_multiple_nals() {
        // Given two distinct HEVC NAL headers.
        let data = [0, 0, 0, 2, 64, 1, 0, 0, 0, 2, 66, 2];
        // When the public parser returns its NAL metadata.
        let nalus = parse_length_prefixed_nalus(&data).unwrap();
        // Then NAL types and payload slices retain the public borrowed contract.
        assert_eq!(nalus.len(), 2);
        assert_eq!((nalus[0].nal_type, nalus[1].nal_type), (32, 33));
        assert_eq!(nalus[0].data, &data[4..6]);
        assert_eq!(nalus[1].data, &data[10..12]);
        assert_eq!(nalus[0].data.as_ptr(), data[4..].as_ptr());
        assert_eq!(nalus[1].data.as_ptr(), data[10..].as_ptr());
    }

    #[test]
    fn strict_errors_preserve_offsets_when_normalization_falls_back() {
        // Given empty, short-prefix, short-NAL, oversized and trailing garbage inputs.
        let cases: &[(&[u8], bool, usize)] = &[
            (&[], false, 0),
            (&[0], false, 0),
            (&[0, 0, 0], false, 0),
            (&[0, 0, 0, 0], true, 0),
            (&[0, 0, 0, 1, 64], true, 0),
            (&[0, 0, 0, 8, 64, 1], false, 0),
            (&[255, 255, 255, 255], false, 0),
            (&[0, 0, 0, 2, 64, 1, 0], false, 6),
        ];
        // When strict preparation receives malformed lengths.
        for &(data, empty, offset) in cases {
            // Then errors retain their type/offset; public normalization falls back.
            for error in [
                prepare_annex_b(data).unwrap_err(),
                parse_length_prefixed_nalus(data).unwrap_err(),
            ] {
                match error {
                    DecodeError::EmptyNalu { offset: actual } => {
                        assert!(empty);
                        assert_eq!(actual, offset);
                    }
                    DecodeError::TruncatedNalu { offset: actual } => {
                        assert!(!empty);
                        assert_eq!(actual, offset);
                    }
                    other => panic!("unexpected error: {other:?}"),
                }
            }
            assert_eq!(to_annex_b(data), data);
        }
    }

    #[test]
    fn corrupt_length_prefix_is_a_graceful_error() {
        assert!(matches!(
            parse_length_prefixed_nalus(&[0, 0, 0, 8, 1, 2]),
            Err(DecodeError::TruncatedNalu { offset: 0 })
        ));
    }

    #[test]
    fn parameter_set_blob_keeps_four_byte_lengths() {
        let mut keyframe = Vec::new();
        for data in [[0x40, 1], [0x42, 2], [0x44, 3]] {
            keyframe.extend_from_slice(&(data.len() as u32).to_be_bytes());
            keyframe.extend_from_slice(&data);
        }
        assert_eq!(hevc_parameter_set_blob(&keyframe).unwrap(), keyframe);
    }

    #[test]
    fn h264_parameter_set_blob_extracts_sps_and_pps() {
        let mut keyframe = Vec::new();
        let sps = [0x67, 0x42, 0xc0, 0x0a];
        keyframe.extend_from_slice(&(sps.len() as u32).to_be_bytes());
        keyframe.extend_from_slice(&sps);
        let pps = [0x68, 0xce, 0x3c, 0x80];
        keyframe.extend_from_slice(&(pps.len() as u32).to_be_bytes());
        keyframe.extend_from_slice(&pps);
        let idr = [0x65, 0x88, 0x84, 0x00];
        keyframe.extend_from_slice(&(idr.len() as u32).to_be_bytes());
        keyframe.extend_from_slice(&idr);

        let blob = h264_parameter_set_blob(&keyframe).unwrap();
        let expected = {
            let mut out = Vec::new();
            out.extend_from_slice(&(sps.len() as u32).to_be_bytes());
            out.extend_from_slice(&sps);
            out.extend_from_slice(&(pps.len() as u32).to_be_bytes());
            out.extend_from_slice(&pps);
            out
        };
        assert_eq!(blob, expected);
    }

    #[test]
    fn h264_parameter_set_blob_missing_parameter_fails() {
        let mut keyframe = Vec::new();
        let sps = [0x67, 0x42, 0xc0, 0x0a];
        keyframe.extend_from_slice(&(sps.len() as u32).to_be_bytes());
        keyframe.extend_from_slice(&sps);
        assert!(matches!(
            h264_parameter_set_blob(&keyframe),
            Err(DecodeError::MissingParameterSets)
        ));
    }

    #[test]
    fn detect_codec_identifies_hevc_and_h264() {
        let mut hevc_au = Vec::new();
        let vps = [0x40, 0x01, 0x0c];
        hevc_au.extend_from_slice(&(vps.len() as u32).to_be_bytes());
        hevc_au.extend_from_slice(&vps);
        assert_eq!(detect_codec(&hevc_au).unwrap(), CodecKind::Hevc);

        let mut h264_au = Vec::new();
        let sps = [0x67, 0x42, 0xc0];
        h264_au.extend_from_slice(&(sps.len() as u32).to_be_bytes());
        h264_au.extend_from_slice(&sps);
        assert_eq!(detect_codec(&h264_au).unwrap(), CodecKind::H264);

        let mut idr_au = Vec::new();
        let idr = [0x65, 0x88];
        idr_au.extend_from_slice(&(idr.len() as u32).to_be_bytes());
        idr_au.extend_from_slice(&idr);
        assert!(matches!(
            detect_codec(&idr_au),
            Err(DecodeError::MissingParameterSets)
        ));
    }
}

#[cfg(all(test, feature = "ffmpeg"))]
#[path = "../../test-support/allocations.rs"]
mod test_alloc;
